//! Memory injection into agent context.
//!
//! This module formats retrieved memories and injects them
//! into the agent's initial context for each turn.

use std::collections::HashMap;
use std::sync::Arc;

use tracing::debug;

use super::retriever::MemoryRetriever;
use super::retriever::RetrievalContext;
use super::types::MemoryConfig;
use super::types::MemoryType;
use super::types::ScoredMemory;

/// Injects relevant memories into the agent context.
pub struct MemoryInjector {
    retriever: Arc<MemoryRetriever>,
    config: MemoryConfig,
}

impl MemoryInjector {
    /// Create a new memory injector.
    pub fn new(retriever: Arc<MemoryRetriever>, config: MemoryConfig) -> Self {
        Self { retriever, config }
    }

    /// Build memory context for the current turn.
    /// Returns formatted memory text to inject into the prompt, or None if no relevant memories.
    pub async fn build_memory_context(
        &self,
        user_message: &str,
        active_files: &[std::path::PathBuf],
        recent_commands: &[String],
    ) -> Option<String> {
        if !self.config.enabled {
            return None;
        }

        let context = RetrievalContext {
            query: user_message.to_string(),
            active_files: active_files.to_vec(),
            recent_commands: recent_commands.to_vec(),
            type_filter: None,
        };

        let memories = self.retriever.retrieve(&context).await;
        if memories.is_empty() {
            return None;
        }

        let formatted = self.format_memories(&memories);
        if formatted.is_empty() {
            return None;
        }

        // Truncate if too long
        let truncated = self.truncate_to_token_limit(&formatted);

        debug!(
            "Injecting {} memories ({} chars) into context",
            memories.len(),
            truncated.len()
        );

        Some(truncated)
    }

    /// Format memories into a structured prompt section.
    fn format_memories(&self, memories: &[ScoredMemory]) -> String {
        // Group by type
        let mut by_type: HashMap<MemoryType, Vec<&ScoredMemory>> = HashMap::new();
        for sm in memories {
            by_type.entry(sm.memory.memory_type).or_default().push(sm);
        }

        let mut sections = Vec::new();

        // Order: Lessons first (most important), then Decisions, Preferences, Patterns, Locations, Facts
        let type_order = [
            (MemoryType::Lesson, "Lessons Learned"),
            (MemoryType::Decision, "Decisions Made"),
            (MemoryType::Preference, "User Preferences"),
            (MemoryType::Pattern, "Codebase Patterns"),
            (MemoryType::Location, "Code Locations"),
            (MemoryType::Fact, "Project Facts"),
        ];

        for (mem_type, header) in type_order {
            if let Some(mems) = by_type.get(&mem_type) {
                if mems.is_empty() {
                    continue;
                }

                let mut section = format!("### {}\n", header);
                for sm in mems.iter().take(5) {
                    // Format based on type
                    let bullet = match mem_type {
                        MemoryType::Lesson => format!(
                            "- **[LESSON]** {}{}",
                            sm.memory.content,
                            if let Some(ref ctx) = sm.memory.context {
                                format!(" _({})", truncate_context(ctx))
                            } else {
                                String::new()
                            }
                        ),
                        MemoryType::Decision => format!("- **[DECISION]** {}", sm.memory.content),
                        MemoryType::Preference => format!("- {}", sm.memory.content),
                        _ => format!("- {}", sm.memory.content),
                    };
                    section.push_str(&bullet);
                    section.push('\n');
                }
                sections.push(section);
            }
        }

        if sections.is_empty() {
            return String::new();
        }

        format!(
            "<project_memory>\n## Project Memory\n\n{}</project_memory>",
            sections.join("\n")
        )
    }

    /// Truncate to approximate token limit (rough estimate: 4 chars per token).
    fn truncate_to_token_limit(&self, text: &str) -> String {
        let max_chars = self.config.max_injection_tokens * 4;
        if text.len() <= max_chars {
            text.to_string()
        } else {
            // Find a good break point
            let truncated = &text[..max_chars];
            if let Some(last_newline) = truncated.rfind('\n') {
                format!("{}\n... (truncated)", &truncated[..last_newline])
            } else {
                format!("{}... (truncated)", truncated)
            }
        }
    }

