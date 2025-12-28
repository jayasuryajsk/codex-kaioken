//! Memory type definitions for the Kaioken memory system.
//!
//! This module defines the core data structures for persistent memory,
//! enabling the agent to learn and remember across sessions.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// The type of memory being stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryType {
    /// Codebase knowledge (e.g., "project uses React 18")
    Fact,
    /// Recurring code patterns (e.g., "tests in __tests__ folders")
    Pattern,
    /// Why choices were made (e.g., "chose Axum over Actix")
    Decision,
    /// Learned from mistakes (e.g., "must mock Redis in tests")
    Lesson,
    /// User's style preferences (e.g., "prefers explicit errors")
    Preference,
    /// Where things are in the codebase (e.g., "auth code in src/auth/")
    Location,
}

impl MemoryType {
    /// Returns whether this memory type should decay over time.
    /// Lessons and Decisions never decay - they're too important.
    pub fn decays(&self) -> bool {
        match self {
            MemoryType::Lesson | MemoryType::Decision => false,
            _ => true,
        }
    }

    /// Returns the default importance for this memory type.
    pub fn default_importance(&self) -> f64 {
        match self {
            MemoryType::Lesson => 0.9,
            MemoryType::Decision => 0.85,
            MemoryType::Preference => 0.8,
            MemoryType::Pattern => 0.7,
            MemoryType::Location => 0.6,
            MemoryType::Fact => 0.5,
        }
    }

    /// Returns the type as a string for storage.
    pub fn as_str(&self) -> &'static str {
        match self {
            MemoryType::Fact => "fact",
            MemoryType::Pattern => "pattern",
            MemoryType::Decision => "decision",
            MemoryType::Lesson => "lesson",
            MemoryType::Preference => "preference",
            MemoryType::Location => "location",
        }
    }

    /// Parse a memory type from a string.
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "fact" => Some(MemoryType::Fact),
            "pattern" => Some(MemoryType::Pattern),
            "decision" => Some(MemoryType::Decision),
            "lesson" => Some(MemoryType::Lesson),
            "preference" => Some(MemoryType::Preference),
            "location" => Some(MemoryType::Location),
            _ => None,
        }
    }
}

/// A single memory record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Memory {
    /// Unique identifier for this memory.
    pub id: String,
    /// The type of memory.
    pub memory_type: MemoryType,
    /// The actual memory content.
    pub content: String,
    /// Optional context about where/why this was learned.
    pub context: Option<String>,
    /// The source file that triggered this memory, if any.
    pub source_file: Option<PathBuf>,
    /// Importance score from 0.0 to 1.0.
    pub importance: f64,
    /// Number of times this memory has been accessed/reinforced.
    pub use_count: u32,
    /// Unix timestamp when this memory was created.
    pub created_at: i64,
    /// Unix timestamp when this memory was last used.
    pub last_used: i64,
    /// Reference to the sgrep embedding, if indexed.
    pub embedding_id: Option<String>,
}

impl Memory {
    /// Create a new memory with default values.
    pub fn new(memory_type: MemoryType, content: String) -> Self {
        let now = chrono::Utc::now().timestamp();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            memory_type,
            content,
            context: None,
            source_file: None,
            importance: memory_type.default_importance(),
            use_count: 0,
            created_at: now,
            last_used: now,
            embedding_id: None,
        }
    }

    /// Set the context for this memory.
    pub fn with_context(mut self, context: impl Into<String>) -> Self {
        self.context = Some(context.into());
        self
    }

    /// Set the source file for this memory.
    pub fn with_source_file(mut self, path: PathBuf) -> Self {
        self.source_file = Some(path);
        self
    }

    /// Set the importance for this memory.
    pub fn with_importance(mut self, importance: f64) -> Self {
        self.importance = importance.clamp(0.0, 1.0);
        self
    }

    /// Calculate the effective importance considering recency and frequency.
    pub fn effective_importance(&self) -> f64 {
        let now = chrono::Utc::now().timestamp();
        let days_since_used = ((now - self.last_used) as f64) / 86400.0;

        // Recency factor: half-life of 30 days
        let recency_factor = (-days_since_used / 30.0).exp();

        // Frequency factor: logarithmic boost, capped at 10 uses
        let frequency_factor = 1.0 + 0.1 * (self.use_count.min(10) as f64);

        // Non-decaying types get a boost
        let type_factor = if self.memory_type.decays() { 1.0 } else { 1.5 };

        self.importance * recency_factor * frequency_factor * type_factor
    }
}

