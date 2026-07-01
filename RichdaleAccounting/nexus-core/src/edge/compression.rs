//! Edge Compression
//!
//! lz4 blob compression for local storage optimization.
//! Compresses large document blobs (receipt images, invoice PDFs, document
//! content, JSON metadata) before storing them in the local SQLite database.
//!
//! # Overview
//!
//! - [`BlobCompressor`] provides static methods for compressing and decompressing
//!   binary blobs using the lz4 algorithm.
//! - [`CompressedBlob`] wraps a (possibly compressed) blob with metadata about
//!   the original and compressed sizes.
//!
//! # Storage Format
//!
//! Compressed blobs carry an 8-byte little-endian header containing the original
//! uncompressed size, followed by the lz4-compressed payload. This allows
//! [`decompress_blob`](BlobCompressor::decompress_blob) to work without the
//! caller knowing the original size beforehand.
//!
//! # When to Compress
//!
//! [`should_compress`](BlobCompressor::should_compress) returns `true` for data
//! larger than 256 bytes. [`compress_if_beneficial`](BlobCompressor::compress_if_beneficial)
//! goes one step further: it only keeps the compressed form when it is actually
//! smaller than the original, falling back to raw storage otherwise.

use thiserror::Error;
use tracing::{debug, info, warn};

/// Minimum data size (in bytes) for compression to be considered.
const COMPRESSION_THRESHOLD: usize = 256;

/// Size of the original-size header prepended to compressed data (u64 LE).
const SIZE_HEADER_LEN: usize = 8;

/// Error type for compression operations.
#[derive(Error, Debug)]
pub enum CompressionError {
    /// Compression failed.
    #[error("Compression failed: {0}")]
    Compress(String),
    /// Decompression failed.
    #[error("Decompression failed: {0}")]
    Decompress(String),
    /// The input data is malformed or incomplete.
    #[error("Invalid data: {0}")]
    InvalidData(String),
}

/// A potentially compressed blob with metadata.
///
/// Produced by [`BlobCompressor::compress_if_beneficial`]. The `data` field
/// holds either the compressed payload (when `is_compressed` is `true`) or the
/// raw original bytes (when `is_compressed` is `false`). Use
/// [`decompress`](Self::decompress) to always recover the original bytes
/// regardless of which form was stored.
#[derive(Debug, Clone)]
pub struct CompressedBlob {
    /// The (possibly compressed) data.
    pub data: Vec<u8>,
    /// Whether compression was applied.
    pub is_compressed: bool,
    /// Original uncompressed size in bytes.
    pub original_size: usize,
    /// Compressed size in bytes (equals `original_size` if not compressed).
    pub compressed_size: usize,
}

impl CompressedBlob {
    /// Get the compression ratio (original / compressed).
    ///
    /// Returns `1.0` if the blob was not compressed or if the compressed size
    /// is zero (avoiding division by zero).
    pub fn ratio(&self) -> f64 {
        if !self.is_compressed || self.compressed_size == 0 {
            return 1.0;
        }
        self.original_size as f64 / self.compressed_size as f64
    }

    /// Get space savings in bytes (original − compressed).
    ///
    /// Returns `0` when the blob was not compressed.
    pub fn savings(&self) -> usize {
        self.original_size.saturating_sub(self.compressed_size)
    }

    /// Decompress this blob back to the original bytes.
    ///
    /// If the blob was not compressed, returns a clone of the stored data
    /// unchanged. This is the recommended way to recover the original payload
    /// from a [`CompressedBlob`] since it handles both compressed and
    /// uncompressed forms transparently.
    pub fn decompress(&self) -> Result<Vec<u8>, CompressionError> {
        if self.is_compressed {
            BlobCompressor::decompress_blob(&self.data)
        } else {
            Ok(self.data.clone())
        }
    }
}

/// Static utility for lz4 blob compression.
///
/// All methods are associated functions — `BlobCompressor` is never instantiated.
pub struct BlobCompressor;

impl BlobCompressor {
    /// Compress data using lz4.
    ///
    /// Prepends an 8-byte little-endian header with the original size so that
    /// [`decompress_blob`](Self::decompress_blob) can recover the data without
    /// external size information.
    ///
    /// # Errors
    ///
    /// Returns [`CompressionError::Compress`] if the underlying lz4 encoder
    /// fails (rare — typically only on allocation failure).
    pub fn compress_blob(data: &[u8]) -> Result<Vec<u8>, CompressionError> {
        let compressed = lz4::compress(data)
            .map_err(|e| CompressionError::Compress(e.to_string()))?;

        let mut result = Vec::with_capacity(SIZE_HEADER_LEN + compressed.len());
        result.extend_from_slice(&(data.len() as u64).to_le_bytes());
        result.extend_from_slice(&compressed);

        debug!(
            original_size = data.len(),
            compressed_size = compressed.len(),
            "Compressed blob"
        );

        Ok(result)
    }

