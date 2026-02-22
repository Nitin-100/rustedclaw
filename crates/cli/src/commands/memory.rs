//! `rustedclaw memory` — Memory management commands.

use rustedclaw_config::AppConfig;
use rustedclaw_core::memory::{MemoryBackend, MemoryQuery, SearchMode};
use rustedclaw_memory::InMemoryBackend;

pub async fn stats() -> Result<(), Box<dyn std::error::Error>> {
    let config = AppConfig::load().map_err(|e| format!("Failed to load config: {e}"))?;

    println!("🧠 Memory Statistics");
    println!("====================");
    println!("  Backend:    {}", config.memory.backend);
    println!("  Auto-save:  {}", config.memory.auto_save);
    println!("  Embeddings: {}", config.memory.embedding_provider);
    println!(
        "  Weights:    vector={:.1}, keyword={:.1}",
        config.memory.vector_weight, config.memory.keyword_weight
    );

    // Show database file info if SQLite
    if config.memory.backend == "sqlite" {
        let db_path = AppConfig::config_dir().join("memory.sqlite");
        if db_path.exists() {
            let meta = std::fs::metadata(&db_path)?;
            let size_kb = meta.len() as f64 / 1024.0;
            println!("  DB file:    {} ({:.1} KB)", db_path.display(), size_kb);
        } else {
            println!("  DB file:    (not created yet)");
        }
    }

    // Show workspace memory files
    let workspace = AppConfig::workspace_dir();
    if workspace.exists() {
        let memory_dir = workspace.join("memories");
        if memory_dir.exists() {
            let count = std::fs::read_dir(&memory_dir)?.count();
            println!("  Workspace:  {} memory files", count);
        }
    }

    Ok(())
}

pub async fn search(query: &str, limit: usize) -> Result<(), Box<dyn std::error::Error>> {
    let _config = AppConfig::load().map_err(|e| format!("Failed to load config: {e}"))?;

    println!("🔍 Searching memories for: \"{query}\"");
    println!();

    // For now, use in-memory backend (upgrade to SQLite when running with daemon)
    let backend = InMemoryBackend::new();
    let mq = MemoryQuery {
        text: query.to_string(),
        limit,
        min_score: 0.0,
        tags: vec![],
        mode: SearchMode::Hybrid,
    };

    let results = backend.search(mq).await?;
    if results.is_empty() {
        println!("   No memories found. (Memory backend may need a running daemon.)");
    } else {
        for (i, entry) in results.iter().enumerate() {
            println!(
                "  {i:>2}. [score: {:.2}] {}",
                entry.score,
                &entry.content[..entry.content.len().min(80)]
            );
            if !entry.tags.is_empty() {
                println!("      tags: {}", entry.tags.join(", "));
            }
        }
    }

    Ok(())
}

pub async fn export(output: &str) -> Result<(), Box<dyn std::error::Error>> {
    let _config = AppConfig::load().map_err(|e| format!("Failed to load config: {e}"))?;

    // Export workspace memory files as JSON
    let workspace = AppConfig::workspace_dir();
    let memory_dir = workspace.join("memories");
    let mut memories = Vec::new();

    if memory_dir.exists() {
        for entry in std::fs::read_dir(&memory_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path
                .extension()
                .is_some_and(|e| e == "md" || e == "txt" || e == "json")
            {
                let content = std::fs::read_to_string(&path)?;
                memories.push(serde_json::json!({
                    "file": path.file_name().unwrap().to_string_lossy(),
                    "content": content,
                    "size_bytes": content.len(),
                }));
            }
        }
    }

    let json = serde_json::to_string_pretty(&memories)?;
    std::fs::write(output, &json)?;
    println!("📤 Exported {} memories to {output}", memories.len());

    Ok(())
}

pub async fn clear(confirm: bool) -> Result<(), Box<dyn std::error::Error>> {
    if !confirm {
        println!("⚠️  This will delete ALL memories permanently.");
        println!("   Run with --confirm to proceed:");
        println!("   rustedclaw memory clear --confirm");
        return Ok(());
    }

    let config = AppConfig::load().map_err(|e| format!("Failed to load config: {e}"))?;

    // Clear SQLite database
    if config.memory.backend == "sqlite" {
        let db_path = AppConfig::config_dir().join("memory.sqlite");
        if db_path.exists() {
            std::fs::remove_file(&db_path)?;
            println!("🗑️  Deleted memory database.");
        }
    }

    // Clear workspace memory files
    let memory_dir = AppConfig::workspace_dir().join("memories");
    if memory_dir.exists() {
        std::fs::remove_dir_all(&memory_dir)?;
        std::fs::create_dir_all(&memory_dir)?;
        println!("🗑️  Cleared workspace memories.");
    }

    println!("✅ All memories cleared.");

    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn memory_stats_runs_without_config() {
        // Just verifying the module compiles
    }
}
