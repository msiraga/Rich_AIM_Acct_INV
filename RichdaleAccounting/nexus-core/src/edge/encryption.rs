//! Edge Encryption
//!
//! AES-256-GCM field-level encryption for sensitive data at rest.
//! Keys derived from user password via HKDF-SHA256.
//!
//! # Security Properties
//!
//! - **Confidentiality**: AES-256-GCM provides authenticated encryption (AEAD).
//! - **Integrity**: The 16-byte GCM authentication tag detects any tampering
//!   of the ciphertext or nonce. A wrong key or a single flipped bit causes
//!   decryption to fail.
//! - **Key separation**: HKDF-SHA256 with a domain-separated info string binds
//!   derived keys to the "field-level encryption" use case, preventing key
//!   reuse across subsystems.
//! - **Nonce uniqueness**: A fresh 96-bit random nonce is generated for every
//!   encryption call. With a random nonce the GCM birthday bound is roughly
//!   2^32 messages per key — far beyond any single-tenant accounting database.
//!
//! # Ciphertext Format
//!
//! All encrypted fields use the following binary layout:
//!
//! ```text
//! +-------------------+--------------------------+------------------+
//! | nonce (12 bytes)  | ciphertext (variable)    | tag (16 bytes)   |
//! +-------------------+--------------------------+------------------+
//! ```
//!
//! The GCM tag is appended to the ciphertext by the `aes-gcm` crate. The
//! complete blob is self-contained: the nonce is prepended so that the
//! decryptor does not need to store or track nonces separately.
//!
//! # Usage
//!
//! ```text
//! let salt = FieldEncryptor::generate_salt();          // store alongside data
//! let key  = FieldEncryptor::derive_key(pw, &salt);    // 32-byte key
//! let ct   = FieldEncryptor::encrypt_field("123456789", &key)?;
//! let pt   = FieldEncryptor::decrypt_field(&ct, &key)?;
//! ```
//!
//! # What Gets Encrypted
//!
//! - Bank account numbers (`BankAccountDetails.account_number`)
//! - Bank routing numbers (`BankAccountDetails.routing_number`)
//! - SSN / tax IDs (organization `tax_id`, employee records)
//! - Any field flagged as sensitive by the data model.
//!
//! # Logging Policy
//!
//! Keys, nonces, plaintext, and derived key material are **never** logged.
//! Only operation-level metadata (success/failure, byte counts of ciphertext)
//! is emitted at `debug` level.

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Key, Nonce,
};
use hkdf::Hkdf;
use rand::{rngs::OsRng, RngCore};
use sha2::Sha256;
use thiserror::Error;
use tracing::{debug, info, warn};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// HKDF-SHA256 info (context) string.
///
/// Domain-separates the derived key so that the same password + salt cannot
/// be reused to derive keys for other subsystems without a different info.
const HKDF_INFO: &[u8] = b"nexus-ledger-field-encryption-v1";

/// Nonce size in bytes (96 bits — the standard for AES-GCM).
const NONCE_SIZE: usize = 12;

/// GCM authentication tag size in bytes (128 bits).
const GCM_TAG_SIZE: usize = 16;

/// Minimum ciphertext blob length: nonce (12) + tag (16) = 28 bytes.
///
/// A zero-length plaintext encrypts to exactly this size.
const MIN_CIPHERTEXT_LEN: usize = NONCE_SIZE + GCM_TAG_SIZE;

/// Salt size in bytes (256 bits).
const SALT_SIZE: usize = 32;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors that can occur during encryption or decryption operations.
#[derive(Error, Debug)]
pub enum EncryptionError {
    /// Encryption operation failed (cipher error, key construction, etc.).
    #[error("Encryption failed: {0}")]
    Encrypt(String),

    /// Decryption operation failed.
    ///
    /// This covers GCM tag verification failures (wrong key, tampered data)
    /// and invalid UTF-8 in the decrypted plaintext.
    #[error("Decryption failed: {0}")]
    Decrypt(String),

    /// The provided key is not exactly 32 bytes (256 bits).
    #[error("Invalid key length: expected 32 bytes, got {0}")]
    InvalidKeyLength(usize),

    /// The ciphertext blob is too short to contain a nonce and GCM tag.
    #[error("Invalid ciphertext: too short ({0} bytes, minimum 28)")]
    InvalidCiphertext(usize),

