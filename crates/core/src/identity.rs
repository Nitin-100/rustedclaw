//! Identity system — agent personality and system prompt construction.
//!
//! Context loading follows a layered hierarchy (later layers override/append):
//!
//! 1. **Built-in defaults** — hardcoded fallback system prompt
//! 2. **Global identity** — `~/.rustedclaw/workspace/IDENTITY.md`
//! 3. **Global soul** — `~/.rustedclaw/workspace/SOUL.md` (personality, tone, style)
//! 4. **Global user prefs** — `~/.rustedclaw/workspace/USER.md` (user-specific context)
//! 5. **Project context** — `.rustedclaw/AGENTS.md` in the current working directory
//! 6. **Project rules** — `.rustedclaw/RULES.md` in the current working directory
//! 7. **Extra context files** — any additional files specified in config
//!
//! Each file is optional. Missing files are silently skipped.
//! The final system prompt is assembled by concatenating all sections.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tracing::{debug, warn};

/// Well-known context file names.
pub const IDENTITY_FILE: &str = "IDENTITY.md";
pub const SOUL_FILE: &str = "SOUL.md";
pub const USER_FILE: &str = "USER.md";
pub const AGENTS_FILE: &str = "AGENTS.md";
pub const RULES_FILE: &str = "RULES.md";

/// The agent's identity configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Identity {
    /// The identity format being used
    pub format: IdentityFormat,

    /// The agent's name
    pub name: String,

    /// Core personality description
    pub personality: String,

    /// System prompt built from identity files
    pub system_prompt: String,

    /// Behavioral guidelines
    #[serde(default)]
    pub guidelines: Vec<String>,

    /// Which context files were loaded (for diagnostics)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub loaded_files: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentityFormat {
    /// Markdown files (IDENTITY.md, SOUL.md, USER.md, AGENTS.md, RULES.md)
    #[default]
    RustedClaw,
    /// JSON-based AI Entity Object Specification
    Aieos,
}

/// Configuration for context loading paths.
#[derive(Debug, Clone, Default)]
pub struct ContextPaths {
    /// Global identity directory (e.g., ~/.rustedclaw/workspace/)
    pub global_dir: Option<PathBuf>,

    /// Project-local context directory (e.g., ./.rustedclaw/)
    pub project_dir: Option<PathBuf>,

    /// Additional context files to load (absolute paths)
    pub extra_files: Vec<PathBuf>,

    /// Optional system prompt override (skips all file loading)
    pub system_prompt_override: Option<String>,
}

/// A single loaded context section with metadata.
#[derive(Debug, Clone)]
struct ContextSection {
    /// Source file path
    source: String,
    /// Section heading for the assembled prompt
    heading: String,
    /// The content loaded from the file
    content: String,
}

impl Identity {
    /// Create a default identity for when no files are configured.
    pub fn default_identity() -> Self {
        Self {
            format: IdentityFormat::RustedClaw,
            name: "RustedClaw".into(),
            personality: "A helpful, capable AI assistant.".into(),
            system_prompt: Self::fallback_system_prompt(),
            guidelines: vec![
                "Be helpful and honest".into(),
                "Use tools when they would help accomplish the task".into(),
                "Ask for clarification when the request is ambiguous".into(),
                "Respect workspace boundaries and security constraints".into(),
            ],
            loaded_files: vec![],
        }
    }

    /// The fallback system prompt when no context files exist.
    fn fallback_system_prompt() -> String {
        concat!(
            "You are RustedClaw, a helpful AI assistant. ",
            "You have access to tools that let you interact with the user's system. ",
            "Use them when appropriate to help the user accomplish their goals. ",
            "Be concise, accurate, and proactive.",
        ).into()
    }

