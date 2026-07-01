//! Document Embedding Storage and Vector Similarity Search
//!
//! Stores document embeddings in SurrealDB and implements cosine similarity
//! search to find related documents. Falls back to an in-memory cache when
//! SurrealDB is unavailable, providing graceful degradation.

use std::sync::Arc;
use std::cmp::Ordering;
use tokio::sync::Mutex;
use serde::{Serialize, Deserialize};
use chrono::{DateTime, Utc};
use tracing::{info, warn, debug};

use surrealdb::Surreal;
use surrealdb::engine::local::Db;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for the embedding store.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingConfig {
    /// Dimensionality of embedding vectors (384 is common for small models).
    pub embedding_dim: usize,
    /// Minimum cosine similarity for a result to be considered "related".
    pub similarity_threshold: f32,
    /// Maximum number of results returned by a search.
    pub max_results: usize,
    /// Whether embedding storage/search is enabled.
    pub enabled: bool,
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            embedding_dim: 384,
            similarity_threshold: 0.7,
            max_results: 10,
            enabled: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Data structures
// ---------------------------------------------------------------------------

/// A single document embedding with associated metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Embedding {
    /// Unique identifier (typically derived from the document ID).
    pub id: String,
    /// The document this embedding belongs to.
    pub document_id: String,
    /// The embedding vector.
    pub vector: Vec<f32>,
    /// Arbitrary metadata (document name, type, etc.).
    pub metadata: serde_json::Value,
    /// When the embedding was created.
    pub created_at: DateTime<Utc>,
}

/// A search result pairing an embedding with its cosine similarity score.
#[derive(Debug, Clone)]
pub struct SearchResult {
    /// The matching embedding.
    pub embedding: Embedding,
    /// Cosine similarity score in [-1.0, 1.0].
    pub score: f32,
}

impl PartialEq for SearchResult {
    fn eq(&self, other: &Self) -> bool {
        self.score == other.score
    }
}

impl Eq for SearchResult {}

impl PartialOrd for SearchResult {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SearchResult {
    fn cmp(&self, other: &Self) -> Ordering {
        // Higher scores first. NaN treated as less than everything.
        other
            .score
            .partial_cmp(&self.score)
            .unwrap_or(Ordering::Equal)
    }
}

// ---------------------------------------------------------------------------
// Cosine similarity
// ---------------------------------------------------------------------------

/// Compute cosine similarity between two vectors.
///
/// Returns 0.0 when the vectors differ in length, are empty, or either has
/// zero magnitude.
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b = b.iter().map(|y| y * y).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a * norm_b)
}

// ---------------------------------------------------------------------------
// Embedding generation helpers
// ---------------------------------------------------------------------------

/// Generate a deterministic hash-based embedding from text.
///
/// Uses n-gram hashing to produce a fixed-size vector. This is a lightweight
/// fallback for environments without a real embedding model — it captures
/// crude lexical overlap but nothing semantic.
pub fn generate_hash_embedding(text: &str, dim: usize) -> Vec<f32> {
    if dim == 0 {
        return Vec::new();
    }

    let mut vector = vec![0.0f32; dim];
    let lower = text.to_lowercase();
    let chars: Vec<char> = lower.chars().collect();

    // Use character n-grams of sizes 2..=4 to populate the vector.
    for ngram_size in 2..=4usize {
        if chars.len() < ngram_size {
            continue;
        }
        for window in chars.windows(ngram_size) {
            // Simple FNV-1a style hash of the n-gram.
            let mut hash: u64 = 0xcbf29ce484222325;
            for &ch in window {
                hash ^= ch as u64;
                hash = hash.wrapping_mul(0x100000001b3);
            }
            let idx = (hash as usize) % dim;
            // Map to [-1, 1] range deterministically.
            let sign_bit = (hash >> 63) & 1;
            let magnitude = ((hash >> 32) & 0xFFFF) as f32 / 65535.0;
            let value = if sign_bit == 1 { -magnitude } else { magnitude };
            vector[idx] += value;
        }
    }

    // L2-normalise so cosine similarity behaves predictably.
    let norm: f32 = vector.iter().map(|v| v * v).sum::<f32>().sqrt();
    if norm > 0.0 {
        for v in &mut vector {
            *v /= norm;
        }
    }

    vector
}

