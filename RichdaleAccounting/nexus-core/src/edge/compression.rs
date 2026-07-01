//! Edge Compression — CRC32-protected lz4 blob compression for offline SQLite storage.
//!
//! Provides transparent compression of large blobs (receipt images, invoice
//! PDFs, JSON metadata) before they are written to the local SQLite database.
//! Every blob carries a CRC32 checksum of the original uncompressed data so
//! that corruption — whether from disk errors, truncated writes, or bit-flips
//! in storage — is detected before the data is handed back to the caller.
//!
//! # Blob Format
//!
//! ```text
//! +----------+-------------------+----------------------------------+
//! | 1 byte   | 4 bytes (LE)      | variable                         |
//! | flag     | CRC32 checksum    | compressed or uncompressed data  |
//! +----------+-------------------+----------------------------------+
//! ```
//!
//! - **Flag 0** — uncompressed: the data segment is the original bytes.
//! - **Flag 1** — compressed: the data segment is lz4-block-compressed bytes.
//!
//! The CRC32 is always computed over the **original (uncompressed)** data,
//! regardless of whether compression was applied. On decompression the CRC32
//! is recomputed from the recovered bytes and compared; a mismatch means the
//! data was corrupted and [`decompress_blob`] returns
//! [`CompressionError::ChecksumMismatch`] without exposing the bad data.
//!
//! # When Compression Is Skipped
//!
//! [`compress_blob`] attempts lz4 compression on every call. If the
//! compressed form is not strictly smaller than the original, the blob is
//! stored uncompressed (flag 0). This naturally handles tiny inputs (where
//! lz4 framing overhead exceeds any savings) and high-entropy data (where
//! compression yields no benefit).

use std::sync::atomic::{AtomicU64, Ordering};

use once_cell::sync::Lazy;
use thiserror::Error;
use tracing::{debug, warn};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Flag byte indicating the payload is stored uncompressed.
const FLAG_UNCOMPRESSED: u8 = 0;

/// Flag byte indicating the payload is lz4-compressed.
const FLAG_COMPRESSED: u8 = 1;

/// Header size: 1-byte flag + 4-byte CRC32 checksum.
const HEADER_LEN: usize = 5;

// ---------------------------------------------------------------------------
// Global statistics counters (thread-safe)
// ---------------------------------------------------------------------------

/// Running total of original (pre-compression) bytes across all
/// [`compress_blob`] calls.
static ORIGINAL_BYTES: Lazy<AtomicU64> = Lazy::new(|| AtomicU64::new(0));

/// Running total of stored blob bytes (header + payload) across all
/// [`compress_blob`] calls.
static COMPRESSED_BYTES: Lazy<AtomicU64> = Lazy::new(|| AtomicU64::new(0));

/// Running count of blobs processed by [`compress_blob`].
static BLOB_COUNT: Lazy<AtomicU64> = Lazy::new(|| AtomicU64::new(0));

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors that can occur during compression or decompression.
#[derive(Error, Debug)]
pub enum CompressionError {
    /// The blob is too short to contain a valid header, or the flag byte
    /// contains an unrecognized value.
    #[error("Invalid blob format: too short or unknown flag")]
    InvalidFormat,

    /// The CRC32 of the recovered data does not match the checksum stored in
    /// the blob header. The data has been corrupted and must not be used.
    #[error("CRC32 checksum mismatch — data may be corrupted")]
    ChecksumMismatch,

    /// The lz4 decoder returned an error while decompressing the payload.
    #[error("Decompression error: {0}")]
    DecompressionError(String),
}

// ---------------------------------------------------------------------------
// CompressionStats
// ---------------------------------------------------------------------------

/// Cumulative statistics for blob compression operations.
///
/// Snapshots are read atomically via [`get_stats`]. Because the underlying
/// counters are separate `AtomicU64` values, the three fields are not
/// guaranteed to be from the same instant in time — they are eventually
/// consistent. This is acceptable for monitoring and reporting purposes.
#[derive(Debug, Clone)]
pub struct CompressionStats {
    /// Total bytes of original (uncompressed) data passed to [`compress_blob`].
    pub original_bytes: u64,