/// The source/trigger of a memory extraction.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MemorySource {
    /// Memory extracted from reading a file.
    FileRead { path: PathBuf },
    /// Memory extracted from editing a file.
    FileEdit { path: PathBuf, diff_summary: Option<String> },
    /// Memory extracted from a successful command.
    CommandSuccess { command: String, output_summary: Option<String> },
    /// Memory extracted from a failed command.
    CommandFailure { command: String, error: String },
    /// Memory extracted from a failed command that was later fixed.
    CommandFixed {
        original_command: String,
        error: String,
        fix_command: String,
        fix_description: Option<String>,
    },
    /// Memory extracted from user correction.
    UserCorrection { original: String, correction: String },
    /// Memory explicitly stored by user via /remember command.
    UserExplicit { input: String },
    /// Memory detected from repeated patterns.
    PatternDetected { pattern_type: String, occurrences: u32 },
}

impl MemorySource {
    /// Get a short description of the source for display.
    pub fn description(&self) -> String {
        match self {
            MemorySource::FileRead { path } => {
                format!("from reading {}", path.display())
            }
            MemorySource::FileEdit { path, .. } => {
                format!("from editing {}", path.display())
            }
            MemorySource::CommandSuccess { command, .. } => {
                format!("from running `{}`", truncate_command(command))
            }
            MemorySource::CommandFailure { command, .. } => {
                format!("from failed `{}`", truncate_command(command))
            }
            MemorySource::CommandFixed { original_command, .. } => {
                format!("from fixing `{}`", truncate_command(original_command))
            }
            MemorySource::UserCorrection { .. } => "from user correction".to_string(),
            MemorySource::UserExplicit { .. } => "explicitly remembered".to_string(),
            MemorySource::PatternDetected { pattern_type, occurrences } => {
                format!("from detecting {} ({} times)", pattern_type, occurrences)
            }
        }
    }
}

/// A memory with its retrieval score.
#[derive(Debug, Clone)]
pub struct ScoredMemory {
    /// The memory itself.
    pub memory: Memory,
    /// Semantic similarity score (0.0 to 1.0).
    pub semantic_score: f64,
    /// Combined score considering all factors.
    pub combined_score: f64,
}

/// Configuration for the memory system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    /// Whether the memory system is enabled.
    pub enabled: bool,
    /// Maximum number of memories to store per type.
    pub max_memories_per_type: usize,
    /// Maximum tokens to use for memory injection.
    pub max_injection_tokens: usize,
    /// Decay rate per session (e.g., 0.95 = 5% decay).
    pub decay_rate: f64,
    /// Minimum importance threshold before pruning.
    pub min_importance_threshold: f64,
    /// Maximum memories to retrieve per turn.
    pub max_retrieval_count: usize,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_memories_per_type: 100,
            max_injection_tokens: 2000,
            decay_rate: 0.95,
            min_importance_threshold: 0.1,
            max_retrieval_count: 15,
        }
    }
}

/// Truncate a command for display purposes.
fn truncate_command(cmd: &str) -> &str {
    let max_len = 50;
    if cmd.len() <= max_len {
        cmd
    } else {
        &cmd[..max_len]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_type_decay() {
        assert!(!MemoryType::Lesson.decays());
        assert!(!MemoryType::Decision.decays());
        assert!(MemoryType::Fact.decays());
        assert!(MemoryType::Pattern.decays());
        assert!(MemoryType::Preference.decays());
        assert!(MemoryType::Location.decays());
    }

    #[test]
    fn test_memory_creation() {
        let memory = Memory::new(MemoryType::Lesson, "always mock redis".to_string())
            .with_context("learned from test failure")
            .with_importance(0.95);

        assert_eq!(memory.memory_type, MemoryType::Lesson);
        assert_eq!(memory.content, "always mock redis");
        assert_eq!(memory.context.as_deref(), Some("learned from test failure"));
        assert!((memory.importance - 0.95).abs() < 0.001);
    }

    #[test]
    fn test_effective_importance() {
        let memory = Memory::new(MemoryType::Fact, "test".to_string());
        // Freshly created memory should have effective importance close to base
        let effective = memory.effective_importance();
        assert!(effective > 0.0);
        assert!(effective <= 1.5); // Can be boosted by type factor
    }

    #[test]
    fn test_memory_type_roundtrip() {
        for mem_type in [
            MemoryType::Fact,
            MemoryType::Pattern,
            MemoryType::Decision,
            MemoryType::Lesson,
            MemoryType::Preference,
            MemoryType::Location,
        ] {
            let s = mem_type.as_str();
            let parsed = MemoryType::from_str(s).unwrap();
            assert_eq!(mem_type, parsed);
        }
    }
}