/// Generate a zero vector of the given dimensionality (fallback placeholder).
pub fn generate_zero_embedding(dim: usize) -> Vec<f32> {
    vec![0.0f32; dim]
}

// ---------------------------------------------------------------------------
// EmbeddingStore
// ---------------------------------------------------------------------------

/// Persistent store for document embeddings with brute-force similarity
/// search.
///
/// When SurrealDB is available the store persists to an `embedding` table;
/// otherwise it falls back to an in-memory cache so that all APIs remain
/// functional (useful for tests and offline scenarios).
pub struct EmbeddingStore {
    /// Store configuration.
    pub config: EmbeddingConfig,
    /// Shared SurrealDB client (may be `None`).
    db: Arc<Mutex<Option<Surreal<Db>>>>,
    /// In-memory cache used as the primary search index and as a fallback
    /// when SurrealDB is unavailable.
    local_cache: Vec<Embedding>,
}

impl std::fmt::Debug for EmbeddingStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EmbeddingStore")
            .field("config", &self.config)
            .field("local_cache_len", &self.local_cache.len())
            .finish()
    }
}

impl EmbeddingStore {
    /// Create a new embedding store with the given configuration and database
    /// handle.
    pub fn new(
        config: EmbeddingConfig,
        db: Arc<Mutex<Option<Surreal<Db>>>>,
    ) -> Self {
        info!(
            "Initializing EmbeddingStore (dim={}, threshold={}, max_results={})",
            config.embedding_dim, config.similarity_threshold, config.max_results
        );
        Self {
            config,
            db,
            local_cache: Vec::new(),
        }
    }

    /// Create an embedding store with default configuration.
    pub fn default(db: Arc<Mutex<Option<Surreal<Db>>>>) -> Self {
        Self::new(EmbeddingConfig::default(), db)
    }

    // ------------------------------------------------------------------
    // Write operations
    // ------------------------------------------------------------------

    /// Store a single embedding.
    ///
    /// Persists to SurrealDB when available; otherwise appends to the local
    /// cache.
    pub async fn store(&mut self, embedding: Embedding) -> Result<(), anyhow::Error> {
        if !self.config.enabled {
            return Err(anyhow::anyhow!("EmbeddingStore is disabled"));
        }

        if embedding.vector.len() != self.config.embedding_dim {
            warn!(
                "Embedding dim mismatch: expected {}, got {}. Storing anyway.",
                self.config.embedding_dim,
                embedding.vector.len()
            );
        }

        // Try SurrealDB first.
        let stored_in_db = {
            let db_guard = self.db.lock().await;
            if let Some(ref client) = *db_guard {
                let vector_json = serde_json::to_string(&embedding.vector)
                    .unwrap_or_default();
                let metadata_json = embedding.metadata.to_string();
                match client
                    .query(
                        "CREATE embedding SET \
                         id = $id, \
                         document_id = $document_id, \
                         vector = $vector, \
                         metadata = $metadata, \
                         created_at = $created_at",
                    )
                    .bind(("id", embedding.id.clone()))
                    .bind(("document_id", embedding.document_id.clone()))
                    .bind(("vector", vector_json))
                    .bind(("metadata", metadata_json))
                    .bind(("created_at", embedding.created_at.to_rfc3339()))
                    .await
                {
                    Ok(_) => {
                        debug!("Stored embedding {} in SurrealDB", embedding.id);
                        true
                    }
                    Err(e) => {
                        warn!(
                            "Failed to store embedding {} in SurrealDB: {}",
                            embedding.id, e
                        );
                        false
                    }
                }
            } else {
                false
            }
        };

        // Always keep local cache in sync (serves as search index).
        // Replace if an entry with the same id already exists.
        if let Some(pos) = self.local_cache.iter().position(|e| e.id == embedding.id) {
            self.local_cache[pos] = embedding;
        } else {
            self.local_cache.push(embedding);
        }

        if !stored_in_db {
            debug!("Embedding stored in local cache only (SurrealDB unavailable)");
        }

        Ok(())
    }

