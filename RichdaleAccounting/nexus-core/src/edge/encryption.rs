//! Edge Encryption — Key Wrapping Architecture
//!
//! Field-level encryption with Data Encryption Key (DEK) / Key Encryption Key
//! (KEK) separation for offline SQLite storage.
//!
//! # Architecture
//!
//! ```text
//! ┌──────────────────────────────────────────────────────────────────┐
//! │                        KEY HIERARCHY                             │
//! │                                                                  │
//! │  User Password ──HKDF-SHA256──▶  KEK (Key Encryption Key)       │
//! │        + Salt                      [u8; 32]                      │
//! │                                        │                         │
//! │                                        ▼                         │
//! │  Random DEK ──AES-256-GCM──▶  Wrapped DEK (stored in DB)        │
//! │  [u8; 32]                      nonce(12) + ct(32) + tag(16)     │
//! │      │                                                         │
//! │      ▼                                                         │
//! │  Field Data ──AES-256-GCM──▶  Encrypted Field (stored in DB)    │
//! │                                nonce(12) + ct(n) + tag(16)      │
//! └──────────────────────────────────────────────────────────────────┘
//! ```
//!
//! - **DEK (Data Encryption Key)**: A 256-bit random key generated once per
//!   database. It directly encrypts sensitive field values. Because it is
//!   random (not derived from a password), changing the user's password does
//!   **not** require re-encrypting any field data — only re-wrapping the DEK.
//!
//! - **KEK (Key Encryption Key)**: Derived from the user's password via
//!   HKDF-SHA256. It encrypts (wraps) the DEK for at-rest storage in the
//!   `encryption_keys` table. When the password changes, a new KEK is derived
//!   and the DEK is re-wrapped — no data re-encryption needed.
//!
//! - **Wrapped DEK**: The DEK encrypted by the KEK using AES-256-GCM. Stored
//!   in the `encryption_keys` table alongside the salt and key version.
//!
//! # Security Properties
//!
//! - **Confidentiality**: AES-256-GCM provides authenticated encryption (AEAD).
//! - **Integrity**: The 16-byte GCM authentication tag detects any tampering
//!   of the ciphertext or nonce. A wrong key or a single flipped bit causes
//!   decryption to fail.
//! - **Key separation**: HKDF-SHA256 with a domain-separated info string binds
//!   the KEK to the "nexus-ledger-encryption-kek" use case.
//! - **Nonce uniqueness**: A fresh 96-bit random nonce is generated for every
//!   `encrypt_field` and `wrap_dek` call. With a random nonce the GCM birthday
//!   bound is roughly 2^32 messages per key — far beyond any single-tenant
//!   accounting database.
//! - **Memory hygiene**: The `zeroize` crate clears KEK/DEK material from
//!   memory after use. [`SecureKey`] wraps a `[u8; 32]` and auto-zeroizes on
//!   drop via `#[derive(Zeroize)]` + `#[zeroize(drop)]`.
//!
//! # Ciphertext Format
//!
//! All encrypted blobs (field data and wrapped DEKs) use the same layout:
//!
//! ```text
//! +-------------------+--------------------------+------------------+
//! | nonce (12 bytes)  | ciphertext (variable)    | tag (16 bytes)   |
//! +-------------------+--------------------------+------------------+
//! ```
//!
//! The GCM tag is appended to the ciphertext by the `aes-gcm` crate. The
//! complete blob is self-contained: the nonce is prepended so the decryptor
//! does not need to store or track nonces separately.
//!
//! # Sensitive Fields
//!
//! The following database field names are classified as sensitive and must be
//! encrypted at rest (see [`SENSITIVE_FIELDS`]):
//!
//! - `bank_account_number`
//! - `routing_number`
//! - `tax_id`
//! - `ssn`
//! - `credit_card_number`
//!
//! # Logging Policy
//!
//! Keys, nonces, plaintext, and derived key material are **never** logged.
//! Only operation-level metadata (success/failure, byte counts) is emitted at
//! `debug` level. [`SecureKey`] implements `Debug` as `[REDACTED; 32]` to
//! prevent accidental key leakage through trace logs.
//!
//! # Usage
//!
//! ## Initial Setup
//!
//! ```text
//! let salt = generate_salt();                           // store in encryption_keys
//! let kek = derive_kek(password, &salt);               // KEK from password
//! let dek = generate_dek();                            // random DEK
//! let wrapped_dek = wrap_dek(&dek, &kek);              // store in encryption_keys
//! // Encrypt fields with the DEK:
//! let ct = encrypt_field("1234567890123456", &dek);    // store in data table
//! ```
//!
//! ## Decryption
//!
//! ```text
//! let kek = derive_kek(password, &salt);               // re-derive KEK
//! let dek = unwrap_dek(&wrapped_dek, &kek)?;           // unwrap DEK
//! let pt = decrypt_field(&ct, &dek)?;                  // decrypt field
//! ```
//!
//! ## Password Change
//!
//! ```text
//! // No data re-encryption — just re-wrap the DEK:
//! let new_wrapped = change_password(old_pw, new_pw, &salt, &wrapped_dek)?;
//! ```

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Key, Nonce,
};
use hkdf::Hkdf;
use rand::{rngs::OsRng, RngCore};
use sha2::Sha256;
use thiserror::Error;
use tracing::{debug, info, warn};
use zeroize::Zeroize;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// HKDF-SHA256 info (context) string for KEK derivation.
///
/// Domain-separates the KEK so the same password + salt cannot be reused to
/// derive keys for other subsystems without a different info string.
const HKDF_INFO: &[u8] = b"nexus-ledger-encryption-kek-v1";

/// Nonce size in bytes (96 bits — the standard for AES-GCM).
const NONCE_SIZE: usize = 12;

/// GCM authentication tag size in bytes (128 bits).
const GCM_TAG_SIZE: usize = 16;

/// DEK / KEK size in bytes (256 bits).
const KEY_SIZE: usize = 32;

/// Minimum ciphertext blob length: nonce (12) + tag (16) = 28 bytes.
///
/// A zero-length plaintext encrypts to exactly this size.
const MIN_BLOB_LEN: usize = NONCE_SIZE + GCM_TAG_SIZE;

/// Salt size in bytes (256 bits).
const SALT_SIZE: usize = 32;

// ---------------------------------------------------------------------------
// Sensitive fields
// ---------------------------------------------------------------------------

/// Database field names that must be encrypted at rest.
///
/// These fields contain PII or financial identifiers that must not be stored
/// in plaintext in the local SQLite database. The [`is_sensitive_field`]
/// helper checks membership against this list.
pub const SENSITIVE_FIELDS: &[&str] = &[
    "bank_account_number",
    "routing_number",
    "tax_id",
    "ssn",
    "credit_card_number",
];