    /// JSON serialization or deserialization error during `encrypt_json` /
    /// `decrypt_json`.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

// ---------------------------------------------------------------------------
// FieldEncryptor
// ---------------------------------------------------------------------------

/// Stateless field-level encryptor using AES-256-GCM.
///
/// All methods are associated functions (no `self`) so the struct serves as a
/// namespace. The key must be exactly 32 bytes (256 bits), typically obtained
/// from [`derive_key`](Self::derive_key).
///
/// # Derive(Debug, Clone)
///
/// `Debug` and `Clone` are derived to match the edge module's struct convention,
/// even though `FieldEncryptor` carries no interior data.
#[derive(Debug, Clone)]
pub struct FieldEncryptor;

impl FieldEncryptor {
    // -----------------------------------------------------------------
    // Key derivation & salt
    // -----------------------------------------------------------------

    /// Derive a 256-bit key from a user password and salt using HKDF-SHA256.
    ///
    /// # Arguments
    ///
    /// * `password` — User password (input key material / IKM).
    /// * `salt` — Random salt, ideally 32 bytes from
    ///   [`generate_salt`](Self::generate_salt). The salt is **not secret**;
    ///   it should be stored alongside the encrypted data so the key can be
    ///   reconstructed during decryption.
    ///
    /// # Returns
    ///
    /// A `[u8; 32]` key suitable for [`encrypt_field`](Self::encrypt_field)
    /// and [`decrypt_field`](Self::decrypt_field).
    ///
    /// # Security Notes
    ///
    /// - HKDF-SHA256 is used per RFC 5869 (Extract-then-Expand).
    /// - A domain-separated info string binds the key to field-level
    ///   encryption, preventing cross-subsystem key reuse.
    /// - HKDF is **not** a password hashing function. For password storage
    ///   (a different concern) use Argon2. HKDF is appropriate here because
    ///   the password is an existing high-entropy secret (e.g. derived from
    ///   Argon2 by the auth layer) used as an encryption key.
    pub fn derive_key(password: &str, salt: &[u8]) -> [u8; 32] {
        if password.is_empty() {
            warn!("Deriving encryption key from an empty password — this is insecure");
        }

        info!("Deriving 256-bit encryption key via HKDF-SHA256");

        // HKDF-Extract: PRK = HMAC-SHA256(salt, IKM)
        // HKDF-Expand:  OKM = HMAC-SHA256(PRK, info || counter)
        let hk = Hkdf::<Sha256>::new(Some(salt), password.as_bytes());
        let mut okm = [0u8; 32];

        // 32 bytes is well within SHA256's HKDF max output (255 × 32 = 8 160 B),
        // so expand() cannot fail for this length.
        hk.expand(HKDF_INFO, &mut okm)
            .expect("HKDF-SHA256 expand to 32 bytes is infallible");

        okm
    }

    /// Generate a cryptographically random 32-byte salt.
    ///
    /// The salt is **not secret** — it should be stored alongside encrypted
    /// data (e.g. in a separate SQLite column or prefixed to the ciphertext
    /// blob) so the key can be reconstructed during decryption.
    ///
    /// Uses `OsRng` (the operating system's CSPRNG) for generation.
    pub fn generate_salt() -> Vec<u8> {
        let mut salt = vec![0u8; SALT_SIZE];
        OsRng.fill_bytes(&mut salt);
        debug!("Generated {}-byte random salt", SALT_SIZE);
        salt
    }

    // -----------------------------------------------------------------
    // String field encryption
    // -----------------------------------------------------------------

    /// Encrypt a plaintext string using AES-256-GCM.
    ///
    /// # Arguments
    ///
    /// * `plaintext` — The sensitive data to encrypt (e.g. bank account
    ///   number, SSN, routing number).
    /// * `key` — 32-byte encryption key from [`derive_key`](Self::derive_key).
    ///
    /// # Returns
    ///
    /// A byte vector containing `nonce (12 B) || ciphertext || tag (16 B)`.
    ///
    /// # Errors
    ///
    /// - [`EncryptionError::InvalidKeyLength`] — key is not 32 bytes.
    /// - [`EncryptionError::Encrypt`] — cipher error (should not occur under
    ///   normal conditions).
    ///
    /// # Security Notes
    ///
    /// - A fresh random 96-bit nonce is generated for each call.
    /// - The nonce is prepended to the ciphertext so the blob is self-contained.
    /// - Neither the key, nonce, nor plaintext is ever logged.
    pub fn encrypt_field(plaintext: &str, key: &[u8]) -> Result<Vec<u8>, EncryptionError> {
        if key.len() != 32 {
            return Err(EncryptionError::InvalidKeyLength(key.len()));
        }

        // Generate a fresh random nonce for each encryption.
        let mut nonce_bytes = [0u8; NONCE_SIZE];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        // Construct the cipher from the provided key.
        let key_array = Key::<Aes256Gcm>::from_slice(key);
        let cipher = Aes256Gcm::new(key_array);

        // Encrypt — aes-gcm appends the 16-byte tag to the ciphertext.
        let ciphertext = cipher
            .encrypt(nonce, plaintext.as_bytes())
            .map_err(|e| EncryptionError::Encrypt(e.to_string()))?;

        // Prepend nonce so the complete blob is self-contained.
        let mut output = Vec::with_capacity(NONCE_SIZE + ciphertext.len());
        output.extend_from_slice(&nonce_bytes);
        output.extend_from_slice(&ciphertext);

        debug!("Field encrypted: {} bytes output blob", output.len());

        Ok(output)
    }

