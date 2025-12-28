//! Persistent memory system for Kaioken.
//!
//! This module provides a SOTA memory system that enables the agent to:
//! - Auto-learn from actions (file reads, edits, commands)
//! - Remember mistakes and how they were fixed
//! - Know where things are in the codebase
//! - Retrieve semantically relevant memories using embeddings
//! - Inject context into each turn automatically
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │                   MemoryManager                         │
//! │  (Facade coordinating all memory operations)            │
//! └─────────────────────────┬───────────────────────────────┘
//!                           │
//!     ┌─────────────────────┼─────────────────────┐
//!     │                     │                     │
//!     ▼                     ▼                     ▼
//! ┌─────────┐         ┌───────────┐         ┌──────────┐
//! │Extractor│         │ Retriever │         │ Injector │
//! │         │         │           │         │          │
//! │ LLM +   │         │ Semantic  │         │ Formats  │
//! │ rules   │         │ search    │         │ & injects│
//! └────┬────┘         └─────┬─────┘         └────┬─────┘
//!      │                    │                    │
//!      └────────────────────┼────────────────────┘
//!                           │
//!              ┌────────────┴────────────┐
//!              ▼                         ▼
//!       ┌─────────────┐          ┌──────────────┐
//!       │MemoryStore  │          │ Embedding    │
//!       │ (SQLite +   │◄────────►│ Service      │
//!       │  vectors)   │          │ (fastembed)  │
//!       └─────────────┘          └──────────────┘
//! ```
//!
//! # Usage
//!
//! ```ignore
//! // Initialize at session start
//! let memory = MemoryManager::init(project_root, config).await?;
//!
//! // Extract from actions (called by tool handlers)
//! memory.on_exec_complete(command, exit_code, stdout, stderr, cwd).await;
//! memory.on_file_read(path, content).await;
//!
//! // Get context for injection (called before each turn)
//! if let Some(context) = memory.build_context(user_message, active_files).await {
//!     // Inject into prompt
//! }
//! ```

pub mod decay;
pub mod embedding;
pub mod extractor;
pub mod injector;
pub mod llm_extractor;
pub mod retriever;
pub mod store;
pub mod types;

use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use tracing::{debug, info, warn};

use crate::AuthManager;
use crate::ModelProviderInfo;
use crate::config::Config;
use codex_otel::otel_event_manager::OtelEventManager;
use codex_protocol::ConversationId;
use codex_protocol::protocol::SessionSource;

pub use decay::DecayManager;
pub use decay::DecayResult;
pub use extractor::MemoryExtractor;
pub use injector::MemoryInjector;
pub use injector::MemorySummary;
pub use llm_extractor::LlmMemoryExtractor;
pub use retriever::MemoryRetriever;
pub use retriever::RetrievalContext;
pub use store::MemoryStore;
pub use store::MemoryStats;
pub use types::Memory;
pub use types::MemoryConfig;
pub use types::MemorySource;
pub use types::MemoryType;
pub use types::ScoredMemory;

/// Main facade for the memory system.
///
/// Coordinates all memory operations including extraction,
/// storage, retrieval, and injection.
pub struct MemoryManager {
    store: Arc<MemoryStore>,
    extractor: Arc<MemoryExtractor>,
    llm_extractor: Arc<LlmMemoryExtractor>,
    retriever: Arc<MemoryRetriever>,
    injector: Arc<MemoryInjector>,
    decay: Arc<DecayManager>,
    config: MemoryConfig,
    project_root: PathBuf,
}

impl MemoryManager {
    /// Initialize the memory system for a project.
    ///
    /// Creates the `.kaioken/memory/` directory structure and initializes
    /// all components.
    pub async fn init(project_root: &Path, config: MemoryConfig) -> anyhow::Result<Self> {
        if !config.enabled {
            info!("Memory system disabled by configuration");
        }

        let store = Arc::new(MemoryStore::init(project_root, config.clone()).await?);
        let extractor = Arc::new(MemoryExtractor::new(store.clone()));
        let llm_extractor = Arc::new(LlmMemoryExtractor::new(store.clone()));
        let retriever = Arc::new(MemoryRetriever::new(store.clone(), config.clone()));
        let injector = Arc::new(MemoryInjector::new(retriever.clone(), config.clone()));
        let decay = Arc::new(DecayManager::new(store.clone(), config.clone()));

        // Apply decay on startup
        if config.enabled {
            if let Err(e) = decay.apply_decay().await {
                warn!("Failed to apply memory decay on startup: {}", e);
            }
        }

        info!("Memory system initialized for {}", project_root.display());

        Ok(Self {
            store,
            extractor,
            llm_extractor,
            retriever,
            injector,
            decay,
            config,
            project_root: project_root.to_path_buf(),
        })
    }