    /// Load identity from context files following the layered hierarchy.
    ///
    /// This is the main entry point for context loading. It reads markdown
    /// files from global and project directories, assembles them into a
    /// system prompt, and returns a fully-populated Identity.
    pub fn load(paths: &ContextPaths) -> Self {
        // If there's a system prompt override, use it directly
        if let Some(override_prompt) = &paths.system_prompt_override {
            debug!("Using system prompt override, skipping file loading");
            return Self {
                system_prompt: override_prompt.clone(),
                loaded_files: vec!["<override>".into()],
                ..Self::default_identity()
            };
        }

        let mut sections: Vec<ContextSection> = Vec::new();
        let mut loaded_files: Vec<String> = Vec::new();

        // Layer 1: Global identity files from ~/.rustedclaw/workspace/
        if let Some(global_dir) = &paths.global_dir {
            Self::try_load_section(
                global_dir, IDENTITY_FILE, "Identity",
                &mut sections, &mut loaded_files,
            );
            Self::try_load_section(
                global_dir, SOUL_FILE, "Personality & Tone",
                &mut sections, &mut loaded_files,
            );
            Self::try_load_section(
                global_dir, USER_FILE, "User Context",
                &mut sections, &mut loaded_files,
            );
        }

        // Layer 2: Project-local context from ./.rustedclaw/
        if let Some(project_dir) = &paths.project_dir {
            Self::try_load_section(
                project_dir, AGENTS_FILE, "Project Agent Instructions",
                &mut sections, &mut loaded_files,
            );
            Self::try_load_section(
                project_dir, RULES_FILE, "Project Rules & Constraints",
                &mut sections, &mut loaded_files,
            );

            // Also check for any .md files in .rustedclaw/context/ subdirectory
            let context_subdir = project_dir.join("context");
            if context_subdir.is_dir() {
                Self::load_context_directory(
                    &context_subdir, &mut sections, &mut loaded_files,
                );
            }
        }

        // Layer 3: Extra context files from config
        for extra_path in &paths.extra_files {
            if let Some(content) = Self::read_file_safe(extra_path) {
                let filename = extra_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("extra");
                let heading = format!("Additional Context ({})", filename);
                loaded_files.push(extra_path.display().to_string());
                sections.push(ContextSection {
                    source: extra_path.display().to_string(),
                    heading,
                    content,
                });
            }
        }

        // Assemble the final system prompt
        if sections.is_empty() {
            debug!("No context files found, using fallback system prompt");
            return Self::default_identity();
        }

        let system_prompt = Self::assemble_prompt(&sections);

        // Try to extract the agent name from IDENTITY.md content
        let name = sections
            .iter()
            .find(|s| s.source.contains(IDENTITY_FILE))
            .and_then(|s| Self::extract_name(&s.content))
            .unwrap_or_else(|| "RustedClaw".into());

        debug!(
            files_loaded = loaded_files.len(),
            prompt_len = system_prompt.len(),
            "Context loaded successfully"
        );

        Self {
            format: IdentityFormat::RustedClaw,
            name,
            personality: sections
                .iter()
                .find(|s| s.source.contains(SOUL_FILE))
                .map(|s| s.content.clone())
                .unwrap_or_else(|| "A helpful, capable AI assistant.".into()),
            system_prompt,
            guidelines: vec![],
            loaded_files,
        }
    }

    /// Try to load a single context file from a directory.
    fn try_load_section(
        dir: &Path,
        filename: &str,
        heading: &str,
        sections: &mut Vec<ContextSection>,
        loaded_files: &mut Vec<String>,
    ) {
        let path = dir.join(filename);
        if let Some(content) = Self::read_file_safe(&path) {
            if !content.trim().is_empty() {
                debug!(file = %path.display(), "Loaded context file");
                loaded_files.push(path.display().to_string());
                sections.push(ContextSection {
                    source: path.display().to_string(),
                    heading: heading.to_string(),
                    content,
                });
            }
        }
    }

    /// Load all .md files from a context directory (sorted alphabetically).
    fn load_context_directory(
        dir: &Path,
        sections: &mut Vec<ContextSection>,
        loaded_files: &mut Vec<String>,
    ) {
        let mut entries: Vec<PathBuf> = match std::fs::read_dir(dir) {
            Ok(rd) => rd
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| {
                    p.extension()
                        .and_then(|ext| ext.to_str())
                        .is_some_and(|ext| ext == "md" || ext == "txt")
                })
                .collect(),
            Err(e) => {
                warn!(dir = %dir.display(), error = %e, "Failed to read context directory");
                return;
            }
        };

        // Sort for deterministic ordering
        entries.sort();

