//! Memory system implementations for RustedClaw.

pub mod file_backend;
pub mod in_memory;
pub mod noop;
pub mod vector;

#[cfg(feature = "sqlite")]
pub mod sqlite;

#[cfg(feature = "postgres")]
pub mod postgres;

pub use file_backend::FileBackend;
pub use in_memory::InMemoryBackend;
pub use noop::NoopMemory;
pub use vector::{cosine_similarity, reciprocal_rank_fusion, vector_search};

#[cfg(feature = "sqlite")]
pub use sqlite::SqliteBackend;

#[cfg(feature = "postgres")]
pub use postgres::PostgresBackend;
