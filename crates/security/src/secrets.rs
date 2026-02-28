//! Secrets encryption using AES-256-GCM with SHA-256-based key derivation.
//!
//! Provides authenticated encryption for API keys and credentials at rest.
//! Uses AES-256-GCM for confidentiality + integrity, with a SHA-256 based
//! key derivation (iterated hashing) from a user passphrase.

use aes_gcm::{Aes256Gcm, KeyInit, Nonce, aead::Aead};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// An encrypted value with its nonce.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EncryptedValue {
    /// 12-byte random nonce used for AES-GCM encryption
    pub nonce: Vec<u8>,
    /// The AES-256-GCM ciphertext (includes 16-byte auth tag)
    pub ciphertext: Vec<u8>,
}

/// Manages encryption/decryption of secrets using AES-256-GCM.
pub struct SecretsManager {
    key: [u8; 32],
}

impl SecretsManager {
    /// Create a new SecretsManager from a password/passphrase.
    ///
    /// Derives a 32-byte key using iterated SHA-256 hashing (100,000 rounds).
    /// Rejects empty passwords to prevent weak keys.
    pub fn new(password: &str) -> Self {
        assert!(
            !password.is_empty(),
            "SecretsManager password must not be empty"
        );
        let key = derive_key(password);
        Self { key }
    }

    /// Create a SecretsManager from raw 32-byte key material.
    ///
    /// Panics if key is not exactly 32 bytes.
    pub fn from_key(key: Vec<u8>) -> Self {
        assert_eq!(key.len(), 32, "Key must be exactly 32 bytes for AES-256");
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&key);
        Self { key: arr }
    }

    /// Encrypt a plaintext string using AES-256-GCM.
    ///
    /// Each call generates a fresh random 12-byte nonce, ensuring that
    /// encrypting the same plaintext twice produces different ciphertexts.
    /// The ciphertext includes a 16-byte authentication tag.
    pub fn encrypt(&self, plaintext: &str) -> EncryptedValue {
        let cipher = Aes256Gcm::new_from_slice(&self.key)
            .expect("AES-256-GCM key init should not fail with 32-byte key");
        let nonce_bytes = generate_nonce();
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = cipher
            .encrypt(nonce, plaintext.as_bytes())
            .expect("AES-256-GCM encryption should not fail");
        EncryptedValue {
            nonce: nonce_bytes.to_vec(),
            ciphertext,
        }
    }

    /// Decrypt an encrypted value back to plaintext using AES-256-GCM.
    ///
    /// Returns an error if the key is wrong or the ciphertext was tampered with
    /// (authenticated encryption detects modification).
    pub fn decrypt(&self, encrypted: &EncryptedValue) -> Result<String, SecretError> {
        if encrypted.nonce.len() != 12 {
            return Err(SecretError::DecryptionFailed(format!(
                "Invalid nonce length: expected 12, got {}",
                encrypted.nonce.len()
            )));
        }
        let cipher = Aes256Gcm::new_from_slice(&self.key)
            .map_err(|e| SecretError::DecryptionFailed(format!("Key init failed: {e}")))?;
        let nonce = Nonce::from_slice(&encrypted.nonce);
        let plaintext_bytes = cipher
            .decrypt(nonce, encrypted.ciphertext.as_ref())
            .map_err(|_| {
                SecretError::DecryptionFailed(
                    "Decryption failed — wrong key or corrupted ciphertext".into(),
                )
            })?;
        String::from_utf8(plaintext_bytes)
            .map_err(|_| SecretError::DecryptionFailed("Invalid UTF-8 after decryption".into()))
    }

    /// Check if an output string contains any of the known secrets (leakage detection).
    pub fn scan_for_leakage(output: &str, secrets: &[String]) -> bool {
        secrets.iter().any(|s| !s.is_empty() && output.contains(s))
    }
}

/// Errors from secrets operations.
#[derive(Debug, thiserror::Error)]
pub enum SecretError {
    #[error("Decryption failed: {0}")]
    DecryptionFailed(String),

    #[error("Key derivation failed: {0}")]
    KeyDerivationFailed(String),
}

