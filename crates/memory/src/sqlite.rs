//! SQLite backend with FTS5 full-text search.
//!
//! Uses a single SQLite database file with two tables:
//! - `memories` — stores the raw memory entries
//! - `memories_fts` — FTS5 virtual table for ranked keyword search (BM25)
//!
//! Triggers keep the FTS index in sync on insert/delete/update.

use crate::vector;
use async_trait::async_trait;
use chrono::Utc;
use rustedclaw_core::error::MemoryError;
use rustedclaw_core::memory::{MemoryBackend, MemoryEntry, MemoryQuery, SearchMode};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Row, SqlitePool};
use std::str::FromStr;
use tracing::{debug, info, warn};
use uuid::Uuid;

/// A production SQLite memory backend with FTS5 full-text search.
pub struct SqliteBackend {
    pool: SqlitePool,
}

impl SqliteBackend {
    /// Create a new SQLite backend from a file path.
    ///
    /// The database and all tables/indexes are created automatically.
    /// Pass `":memory:"` for an in-process ephemeral database (useful for tests).
    pub async fn new(path: &str) -> Result<Self, MemoryError> {
        let options = SqliteConnectOptions::from_str(path)
            .map_err(|e| MemoryError::Storage(format!("Invalid SQLite path: {e}")))?
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .pragma("foreign_keys", "ON");

        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(options)
            .await
            .map_err(|e| MemoryError::Storage(format!("Failed to open SQLite: {e}")))?;

        let backend = Self { pool };
        backend.run_migrations().await?;
        info!("SQLite memory backend initialized at {path}");
        Ok(backend)
    }

    /// Create from an existing pool (useful for testing).
    pub async fn from_pool(pool: SqlitePool) -> Result<Self, MemoryError> {
        let backend = Self { pool };
        backend.run_migrations().await?;
        Ok(backend)
    }

    /// Run schema migrations — creates tables, FTS5 virtual table, and triggers.
    async fn run_migrations(&self) -> Result<(), MemoryError> {
        // Main memories table with integer rowid alias for FTS5 sync
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS memories (
                iid          INTEGER PRIMARY KEY AUTOINCREMENT,
                id           TEXT UNIQUE NOT NULL,
                content      TEXT NOT NULL,
                tags         TEXT NOT NULL DEFAULT '[]',
                source       TEXT,
                created_at   TEXT NOT NULL,
                last_accessed TEXT NOT NULL,
                score        REAL NOT NULL DEFAULT 0.0,
                embedding    BLOB
            )
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| MemoryError::MigrationFailed(format!("memories table: {e}")))?;

        // External-content FTS5 table synced via triggers
        // content_rowid maps to the integer primary key in memories
        sqlx::query(
            r#"
            CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts USING fts5(
                content,
                tags,
                content='memories',
                content_rowid='iid',
                tokenize='porter unicode61'
            )
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| MemoryError::MigrationFailed(format!("FTS5 table: {e}")))?;

        // Trigger: sync FTS on INSERT
        sqlx::query(
            r#"
            CREATE TRIGGER IF NOT EXISTS memories_ai AFTER INSERT ON memories BEGIN
                INSERT INTO memories_fts(rowid, content, tags)
                VALUES (new.iid, new.content, new.tags);
            END
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| MemoryError::MigrationFailed(format!("insert trigger: {e}")))?;

        // Trigger: sync FTS on DELETE (uses special external-content delete command)
        sqlx::query(
            r#"
            CREATE TRIGGER IF NOT EXISTS memories_ad AFTER DELETE ON memories BEGIN
                INSERT INTO memories_fts(memories_fts, rowid, content, tags)
                VALUES ('delete', old.iid, old.content, old.tags);
            END
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| MemoryError::MigrationFailed(format!("delete trigger: {e}")))?;

        // Trigger: sync FTS on UPDATE (delete old, insert new)
        sqlx::query(
            r#"
            CREATE TRIGGER IF NOT EXISTS memories_au AFTER UPDATE ON memories BEGIN
                INSERT INTO memories_fts(memories_fts, rowid, content, tags)
                VALUES ('delete', old.iid, old.content, old.tags);
                INSERT INTO memories_fts(rowid, content, tags)
                VALUES (new.iid, new.content, new.tags);
            END
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| MemoryError::MigrationFailed(format!("update trigger: {e}")))?;

