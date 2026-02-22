//! No-op memory backend — disables persistent memory entirely.

use async_trait::async_trait;
use rustedclaw_core::error::MemoryError;
use rustedclaw_core::memory::{MemoryBackend, MemoryEntry, MemoryQuery};

/// A no-op memory backend that stores nothing.
pub struct NoopMemory;

#[async_trait]
impl MemoryBackend for NoopMemory {
    fn name(&self) -> &str { "none" }

    async fn store(&self, _entry: MemoryEntry) -> Result<String, MemoryError> {
        Ok(String::new())
    }

    async fn search(&self, _query: MemoryQuery) -> Result<Vec<MemoryEntry>, MemoryError> {
        Ok(Vec::new())
    }

    async fn delete(&self, _id: &str) -> Result<bool, MemoryError> {
        Ok(false)
    }

    async fn get(&self, _id: &str) -> Result<Option<MemoryEntry>, MemoryError> {
        Ok(None)
    }

    async fn count(&self) -> Result<usize, MemoryError> {
        Ok(0)
    }

    async fn clear(&self) -> Result<(), MemoryError> {
        Ok(())
    }
}