    /// Total bytes of stored blobs (header + payload) produced by
    /// [`compress_blob`].
    pub compressed_bytes: u64,

    /// Number of blobs processed by [`compress_blob`].
    pub blob_count: u64,
}

impl CompressionStats {
    /// Compression ratio (`compressed_bytes / original_bytes`).
    ///
    /// A value below `1.0` means the combined blobs occupy less space than
    /// the original data. Returns `0.0` when no data has been compressed yet
    /// (avoids division by zero).
    pub fn ratio(&self) -> f64 {
        if self.original_bytes == 0 {
            return 0.0;
        }
        self.compressed_bytes as f64 / self.original_bytes as f64
    }
}

// ---------------------------------------------------------------------------
// Core functions
// ---------------------------------------------------------------------------

/// Compress a data blob with CRC32 integrity protection.
///
/// Attempts lz4 block compression. If the compressed form is **not strictly
/// smaller** than the original (`compressed.len() >= data.len()`), the
/// original bytes are stored verbatim with flag 0. Otherwise the compressed
/// bytes are stored with flag 1.
///
/// A CRC32 checksum of the **original** data is always prepended (as a
/// 4-byte little-endian integer) so that [`decompress_blob`] can verify
/// integrity after recovery.
///
/// This function never fails — if lz4 compression returns an error, the data
/// is simply stored uncompressed. The returned `Vec<u8>` is always a valid,
/// self-describing blob.
///
/// # Format
///
/// ```text
/// [1-byte flag][4-byte CRC32 (LE)][compressed or raw data]
/// ```
///
/// # Example
///
/// ```
/// # use nexus_core::edge::compression::{compress_blob, decompress_blob};
/// let data = b"hello, world!".repeat(1000);
/// let blob = compress_blob(&data);
/// let recovered = decompress_blob(&blob).unwrap();
/// assert_eq!(recovered, data);
/// ```
pub fn compress_blob(data: &[u8]) -> Vec<u8> {
    let crc = crc32fast::hash(data);

    // Attempt lz4 block compression. On error, fall back to uncompressed.
    let compressed = lz4::block::compress(data, None, false).ok();

    let (flag, payload): (u8, &[u8]) = match compressed {
        Some(ref c) if c.len() < data.len() => (FLAG_COMPRESSED, c.as_slice()),
        _ => (FLAG_UNCOMPRESSED, data),
    };

    let mut blob = Vec::with_capacity(HEADER_LEN + payload.len());
    blob.push(flag);
    blob.extend_from_slice(&crc.to_le_bytes());
    blob.extend_from_slice(payload);

    // Update global stats.
    ORIGINAL_BYTES.fetch_add(data.len() as u64, Ordering::Relaxed);
    COMPRESSED_BYTES.fetch_add(blob.len() as u64, Ordering::Relaxed);
    BLOB_COUNT.fetch_add(1, Ordering::Relaxed);

    if flag == FLAG_COMPRESSED {
        debug!(
            original_len = data.len(),
            blob_len = blob.len(),
            "Blob compressed (lz4)"
        );
    } else {
        debug!(
            original_len = data.len(),
            blob_len = blob.len(),
            "Blob stored uncompressed (lz4 not beneficial)"
        );
    }

    blob
}