    /// Decompress lz4-compressed data produced by [`compress_blob`](Self::compress_blob).
    ///
    /// Reads the 8-byte size header to determine the expected output length,
    /// then decompresses the remaining payload. The decompressed size is
    /// validated against the header value.
    ///
    /// # Errors
    ///
    /// - [`CompressionError::InvalidData`] if the input is too short to contain
    ///   a header.
    /// - [`CompressionError::Decompress`] if the lz4 decoder fails or the
    ///   output size does not match the header.
    pub fn decompress_blob(data: &[u8]) -> Result<Vec<u8>, CompressionError> {
        if data.len() < SIZE_HEADER_LEN {
            return Err(CompressionError::InvalidData(format!(
                "Data too short: got {} bytes, need at least {} for the size header",
                data.len(),
                SIZE_HEADER_LEN
            )));
        }

        let (header, payload) = data.split_at(SIZE_HEADER_LEN);
        let original_size = u64::from_le_bytes(
            header
                .try_into()
                .map_err(|_| CompressionError::InvalidData("Failed to read size header".to_string()))?,
        ) as usize;

        let decompressed = lz4::decompress(payload, original_size)
            .map_err(|e| CompressionError::Decompress(e.to_string()))?;

        if decompressed.len() != original_size {
            warn!(
                expected = original_size,
                actual = decompressed.len(),
                "Decompressed size mismatch"
            );
            return Err(CompressionError::Decompress(format!(
                "Size mismatch: expected {}, got {}",
                original_size,
                decompressed.len()
            )));
        }

        Ok(decompressed)
    }

    /// Calculate compression ratio (original / compressed).
    ///
    /// A ratio of `2.0` means the compressed data is half the size of the
    /// original. Returns `1.0` if the compressed size is zero to avoid
    /// division by zero.
    pub fn compression_ratio(original: &[u8], compressed: &[u8]) -> f64 {
        if compressed.is_empty() {
            return 1.0;
        }
        original.len() as f64 / compressed.len() as f64
    }

    /// Check if data is large enough to benefit from compression.
    ///
    /// Returns `true` when `data.len() > 256`. Below this threshold the lz4
    /// header and framing overhead typically negates any savings.
    pub fn should_compress(data: &[u8]) -> bool {
        data.len() > COMPRESSION_THRESHOLD
    }