/// Check whether a database field name should be encrypted at rest.
///
/// Case-sensitive — field names must match exactly as they appear in
/// [`SENSITIVE_FIELDS`].
pub fn is_sensitive_field(field_name: &str) -> bool {
    SENSITIVE_FIELDS.contains(&field_name)
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors that can occur during encryption, decryption, or key wrapping.
#[derive(Error, Debug)]
pub enum EncryptionError {
    /// Encryption operation failed (cipher error, key construction, etc.).
    ///
    /// This should not occur under normal conditions with valid 32-byte keys.
    #[error("Encryption failed: {0}")]
    Encrypt(String),

    /// Decryption operation failed.
    ///
    /// Covers GCM tag verification failures (wrong key, tampered data) and
    /// invalid UTF-8 in the decrypted plaintext.
    #[error("Decryption failed: {0}")]
    Decrypt(String),

    /// The provided key is not exactly 32 bytes (256 bits).
    ///
    /// Only returned by the legacy `FieldEncryptor` API that accepts `&[u8]`
    /// slices — the new free-function API uses `&[u8; 32]` which enforces the
    /// length at compile time.
    #[error("Invalid key length: expected 32 bytes, got {0}")]
    InvalidKeyLength(usize),

    /// The ciphertext blob is too short to contain a nonce and GCM tag.
    #[error("Invalid ciphertext: too short ({0} bytes, minimum {1})")]
    InvalidCiphertext(usize, usize),

    /// DEK unwrap failed — the decrypted blob is not a valid 32-byte DEK.
    #[error("Key unwrap failed: {0}")]
    UnwrapFailed(String),

    /// JSON serialization or deserialization error during `encrypt_json` /
    /// `decrypt_json`.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

// ---------------------------------------------------------------------------
// SecureKey — zeroizing key wrapper
// ---------------------------------------------------------------------------

/// A 256-bit key that is automatically zeroized from memory when dropped.
///
/// Wraps a `[u8; 32]` and derives [`Zeroize`] with `#[zeroize(drop)]`, ensuring
/// that key material is overwritten with zeros before the stack or heap memory
/// is released. Use this for KEKs and DEKs that are derived or unwrapped
/// inside functions (e.g. [`change_password`]) so that the keys do not linger
/// in memory after the function returns.
///
/// # Logging Safety
///
/// `Debug` is implemented manually as `SecureKey([REDACTED; 32])` to prevent
/// accidental key leakage through `tracing` or `println!` calls.
///
/// # Example
///
/// ```text
/// let kek = SecureKey::new(derive_kek(password, &salt));
/// let dek = SecureKey::new(unwrap_dek(&wrapped, kek.as_bytes())?);
/// // ... use dek.as_bytes() for encryption ...
/// // kek and dek are zeroized when they go out of scope
/// ```
#[derive(Zeroize)]
#[zeroize(drop)]
pub struct SecureKey([u8; 32]);

impl SecureKey {
    /// Wrap a 256-bit key. The caller is responsible for zeroizing the
    /// original array if they retained a copy.
    pub fn new(key: [u8; 32]) -> Self {
        Self(key)
    }

    /// Returns a reference to the raw key bytes.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl std::fmt::Debug for SecureKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SecureKey([REDACTED; 32])")
    }
}

// ---------------------------------------------------------------------------
// Key derivation & generation
// ---------------------------------------------------------------------------

/// Derive a 256-bit Key Encryption Key (KEK) from a user password and salt
/// using HKDF-SHA256 (RFC 5869 Extract-then-Expand).
///
/// # Arguments
///
/// * `password` — User password (input key material / IKM).
/// * `salt` — Random salt, ideally 32 bytes from [`generate_salt`]. The salt
///   is **not secret**; it should be stored in the `encryption_keys` table
///   alongside the wrapped DEK so the KEK can be reconstructed.
///
/// # Returns
///
/// A `[u8; 32]` KEK suitable for [`wrap_dek`] and [`unwrap_dek`].
///
/// # Security Notes
///
/// - HKDF-SHA256 is used per RFC 5869 (Extract-then-Expand).
/// - A domain-separated info string (`nexus-ledger-encryption-kek-v1`) binds
///   the key to KEK derivation, preventing cross-subsystem key reuse.
/// - HKDF is **not** a password hashing function. For password *storage*
///   (a different concern) use Argon2. HKDF is appropriate here because the
///   password is an existing secret (e.g. verified by the auth layer via
///   Argon2) used as an encryption key.
/// - Neither the password nor the derived key is ever logged.
pub fn derive_kek(password: &str, salt: &[u8]) -> [u8; 32] {
    if password.is_empty() {
        warn!("Deriving KEK from an empty password — this is insecure");
    }

    info!("Deriving 256-bit KEK via HKDF-SHA256");

    // HKDF-Extract: PRK = HMAC-SHA256(salt, IKM)
    // HKDF-Expand:  OKM = HMAC-SHA256(PRK, info || counter)
    let hk = Hkdf::<Sha256>::new(Some(salt), password.as_bytes());
    let mut okm = [0u8; KEY_SIZE];

    // 32 bytes is well within SHA256's HKDF max output (255 * 32 = 8 160 B),
    // so expand() cannot fail for this length.
    hk.expand(HKDF_INFO, &mut okm)
        .expect("HKDF-SHA256 expand to 32 bytes is infallible");

    okm
}

/// Generate a cryptographically random 256-bit Data Encryption Key (DEK).
///
/// Uses `OsRng` (the operating system's CSPRNG) for generation. The DEK is
/// used to encrypt field data via [`encrypt_field`]. It should be wrapped
/// (encrypted) by the KEK via [`wrap_dek`] before being stored.
///
/// # Returns
///
/// A fresh `[u8; 32]` DEK.
pub fn generate_dek() -> [u8; 32] {
    let mut dek = [0u8; KEY_SIZE];
    OsRng.fill_bytes(&mut dek);
    debug!("Generated 256-bit DEK");
    dek
}

/// Generate a cryptographically random 32-byte salt for KEK derivation.
///
/// The salt is **not secret** — it should be stored in the `encryption_keys`
/// table alongside the wrapped DEK so the KEK can be reconstructed.
pub fn generate_salt() -> Vec<u8> {
    let mut salt = vec![0u8; SALT_SIZE];
    OsRng.fill_bytes(&mut salt);
    debug!("Generated {}-byte random salt", SALT_SIZE);
    salt
}

// ---------------------------------------------------------------------------
// Internal: low-level AES-256-GCM encrypt/decrypt of raw bytes
// ---------------------------------------------------------------------------

/// Encrypt arbitrary bytes with AES-256-GCM.
///
/// Returns `nonce(12) || ciphertext || tag(16)`. The nonce is freshly random
/// per call. This is the shared internal primitive used by both
/// [`encrypt_field`] (for string data) and [`wrap_dek`] (for key material).
///
/// # Panics
///
/// Panics if AES-256-GCM encryption fails. With a valid 32-byte key and a
/// 12-byte nonce this is provably infallible — the only failure mode is an
/// allocation error, which would abort the process anyway.
fn encrypt_bytes(plaintext: &[u8], key: &[u8; 32]) -> Vec<u8> {
    // Generate a fresh random nonce for each encryption — NEVER reuse.
    let mut nonce_bytes = [0u8; NONCE_SIZE];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .expect("AES-256-GCM encryption is infallible with valid 32-byte key");

    // Prepend nonce so the complete blob is self-contained.
    let mut output = Vec::with_capacity(NONCE_SIZE + ciphertext.len());
    output.extend_from_slice(&nonce_bytes);
    output.extend_from_slice(&ciphertext);

    output
}

/// Decrypt a blob produced by [`encrypt_bytes`].
///
/// Splits the nonce (first 12 bytes) from ciphertext+tag (remainder), verifies
/// the GCM tag, and returns the plaintext bytes on success.
///
/// # Errors
///
/// - [`EncryptionError::InvalidCiphertext`] — blob is shorter than 28 bytes.
/// - [`EncryptionError::Decrypt`] — GCM tag verification failed (wrong key,
///   tampered nonce, or corrupted ciphertext).
fn decrypt_bytes(blob: &[u8], key: &[u8; 32]) -> Result<Vec<u8>, EncryptionError> {
    if blob.len() < MIN_BLOB_LEN {
        return Err(EncryptionError::InvalidCiphertext(blob.len(), MIN_BLOB_LEN));
    }

    let (nonce_bytes, ct) = blob.split_at(NONCE_SIZE);
    let nonce = Nonce::from_slice(nonce_bytes);

    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));

    cipher
        .decrypt(nonce, ct)
        .map_err(|e| {
            warn!("AES-256-GCM decryption failed (tag verification or data corruption)");
            EncryptionError::Decrypt(e.to_string())
        })
}