/// Decompress a blob produced by [`compress_blob`].
///
/// Reads the flag byte and CRC32 checksum from the header, recovers the
/// original data (decompressing with lz4 if flag 1, copying verbatim if
/// flag 0), then verifies that the CRC32 of the recovered data matches the
/// stored checksum.
///
/// **Corrupted data is never returned.** If the CRC32 does not match,
/// [`CompressionError::ChecksumMismatch`] is returned and the recovered
/// bytes are discarded.
///
/// # Errors
///
/// - [`CompressionError::InvalidFormat`] — blob shorter than the 5-byte
///   header, or flag byte is not 0 or 1.
/// - [`CompressionError::DecompressionError`] — lz4 decompression of a
///   flag-1 payload failed (the compressed data is structurally invalid).
/// - [`CompressionError::ChecksumMismatch`] — CRC32 of the recovered data
///   does not match the stored checksum.
///
/// # Example
///
/// ```
/// # use nexus_core::edge::compression::{compress_blob, decompress_blob};
/// let blob = compress_blob(b"some data");
/// let original = decompress_blob(&blob).unwrap();
/// assert_eq!(original, b"some data");
/// ```
pub fn decompress_blob(blob: &[u8]) -> Result<Vec<u8>, CompressionError> {
    if blob.len() < HEADER_LEN {
        return Err(CompressionError::InvalidFormat);
    }

    let flag = blob[0];
    let crc = u32::from_le_bytes([blob[1], blob[2], blob[3], blob[4]]);
    let payload = &blob[HEADER_LEN..];

    let original = match flag {
        FLAG_UNCOMPRESSED => payload.to_vec(),
        FLAG_COMPRESSED => {
            lz4::block::decompress(payload, None)
                .map_err(|e| CompressionError::DecompressionError(e.to_string()))?
        }
        _ => {
            warn!(flag, "Unknown compression flag in blob header");
            return Err(CompressionError::InvalidFormat);
        }
    };

    // Verify CRC32 — never expose corrupted data.
    let computed_crc = crc32fast::hash(&original);
    if computed_crc != crc {
        warn!(
            stored_crc = crc,
            computed_crc = computed_crc,
            "CRC32 checksum mismatch — blob may be corrupted"
        );
        return Err(CompressionError::ChecksumMismatch);
    }

    Ok(original)
}

