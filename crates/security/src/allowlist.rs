//! Allowlist policies — sender validation and endpoint allowlisting.
//!
//! Enforces access control for channels (which senders are allowed)
//! and for HTTP tools (which endpoints can be accessed).

use rustedclaw_config::ChannelConfig;

/// Result of checking a sender against the allowlist.
#[derive(Debug, Clone, PartialEq)]
pub enum SenderCheckResult {
    /// Sender is allowed
    Allowed,
    /// Sender is denied
    Denied { sender_id: String, reason: String },
    /// No allowlist configured (deny by default)
    NoConfig,
}

/// Unified allowlist policy enforcement.
pub struct AllowlistPolicy;

impl AllowlistPolicy {
    /// Check if a sender is allowed for a given channel configuration.
    ///
    /// Rules:
    /// - If `allowed_users` is empty → deny all (secure by default)
    /// - If `allowed_users` contains `"*"` → allow all
    /// - Otherwise, sender must be in the list
    pub fn check_sender(config: &ChannelConfig, sender_id: &str) -> SenderCheckResult {
        if !config.enabled {
            return SenderCheckResult::Denied {
                sender_id: sender_id.into(),
                reason: "Channel is disabled".into(),
            };
        }

        if config.allowed_users.is_empty() {
            return SenderCheckResult::Denied {
                sender_id: sender_id.into(),
                reason: "No users configured (deny by default)".into(),
            };
        }

        if config.allowed_users.iter().any(|u| u == "*") {
            return SenderCheckResult::Allowed;
        }

        if config.allowed_users.iter().any(|u| u == sender_id) {
            SenderCheckResult::Allowed
        } else {
            SenderCheckResult::Denied {
                sender_id: sender_id.into(),
                reason: format!(
                    "Sender '{}' not in allowlist ({} users configured)",
                    sender_id,
                    config.allowed_users.len()
                ),
            }
        }
    }

    /// Check if a URL is allowed by the endpoint allowlist.
    ///
    /// Rules:
    /// - If `allowed_endpoints` is empty → allow all (open by default for HTTP tool)
    /// - If `allowed_endpoints` contains `"*"` → allow all
    /// - Otherwise, URL must start with one of the allowed prefixes
    pub fn check_endpoint(url: &str, allowed_endpoints: &[String]) -> SenderCheckResult {
        if allowed_endpoints.is_empty() {
            return SenderCheckResult::Allowed;
        }

        if allowed_endpoints.iter().any(|e| e == "*") {
            return SenderCheckResult::Allowed;
        }

        // Block private/internal IPs (SSRF prevention)
        if is_private_url(url) {
            return SenderCheckResult::Denied {
                sender_id: url.into(),
                reason: "Request to private/internal IP blocked (SSRF prevention)".into(),
            };
        }

        if allowed_endpoints.iter().any(|e| url.starts_with(e)) {
            SenderCheckResult::Allowed
        } else {
            SenderCheckResult::Denied {
                sender_id: url.into(),
                reason: format!(
                    "URL '{}' not in allowed endpoints ({} configured)",
                    url,
                    allowed_endpoints.len()
                ),
            }
        }
    }
}