/// Derive a 32-byte AES key from a password using iterated SHA-256.
///
/// Performs 100,000 rounds of SHA-256 hashing to slow down brute-force attacks.
/// A unique salt is mixed in to prevent rainbow table attacks.
fn derive_key(password: &str) -> [u8; 32] {
    let salt = b"rustedclaw-secrets-v1-salt";
    let mut hash = Sha256::new();
    hash.update(salt);
    hash.update(password.as_bytes());
    let mut result = hash.finalize();

    // Iterated hashing — 100k rounds for brute-force resistance
    for _ in 0..100_000 {
        let mut h = Sha256::new();
        h.update(result);
        h.update(password.as_bytes());
        result = h.finalize();
    }

    let mut key = [0u8; 32];
    key.copy_from_slice(&result);
    key
}

/// Generate a cryptographically random 12-byte nonce for AES-GCM.
fn generate_nonce() -> [u8; 12] {
    use rand::Rng;
    let mut rng = rand::rng();
    let mut nonce = [0u8; 12];
    rng.fill(&mut nonce);
    nonce
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let manager = SecretsManager::new("my-secure-password-123");
        let plaintext = "sk-1234567890abcdef";

        let encrypted = manager.encrypt(plaintext);
        assert_ne!(encrypted.ciphertext, plaintext.as_bytes());
        assert!(!encrypted.nonce.is_empty());

        let decrypted = manager.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn different_nonces_produce_different_ciphertext() {
        let manager = SecretsManager::new("password");
        let plaintext = "test-secret";

        let enc1 = manager.encrypt(plaintext);
        let enc2 = manager.encrypt(plaintext);

        // Different nonces should produce different ciphertext
        assert_ne!(enc1.nonce, enc2.nonce);
        assert_ne!(enc1.ciphertext, enc2.ciphertext);

        // But both decrypt to same value
        assert_eq!(manager.decrypt(&enc1).unwrap(), plaintext);
        assert_eq!(manager.decrypt(&enc2).unwrap(), plaintext);
    }

    #[test]
    fn wrong_password_fails_to_decrypt() {
        let manager1 = SecretsManager::new("correct-password");
        let manager2 = SecretsManager::new("wrong-password");

        let encrypted = manager1.encrypt("my-api-key");
        let result = manager2.decrypt(&encrypted);

        // AES-GCM authenticated encryption: wrong key always returns Err
        assert!(result.is_err(), "Wrong key must fail with AES-GCM");
    }

    #[test]
    fn encrypt_empty_string() {
        let manager = SecretsManager::new("password");
        let encrypted = manager.encrypt("");
        let decrypted = manager.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, "");
    }

    #[test]
    fn encrypt_long_string() {
        let manager = SecretsManager::new("password");
        let plaintext = "a".repeat(1000);
        let encrypted = manager.encrypt(&plaintext);
        let decrypted = manager.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn serialization_roundtrip() {
        let manager = SecretsManager::new("password");
        let encrypted = manager.encrypt("secret-value");

        let json = serde_json::to_string(&encrypted).unwrap();
        let deserialized: EncryptedValue = serde_json::from_str(&json).unwrap();

        assert_eq!(encrypted, deserialized);
        assert_eq!(manager.decrypt(&deserialized).unwrap(), "secret-value");
    }

    #[test]
    fn leakage_detection_finds_secret() {
        let secrets = vec!["sk-abc123".to_string(), "token-xyz".to_string()];

        assert!(SecretsManager::scan_for_leakage(
            "The API key is sk-abc123 and it works",
            &secrets
        ));

        assert!(SecretsManager::scan_for_leakage(
            "Using token-xyz to authenticate",
            &secrets
        ));
    }

    #[test]
    fn leakage_detection_no_false_positives() {
        let secrets = vec!["sk-abc123".to_string()];

        assert!(!SecretsManager::scan_for_leakage(
            "No secrets here, just normal output",
            &secrets
        ));
    }

    #[test]
    fn leakage_detection_empty_secrets() {
        assert!(!SecretsManager::scan_for_leakage("anything", &[]));
        assert!(!SecretsManager::scan_for_leakage(
            "anything",
            &["".to_string()]
        ));
    }

    #[test]
    fn different_keys_produce_different_derivations() {
        let key1 = derive_key("password1");
        let key2 = derive_key("password2");
        assert_ne!(key1, key2);
        assert_eq!(key1.len(), 32);
        assert_eq!(key2.len(), 32);
    }
}