    /// Get a summary of current memories for status display.
    pub async fn get_summary(&self) -> MemorySummary {
        let context = RetrievalContext::default();
        let memories = self.retriever.retrieve(&context).await;

        let mut by_type: HashMap<MemoryType, usize> = HashMap::new();
        for sm in &memories {
            *by_type.entry(sm.memory.memory_type).or_insert(0) += 1;
        }

        // Get recent memories (up to 10)
        let recent: Vec<_> = memories.into_iter().take(10).map(|sm| sm.memory).collect();

        MemorySummary {
            total: recent.len(), // This is the retrieved count
            lessons: by_type.get(&MemoryType::Lesson).copied().unwrap_or(0),
            decisions: by_type.get(&MemoryType::Decision).copied().unwrap_or(0),
            patterns: by_type.get(&MemoryType::Pattern).copied().unwrap_or(0),
            facts: by_type.get(&MemoryType::Fact).copied().unwrap_or(0),
            locations: by_type.get(&MemoryType::Location).copied().unwrap_or(0),
            preferences: by_type.get(&MemoryType::Preference).copied().unwrap_or(0),
            recent,
        }
    }
}

/// Truncate context to a reasonable length for display.
fn truncate_context(ctx: &str) -> &str {
    let max_len = 60;
    if ctx.len() <= max_len {
        ctx
    } else {
        &ctx[..max_len]
    }
}

/// Summary of memory counts by type.
#[derive(Debug, Clone, Default)]
pub struct MemorySummary {
    pub total: usize,
    pub lessons: usize,
    pub decisions: usize,
    pub patterns: usize,
    pub facts: usize,
    pub locations: usize,
    pub preferences: usize,
    /// Recent memories for display.
    pub recent: Vec<crate::memory::types::Memory>,
}

impl std::fmt::Display for MemorySummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} memories (L:{} D:{} P:{} F:{})",
            self.total, self.lessons, self.decisions, self.patterns, self.facts
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::store::MemoryStore;
    use crate::memory::types::Memory;
    use tempfile::TempDir;

    async fn create_test_injector() -> (MemoryInjector, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let config = MemoryConfig::default();
        let store = Arc::new(
            MemoryStore::init(temp_dir.path(), config.clone())
                .await
                .unwrap(),
        );
        let retriever = Arc::new(MemoryRetriever::new(store, config.clone()));
        let injector = MemoryInjector::new(retriever, config);
        (injector, temp_dir)
    }

    #[tokio::test]
    async fn test_format_memories() {
        let (injector, _dir) = create_test_injector().await;

        let memories = vec![
            ScoredMemory {
                memory: Memory::new(MemoryType::Lesson, "always mock Redis".to_string()),
                semantic_score: 0.9,
                combined_score: 0.9,
            },
            ScoredMemory {
                memory: Memory::new(MemoryType::Pattern, "tests in __tests__".to_string()),
                semantic_score: 0.8,
                combined_score: 0.8,
            },
        ];

        let formatted = injector.format_memories(&memories);
        assert!(formatted.contains("<project_memory>"));
        assert!(formatted.contains("[LESSON]"));
        assert!(formatted.contains("always mock Redis"));
        assert!(formatted.contains("tests in __tests__"));
    }

    #[tokio::test]
    async fn test_truncate_to_token_limit() {
        let (injector, _dir) = create_test_injector().await;

        let short_text = "short text";
        assert_eq!(injector.truncate_to_token_limit(short_text), short_text);

        let long_text = "a".repeat(50000);
        let truncated = injector.truncate_to_token_limit(&long_text);
        assert!(truncated.len() < long_text.len());
        assert!(truncated.contains("truncated"));
    }
}
