//! SQLite-based memory storage for persistent memories.
//!
//! This module provides durable storage for memories using SQLite,
//! with support for efficient querying by type, importance, and keywords.
//! Embeddings are stored as BLOBs for semantic similarity search.

use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use rusqlite::Connection;
use rusqlite::OptionalExtension;
use rusqlite::params;
use tokio::sync::Mutex;
use tracing::debug;
use tracing::info;
use tracing::warn;

use super::embedding::EmbeddingService;
use super::embedding::EmbeddingVector;
use super::embedding::bytes_to_embedding;
use super::embedding::embedding_to_bytes;
use super::types::Memory;
use super::types::MemoryConfig;
use super::types::MemoryType;

/// SQLite-based memory store with embedding support.
pub struct MemoryStore {
    /// Database connection (wrapped in mutex for async safety).
    conn: Arc<Mutex<Connection>>,
    /// Path to the database file.
    db_path: PathBuf,
    /// Path to the memory docs directory.
    docs_path: PathBuf,
    /// Configuration.
    config: MemoryConfig,
    /// Embedding service for semantic search.
    embedding_service: Option<Arc<EmbeddingService>>,
}

impl MemoryStore {
    /// Initialize the memory store at the given project path.
    /// Creates `.kaioken/memory/` directory structure if needed.
    pub async fn init(project_root: &Path, config: MemoryConfig) -> anyhow::Result<Self> {
        let memory_dir = project_root.join(".kaioken").join("memory");
        let docs_dir = memory_dir.join("docs");
        let db_path = memory_dir.join("memories.db");

        // Create directories
        tokio::fs::create_dir_all(&docs_dir).await?;

        // Open database connection
        let conn = Connection::open(&db_path)?;

        // Initialize schema
        Self::init_schema(&conn)?;

        // Initialize embedding service (lazy, may fail if model can't be loaded)
        let embedding_service = match EmbeddingService::new().await {
            Ok(svc) => {
                info!("Embedding service initialized for semantic memory search");
                Some(Arc::new(svc))
            }
            Err(e) => {
                warn!(
                    "Embedding service unavailable: {} - falling back to keyword search",
                    e
                );
                None
            }
        };

        info!("Memory store initialized at {}", db_path.display());

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            db_path,
            docs_path: docs_dir,
            config,
            embedding_service,
        })
    }

    /// Initialize the database schema.
    fn init_schema(conn: &Connection) -> anyhow::Result<()> {
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS memories (
                id TEXT PRIMARY KEY,
                type TEXT NOT NULL,
                content TEXT NOT NULL,
                context TEXT,
                source_file TEXT,
                importance REAL DEFAULT 0.5,
                use_count INTEGER DEFAULT 0,
                created_at INTEGER NOT NULL,
                last_used INTEGER NOT NULL,
                embedding_id TEXT,
                embedding BLOB
            );

            CREATE INDEX IF NOT EXISTS idx_memories_type ON memories(type);
            CREATE INDEX IF NOT EXISTS idx_memories_importance ON memories(importance);
            CREATE INDEX IF NOT EXISTS idx_memories_last_used ON memories(last_used);

            CREATE TABLE IF NOT EXISTS relationships (
                from_id TEXT NOT NULL,
                to_id TEXT NOT NULL,
                relation TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                PRIMARY KEY (from_id, to_id, relation),
                FOREIGN KEY (from_id) REFERENCES memories(id) ON DELETE CASCADE,
                FOREIGN KEY (to_id) REFERENCES memories(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS metadata (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            "#,
        )?;

        // Migrate: add embedding column if missing (for existing databases)
        let _ = conn.execute("ALTER TABLE memories ADD COLUMN embedding BLOB", []);

        // Store schema version
        conn.execute(
            "INSERT OR REPLACE INTO metadata (key, value) VALUES ('schema_version', '2')",
            [],
        )?;

        Ok(())
    }

    /// Insert a new memory into the store.
    pub async fn insert(&self, memory: &Memory) -> anyhow::Result<()> {
        // Generate embedding if service is available
        let embedding_bytes: Option<Vec<u8>> = if let Some(ref svc) = self.embedding_service {
            match svc.embed(&memory.content) {
                Ok(emb) => Some(embedding_to_bytes(&emb)),
                Err(e) => {
                    warn!("Failed to generate embedding for memory: {}", e);
                    None
                }
            }
        } else {
            None
        };

        let conn = self.conn.lock().await;

        conn.execute(
            r#"
            INSERT INTO memories (
                id, type, content, context, source_file,
                importance, use_count, created_at, last_used, embedding_id, embedding
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
            "#,
            params![
                memory.id,
                memory.memory_type.as_str(),
                memory.content,
                memory.context,
                memory
                    .source_file
                    .as_ref()
                    .map(|p| p.to_string_lossy().to_string()),
                memory.importance,
                memory.use_count,
                memory.created_at,
                memory.last_used,
                memory.embedding_id,
                embedding_bytes,
            ],
        )?;

        debug!(
            "Inserted memory: {} ({})",
            memory.id,
            memory.memory_type.as_str()
        );

        // Write to docs directory (kept for compatibility)
        self.write_memory_doc(memory).await?;

        Ok(())
    }

    /// Write memory content to a markdown file for sgrep indexing.
    async fn write_memory_doc(&self, memory: &Memory) -> anyhow::Result<()> {
        let doc_path = self.docs_path.join(format!("{}.md", memory.id));
        let content = format!(
            "# {}\n\nType: {}\n\n{}\n\n{}",
            memory.memory_type.as_str(),
            memory.memory_type.as_str(),
            memory.content,
            memory.context.as_deref().unwrap_or("")
        );
        tokio::fs::write(&doc_path, content).await?;
        Ok(())
    }

    /// Get a memory by ID.
    pub async fn get(&self, id: &str) -> anyhow::Result<Option<Memory>> {
        let conn = self.conn.lock().await;

        let result = conn
            .query_row("SELECT * FROM memories WHERE id = ?1", params![id], |row| {
                Self::row_to_memory(row)
            })
            .optional()?;

        Ok(result)
    }

    /// Get all memories of a specific type.
    pub async fn get_by_type(&self, memory_type: MemoryType) -> anyhow::Result<Vec<Memory>> {
        let conn = self.conn.lock().await;

        let mut stmt = conn.prepare(
            "SELECT * FROM memories WHERE type = ?1 ORDER BY importance DESC, last_used DESC",
        )?;

        let memories = stmt
            .query_map(params![memory_type.as_str()], |row| {
                Self::row_to_memory(row)
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(memories)
    }

    /// Get top memories by effective importance.
    pub async fn get_top_memories(&self, limit: usize) -> anyhow::Result<Vec<Memory>> {
        let conn = self.conn.lock().await;

        // Get all memories and sort by effective importance in Rust
        // (SQLite can't compute the complex effective_importance formula)
        let mut stmt = conn.prepare("SELECT * FROM memories")?;

        let mut memories: Vec<Memory> = stmt
            .query_map([], |row| Self::row_to_memory(row))?
            .filter_map(|r| r.ok())
            .collect();

        // Sort by effective importance
        memories.sort_by(|a, b| {
            b.effective_importance()
                .partial_cmp(&a.effective_importance())
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Take top N
        memories.truncate(limit);

        Ok(memories)
    }

    /// Search memories by keyword match in content.
    pub async fn search_by_keywords(&self, keywords: &[&str]) -> anyhow::Result<Vec<Memory>> {
        if keywords.is_empty() {
            return Ok(vec![]);
        }

        let conn = self.conn.lock().await;

        // Build LIKE query for each keyword
        let conditions: Vec<String> = keywords
            .iter()
            .map(|_| "content LIKE ?".to_string())
            .collect();
        let query = format!(
            "SELECT * FROM memories WHERE {} ORDER BY importance DESC",
            conditions.join(" OR ")
        );

        let params: Vec<String> = keywords.iter().map(|k| format!("%{}%", k)).collect();

        let mut stmt = conn.prepare(&query)?;

        let memories = stmt
            .query_map(rusqlite::params_from_iter(params.iter()), |row| {
                Self::row_to_memory(row)
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(memories)
    }

    /// Search memories by semantic similarity to a query.
    /// Returns memories with similarity scores, sorted by similarity.
    pub async fn search_by_similarity(
        &self,
        query: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<(Memory, f32)>> {
        let Some(ref svc) = self.embedding_service else {
            // Fall back to keyword search if no embedding service
            debug!("No embedding service, falling back to keyword search");
            let keywords: Vec<&str> = query.split_whitespace().take(5).collect();
            let memories = self.search_by_keywords(&keywords).await?;
            return Ok(memories.into_iter().map(|m| (m, 0.5)).collect());
        };

        // Generate query embedding
        let query_embedding = svc.embed(query)?;

        // Get all memories with embeddings (collect while holding lock, then release)
        let memories_with_embeddings: Vec<(Memory, EmbeddingVector)> = {
            let conn = self.conn.lock().await;
            let mut stmt = conn.prepare("SELECT * FROM memories WHERE embedding IS NOT NULL")?;

            stmt.query_map([], |row| {
                let memory = Self::row_to_memory(row)?;
                let embedding_bytes: Option<Vec<u8>> = row.get("embedding")?;
                Ok((memory, embedding_bytes))
            })?
            .filter_map(|r| r.ok())
            .filter_map(|(m, bytes)| {
                bytes
                    .and_then(|b| bytes_to_embedding(&b))
                    .map(|emb| (m, emb))
            })
            .collect()
        };

        // Compute similarities and sort (lock already released)
        let mut scored: Vec<(Memory, f32)> = memories_with_embeddings
            .into_iter()
            .map(|(memory, emb)| {
                let similarity = EmbeddingService::cosine_similarity(&query_embedding, &emb);
                (memory, similarity)
            })
            .collect();

        // Sort by similarity descending
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Take top N
        scored.truncate(limit);

        Ok(scored)
    }

    /// Check if a memory is semantically similar to existing memories.
    /// Returns true if any existing memory has similarity above the threshold.
    pub async fn exists_semantically_similar(
        &self,
        content: &str,
        memory_type: MemoryType,
        threshold: f32,
    ) -> anyhow::Result<bool> {
        let Some(ref svc) = self.embedding_service else {
            // Fall back to exact match
            return self.exists_similar(content, memory_type).await;
        };

        let query_embedding = svc.embed(content)?;

        // Get embeddings while holding lock, then release
        let embeddings: Vec<EmbeddingVector> = {
            let conn = self.conn.lock().await;
            let mut stmt = conn.prepare(
                "SELECT embedding FROM memories WHERE type = ?1 AND embedding IS NOT NULL",
            )?;

            stmt.query_map(params![memory_type.as_str()], |row| {
                let bytes: Option<Vec<u8>> = row.get(0)?;
                Ok(bytes)
            })?
            .filter_map(|r| r.ok())
            .filter_map(|bytes| bytes.and_then(|b| bytes_to_embedding(&b)))
            .collect()
        };

        // Check if any existing embedding is similar enough (lock released)
        for emb in embeddings {
            let similarity = EmbeddingService::cosine_similarity(&query_embedding, &emb);
            if similarity >= threshold {
                return Ok(true);
            }
        }

        Ok(false)
    }

    /// Get the embedding service (if available).
    pub fn embedding_service(&self) -> Option<&Arc<EmbeddingService>> {
        self.embedding_service.as_ref()
    }

    /// Update a memory's importance and use count (reinforcement).
    pub async fn reinforce(&self, id: &str, importance_boost: f64) -> anyhow::Result<()> {
        let conn = self.conn.lock().await;
        let now = chrono::Utc::now().timestamp();

        conn.execute(
            r#"
            UPDATE memories
            SET use_count = use_count + 1,
                last_used = ?1,
                importance = MIN(1.0, importance + ?2)
            WHERE id = ?3
            "#,
            params![now, importance_boost, id],
        )?;

        debug!("Reinforced memory: {}", id);
        Ok(())
    }

    /// Mark a memory as used (updates last_used timestamp).
    pub async fn mark_used(&self, id: &str) -> anyhow::Result<()> {
        let conn = self.conn.lock().await;
        let now = chrono::Utc::now().timestamp();

        conn.execute(
            "UPDATE memories SET last_used = ?1, use_count = use_count + 1 WHERE id = ?2",
            params![now, id],
        )?;

        Ok(())
    }

    /// Apply decay to all decayable memories.
    pub async fn apply_decay(&self) -> anyhow::Result<u32> {
        let conn = self.conn.lock().await;
        let decay_rate = self.config.decay_rate;

        // Only decay types that should decay (not lessons/decisions)
        let decaying_types: Vec<&str> = [
            MemoryType::Fact,
            MemoryType::Pattern,
            MemoryType::Preference,
            MemoryType::Location,
        ]
        .iter()
        .map(|t| t.as_str())
        .collect();

        let placeholders: Vec<&str> = decaying_types.iter().map(|_| "?").collect();
        let query = format!(
            "UPDATE memories SET importance = importance * ?1 WHERE type IN ({})",
            placeholders.join(", ")
        );

        let mut all_params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(decay_rate)];
        for t in &decaying_types {
            all_params.push(Box::new(*t));
        }

        let updated = conn.execute(
            &query,
            rusqlite::params_from_iter(all_params.iter().map(|p| p.as_ref())),
        )?;

        info!("Applied decay to {} memories", updated);
        Ok(updated as u32)
    }

    /// Prune memories below the importance threshold.
    pub async fn prune_low_importance(&self) -> anyhow::Result<u32> {
        let conn = self.conn.lock().await;
        let threshold = self.config.min_importance_threshold;

        // Don't prune lessons or decisions regardless of importance
        let deleted = conn.execute(
            r#"
            DELETE FROM memories
            WHERE importance < ?1
            AND type NOT IN ('lesson', 'decision')
            "#,
            params![threshold],
        )?;

        if deleted > 0 {
            info!("Pruned {} low-importance memories", deleted);

            // Clean up orphaned doc files
            self.cleanup_orphaned_docs().await?;
        }

        Ok(deleted as u32)
    }

    /// Clean up doc files for deleted memories.
    async fn cleanup_orphaned_docs(&self) -> anyhow::Result<()> {
        // Get all valid memory IDs (complete DB work before async operations)
        let valid_ids: std::collections::HashSet<String> = {
            let conn = self.conn.lock().await;
            let mut stmt = conn.prepare("SELECT id FROM memories")?;
            let ids = stmt
                .query_map([], |row| row.get::<_, String>(0))?
                .filter_map(|r| r.ok())
                .collect();
            ids
        };

        // Read docs directory and delete orphaned files
        let mut entries = tokio::fs::read_dir(&self.docs_path).await?;
        while let Some(entry) = entries.next_entry().await? {
            if let Some(stem) = entry.path().file_stem() {
                let id = stem.to_string_lossy().to_string();
                if !valid_ids.contains(&id) {
                    tokio::fs::remove_file(entry.path()).await?;
                    debug!("Removed orphaned doc: {}", entry.path().display());
                }
            }
        }

        Ok(())
    }

    /// Check if a similar memory already exists (deduplication).
    pub async fn exists_similar(
        &self,
        content: &str,
        memory_type: MemoryType,
    ) -> anyhow::Result<bool> {
        let conn = self.conn.lock().await;

        // Simple similarity check: exact content match with same type
        let exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM memories WHERE content = ?1 AND type = ?2)",
            params![content, memory_type.as_str()],
            |row| row.get(0),
        )?;

        Ok(exists)
    }

    /// Get memory statistics.
    pub async fn stats(&self) -> anyhow::Result<MemoryStats> {
        let conn = self.conn.lock().await;

        let total_count: usize =
            conn.query_row("SELECT COUNT(*) FROM memories", [], |row| row.get(0))?;

        let counts_by_type: std::collections::HashMap<String, usize> = {
            let mut stmt = conn.prepare("SELECT type, COUNT(*) FROM memories GROUP BY type")?;
            stmt.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, usize>(1)?))
            })?
            .filter_map(|r| r.ok())
            .collect()
        };

        let avg_importance: f64 = conn
            .query_row("SELECT AVG(importance) FROM memories", [], |row| {
                row.get::<_, Option<f64>>(0)
            })?
            .unwrap_or(0.0);

        let storage_path = Some(self.db_path.display().to_string());

        Ok(MemoryStats {
            total_count,
            counts_by_type,
            avg_importance,
            storage_path,
        })
    }

    /// Delete a memory by ID.
    pub async fn delete(&self, id: &str) -> anyhow::Result<bool> {
        let conn = self.conn.lock().await;

        let deleted = conn.execute("DELETE FROM memories WHERE id = ?1", params![id])?;

        if deleted > 0 {
            // Remove doc file
            let doc_path = self.docs_path.join(format!("{}.md", id));
            let _ = tokio::fs::remove_file(&doc_path).await;
            debug!("Deleted memory: {}", id);
        }

        Ok(deleted > 0)
    }

    /// Add a relationship between two memories.
    pub async fn add_relationship(
        &self,
        from_id: &str,
        to_id: &str,
        relation: &str,
    ) -> anyhow::Result<()> {
        let conn = self.conn.lock().await;
        let now = chrono::Utc::now().timestamp();

        conn.execute(
            "INSERT OR IGNORE INTO relationships (from_id, to_id, relation, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![from_id, to_id, relation, now],
        )?;

        Ok(())
    }

    /// Get related memories.
    pub async fn get_related(&self, id: &str) -> anyhow::Result<Vec<(Memory, String)>> {
        let conn = self.conn.lock().await;

        let mut stmt = conn.prepare(
            r#"
            SELECT m.*, r.relation
            FROM memories m
            JOIN relationships r ON m.id = r.to_id
            WHERE r.from_id = ?1
            "#,
        )?;

        let results = stmt
            .query_map(params![id], |row| {
                let memory = Self::row_to_memory(row)?;
                let relation: String = row.get("relation")?;
                Ok((memory, relation))
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(results)
    }

    /// Convert a database row to a Memory struct.
    fn row_to_memory(row: &rusqlite::Row) -> rusqlite::Result<Memory> {
        let type_str: String = row.get("type")?;
        let source_file: Option<String> = row.get("source_file")?;

        Ok(Memory {
            id: row.get("id")?,
            memory_type: MemoryType::from_str(&type_str).unwrap_or(MemoryType::Fact),
            content: row.get("content")?,
            context: row.get("context")?,
            source_file: source_file.map(PathBuf::from),
            importance: row.get("importance")?,
            use_count: row.get("use_count")?,
            created_at: row.get("created_at")?,
            last_used: row.get("last_used")?,
            embedding_id: row.get("embedding_id")?,
        })
    }

    /// Get the path to the docs directory (for sgrep indexing).
    pub fn docs_path(&self) -> &Path {
        &self.docs_path
    }

    /// Get the database path.
    pub fn db_path(&self) -> &Path {
        &self.db_path
    }
}

/// Memory statistics.
#[derive(Debug, Clone, Default)]
pub struct MemoryStats {
    pub total_count: usize,
    pub counts_by_type: std::collections::HashMap<String, usize>,
    pub avg_importance: f64,
    pub storage_path: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    async fn create_test_store() -> (MemoryStore, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let store = MemoryStore::init(temp_dir.path(), MemoryConfig::default())
            .await
            .unwrap();
        (store, temp_dir)
    }

    #[tokio::test]
    async fn test_insert_and_get() {
        let (store, _dir) = create_test_store().await;

        let memory = Memory::new(MemoryType::Fact, "test content".to_string());
        let id = memory.id.clone();

        store.insert(&memory).await.unwrap();

        let retrieved = store.get(&id).await.unwrap().unwrap();
        assert_eq!(retrieved.content, "test content");
        assert_eq!(retrieved.memory_type, MemoryType::Fact);
    }

    #[tokio::test]
    async fn test_search_by_keywords() {
        let (store, _dir) = create_test_store().await;

        store
            .insert(&Memory::new(
                MemoryType::Fact,
                "uses React for frontend".to_string(),
            ))
            .await
            .unwrap();
        store
            .insert(&Memory::new(
                MemoryType::Fact,
                "uses Rust for backend".to_string(),
            ))
            .await
            .unwrap();

        let results = store.search_by_keywords(&["React"]).await.unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].content.contains("React"));
    }

    #[tokio::test]
    async fn test_reinforce() {
        let (store, _dir) = create_test_store().await;

        let memory = Memory::new(MemoryType::Pattern, "test pattern".to_string());
        let id = memory.id.clone();
        let original_importance = memory.importance;

        store.insert(&memory).await.unwrap();
        store.reinforce(&id, 0.1).await.unwrap();

        let updated = store.get(&id).await.unwrap().unwrap();
        assert!(updated.importance > original_importance);
        assert_eq!(updated.use_count, 1);
    }

    #[tokio::test]
    async fn test_exists_similar() {
        let (store, _dir) = create_test_store().await;

        let memory = Memory::new(MemoryType::Fact, "unique content".to_string());
        store.insert(&memory).await.unwrap();

        assert!(
            store
                .exists_similar("unique content", MemoryType::Fact)
                .await
                .unwrap()
        );
        assert!(
            !store
                .exists_similar("different content", MemoryType::Fact)
                .await
                .unwrap()
        );
    }
}
