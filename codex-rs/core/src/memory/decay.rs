//! Memory decay and reinforcement management.
//!
//! This module handles the decay of memory importance over time
//! and reinforcement when memories are used.

use std::sync::Arc;

use tracing::info;

use super::store::MemoryStore;
use super::types::MemoryConfig;

/// Manages memory decay and reinforcement.
pub struct DecayManager {
    store: Arc<MemoryStore>,
    config: MemoryConfig,
}

impl DecayManager {
    /// Create a new decay manager.
    pub fn new(store: Arc<MemoryStore>, config: MemoryConfig) -> Self {
        Self { store, config }
    }

    /// Apply decay to all decayable memories.
    /// Should be called periodically (e.g., once per session start).
    pub async fn apply_decay(&self) -> anyhow::Result<DecayResult> {
        let decayed = self.store.apply_decay().await?;
        let pruned = self.store.prune_low_importance().await?;

        let result = DecayResult { decayed, pruned };

        if result.decayed > 0 || result.pruned > 0 {
            info!(
                "Memory decay applied: {} memories decayed, {} pruned",
                result.decayed, result.pruned
            );
        }

        Ok(result)
    }

    /// Reinforce a memory when it's used.
    pub async fn reinforce(&self, memory_id: &str) -> anyhow::Result<()> {
        // Small boost for each use
        let boost = 0.02;
        self.store.reinforce(memory_id, boost).await
    }

    /// Check if decay should be applied (e.g., based on time since last decay).
    pub async fn needs_decay(&self) -> bool {
        // For now, always return true - decay is idempotent
        // Could add timestamp tracking to only decay once per day
        true
    }

    /// Get the current decay configuration.
    pub fn config(&self) -> &MemoryConfig {
        &self.config
    }
}

/// Result of a decay operation.
#[derive(Debug, Clone, Default)]
pub struct DecayResult {
    /// Number of memories that had their importance reduced.
    pub decayed: u32,
    /// Number of memories that were pruned (deleted).
    pub pruned: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::types::Memory;
    use crate::memory::types::MemoryType;
    use tempfile::TempDir;

    async fn create_test_decay_manager() -> (DecayManager, Arc<MemoryStore>, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let config = MemoryConfig::default();
        let store = Arc::new(
            MemoryStore::init(temp_dir.path(), config.clone())
                .await
                .unwrap(),
        );
        let manager = DecayManager::new(store.clone(), config);
        (manager, store, temp_dir)
    }

    #[tokio::test]
    async fn test_apply_decay() {
        let (manager, store, _dir) = create_test_decay_manager().await;

        // Insert some memories
        for i in 0..5 {
            let memory = Memory::new(MemoryType::Fact, format!("fact {}", i)).with_importance(0.5);
            store.insert(&memory).await.unwrap();
        }

        // Apply decay
        let result = manager.apply_decay().await.unwrap();
        assert!(result.decayed > 0);
    }

    #[tokio::test]
    async fn test_lessons_dont_decay() {
        let (manager, store, _dir) = create_test_decay_manager().await;

        // Insert a lesson with high importance
        let lesson =
            Memory::new(MemoryType::Lesson, "important lesson".to_string()).with_importance(0.9);
        let id = lesson.id.clone();
        store.insert(&lesson).await.unwrap();

        // Apply decay multiple times
        for _ in 0..10 {
            manager.apply_decay().await.unwrap();
        }

        // Lesson should still exist with same importance
        let retrieved = store.get(&id).await.unwrap().unwrap();
        assert!((retrieved.importance - 0.9).abs() < 0.01);
    }
}