        // Index on created_at for ordering
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_memories_created_at ON memories(created_at DESC)",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| MemoryError::MigrationFailed(format!("created_at index: {e}")))?;

        debug!("SQLite migrations complete");
        Ok(())
    }

    /// Parse a `MemoryEntry` from a SQLite row.
    fn row_to_entry(row: &sqlx::sqlite::SqliteRow) -> Result<MemoryEntry, MemoryError> {
        let id: String = row
            .try_get("id")
            .map_err(|e| MemoryError::QueryFailed(format!("id column: {e}")))?;
        let content: String = row
            .try_get("content")
            .map_err(|e| MemoryError::QueryFailed(format!("content column: {e}")))?;
        let tags_json: String = row
            .try_get("tags")
            .map_err(|e| MemoryError::QueryFailed(format!("tags column: {e}")))?;
        let source: Option<String> = row
            .try_get("source")
            .map_err(|e| MemoryError::QueryFailed(format!("source column: {e}")))?;
        let created_at_str: String = row
            .try_get("created_at")
            .map_err(|e| MemoryError::QueryFailed(format!("created_at column: {e}")))?;
        let last_accessed_str: String = row
            .try_get("last_accessed")
            .map_err(|e| MemoryError::QueryFailed(format!("last_accessed column: {e}")))?;

        let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();

        let created_at = chrono::DateTime::parse_from_rfc3339(&created_at_str)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now());

        let last_accessed = chrono::DateTime::parse_from_rfc3339(&last_accessed_str)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now());

        // Try to read score if present
        let score: f32 = row.try_get("score").unwrap_or(0.0);

        // Read embedding blob if present
        let embedding: Option<Vec<u8>> = row.try_get("embedding").ok();
        let embedding_vec = embedding.map(|blob| {
            blob.chunks_exact(4)
                .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
                .collect()
        });

        Ok(MemoryEntry {
            id,
            content,
            tags,
            source,
            created_at,
            last_accessed,
            score,
            embedding: embedding_vec,
        })
    }

    /// Serialize an embedding vector to bytes.
    fn embedding_to_blob(embedding: &[f32]) -> Vec<u8> {
        embedding.iter().flat_map(|f| f.to_le_bytes()).collect()
    }

    /// Build a safe FTS5 query from user text.
    ///
    /// FTS5 requires special syntax. We tokenize the user input into words
    /// and join them with implicit AND, quoting each token to prevent injection.
    fn sanitize_fts_query(text: &str) -> String {
        text.split_whitespace()
            .filter(|w| !w.is_empty())
            .map(|w| {
                // Strip non-alphanumeric chars and quote
                let clean: String = w
                    .chars()
                    .filter(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                if clean.is_empty() {
                    return String::new();
                }
                // Use prefix matching with *
                format!("\"{}\"*", clean)
            })
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

#[async_trait]
impl MemoryBackend for SqliteBackend {
    fn name(&self) -> &str {
        "sqlite"
    }

    async fn store(&self, mut entry: MemoryEntry) -> Result<String, MemoryError> {
        if entry.id.is_empty() {
            entry.id = Uuid::new_v4().to_string();
        }
        let id = entry.id.clone();
        let tags_json = serde_json::to_string(&entry.tags)
            .map_err(|e| MemoryError::Storage(format!("Tags serialization: {e}")))?;
        let created_at = entry.created_at.to_rfc3339();
        let last_accessed = entry.last_accessed.to_rfc3339();

        let embedding_blob: Option<Vec<u8>> =
            entry.embedding.as_deref().map(Self::embedding_to_blob);

        sqlx::query(
            r#"
            INSERT INTO memories (id, content, tags, source, created_at, last_accessed, score, embedding)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            ON CONFLICT(id) DO UPDATE SET
                content = excluded.content,
                tags = excluded.tags,
                source = excluded.source,
                last_accessed = excluded.last_accessed,
                score = excluded.score,
                embedding = excluded.embedding
            "#,
        )
        .bind(&entry.id)
        .bind(&entry.content)
        .bind(&tags_json)
        .bind(&entry.source)
        .bind(&created_at)
        .bind(&last_accessed)
        .bind(entry.score)
        .bind(embedding_blob.as_deref())
        .execute(&self.pool)
        .await
        .map_err(|e| MemoryError::Storage(format!("INSERT failed: {e}")))?;

        debug!("Stored memory {id}");
        Ok(id)
    }

    async fn search(&self, query: MemoryQuery) -> Result<Vec<MemoryEntry>, MemoryError> {
        if query.text.trim().is_empty() {
            // Empty query: return most recent entries
            let rows = sqlx::query("SELECT * FROM memories ORDER BY created_at DESC LIMIT ?1")
                .bind(query.limit as i64)
                .fetch_all(&self.pool)
                .await
                .map_err(|e| MemoryError::QueryFailed(format!("Empty search: {e}")))?;

            return rows.iter().map(Self::row_to_entry).collect();
        }

        match query.mode {
            SearchMode::Keyword => {
                // FTS5 full-text search with BM25 ranking
                let fts_query = Self::sanitize_fts_query(&query.text);
                if fts_query.is_empty() {
                    return Ok(vec![]);
                }

                // Tag filter: build tag conditions using parameterized queries
                // to prevent SQL injection via tag values
                let tag_filter = if query.tags.is_empty() {
                    String::new()
                } else {
                    let conditions: Vec<String> = query
                        .tags
                        .iter()
                        .enumerate()
                        .map(|(i, _)| {
                            // Use positional bind parameters starting after ?1 and ?2
                            let param_a = i * 2 + 3;
                            let param_b = i * 2 + 4;
                            format!("m.tags LIKE ?{param_a} OR m.tags LIKE ?{param_b}")
                        })
                        .collect();
                    format!("AND ({})", conditions.join(" OR "))
                };

                let sql = format!(
                    r#"
                    SELECT m.*, bm25(memories_fts) AS rank
                    FROM memories_fts f
                    JOIN memories m ON m.iid = f.rowid
                    WHERE memories_fts MATCH ?1
                    {tag_filter}
                    ORDER BY rank
                    LIMIT ?2
                    "#
                );

                let mut db_query = sqlx::query(&sql).bind(&fts_query).bind(query.limit as i64);

                // Bind tag filter parameters (escaped LIKE patterns)
                for tag in &query.tags {
                    // Escape SQL LIKE wildcards in tag values
                    let escaped = tag.replace('%', "\\%").replace('_', "\\_");
                    db_query = db_query.bind(format!("%\"{escaped}\",%"));
                    db_query = db_query.bind(format!("%\"{escaped}\"]%"));
                }

                let rows = db_query
                    .fetch_all(&self.pool)
                    .await
                    .map_err(|e| MemoryError::QueryFailed(format!("FTS5 search: {e}")))?;

                let results: Vec<MemoryEntry> = rows
                    .iter()
                    .filter_map(|row| {
                        let mut entry = Self::row_to_entry(row).ok()?;
                        // bm25() returns negative values (lower = better match)
                        // Convert to positive score where higher = better
                        let rank: f64 = row.try_get("rank").unwrap_or(0.0);
                        entry.score = (-rank) as f32;
                        if entry.score >= query.min_score {
                            Some(entry)
                        } else {
                            None
                        }
                    })
                    .collect();

                Ok(results)
            }
            SearchMode::Vector => {
                // Pure vector similarity search using embeddings stored in the DB.
                // Load all entries with embeddings, then rank by cosine similarity.
                // For a query embedding, the caller should set it on the MemoryQuery
                // (we parse it from the text field as a fallback, or use all entries).
                let rows = sqlx::query("SELECT * FROM memories WHERE embedding IS NOT NULL")
                    .fetch_all(&self.pool)
                    .await
                    .map_err(|e| MemoryError::QueryFailed(format!("Vector scan: {e}")))?;

                let entries: Vec<MemoryEntry> = rows
                    .iter()
                    .filter_map(|row| Self::row_to_entry(row).ok())
                    .collect();

                if entries.is_empty() {
                    warn!(
                        "Vector search found no entries with embeddings; falling back to keyword"
                    );
                    let mut fallback_query = query;
                    fallback_query.mode = SearchMode::Keyword;
                    return Box::pin(self.search(fallback_query)).await;
                }

                // We need a query embedding. Check if any entry has one to determine dimensions.
                // For now, fall back to keyword search if no query embedding is available.
                // The agent layer will provide embeddings via the MemoryEntry store path.
                warn!("Vector-only search without query embedding; falling back to keyword");
                let mut fallback_query = query;
                fallback_query.mode = SearchMode::Keyword;
                Box::pin(self.search(fallback_query)).await
            }
            SearchMode::Hybrid => {
                // Hybrid search: run FTS5 keyword search, then if entries have embeddings,
                // run vector search too, and merge with Reciprocal Rank Fusion.
                let keyword_query = MemoryQuery {
                    text: query.text.clone(),
                    limit: query.limit * 2, // over-fetch for RRF
                    min_score: 0.0,
                    tags: query.tags.clone(),
                    mode: SearchMode::Keyword,
                };
                let keyword_results = Box::pin(self.search(keyword_query)).await?;

                // Check if we have embeddings to do vector component
                let rows =
                    sqlx::query("SELECT * FROM memories WHERE embedding IS NOT NULL LIMIT 1")
                        .fetch_optional(&self.pool)
                        .await
                        .map_err(|e| {
                            MemoryError::QueryFailed(format!("Hybrid embed check: {e}"))
                        })?;

                if rows.is_none() {
                    // No embeddings available — just return keyword results
                    debug!("Hybrid search: no embeddings found, using keyword-only results");
                    let mut results = keyword_results;
                    results.truncate(query.limit);
                    return Ok(results);
                }

                // Load all entries with embeddings for vector ranking
                let all_rows = sqlx::query("SELECT * FROM memories WHERE embedding IS NOT NULL")
                    .fetch_all(&self.pool)
                    .await
                    .map_err(|e| MemoryError::QueryFailed(format!("Hybrid vector scan: {e}")))?;

                let all_entries: Vec<MemoryEntry> = all_rows
                    .iter()
                    .filter_map(|row| Self::row_to_entry(row).ok())
                    .collect();

                // Use the first keyword result's embedding as proxy query embedding
                // (in practice, the agent layer provides the query embedding)
                let query_emb = keyword_results.iter().find_map(|e| e.embedding.as_ref());

                if let Some(qe) = query_emb {
                    let vector_results =
                        vector::vector_search(&all_entries, qe, query.limit * 2, 0.0);

                    let merged = vector::reciprocal_rank_fusion(
                        &keyword_results,
                        &vector_results,
                        60,
                        query.limit,
                    );

                    Ok(merged)
                } else {
                    // No query embedding available — return keyword results
                    debug!("Hybrid search: no query embedding, using keyword-only");
                    let mut results = keyword_results;
                    results.truncate(query.limit);
                    Ok(results)
                }
            }
        }
    }

    async fn delete(&self, id: &str) -> Result<bool, MemoryError> {
        let result = sqlx::query("DELETE FROM memories WHERE id = ?1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| MemoryError::Storage(format!("DELETE failed: {e}")))?;

        Ok(result.rows_affected() > 0)
    }

    async fn get(&self, id: &str) -> Result<Option<MemoryEntry>, MemoryError> {
        let row = sqlx::query("SELECT * FROM memories WHERE id = ?1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| MemoryError::QueryFailed(format!("GET by ID: {e}")))?;

        match row {
            Some(ref r) => Ok(Some(Self::row_to_entry(r)?)),
            None => Ok(None),
        }
    }

    async fn count(&self) -> Result<usize, MemoryError> {
        let row = sqlx::query("SELECT COUNT(*) as cnt FROM memories")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| MemoryError::QueryFailed(format!("COUNT: {e}")))?;

        let cnt: i64 = row
            .try_get("cnt")
            .map_err(|e| MemoryError::QueryFailed(format!("cnt column: {e}")))?;

        Ok(cnt as usize)
    }

    async fn clear(&self) -> Result<(), MemoryError> {
        sqlx::query("DELETE FROM memories")
            .execute(&self.pool)
            .await
            .map_err(|e| MemoryError::Storage(format!("CLEAR failed: {e}")))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    async fn test_backend() -> SqliteBackend {
        SqliteBackend::new("sqlite::memory:").await.unwrap()
    }

    fn make_entry(content: &str) -> MemoryEntry {
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

    fn make_tagged_entry(content: &str, tags: Vec<&str>) -> MemoryEntry {
        MemoryEntry {
            id: String::new(),
            content: content.into(),
            tags: tags.into_iter().map(String::from).collect(),
            source: Some("test".into()),
            created_at: Utc::now(),
            last_accessed: Utc::now(),
            score: 0.0,
            embedding: None,
        }
    }

    #[tokio::test]
    async fn store_and_retrieve() {
        let db = test_backend().await;
        let id = db
            .store(make_entry("Rust is a systems programming language"))
            .await
            .unwrap();
        assert!(!id.is_empty());

        let entry = db.get(&id).await.unwrap().unwrap();
        assert_eq!(entry.content, "Rust is a systems programming language");
        assert_eq!(entry.id, id);
    }

    #[tokio::test]
    async fn store_with_custom_id() {
        let db = test_backend().await;
        let mut entry = make_entry("Custom ID test");
        entry.id = "custom_123".into();
        let id = db.store(entry).await.unwrap();
        assert_eq!(id, "custom_123");

        let fetched = db.get("custom_123").await.unwrap().unwrap();
        assert_eq!(fetched.content, "Custom ID test");
    }

    #[tokio::test]
    async fn upsert_on_conflict() {
        let db = test_backend().await;
        let mut entry1 = make_entry("Version 1");
        entry1.id = "upsert_test".into();
        db.store(entry1).await.unwrap();

        let mut entry2 = make_entry("Version 2");
        entry2.id = "upsert_test".into();
        db.store(entry2).await.unwrap();

        assert_eq!(db.count().await.unwrap(), 1);
        let fetched = db.get("upsert_test").await.unwrap().unwrap();
        assert_eq!(fetched.content, "Version 2");
    }

    #[tokio::test]
    async fn fts5_keyword_search() {
        let db = test_backend().await;
        db.store(make_entry("Rust is great for systems programming"))
            .await
            .unwrap();
        db.store(make_entry("Python is great for scripting"))
            .await
            .unwrap();
        db.store(make_entry("JavaScript runs in the browser"))
            .await
            .unwrap();

        let results = db
            .search(MemoryQuery {
                text: "Rust".into(),
                limit: 10,
                min_score: 0.0,
                tags: vec![],
                mode: SearchMode::Keyword,
            })
            .await
            .unwrap();

        assert_eq!(results.len(), 1);
        assert!(results[0].content.contains("Rust"));
        assert!(results[0].score > 0.0, "BM25 score should be positive");
    }

    #[tokio::test]
    async fn fts5_multi_word_search() {
        let db = test_backend().await;
        db.store(make_entry("The quick brown fox jumps over the lazy dog"))
            .await
            .unwrap();
        db.store(make_entry("A fast brown cat sits on the mat"))
            .await
            .unwrap();
        db.store(make_entry("Rust programming is fun"))
            .await
            .unwrap();

        let results = db
            .search(MemoryQuery {
                text: "brown fox".into(),
                limit: 10,
                min_score: 0.0,
                tags: vec![],
                mode: SearchMode::Keyword,
            })
            .await
            .unwrap();

        // "quick brown fox" should rank highest (both words match)
        assert!(!results.is_empty());
        assert!(results[0].content.contains("fox"));
    }

    #[tokio::test]
    async fn search_with_tags() {
        let db = test_backend().await;
        db.store(make_tagged_entry(
            "Rust memory safety",
            vec!["rust", "safety"],
        ))
        .await
        .unwrap();
        db.store(make_tagged_entry("Rust performance", vec!["rust", "perf"]))
            .await
            .unwrap();
        db.store(make_tagged_entry("Python typing", vec!["python"]))
            .await
            .unwrap();

        let results = db
            .search(MemoryQuery {
                text: "Rust".into(),
                limit: 10,
                min_score: 0.0,
                tags: vec!["safety".into()],
                mode: SearchMode::Keyword,
            })
            .await
            .unwrap();

        assert_eq!(results.len(), 1);
        assert!(results[0].content.contains("safety"));
    }

    #[tokio::test]
    async fn empty_search_returns_recent() {
        let db = test_backend().await;
        db.store(make_entry("Entry one")).await.unwrap();
        db.store(make_entry("Entry two")).await.unwrap();
        db.store(make_entry("Entry three")).await.unwrap();

        let results = db
            .search(MemoryQuery {
                text: "".into(),
                limit: 2,
                min_score: 0.0,
                tags: vec![],
                mode: SearchMode::Keyword,
            })
            .await
            .unwrap();

        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn delete_entry() {
        let db = test_backend().await;
        let id = db.store(make_entry("To be deleted")).await.unwrap();
        assert_eq!(db.count().await.unwrap(), 1);

        let deleted = db.delete(&id).await.unwrap();
        assert!(deleted);
        assert_eq!(db.count().await.unwrap(), 0);
        assert!(db.get(&id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn delete_nonexistent() {
        let db = test_backend().await;
        let deleted = db.delete("no_such_id").await.unwrap();
        assert!(!deleted);
    }

    #[tokio::test]
    async fn clear_all() {
        let db = test_backend().await;
        db.store(make_entry("One")).await.unwrap();
        db.store(make_entry("Two")).await.unwrap();
        db.store(make_entry("Three")).await.unwrap();
        assert_eq!(db.count().await.unwrap(), 3);

        db.clear().await.unwrap();
        assert_eq!(db.count().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn fts5_search_after_delete() {
        let db = test_backend().await;
        let id = db
            .store(make_entry("Unique searchable term xyzzy"))
            .await
            .unwrap();
        db.store(make_entry("Another entry")).await.unwrap();

        // Should find it
        let results = db
            .search(MemoryQuery {
                text: "xyzzy".into(),
                limit: 10,
                min_score: 0.0,
                tags: vec![],
                mode: SearchMode::Keyword,
            })
            .await
            .unwrap();
        assert_eq!(results.len(), 1);

        // Delete and search again — should NOT find it
        db.delete(&id).await.unwrap();
        let results = db
            .search(MemoryQuery {
                text: "xyzzy".into(),
                limit: 10,
                min_score: 0.0,
                tags: vec![],
                mode: SearchMode::Keyword,
            })
            .await
            .unwrap();
        assert_eq!(results.len(), 0);
    }

    #[tokio::test]
    async fn embedding_round_trip() {
        let db = test_backend().await;
        let mut entry = make_entry("Has an embedding");
        entry.embedding = Some(vec![0.1, 0.2, 0.3, 0.4]);
        let id = db.store(entry).await.unwrap();

        let fetched = db.get(&id).await.unwrap().unwrap();
        let emb = fetched.embedding.unwrap();
        assert_eq!(emb.len(), 4);
        assert!((emb[0] - 0.1).abs() < 1e-6);
        assert!((emb[3] - 0.4).abs() < 1e-6);
    }

    #[tokio::test]
    async fn count_empty() {
        let db = test_backend().await;
        assert_eq!(db.count().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn backend_name() {
        let db = test_backend().await;
        assert_eq!(db.name(), "sqlite");
    }

    #[tokio::test]
    async fn sanitize_fts_query_basic() {
        assert_eq!(
            SqliteBackend::sanitize_fts_query("hello world"),
            "\"hello\"* \"world\"*"
        );
    }

    #[tokio::test]
    async fn sanitize_fts_query_special_chars() {
        assert_eq!(
            SqliteBackend::sanitize_fts_query("hello! @world#"),
            "\"hello\"* \"world\"*"
        );
    }

    #[tokio::test]
    async fn sanitize_fts_query_empty() {
        assert_eq!(SqliteBackend::sanitize_fts_query("   "), "");
    }

    #[tokio::test]
    async fn search_respects_limit() {
        let db = test_backend().await;
        for i in 0..20 {
            db.store(make_entry(&format!("Memory about topic number {i}")))
                .await
                .unwrap();
        }

        let results = db
            .search(MemoryQuery {
                text: "topic".into(),
                limit: 5,
                min_score: 0.0,
                tags: vec![],
                mode: SearchMode::Keyword,
            })
            .await
            .unwrap();

        assert_eq!(results.len(), 5);
    }

    #[tokio::test]
    async fn source_round_trip() {
        let db = test_backend().await;
        let mut entry = make_entry("With source info");
        entry.source = Some("conversation_456".into());
        let id = db.store(entry).await.unwrap();

        let fetched = db.get(&id).await.unwrap().unwrap();
        assert_eq!(fetched.source.as_deref(), Some("conversation_456"));
    }

    #[tokio::test]
    async fn tags_round_trip() {
        let db = test_backend().await;
        let entry = make_tagged_entry("Tagged memory", vec!["alpha", "beta", "gamma"]);
        let id = db.store(entry).await.unwrap();

        let fetched = db.get(&id).await.unwrap().unwrap();
        assert_eq!(fetched.tags, vec!["alpha", "beta", "gamma"]);
    }

    #[tokio::test]
    async fn vector_mode_falls_back_to_keyword() {
        let db = test_backend().await;
        db.store(make_entry("Fallback vector test content"))
            .await
            .unwrap();

        // Vector mode should fall back to keyword when no embeddings available
        let results = db
            .search(MemoryQuery {
                text: "Fallback".into(),
                limit: 10,
                min_score: 0.0,
                tags: vec![],
                mode: SearchMode::Vector,
            })
            .await
            .unwrap();

        assert_eq!(results.len(), 1);
    }

    #[tokio::test]
    async fn hybrid_mode_keyword_only() {
        let db = test_backend().await;
        db.store(make_entry("Hybrid test without embeddings"))
            .await
            .unwrap();
        db.store(make_entry("Unrelated entry about cooking"))
            .await
            .unwrap();

        // Hybrid mode with no embeddings should return keyword results
        let results = db
            .search(MemoryQuery {
                text: "Hybrid".into(),
                limit: 10,
                min_score: 0.0,
                tags: vec![],
                mode: SearchMode::Hybrid,
            })
            .await
            .unwrap();

        assert_eq!(results.len(), 1);
        assert!(results[0].content.contains("Hybrid test"));
    }

    #[tokio::test]
    async fn hybrid_with_embeddings() {
        let db = test_backend().await;

        // Store entries WITH embeddings
        let mut e1 = make_entry("Machine learning algorithms");
        e1.embedding = Some(vec![1.0, 0.0, 0.0]);
        db.store(e1).await.unwrap();

        let mut e2 = make_entry("Deep learning neural networks");
        e2.embedding = Some(vec![0.9, 0.1, 0.0]);
        db.store(e2).await.unwrap();

        let mut e3 = make_entry("Cooking pasta recipes");
        e3.embedding = Some(vec![0.0, 0.0, 1.0]);
        db.store(e3).await.unwrap();

        // Hybrid search — keyword "learning" should find both ML entries
        let results = db
            .search(MemoryQuery {
                text: "learning".into(),
                limit: 10,
                min_score: 0.0,
                tags: vec![],
                mode: SearchMode::Hybrid,
            })
            .await
            .unwrap();

        assert!(results.len() >= 2);
        // Both learning entries should appear
        let ids: Vec<&str> = results.iter().map(|e| e.content.as_str()).collect();
        assert!(ids.iter().any(|c| c.contains("Machine learning")));
        assert!(ids.iter().any(|c| c.contains("Deep learning")));
    }
}