// ---------------------------------------------------------------------------
// DEK wrapping (KEK encrypts DEK)
// ---------------------------------------------------------------------------

/// Wrap (encrypt) a Data Encryption Key with a Key Encryption Key using
/// AES-256-GCM.
///
/// The wrapped DEK is stored in the `encryption_keys` table. The blob format
/// is `nonce(12) || ciphertext(32) || tag(16)` = 60 bytes total.
///
/// # Arguments
///
/// * `dek` — The 256-bit DEK to wrap.
/// * `kek` — The 256-bit KEK derived from [`derive_kek`].
///
/// # Returns
///
/// A 60-byte blob containing the wrapped DEK.
///
/// # Security Notes
///
/// - A fresh random 96-bit nonce is generated for each call.
/// - Neither the DEK, KEK, nor nonce is ever logged.
pub fn wrap_dek(dek: &[u8; 32], kek: &[u8; 32]) -> Vec<u8> {
    let wrapped = encrypt_bytes(dek, kek);
    debug!("DEK wrapped: {} bytes", wrapped.len());
    wrapped
}

/// Unwrap (decrypt) a wrapped Data Encryption Key using a Key Encryption Key.
///
/// # Arguments
///
/// * `wrapped` — The blob from [`wrap_dek`] (`nonce || ciphertext || tag`).
/// * `kek` — The 256-bit KEK derived from [`derive_kek`].
///
/// # Returns
///
/// The decrypted 256-bit DEK on success.
///
/// # Errors
///
/// - [`EncryptionError::InvalidCiphertext`] — blob is too short.
/// - [`EncryptionError::Decrypt`] — GCM tag verification failed (wrong KEK
///   or tampered blob).
/// - [`EncryptionError::UnwrapFailed`] — decrypted blob is not exactly
///   32 bytes (should not occur if the blob was produced by [`wrap_dek`]).
pub fn unwrap_dek(wrapped: &[u8], kek: &[u8; 32]) -> Result<[u8; 32], EncryptionError> {
    let dek_bytes = decrypt_bytes(wrapped, kek)?;

    if dek_bytes.len() != KEY_SIZE {
        return Err(EncryptionError::UnwrapFailed(format!(
            "Unwrapped DEK is {} bytes, expected {}",
            dek_bytes.len(),
            KEY_SIZE
        )));
    }

    let mut dek = [0u8; KEY_SIZE];
    dek.copy_from_slice(&dek_bytes);

    debug!("DEK unwrapped successfully");
    Ok(dek)
}

// ---------------------------------------------------------------------------
// Field-level encryption (DEK encrypts data)
// ---------------------------------------------------------------------------

/// Encrypt a plaintext string field using AES-256-GCM with the Data Encryption
/// Key.
///
/// # Arguments
///
/// * `plaintext` — The sensitive data to encrypt (e.g. bank account number,
///   SSN, routing number).
/// * `dek` — 256-bit DEK from [`generate_dek`] or [`unwrap_dek`].
///
/// # Returns
///
/// A byte vector containing `nonce(12) || ciphertext || tag(16)`. Store this
/// blob directly in the SQLite data table.
///
/// # Security Notes
///
/// - A fresh random 96-bit nonce is generated for each call — nonces are
///   **never** reused with the same key.
/// - The nonce is prepended to the ciphertext so the blob is self-contained.
/// - Neither the DEK, nonce, nor plaintext is ever logged.
pub fn encrypt_field(plaintext: &str, dek: &[u8; 32]) -> Vec<u8> {
    let blob = encrypt_bytes(plaintext.as_bytes(), dek);
    debug!("Field encrypted: {} bytes output blob", blob.len());
    blob
}

/// Decrypt a ciphertext blob to a plaintext string using the Data Encryption
/// Key.
///
/// # Arguments
///
/// * `blob` — The blob from [`encrypt_field`] (`nonce || ciphertext || tag`).
/// * `dek` — 256-bit DEK (must match the key used for encryption).
///
/// # Returns
///
/// The decrypted plaintext string on success.
///
/// # Errors
///
/// - [`EncryptionError::InvalidCiphertext`] — blob is shorter than 28 bytes.
/// - [`EncryptionError::Decrypt`] — GCM tag verification failed (wrong DEK,
///   tampered blob) or the decrypted bytes are not valid UTF-8.
///
/// # Security Notes
///
/// - Decryption failure does not reveal whether the key was wrong or the data
///   was tampered — GCM treats both identically.
/// - Neither the DEK, nonce, nor plaintext is ever logged.
pub fn decrypt_field(blob: &[u8], dek: &[u8; 32]) -> Result<String, EncryptionError> {
    let plaintext_bytes = decrypt_bytes(blob, dek)?;

    let plaintext = String::from_utf8(plaintext_bytes).map_err(|e| {
        warn!("Decrypted field is not valid UTF-8");
        EncryptionError::Decrypt(format!("invalid UTF-8 in plaintext: {}", e))
    })?;

    debug!("Field decrypted: {} bytes input blob", blob.len());
    Ok(plaintext)
}