    /// Decrypt a ciphertext blob to a plaintext string.
    ///
    /// # Arguments
    ///
    /// * `ciphertext` — The blob from [`encrypt_field`](Self::encrypt_field)
    ///   (`nonce || ciphertext || tag`).
    /// * `key` — 32-byte encryption key (must match the key used for
    ///   encryption).
    ///
    /// # Errors
    ///
    /// - [`EncryptionError::InvalidKeyLength`] — key is not 32 bytes.
    /// - [`EncryptionError::InvalidCiphertext`] — blob is shorter than 28
    ///   bytes (cannot contain nonce + tag).
    /// - [`EncryptionError::Decrypt`] — GCM tag verification failed (wrong
    ///   key, tampered nonce, or corrupted ciphertext), or the decrypted
    ///   bytes are not valid UTF-8.
    ///
    /// # Security Notes
    ///
    /// - Decryption failure does not reveal whether the key was wrong or the
    ///   data was tampered — GCM treats both identically.
    /// - Neither the key, nonce, nor plaintext is ever logged.
    pub fn decrypt_field(ciphertext: &[u8], key: &[u8]) -> Result<String, EncryptionError> {
        if key.len() != 32 {
            return Err(EncryptionError::InvalidKeyLength(key.len()));
        }

        if ciphertext.len() < MIN_CIPHERTEXT_LEN {
            return Err(EncryptionError::InvalidCiphertext(ciphertext.len()));
        }

        // Split nonce (first 12 bytes) from ciphertext+tag (remainder).
        let (nonce_bytes, ct) = ciphertext.split_at(NONCE_SIZE);
        let nonce = Nonce::from_slice(nonce_bytes);

        let key_array = Key::<Aes256Gcm>::from_slice(key);
        let cipher = Aes256Gcm::new(key_array);

        // Decrypt — verifies the GCM tag. Returns Vec<u8> on success.
        let plaintext_bytes = cipher.decrypt(nonce, ct).map_err(|e| {
            warn!("Field decryption failed (GCM tag verification or data corruption)");
            EncryptionError::Decrypt(e.to_string())
        })?;

        // Convert decrypted bytes to a UTF-8 string.
        let plaintext = String::from_utf8(plaintext_bytes).map_err(|e| {
            warn!("Decrypted field is not valid UTF-8");
            EncryptionError::Decrypt(format!("invalid UTF-8 in plaintext: {}", e))
        })?;

        debug!("Field decrypted: {} bytes input blob", ciphertext.len());

        Ok(plaintext)
    }

    // -----------------------------------------------------------------
    // JSON field encryption
    // -----------------------------------------------------------------

    /// Encrypt a JSON value using AES-256-GCM.
    ///
    /// Serializes the JSON value to a compact string representation and then
    /// encrypts it. Use this for complex sensitive fields (e.g. full bank
    /// account details stored as a JSON object, employee records with
    /// multiple sensitive fields).
    ///
    /// # Arguments
    ///
    /// * `value` — The JSON value to encrypt.
    /// * `key` — 32-byte encryption key.
    ///
    /// # Errors
    ///
    /// - [`EncryptionError::Json`] — JSON serialization failed.
    /// - Encryption errors from [`encrypt_field`](Self::encrypt_field).
    pub fn encrypt_json(value: &serde_json::Value, key: &[u8]) -> Result<Vec<u8>, EncryptionError> {
        // Serialize JSON to a compact string (always valid UTF-8).
        let json_str = serde_json::to_string(value)?;
        debug!("JSON value serialized for encryption");
        Self::encrypt_field(&json_str, key)
    }

    /// Decrypt a ciphertext blob to a JSON value.
    ///
    /// # Arguments
    ///
    /// * `ciphertext` — The blob from [`encrypt_json`](Self::encrypt_json).
    /// * `key` — 32-byte encryption key.
    ///
    /// # Errors
    ///
    /// - Decryption errors from [`decrypt_field`](Self::decrypt_field).
    /// - [`EncryptionError::Json`] — JSON deserialization failed (the
    ///   decrypted bytes are not valid JSON).
    pub fn decrypt_json(ciphertext: &[u8], key: &[u8]) -> Result<serde_json::Value, EncryptionError> {
        let json_str = Self::decrypt_field(ciphertext, key)?;
        let value: serde_json::Value = serde_json::from_str(&json_str)?;
        debug!("JSON value deserialized after decryption");
        Ok(value)
    }
}

// ---------------------------------------------------------------------------
// Unit Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // -----------------------------------------------------------------
    // Test helpers
    // -----------------------------------------------------------------