    /// Check if the memory system is enabled.
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Extraction methods (called by tool handlers)
    // ─────────────────────────────────────────────────────────────────────────

    /// Called when a command execution completes.
    pub async fn on_exec_complete(
        &self,
        command: &str,
        exit_code: i32,
        stdout: &str,
        stderr: &str,
        cwd: &Path,
    ) {
        if !self.config.enabled {
            return;
        }

        let memories = self
            .extractor
            .on_exec_complete(command, exit_code, stdout, stderr, cwd)
            .await;

        if !memories.is_empty() {
            debug!("Extracted {} memories from command execution", memories.len());
        }
    }

    /// Called when a file is read.
    pub async fn on_file_read(&self, path: &Path, content: &str) {
        if !self.config.enabled {
            return;
        }

        let memories = self.extractor.on_file_read(path, content).await;

        if !memories.is_empty() {
            debug!("Extracted {} memories from file read", memories.len());
        }
    }

    /// Called when a file is edited.
    pub async fn on_file_edit(&self, path: &Path, diff: &str) {
        if !self.config.enabled {
            return;
        }

        let memories = self.extractor.on_file_edit(path, diff).await;

        if !memories.is_empty() {
            debug!("Extracted {} memories from file edit", memories.len());
        }
    }

    /// Called when the user explicitly requests to remember something.
    pub async fn remember(&self, input: &str) -> anyhow::Result<Memory> {
        self.extractor.on_user_remember(input).await
    }

    /// Called when the user corrects the agent.
    pub async fn on_user_correction(&self, original: &str, correction: &str) {
        if !self.config.enabled {
            return;
        }

        if let Some(memory) = self.extractor.on_user_correction(original, correction).await {
            debug!("Stored correction as memory: {}", memory.id);
        }
    }

