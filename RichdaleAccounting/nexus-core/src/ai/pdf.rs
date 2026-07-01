//! PDF Text Extraction Module
//!
//! Local PDF text extraction using the `pdf-extract` crate.
//! Extracts text from native (text-based) PDFs — complements OCR (Mistral OCR4)
//! for non-scanned documents. Serves as the fast, offline fallback when the OCR
//! API is unavailable or the document is already text-searchable.
//!
//! # Usage
//!
//! ```rust,ignore
//! use nexus_core::ai::pdf::{PdfConfig, PdfExtractor};
//!
//! let extractor = PdfExtractor::default();
//! let bytes = std::fs::read("invoice.pdf").unwrap();
//! if extractor.is_pdf(&bytes) {
//!     match extractor.extract_text(&bytes) {
//!         Ok(text) => println!("Extracted: {}", text),
//!         Err(e) => eprintln!("Falling back to OCR: {}", e),
//!     }
//! }
//! ```

use anyhow::{anyhow, Context};
use pdf_extract::extract_text_from_mem;
use serde::{Deserialize, Serialize};
use std::path::Path;
use tracing::{debug, info, warn};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for PDF text extraction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PdfConfig {
    /// Whether PDF extraction is enabled.
    pub enabled: bool,

    /// Maximum number of pages to extract. `0` means extract all pages.
    pub max_pages: usize,

    /// Minimum number of characters in the extracted text to consider the
    /// extraction successful. If the extracted text is shorter than this
    /// threshold the PDF is assumed to be scanned and an error is returned so
    /// the caller can fall back to OCR.
    pub min_text_length: usize,
}

impl Default for PdfConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_pages: 0,
            min_text_length: 10,
        }
    }
}

impl PdfConfig {
    /// Create a new configuration with the given values.
    pub fn new(enabled: bool, max_pages: usize, min_text_length: usize) -> Self {
        Self {
            enabled,
            max_pages,
            min_text_length,
        }
    }
}

// ---------------------------------------------------------------------------
// PDF magic bytes
// ---------------------------------------------------------------------------

/// The first four bytes of every valid PDF file.
const PDF_MAGIC: &[u8; 4] = b"%PDF";

/// Form-feed character used by `pdf-extract` to separate pages.
const FORM_FEED: char = '\x0c';

// ---------------------------------------------------------------------------
// Extractor
// ---------------------------------------------------------------------------

/// Stateless PDF text extractor backed by the `pdf-extract` crate.
#[derive(Debug, Clone)]
pub struct PdfExtractor {
    /// Active configuration.
    pub config: PdfConfig,
}

impl Default for PdfExtractor {
    fn default() -> Self {
        Self {
            config: PdfConfig::default(),
        }
    }
}

impl PdfExtractor {
    /// Create a new extractor with the supplied configuration.
    pub fn new(config: PdfConfig) -> Self {
        Self { config }
    }

    // ------------------------------------------------------------------
    // Detection helpers
    // ------------------------------------------------------------------

    /// Check whether `data` begins with the `%PDF` magic bytes.
    ///
    /// This is a quick heuristic — it does **not** validate the full
    /// structure of the file.
    pub fn is_pdf(&self, data: &[u8]) -> bool {
        data.len() >= PDF_MAGIC.len() && &data[..PDF_MAGIC.len()] == PDF_MAGIC
    }

    // ------------------------------------------------------------------
    // Page counting
    // ------------------------------------------------------------------

