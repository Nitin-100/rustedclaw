//! `rustedclaw estop` — Emergency stop all running tasks.

use rustedclaw_config::AppConfig;
use std::path::PathBuf;

/// Estop state file path.
fn estop_file() -> PathBuf {
    AppConfig::config_dir().join(".estop")
}

pub async fn run(resume: bool) -> Result<(), Box<dyn std::error::Error>> {
    let path = estop_file();

    if resume {
        if path.exists() {
            std::fs::remove_file(&path)?;
            println!("✅ Emergency stop RELEASED — tasks may resume.");
        } else {
            println!("ℹ️  No emergency stop active.");
        }
    } else {
        // Engage estop
        std::fs::create_dir_all(AppConfig::config_dir())?;
        std::fs::write(&path, chrono::Utc::now().to_rfc3339())?;
        println!("🛑 EMERGENCY STOP ENGAGED");
        println!("   All background tasks halted.");
        println!("   Gateway will reject new requests.");
        println!();
        println!("   To resume: rustedclaw estop --resume");
    }

    Ok(())
}

/// Check if estop is currently engaged (for use by other subsystems).
#[allow(dead_code)]
pub fn is_engaged() -> bool {
    estop_file().exists()
}

#[cfg(test)]
mod tests {
    #[test]
    fn estop_file_path_is_valid() {
        let path = super::estop_file();
        assert!(path.to_str().unwrap().contains(".estop"));
    }
}