// ---------------------------------------------------------------------------
// Password change (re-wrap DEK, no data re-encryption)
// ---------------------------------------------------------------------------

/// Change the user's password by re-wrapping the DEK with a new KEK.
///
/// This is the key benefit of the DEK/KEK architecture: changing the password
/// does **not** require re-encrypting any field data. The process is:
///
/// 1. Derive the old KEK from `old_password` + `salt`.
/// 2. Unwrap the DEK using the old KEK.
/// 3. Derive the new KEK from `new_password` + `salt`.
/// 4. Re-wrap the DEK with the new KEK.
/// 5. Return the new wrapped DEK (store it in `encryption_keys`).
///
/// All intermediate key material (old KEK, DEK, new KEK) is wrapped in
/// [`SecureKey`] and automatically zeroized from memory when the function
/// returns.
///
/// # Arguments
///
/// * `old_password` — The current user password.
/// * `new_password` — The new user password.
/// * `salt` — The salt stored in `encryption_keys` (same salt for old and new
///   KEK derivation).
/// * `wrapped_dek` — The currently stored wrapped DEK blob.
///
/// # Returns
///
/// The new wrapped DEK blob on success. Store this in the `encryption_keys`
/// table, replacing the old wrapped DEK.
///
/// # Errors
///
/// - [`EncryptionError::Decrypt`] — the old password is wrong (old KEK fails
///   to unwrap the DEK).
/// - [`EncryptionError::InvalidCiphertext`] — the wrapped DEK blob is
///   malformed.
/// - [`EncryptionError::UnwrapFailed`] — the unwrapped DEK is not 32 bytes.
pub fn change_password(
    old_password: &str,
    new_password: &str,
    salt: &[u8],
    wrapped_dek: &[u8],
) -> Result<Vec<u8>, EncryptionError> {
    info!("Re-wrapping DEK for password change");

    // 1. Derive old KEK and unwrap DEK.
    let old_kek = SecureKey::new(derive_kek(old_password, salt));
    let dek = SecureKey::new(unwrap_dek(wrapped_dek, old_kek.as_bytes())?);

    // 2. Derive new KEK and re-wrap DEK.
    let new_kek = SecureKey::new(derive_kek(new_password, salt));
    let new_wrapped = wrap_dek(dek.as_bytes(), new_kek.as_bytes());

    // old_kek, dek, and new_kek are automatically zeroized on drop
    // (via #[derive(Zeroize)] + #[zeroize(drop)] on SecureKey).
    info!("DEK re-wrapped successfully for password change");
    Ok(new_wrapped)
}

// ---------------------------------------------------------------------------
// FieldEncryptor — legacy backward-compatible API
// ---------------------------------------------------------------------------

/// Legacy field-level encryptor using AES-256-GCM.
///
/// This struct exists for backward compatibility with code that uses the
/// pre-key-wrapping API (associated functions on a namespace struct). All
/// methods delegate to the free functions above.
///
/// **New code should call the free functions directly** (`derive_kek`,
/// `generate_dek`, `wrap_dek`, `unwrap_dek`, `encrypt_field`, `decrypt_field`,
/// `change_password`) instead of using `FieldEncryptor`.
#[derive(Debug, Clone)]
pub struct FieldEncryptor;

impl FieldEncryptor {
    /// Generate a cryptographically random 32-byte salt.
    ///
    /// Delegates to [`generate_salt`].
    pub fn generate_salt() -> Vec<u8> {
        generate_salt()
    }

    /// Derive a 256-bit key from a user password and salt using HKDF-SHA256.
    ///
    /// Delegates to [`derive_kek`]. The derived key can be used as a KEK for
    /// [`wrap_dek`] / [`unwrap_dek`], or directly as an encryption key for
    /// [`Self::encrypt_field`] / [`Self::decrypt_field`] in legacy code paths
    /// that have not yet migrated to the DEK/KEK architecture.
    pub fn derive_key(password: &str, salt: &[u8]) -> [u8; 32] {
        derive_kek(password, salt)
    }

    /// Encrypt a plaintext string using AES-256-GCM.
    ///
    /// Accepts a `&[u8]` slice for backward compatibility. The key must be
    /// exactly 32 bytes.
    ///
    /// # Errors
    ///
    /// - [`EncryptionError::InvalidKeyLength`] — key is not 32 bytes.
    pub fn encrypt_field(plaintext: &str, key: &[u8]) -> Result<Vec<u8>, EncryptionError> {
        let key_array: [u8; 32] = key
            .try_into()
            .map_err(|_| EncryptionError::InvalidKeyLength(key.len()))?;
        Ok(encrypt_field(plaintext, &key_array))
    }

    /// Decrypt a ciphertext blob to a plaintext string.
    ///
    /// Accepts a `&[u8]` slice for backward compatibility. The key must be
    /// exactly 32 bytes.
    ///
    /// # Errors
    ///
    /// - [`EncryptionError::InvalidKeyLength`] — key is not 32 bytes.
    /// - Decryption errors from [`decrypt_field`].
    pub fn decrypt_field(ciphertext: &[u8], key: &[u8]) -> Result<String, EncryptionError> {
        let key_array: [u8; 32] = key
            .try_into()
            .map_err(|_| EncryptionError::InvalidKeyLength(key.len()))?;
        decrypt_field(ciphertext, &key_array)
    }

    /// Encrypt a JSON value using AES-256-GCM.
    ///
    /// Serializes the JSON value to a compact string and encrypts it. The key
    /// must be exactly 32 bytes.
    ///
    /// # Errors
    ///
    /// - [`EncryptionError::Json`] — JSON serialization failed.
    /// - [`EncryptionError::InvalidKeyLength`] — key is not 32 bytes.
    pub fn encrypt_json(
        value: &serde_json::Value,
        key: &[u8],
    ) -> Result<Vec<u8>, EncryptionError> {
        let json_str = serde_json::to_string(value)?;
        debug!("JSON value serialized for encryption");
        Self::encrypt_field(&json_str, key)
    }

    /// Decrypt a ciphertext blob to a JSON value.
    ///
    /// # Errors
    ///
    /// - [`EncryptionError::InvalidKeyLength`] — key is not 32 bytes.
    /// - Decryption errors from [`decrypt_field`].
    /// - [`EncryptionError::Json`] — JSON deserialization failed.
    pub fn decrypt_json(
        ciphertext: &[u8],
        key: &[u8],
    ) -> Result<serde_json::Value, EncryptionError> {
        let json_str = Self::decrypt_field(ciphertext, key)?;
        let value: serde_json::Value = serde_json::from_str(&json_str)?;
        debug!("JSON value deserialized after decryption");
        Ok(value)
    }
}