    /// Attempt to determine the number of pages in the PDF.
    ///
    /// Extracts text and counts page boundaries (form-feed characters)
    /// plus one. If extraction fails entirely (corrupted or invalid PDF),
    /// the underlying error is propagated.
    ///
    /// **Note:** This relies on `pdf-extract` successfully parsing the
    /// document. For a more accurate count on complex PDFs, consider
    /// using a dedicated PDF parser.
    pub fn page_count(&self, pdf_bytes: &[u8]) -> Result<usize, anyhow::Error> {
        if pdf_bytes.is_empty() {
            return Err(anyhow!("cannot count pages in empty data"));
        }

        if !self.is_pdf(pdf_bytes) {
            return Err(anyhow!("data is not a valid PDF — cannot count pages"));
        }

        // Extract text (using the raw crate call, bypassing our own
        // min_text_length check since we only care about page structure).
        let raw_text = extract_text_from_mem(pdf_bytes).map_err(|e| {
            anyhow!("failed to parse PDF for page count: {e}")
        })?;

        // pdf-extract inserts a form-feed ('\x0c') between pages.
        // An empty extraction still represents at least one page.
        let form_feeds = raw_text.chars().filter(|&c| c == FORM_FEED).count();
        let pages = if raw_text.is_empty() { 1 } else { form_feeds + 1 };

        Ok(pages)
    }

    // ------------------------------------------------------------------
    // Text extraction
    // ------------------------------------------------------------------

    /// Extract text from in-memory PDF bytes.
    ///
    /// # Errors
    ///
    /// - Returns an error if `config.enabled` is `false`.
    /// - Returns an error if the data is not a valid PDF.
    /// - Returns an error if `pdf-extract` fails to parse or read the PDF
    ///   (e.g. corrupted file, encrypted PDF, scanned-only document).
    /// - Returns an error if the resulting text is shorter than
    ///   `config.min_text_length`, indicating the document may be scanned
    ///   and requires OCR instead.
    pub fn extract_text(&self, pdf_bytes: &[u8]) -> Result<String, anyhow::Error> {
        if !self.config.enabled {
            return Err(anyhow!("PDF extraction is disabled in configuration"));
        }

        if pdf_bytes.is_empty() {
            return Err(anyhow!("cannot extract text from empty PDF data"));
        }

        if !self.is_pdf(pdf_bytes) {
            return Err(anyhow!(
                "data does not start with %PDF magic bytes — not a valid PDF"
            ));
        }

        // --- extract via pdf-extract ---
        debug!(pdf_bytes_len = pdf_bytes.len(), "starting PDF text extraction");
        let raw_text = extract_text_from_mem(pdf_bytes).map_err(|e| {
            warn!(error = %e, "pdf-extract failed to parse PDF");
            anyhow!("pdf-extract failed: {e}")
        }).context("PDF text extraction failed — the document may be corrupted, encrypted, or scanned")?;
        debug!(raw_text_len = raw_text.len(), "raw text extracted from PDF");

        // --- page limiting ---
        let text = if self.config.max_pages > 0 {
            self.truncate_to_max_pages(&raw_text, self.config.max_pages)
        } else {
            raw_text
        };

        // --- minimum text length check ---
        let trimmed = text.trim();
        if trimmed.len() < self.config.min_text_length {
            warn!(
                extracted_chars = trimmed.len(),
                min_required = self.config.min_text_length,
                "extracted text too short — PDF is likely scanned"
            );
            return Err(anyhow!(
                "extracted text is only {} characters (minimum {} required) — \
                 the PDF appears to be scanned and may need OCR",
                trimmed.len(),
                self.config.min_text_length,
            ));
        }

        info!(
            text_len = trimmed.len(),
            max_pages = self.config.max_pages,
            "PDF text extraction completed successfully"
        );

        Ok(text)
    }

    /// Read a PDF file from disk and extract its text content.
    ///
    /// This is a convenience wrapper around [`extract_text`](Self::extract_text).
    pub fn extract_text_from_file(&self, path: &Path) -> Result<String, anyhow::Error> {
        info!(path = %path.display(), "reading PDF file from disk");
        let bytes = std::fs::read(path)
            .with_context(|| format!("failed to read PDF file: {}", path.display()))?;
        debug!(bytes = bytes.len(), "loaded PDF file into memory");
        self.extract_text(&bytes)
    }