        for path in entries {
            if let Some(content) = Self::read_file_safe(&path) {
                if !content.trim().is_empty() {
                    let filename = path
                        .file_stem()
                        .and_then(|n| n.to_str())
                        .unwrap_or("context");
                    let heading = format!("Context: {}", filename);
                    debug!(file = %path.display(), "Loaded extra context file");
                    loaded_files.push(path.display().to_string());
                    sections.push(ContextSection {
                        source: path.display().to_string(),
                        heading,
                        content,
                    });
                }
            }
        }
    }

    /// Safely read a file, returning None on any error.
    fn read_file_safe(path: &Path) -> Option<String> {
        match std::fs::read_to_string(path) {
            Ok(content) => Some(content),
            Err(_) => None,
        }
    }

    /// Assemble a system prompt from loaded sections.
    ///
    /// Format:
    /// ```text
    /// <identity>
    /// {IDENTITY.md content}
    /// </identity>
    ///
    /// <personality>
    /// {SOUL.md content}
    /// </personality>
    /// ...
    /// ```
    fn assemble_prompt(sections: &[ContextSection]) -> String {
        let mut prompt = String::with_capacity(4096);

        for (i, section) in sections.iter().enumerate() {
            if i > 0 {
                prompt.push('\n');
            }

            // Use XML-style tags for clear section delineation (LLM-friendly)
            let tag = section
                .heading
                .to_lowercase()
                .replace(' ', "_")
                .replace(':', "")
                .replace('(', "")
                .replace(')', "");

            prompt.push_str(&format!("<{}>\n", tag));
            prompt.push_str(section.content.trim());
            prompt.push_str(&format!("\n</{}>\n", tag));
        }

        // Append tool usage instructions
        prompt.push_str("\n<capabilities>\n");
        prompt.push_str("You have access to tools that let you interact with the user's system. ");
        prompt.push_str("Use them when appropriate to help accomplish tasks. ");
        prompt.push_str("Be concise, accurate, and proactive.\n");
        prompt.push_str("</capabilities>\n");

        prompt
    }

    /// Try to extract the agent's name from IDENTITY.md content.
    ///
    /// Looks for patterns like:
    /// - "# Identity\n\nYou are AgentName"
    /// - "name: AgentName"
    /// - "# AgentName"
    fn extract_name(content: &str) -> Option<String> {
        // Pattern 1: "You are <Name>"
        if let Some(pos) = content.find("You are ") {
            let rest = &content[pos + 8..];
            let name: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == ' ' || *c == '-' || *c == '_')
                .collect();
            let name = name.trim().to_string();
            if !name.is_empty() && name.len() < 50 {
                return Some(name);
            }
        }

        // Pattern 2: First H1 heading
        for line in content.lines() {
            let trimmed = line.trim();
            if let Some(heading) = trimmed.strip_prefix("# ") {
                let heading = heading.trim();
                if heading != "Identity" && !heading.is_empty() {
                    return Some(heading.to_string());
                }
            }
        }

        None
    }

    /// Estimate the token count of the system prompt (rough: 4 chars ≈ 1 token).
    pub fn estimated_tokens(&self) -> usize {
        self.system_prompt.len() / 4
    }

    /// Get a diagnostic summary of loaded context.
    pub fn diagnostic_summary(&self) -> String {
        let mut summary = String::new();
        summary.push_str(&format!("Agent Name: {}\n", self.name));
        summary.push_str(&format!("Format: {:?}\n", self.format));
        summary.push_str(&format!(
            "System Prompt: {} chars (~{} tokens)\n",
            self.system_prompt.len(),
            self.estimated_tokens()
        ));
        summary.push_str(&format!("Files Loaded: {}\n", self.loaded_files.len()));
        for f in &self.loaded_files {
            summary.push_str(&format!("  - {f}\n"));
        }
        summary
    }
}