    /// Compress only if beneficial, otherwise return the original data.
    ///
    /// Compression is attempted when [`should_compress`](Self::should_compress)
    /// returns `true`. The compressed form is kept only when it is strictly
    /// smaller than the original; otherwise the raw bytes are stored and
    /// `is_compressed` is set to `false`.
    ///
    /// This is the recommended entry point for callers that simply want to
    /// store a blob optimally — the returned [`CompressedBlob`] can later be
    /// passed to [`CompressedBlob::decompress`] to recover the original data
    /// regardless of whether compression was applied.
    pub fn compress_if_beneficial(data: &[u8]) -> Result<CompressedBlob, CompressionError> {
        let original_size = data.len();

        // Step 1: skip compression entirely for small data.
        if !Self::should_compress(data) {
            debug!(
                size = original_size,
                threshold = COMPRESSION_THRESHOLD,
                "Data below compression threshold, storing uncompressed"
            );
            return Ok(CompressedBlob {
                data: data.to_vec(),
                is_compressed: false,
                original_size,
                compressed_size: original_size,
            });
        }

        // Step 2: attempt compression.
        let compressed = Self::compress_blob(data)?;
        let compressed_size = compressed.len();

        // Step 3: keep compressed form only if it is actually smaller.
        if compressed_size >= original_size {
            debug!(
                original_size,
                compressed_size,
                "Compression not beneficial (compressed >= original), storing uncompressed"
            );
            return Ok(CompressedBlob {
                data: data.to_vec(),
                is_compressed: false,
                original_size,
                compressed_size: original_size,
            });
        }

        let ratio = Self::compression_ratio(data, &compressed);
        info!(
            original_size,
            compressed_size,
            ratio = format!("{:.2}", ratio),
            "Data compressed successfully"
        );

        Ok(CompressedBlob {
            data: compressed,
            is_compressed: true,
            original_size,
            compressed_size,
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // =====================================================================
    // Round-trip tests — compressed data must decompress bit-for-bit.
    // =====================================================================

    #[test]
    fn test_round_trip_large_repetitive_data() {
        let data = vec![0xAB_u8; 10_000];
        let compressed = BlobCompressor::compress_blob(&data).unwrap();
        let decompressed = BlobCompressor::decompress_blob(&compressed).unwrap();
        assert_eq!(decompressed, data, "Round-trip must be bit-for-bit identical");
    }

    #[test]
    fn test_round_trip_random_binary() {
        // Deterministic pseudo-random data (no external RNG dependency).
        let data: Vec<u8> = (0..5_000u32).map(|i| (i.wrapping_mul(7).wrapping_add(13) & 0xFF) as u8).collect();
        let compressed = BlobCompressor::compress_blob(&data).unwrap();
        let decompressed = BlobCompressor::decompress_blob(&compressed).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_round_trip_json_text() {
        let json = r#"{"invoice_id":"INV-2026-0001","vendor":"Acme Corp","line_items":[{"sku":"WIDGET-001","qty":42,"unit_price":"12.50"},{"sku":"WIDGET-002","qty":17,"unit_price":"8.75"}],"subtotal":"648.75","tax_rate":"0.08","tax_amount":"51.90","total":"700.65","currency":"USD","due_date":"2026-07-15"}"#;
        // Repeat to push the blob above the compression threshold.
        let large: Vec<u8> = json.as_bytes().repeat(20);
        let compressed = BlobCompressor::compress_blob(&large).unwrap();
        let decompressed = BlobCompressor::decompress_blob(&compressed).unwrap();
        assert_eq!(decompressed, large);
    }

    #[test]
    fn test_round_trip_empty_data() {
        let data: Vec<u8> = vec![];
        let compressed = BlobCompressor::compress_blob(&data).unwrap();
        let decompressed = BlobCompressor::decompress_blob(&compressed).unwrap();
        assert_eq!(decompressed, data);
        // Empty data should produce at least the 8-byte header.
        assert!(compressed.len() >= SIZE_HEADER_LEN);
    }

    #[test]
    fn test_round_trip_receipt_like_content() {
        // Mix of structured JSON header + binary payload — mimics a stored
        // receipt with embedded metadata.
        let header = b"{\"type\":\"receipt\",\"format\":\"png\",\"size\":4096,\"vendor\":\"Staples\",\"date\":\"2026-06-30\"}";
        let mut data = header.to_vec();
        // Deterministic binary pattern with some compressibility.
        data.extend((0..4096u32).map(|i| (i % 251) as u8));

        let compressed = BlobCompressor::compress_blob(&data).unwrap();
        let decompressed = BlobCompressor::decompress_blob(&compressed).unwrap();
        assert_eq!(decompressed, data);
    }

    // =====================================================================
    // should_compress threshold
    // =====================================================================

    #[test]
    fn test_should_compress_threshold() {
        assert!(!BlobCompressor::should_compress(&[]));
        assert!(!BlobCompressor::should_compress(&vec![0u8; 100]));
        assert!(!BlobCompressor::should_compress(&vec![0u8; 256]), "256 bytes is not > 256");
        assert!(BlobCompressor::should_compress(&vec![0u8; 257]), "257 bytes is > 256");
        assert!(BlobCompressor::should_compress(&vec![0u8; 1_000_000]));
    }

    // =====================================================================
    // compress_if_beneficial
    // =====================================================================

    #[test]
    fn test_compress_if_beneficial_small_data() {
        let data = vec![0x42u8; 100];
        let blob = BlobCompressor::compress_if_beneficial(&data).unwrap();
        assert!(!blob.is_compressed, "Small data should not be compressed");
        assert_eq!(blob.original_size, 100);
        assert_eq!(blob.compressed_size, 100);
        assert_eq!(blob.data, data);
        assert_eq!(blob.ratio(), 1.0);
        assert_eq!(blob.savings(), 0);
    }

    #[test]
    fn test_compress_if_beneficial_empty() {
        let blob = BlobCompressor::compress_if_beneficial(&[]).unwrap();
        assert!(!blob.is_compressed);
        assert_eq!(blob.original_size, 0);
        assert_eq!(blob.compressed_size, 0);
        assert!(blob.data.is_empty());
    }

    #[test]
    fn test_compress_if_beneficial_large_repetitive() {
        let data = vec![0xAAu8; 50_000];
        let blob = BlobCompressor::compress_if_beneficial(&data).unwrap();
        assert!(blob.is_compressed, "Large repetitive data should compress");
        assert!(blob.compressed_size < blob.original_size);
        assert!(blob.ratio() > 1.0);
        assert!(blob.savings() > 0);

        // Round-trip via CompressedBlob::decompress.
        let decompressed = blob.decompress().unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_compress_if_beneficial_incompressible_data() {
        // High-entropy data that won't compress well. compress_if_beneficial
        // may or may not compress it, but the round-trip must always work.
        let data: Vec<u8> = (0..1_000u32)
            .map(|i| {
                let mut x = i.wrapping_mul(2654435761);
                x ^= x >> 13;
                x ^= x << 7;
                x ^= x >> 17;
                (x & 0xFF) as u8
            })
            .collect();

        let blob = BlobCompressor::compress_if_beneficial(&data).unwrap();
        let decompressed = blob.decompress().unwrap();
        assert_eq!(decompressed, data);
    }

    // =====================================================================
    // compression_ratio calculation
    // =====================================================================

    #[test]
    fn test_compression_ratio() {
        assert_eq!(BlobCompressor::compression_ratio(&[0u8; 100], &[0u8; 50]), 2.0);
        assert_eq!(BlobCompressor::compression_ratio(&[0u8; 1000], &[0u8; 100]), 10.0);
        assert_eq!(BlobCompressor::compression_ratio(&[0u8; 100], &[0u8; 0]), 1.0);
    }

    // =====================================================================
    // CompressedBlob methods (ratio, savings)
    // =====================================================================

    #[test]
    fn test_compressed_blob_ratio_uncompressed() {
        let blob = CompressedBlob {
            data: vec![1u8; 500],
            is_compressed: false,
            original_size: 500,
            compressed_size: 500,
        };
        assert_eq!(blob.ratio(), 1.0);
        assert_eq!(blob.savings(), 0);
    }

    #[test]
    fn test_compressed_blob_ratio_compressed() {
        let blob = CompressedBlob {
            data: vec![1u8; 100],
            is_compressed: true,
            original_size: 1000,
            compressed_size: 100,
        };
        assert_eq!(blob.ratio(), 10.0);
        assert_eq!(blob.savings(), 900);
    }

    // =====================================================================
    // Highly compressible data (all zeros)
    // =====================================================================

    #[test]
    fn test_all_zeros_good_ratio() {
        let data = vec![0u8; 100_000];
        let compressed = BlobCompressor::compress_blob(&data).unwrap();
        let ratio = BlobCompressor::compression_ratio(&data, &compressed);
        assert!(
            ratio > 10.0,
            "All-zeros should achieve >10x compression ratio, got {:.2}x",
            ratio
        );

        let decompressed = BlobCompressor::decompress_blob(&compressed).unwrap();
        assert_eq!(decompressed, data);
    }

    // =====================================================================
    // Error cases
    // =====================================================================

    #[test]
    fn test_decompress_too_short() {
        let result = BlobCompressor::decompress_blob(&[0u8; 4]);
        assert!(result.is_err());
        match result {
            Err(CompressionError::InvalidData(_)) => {}
            other => panic!("Expected InvalidData error, got {:?}", other),
        }
    }

    #[test]
    fn test_decompress_empty() {
        let result = BlobCompressor::decompress_blob(&[]);
        assert!(result.is_err());
        match result {
            Err(CompressionError::InvalidData(_)) => {}
            other => panic!("Expected InvalidData error, got {:?}", other),
        }
    }

    #[test]
    fn test_decompress_garbage_payload() {
        // Valid-looking 8-byte header claiming 1000 bytes, followed by garbage.
        let mut data = (1000u64).to_le_bytes().to_vec();
        data.extend_from_slice(&[0xFF; 50]);
        let result = BlobCompressor::decompress_blob(&data);
        assert!(result.is_err());
    }

    // =====================================================================
    // Compression ratio reporting (integration-style)
    // =====================================================================

    #[test]
    fn test_compression_ratio_reporting() {
        // 100 KB of repetitive data — simulates a large JSON metadata blob.
        let data = vec![0x42u8; 100_000];
        let blob = BlobCompressor::compress_if_beneficial(&data).unwrap();
        assert!(blob.is_compressed);

        let ratio = blob.ratio();
        println!(
            "Repetitive 100KB blob → ratio: {:.2}x, original: {} bytes, \
             compressed: {} bytes, savings: {} bytes",
            ratio,
            blob.original_size,
            blob.compressed_size,
            blob.savings()
        );
        assert!(ratio > 1.0);
    }

    #[test]
    fn test_compressed_blob_decompress_round_trip_uncompressed() {
        // An uncompressed CompressedBlob must decompress back to the original.
        let original = vec![0x77u8; 50];
        let blob = CompressedBlob {
            data: original.clone(),
            is_compressed: false,
            original_size: 50,
            compressed_size: 50,
        };
        let recovered = blob.decompress().unwrap();
        assert_eq!(recovered, original);
    }
}
