//! In-memory backend — useful for testing and ephemeral sessions.

use async_trait::async_trait;
use chrono::Utc;
use rustedclaw_core::error::MemoryError;
use rustedclaw_core::memory::{MemoryBackend, MemoryEntry, MemoryQuery};
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

/// An in-memory backend that stores memories in a Vec.
/// Useful for testing and sessions where persistence isn't needed.
pub struct InMemoryBackend {
    entries: Arc<RwLock<Vec<MemoryEntry>>>,
}

impl InMemoryBackend {
    pub fn new() -> Self {
        Self {
            entries: Arc::new(RwLock::new(Vec::new())),
        }
    }
}

impl Default for InMemoryBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl MemoryBackend for InMemoryBackend {
    fn name(&self) -> &str { "in_memory" }

    async fn store(&self, mut entry: MemoryEntry) -> Result<String, MemoryError> {
        if entry.id.is_empty() {
            entry.id = Uuid::new_v4().to_string();
        }
        let id = entry.id.clone();
        self.entries.write().await.push(entry);
        Ok(id)
    }

    async fn search(&self, query: MemoryQuery) -> Result<Vec<MemoryEntry>, MemoryError> {
        let entries = self.entries.read().await;
        let query_lower = query.text.to_lowercase();

        let mut results: Vec<MemoryEntry> = entries
            .iter()
            .filter(|e| {
                let content_match = e.content.to_lowercase().contains(&query_lower);
                let tag_match = query.tags.is_empty()
                    || query.tags.iter().any(|t| e.tags.contains(t));
                content_match && tag_match
            })
            .cloned()
            .map(|mut e| {
                // Simple keyword relevance score
                let occurrences = e.content.to_lowercase().matches(&query_lower).count();
                e.score = occurrences as f32 / (e.content.len() as f32 / 100.0).max(1.0);
                e.last_accessed = Utc::now();
                e
            })
            .filter(|e| e.score >= query.min_score)
            .collect();

        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(query.limit);

        Ok(results)
    }

    async fn delete(&self, id: &str) -> Result<bool, MemoryError> {
        let mut entries = self.entries.write().await;
        let len_before = entries.len();
        entries.retain(|e| e.id != id);
        Ok(entries.len() < len_before)
    }

    async fn get(&self, id: &str) -> Result<Option<MemoryEntry>, MemoryError> {
        let entries = self.entries.read().await;
        Ok(entries.iter().find(|e| e.id == id).cloned())
    }

    async fn count(&self) -> Result<usize, MemoryError> {
        Ok(self.entries.read().await.len())
    }

    async fn clear(&self) -> Result<(), MemoryError> {
        self.entries.write().await.clear();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustedclaw_core::memory::SearchMode;

    fn test_entry(content: &str) -> MemoryEntry {
        MemoryEntry {
            id: String::new(),
            content: content.into(),
            tags: vec![],
            source: None,
            created_at: Utc::now(),
            last_accessed: Utc::now(),
            score: 0.0,
            embedding: None,
        }
    }

    #[tokio::test]
    async fn store_and_retrieve() {
        let mem = InMemoryBackend::new();
        let id = mem.store(test_entry("Rust is a systems language")).await.unwrap();
        assert!(!id.is_empty());

        let entry = mem.get(&id).await.unwrap();
        assert!(entry.is_some());
        assert_eq!(entry.unwrap().content, "Rust is a systems language");
    }

    #[tokio::test]
    async fn search_by_keyword() {
        let mem = InMemoryBackend::new();
        mem.store(test_entry("Rust is great for systems programming")).await.unwrap();
        mem.store(test_entry("Python is great for scripting")).await.unwrap();
        mem.store(test_entry("JavaScript runs in the browser")).await.unwrap();

        let results = mem.search(MemoryQuery {
            text: "Rust".into(),
            limit: 10,
            min_score: 0.0,
            tags: vec![],
            mode: SearchMode::Keyword,
        }).await.unwrap();

        assert_eq!(results.len(), 1);
        assert!(results[0].content.contains("Rust"));
    }

    #[tokio::test]
    async fn delete_entry() {
        let mem = InMemoryBackend::new();
        let id = mem.store(test_entry("To be deleted")).await.unwrap();
        assert_eq!(mem.count().await.unwrap(), 1);

        let deleted = mem.delete(&id).await.unwrap();
        assert!(deleted);
        assert_eq!(mem.count().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn clear_all() {
        let mem = InMemoryBackend::new();
        mem.store(test_entry("Entry 1")).await.unwrap();
        mem.store(test_entry("Entry 2")).await.unwrap();
        assert_eq!(mem.count().await.unwrap(), 2);

        mem.clear().await.unwrap();
        assert_eq!(mem.count().await.unwrap(), 0);
    }
}