impl Default for Identity {
    fn default() -> Self {
        Self::default_identity()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn default_identity_has_name() {
        let id = Identity::default();
        assert_eq!(id.name, "RustedClaw");
        assert!(!id.system_prompt.is_empty());
    }

    #[test]
    fn load_with_no_paths_returns_default() {
        let paths = ContextPaths::default();
        let id = Identity::load(&paths);
        assert_eq!(id.name, "RustedClaw");
        assert!(id.loaded_files.is_empty());
    }

    #[test]
    fn load_with_override_skips_files() {
        let paths = ContextPaths {
            system_prompt_override: Some("Custom prompt".into()),
            ..Default::default()
        };
        let id = Identity::load(&paths);
        assert_eq!(id.system_prompt, "Custom prompt");
        assert_eq!(id.loaded_files, vec!["<override>"]);
    }

    #[test]
    fn load_from_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();

        // Create test context files
        fs::write(dir.join("IDENTITY.md"), "# MyAgent\n\nYou are MyAgent, a coding assistant.").unwrap();
        fs::write(dir.join("SOUL.md"), "Be friendly and thorough.").unwrap();

        let paths = ContextPaths {
            global_dir: Some(dir.to_path_buf()),
            ..Default::default()
        };
        let id = Identity::load(&paths);

        assert_eq!(id.name, "MyAgent");
        assert_eq!(id.loaded_files.len(), 2);
        assert!(id.system_prompt.contains("MyAgent"));
        assert!(id.system_prompt.contains("friendly"));
        assert!(id.system_prompt.contains("<identity>"));
        assert!(id.system_prompt.contains("</identity>"));
    }

    #[test]
    fn load_project_context_layered() {
        let tmp_global = tempfile::tempdir().unwrap();
        let tmp_project = tempfile::tempdir().unwrap();

        fs::write(
            tmp_global.path().join("IDENTITY.md"),
            "You are TestBot, a general assistant.",
        ).unwrap();

        fs::write(
            tmp_project.path().join("AGENTS.md"),
            "This is a Rust project. Prefer idiomatic Rust patterns.",
        ).unwrap();

        fs::write(
            tmp_project.path().join("RULES.md"),
            "Never use unwrap() in production code.\nAlways handle errors with Result.",
        ).unwrap();

        let paths = ContextPaths {
            global_dir: Some(tmp_global.path().to_path_buf()),
            project_dir: Some(tmp_project.path().to_path_buf()),
            ..Default::default()
        };
        let id = Identity::load(&paths);

        assert_eq!(id.loaded_files.len(), 3); // IDENTITY + AGENTS + RULES
        assert!(id.system_prompt.contains("TestBot"));
        assert!(id.system_prompt.contains("Rust project"));
        assert!(id.system_prompt.contains("unwrap()"));
    }

    #[test]
    fn load_extra_context_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let context_dir = tmp.path().join("context");
        fs::create_dir_all(&context_dir).unwrap();

        fs::write(context_dir.join("01-api-docs.md"), "API endpoint: /v1/chat").unwrap();
        fs::write(context_dir.join("02-schema.md"), "User table has id, name, email").unwrap();
        fs::write(context_dir.join("readme.txt"), "This is a text file context").unwrap();
        fs::write(context_dir.join("ignore.json"), "this should be ignored").unwrap();

        let paths = ContextPaths {
            project_dir: Some(tmp.path().to_path_buf()),
            ..Default::default()
        };
        let id = Identity::load(&paths);

        // Should load .md and .txt files, skip .json
        assert_eq!(id.loaded_files.len(), 3);
        assert!(id.system_prompt.contains("API endpoint"));
        assert!(id.system_prompt.contains("User table"));
        assert!(id.system_prompt.contains("text file context"));
        assert!(!id.system_prompt.contains("should be ignored"));
    }

    #[test]
    fn extract_name_patterns() {
        assert_eq!(
            Identity::extract_name("# Identity\n\nYou are Jarvis, an AI butler."),
            Some("Jarvis".into())
        );
        assert_eq!(
            Identity::extract_name("# CodingBot\n\nI help with code."),
            Some("CodingBot".into())
        );
        assert_eq!(
            Identity::extract_name("Just some text without a name pattern"),
            None
        );
    }

    #[test]
    fn estimated_tokens_reasonable() {
        let id = Identity::default();
        let tokens = id.estimated_tokens();
        assert!(tokens > 10);
        assert!(tokens < 500);
    }

    #[test]
    fn missing_files_silently_skipped() {
        let paths = ContextPaths {
            global_dir: Some(PathBuf::from("/nonexistent/path/that/doesnt/exist")),
            ..Default::default()
        };
        let id = Identity::load(&paths);
        // Should fall back to defaults without panicking
        assert!(id.loaded_files.is_empty());
    }
}