/// Check if a URL targets a private/internal IP address.
fn is_private_url(url: &str) -> bool {
    let lower = url.to_lowercase();

    // Extract host from URL
    let host = lower
        .strip_prefix("http://")
        .or_else(|| lower.strip_prefix("https://"))
        .unwrap_or(&lower);

    let host = host.split('/').next().unwrap_or(host);
    let host = host.split(':').next().unwrap_or(host);

    // Check for private IP ranges and localhost
    host == "localhost"
        || host == "127.0.0.1"
        || host.starts_with("10.")
        || host.starts_with("192.168.")
        || host.starts_with("172.16.")
        || host.starts_with("172.17.")
        || host.starts_with("172.18.")
        || host.starts_with("172.19.")
        || host.starts_with("172.2")
        || host.starts_with("172.30.")
        || host.starts_with("172.31.")
        || host == "169.254.169.254" // AWS metadata
        || host == "[::1]"
        || host == "0.0.0.0"
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn make_config(enabled: bool, users: Vec<&str>) -> ChannelConfig {
        ChannelConfig {
            enabled,
            allowed_users: users.into_iter().map(String::from).collect(),
            settings: HashMap::new(),
        }
    }

    #[test]
    fn empty_allowlist_denies_all() {
        let config = make_config(true, vec![]);
        let result = AllowlistPolicy::check_sender(&config, "user123");
        assert_eq!(
            result,
            SenderCheckResult::Denied {
                sender_id: "user123".into(),
                reason: "No users configured (deny by default)".into(),
            }
        );
    }

    #[test]
    fn wildcard_allows_all() {
        let config = make_config(true, vec!["*"]);
        assert_eq!(
            AllowlistPolicy::check_sender(&config, "anyone"),
            SenderCheckResult::Allowed
        );
    }

    #[test]
    fn specific_user_allowed() {
        let config = make_config(true, vec!["user123", "user456"]);
        assert_eq!(
            AllowlistPolicy::check_sender(&config, "user123"),
            SenderCheckResult::Allowed
        );
    }

    #[test]
    fn unknown_user_denied() {
        let config = make_config(true, vec!["user123"]);
        let result = AllowlistPolicy::check_sender(&config, "hacker");
        match result {
            SenderCheckResult::Denied { sender_id, .. } => {
                assert_eq!(sender_id, "hacker");
            }
            _ => panic!("Expected denied"),
        }
    }

    #[test]
    fn disabled_channel_denies() {
        let config = make_config(false, vec!["*"]);
        let result = AllowlistPolicy::check_sender(&config, "user123");
        match result {
            SenderCheckResult::Denied { reason, .. } => {
                assert!(reason.contains("disabled"));
            }
            _ => panic!("Expected denied"),
        }
    }

    #[test]
    fn empty_endpoints_allows_all() {
        assert_eq!(
            AllowlistPolicy::check_endpoint("https://example.com/api", &[]),
            SenderCheckResult::Allowed
        );
    }

    #[test]
    fn wildcard_endpoints_allows_all() {
        let allowed = vec!["*".into()];
        assert_eq!(
            AllowlistPolicy::check_endpoint("https://example.com", &allowed),
            SenderCheckResult::Allowed
        );
    }

    #[test]
    fn matching_endpoint_allowed() {
        let allowed = vec!["https://api.example.com".into(), "https://myapp.com".into()];
        assert_eq!(
            AllowlistPolicy::check_endpoint("https://api.example.com/v1/data", &allowed),
            SenderCheckResult::Allowed
        );
    }

    #[test]
    fn non_matching_endpoint_denied() {
        let allowed = vec!["https://api.example.com".into()];
        let result = AllowlistPolicy::check_endpoint("https://evil.com/steal", &allowed);
        match result {
            SenderCheckResult::Denied { .. } => {}
            _ => panic!("Expected denied"),
        }
    }

    #[test]
    fn ssrf_localhost_blocked() {
        let _allowed: Vec<String> = vec!["*".into()]; // Even with wildcard
        // But with endpoint list, private IPs should be blocked
        let specific = vec!["https://api.example.com".into()];
        let result = AllowlistPolicy::check_endpoint("http://127.0.0.1:8080/admin", &specific);
        match result {
            SenderCheckResult::Denied { reason, .. } => {
                assert!(reason.contains("SSRF"));
            }
            _ => panic!("Expected SSRF block"),
        }
    }

    #[test]
    fn ssrf_metadata_blocked() {
        let specific = vec!["https://api.example.com".into()];
        let result =
            AllowlistPolicy::check_endpoint("http://169.254.169.254/latest/meta-data/", &specific);
        match result {
            SenderCheckResult::Denied { .. } => {}
            _ => panic!("Expected SSRF block"),
        }
    }

    #[test]
    fn private_url_detection() {
        assert!(is_private_url("http://localhost:3000"));
        assert!(is_private_url("http://127.0.0.1/api"));
        assert!(is_private_url("http://192.168.1.1/admin"));
        assert!(is_private_url("http://10.0.0.1/internal"));
        assert!(is_private_url("http://169.254.169.254/meta"));
        assert!(!is_private_url("https://api.example.com/v1"));
        assert!(!is_private_url("https://google.com"));
    }
}
