//! Path validation — filesystem sandboxing to workspace directory.
//!
//! Ensures file tools can only access paths within allowed roots and
//! blocks access to forbidden paths (e.g., ~/.ssh, /etc).

use std::path::{Path, PathBuf};

/// Error returned when path validation fails.
#[derive(Debug, thiserror::Error)]
pub enum PathValidationError {
    #[error("Path '{path}' is outside allowed roots")]
    OutsideAllowedRoots { path: String },

    #[error("Path '{path}' matches forbidden pattern '{pattern}'")]
    ForbiddenPath { path: String, pattern: String },

    #[error("Path traversal detected in '{path}'")]
    PathTraversal { path: String },

    #[error("Failed to canonicalize path '{path}': {reason}")]
    CanonicalizeFailed { path: String, reason: String },
}

/// Validate that a path is safe to access.
///
/// Checks:
/// 1. No path traversal attacks (`..\..` sequences)
/// 2. Path is canonicalized to resolve symlinks and relative components
/// 3. Path is within allowed roots (if specified)
/// 4. Path is not in forbidden paths list
///
/// Returns the canonicalized (resolved) path on success.
pub fn validate_path(
    path: &str,
    allowed_roots: &[String],
    forbidden_paths: &[String],
) -> Result<PathBuf, PathValidationError> {
    let input_path = Path::new(path);

    // Check for obvious path traversal attempts in the raw string
    let path_str = path.replace('\\', "/");
    if path_str.contains("../") || path_str.contains("/..") || path_str == ".." {
        return Err(PathValidationError::PathTraversal { path: path.into() });
    }

    // Attempt to canonicalize the path to resolve symlinks, `.`, `..`, etc.
    // If the file doesn't exist yet (e.g., for writes), canonicalize the parent.
    let canonical = if input_path.exists() {
        input_path
            .canonicalize()
            .map_err(|e| PathValidationError::CanonicalizeFailed {
                path: path.into(),
                reason: e.to_string(),
            })?
    } else if let Some(parent) = input_path.parent()
        && parent.exists()
    {
        let canonical_parent =
            parent
                .canonicalize()
                .map_err(|e| PathValidationError::CanonicalizeFailed {
                    path: path.into(),
                    reason: format!("Parent dir: {e}"),
                })?;
        canonical_parent.join(input_path.file_name().unwrap_or_default())
    } else {
        // Can't canonicalize — fall back to the raw path but normalize it
        input_path.to_path_buf()
    };

    let canonical_str = canonical
        .to_string_lossy()
        .replace('\\', "/")
        .to_lowercase();

    // Strip the Windows extended-length path prefix (\\?\) that canonicalize() adds.
    // \\?\ becomes //?/ after backslash replacement
    let canonical_str = canonical_str
        .strip_prefix("//?/")
        .unwrap_or(&canonical_str)
        .to_string();

    // Check against forbidden paths (using canonical path)
    for forbidden in forbidden_paths {
        let expanded = expand_tilde(forbidden);
        let forbidden_normalized = expanded.replace('\\', "/").to_lowercase();

        if canonical_str.starts_with(&forbidden_normalized) {
            return Err(PathValidationError::ForbiddenPath {
                path: path.into(),
                pattern: forbidden.clone(),
            });
        }
    }

    // Check allowed roots (if any are configured) using canonical path
    if !allowed_roots.is_empty() {
        let is_allowed = allowed_roots.iter().any(|root| {
            let expanded = expand_tilde(root);
            let root_normalized = expanded.replace('\\', "/").to_lowercase();
            canonical_str.starts_with(&root_normalized)
        });

        if !is_allowed {
            return Err(PathValidationError::OutsideAllowedRoots { path: path.into() });
        }
    }

    Ok(canonical)
}

/// Expand ~ to the user's home directory.
fn expand_tilde(path: &str) -> String {
    if (path.starts_with("~/") || path == "~")
        && let Ok(home) = home_dir()
    {
        return path.replacen('~', &home, 1);
    }
    path.to_string()
}

fn home_dir() -> Result<String, ()> {
    #[cfg(target_os = "windows")]
    {
        std::env::var("USERPROFILE").map_err(|_| ())
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::env::var("HOME").map_err(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_path_no_restrictions() {
        let result = validate_path("/home/user/project/file.txt", &[], &[]);
        assert!(result.is_ok());
    }

    #[test]
    fn path_traversal_blocked() {
        let result = validate_path("../../../etc/passwd", &[], &[]);
        assert!(result.is_err());
        match result.unwrap_err() {
            PathValidationError::PathTraversal { .. } => {}
            other => panic!("Expected PathTraversal, got: {other}"),
        }
    }

    #[test]
    fn path_traversal_mid_path_blocked() {
        let result = validate_path("/home/user/../../../etc/passwd", &[], &[]);
        assert!(result.is_err());
    }

    #[test]
    fn forbidden_path_blocked() {
        let forbidden = vec!["/etc".into(), "/root".into()];
        let result = validate_path("/etc/passwd", &[], &forbidden);
        assert!(result.is_err());
        match result.unwrap_err() {
            PathValidationError::ForbiddenPath { pattern, .. } => {
                assert_eq!(pattern, "/etc");
            }
            other => panic!("Expected ForbiddenPath, got: {other}"),
        }
    }

    #[test]
    fn allowed_roots_enforced() {
        let allowed = vec!["/home/user/workspace".into()];
        let result = validate_path("/home/user/workspace/src/main.rs", &allowed, &[]);
        assert!(result.is_ok());

        let result = validate_path("/home/other/secret.txt", &allowed, &[]);
        assert!(result.is_err());
        match result.unwrap_err() {
            PathValidationError::OutsideAllowedRoots { .. } => {}
            other => panic!("Expected OutsideAllowedRoots, got: {other}"),
        }
    }

    #[test]
    fn empty_allowed_roots_allows_all() {
        let result = validate_path("/any/path/file.txt", &[], &[]);
        assert!(result.is_ok());
    }

    #[test]
    fn forbidden_with_tilde_expansion() {
        // This test verifies the tilde expansion logic works
        let forbidden = vec!["~/.ssh".into(), "~/.gnupg".into()];
        // If HOME is set, ~/. paths should be expanded and checked
        if home_dir().is_ok() {
            let home = home_dir().unwrap();
            let ssh_path = format!("{home}/.ssh/id_rsa");
            let result = validate_path(&ssh_path, &[], &forbidden);
            assert!(result.is_err());
        }
    }

    #[test]
    fn case_insensitive_on_windows_paths() {
        let forbidden = vec!["/etc".into()];
        let result = validate_path("/ETC/passwd", &[], &forbidden);
        // Should still be blocked (case-insensitive comparison)
        assert!(result.is_err());
    }

    #[test]
    fn multiple_roots_any_match_allowed() {
        let allowed = vec!["/home/user/project1".into(), "/home/user/project2".into()];
        assert!(validate_path("/home/user/project1/file.rs", &allowed, &[]).is_ok());
        assert!(validate_path("/home/user/project2/file.rs", &allowed, &[]).is_ok());
        assert!(validate_path("/home/user/project3/file.rs", &allowed, &[]).is_err());
    }

    #[test]
    fn forbidden_takes_precedence_over_allowed() {
        let allowed = vec!["/home/user".into()];
        let forbidden = vec!["/home/user/.ssh".into()];
        let result = validate_path("/home/user/.ssh/id_rsa", &allowed, &forbidden);
        assert!(result.is_err());
    }
}