    /// Store multiple embeddings in a batch.
    pub async fn store_batch(
        &mut self,
        embeddings: Vec<Embedding>,
    ) -> Result<(), anyhow::Error> {
        let count = embeddings.len();
        info!("Batch storing {} embeddings", count);
        for embedding in embeddings {
            self.store(embedding).await?;
        }
        debug!("Batch store complete ({} embeddings)", count);
        Ok(())
    }

    // ------------------------------------------------------------------
    // Read / search operations
    // ------------------------------------------------------------------

    /// Search for embeddings most similar to `query` using cosine similarity.
    ///
    /// Loads all embeddings (from local cache, which is kept in sync with
    /// SurrealDB on every write), computes similarity, and returns the top
    /// `limit` results that meet the configured similarity threshold.
    pub async fn search(
        &self,
        query: &[f32],
        limit: usize,
    ) -> Result<Vec<SearchResult>, anyhow::Error> {
        if !self.config.enabled {
            return Err(anyhow::anyhow!("EmbeddingStore is disabled"));
        }

        if query.is_empty() {
            return Ok(Vec::new());
        }

        let candidates = self.load_all_embeddings().await?;

        let mut results: Vec<SearchResult> = candidates
            .into_iter()
            .map(|emb| {
                let score = cosine_similarity(query, &emb.vector);
                SearchResult {
                    embedding: emb,
                    score,
                }
            })
            .filter(|r| r.score >= self.config.similarity_threshold)
            .collect();

        // Sort descending by score.
        results.sort();

        results.truncate(limit);

        debug!(
            "Search returned {} results (threshold={}, limit={})",
            results.len(),
            self.config.similarity_threshold,
            limit
        );

        Ok(results)
    }

    /// Find documents similar to the one identified by `doc_id`.
    ///
    /// Looks up the embedding for `doc_id`, then delegates to `search`.
    pub async fn search_by_document_id(
        &self,
        doc_id: &str,
        limit: usize,
    ) -> Result<Vec<SearchResult>, anyhow::Error> {
        let all = self.load_all_embeddings().await?;

        let source = all
            .iter()
            .find(|e| e.document_id == doc_id)
            .ok_or_else(|| {
                anyhow::anyhow!("No embedding found for document_id: {}", doc_id)
            })?;

        let query_vector = source.vector.clone();
        self.search(&query_vector, limit).await
    }

    // ------------------------------------------------------------------
    // Delete / count
    // ------------------------------------------------------------------

    /// Delete the embedding associated with `document_id`.
    pub async fn delete(&mut self, document_id: &str) -> Result<(), anyhow::Error> {
        // Remove from SurrealDB.
        {
            let db_guard = self.db.lock().await;
            if let Some(ref client) = *db_guard {
                if let Err(e) = client
                    .query("DELETE embedding WHERE document_id = $document_id")
                    .bind(("document_id", document_id.to_string()))
                    .await
                {
                    warn!(
                        "Failed to delete embedding for {} from SurrealDB: {}",
                        document_id, e
                    );
                } else {
                    debug!("Deleted embedding for {} from SurrealDB", document_id);
                }
            }
        }

        // Remove from local cache.
        let before = self.local_cache.len();
        self.local_cache.retain(|e| e.document_id != document_id);
        let removed = before - self.local_cache.len();

        debug!(
            "Removed {} local cache entries for document_id={}",
            removed, document_id
        );

        Ok(())
    }

    /// Return the total number of stored embeddings.
    pub async fn count(&self) -> Result<usize, anyhow::Error> {
        // Prefer SurrealDB count if available.
        {
            let db_guard = self.db.lock().await;
            if let Some(ref client) = *db_guard {
                match client.query("SELECT count() FROM embedding GROUP ALL").await {
                    Ok(mut response) => {
                        if let Ok(Some(val)) = response.take::<Option<serde_json::Value>>(0) {
                            if let Some(n) = val.get("count").and_then(|v| v.as_u64()) {
                                return Ok(n as usize);
                            }
                        }
                    }
                    Err(e) => {
                        debug!("SurrealDB count query failed, falling back to cache: {}", e);
                    }
                }
            }
        }

        Ok(self.local_cache.len())
    }

    // ------------------------------------------------------------------
    // Internal helpers
    // ------------------------------------------------------------------

