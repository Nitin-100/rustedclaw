//! File-based memory backend — persistent JSON-lines storage.
//!
//! Inspired by nanobot's MEMORY.md approach but uses a structured JSONL file
//! for reliable serialization. Each line is a JSON-encoded `MemoryEntry`.
//!
//! Storage location: `~/.rustedclaw/memory/memories.jsonl`
//!
//! This backend is simple, portable, human-inspectable, and requires zero
//! external dependencies (no SQLite, no Postgres).

use async_trait::async_trait;
use chrono::Utc;
use rustedclaw_core::error::MemoryError;
use rustedclaw_core::memory::{MemoryBackend, MemoryEntry, MemoryQuery};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, warn};
use uuid::Uuid;

/// A file-backed memory store using JSONL (one JSON object per line).
///
/// Entries are loaded into memory on creation and flushed to disk on every
/// mutation (store, delete, clear). This gives fast reads with durable writes.
pub struct FileBackend {
    path: PathBuf,
    entries: Arc<RwLock<Vec<MemoryEntry>>>,
}

impl FileBackend {
    /// Create a new file-based backend at the given path.
    ///
    /// If the file exists, entries are loaded from it.
    /// If the file does not exist, starts empty (file created on first write).
    pub fn new(path: PathBuf) -> Self {
        let entries = Self::load_from_disk(&path);
        debug!(path = %path.display(), count = entries.len(), "File memory backend loaded");
        Self {
            path,
            entries: Arc::new(RwLock::new(entries)),
        }
    }

    /// Default path: `~/.rustedclaw/memory/memories.jsonl`
    pub fn default_path() -> PathBuf {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home)
            .join(".rustedclaw")
            .join("memory")
            .join("memories.jsonl")
    }

    /// Load entries from a JSONL file.
    fn load_from_disk(path: &PathBuf) -> Vec<MemoryEntry> {
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return Vec::new(), // File doesn't exist yet — start empty
        };

        content
            .lines()
            .filter(|line| !line.trim().is_empty())
            .filter_map(|line| {
                match serde_json::from_str::<MemoryEntry>(line) {
                    Ok(entry) => Some(entry),
                    Err(e) => {
                        warn!(error = %e, "Skipping corrupted memory entry");
                        None
                    }
                }
            })
            .collect()
    }

    /// Flush all entries to disk as JSONL.
    async fn flush(&self) -> Result<(), MemoryError> {
        let entries = self.entries.read().await;

        // Ensure parent directory exists
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                MemoryError::Storage(format!("Failed to create memory directory: {e}"))
            })?;
        }

        let mut content = String::new();
        for entry in entries.iter() {
            let line = serde_json::to_string(entry).map_err(|e| {
                MemoryError::Storage(format!("Failed to serialize memory entry: {e}"))
            })?;
            content.push_str(&line);
            content.push('\n');
        }

        std::fs::write(&self.path, &content).map_err(|e| {
            MemoryError::Storage(format!("Failed to write memory file: {e}"))
        })?;

        Ok(())
    }
}

#[async_trait]
impl MemoryBackend for FileBackend {
    fn name(&self) -> &str {
        "file"
    }

    async fn store(&self, mut entry: MemoryEntry) -> Result<String, MemoryError> {
        if entry.id.is_empty() {
            entry.id = Uuid::new_v4().to_string();
        }
        let id = entry.id.clone();
        self.entries.write().await.push(entry);
        self.flush().await?;
        Ok(id)
    }

    async fn search(&self, query: MemoryQuery) -> Result<Vec<MemoryEntry>, MemoryError> {
        let entries = self.entries.read().await;
        let query_lower = query.text.to_lowercase();

        let mut results: Vec<MemoryEntry> = entries
            .iter()
            .filter(|e| {
                let content_match = e.content.to_lowercase().contains(&query_lower);
                let tag_match =
                    query.tags.is_empty() || query.tags.iter().any(|t| e.tags.contains(t));
                content_match && tag_match
            })
            .cloned()
            .map(|mut e| {
                // Keyword relevance scoring
                let occurrences = e.content.to_lowercase().matches(&query_lower).count();
                e.score = occurrences as f32 / (e.content.len() as f32 / 100.0).max(1.0);
                e.last_accessed = Utc::now();
                e
            })
            .filter(|e| e.score >= query.min_score)
            .collect();

        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(query.limit);

        Ok(results)
    }

