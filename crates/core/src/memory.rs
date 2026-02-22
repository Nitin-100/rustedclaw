//! Memory trait — persistent knowledge storage with hybrid search.
//!
//! The memory system allows the agent to remember facts, context, and
//! conversations across sessions. It supports:
//! - Full-text search (keyword matching via FTS)
//! - Vector search (semantic similarity via embeddings)
//! - Hybrid search (weighted combination of both)

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use crate::error::MemoryError;

/// A single memory entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    /// Unique ID for this memory
    pub id: String,

    /// The content of the memory
    pub content: String,

    /// Tags for categorization
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,

    /// Source of the memory (conversation ID, tool output, etc.)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,

    /// When this memory was created
    pub created_at: DateTime<Utc>,

    /// When this memory was last accessed
    pub last_accessed: DateTime<Utc>,

    /// Relevance score (set by search operations)
    #[serde(default)]
    pub score: f32,

    /// Optional embedding vector (stored as blob in DB)
    #[serde(skip)]
    pub embedding: Option<Vec<f32>>,
}

/// A query for searching memories.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryQuery {
    /// The search text
    pub text: String,

    /// Maximum number of results
    #[serde(default = "default_limit")]
    pub limit: usize,

    /// Minimum relevance score threshold
    #[serde(default)]
    pub min_score: f32,

    /// Filter by tags
    #[serde(default)]
    pub tags: Vec<String>,

    /// Search mode
    #[serde(default)]
    pub mode: SearchMode,
}

fn default_limit() -> usize {
    10
}

/// How to search the memory.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchMode {
    /// Full-text search only (BM25)
    Keyword,
    /// Vector similarity only (cosine distance)
    Vector,
    /// Weighted combination of both (default)
    #[default]
    Hybrid,
}

/// The core MemoryBackend trait.
///
/// Implementations: SQLite, PostgreSQL, in-memory (for testing), none (no-op).
#[async_trait]
pub trait MemoryBackend: Send + Sync {
    /// The backend name (e.g., "sqlite", "postgres", "none").
    fn name(&self) -> &str;

    /// Store a new memory entry.
    async fn store(&self, entry: MemoryEntry) -> std::result::Result<String, MemoryError>;

    /// Search memories by query.
    async fn search(&self, query: MemoryQuery) -> std::result::Result<Vec<MemoryEntry>, MemoryError>;

    /// Delete a memory by ID.
    async fn delete(&self, id: &str) -> std::result::Result<bool, MemoryError>;

    /// Get a memory by ID.
    async fn get(&self, id: &str) -> std::result::Result<Option<MemoryEntry>, MemoryError>;

    /// Get total memory count.
    async fn count(&self) -> std::result::Result<usize, MemoryError>;

    /// Clear all memories.
    async fn clear(&self) -> std::result::Result<(), MemoryError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_query_defaults() {
        let query = MemoryQuery {
            text: "rust programming".into(),
            limit: default_limit(),
            min_score: 0.0,
            tags: vec![],
            mode: SearchMode::default(),
        };
        assert_eq!(query.limit, 10);
        assert!(matches!(query.mode, SearchMode::Hybrid));
    }

    #[test]
    fn memory_entry_serialization() {
        let entry = MemoryEntry {
            id: "mem_001".into(),
            content: "The user prefers Rust over C++".into(),
            tags: vec!["preference".into()],
            source: Some("conversation_123".into()),
            created_at: Utc::now(),
            last_accessed: Utc::now(),
            score: 0.95,
            embedding: None,
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("Rust over C++"));
        assert!(json.contains("preference"));
    }
}
