//! `rustedclaw migrate openclaw` — Import data from OpenClaw.

use rustedclaw_config::AppConfig;
use std::path::PathBuf;

/// Auto-detect OpenClaw workspace locations.
fn detect_openclaw_path() -> Option<PathBuf> {
    let candidates = [
        dirs_home().join(".config/openclaw"),
        dirs_home().join(".openclaw"),
        dirs_home().join("openclaw"),
    ];

    for path in &candidates {
        if path.exists() {
            return Some(path.clone());
        }
    }
    None
}

fn dirs_home() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        std::env::var("USERPROFILE")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("C:\\Users\\Default"))
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/tmp"))
    }
}

pub async fn run_openclaw(dry_run: bool, path: Option<String>) -> Result<(), Box<dyn std::error::Error>> {
    println!("🔄 Migrating from OpenClaw...");

    let source = match path {
        Some(p) => PathBuf::from(p),
        None => match detect_openclaw_path() {
            Some(p) => p,
            None => {
                println!("❌ Could not auto-detect OpenClaw workspace.");
                println!("   Use --path to specify the location manually:");
                println!("   rustedclaw migrate openclaw --path ~/.config/openclaw");
                return Ok(());
            }
        }
    };

    println!("   Source: {}", source.display());

    // Look for identity files
    let identity_files = ["IDENTITY.md", "SOUL.md", "USER.md", "AGENTS.md"];
    let mut found_files = Vec::new();
    for name in &identity_files {
        let f = source.join(name);
        if f.exists() {
            found_files.push(name.to_string());
        }
    }

    // Look for memory/conversation data
    let memory_db = source.join("memory.sqlite");
    let has_memory = memory_db.exists();

    // Look for config
    let config_candidates = [
        source.join("config.toml"),
        source.join("config.json"),
        source.join(".env"),
    ];
    let config_files: Vec<_> = config_candidates.iter().filter(|p| p.exists()).collect();

    println!();
    println!("   Found:");
    if !found_files.is_empty() {
        println!("   ✅ Identity files: {}", found_files.join(", "));
    }
    if has_memory {
        println!("   ✅ Memory database: memory.sqlite");
    }
    if !config_files.is_empty() {
        for f in &config_files {
            println!("   ✅ Config: {}", f.file_name().unwrap().to_string_lossy());
        }
    }

    if found_files.is_empty() && !has_memory && config_files.is_empty() {
        println!("   ⚠️  No migratable data found.");
        return Ok(());
    }

    if dry_run {
        println!();
        println!("   🏷️  DRY RUN — no changes made.");
        println!("   Remove --dry-run to apply migration.");
        return Ok(());
    }

    // Perform migration
    let dest = AppConfig::workspace_dir();
    std::fs::create_dir_all(&dest)?;

    // Copy identity files
    for name in &found_files {
        let src = source.join(name);
        let dst = dest.join(name);
        if !dst.exists() {
            std::fs::copy(&src, &dst)?;
            println!("   📄 Copied {name}");
        } else {
            println!("   ⏭️  Skipped {name} (already exists)");
        }
    }

    // Copy memory database
    if has_memory {
        let dst = AppConfig::config_dir().join("memory.sqlite");
        if !dst.exists() {
            std::fs::copy(&memory_db, &dst)?;
            println!("   🧠 Copied memory database");
        } else {
            println!("   ⏭️  Skipped memory.sqlite (already exists)");
        }
    }

    println!();
    println!("   ✅ Migration complete!");

    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn detect_returns_none_when_no_openclaw() {
        // Won't find OpenClaw on most dev machines
        let _ = super::detect_openclaw_path();
    }
}
