//! Embedding service for semantic memory operations.
//!
//! Uses fastembed for local, fast embeddings without external API calls.

use std::sync::Arc;

use anyhow::Result;
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use tokio::sync::OnceCell;
use tracing::{debug, info, warn};

/// Embedding vector type (f32 for compatibility with most models).
pub type EmbeddingVector = Vec<f32>;

/// Dimension of the embedding vectors (BGE-small uses 384 dimensions).
pub const EMBEDDING_DIM: usize = 384;

/// Service for generating and comparing text embeddings.
pub struct EmbeddingService {
    model: Arc<TextEmbedding>,
}

/// Global singleton for the embedding model (expensive to initialize).
static EMBEDDING_MODEL: OnceCell<Arc<TextEmbedding>> = OnceCell::const_new();

impl EmbeddingService {
    /// Create a new embedding service, lazily initializing the model.
    pub async fn new() -> Result<Self> {
        let model = EMBEDDING_MODEL
            .get_or_try_init(|| async {
                info!("Initializing embedding model (BGE-small-en-v1.5)...");

                let options = InitOptions::new(EmbeddingModel::BGESmallENV15)
                    .with_show_download_progress(false);

                match TextEmbedding::try_new(options) {
                    Ok(model) => {
                        info!("Embedding model initialized successfully");
                        Ok(Arc::new(model))
                    }
                    Err(e) => {
                        warn!("Failed to initialize embedding model: {}", e);
                        Err(anyhow::anyhow!("Failed to initialize embedding model: {}", e))
                    }
                }
            })
            .await?;

        Ok(Self {
            model: Arc::clone(model),
        })
    }

    /// Generate an embedding for a single text.
    pub fn embed(&self, text: &str) -> Result<EmbeddingVector> {
        let embeddings = self.model.embed(vec![text], None)?;
        embeddings
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("No embedding generated"))
    }

    /// Generate embeddings for multiple texts (batch processing).
    pub fn embed_batch(&self, texts: &[&str]) -> Result<Vec<EmbeddingVector>> {
        let text_vec: Vec<String> = texts.iter().map(|s| s.to_string()).collect();
        let embeddings = self.model.embed(text_vec, None)?;
        Ok(embeddings)
    }

    /// Compute cosine similarity between two embeddings.
    /// Returns a value between -1 and 1 (1 = identical, 0 = orthogonal, -1 = opposite).
    pub fn cosine_similarity(a: &EmbeddingVector, b: &EmbeddingVector) -> f32 {
        if a.len() != b.len() {
            return 0.0;
        }

        let dot_product: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

        if norm_a == 0.0 || norm_b == 0.0 {
            return 0.0;
        }

        dot_product / (norm_a * norm_b)
    }

    /// Find the most similar embeddings from a list.
    /// Returns indices and similarity scores, sorted by similarity (descending).
    pub fn find_similar(
        &self,
        query: &EmbeddingVector,
        candidates: &[EmbeddingVector],
        top_k: usize,
    ) -> Vec<(usize, f32)> {
        let mut scored: Vec<(usize, f32)> = candidates
            .iter()
            .enumerate()
            .map(|(i, candidate)| (i, Self::cosine_similarity(query, candidate)))
            .collect();

        // Sort by similarity (descending)
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        scored.into_iter().take(top_k).collect()
    }

    /// Check if two texts are semantically similar (above threshold).
    pub fn are_similar(&self, text_a: &str, text_b: &str, threshold: f32) -> Result<bool> {
        let emb_a = self.embed(text_a)?;
        let emb_b = self.embed(text_b)?;
        let similarity = Self::cosine_similarity(&emb_a, &emb_b);
        debug!(
            "Similarity between '{}' and '{}': {:.3}",
            truncate(text_a, 30),
            truncate(text_b, 30),
            similarity
        );
        Ok(similarity >= threshold)
    }
}

/// Truncate text for logging.
fn truncate(s: &str, max_len: usize) -> &str {
    if s.len() <= max_len {
        s
    } else {
        &s[..max_len]
    }
}

/// Serialize an embedding vector to bytes for storage.
pub fn embedding_to_bytes(embedding: &EmbeddingVector) -> Vec<u8> {
    embedding
        .iter()
        .flat_map(|f| f.to_le_bytes())
        .collect()
}

/// Deserialize an embedding vector from bytes.
pub fn bytes_to_embedding(bytes: &[u8]) -> Option<EmbeddingVector> {
    if bytes.len() % 4 != 0 {
        return None;
    }

    Some(
        bytes
            .chunks(4)
            .map(|chunk| {
                let arr: [u8; 4] = chunk.try_into().ok()?;
                Some(f32::from_le_bytes(arr))
            })
            .collect::<Option<Vec<f32>>>()?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosine_similarity() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![1.0, 0.0, 0.0];
        assert!((EmbeddingService::cosine_similarity(&a, &b) - 1.0).abs() < 0.001);

        let c = vec![0.0, 1.0, 0.0];
        assert!(EmbeddingService::cosine_similarity(&a, &c).abs() < 0.001);

        let d = vec![-1.0, 0.0, 0.0];
        assert!((EmbeddingService::cosine_similarity(&a, &d) + 1.0).abs() < 0.001);
    }

    #[test]
    fn test_embedding_serialization() {
        let embedding = vec![1.0f32, 2.0, 3.0, -1.5];
        let bytes = embedding_to_bytes(&embedding);
        let recovered = bytes_to_embedding(&bytes).unwrap();
        assert_eq!(embedding, recovered);
    }

    #[tokio::test]
    async fn test_embedding_service() {
        let service = EmbeddingService::new().await;
        if let Ok(svc) = service {
            let emb = svc.embed("hello world").unwrap();
            assert_eq!(emb.len(), EMBEDDING_DIM);

            // Same text should have high similarity
            let emb2 = svc.embed("hello world").unwrap();
            let sim = EmbeddingService::cosine_similarity(&emb, &emb2);
            assert!(sim > 0.99);

            // Different text should have lower similarity
            let emb3 = svc.embed("quantum physics equations").unwrap();
            let sim2 = EmbeddingService::cosine_similarity(&emb, &emb3);
            assert!(sim2 < sim);
        }
    }
}
