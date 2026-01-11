//! Memory retrieval using semantic search via embeddings.
//!
//! This module retrieves relevant memories for the current context
//! using a combination of embedding-based semantic search and importance scoring.

use std::path::PathBuf;
use std::sync::Arc;

use tracing::debug;

use super::store::MemoryStore;
use super::types::Memory;
use super::types::MemoryConfig;
use super::types::MemoryType;
use super::types::ScoredMemory;

// PathBuf is still used in RetrievalContext

/// Retrieves relevant memories using semantic search.
pub struct MemoryRetriever {
    store: Arc<MemoryStore>,
    config: MemoryConfig,
}

/// Context for memory retrieval.
#[derive(Debug, Clone, Default)]
pub struct RetrievalContext {
    /// Current user query/message.
    pub query: String,
    /// Files currently being worked on.
    pub active_files: Vec<PathBuf>,
    /// Recent commands executed.
    pub recent_commands: Vec<String>,
    /// Filter to specific memory types.
    pub type_filter: Option<Vec<MemoryType>>,
}

impl MemoryRetriever {
    /// Create a new memory retriever.
    pub fn new(store: Arc<MemoryStore>, config: MemoryConfig) -> Self {
        Self { store, config }
    }

    /// Retrieve memories relevant to the given context.
    pub async fn retrieve(&self, context: &RetrievalContext) -> Vec<ScoredMemory> {
        let limit = self.config.max_retrieval_count;

        // Try semantic search first using embeddings
        let mut scored = if !context.query.is_empty() {
            self.semantic_search(&context.query, limit * 2).await
        } else {
            Vec::new()
        };

        // Supplement with keyword search if we don't have enough results
        if scored.len() < limit {
            let keywords = self.extract_keywords(&context.query);
            if !keywords.is_empty() {
                let keyword_results = self.keyword_search(&keywords).await;
                for memory in keyword_results {
                    if !scored.iter().any(|s| s.memory.id == memory.id) {
                        scored.push(ScoredMemory {
                            memory,
                            semantic_score: 0.0,
                            combined_score: 0.0,
                        });
                    }
                }
            }
        }

        // Always include high-importance memories (lessons, decisions)
        let important = self.get_always_include().await;
        for memory in important {
            if !scored.iter().any(|s| s.memory.id == memory.id) {
                scored.push(ScoredMemory {
                    memory,
                    semantic_score: 0.0,
                    combined_score: 0.0,
                });
            }
        }

        // Score all memories
        for sm in &mut scored {
            sm.combined_score = self.compute_combined_score(sm, context);
        }

        // Sort by combined score
        scored.sort_by(|a, b| {
            b.combined_score
                .partial_cmp(&a.combined_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Apply type filter if specified
        if let Some(ref types) = context.type_filter {
            scored.retain(|sm| types.contains(&sm.memory.memory_type));
        }

        // Take top results with diversity
        let result = self.diverse_top_k(scored, limit);

        // Mark retrieved memories as used
        for sm in &result {
            let _ = self.store.mark_used(&sm.memory.id).await;
        }

        debug!(
            "Retrieved {} memories for query: {}",
            result.len(),
            truncate(&context.query, 30)
        );
        result
    }

    /// Perform semantic search using embeddings.
    async fn semantic_search(&self, query: &str, limit: usize) -> Vec<ScoredMemory> {
        match self.store.search_by_similarity(query, limit).await {
            Ok(results) => results
                .into_iter()
                .map(|(memory, similarity)| ScoredMemory {
                    memory,
                    semantic_score: similarity as f64,
                    combined_score: 0.0,
                })
                .collect(),
            Err(e) => {
                debug!("Semantic search failed: {} - falling back to keywords", e);
                Vec::new()
            }
        }
    }

    /// Keyword-based search fallback.
    async fn keyword_search(&self, keywords: &[String]) -> Vec<Memory> {
        let keyword_refs: Vec<&str> = keywords.iter().map(|s| s.as_str()).collect();
        self.store
            .search_by_keywords(&keyword_refs)
            .await
            .unwrap_or_default()
    }

    /// Get memories that should always be included (high importance lessons/decisions).
    async fn get_always_include(&self) -> Vec<Memory> {
        let mut result = Vec::new();

        // Always include recent lessons and decisions
        for mem_type in [MemoryType::Lesson, MemoryType::Decision] {
            if let Ok(memories) = self.store.get_by_type(mem_type).await {
                for memory in memories.into_iter().take(3) {
                    if memory.importance >= 0.7 {
                        result.push(memory);
                    }
                }
            }
        }

        result
    }

    /// Compute combined score for a memory.
    fn compute_combined_score(&self, sm: &ScoredMemory, context: &RetrievalContext) -> f64 {
        let memory = &sm.memory;

        // Base score from semantic search
        let semantic = sm.semantic_score;

        // Importance score
        let importance = memory.effective_importance();

        // Recency boost
        let now = chrono::Utc::now().timestamp();
        let days_since_used = ((now - memory.last_used) as f64) / 86400.0;
        let recency = (-days_since_used / 30.0).exp();

        // Frequency boost
        let frequency = 1.0 + 0.05 * memory.use_count.min(20) as f64;

        // Type boost for lessons and decisions
        let type_boost = match memory.memory_type {
            MemoryType::Lesson => 1.5,
            MemoryType::Decision => 1.3,
            MemoryType::Preference => 1.2,
            _ => 1.0,
        };

        // File relevance boost
        let file_boost = if let Some(ref source) = memory.source_file {
            if context
                .active_files
                .iter()
                .any(|f| f == source || f.starts_with(source.parent().unwrap_or(source)))
            {
                1.3
            } else {
                1.0
            }
        } else {
            1.0
        };

        // Combine scores with weights
        let combined =
            semantic * 0.35 + importance * 0.25 + recency * 0.15 + (frequency - 1.0) * 0.1 + 0.15; // base score

        combined * type_boost * file_boost
    }

    /// Select top-K results with diversity across types.
    fn diverse_top_k(&self, mut scored: Vec<ScoredMemory>, k: usize) -> Vec<ScoredMemory> {
        if scored.len() <= k {
            return scored;
        }

        let mut result = Vec::with_capacity(k);
        let mut type_counts: std::collections::HashMap<MemoryType, usize> =
            std::collections::HashMap::new();
        let max_per_type = (k / 3).max(2);

        // First pass: take top memories respecting type limits
        scored.sort_by(|a, b| {
            b.combined_score
                .partial_cmp(&a.combined_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        for sm in scored {
            let count = type_counts.entry(sm.memory.memory_type).or_insert(0);
            if *count < max_per_type || result.len() < k / 2 {
                *count += 1;
                result.push(sm);
                if result.len() >= k {
                    break;
                }
            }
        }

        result
    }

    /// Extract keywords from a query.
    fn extract_keywords(&self, query: &str) -> Vec<String> {
        let stop_words = [
            "the", "a", "an", "is", "are", "was", "were", "be", "been", "being", "have", "has",
            "had", "do", "does", "did", "will", "would", "could", "should", "may", "might", "must",
            "shall", "can", "need", "dare", "to", "of", "in", "for", "on", "with", "at", "by",
            "from", "as", "into", "through", "during", "before", "after", "above", "below",
            "between", "under", "again", "further", "then", "once", "here", "there", "when",
            "where", "why", "how", "all", "each", "few", "more", "most", "other", "some", "such",
            "no", "nor", "not", "only", "own", "same", "so", "than", "too", "very", "just", "and",
            "but", "or", "if", "because", "until", "while", "this", "that", "these", "those",
            "what", "which", "who", "whom", "it", "its", "i", "me", "my", "we", "our", "you",
            "your", "he", "his", "she", "her", "they",
        ];

        query
            .split_whitespace()
            .map(|w| w.to_lowercase())
            .filter(|w| w.len() > 2 && !stop_words.contains(&w.as_str()))
            .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_string())
            .filter(|w| !w.is_empty())
            .collect()
    }
}

/// Truncate a string for logging.
fn truncate(s: &str, max_len: usize) -> &str {
    if s.len() <= max_len { s } else { &s[..max_len] }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::types::MemoryConfig;
    use tempfile::TempDir;

    async fn create_test_retriever() -> (MemoryRetriever, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let store = MemoryStore::init(temp_dir.path(), MemoryConfig::default())
            .await
            .unwrap();
        let retriever = MemoryRetriever::new(Arc::new(store), MemoryConfig::default());
        (retriever, temp_dir)
    }

    #[tokio::test]
    async fn test_extract_keywords() {
        let (retriever, _dir) = create_test_retriever().await;

        let keywords = retriever.extract_keywords("where is the authentication code");
        assert!(keywords.contains(&"authentication".to_string()));
        assert!(keywords.contains(&"code".to_string()));
        assert!(!keywords.contains(&"the".to_string())); // stop word
        assert!(!keywords.contains(&"is".to_string())); // stop word
    }

    #[tokio::test]
    async fn test_diverse_top_k() {
        let (retriever, _dir) = create_test_retriever().await;

        let scored: Vec<ScoredMemory> = (0..10)
            .map(|i| ScoredMemory {
                memory: Memory::new(
                    if i % 2 == 0 {
                        MemoryType::Fact
                    } else {
                        MemoryType::Pattern
                    },
                    format!("memory {}", i),
                ),
                semantic_score: 1.0 - (i as f64 * 0.1),
                combined_score: 1.0 - (i as f64 * 0.1),
            })
            .collect();

        let result = retriever.diverse_top_k(scored, 5);
        // Algorithm enforces type diversity (max 2 per type when k=5),
        // resulting in 4 results (2 Facts + 2 Patterns)
        assert!(result.len() >= 4 && result.len() <= 5);

        // Should have mix of types
        let facts = result
            .iter()
            .filter(|s| s.memory.memory_type == MemoryType::Fact)
            .count();
        let patterns = result
            .iter()
            .filter(|s| s.memory.memory_type == MemoryType::Pattern)
            .count();
        assert!(facts > 0);
        assert!(patterns > 0);
    }
}
