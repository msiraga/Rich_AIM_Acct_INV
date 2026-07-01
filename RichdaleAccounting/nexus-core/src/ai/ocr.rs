//! OCR Engine Integration
//!
//! Provides text extraction from images (PNG, JPEG) and scanned PDFs
//! using the Mistral OCR4 cloud API. This is NOT a local Tesseract
//! integration — all OCR requests go to Mistral's cloud endpoint via HTTP.
//!
//! # Graceful Degradation
//! When the API key is missing or the API is unreachable, methods return
//! a clear `anyhow::Error` so callers can fall back to local PDF extraction.
//!
//! # Environment Variables
//! - `MISTRAL_API_KEY` — Bearer token for the Mistral OCR4 API
//! - `MISTRAL_OCR_URL` — (optional) Override the default API endpoint
//! - `MISTRAL_OCR_TIMEOUT` — (optional) Override the default timeout (seconds)
//! - `MISTRAL_OCR_ENABLED` — (optional) Set to `"false"` to disable OCR

use std::time::Duration;

use anyhow::{anyhow, Context};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info, warn};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for the Mistral OCR4 engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrConfig {
    /// API key for Mistral OCR. Sourced from `MISTRAL_API_KEY`.
    pub api_key: Option<String>,

    /// Base URL for the OCR endpoint.
    pub api_url: String,

    /// Request timeout in seconds.
    pub timeout: u64,

    /// Whether OCR is enabled.
    pub enabled: bool,
}

impl Default for OcrConfig {
    fn default() -> Self {
        Self {
            api_key: None,
            api_url: "https://api.mistral.ai/v1/ocr".to_string(),
            timeout: 30,
            enabled: true,
        }
    }
}

impl OcrConfig {
    /// Create a new configuration with all defaults.
    pub fn new() -> Self {
        Self::default()
    }

    /// Build a configuration from environment variables.
    ///
    /// | Variable | Maps to |
    /// |---|---|
    /// | `MISTRAL_API_KEY` | `api_key` |
    /// | `MISTRAL_OCR_URL` | `api_url` |
    /// | `MISTRAL_OCR_TIMEOUT` | `timeout` |
    /// | `MISTRAL_OCR_ENABLED` | `enabled` |
    pub fn from_env() -> Self {
        let api_key = std::env::var("MISTRAL_API_KEY").ok().filter(|k| !k.is_empty());

        let api_url = std::env::var("MISTRAL_OCR_URL")
            .unwrap_or_else(|_| "https://api.mistral.ai/v1/ocr".to_string());

        let timeout = std::env::var("MISTRAL_OCR_TIMEOUT")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(30);

        let enabled = std::env::var("MISTRAL_OCR_ENABLED")
            .ok()
            .and_then(|s| s.parse::<bool>().ok())
            .unwrap_or(true);

        Self {
            api_key,
            api_url,
            timeout,
            enabled,
        }
    }
}

// ---------------------------------------------------------------------------
// Request / response models (Mistral OCR4 API)
// ---------------------------------------------------------------------------

/// Top-level request body for the Mistral OCR endpoint.
#[derive(Debug, Serialize)]
struct OcrRequest {
    model: String,
    document: OcrDocument,
}

/// The `document` field inside an OCR request.
#[derive(Debug, Serialize)]
struct OcrDocument {
    #[serde(rename = "type")]
    doc_type: String,
    image_url: String,
}

/// Top-level response from the Mistral OCR endpoint.
#[derive(Debug, Deserialize)]
struct OcrResponse {
    #[serde(default)]
    pages: Vec<OcrPage>,
    #[serde(default)]
    content: Option<String>,
}

/// A single page inside the OCR response.
#[derive(Debug, Deserialize)]
struct OcrPage {
    #[serde(default)]
    index: Option<u32>,
    #[serde(default)]
    markdown: Option<String>,
    #[serde(default)]
    text: Option<String>,
}

// ---------------------------------------------------------------------------
// Engine
// ---------------------------------------------------------------------------

/// OCR engine backed by the Mistral OCR4 cloud API.
///
/// All public async methods return `anyhow::Error` so callers can
/// pattern-match on the error or fall back to local extraction.
#[derive(Debug, Clone)]
pub struct OcrEngine {
    config: OcrConfig,
    client: Client,
}