    async fn delete(&self, id: &str) -> Result<bool, MemoryError> {
        let mut entries = self.entries.write().await;
        let len_before = entries.len();
        entries.retain(|e| e.id != id);
        let deleted = entries.len() < len_before;
        drop(entries);
        if deleted {
            self.flush().await?;
        }
        Ok(deleted)
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
        self.flush().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustedclaw_core::memory::SearchMode;
    use std::io::Write;
    use tempfile::NamedTempFile;

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
    async fn store_and_retrieve_persists() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_path_buf();
        drop(tmp); // Close file so backend can use it

        // Store
        let mem = FileBackend::new(path.clone());
        let id = mem.store(test_entry("Rust is great")).await.unwrap();
        assert!(!id.is_empty());

        // Verify file was written
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("Rust is great"));

        // Reload from disk — should find the entry
        let mem2 = FileBackend::new(path);
        let entry = mem2.get(&id).await.unwrap();
        assert!(entry.is_some());
        assert_eq!(entry.unwrap().content, "Rust is great");
    }

    #[tokio::test]
    async fn search_finds_by_keyword() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_path_buf();
        drop(tmp);

        let mem = FileBackend::new(path);
        mem.store(test_entry("The user prefers Rust")).await.unwrap();
        mem.store(test_entry("Python is also good")).await.unwrap();
        mem.store(test_entry("Rust has great performance")).await.unwrap();

        let query = MemoryQuery {
            text: "rust".into(),
            limit: 10,
            min_score: 0.0,
            tags: vec![],
            mode: SearchMode::Keyword,
        };

        let results = mem.search(query).await.unwrap();
        assert_eq!(results.len(), 2);
        // Both results should contain "rust" (case-insensitive)
        for r in &results {
            assert!(r.content.to_lowercase().contains("rust"));
        }
    }

    #[tokio::test]
    async fn delete_persists() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_path_buf();
        drop(tmp);

        let mem = FileBackend::new(path.clone());
        let id = mem.store(test_entry("To be deleted")).await.unwrap();
        assert!(mem.delete(&id).await.unwrap());

        // Reload — should be gone
        let mem2 = FileBackend::new(path);
        assert!(mem2.get(&id).await.unwrap().is_none());
        assert_eq!(mem2.count().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn clear_persists() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_path_buf();
        drop(tmp);

        let mem = FileBackend::new(path.clone());
        mem.store(test_entry("Entry 1")).await.unwrap();
        mem.store(test_entry("Entry 2")).await.unwrap();
        mem.clear().await.unwrap();

        let mem2 = FileBackend::new(path);
        assert_eq!(mem2.count().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn handles_missing_file_gracefully() {
        let path = PathBuf::from("/tmp/rustedclaw_test_nonexistent_memory.jsonl");
        let _ = std::fs::remove_file(&path);
        let mem = FileBackend::new(path);
        assert_eq!(mem.count().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn handles_corrupted_lines() {
        let mut tmp = NamedTempFile::new().unwrap();
        writeln!(tmp, r#"{{"id":"1","content":"valid","tags":[],"source":null,"created_at":"2026-01-01T00:00:00Z","last_accessed":"2026-01-01T00:00:00Z","score":0.0}}"#).unwrap();
        writeln!(tmp, "this is not json").unwrap();
        writeln!(tmp, r#"{{"id":"2","content":"also valid","tags":[],"source":null,"created_at":"2026-01-01T00:00:00Z","last_accessed":"2026-01-01T00:00:00Z","score":0.0}}"#).unwrap();
        let path = tmp.path().to_path_buf();

        let mem = FileBackend::new(path);
        // Should load 2 valid entries, skip the corrupted one
        assert_eq!(mem.count().await.unwrap(), 2);
    }
}