    /// Load all embeddings from the local cache.
    ///
    /// In a future iteration this could refresh the cache from SurrealDB on
    /// demand; for now the cache is kept in sync via `store` / `delete`.
    async fn load_all_embeddings(&self) -> Result<Vec<Embedding>, anyhow::Error> {
        // The local cache is always kept up-to-date with writes, so we can
        // serve reads directly from it without hitting SurrealDB.
        Ok(self.local_cache.clone())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: create an EmbeddingStore backed by `None` (local cache only).
    fn test_store(config: EmbeddingConfig) -> EmbeddingStore {
        let db: Arc<Mutex<Option<Surreal<Db>>>> = Arc::new(Mutex::new(None));
        EmbeddingStore::new(config, db)
    }

    /// Helper: build an `Embedding` quickly with a given id and vector.
    fn make_embedding(id: &str, doc_id: &str, vector: Vec<f32>) -> Embedding {
        Embedding {
            id: id.to_string(),
            document_id: doc_id.to_string(),
            vector,
            metadata: serde_json::json!({}),
            created_at: Utc::now(),
        }
    }

    // -- cosine_similarity --------------------------------------------------

    #[test]
    fn test_cosine_similarity_identical() {
        let v = vec![1.0, 2.0, 3.0];
        let score = cosine_similarity(&v, &v);
        assert!((score - 1.0).abs() < 1e-6, "identical vectors should give ~1.0, got {}", score);
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        let score = cosine_similarity(&a, &b);
        assert!(score.abs() < 1e-6, "orthogonal vectors should give ~0.0, got {}", score);
    }

    #[test]
    fn test_cosine_similarity_opposite() {
        let a = vec![1.0, 2.0, 3.0];
        let b: Vec<f32> = a.iter().map(|v| -v).collect();
        let score = cosine_similarity(&a, &b);
        assert!((score - (-1.0)).abs() < 1e-6, "opposite vectors should give ~-1.0, got {}", score);
    }

    #[test]
    fn test_cosine_similarity_different_lengths() {
        let a = vec![1.0, 2.0];
        let b = vec![1.0, 2.0, 3.0];
        let score = cosine_similarity(&a, &b);
        assert_eq!(score, 0.0, "different-length vectors should return 0.0");
    }

    #[test]
    fn test_cosine_similarity_empty() {
        let a: Vec<f32> = vec![];
        let b: Vec<f32> = vec![];
        let score = cosine_similarity(&a, &b);
        assert_eq!(score, 0.0, "empty vectors should return 0.0");
    }

    #[test]
    fn test_cosine_similarity_zero_vector() {
        let a = vec![0.0, 0.0, 0.0];
        let b = vec![1.0, 2.0, 3.0];
        let score = cosine_similarity(&a, &b);
        assert_eq!(score, 0.0, "zero vector should return 0.0");
    }

    // -- EmbeddingConfig ----------------------------------------------------

    #[test]
    fn test_embedding_config_defaults() {
        let config = EmbeddingConfig::default();
        assert_eq!(config.embedding_dim, 384);
        assert!((config.similarity_threshold - 0.7).abs() < f32::EPSILON);
        assert_eq!(config.max_results, 10);
        assert!(config.enabled);
    }

    // -- EmbeddingStore creation --------------------------------------------

    #[test]
    fn test_store_creation_with_none_db() {
        let store = test_store(EmbeddingConfig::default());
        assert_eq!(store.config.embedding_dim, 384);
        assert!(store.local_cache.is_empty());
    }

    // -- store & search (local cache) ---------------------------------------

    #[tokio::test]
    async fn test_store_and_search_local_cache() {
        let mut config = EmbeddingConfig::default();
        config.embedding_dim = 3;
        config.similarity_threshold = 0.0; // accept everything
        let mut store = test_store(config);

        let emb = make_embedding("e1", "doc1", vec![1.0, 0.0, 0.0]);
        store.store(emb).await.unwrap();

        let results = store.search(&[1.0, 0.0, 0.0], 10).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].embedding.id, "e1");
        assert!((results[0].score - 1.0).abs() < 1e-6);
    }

    #[tokio::test]
    async fn test_search_sorted_by_score() {
        let mut config = EmbeddingConfig::default();
        config.embedding_dim = 3;
        config.similarity_threshold = -2.0; // accept everything including negative
        let mut store = test_store(config);

        // Three vectors at varying similarity to [1,0,0].
        store
            .store(make_embedding("e_opp", "doc_opp", vec![-1.0, 0.0, 0.0]))
            .await
            .unwrap();
        store
            .store(make_embedding("e_same", "doc_same", vec![1.0, 0.0, 0.0]))
            .await
            .unwrap();
        store
            .store(make_embedding("e_ortho", "doc_ortho", vec![0.0, 1.0, 0.0]))
            .await
            .unwrap();

        let results = store.search(&[1.0, 0.0, 0.0], 10).await.unwrap();
        assert!(results.len() >= 3);

        // Scores should be monotonically non-increasing.
        for window in results.windows(2) {
            assert!(
                window[0].score >= window[1].score,
                "results not sorted: {} < {}",
                window[0].score,
                window[1].score
            );
        }

        // Best match should be the identical vector.
        assert_eq!(results[0].embedding.id, "e_same");
    }

    #[tokio::test]
    async fn test_search_respects_limit() {
        let mut config = EmbeddingConfig::default();
        config.embedding_dim = 3;
        config.similarity_threshold = -2.0;
        let mut store = test_store(config);

        for i in 0..5 {
            store
                .store(make_embedding(
                    &format!("e{}", i),
                    &format!("doc{}", i),
                    vec![1.0, i as f32, 0.0],
                ))
                .await
                .unwrap();
        }

        let results = store.search(&[1.0, 0.0, 0.0], 2).await.unwrap();
        assert_eq!(results.len(), 2, "limit should cap results at 2");
    }

    #[tokio::test]
    async fn test_search_respects_similarity_threshold() {
        let mut config = EmbeddingConfig::default();
        config.embedding_dim = 3;
        config.similarity_threshold = 0.99; // very strict
        let mut store = test_store(config);

        store
            .store(make_embedding("e1", "doc1", vec![1.0, 0.0, 0.0]))
            .await
            .unwrap();
        store
            .store(make_embedding("e2", "doc2", vec![0.0, 1.0, 0.0]))
            .await
            .unwrap();

        // Query identical to e1 — only e1 should pass the 0.99 threshold.
        let results = store.search(&[1.0, 0.0, 0.0], 10).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].embedding.id, "e1");
    }

    // -- generate_hash_embedding --------------------------------------------

    #[test]
    fn test_hash_embedding_consistency() {
        let a = generate_hash_embedding("hello world", 64);
        let b = generate_hash_embedding("hello world", 64);
        assert_eq!(a.len(), 64);
        assert_eq!(a, b, "same text must produce the same embedding");
    }

    #[test]
    fn test_hash_embedding_different_text() {
        let a = generate_hash_embedding("invoice 12345", 64);
        let b = generate_hash_embedding("receipt 67890", 64);
        assert_eq!(a.len(), 64);
        assert_eq!(b.len(), 64);
        assert_ne!(a, b, "different text should produce different embeddings");
    }

    #[test]
    fn test_hash_embedding_zero_dim() {
        let v = generate_hash_embedding("anything", 0);
        assert!(v.is_empty());
    }

    #[test]
    fn test_hash_embedding_is_normalised() {
        let v = generate_hash_embedding("some accounting text", 128);
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!(
            (norm - 1.0).abs() < 1e-4 || norm == 0.0,
            "hash embedding should be L2-normalised (norm={})",
            norm
        );
    }

    // -- generate_zero_embedding --------------------------------------------

    #[test]
    fn test_zero_embedding() {
        let v = generate_zero_embedding(16);
        assert_eq!(v.len(), 16);
        assert!(v.iter().all(|&x| x == 0.0));
    }

    // -- store_batch --------------------------------------------------------

    #[tokio::test]
    async fn test_store_batch() {
        let mut config = EmbeddingConfig::default();
        config.embedding_dim = 3;
        let mut store = test_store(config);

        let batch = vec![
            make_embedding("b1", "doc1", vec![1.0, 0.0, 0.0]),
            make_embedding("b2", "doc2", vec![0.0, 1.0, 0.0]),
            make_embedding("b3", "doc3", vec![0.0, 0.0, 1.0]),
        ];
        store.store_batch(batch).await.unwrap();

        let count = store.count().await.unwrap();
        assert_eq!(count, 3);
    }

    // -- delete -------------------------------------------------------------

    #[tokio::test]
    async fn test_delete() {
        let mut config = EmbeddingConfig::default();
        config.embedding_dim = 3;
        config.similarity_threshold = -2.0;
        let mut store = test_store(config);

        store
            .store(make_embedding("d1", "doc_to_delete", vec![1.0, 0.0, 0.0]))
            .await
            .unwrap();
        store
            .store(make_embedding("d2", "doc_to_keep", vec![0.0, 1.0, 0.0]))
            .await
            .unwrap();

        assert_eq!(store.count().await.unwrap(), 2);

        store.delete("doc_to_delete").await.unwrap();

        assert_eq!(store.count().await.unwrap(), 1);
        let results = store.search(&[0.0, 1.0, 0.0], 10).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].embedding.document_id, "doc_to_keep");
    }

    // -- count --------------------------------------------------------------

    #[tokio::test]
    async fn test_count_empty() {
        let store = test_store(EmbeddingConfig::default());
        assert_eq!(store.count().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn test_count_after_stores() {
        let mut config = EmbeddingConfig::default();
        config.embedding_dim = 2;
        let mut store = test_store(config);

        store
            .store(make_embedding("c1", "d1", vec![1.0, 0.0]))
            .await
            .unwrap();
        store
            .store(make_embedding("c2", "d2", vec![0.0, 1.0]))
            .await
            .unwrap();

        assert_eq!(store.count().await.unwrap(), 2);
    }

    // -- search_by_document_id ----------------------------------------------

    #[tokio::test]
    async fn test_search_by_document_id() {
        let mut config = EmbeddingConfig::default();
        config.embedding_dim = 3;
        config.similarity_threshold = 0.0;
        let mut store = test_store(config);

        store
            .store(make_embedding("e1", "doc_a", vec![1.0, 0.0, 0.0]))
            .await
            .unwrap();
        store
            .store(make_embedding("e2", "doc_b", vec![0.9, 0.1, 0.0]))
            .await
            .unwrap();
        store
            .store(make_embedding("e3", "doc_c", vec![0.0, 0.0, 1.0]))
            .await
            .unwrap();

        let results = store.search_by_document_id("doc_a", 10).await.unwrap();
        assert!(!results.is_empty());
        // The source document itself should be the top result.
        assert_eq!(results[0].embedding.document_id, "doc_a");
    }

    #[tokio::test]
    async fn test_search_by_document_id_not_found() {
        let store = test_store(EmbeddingConfig::default());
        let result = store.search_by_document_id("nonexistent", 10).await;
        assert!(result.is_err());
    }

    // -- disabled store -----------------------------------------------------

    #[tokio::test]
    async fn test_disabled_store_rejects_writes() {
        let mut config = EmbeddingConfig::default();
        config.enabled = false;
        let mut store = test_store(config);

        let emb = make_embedding("x", "doc_x", vec![1.0]);
        let result = store.store(emb).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_disabled_store_rejects_search() {
        let mut config = EmbeddingConfig::default();
        config.enabled = false;
        let store = test_store(config);

        let result = store.search(&[1.0], 10).await;
        assert!(result.is_err());
    }

    // -- store upsert (same id replaces) ------------------------------------

    #[tokio::test]
    async fn test_store_upsert_same_id() {
        let mut config = EmbeddingConfig::default();
        config.embedding_dim = 3;
        let mut store = test_store(config);

        store
            .store(make_embedding("same_id", "doc_v1", vec![1.0, 0.0, 0.0]))
            .await
            .unwrap();
        store
            .store(make_embedding("same_id", "doc_v2", vec![0.0, 1.0, 0.0]))
            .await
            .unwrap();

        assert_eq!(store.count().await.unwrap(), 1);
        // The cached entry should reflect the second write.
        assert_eq!(store.local_cache[0].document_id, "doc_v2");
    }
}