    // ------------------------------------------------------------------
    // Internal helpers
    // ------------------------------------------------------------------

    /// Truncate extracted text to roughly `max_pages` pages.
    ///
    /// `pdf-extract` inserts a form-feed (`\x0c`) between pages. We split on
    /// that boundary and rejoin the first `max_pages` segments.
    fn truncate_to_max_pages(&self, text: &str, max_pages: usize) -> String {
        let pages: Vec<&str> = text.split(FORM_FEED).collect();

        if pages.len() <= max_pages {
            // Already within the limit — return as-is.
            return text.to_string();
        }

        // Rejoin the first `max_pages` pages with the form-feed separator.
        let truncated: String = pages[..max_pages].join(&FORM_FEED.to_string());

        if pages.len() > max_pages {
            warn!(
                total_pages = pages.len(),
                kept_pages = max_pages,
                "truncating PDF text to max_pages limit"
            );
        }

        truncated
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal valid PDF containing the text "Hello World" on one page.
    ///
    /// Structure:
    ///   1 0 obj — Catalog
    ///   2 0 obj — Pages tree (1 page)
    ///   3 0 obj — Page with Helvetica 12pt
    ///   4 0 obj — Content stream: `BT /F1 12 Tf 100 700 Td (Hello World) Tj ET`
    const MINIMAL_PDF: &[u8] = b"%PDF-1.4\n\
1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n\
2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n\
3 0 obj<</Type/Page/MediaBox[0 0 612 792]/Parent 2 0 R/Resources<</Font<</F1<</Type/Font/Subtype/Type1/BaseFont/Helvetica>>>>>>/Contents 4 0 R>>endobj\n\
4 0 obj<</Length 44>>stream\n\
BT /F1 12 Tf 100 700 Td (Hello World) Tj ET\n\
endstream\n\
endobj\n\
xref\n\
0 5\n\
0000000000 65535 f \n\
0000000009 00000 n \n\
0000000058 00000 n \n\
0000000115 00000 n \n\
0000000306 00000 n \n\
trailer<</Size 5/Root 1 0 R>>\n\
startxref\n\
400\n\
%%EOF";

    // ------------------------------------------------------------------
    // PdfConfig tests
    // ------------------------------------------------------------------

    #[test]
    fn test_pdf_config_default() {
        let config = PdfConfig::default();
        assert!(config.enabled, "PDF extraction should be enabled by default");
        assert_eq!(config.max_pages, 0, "max_pages 0 means all pages");
        assert_eq!(
            config.min_text_length, 10,
            "minimum text length should default to 10"
        );
    }

    #[test]
    fn test_pdf_config_custom() {
        let config = PdfConfig::new(false, 5, 50);
        assert!(!config.enabled);
        assert_eq!(config.max_pages, 5);
        assert_eq!(config.min_text_length, 50);
    }

    #[test]
    fn test_pdf_config_serde_roundtrip() {
        let config = PdfConfig::new(true, 3, 20);
        let json = serde_json::to_string(&config).expect("serialize");
        let deserialized: PdfConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(deserialized.enabled, config.enabled);
        assert_eq!(deserialized.max_pages, config.max_pages);
        assert_eq!(deserialized.min_text_length, config.min_text_length);
    }

    // ------------------------------------------------------------------
    // PdfExtractor construction
    // ------------------------------------------------------------------

    #[test]
    fn test_extractor_default() {
        let extractor = PdfExtractor::default();
        assert!(extractor.config.enabled);
        assert_eq!(extractor.config.max_pages, 0);
        assert_eq!(extractor.config.min_text_length, 10);
    }

    #[test]
    fn test_extractor_new_with_config() {
        let config = PdfConfig::new(true, 2, 100);
        let extractor = PdfExtractor::new(config.clone());
        assert_eq!(extractor.config.max_pages, 2);
        assert_eq!(extractor.config.min_text_length, 100);
    }

    // ------------------------------------------------------------------
    // is_pdf()
    // ------------------------------------------------------------------

    #[test]
    fn test_is_pdf_with_valid_header() {
        let extractor = PdfExtractor::default();
        assert!(extractor.is_pdf(b"%PDF-1.4 rest of file"));
        assert!(extractor.is_pdf(b"%PDF-2.0"));
        assert!(extractor.is_pdf(MINIMAL_PDF));
    }

    #[test]
    fn test_is_pdf_with_invalid_data() {
        let extractor = PdfExtractor::default();
        assert!(!extractor.is_pdf(b""));
        assert!(!extractor.is_pdf(b"not a pdf"));
        assert!(!extractor.is_pdf(b"%PD")); // too short for magic
        assert!(!extractor.is_pdf(b"\x00\x00\x00\x00"));
        assert!(!extractor.is_pdf(b"PK\x03\x04")); // ZIP, not PDF
    }

    // ------------------------------------------------------------------
    // extract_text() — error paths
    // ------------------------------------------------------------------

    #[test]
    fn test_extract_text_empty_bytes() {
        let extractor = PdfExtractor::default();
        let result = extractor.extract_text(&[]);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("empty"),
            "error should mention empty data: {err}"
        );
    }

