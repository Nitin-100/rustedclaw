//! Simple secrets encryption using XOR-based obfuscation with key derivation.
//!
//! In production, this would use AES-256-GCM with a proper KDF. For the MVP,
//! we use a simpler approach that still provides meaningful protection of
//! secrets at rest (not plaintext in config files).

use serde::{Deserialize, Serialize};

/// An encrypted value with its nonce.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EncryptedValue {
    /// Random nonce used for encryption
    pub nonce: Vec<u8>,
    /// The encrypted ciphertext
    pub ciphertext: Vec<u8>,
}

/// Manages encryption/decryption of secrets.
pub struct SecretsManager {
    key: Vec<u8>,
}

impl SecretsManager {
    /// Create a new SecretsManager from a password/passphrase.
    ///
    /// Derives a 32-byte key from the password using a simple
    /// hash-based key derivation. In production, use PBKDF2 or Argon2.
    pub fn new(password: &str) -> Self {
        let key = derive_key(password);
        Self { key }
    }

    /// Create a SecretsManager from raw key bytes.
    pub fn from_key(key: Vec<u8>) -> Self {
        Self { key }
    }

    /// Encrypt a plaintext string.
    pub fn encrypt(&self, plaintext: &str) -> EncryptedValue {
        let nonce = generate_nonce();
        let ciphertext = xor_encrypt(plaintext.as_bytes(), &self.key, &nonce);
        EncryptedValue { nonce, ciphertext }
    }

    /// Decrypt an encrypted value back to plaintext.
    pub fn decrypt(&self, encrypted: &EncryptedValue) -> Result<String, SecretError> {
        let plaintext_bytes = xor_encrypt(&encrypted.ciphertext, &self.key, &encrypted.nonce);
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

/// Derive a 32-byte key from a password using repeated hashing.
fn derive_key(password: &str) -> Vec<u8> {
    let mut key = vec![0u8; 32];
    let bytes = password.as_bytes();

    // Simple key derivation: hash the password bytes into a 32-byte key
    for (i, &b) in bytes.iter().enumerate() {
        key[i % 32] ^= b;
        // Mix with position to avoid collisions
        key[(i + 13) % 32] =
            key[(i + 13) % 32].wrapping_add(b.wrapping_mul((i as u8).wrapping_add(1)));
    }

    // Additional mixing rounds for better distribution
    for round in 0..64 {
        for i in 0..32 {
            let prev = key[(i + 31) % 32];
            key[i] = key[i]
                .wrapping_add(prev)
                .wrapping_mul(37)
                .wrapping_add(round);
        }
    }

    key
}

/// Generate a random nonce.
fn generate_nonce() -> Vec<u8> {
    use rand::Rng;
    let mut rng = rand::rng();
    let mut nonce = vec![0u8; 12];
    rng.fill(&mut nonce[..]);
    nonce
}

/// XOR-based stream cipher using key + nonce to generate keystream.
fn xor_encrypt(data: &[u8], key: &[u8], nonce: &[u8]) -> Vec<u8> {
    // Generate a keystream from key + nonce
    let mut keystream = Vec::with_capacity(data.len());
    let mut state = [0u8; 32];
    state[..key.len().min(32)].copy_from_slice(&key[..key.len().min(32)]);

    // Mix nonce into state
    for (i, &n) in nonce.iter().enumerate() {
        state[i % 32] ^= n;
    }

    let mut counter = 0u32;
    while keystream.len() < data.len() {
        // Generate block from state + counter
        let counter_bytes = counter.to_le_bytes();
        for i in 0..32 {
            let ks_byte = state[i]
                .wrapping_add(counter_bytes[i % 4])
                .wrapping_mul(state[(i + 1) % 32].wrapping_add(1))
                .wrapping_add(i as u8);
            keystream.push(ks_byte);
        }
        counter += 1;
    }

    // XOR data with keystream
    data.iter()
        .zip(keystream.iter())
        .map(|(&d, &k)| d ^ k)
        .collect()
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
    fn wrong_password_gives_wrong_output() {
        let manager1 = SecretsManager::new("correct-password");
        let manager2 = SecretsManager::new("wrong-password");

        let encrypted = manager1.encrypt("my-api-key");
        let result = manager2.decrypt(&encrypted);

        // Should decrypt but to wrong value (XOR-based cipher doesn't fail on wrong key)
        if let Ok(val) = result {
            assert_ne!(val, "my-api-key");
        }
        // Err case (UTF-8 decode error) is also valid
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