impl OcrEngine {
    /// Create an engine from an explicit configuration.
    pub fn new(config: OcrConfig) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(config.timeout))
            .build()
            .unwrap_or_else(|e| {
                warn!("Failed to build HTTP client with custom timeout, using default: {e}");
                Client::new()
            });

        info!(
            api_url = %config.api_url,
            timeout_s = config.timeout,
            enabled = config.enabled,
            api_key_set = config.api_key.is_some(),
            "OCR engine created"
        );

        Self { config, client }
    }

    /// Create an engine by reading configuration from environment variables.
    pub fn from_env() -> Self {
        let config = OcrConfig::from_env();
        Self::new(config)
    }

    // -- availability helpers -----------------------------------------------

    /// Returns `true` when an API key is configured **and** the engine is enabled.
    pub fn is_available(&self) -> bool {
        self.config.enabled && self.config.api_key.is_some()
    }

    /// Ping the Mistral API to check reachability.
    ///
    /// Sends a minimal request (empty body) to the configured endpoint and
    /// returns `true` when the server responds with any non-5xx status.
    pub async fn health_check(&self) -> Result<bool, anyhow::Error> {
        if !self.config.enabled {
            warn!("OCR health check skipped: engine is disabled");
            return Ok(false);
        }

        let api_key = self
            .config
            .api_key
            .as_deref()
            .ok_or_else(|| anyhow!("OCR health check failed: MISTRAL_API_KEY is not set"))?;

        debug!(url = %self.config.api_url, "Running OCR health check");

        let resp = self
            .client
            .get(&self.config.api_url)
            .bearer_auth(api_key)
            .send()
            .await
            .context("OCR health check: HTTP request failed")?;

        let status = resp.status();
        debug!(status = %status, "OCR health check response");

        // Any non-server-error means the service is reachable.
        Ok(!status.is_server_error())
    }

    // -- extraction ---------------------------------------------------------

    /// Extract text from raw image bytes.
    ///
    /// `mime_type` should be e.g. `"image/png"`, `"image/jpeg"`, `"application/pdf"`.
    /// The bytes are base64-encoded and sent to the Mistral OCR4 API.
    pub async fn extract_text(
        &self,
        image_bytes: &[u8],
        mime_type: &str,
    ) -> Result<String, anyhow::Error> {
        let base64_data = STANDARD.encode(image_bytes);
        debug!(
            mime_type = mime_type,
            raw_bytes = image_bytes.len(),
            base64_len = base64_data.len(),
            "Encoding image for OCR"
        );
        self.extract_text_from_base64(&base64_data, mime_type).await
    }

    /// Extract text from an already base64-encoded string.
    ///
    /// This is a convenience wrapper that skips the encoding step when the
    /// caller already has the data in base64 form.
    pub async fn extract_text_from_base64(
        &self,
        base64_data: &str,
        mime_type: &str,
    ) -> Result<String, anyhow::Error> {
        // -- pre-flight checks ----------------------------------------------
        if !self.config.enabled {
            return Err(anyhow!(
                "OCR is disabled. Set MISTRAL_OCR_ENABLED=true or use OcrConfig {{ enabled: true }}"
            ));
        }

        let api_key = self
            .config
            .api_key
            .as_deref()
            .ok_or_else(|| anyhow!(
                "OCR extraction failed: MISTRAL_API_KEY is not set. \
                 Set the environment variable or provide api_key in OcrConfig."
            ))?;

        if base64_data.is_empty() {
            return Err(anyhow!("OCR extraction failed: input data is empty"));
        }

        // -- build request --------------------------------------------------
        let data_url = format!("data:{};base64,{}", mime_type, base64_data);

        let body = OcrRequest {
            model: "mistral-ocr-latest".to_string(),
            document: OcrDocument {
                doc_type: "image_url".to_string(),
                image_url: data_url,
            },
        };

        debug!(
            model = "mistral-ocr-latest",
            mime_type = mime_type,
            base64_len = base64_data.len(),
            "Sending OCR request to Mistral"
        );

        // -- send request ---------------------------------------------------
        let response = self
            .client
            .post(&self.config.api_url)
            .bearer_auth(api_key)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .context("OCR request failed: could not reach Mistral API")?;

        let status = response.status();

        if !status.is_success() {
            let error_body = response.text().await.unwrap_or_default();
            error!(
                status = %status,
                body = %error_body,
                "Mistral OCR API returned an error"
            );
            return Err(anyhow!(
                "Mistral OCR API error (HTTP {status}): {error_body}"
            ));
        }

        // -- parse response -------------------------------------------------
        let ocr_response: OcrResponse = response
            .json()
            .await
            .context("Failed to parse Mistral OCR response as JSON")?;

        // Prefer per-page text, fall back to top-level content.
        let text = Self::assemble_text(&ocr_response);

        if text.is_empty() {
            warn!("Mistral OCR returned empty text for the document");
        } else {
            info!(
                chars = text.len(),
                pages = ocr_response.pages.len(),
                "OCR extraction complete"
            );
        }

        Ok(text)
    }

    // -- internals ----------------------------------------------------------

    /// Walk the response pages and concatenate their text content.
    fn assemble_text(resp: &OcrResponse) -> String {
        if !resp.pages.is_empty() {
            let mut parts: Vec<&str> = Vec::with_capacity(resp.pages.len());
            for page in &resp.pages {
                // Prefer markdown (richer), fall back to plain text.
                if let Some(md) = page.markdown.as_deref() {
                    if !md.is_empty() {
                        parts.push(md);
                        continue;
                    }
                }
                if let Some(t) = page.text.as_deref() {
                    if !t.is_empty() {
                        parts.push(t);
                    }
                }
            }
            if !parts.is_empty() {
                return parts.join("\n\n");
            }
        }

        // Fallback: top-level content field.
        resp.content
            .as_deref()
            .unwrap_or("")
            .to_string()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- OcrConfig tests ----------------------------------------------------

    #[test]
    fn test_ocr_config_default() {
        let cfg = OcrConfig::default();
        assert!(cfg.api_key.is_none());
        assert_eq!(cfg.api_url, "https://api.mistral.ai/v1/ocr");
        assert_eq!(cfg.timeout, 30);
        assert!(cfg.enabled);
    }

    #[test]
    fn test_ocr_config_new_matches_default() {
        let a = OcrConfig::new();
        let b = OcrConfig::default();
        assert_eq!(a.api_url, b.api_url);
        assert_eq!(a.timeout, b.timeout);
        assert_eq!(a.enabled, b.enabled);
    }

    #[test]
    fn test_ocr_config_from_env_without_vars() {
        // Temporarily clear the env vars we care about so the test is deterministic.
        // SAFETY: we only remove variables that are unlikely to affect other tests,
        // and we restore them afterwards.
        let saved_key = std::env::var("MISTRAL_API_KEY").ok();
        let saved_url = std::env::var("MISTRAL_OCR_URL").ok();
        let saved_timeout = std::env::var("MISTRAL_OCR_TIMEOUT").ok();
        let saved_enabled = std::env::var("MISTRAL_OCR_ENABLED").ok();

        std::env::remove_var("MISTRAL_API_KEY");
        std::env::remove_var("MISTRAL_OCR_URL");
        std::env::remove_var("MISTRAL_OCR_TIMEOUT");
        std::env::remove_var("MISTRAL_OCR_ENABLED");

        let cfg = OcrConfig::from_env();
        assert!(cfg.api_key.is_none());
        assert_eq!(cfg.api_url, "https://api.mistral.ai/v1/ocr");
        assert_eq!(cfg.timeout, 30);
        assert!(cfg.enabled);

        // Restore
        if let Some(v) = saved_key { std::env::set_var("MISTRAL_API_KEY", v); }
        if let Some(v) = saved_url { std::env::set_var("MISTRAL_OCR_URL", v); }
        if let Some(v) = saved_timeout { std::env::set_var("MISTRAL_OCR_TIMEOUT", v); }
        if let Some(v) = saved_enabled { std::env::set_var("MISTRAL_OCR_ENABLED", v); }
    }

    #[test]
    fn test_ocr_config_from_env_with_vars() {
        let saved_key = std::env::var("MISTRAL_API_KEY").ok();
        let saved_url = std::env::var("MISTRAL_OCR_URL").ok();
        let saved_timeout = std::env::var("MISTRAL_OCR_TIMEOUT").ok();
        let saved_enabled = std::env::var("MISTRAL_OCR_ENABLED").ok();

        std::env::set_var("MISTRAL_API_KEY", "test-key-123");
        std::env::set_var("MISTRAL_OCR_URL", "https://custom.example.com/ocr");
        std::env::set_var("MISTRAL_OCR_TIMEOUT", "60");
        std::env::set_var("MISTRAL_OCR_ENABLED", "false");

        let cfg = OcrConfig::from_env();
        assert_eq!(cfg.api_key.as_deref(), Some("test-key-123"));
        assert_eq!(cfg.api_url, "https://custom.example.com/ocr");
        assert_eq!(cfg.timeout, 60);
        assert!(!cfg.enabled);

        // Restore
        match saved_key {
            Some(v) => std::env::set_var("MISTRAL_API_KEY", v),
            None => std::env::remove_var("MISTRAL_API_KEY"),
        }
        match saved_url {
            Some(v) => std::env::set_var("MISTRAL_OCR_URL", v),
            None => std::env::remove_var("MISTRAL_OCR_URL"),
        }
        match saved_timeout {
            Some(v) => std::env::set_var("MISTRAL_OCR_TIMEOUT", v),
            None => std::env::remove_var("MISTRAL_OCR_TIMEOUT"),
        }
        match saved_enabled {
            Some(v) => std::env::set_var("MISTRAL_OCR_ENABLED", v),
            None => std::env::remove_var("MISTRAL_OCR_ENABLED"),
        }
    }

    // -- OcrEngine construction tests ---------------------------------------

    #[test]
    fn test_engine_new_with_default_config() {
        let engine = OcrEngine::new(OcrConfig::default());
        assert!(!engine.is_available()); // no API key
    }

    #[test]
    fn test_engine_new_with_api_key() {
        let cfg = OcrConfig {
            api_key: Some("sk-test-abc".to_string()),
            ..OcrConfig::default()
        };
        let engine = OcrEngine::new(cfg);
        assert!(engine.is_available());
    }

    #[test]
    fn test_is_available_when_disabled() {
        let cfg = OcrConfig {
            api_key: Some("key".to_string()),
            enabled: false,
            ..OcrConfig::default()
        };
        let engine = OcrEngine::new(cfg);
        assert!(!engine.is_available());
    }

    #[test]
    fn test_is_available_when_no_key() {
        let cfg = OcrConfig {
            api_key: None,
            enabled: true,
            ..OcrConfig::default()
        };
        let engine = OcrEngine::new(cfg);
        assert!(!engine.is_available());
    }

    #[test]
    fn test_from_env_creates_engine() {
        // Just ensure from_env() does not panic.
        let _engine = OcrEngine::from_env();
    }

    // -- extract_text error paths -------------------------------------------

    #[tokio::test]
    async fn test_extract_text_no_api_key() {
        let engine = OcrEngine::new(OcrConfig::default());
        let result = engine.extract_text(b"fake-image-bytes", "image/png").await;
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("MISTRAL_API_KEY"),
            "Error should mention missing API key, got: {err_msg}"
        );
    }

    #[tokio::test]
    async fn test_extract_text_disabled() {
        let cfg = OcrConfig {
            api_key: Some("key".to_string()),
            enabled: false,
            ..OcrConfig::default()
        };
        let engine = OcrEngine::new(cfg);
        let result = engine.extract_text(b"data", "image/png").await;
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("disabled"),
            "Error should mention disabled, got: {err_msg}"
        );
    }

    #[tokio::test]
    async fn test_extract_text_from_base64_empty_input() {
        let cfg = OcrConfig {
            api_key: Some("key".to_string()),
            ..OcrConfig::default()
        };
        let engine = OcrEngine::new(cfg);
        let result = engine.extract_text_from_base64("", "image/png").await;
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("empty"),
            "Error should mention empty input, got: {err_msg}"
        );
    }

    // -- base64 encoding tests ----------------------------------------------

    #[test]
    fn test_base64_encoding_roundtrip() {
        let original = b"Hello, OCR world! \x00\xff";
        let encoded = STANDARD.encode(original);
        let decoded = STANDARD.decode(&encoded).expect("valid base64");
        assert_eq!(decoded, original);
    }

    #[test]
    fn test_base64_encoding_empty() {
        let encoded = STANDARD.encode(b"");
        assert!(encoded.is_empty());
    }

    #[test]
    fn test_base64_encoding_binary_image_like_data() {
        // Simulate a small PNG-like byte sequence (8-byte PNG header).
        let png_header: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        let encoded = STANDARD.encode(&png_header);
        assert_eq!(encoded, "iVBORw0KGgo=");
        let decoded = STANDARD.decode(&encoded).expect("valid base64");
        assert_eq!(decoded, png_header);
    }

    // -- response assembly tests --------------------------------------------

    #[test]
    fn test_assemble_text_from_pages_with_markdown() {
        let resp = OcrResponse {
            pages: vec![
                OcrPage {
                    index: Some(0),
                    markdown: Some("# Invoice\nTotal: $100".to_string()),
                    text: Some("Invoice Total: $100".to_string()),
                },
                OcrPage {
                    index: Some(1),
                    markdown: Some("## Page 2\nNotes".to_string()),
                    text: Some("Page 2 Notes".to_string()),
                },
            ],
            content: None,
        };
        let text = OcrEngine::assemble_text(&resp);
        assert_eq!(text, "# Invoice\nTotal: $100\n\n## Page 2\nNotes");
    }

    #[test]
    fn test_assemble_text_falls_back_to_plain_text() {
        let resp = OcrResponse {
            pages: vec![OcrPage {
                index: Some(0),
                markdown: None,
                text: Some("Plain text content".to_string()),
            }],
            content: None,
        };
        let text = OcrEngine::assemble_text(&resp);
        assert_eq!(text, "Plain text content");
    }

    #[test]
    fn test_assemble_text_falls_back_to_content_field() {
        let resp = OcrResponse {
            pages: vec![],
            content: Some("Top-level content".to_string()),
        };
        let text = OcrEngine::assemble_text(&resp);
        assert_eq!(text, "Top-level content");
    }

    #[test]
    fn test_assemble_text_empty_response() {
        let resp = OcrResponse {
            pages: vec![],
            content: None,
        };
        let text = OcrEngine::assemble_text(&resp);
        assert!(text.is_empty());
    }

    // -- health_check error paths -------------------------------------------

    #[tokio::test]
    async fn test_health_check_no_api_key() {
        let engine = OcrEngine::new(OcrConfig::default());
        let result = engine.health_check().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_health_check_disabled() {
        let cfg = OcrConfig {
            api_key: Some("key".to_string()),
            enabled: false,
            ..OcrConfig::default()
        };
        let engine = OcrEngine::new(cfg);
        let result = engine.health_check().await;
        assert!(result.is_ok());
        assert!(!result.unwrap()); // disabled => false
    }

    // -- serialization tests ------------------------------------------------

    #[test]
    fn test_ocr_request_serialization() {
        let req = OcrRequest {
            model: "mistral-ocr-latest".to_string(),
            document: OcrDocument {
                doc_type: "image_url".to_string(),
                image_url: "data:image/png;base64,abc123".to_string(),
            },
        };
        let json = serde_json::to_string(&req).expect("serializable");
        assert!(json.contains("mistral-ocr-latest"));
        assert!(json.contains("image_url"));
        assert!(json.contains("data:image/png;base64,abc123"));
    }

    #[test]
    fn test_ocr_response_deserialization_pages() {
        let json = r##"{
            "pages": [
                {"index": 0, "markdown": "# Hello", "text": "Hello"},
                {"index": 1, "text": "World"}
            ]
        }"##;
        let resp: OcrResponse = serde_json::from_str(json).expect("valid JSON");
        assert_eq!(resp.pages.len(), 2);
        assert_eq!(resp.pages[0].markdown.as_deref(), Some("# Hello"));
        assert_eq!(resp.pages[1].text.as_deref(), Some("World"));
        assert!(resp.content.is_none());
    }

    #[test]
    fn test_ocr_response_deserialization_content_only() {
        let json = r#"{"content": "Extracted text"}"#;
        let resp: OcrResponse = serde_json::from_str(json).expect("valid JSON");
        assert!(resp.pages.is_empty());
        assert_eq!(resp.content.as_deref(), Some("Extracted text"));
    }

    #[test]
    fn test_ocr_config_serde_roundtrip() {
        let cfg = OcrConfig {
            api_key: Some("test-key".to_string()),
            api_url: "https://custom.api/ocr".to_string(),
            timeout: 45,
            enabled: false,
        };
        let json = serde_json::to_string(&cfg).expect("serializable");
        let restored: OcrConfig = serde_json::from_str(&json).expect("deserializable");
        assert_eq!(restored.api_key, cfg.api_key);
        assert_eq!(restored.api_url, cfg.api_url);
        assert_eq!(restored.timeout, cfg.timeout);
        assert_eq!(restored.enabled, cfg.enabled);
    }
}