    /// Derive a throwaway test key from a random salt.
    fn test_key() -> [u8; 32] {
        let salt = FieldEncryptor::generate_salt();
        FieldEncryptor::derive_key("test-password-12345", &salt)
    }

    /// Derive a test key with a fixed salt (for deterministic tests).
    fn test_key_fixed() -> [u8; 32] {
        FieldEncryptor::derive_key("test-password-12345", &[1, 2, 3, 4])
    }

    // -----------------------------------------------------------------
    // String round-trip tests
    // -----------------------------------------------------------------

    #[test]
    fn test_encrypt_decrypt_round_trip() {
        let key = test_key();
        let plaintext = "sensitive-bank-account-123456789";
        let ciphertext = FieldEncryptor::encrypt_field(plaintext, &key).unwrap();
        let decrypted = FieldEncryptor::decrypt_field(&ciphertext, &key).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_empty_plaintext_round_trip() {
        let key = test_key();
        let plaintext = "";
        let ciphertext = FieldEncryptor::encrypt_field(plaintext, &key).unwrap();
        // Even empty plaintext produces nonce(12) + tag(16) = 28 bytes.
        assert_eq!(ciphertext.len(), MIN_CIPHERTEXT_LEN);
        let decrypted = FieldEncryptor::decrypt_field(&ciphertext, &key).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_large_text_round_trip() {
        let key = test_key();
        let plaintext = "A".repeat(10_000);
        let ciphertext = FieldEncryptor::encrypt_field(&plaintext, &key).unwrap();
        // 10 000 bytes plaintext → nonce(12) + ciphertext(10 000) + tag(16)
        assert_eq!(ciphertext.len(), NONCE_SIZE + 10_000 + GCM_TAG_SIZE);
        let decrypted = FieldEncryptor::decrypt_field(&ciphertext, &key).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_unicode_text_round_trip() {
        let key = test_key();
        let plaintext = "日本語テスト — Эmöji: 🚀💰📊 — العربية — Ελληνικά";
        let ciphertext = FieldEncryptor::encrypt_field(plaintext, &key).unwrap();
        let decrypted = FieldEncryptor::decrypt_field(&ciphertext, &key).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_single_character_round_trip() {
        let key = test_key();
        let plaintext = "X";
        let ciphertext = FieldEncryptor::encrypt_field(plaintext, &key).unwrap();
        let decrypted = FieldEncryptor::decrypt_field(&ciphertext, &key).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_bank_account_number_round_trip() {
        let key = test_key();
        let plaintext = "1234567890123456";
        let ciphertext = FieldEncryptor::encrypt_field(plaintext, &key).unwrap();
        let decrypted = FieldEncryptor::decrypt_field(&ciphertext, &key).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_routing_number_round_trip() {
        let key = test_key();
        let plaintext = "021000021";
        let ciphertext = FieldEncryptor::encrypt_field(plaintext, &key).unwrap();
        let decrypted = FieldEncryptor::decrypt_field(&ciphertext, &key).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_ssn_round_trip() {
        let key = test_key();
        let plaintext = "123-45-6789";
        let ciphertext = FieldEncryptor::encrypt_field(plaintext, &key).unwrap();
        let decrypted = FieldEncryptor::decrypt_field(&ciphertext, &key).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    // -----------------------------------------------------------------
    // JSON round-trip tests
    // -----------------------------------------------------------------

    #[test]
    fn test_encrypt_decrypt_json_round_trip() {
        let key = test_key();
        let value = json!({
            "account_number": "1234567890",
            "routing_number": "021000021",
            "account_type": "checking",
            "holder_name": "John Doe"
        });
        let ciphertext = FieldEncryptor::encrypt_json(&value, &key).unwrap();
        let decrypted = FieldEncryptor::decrypt_json(&ciphertext, &key).unwrap();
        assert_eq!(decrypted, value);
    }

    #[test]
    fn test_encrypt_decrypt_json_array() {
        let key = test_key();
        let value = json!(["1234567890", "021000021", "9876543210"]);
        let ciphertext = FieldEncryptor::encrypt_json(&value, &key).unwrap();
        let decrypted = FieldEncryptor::decrypt_json(&ciphertext, &key).unwrap();
        assert_eq!(decrypted, value);
    }

    #[test]
    fn test_encrypt_decrypt_json_null() {
        let key = test_key();
        let value = json!(null);
        let ciphertext = FieldEncryptor::encrypt_json(&value, &key).unwrap();
        let decrypted = FieldEncryptor::decrypt_json(&ciphertext, &key).unwrap();
        assert_eq!(decrypted, value);
    }

    #[test]
    fn test_encrypt_decrypt_json_number() {
        let key = test_key();
        let value = json!(12345.67);
        let ciphertext = FieldEncryptor::encrypt_json(&value, &key).unwrap();
        let decrypted = FieldEncryptor::decrypt_json(&ciphertext, &key).unwrap();
        assert_eq!(decrypted, value);
    }

    #[test]
    fn test_encrypt_decrypt_json_boolean() {
        let key = test_key();
        let value = json!(true);
        let ciphertext = FieldEncryptor::encrypt_json(&value, &key).unwrap();
        let decrypted = FieldEncryptor::decrypt_json(&ciphertext, &key).unwrap();
        assert_eq!(decrypted, value);
    }

    #[test]
    fn test_encrypt_decrypt_json_nested_object() {
        let key = test_key();
        let value = json!({
            "level1": {
                "level2": {
                    "level3": "deep-secret-value"
                }
            }
        });
        let ciphertext = FieldEncryptor::encrypt_json(&value, &key).unwrap();
        let decrypted = FieldEncryptor::decrypt_json(&ciphertext, &key).unwrap();
        assert_eq!(decrypted, value);
    }

    #[test]
    fn test_encrypt_decrypt_json_empty_object() {
        let key = test_key();
        let value = json!({});
        let ciphertext = FieldEncryptor::encrypt_json(&value, &key).unwrap();
        let decrypted = FieldEncryptor::decrypt_json(&ciphertext, &key).unwrap();
        assert_eq!(decrypted, value);
    }

    #[test]
    fn test_encrypt_decrypt_json_empty_array() {
        let key = test_key();
        let value = json!([]);
        let ciphertext = FieldEncryptor::encrypt_json(&value, &key).unwrap();
        let decrypted = FieldEncryptor::decrypt_json(&ciphertext, &key).unwrap();
        assert_eq!(decrypted, value);
    }

    // -----------------------------------------------------------------
    // Nonce randomness tests
    // -----------------------------------------------------------------

    #[test]
    fn test_different_encryptions_produce_different_ciphertexts() {
        let key = test_key_fixed();
        let plaintext = "same-plaintext-value";
        let ct1 = FieldEncryptor::encrypt_field(plaintext, &key).unwrap();
        let ct2 = FieldEncryptor::encrypt_field(plaintext, &key).unwrap();
        let ct3 = FieldEncryptor::encrypt_field(plaintext, &key).unwrap();

        // All three should decrypt to the same plaintext.
        assert_eq!(FieldEncryptor::decrypt_field(&ct1, &key).unwrap(), plaintext);
        assert_eq!(FieldEncryptor::decrypt_field(&ct2, &key).unwrap(), plaintext);
        assert_eq!(FieldEncryptor::decrypt_field(&ct3, &key).unwrap(), plaintext);

        // But the ciphertext blobs should differ (random nonce).
        assert_ne!(ct1, ct2, "two encryptions of the same plaintext must differ");
        assert_ne!(ct1, ct3);
        assert_ne!(ct2, ct3);
    }

    #[test]
    fn test_nonce_is_random_per_encryption() {
        let key = test_key_fixed();
        let plaintext = "test";
        let ct1 = FieldEncryptor::encrypt_field(plaintext, &key).unwrap();
        let ct2 = FieldEncryptor::encrypt_field(plaintext, &key).unwrap();

        // Nonces are the first NONCE_SIZE bytes.
        let nonce1 = &ct1[..NONCE_SIZE];
        let nonce2 = &ct2[..NONCE_SIZE];
        assert_ne!(
            nonce1, nonce2,
            "nonce must be freshly random per encryption"
        );
    }

    // -----------------------------------------------------------------
    // Wrong key tests
    // -----------------------------------------------------------------

    #[test]
    fn test_wrong_key_fails_decryption() {
        let salt = FieldEncryptor::generate_salt();
        let key1 = FieldEncryptor::derive_key("password-correct", &salt);
        let key2 = FieldEncryptor::derive_key("password-wrong", &salt);
        let plaintext = "secret-data-12345";
        let ciphertext = FieldEncryptor::encrypt_field(plaintext, &key1).unwrap();

        let result = FieldEncryptor::decrypt_field(&ciphertext, &key2);
        assert!(result.is_err(), "decryption with wrong key must fail");
        match result {
            Err(EncryptionError::Decrypt(_)) => {}
            Err(e) => panic!("expected Decrypt error, got {:?}", e),
            Ok(_) => panic!("expected error, got Ok"),
        }
    }

    #[test]
    fn test_wrong_key_fails_json_decryption() {
        let salt = FieldEncryptor::generate_salt();
        let key1 = FieldEncryptor::derive_key("password-correct", &salt);
        let key2 = FieldEncryptor::derive_key("password-wrong", &salt);
        let value = json!({"secret": "data"});
        let ciphertext = FieldEncryptor::encrypt_json(&value, &key1).unwrap();

        let result = FieldEncryptor::decrypt_json(&ciphertext, &key2);
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_key_fails() {
        let key = [];
        let result = FieldEncryptor::encrypt_field("test", &key);
        assert!(matches!(result, Err(EncryptionError::InvalidKeyLength(0))));
    }

    // -----------------------------------------------------------------
    // Truncated / tampered ciphertext tests
    // -----------------------------------------------------------------

    #[test]
    fn test_truncated_ciphertext_fails() {
        let key = test_key();
        let ciphertext = FieldEncryptor::encrypt_field("secret", &key).unwrap();
        // Truncate to 10 bytes (< MIN_CIPHERTEXT_LEN of 28).
        let truncated = &ciphertext[..10];
        let result = FieldEncryptor::decrypt_field(truncated, &key);
        assert!(matches!(result, Err(EncryptionError::InvalidCiphertext(10))));
    }

    #[test]
    fn test_empty_ciphertext_fails() {
        let key = test_key();
        let result = FieldEncryptor::decrypt_field(&[], &key);
        assert!(matches!(result, Err(EncryptionError::InvalidCiphertext(0))));
    }

    #[test]
    fn test_ciphertext_exactly_27_bytes_fails() {
        let key = test_key();
        let fake_ct = vec![0u8; 27]; // one byte short of minimum
        let result = FieldEncryptor::decrypt_field(&fake_ct, &key);
        assert!(matches!(result, Err(EncryptionError::InvalidCiphertext(27))));
    }

    #[test]
    fn test_ciphertext_exactly_28_bytes_fails_tag_verification() {
        let key = test_key();
        // 28 bytes passes length check but should fail GCM tag verification.
        let fake_ct = vec![0u8; 28];
        let result = FieldEncryptor::decrypt_field(&fake_ct, &key);
        assert!(matches!(result, Err(EncryptionError::Decrypt(_))));
    }

    #[test]
    fn test_tampered_ciphertext_fails() {
        let key = test_key();
        let mut ciphertext = FieldEncryptor::encrypt_field("secret-data", &key).unwrap();
        // Flip a bit in the ciphertext portion (after the nonce).
        let last = ciphertext.len() - 1;
        ciphertext[last] ^= 0xFF;
        let result = FieldEncryptor::decrypt_field(&ciphertext, &key);
        assert!(matches!(result, Err(EncryptionError::Decrypt(_))));
    }

    #[test]
    fn test_tampered_nonce_fails() {
        let key = test_key();
        let mut ciphertext = FieldEncryptor::encrypt_field("secret-data", &key).unwrap();
        // Flip a bit in the nonce (first byte).
        ciphertext[0] ^= 0xFF;
        let result = FieldEncryptor::decrypt_field(&ciphertext, &key);
        assert!(matches!(result, Err(EncryptionError::Decrypt(_))));
    }

    #[test]
    fn test_tampered_tag_fails() {
        let key = test_key();
        let mut ciphertext = FieldEncryptor::encrypt_field("secret-data", &key).unwrap();
        // Flip a bit in the tag (last 16 bytes).
        let tag_start = ciphertext.len() - GCM_TAG_SIZE;
        ciphertext[tag_start] ^= 0x01;
        let result = FieldEncryptor::decrypt_field(&ciphertext, &key);
        assert!(matches!(result, Err(EncryptionError::Decrypt(_))));
    }

    // -----------------------------------------------------------------
    // Key derivation tests
    // -----------------------------------------------------------------

    #[test]
    fn test_derive_key_produces_32_bytes() {
        let salt = FieldEncryptor::generate_salt();
        let key = FieldEncryptor::derive_key("my-password", &salt);
        assert_eq!(key.len(), 32);
    }

    #[test]
    fn test_derive_key_is_deterministic_with_same_inputs() {
        let salt = vec![1, 2, 3, 4];
        let key1 = FieldEncryptor::derive_key("same-password", &salt);
        let key2 = FieldEncryptor::derive_key("same-password", &salt);
        assert_eq!(key1, key2, "same password + salt must produce same key");
    }

    #[test]
    fn test_derive_key_different_passwords_produce_different_keys() {
        let salt = vec![1, 2, 3, 4];
        let key1 = FieldEncryptor::derive_key("password-1", &salt);
        let key2 = FieldEncryptor::derive_key("password-2", &salt);
        assert_ne!(key1, key2);
    }

    #[test]
    fn test_derive_key_different_salts_produce_different_keys() {
        let salt1 = vec![1, 2, 3, 4];
        let salt2 = vec![5, 6, 7, 8];
        let key1 = FieldEncryptor::derive_key("same-password", &salt1);
        let key2 = FieldEncryptor::derive_key("same-password", &salt2);
        assert_ne!(key1, key2);
    }

    #[test]
    fn test_derive_key_empty_salt() {
        // Empty salt is valid in HKDF (uses zero-byte salt internally).
        let key = FieldEncryptor::derive_key("password", &[]);
        assert_eq!(key.len(), 32);
    }

    #[test]
    fn test_derive_key_empty_password() {
        // Empty password is valid HKDF input (but insecure — logged as warn).
        let key = FieldEncryptor::derive_key("", &[1, 2, 3]);
        assert_eq!(key.len(), 32);
    }

    // -----------------------------------------------------------------
    // Salt generation tests
    // -----------------------------------------------------------------

    #[test]
    fn test_generate_salt_produces_32_bytes() {
        let salt = FieldEncryptor::generate_salt();
        assert_eq!(salt.len(), 32);
    }

    #[test]
    fn test_generate_salt_is_random() {
        let salt1 = FieldEncryptor::generate_salt();
        let salt2 = FieldEncryptor::generate_salt();
        let salt3 = FieldEncryptor::generate_salt();
        assert_ne!(salt1, salt2, "salts must be random");
        assert_ne!(salt1, salt3);
        assert_ne!(salt2, salt3);
    }

    // -----------------------------------------------------------------
    // Key length validation tests
    // -----------------------------------------------------------------

    #[test]
    fn test_encrypt_with_short_key_fails() {
        let short_key = vec![0u8; 16];
        let result = FieldEncryptor::encrypt_field("test", &short_key);
        assert!(matches!(result, Err(EncryptionError::InvalidKeyLength(16))));
    }

    #[test]
    fn test_decrypt_with_short_key_fails() {
        let short_key = vec![0u8; 16];
        let result = FieldEncryptor::decrypt_field(&[0u8; 28], &short_key);
        assert!(matches!(result, Err(EncryptionError::InvalidKeyLength(16))));
    }

    #[test]
    fn test_encrypt_with_long_key_fails() {
        let long_key = vec![0u8; 64];
        let result = FieldEncryptor::encrypt_field("test", &long_key);
        assert!(matches!(result, Err(EncryptionError::InvalidKeyLength(64))));
    }

    #[test]
    fn test_decrypt_with_long_key_fails() {
        let long_key = vec![0u8; 64];
        let result = FieldEncryptor::decrypt_field(&[0u8; 28], &long_key);
        assert!(matches!(result, Err(EncryptionError::InvalidKeyLength(64))));
    }

    #[test]
    fn test_encrypt_json_with_short_key_fails() {
        let short_key = vec![0u8; 16];
        let result = FieldEncryptor::encrypt_json(&json!("test"), &short_key);
        assert!(matches!(result, Err(EncryptionError::InvalidKeyLength(16))));
    }

    #[test]
    fn test_decrypt_json_with_short_key_fails() {
        let short_key = vec![0u8; 16];
        let result = FieldEncryptor::decrypt_json(&[0u8; 28], &short_key);
        assert!(matches!(result, Err(EncryptionError::InvalidKeyLength(16))));
    }

    // -----------------------------------------------------------------
    // Integration tests: key derivation + encryption
    // -----------------------------------------------------------------

    #[test]
    fn test_derive_key_then_encrypt_decrypt() {
        let salt = FieldEncryptor::generate_salt();
        let key = FieldEncryptor::derive_key("user-master-password", &salt);
        let plaintext = "1234567890123456"; // bank account number
        let ciphertext = FieldEncryptor::encrypt_field(plaintext, &key).unwrap();
        let decrypted = FieldEncryptor::decrypt_field(&ciphertext, &key).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_derive_key_with_different_salts_are_independent() {
        let salt1 = FieldEncryptor::generate_salt();
        let salt2 = FieldEncryptor::generate_salt();
        let key1 = FieldEncryptor::derive_key("password", &salt1);
        let key2 = FieldEncryptor::derive_key("password", &salt2);
        // Encrypt with key1, decrypt with key2 should fail.
        let ct = FieldEncryptor::encrypt_field("test", &key1).unwrap();
        assert!(FieldEncryptor::decrypt_field(&ct, &key2).is_err());
    }

    #[test]
    fn test_full_bank_account_json_round_trip() {
        let salt = FieldEncryptor::generate_salt();
        let key = FieldEncryptor::derive_key("banking-password", &salt);

        let bank_details = json!({
            "bank_name": "First National Bank",
            "account_number": "9876543210",
            "routing_number": "021000021",
            "account_holder": "Richdale Accounting LLC",
            "account_type": "checking"
        });

        let ciphertext = FieldEncryptor::encrypt_json(&bank_details, &key).unwrap();
        let decrypted = FieldEncryptor::decrypt_json(&ciphertext, &key).unwrap();

        assert_eq!(decrypted, bank_details);
        // Verify specific sensitive fields round-trip correctly.
        assert_eq!(
            decrypted["account_number"].as_str().unwrap(),
            "9876543210"
        );
        assert_eq!(
            decrypted["routing_number"].as_str().unwrap(),
            "021000021"
        );
    }

    // -----------------------------------------------------------------
    // Ciphertext format tests
    // -----------------------------------------------------------------

    #[test]
    fn test_ciphertext_starts_with_nonce() {
        let key = test_key();
        let ciphertext = FieldEncryptor::encrypt_field("test", &key).unwrap();
        // First NONCE_SIZE bytes are the nonce.
        assert!(ciphertext.len() > NONCE_SIZE);
        let _nonce = &ciphertext[..NONCE_SIZE];
        // The remainder is ciphertext + tag.
        let _ct_and_tag = &ciphertext[NONCE_SIZE..];
    }

    #[test]
    fn test_min_ciphertext_length_for_empty_plaintext() {
        let key = test_key();
        let ciphertext = FieldEncryptor::encrypt_field("", &key).unwrap();
        // nonce(12) + tag(16) = 28, no ciphertext body for empty plaintext.
        assert_eq!(ciphertext.len(), MIN_CIPHERTEXT_LEN);
    }

    #[test]
    fn test_ciphertext_length_grows_with_plaintext() {
        let key = test_key();

        let ct_short = FieldEncryptor::encrypt_field("short", &key).unwrap();
        let ct_long = FieldEncryptor::encrypt_field("this is a much longer plaintext value", &key).unwrap();

        // Both have nonce(12) + tag(16) = 28 bytes overhead.
        assert_eq!(ct_short.len(), NONCE_SIZE + 5 + GCM_TAG_SIZE);
        assert_eq!(ct_long.len(), NONCE_SIZE + 37 + GCM_TAG_SIZE);
    }

    // -----------------------------------------------------------------
    // Multiple field encryption (simulating encrypting several DB columns)
    // -----------------------------------------------------------------

    #[test]
    fn test_multiple_fields_independent_encryption() {
        let key = test_key();

        let account_number = "1234567890123456";
        let routing_number = "021000021";
        let tax_id = "12-3456789";

        let ct_acct = FieldEncryptor::encrypt_field(account_number, &key).unwrap();
        let ct_routing = FieldEncryptor::encrypt_field(routing_number, &key).unwrap();
        let ct_tax = FieldEncryptor::encrypt_field(tax_id, &key).unwrap();

        // All ciphertexts should be different (different nonces).
        assert_ne!(ct_acct, ct_routing);
        assert_ne!(ct_acct, ct_tax);
        assert_ne!(ct_routing, ct_tax);

        // Each decrypts to its original value.
        assert_eq!(FieldEncryptor::decrypt_field(&ct_acct, &key).unwrap(), account_number);
        assert_eq!(FieldEncryptor::decrypt_field(&ct_routing, &key).unwrap(), routing_number);
        assert_eq!(FieldEncryptor::decrypt_field(&ct_tax, &key).unwrap(), tax_id);
    }

    // -----------------------------------------------------------------
    // Cross-encrypt: string then decrypt as JSON (should fail)
    // -----------------------------------------------------------------

    #[test]
    fn test_decrypt_string_as_json_fails() {
        let key = test_key();
        // Encrypt a non-JSON string.
        let ciphertext = FieldEncryptor::encrypt_field("not-json-at-all", &key).unwrap();
        // Attempting to decrypt as JSON should fail.
        let result = FieldEncryptor::decrypt_json(&ciphertext, &key);
        assert!(matches!(result, Err(EncryptionError::Json(_))));
    }

    #[test]
    fn test_decrypt_json_as_string_succeeds() {
        let key = test_key();
        let value = json!({"key": "value"});
        let ciphertext = FieldEncryptor::encrypt_json(&value, &key).unwrap();
        // Decrypting as a raw string should give the JSON string.
        let raw_string = FieldEncryptor::decrypt_field(&ciphertext, &key).unwrap();
        assert!(raw_string.contains("\"key\""));
        assert!(raw_string.contains("\"value\""));
    }
}