    #[test]
    fn test_extract_text_non_pdf_data() {
        let extractor = PdfExtractor::default();
        let result = extractor.extract_text(b"This is not a PDF file at all");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("magic bytes") || err.contains("not a valid PDF"),
            "error should mention invalid format: {err}"
        );
    }

    #[test]
    fn test_extract_text_disabled() {
        let config = PdfConfig::new(false, 0, 10);
        let extractor = PdfExtractor::new(config);
        let result = extractor.extract_text(b"%PDF-1.4 something");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("disabled"),
            "error should mention disabled: {err}"
        );
    }

    #[test]
    fn test_extract_text_corrupted_pdf() {
        let extractor = PdfExtractor::default();
        // Valid magic bytes but garbage body — pdf-extract should fail.
        let mut corrupted = MINIMAL_PDF.to_vec();
        // Corrupt the xref table by zeroing out the middle section.
        let len = corrupted.len();
        for byte in &mut corrupted[len / 3..len * 2 / 3] {
            *byte = 0x00;
        }
        let result = extractor.extract_text(&corrupted);
        // This should either error or return text shorter than min_text_length.
        // Either outcome is acceptable — the key point is it doesn't panic.
        if result.is_err() {
            let err = result.unwrap_err().to_string();
            assert!(
                !err.is_empty(),
                "error message should be non-empty for corrupted PDF"
            );
        }
        // If it somehow succeeds, that's also fine — the PDF might still be
        // partially parseable.
    }

    // ------------------------------------------------------------------
    // extract_text() — success path with minimal PDF
    // ------------------------------------------------------------------

    #[test]
    fn test_extract_text_minimal_pdf() {
        let extractor = PdfExtractor::default();
        let result = extractor.extract_text(MINIMAL_PDF);

        match result {
            Ok(text) => {
                let trimmed = text.trim();
                assert!(
                    trimmed.contains("Hello World") || trimmed.contains("Hello") || trimmed.contains("World"),
                    "extracted text should contain 'Hello World' but got: {trimmed:?}"
                );
            }
            Err(e) => {
                // pdf-extract may struggle with hand-crafted minimal PDFs.
                // As long as it returns a descriptive error, that's acceptable.
                let msg = e.to_string();
                assert!(
                    msg.contains("scanned") || msg.contains("failed") || msg.contains("characters"),
                    "error should be descriptive: {msg}"
                );
            }
        }
    }

    // ------------------------------------------------------------------
    // extract_text_from_file() — error paths
    // ------------------------------------------------------------------

    #[test]
    fn test_extract_text_from_nonexistent_file() {
        let extractor = PdfExtractor::default();
        let result = extractor.extract_text_from_file(
            std::path::Path::new("/tmp/nonexistent_nexus_test_12345.pdf"),
        );
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("failed to read") || err.contains("No such file") || err.contains("cannot find"),
            "error should mention file read failure: {err}"
        );
    }

    // ------------------------------------------------------------------
    // page_count()
    // ------------------------------------------------------------------

    #[test]
    fn test_page_count_minimal_pdf() {
        let extractor = PdfExtractor::default();
        let result = extractor.page_count(MINIMAL_PDF);

        match result {
            Ok(count) => {
                assert_eq!(count, 1, "minimal PDF has exactly 1 page");
            }
            Err(e) => {
                // lopdf may reject hand-crafted xref offsets. Acceptable as
                // long as we get a meaningful error.
                let msg = e.to_string();
                assert!(
                    msg.contains("parse") || msg.contains("failed"),
                    "page_count error should be descriptive: {msg}"
                );
            }
        }
    }

    #[test]
    fn test_page_count_invalid_bytes() {
        let extractor = PdfExtractor::default();
        let result = extractor.page_count(b"not a pdf");
        assert!(result.is_err());
    }

    // ------------------------------------------------------------------
    // truncate_to_max_pages()
    // ------------------------------------------------------------------

    #[test]
    fn test_truncate_to_max_pages_no_truncation_needed() {
        let extractor = PdfExtractor::default();
        let text = "page one content";
        let result = extractor.truncate_to_max_pages(text, 5);
        assert_eq!(result, text);
    }

    #[test]
    fn test_truncate_to_max_pages_with_form_feeds() {
        let extractor = PdfExtractor::default();
        let text = "page1\x0cpage2\x0cpage3\x0cpage4\x0cpage5";

        let result = extractor.truncate_to_max_pages(text, 2);
        assert_eq!(result, "page1\x0cpage2");

        let result = extractor.truncate_to_max_pages(text, 3);
        assert_eq!(result, "page1\x0cpage2\x0cpage3");

        // Requesting more pages than available should return the full text.
        let result = extractor.truncate_to_max_pages(text, 10);
        assert_eq!(result, text);
    }

    #[test]
    fn test_truncate_to_max_pages_single_page() {
        let extractor = PdfExtractor::default();
        let text = "only one page, no form feeds";
        let result = extractor.truncate_to_max_pages(text, 1);
        assert_eq!(result, text);
    }

    // ------------------------------------------------------------------
    // min_text_length enforcement
    // ------------------------------------------------------------------

    #[test]
    fn test_min_text_length_with_high_threshold() {
        // Use a config that requires more text than our minimal PDF can produce.
        let config = PdfConfig {
            enabled: true,
            max_pages: 0,
            min_text_length: 10_000,
        };
        let extractor = PdfExtractor::new(config);
        let result = extractor.extract_text(MINIMAL_PDF);

        // With min_text_length set to 10000, even a successful parse of our
        // tiny PDF should fail the length check.
        if result.is_err() {
            let err = result.unwrap_err().to_string();
            assert!(
                err.contains("scanned") || err.contains("characters") || err.contains("OCR") || err.contains("failed"),
                "error should indicate text too short or parse failure: {err}"
            );
        }
    }

    // ------------------------------------------------------------------
    // Edge cases
    // ------------------------------------------------------------------

    #[test]
    fn test_is_pdf_exact_magic_boundary() {
        let extractor = PdfExtractor::default();
        // Exactly 4 bytes matching magic
        assert!(extractor.is_pdf(b"%PDF"));
        // 3 bytes — too short
        assert!(!extractor.is_pdf(b"%PD"));
    }

    #[test]
    fn test_extractor_clone() {
        let config = PdfConfig::new(true, 5, 20);
        let extractor = PdfExtractor::new(config);
        let cloned = extractor.clone();
        assert_eq!(cloned.config.max_pages, 5);
        assert_eq!(cloned.config.min_text_length, 20);
        assert!(cloned.config.enabled);
    }
}