// ---------------------------------------------------------------------------
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // =====================================================================
    // Test helpers
    // =====================================================================

    /// Derive a throwaway test KEK from a random salt.
    fn test_kek() -> [u8; 32] {
        let salt = generate_salt();
        derive_kek("test-password-12345", &salt)
    }

    /// Derive a test KEK with a fixed salt (for deterministic tests).
    fn test_kek_fixed() -> [u8; 32] {
        derive_kek("test-password-12345", &[1, 2, 3, 4])
    }

    // =====================================================================
    // 1. Encrypt / decrypt round-trip
    // =====================================================================

    #[test]
    fn test_encrypt_decrypt_round_trip() {
        let dek = generate_dek();
        let plaintext = "sensitive-bank-account-123456789";
        let ciphertext = encrypt_field(plaintext, &dek);
        let decrypted = decrypt_field(&ciphertext, &dek).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_empty_plaintext_round_trip() {
        let dek = generate_dek();
        let plaintext = "";
        let ciphertext = encrypt_field(plaintext, &dek);
        // Even empty plaintext produces nonce(12) + tag(16) = 28 bytes.
        assert_eq!(ciphertext.len(), MIN_BLOB_LEN);
        let decrypted = decrypt_field(&ciphertext, &dek).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_large_text_round_trip() {
        let dek = generate_dek();
        let plaintext = "A".repeat(10_000);
        let ciphertext = encrypt_field(&plaintext, &dek);
        // 10 000 bytes plaintext → nonce(12) + ciphertext(10 000) + tag(16)
        assert_eq!(ciphertext.len(), NONCE_SIZE + 10_000 + GCM_TAG_SIZE);
        let decrypted = decrypt_field(&ciphertext, &dek).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_unicode_text_round_trip() {
        let dek = generate_dek();
        let plaintext = "日本語テスト — Эmöji: 🚀💰📊 — العربية — Ελληνικά";
        let ciphertext = encrypt_field(plaintext, &dek);
        let decrypted = decrypt_field(&ciphertext, &dek).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_single_character_round_trip() {
        let dek = generate_dek();
        let plaintext = "X";
        let ciphertext = encrypt_field(plaintext, &dek);
        let decrypted = decrypt_field(&ciphertext, &dek).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_bank_account_number_round_trip() {
        let dek = generate_dek();
        let plaintext = "1234567890123456";
        let ciphertext = encrypt_field(plaintext, &dek);
        let decrypted = decrypt_field(&ciphertext, &dek).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_routing_number_round_trip() {
        let dek = generate_dek();
        let plaintext = "021000021";
        let ciphertext = encrypt_field(plaintext, &dek);
        let decrypted = decrypt_field(&ciphertext, &dek).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_ssn_round_trip() {
        let dek = generate_dek();
        let plaintext = "123-45-6789";
        let ciphertext = encrypt_field(plaintext, &dek);
        let decrypted = decrypt_field(&ciphertext, &dek).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_credit_card_number_round_trip() {
        let dek = generate_dek();
        let plaintext = "4532-1234-5678-9010";
        let ciphertext = encrypt_field(plaintext, &dek);
        let decrypted = decrypt_field(&ciphertext, &dek).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_tax_id_round_trip() {
        let dek = generate_dek();
        let plaintext = "12-3456789";
        let ciphertext = encrypt_field(plaintext, &dek);
        let decrypted = decrypt_field(&ciphertext, &dek).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    // =====================================================================
    // 2. Wrong key fails decryption
    // =====================================================================

    #[test]
    fn test_wrong_dek_fails_decryption() {
        let salt = generate_salt();
        let kek1 = derive_kek("password-correct", &salt);
        let kek2 = derive_kek("password-wrong", &salt);

        let dek = generate_dek();
        let wrapped = wrap_dek(&dek, &kek1);

        // Unwrapping with the wrong KEK must fail.
        let result = unwrap_dek(&wrapped, &kek2);
        assert!(result.is_err(), "unwrap with wrong KEK must fail");
        match result {
            Err(EncryptionError::Decrypt(_)) => {}
            Err(e) => panic!("expected Decrypt error, got {:?}", e),
            Ok(_) => panic!("expected error, got Ok"),
        }
    }

    #[test]
    fn test_wrong_dek_fails_field_decryption() {
        let dek1 = generate_dek();
        let dek2 = generate_dek();
        let plaintext = "secret-data-12345";
        let ciphertext = encrypt_field(plaintext, &dek1);

        let result = decrypt_field(&ciphertext, &dek2);
        assert!(result.is_err(), "decryption with wrong DEK must fail");
        match result {
            Err(EncryptionError::Decrypt(_)) => {}
            Err(e) => panic!("expected Decrypt error, got {:?}", e),
            Ok(_) => panic!("expected error, got Ok"),
        }
    }

    // =====================================================================
    // 3. Key wrapping round-trip
    // =====================================================================

    #[test]
    fn test_wrap_unwrap_round_trip() {
        let kek = test_kek();
        let dek = generate_dek();
        let wrapped = wrap_dek(&dek, &kek);
        let unwrapped = unwrap_dek(&wrapped, &kek).unwrap();
        assert_eq!(unwrapped, dek, "unwrapped DEK must match original");
    }

    #[test]
    fn test_wrapped_dek_is_60_bytes() {
        let kek = test_kek();
        let dek = generate_dek();
        let wrapped = wrap_dek(&dek, &kek);
        // nonce(12) + ciphertext(32) + tag(16) = 60
        assert_eq!(wrapped.len(), NONCE_SIZE + KEY_SIZE + GCM_TAG_SIZE);
    }

    #[test]
    fn test_wrapped_dek_does_not_contain_plaintext_dek() {
        let kek = test_kek();
        let dek = generate_dek();
        let wrapped = wrap_dek(&dek, &kek);
        // The wrapped DEK must not contain the raw DEK bytes in plaintext.
        assert!(
            !wrapped[NONCE_SIZE..].windows(32).any(|w| w == dek),
            "wrapped DEK must not contain plaintext DEK bytes"
        );
    }

    #[test]
    fn test_wrap_unwrap_with_empty_salt() {
        let kek = derive_kek("password", &[]);
        let dek = generate_dek();
        let wrapped = wrap_dek(&dek, &kek);
        let unwrapped = unwrap_dek(&wrapped, &kek).unwrap();
        assert_eq!(unwrapped, dek);
    }

    // =====================================================================
    // 4. Password change preserves data
    // =====================================================================

    #[test]
    fn test_password_change_preserves_data() {
        let salt = generate_salt();
        let old_password = "old-secure-password-123";
        let new_password = "new-secure-password-456";

        // Initial setup: derive KEK, generate DEK, wrap it.
        let old_kek = derive_kek(old_password, &salt);
        let dek = generate_dek();
        let wrapped_dek = wrap_dek(&dek, &old_kek);

        // Encrypt some field data with the DEK.
        let plaintext = "1234567890123456";
        let ciphertext = encrypt_field(plaintext, &dek);

        // Change password — re-wrap DEK.
        let new_wrapped = change_password(old_password, new_password, &salt, &wrapped_dek)
            .expect("password change should succeed");

        // The new wrapped DEK must differ from the old one.
        assert_ne!(
            new_wrapped, wrapped_dek,
            "re-wrapped DEK must differ from old wrapped DEK"
        );

        // Unwrap with new KEK to recover the DEK.
        let new_kek = derive_kek(new_password, &salt);
        let recovered_dek = unwrap_dek(&new_wrapped, &new_kek).expect("unwrap should succeed");

        // The recovered DEK must match the original — data is preserved.
        assert_eq!(
            recovered_dek, dek,
            "recovered DEK must match original DEK after password change"
        );

        // The ciphertext can still be decrypted with the recovered DEK.
        let decrypted = decrypt_field(&ciphertext, &recovered_dek).unwrap();
        assert_eq!(
            decrypted, plaintext,
            "field data must be decryptable after password change"
        );
    }

    #[test]
    fn test_password_change_with_wrong_old_password_fails() {
        let salt = generate_salt();
        let old_password = "correct-old-password";
        let wrong_old_password = "wrong-old-password";
        let new_password = "new-password";

        let old_kek = derive_kek(old_password, &salt);
        let dek = generate_dek();
        let wrapped_dek = wrap_dek(&dek, &old_kek);

        let result = change_password(wrong_old_password, new_password, &salt, &wrapped_dek);
        assert!(
            result.is_err(),
            "password change with wrong old password must fail"
        );
    }

    #[test]
    fn test_password_change_no_data_re_encryption_needed() {
        // Verify that the DEK itself does not change during password change —
        // only the wrapping changes. This is the core property that makes
        // password changes O(1) regardless of how many fields are encrypted.
        let salt = generate_salt();

        let old_kek = derive_kek("old-pw", &salt);
        let dek = generate_dek();
        let wrapped = wrap_dek(&dek, &old_kek);

        // Encrypt multiple fields with the DEK.
        let ct1 = encrypt_field("field-1-data", &dek);
        let ct2 = encrypt_field("field-2-data", &dek);
        let ct3 = encrypt_field("field-3-data", &dek);

        // Change password.
        let new_wrapped = change_password("old-pw", "new-pw", &salt, &wrapped).unwrap();

        // Unwrap with new KEK — same DEK.
        let new_kek = derive_kek("new-pw", &salt);
        let recovered_dek = unwrap_dek(&new_wrapped, &new_kek).unwrap();
        assert_eq!(recovered_dek, dek);

        // All fields still decrypt correctly — no re-encryption was needed.
        assert_eq!(decrypt_field(&ct1, &recovered_dek).unwrap(), "field-1-data");
        assert_eq!(decrypt_field(&ct2, &recovered_dek).unwrap(), "field-2-data");
        assert_eq!(decrypt_field(&ct3, &recovered_dek).unwrap(), "field-3-data");
    }

    // =====================================================================
    // 5. Nonce uniqueness (encrypt same text twice → different ciphertexts)
    // =====================================================================

    #[test]
    fn test_nonce_uniqueness_same_plaintext_different_ciphertexts() {
        let dek = test_kek_fixed();
        let plaintext = "same-plaintext-value";
        let ct1 = encrypt_field(plaintext, &dek);
        let ct2 = encrypt_field(plaintext, &dek);
        let ct3 = encrypt_field(plaintext, &dek);

        // All three should decrypt to the same plaintext.
        assert_eq!(decrypt_field(&ct1, &dek).unwrap(), plaintext);
        assert_eq!(decrypt_field(&ct2, &dek).unwrap(), plaintext);
        assert_eq!(decrypt_field(&ct3, &dek).unwrap(), plaintext);

        // But the ciphertext blobs must differ (random nonce per call).
        assert_ne!(ct1, ct2, "two encryptions of the same plaintext must differ");
        assert_ne!(ct1, ct3);
        assert_ne!(ct2, ct3);
    }

    #[test]
    fn test_nonce_bytes_are_unique() {
        let dek = test_kek_fixed();
        let plaintext = "test";

        // Encrypt many times and collect the nonces (first 12 bytes).
        let mut nonces: Vec<[u8; NONCE_SIZE]> = Vec::new();
        for _ in 0..100 {
            let ct = encrypt_field(plaintext, &dek);
            let mut nonce = [0u8; NONCE_SIZE];
            nonce.copy_from_slice(&ct[..NONCE_SIZE]);
            nonces.push(nonce);
        }

        // Check that all 100 nonces are unique.
        let mut sorted = nonces.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(
            sorted.len(),
            nonces.len(),
            "all 100 nonces must be unique (found duplicates)"
        );
    }

    #[test]
    fn test_wrap_dek_nonce_uniqueness() {
        let kek = test_kek_fixed();
        let dek = generate_dek();

        let mut nonces: Vec<[u8; NONCE_SIZE]> = Vec::new();
        for _ in 0..50 {
            let wrapped = wrap_dek(&dek, &kek);
            let mut nonce = [0u8; NONCE_SIZE];
            nonce.copy_from_slice(&wrapped[..NONCE_SIZE]);
            nonces.push(nonce);
        }

        let mut sorted = nonces.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), nonces.len(), "all wrap nonces must be unique");
    }

    // =====================================================================
    // KEK derivation tests
    // =====================================================================

    #[test]
    fn test_derive_kek_produces_32_bytes() {
        let salt = generate_salt();
        let kek = derive_kek("my-password", &salt);
        assert_eq!(kek.len(), 32);
    }

    #[test]
    fn test_derive_kek_is_deterministic_with_same_inputs() {
        let salt = vec![1, 2, 3, 4];
        let kek1 = derive_kek("same-password", &salt);
        let kek2 = derive_kek("same-password", &salt);
        assert_eq!(kek1, kek2, "same password + salt must produce same KEK");
    }

    #[test]
    fn test_derive_kek_different_passwords_produce_different_keks() {
        let salt = vec![1, 2, 3, 4];
        let kek1 = derive_kek("password-1", &salt);
        let kek2 = derive_kek("password-2", &salt);
        assert_ne!(kek1, kek2);
    }

    #[test]
    fn test_derive_kek_different_salts_produce_different_keks() {
        let salt1 = vec![1, 2, 3, 4];
        let salt2 = vec![5, 6, 7, 8];
        let kek1 = derive_kek("same-password", &salt1);
        let kek2 = derive_kek("same-password", &salt2);
        assert_ne!(kek1, kek2);
    }

    #[test]
    fn test_derive_kek_empty_salt() {
        let kek = derive_kek("password", &[]);
        assert_eq!(kek.len(), 32);
    }

    // =====================================================================
    // DEK generation tests
    // =====================================================================

    #[test]
    fn test_generate_dek_produces_32_bytes() {
        let dek = generate_dek();
        assert_eq!(dek.len(), 32);
    }

    #[test]
    fn test_generate_dek_is_random() {
        let dek1 = generate_dek();
        let dek2 = generate_dek();
        let dek3 = generate_dek();
        assert_ne!(dek1, dek2, "DEKs must be random");
        assert_ne!(dek1, dek3);
        assert_ne!(dek2, dek3);
    }

    // =====================================================================
    // Salt generation tests
    // =====================================================================

    #[test]
    fn test_generate_salt_produces_32_bytes() {
        let salt = generate_salt();
        assert_eq!(salt.len(), 32);
    }

    #[test]
    fn test_generate_salt_is_random() {
        let salt1 = generate_salt();
        let salt2 = generate_salt();
        let salt3 = generate_salt();
        assert_ne!(salt1, salt2, "salts must be random");
        assert_ne!(salt1, salt3);
        assert_ne!(salt2, salt3);
    }

    // =====================================================================
    // Sensitive fields tests
    // =====================================================================

    #[test]
    fn test_sensitive_fields_list() {
        assert!(is_sensitive_field("bank_account_number"));
        assert!(is_sensitive_field("routing_number"));
        assert!(is_sensitive_field("tax_id"));
        assert!(is_sensitive_field("ssn"));
        assert!(is_sensitive_field("credit_card_number"));
    }

    #[test]
    fn test_non_sensitive_fields() {
        assert!(!is_sensitive_field("account_name"));
        assert!(!is_sensitive_field("description"));
        assert!(!is_sensitive_field("amount"));
        assert!(!is_sensitive_field("date"));
        assert!(!is_sensitive_field(""));
    }

    #[test]
    fn test_sensitive_fields_count() {
        assert_eq!(SENSITIVE_FIELDS.len(), 5);
    }

    // =====================================================================
    // Tamper detection tests
    // =====================================================================

    #[test]
    fn test_tampered_ciphertext_fails() {
        let dek = generate_dek();
        let mut ciphertext = encrypt_field("secret-data", &dek);
        // Flip a bit in the ciphertext portion (after the nonce).
        let last = ciphertext.len() - 1;
        ciphertext[last] ^= 0xFF;
        let result = decrypt_field(&ciphertext, &dek);
        assert!(matches!(result, Err(EncryptionError::Decrypt(_))));
    }

    #[test]
    fn test_tampered_nonce_fails() {
        let dek = generate_dek();
        let mut ciphertext = encrypt_field("secret-data", &dek);
        // Flip a bit in the nonce (first byte).
        ciphertext[0] ^= 0xFF;
        let result = decrypt_field(&ciphertext, &dek);
        assert!(matches!(result, Err(EncryptionError::Decrypt(_))));
    }

    #[test]
    fn test_tampered_tag_fails() {
        let dek = generate_dek();
        let mut ciphertext = encrypt_field("secret-data", &dek);
        // Flip a bit in the tag (last 16 bytes).
        let tag_start = ciphertext.len() - GCM_TAG_SIZE;
        ciphertext[tag_start] ^= 0x01;
        let result = decrypt_field(&ciphertext, &dek);
        assert!(matches!(result, Err(EncryptionError::Decrypt(_))));
    }

    #[test]
    fn test_tampered_wrapped_dek_fails() {
        let kek = test_kek();
        let dek = generate_dek();
        let mut wrapped = wrap_dek(&dek, &kek);
        // Flip a bit in the ciphertext portion.
        let last = wrapped.len() - 1;
        wrapped[last] ^= 0xFF;
        let result = unwrap_dek(&wrapped, &kek);
        assert!(matches!(result, Err(EncryptionError::Decrypt(_))));
    }

    // =====================================================================
    // Truncated / invalid ciphertext tests
    // =====================================================================

    #[test]
    fn test_truncated_ciphertext_fails() {
        let dek = generate_dek();
        let ciphertext = encrypt_field("secret", &dek);
        // Truncate to 10 bytes (< MIN_BLOB_LEN of 28).
        let truncated = &ciphertext[..10];
        let result = decrypt_field(truncated, &dek);
        assert!(matches!(result, Err(EncryptionError::InvalidCiphertext(10, _))));
    }

    #[test]
    fn test_empty_ciphertext_fails() {
        let dek = generate_dek();
        let result = decrypt_field(&[], &dek);
        assert!(matches!(result, Err(EncryptionError::InvalidCiphertext(0, _))));
    }

    #[test]
    fn test_ciphertext_exactly_27_bytes_fails() {
        let dek = generate_dek();
        let fake_ct = vec![0u8; 27];
        let result = decrypt_field(&fake_ct, &dek);
        assert!(matches!(result, Err(EncryptionError::InvalidCiphertext(27, _))));
    }

    #[test]
    fn test_ciphertext_exactly_28_bytes_fails_tag_verification() {
        let dek = generate_dek();
        // 28 bytes passes length check but should fail GCM tag verification.
        let fake_ct = vec![0u8; 28];
        let result = decrypt_field(&fake_ct, &dek);
        assert!(matches!(result, Err(EncryptionError::Decrypt(_))));
    }

    #[test]
    fn test_truncated_wrapped_dek_fails() {
        let kek = test_kek();
        let result = unwrap_dek(&[0u8; 10], &kek);
        assert!(matches!(result, Err(EncryptionError::InvalidCiphertext(10, _))));
    }

    // =====================================================================
    // SecureKey tests
    // =====================================================================

    #[test]
    fn test_secure_key_as_bytes() {
        let key = [0xAB; 32];
        let secure = SecureKey::new(key);
        assert_eq!(secure.as_bytes(), &key);
    }

    #[test]
    fn test_secure_key_debug_does_not_leak() {
        let key = [0x42; 32];
        let secure = SecureKey::new(key);
        let debug_str = format!("{:?}", secure);
        assert!(debug_str.contains("REDACTED"), "Debug must not leak key bytes");
        assert!(
            !debug_str.contains("42"),
            "Debug must not contain any key byte values"
        );
    }

    // =====================================================================
    // Multiple fields independent encryption
    // =====================================================================

    #[test]
    fn test_multiple_fields_independent_encryption() {
        let dek = generate_dek();

        let account_number = "1234567890123456";
        let routing_number = "021000021";
        let tax_id = "12-3456789";

        let ct_acct = encrypt_field(account_number, &dek);
        let ct_routing = encrypt_field(routing_number, &dek);
        let ct_tax = encrypt_field(tax_id, &dek);

        // All ciphertexts should be different (different nonces).
        assert_ne!(ct_acct, ct_routing);
        assert_ne!(ct_acct, ct_tax);
        assert_ne!(ct_routing, ct_tax);

        // Each decrypts to its original value.
        assert_eq!(decrypt_field(&ct_acct, &dek).unwrap(), account_number);
        assert_eq!(decrypt_field(&ct_routing, &dek).unwrap(), routing_number);
        assert_eq!(decrypt_field(&ct_tax, &dek).unwrap(), tax_id);
    }

    // =====================================================================
    // Full lifecycle integration test
    // =====================================================================

    #[test]
    fn test_full_lifecycle() {
        // 1. Generate salt and derive KEK from password.
        let salt = generate_salt();
        let kek = derive_kek("user-master-password", &salt);

        // 2. Generate DEK and wrap it.
        let dek = generate_dek();
        let wrapped_dek = wrap_dek(&dek, &kek);

        // 3. Encrypt all sensitive fields with the DEK.
        let fields = vec![
            ("bank_account_number", "1234567890123456"),
            ("routing_number", "021000021"),
            ("tax_id", "12-3456789"),
            ("ssn", "123-45-6789"),
            ("credit_card_number", "4532-1234-5678-9010"),
        ];

        let encrypted_fields: Vec<(&str, Vec<u8>)> = fields
            .iter()
            .map(|(name, value)| (*name, encrypt_field(value, &dek)))
            .collect();

        // 4. Simulate a session restart: re-derive KEK, unwrap DEK.
        let kek_again = derive_kek("user-master-password", &salt);
        let dek_again = unwrap_dek(&wrapped_dek, &kek_again).unwrap();
        assert_eq!(dek_again, dek, "recovered DEK must match original");

        // 5. Decrypt all fields — must match originals.
        for ((name, original), (_, ciphertext)) in fields.iter().zip(encrypted_fields.iter()) {
            let decrypted = decrypt_field(ciphertext, &dek_again).unwrap();
            assert_eq!(
                &decrypted, original,
                "field '{}' round-trip failed",
                name
            );
        }

        // 6. Change password — re-wrap DEK.
        let new_wrapped = change_password(
            "user-master-password",
            "new-master-password",
            &salt,
            &wrapped_dek,
        )
        .unwrap();

        // 7. Unwrap with new KEK — same DEK, all fields still decrypt.
        let new_kek = derive_kek("new-master-password", &salt);
        let recovered_dek = unwrap_dek(&new_wrapped, &new_kek).unwrap();
        assert_eq!(recovered_dek, dek);

        for ((name, original), (_, ciphertext)) in fields.iter().zip(encrypted_fields.iter()) {
            let decrypted = decrypt_field(ciphertext, &recovered_dek).unwrap();
            assert_eq!(
                &decrypted, original,
                "field '{}' must still decrypt after password change",
                name
            );
        }
    }

    // =====================================================================
    // Backward compatibility: FieldEncryptor API
    // =====================================================================

    #[test]
    fn test_field_encryptor_encrypt_decrypt_round_trip() {
        let salt = FieldEncryptor::generate_salt();
        let key = FieldEncryptor::derive_key("password", &salt);
        let plaintext = "1234567890123456";
        let ciphertext = FieldEncryptor::encrypt_field(plaintext, &key).unwrap();
        let decrypted = FieldEncryptor::decrypt_field(&ciphertext, &key).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_field_encryptor_encrypt_decrypt_json_round_trip() {
        let salt = FieldEncryptor::generate_salt();
        let key = FieldEncryptor::derive_key("password", &salt);
        let value = serde_json::json!({
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
    fn test_field_encryptor_wrong_key_fails() {
        let salt = FieldEncryptor::generate_salt();
        let key1 = FieldEncryptor::derive_key("password-correct", &salt);
        let key2 = FieldEncryptor::derive_key("password-wrong", &salt);
        let ciphertext = FieldEncryptor::encrypt_field("secret", &key1).unwrap();
        assert!(FieldEncryptor::decrypt_field(&ciphertext, &key2).is_err());
    }

    #[test]
    fn test_field_encryptor_invalid_key_length() {
        assert!(matches!(
            FieldEncryptor::encrypt_field("test", &[0u8; 16]),
            Err(EncryptionError::InvalidKeyLength(16))
        ));
        assert!(matches!(
            FieldEncryptor::encrypt_field("test", &[0u8; 64]),
            Err(EncryptionError::InvalidKeyLength(64))
        ));
        assert!(matches!(
            FieldEncryptor::decrypt_field(&[0u8; 28], &[0u8; 16]),
            Err(EncryptionError::InvalidKeyLength(16))
        ));
    }

    #[test]
    fn test_field_encryptor_derive_key_matches_derive_kek() {
        let salt = vec![1, 2, 3, 4];
        let key = FieldEncryptor::derive_key("password", &salt);
        let kek = derive_kek("password", &salt);
        assert_eq!(key, kek, "FieldEncryptor::derive_key must match derive_kek");
    }

    #[test]
    fn test_field_encryptor_ciphertext_not_contains_plaintext() {
        let salt = FieldEncryptor::generate_salt();
        let key = FieldEncryptor::derive_key("password", &salt);
        let plaintext = "1234567890123456";
        let ciphertext = FieldEncryptor::encrypt_field(plaintext, &key).unwrap();
        let ct_str = String::from_utf8_lossy(&ciphertext);
        assert!(
            !ct_str.contains(plaintext),
            "encrypted data must not contain plaintext"
        );
    }

    // =====================================================================
    // Ciphertext format tests
    // =====================================================================

    #[test]
    fn test_ciphertext_starts_with_nonce() {
        let dek = generate_dek();
        let ciphertext = encrypt_field("test", &dek);
        assert!(ciphertext.len() > NONCE_SIZE);
        let _nonce = &ciphertext[..NONCE_SIZE];
        let _ct_and_tag = &ciphertext[NONCE_SIZE..];
    }

    #[test]
    fn test_min_ciphertext_length_for_empty_plaintext() {
        let dek = generate_dek();
        let ciphertext = encrypt_field("", &dek);
        assert_eq!(ciphertext.len(), MIN_BLOB_LEN);
    }

    #[test]
    fn test_ciphertext_length_grows_with_plaintext() {
        let dek = generate_dek();
        let ct_short = encrypt_field("short", &dek);
        let ct_long = encrypt_field("this is a much longer plaintext value", &dek);
        // Both have nonce(12) + tag(16) = 28 bytes overhead.
        assert_eq!(ct_short.len(), NONCE_SIZE + 5 + GCM_TAG_SIZE);
        assert_eq!(ct_long.len(), NONCE_SIZE + 37 + GCM_TAG_SIZE);
    }

    // =====================================================================
    // Cross-encrypt: string then decrypt as different key (should fail)
    // =====================================================================

    #[test]
    fn test_decrypt_with_unwrap_dek_key_fails() {
        // Encrypt a field with one DEK, then try to use that field's ciphertext
        // as a wrapped DEK for a different KEK — must fail.
        let kek = test_kek();
        let dek = generate_dek();
        let field_ct = encrypt_field("not-a-dek", &dek);

        // The field ciphertext is not a valid wrapped DEK — it should fail
        // either at the GCM tag verification (wrong key) or at the length check.
        let result = unwrap_dek(&field_ct, &kek);
        assert!(result.is_err(), "field ciphertext must not unwrap as DEK");
    }
}