    /// Called after a turn completes to extract memories using LLM analysis.
    ///
    /// This analyzes the turn's conversation and tool calls to extract valuable
    /// learnings, patterns, and insights.
    pub async fn on_turn_complete(
        &self,
        config: Arc<Config>,
        provider: ModelProviderInfo,
        auth_manager: Arc<AuthManager>,
        otel: &OtelEventManager,
        conversation_id: ConversationId,
        session_source: SessionSource,
        user_message: &str,
        agent_response: &str,
        tool_calls: &[(String, String, bool)], // (tool_name, summary, success)
        files_touched: &[String],
    ) {
        if !self.config.enabled {
            return;
        }

        // Build the turn summary
        let turn_summary = LlmMemoryExtractor::build_turn_summary(
            user_message,
            agent_response,
            tool_calls,
            files_touched,
        );

        // Extract memories asynchronously
        let memories = self
            .llm_extractor
            .extract_from_turn(
                config,
                provider,
                auth_manager,
                otel,
                conversation_id,
                session_source,
                &turn_summary,
            )
            .await;

        if !memories.is_empty() {
            info!(
                "LLM extracted {} memories from turn",
                memories.len()
            );
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Retrieval and injection methods
    // ─────────────────────────────────────────────────────────────────────────

    /// Build memory context for injection into the prompt.
    pub async fn build_context(
        &self,
        user_message: &str,
        active_files: &[PathBuf],
    ) -> Option<String> {
        if !self.config.enabled {
            return None;
        }

        self.injector
            .build_memory_context(user_message, active_files, &[])
            .await
    }

    /// Retrieve memories matching the given context.
    pub async fn retrieve(&self, context: &RetrievalContext) -> Vec<ScoredMemory> {
        if !self.config.enabled {
            return Vec::new();
        }

        self.retriever.retrieve(context).await
    }

    /// Search memories by query string (for tool use).
    pub async fn search(
        &self,
        query: &str,
        type_filter: Option<MemoryType>,
        limit: usize,
    ) -> Vec<ScoredMemory> {
        if !self.config.enabled {
            return Vec::new();
        }

        let context = RetrievalContext {
            query: query.to_string(),
            type_filter: type_filter.map(|t| vec![t]),
            ..Default::default()
        };

        let mut results = self.retriever.retrieve(&context).await;
        results.truncate(limit);
        results
    }

    /// Save a memory explicitly (for tool use).
    pub async fn save_explicit(
        &self,
        memory_type: MemoryType,
        content: &str,
        context_str: Option<&str>,
        source_file: Option<&Path>,
    ) -> anyhow::Result<Memory> {
        let mut memory = Memory::new(memory_type, content.to_string());
        memory.context = context_str.map(|s| s.to_string());
        memory.source_file = source_file.map(|p| p.to_path_buf());

        self.store.insert(&memory).await?;
        debug!("Saved explicit memory: {} - {}", memory.id, memory.content);
        Ok(memory)
    }

    /// Get a summary of current memories.
    pub async fn summary(&self) -> MemorySummary {
        self.injector.get_summary().await
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Management methods
    // ─────────────────────────────────────────────────────────────────────────

    /// Get memory statistics.
    pub async fn stats(&self) -> anyhow::Result<MemoryStats> {
        self.store.stats().await
    }

    /// Forget (delete) a memory by ID.
    pub async fn forget(&self, id: &str) -> anyhow::Result<bool> {
        self.store.delete(id).await
    }

    /// Forget memories matching a query.
    pub async fn forget_matching(&self, query: &str) -> anyhow::Result<usize> {
        let context = RetrievalContext {
            query: query.to_string(),
            ..Default::default()
        };

        let matches = self.retriever.retrieve(&context).await;
        let mut deleted = 0;

        for sm in matches {
            if self.store.delete(&sm.memory.id).await? {
                deleted += 1;
            }
        }

        Ok(deleted)
    }

    /// List all memories, optionally filtered by type.
    pub async fn list(&self, type_filter: Option<MemoryType>) -> anyhow::Result<Vec<Memory>> {
        if let Some(mem_type) = type_filter {
            self.store.get_by_type(mem_type).await
        } else {
            self.store.get_top_memories(100).await
        }
    }

    // Note: Reindexing is no longer needed - embeddings are generated on insert.

    /// Apply decay manually (normally done on startup).
    pub async fn apply_decay(&self) -> anyhow::Result<DecayResult> {
        self.decay.apply_decay().await
    }

    /// Get the project root this memory manager is for.
    pub fn project_root(&self) -> &Path {
        &self.project_root
    }

    /// Get the configuration.
    pub fn config(&self) -> &MemoryConfig {
        &self.config
    }

    /// Get access to the store for advanced operations.
    pub fn store(&self) -> &Arc<MemoryStore> {
        &self.store
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    async fn create_test_manager() -> (MemoryManager, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let manager = MemoryManager::init(temp_dir.path(), MemoryConfig::default())
            .await
            .unwrap();
        (manager, temp_dir)
    }

    #[tokio::test]
    async fn test_manager_init() {
        let (manager, _dir) = create_test_manager().await;
        assert!(manager.is_enabled());
    }

    #[tokio::test]
    async fn test_remember_and_retrieve() {
        let (manager, _dir) = create_test_manager().await;

        // Remember something
        let memory = manager.remember("always use explicit error handling").await.unwrap();
        assert_eq!(memory.memory_type, MemoryType::Preference);

        // Retrieve it
        let context = RetrievalContext {
            query: "error handling".to_string(),
            ..Default::default()
        };
        let results = manager.retrieve(&context).await;

        // Should find the memory
        assert!(results.iter().any(|sm| sm.memory.id == memory.id));
    }

    #[tokio::test]
    async fn test_forget() {
        let (manager, _dir) = create_test_manager().await;

        // Remember something
        let memory = manager.remember("temporary fact").await.unwrap();
        let id = memory.id.clone();

        // Forget it
        assert!(manager.forget(&id).await.unwrap());

        // Should not be retrievable
        let context = RetrievalContext {
            query: "temporary".to_string(),
            ..Default::default()
        };
        let results = manager.retrieve(&context).await;
        assert!(!results.iter().any(|sm| sm.memory.id == id));
    }

    #[tokio::test]
    async fn test_stats() {
        let (manager, _dir) = create_test_manager().await;

        // Add some memories
        manager.remember("fact one").await.unwrap();
        manager.remember("decision: use Rust").await.unwrap();

        // Check stats
        let stats = manager.stats().await.unwrap();
        assert!(stats.total_count >= 2);
    }
}