/// Return a snapshot of cumulative compression statistics.
///
/// The counters are thread-safe (`AtomicU64` with `Relaxed` ordering) and
/// accumulate across all calls to [`compress_blob`] for the lifetime of the
/// process.
pub fn get_stats() -> CompressionStats {
    CompressionStats {
        original_bytes: ORIGINAL_BYTES.load(Ordering::Relaxed),
        compressed_bytes: COMPRESSED_BYTES.load(Ordering::Relaxed),
        blob_count: BLOB_COUNT.load(Ordering::Relaxed),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // =====================================================================
    // Round-trip: compress → decompress → byte-for-byte match
    // =====================================================================

    #[test]
    fn test_round_trip() {
        // Highly compressible data — exercises the flag=1 (compressed) path.
        let data = vec![0x42_u8; 10_000];
        let blob = compress_blob(&data);
        let recovered = decompress_blob(&blob).expect("decompression must succeed");
        assert_eq!(
            recovered, data,
            "Round-trip must be byte-for-byte identical"
        );
    }

    #[test]
    fn test_round_trip_empty() {
        let data: Vec<u8> = vec![];
        let blob = compress_blob(&data);
        let recovered = decompress_blob(&blob).expect("empty round-trip must succeed");
        assert_eq!(recovered, data);
    }

    #[test]
    fn test_round_trip_json_metadata() {
        // Simulates a JSON metadata blob stored in SQLite.
        let json = r#"{"invoice_id":"INV-2026-0001","vendor":"Acme Corp","line_items":[{"sku":"WIDGET-001","qty":42,"unit_price":"12.50"},{"sku":"WIDGET-002","qty":17,"unit_price":"8.75"}],"subtotal":"648.75","tax_rate":"0.08","tax_amount":"51.90","total":"700.65","currency":"USD","due_date":"2026-07-15"}"#;
        let data = json.as_bytes().repeat(30); // ~9 KB, highly compressible
        let blob = compress_blob(&data);
        let recovered = decompress_blob(&blob).expect("JSON round-trip must succeed");
        assert_eq!(recovered, data);
    }

    #[test]
    fn test_round_trip_mixed_binary() {
        // Structured header + pseudo-random binary payload (receipt-like).
        let header = b"{\"type\":\"receipt\",\"format\":\"png\",\"vendor\":\"Staples\"}";
        let mut data = header.to_vec();
        data.extend((0..8_000u32).map(|i| (i % 251) as u8));
        let blob = compress_blob(&data);
        let recovered = decompress_blob(&blob).expect("binary round-trip must succeed");
        assert_eq!(recovered, data);
    }

    // =====================================================================
    // Large blob: 10 KB → compressed < original → decompress → match
    // =====================================================================

    #[test]
    fn test_large_blob_compresses_smaller() {
        let data = vec![0xAB_u8; 10_000];
        let blob = compress_blob(&data);

        assert!(
            blob.len() < data.len(),
            "Compressed blob ({} bytes) must be smaller than original ({} bytes)",
            blob.len(),
            data.len()
        );

        // Verify the flag indicates compression.
        assert_eq!(blob[0], FLAG_COMPRESSED, "Large compressible blob must use flag=1");

        let recovered = decompress_blob(&blob).expect("decompression must succeed");
        assert_eq!(recovered, data, "Decompressed data must match original");
    }

    // =====================================================================
    // Small blob: 10 bytes → stored uncompressed (flag=0)
    // =====================================================================

    #[test]
    fn test_small_blob_stored_uncompressed() {
        let data = vec![0x01_u8; 10];
        let blob = compress_blob(&data);

        assert_eq!(
            blob[0],
            FLAG_UNCOMPRESSED,
            "10-byte blob must be stored uncompressed (flag=0)"
        );

        // Total size = 5-byte header + 10 bytes data.
        assert_eq!(blob.len(), HEADER_LEN + 10);

        let recovered = decompress_blob(&blob).expect("small blob round-trip must succeed");
        assert_eq!(recovered, data);
    }

    // =====================================================================
    // Corruption: flip a byte → ChecksumMismatch
    // =====================================================================

    #[test]
    fn test_corruption_detection() {
        let data = vec![0x42_u8; 10_000];
        let mut blob = compress_blob(&data);

        // Flip a byte in the CRC32 checksum field (byte index 1).
        // The payload is intact, so decompression succeeds, but the stored
        // CRC32 no longer matches the computed one.
        blob[1] ^= 0xFF;

        let result = decompress_blob(&blob);
        assert!(result.is_err(), "Corrupted blob must produce an error");
        assert!(
            matches!(result, Err(CompressionError::ChecksumMismatch)),
            "Expected ChecksumMismatch, got {:?}",
            result
        );
    }

    #[test]
    fn test_corruption_data_byte_uncompressed() {
        // Use a small blob stored uncompressed (flag=0), then corrupt a
        // data byte. The CRC32 of the modified data won't match.
        let data = vec![0x77_u8; 50];
        let mut blob = compress_blob(&data);
        assert_eq!(blob[0], FLAG_UNCOMPRESSED);

        // Flip the first data byte (index 5, right after the 5-byte header).
        blob[HEADER_LEN] ^= 0x01;

        let result = decompress_blob(&blob);
        assert!(result.is_err());
        assert!(
            matches!(result, Err(CompressionError::ChecksumMismatch)),
            "Expected ChecksumMismatch, got {:?}",
            result
        );
    }

    #[test]
    fn test_corruption_data_byte_compressed() {
        // Corrupt a byte in the compressed payload of a flag=1 blob.
        // lz4 may either fail (DecompressionError) or produce different
        // output (ChecksumMismatch) — either way, no valid data is returned.
        let data = vec![0x55_u8; 10_000];
        let mut blob = compress_blob(&data);
        assert_eq!(blob[0], FLAG_COMPRESSED, "precondition: blob must be compressed");

        // Flip a byte in the compressed payload.
        let payload_start = HEADER_LEN;
        blob[payload_start] ^= 0xFF;

        let result = decompress_blob(&blob);
        assert!(
            result.is_err(),
            "Corrupted compressed payload must produce an error"
        );
        // Accept either DecompressionError or ChecksumMismatch — both
        // correctly prevent corrupted data from being returned.
        assert!(
            matches!(
                result,
                Err(CompressionError::ChecksumMismatch) | Err(CompressionError::DecompressionError(_))
            ),
            "Expected ChecksumMismatch or DecompressionError, got {:?}",
            result
        );
    }

    // =====================================================================
    // Skip-if-larger: 5 bytes → output is uncompressed
    // =====================================================================

    #[test]
    fn test_skip_if_larger() {
        let data = vec![0x01_u8; 5];
        let blob = compress_blob(&data);

        assert_eq!(
            blob[0],
            FLAG_UNCOMPRESSED,
            "5-byte blob must be stored uncompressed (compression would enlarge it)"
        );

        // Verify round-trip still works.
        let recovered = decompress_blob(&blob).expect("round-trip must succeed");
        assert_eq!(recovered, data);
    }

    // =====================================================================
    // Compression stats: 3 blobs → count >= 3, ratio < 1.0
    // =====================================================================

    #[test]
    fn test_compression_stats() {
        let before = get_stats();

        // Three large, highly compressible blobs.
        let data1 = vec![0xAB_u8; 10_000];
        let data2 = vec![0xCD_u8; 20_000];
        let data3 = vec![0xEF_u8; 15_000];

        let blob1 = compress_blob(&data1);
        let blob2 = compress_blob(&data2);
        let blob3 = compress_blob(&data3);

        let after = get_stats();

        // Blob count must have increased by at least 3 (other tests may have
        // added more in parallel).
        assert!(
            after.blob_count >= before.blob_count + 3,
            "Blob count should increase by at least 3 (before={}, after={})",
            before.blob_count,
            after.blob_count
        );

        // Each of the three blobs must be compressed (flag=1).
        assert_eq!(blob1[0], FLAG_COMPRESSED, "blob1 should be compressed");
        assert_eq!(blob2[0], FLAG_COMPRESSED, "blob2 should be compressed");
        assert_eq!(blob3[0], FLAG_COMPRESSED, "blob3 should be compressed");

        // The overall ratio must be below 1.0 — the three large compressible
        // blobs dominate any small uncompressed blobs from other tests.
        let ratio = after.ratio();
        assert!(
            ratio < 1.0,
            "Overall compression ratio should be < 1.0, got {:.4}",
            ratio
        );
    }

    // =====================================================================
    // Error cases
    // =====================================================================

    #[test]
    fn test_decompress_too_short() {
        let result = decompress_blob(&[0u8; 4]);
        assert!(result.is_err());
        assert!(
            matches!(result, Err(CompressionError::InvalidFormat)),
            "Expected InvalidFormat for 4-byte blob, got {:?}",
            result
        );
    }

    #[test]
    fn test_decompress_empty() {
        let result = decompress_blob(&[]);
        assert!(result.is_err());
        assert!(
            matches!(result, Err(CompressionError::InvalidFormat)),
            "Expected InvalidFormat for empty blob, got {:?}",
            result
        );
    }

    #[test]
    fn test_decompress_invalid_flag() {
        // 5-byte header with flag=2 (invalid), valid-looking CRC32, no data.
        let blob = [0x02, 0x00, 0x00, 0x00, 0x00];
        let result = decompress_blob(&blob);
        assert!(result.is_err());
        assert!(
            matches!(result, Err(CompressionError::InvalidFormat)),
            "Expected InvalidFormat for unknown flag, got {:?}",
            result
        );
    }

    #[test]
    fn test_decompress_truncated_payload() {
        // Compress real data, then truncate the payload.
        let data = vec![0x42_u8; 10_000];
        let blob = compress_blob(&data);
        let truncated = &blob[..HEADER_LEN + 3]; // keep header + 3 bytes of payload

        let result = decompress_blob(truncated);
        assert!(
            result.is_err(),
            "Truncated payload must produce an error"
        );
    }

    // =====================================================================
    // CompressionStats::ratio
    // =====================================================================

    #[test]
    fn test_stats_ratio_zero_bytes() {
        let stats = CompressionStats {
            original_bytes: 0,
            compressed_bytes: 0,
            blob_count: 0,
        };
        assert_eq!(stats.ratio(), 0.0);
    }

    #[test]
    fn test_stats_ratio_compressed() {
        let stats = CompressionStats {
            original_bytes: 1000,
            compressed_bytes: 300,
            blob_count: 5,
        };
        assert!((stats.ratio() - 0.3).abs() < 1e-9);
    }

    #[test]
    fn test_stats_ratio_no_savings() {
        let stats = CompressionStats {
            original_bytes: 500,
            compressed_bytes: 500,
            blob_count: 1,
        };
        assert!((stats.ratio() - 1.0).abs() < 1e-9);
    }

    // =====================================================================
    // Edge cases
    // =====================================================================

    #[test]
    fn test_single_byte() {
        let data = vec![0x7F_u8];
        let blob = compress_blob(&data);
        assert_eq!(blob[0], FLAG_UNCOMPRESSED, "Single byte must be uncompressed");
        let recovered = decompress_blob(&blob).expect("single byte round-trip");
        assert_eq!(recovered, data);
    }

    #[test]
    fn test_all_zeros_large() {
        let data = vec![0u8; 100_000];
        let blob = compress_blob(&data);
        assert_eq!(blob[0], FLAG_COMPRESSED, "Large all-zeros must compress");
        assert!(
            blob.len() < data.len(),
            "100 KB of zeros should compress well (blob={} bytes)",
            blob.len()
        );
        let recovered = decompress_blob(&blob).expect("all-zeros round-trip");
        assert_eq!(recovered, data);
    }

    #[test]
    fn test_high_entropy_data_may_skip_compression() {
        // Pseudo-random data that is unlikely to compress. The blob may be
        // stored uncompressed (flag=0), but the round-trip must still work.
        let data: Vec<u8> = (0..5_000u32)
            .map(|i| {
                let mut x = i.wrapping_mul(2654435761);
                x ^= x >> 13;
                x ^= x << 7;
                x ^= x >> 17;
                (x & 0xFF) as u8
            })
            .collect();

        let blob = compress_blob(&data);
        let recovered = decompress_blob(&blob).expect("high-entropy round-trip");
        assert_eq!(recovered, data, "Round-trip must succeed regardless of compression");
    }

    #[test]
    fn test_multiple_distinct_blobs() {
        // Compress several different blobs and ensure each round-trips.
        let blobs_data: Vec<Vec<u8>> = vec![
            vec![0xAA; 5_000],
            b"Hello, NexusLedger!".repeat(200),
            (0..3_000u32).map(|i| (i % 256) as u8).collect(),
            vec![],
            vec![0xFF; 1],
        ];

        for data in &blobs_data {
            let blob = compress_blob(data);
            let recovered = decompress_blob(&blob).expect("round-trip must succeed");
            assert_eq!(recovered, *data, "round-trip failed for data of len {}", data.len());
        }
    }

    #[test]
    fn test_header_structure() {
        // Verify the exact header layout: [flag][crc32 LE 4 bytes][data].
        let data = vec![0x11_u8; 10];
        let blob = compress_blob(&data);

        // Header is 5 bytes.
        assert_eq!(blob.len(), HEADER_LEN + data.len());

        // Flag byte.
        assert_eq!(blob[0], FLAG_UNCOMPRESSED);

        // CRC32 stored as little-endian.
        let expected_crc = crc32fast::hash(&data);
        let stored_crc = u32::from_le_bytes([blob[1], blob[2], blob[3], blob[4]]);
        assert_eq!(stored_crc, expected_crc, "CRC32 must be stored as LE in header");

        // Payload follows header.
        assert_eq!(&blob[HEADER_LEN..], &data[..]);
    }

    #[test]
    fn test_crc_is_of_original_data() {
        // For a compressed blob, the CRC32 must match the ORIGINAL data,
        // not the compressed payload.
        let data = vec![0x99_u8; 10_000];
        let blob = compress_blob(&data);
        assert_eq!(blob[0], FLAG_COMPRESSED, "precondition: must be compressed");

        let stored_crc = u32::from_le_bytes([blob[1], blob[2], blob[3], blob[4]]);
        let original_crc = crc32fast::hash(&data);
        let payload_crc = crc32fast::hash(&blob[HEADER_LEN..]);

        assert_eq!(stored_crc, original_crc, "CRC32 must be of original data");
        assert_ne!(
            stored_crc, payload_crc,
            "CRC32 must NOT be of compressed payload"
        );
    }
}
