//! Security module for RustedClaw — encryption, allowlists, path validation, and audit logging.
//!
//! Provides:
//! - **Secrets**: Encrypt/decrypt API keys and credentials at rest
//! - **Allowlists**: Sender validation per channel, endpoint allowlisting
//! - **Path validation**: Filesystem sandboxing to workspace directory
//! - **Audit logging**: Structured security event logging

pub mod allowlist;
pub mod audit;
pub mod path;
pub mod secrets;

pub use allowlist::{AllowlistPolicy, SenderCheckResult};
pub use audit::{AuditEntry, AuditEvent, AuditLogger, AuditOutcome, AuditSink, TracingSink};
pub use path::{validate_path, PathValidationError};
pub use secrets::{EncryptedValue, SecretsManager};
