//! Transaction Anomaly Detection
//!
//! Analyzes transactions and flags potential issues using rule-based heuristics
//! with optional LLM-enhanced semantic analysis via a local Qwen3-4B GGUF model.
//!
//! # Detection Rules
//! - **Duplicate amounts** — same amount, same vendor, within a configurable day window
//! - **Unusual vendors** — vendor not seen in the last N days
//! - **Round-number amounts** — amounts that are exact multiples of 1000 above a threshold
//! - **Amount outliers** — amounts significantly larger than the historical vendor average
//! - **Weekend/after-hours** — transactions occurring on Saturday or Sunday
//!
//! # LLM Enhancement
//! When a Qwen3-4B-Q8_0 GGUF model is available via `llama-cpp-rs`, the detector can
//! augment rule-based findings with semantic analysis. All rule-based checks work
//! independently of the LLM — it is purely an enhancement layer.

use chrono::{DateTime, Datelike, Utc, Duration};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for the anomaly detection engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnomalyConfig {
    /// Number of days to look back when checking for duplicate amounts.
    pub duplicate_days_window: u32,
    /// Number of days a vendor must have appeared within to be considered "known".
    pub unusual_vendor_days: u32,
    /// Minimum amount (inclusive) at which round-number checks apply.
    pub round_number_threshold: f64,
    /// Number of standard deviations above the mean to flag as an outlier.
    pub outlier_std_dev_threshold: f64,
    /// Whether to flag transactions that fall on Saturday or Sunday.
    pub enable_weekend_check: bool,
    /// Optional filesystem path to a Qwen3-4B-Q8_0 GGUF model.
    pub llm_model_path: Option<String>,
    /// Whether LLM-enhanced analysis is enabled.
    pub llm_enabled: bool,
}

impl Default for AnomalyConfig {
    fn default() -> Self {
        Self {
            duplicate_days_window: 7,
            unusual_vendor_days: 90,
            round_number_threshold: 5000.0,
            outlier_std_dev_threshold: 3.0,
            enable_weekend_check: true,
            llm_model_path: None,
            llm_enabled: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Anomaly types
// ---------------------------------------------------------------------------

/// The category of anomaly detected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnomalyType {
    /// Same amount and vendor within the duplicate window.
    DuplicateAmount,
    /// Vendor has not been seen in the configured number of days.
    UnusualVendor,
    /// Amount is an exact round number (multiple of 1000) above the threshold.
    RoundNumber,
    /// Amount is a statistical outlier relative to the vendor's history.
    AmountOutlier,
    /// Transaction date falls on a weekend (Saturday or Sunday).
    WeekendTransaction,
    /// A custom anomaly type, typically produced by the LLM layer.
    Custom(String),
}

impl std::fmt::Display for AnomalyType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AnomalyType::DuplicateAmount => write!(f, "DuplicateAmount"),
            AnomalyType::UnusualVendor => write!(f, "UnusualVendor"),
            AnomalyType::RoundNumber => write!(f, "RoundNumber"),
            AnomalyType::AmountOutlier => write!(f, "AmountOutlier"),
            AnomalyType::WeekendTransaction => write!(f, "WeekendTransaction"),
            AnomalyType::Custom(name) => write!(f, "Custom({})", name),
        }
    }
}

/// A single anomaly finding attached to a transaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Anomaly {
    /// What kind of anomaly was detected.
    pub anomaly_type: AnomalyType,
    /// How severe the anomaly is, from 0.0 (informational) to 1.0 (critical).
    pub severity: f32,
    /// Human-readable description of the finding.
    pub description: String,
    /// Confidence that this is a genuine anomaly, from 0.0 to 1.0.
    pub confidence: f32,
    /// Suggested next step for the reviewer.
    pub recommendation: String,
}

// ---------------------------------------------------------------------------
// Historical transaction data
// ---------------------------------------------------------------------------

/// A single historical transaction record used as input to the detector.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoricalTransaction {
    /// Unique identifier.
    pub id: String,
    /// Vendor / payee name (may be absent for some transaction types).
    pub vendor: Option<String>,
    /// Transaction amount (positive = outflow by convention).
    pub amount: Decimal,
    /// Timestamp of the transaction.
    pub date: DateTime<Utc>,
    /// Free-form description.
    pub description: String,
}

/// A collection of historical transactions that provides context for anomaly checks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionHistory {
    /// All known historical transactions.
    pub transactions: Vec<HistoricalTransaction>,
}

impl TransactionHistory {
    /// Create an empty history.
    pub fn new() -> Self {
        Self {
            transactions: Vec::new(),
        }
    }

    /// Create a history pre-populated with the given transactions.
    pub fn with_transactions(transactions: Vec<HistoricalTransaction>) -> Self {
        Self { transactions }
    }

    /// Add a transaction to the history.
    pub fn add(&mut self, txn: HistoricalTransaction) {
        self.transactions.push(txn);
    }

    /// Add a transaction to the history (alias for [`add`](Self::add)).
    pub fn push(&mut self, txn: HistoricalTransaction) {
        self.add(txn);
    }

    /// Return all transactions for a given vendor name (case-insensitive).
    pub fn by_vendor(&self, vendor: &str) -> Vec<&HistoricalTransaction> {
        let vendor_lower = vendor.to_lowercase();
        self.transactions
            .iter()
            .filter(|t| {
                t.vendor
                    .as_ref()
                    .map(|v| v.to_lowercase() == vendor_lower)
                    .unwrap_or(false)
            })
            .collect()
    }

    /// Return all transactions within the given date range (inclusive on both ends).
    pub fn by_date_range(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Vec<&HistoricalTransaction> {
        self.transactions
            .iter()
            .filter(|t| t.date >= from && t.date <= to)
            .collect()
    }

    /// Return all transactions for a given vendor within a date range.
    pub fn by_vendor_and_date_range(
        &self,
        vendor: &str,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Vec<&HistoricalTransaction> {
        let vendor_lower = vendor.to_lowercase();
        self.transactions
            .iter()
            .filter(|t| {
                t.date >= from
                    && t.date <= to
                    && t.vendor
                        .as_ref()
                        .map(|v| v.to_lowercase() == vendor_lower)
                        .unwrap_or(false)
            })
            .collect()
    }

    /// Return the total number of transactions in the history.
    pub fn len(&self) -> usize {
        self.transactions.len()
    }

    /// Return whether the history is empty.
    pub fn is_empty(&self) -> bool {
        self.transactions.is_empty()
    }

    /// Check whether a vendor has appeared at all in the history.
    pub fn has_vendor(&self, vendor: &str) -> bool {
        !self.by_vendor(vendor).is_empty()
    }

    /// Check whether a vendor has appeared within the last `days` days relative to `reference`.
    pub fn vendor_seen_within_days(&self, vendor: &str, days: u32, reference: DateTime<Utc>) -> bool {
        let cutoff = reference - Duration::days(days as i64);
        !self.by_vendor_and_date_range(vendor, cutoff, reference).is_empty()
    }

    /// Return the arithmetic mean of amounts for a given vendor (case-insensitive).
    /// Returns `None` when no matching transactions exist.
    pub fn average_amount_for_vendor(&self, vendor: &str) -> Option<Decimal> {
        let txns = self.by_vendor(vendor);
        if txns.is_empty() {
            return None;
        }
        let sum: Decimal = txns.iter().map(|t| t.amount).sum();
        Some(sum / Decimal::from(txns.len()))
    }
}

impl Default for TransactionHistory {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Anomaly detector
// ---------------------------------------------------------------------------

/// The main anomaly detection engine.
///
/// Runs a suite of rule-based checks against a transaction and its history,
/// optionally augmenting results with LLM-based semantic analysis.
pub struct AnomalyDetector {
    config: AnomalyConfig,
}

impl AnomalyDetector {
    /// Create a new detector with the given configuration.
    pub fn new(config: AnomalyConfig) -> Self {
        info!(
            duplicate_window = config.duplicate_days_window,
            unusual_vendor_days = config.unusual_vendor_days,
            round_threshold = config.round_number_threshold,
            outlier_stddev = config.outlier_std_dev_threshold,
            weekend_check = config.enable_weekend_check,
            llm_enabled = config.llm_enabled,
            "AnomalyDetector initialized"
        );
        Self { config }
    }

    /// Return a reference to the current configuration.
    pub fn config(&self) -> &AnomalyConfig {
        &self.config
    }

    // -----------------------------------------------------------------------
    // Primary entry point — rule-based analysis
    // -----------------------------------------------------------------------

    /// Run all rule-based anomaly checks on a transaction against its history.
    ///
    /// Returns zero or more [`Anomaly`] findings.
    pub fn analyze(
        &self,
        transaction: &HistoricalTransaction,
        history: &TransactionHistory,
    ) -> Vec<Anomaly> {
        debug!(
            txn_id = %transaction.id,
            amount = %transaction.amount,
            vendor = ?transaction.vendor,
            "Running rule-based anomaly analysis"
        );

        let mut anomalies: Vec<Anomaly> = Vec::new();

        if let Some(a) = self.check_duplicate_amount(transaction, history) {
            anomalies.push(a);
        }

        if let Some(a) = self.check_unusual_vendor(transaction, history) {
            anomalies.push(a);
        }

        if let Some(a) = self.check_round_number(transaction.amount) {
            anomalies.push(a);
        }

        if let Some(a) = self.check_amount_outlier(transaction, history) {
            anomalies.push(a);
        }

        if self.config.enable_weekend_check {
            if let Some(a) = self.check_weekend(transaction.date) {
                anomalies.push(a);
            }
        }

        debug!(
            txn_id = %transaction.id,
            anomaly_count = anomalies.len(),
            "Rule-based analysis complete"
        );

        anomalies
    }

    // -----------------------------------------------------------------------
    // Individual rule checks
    // -----------------------------------------------------------------------

    /// Check whether an identical amount for the same vendor already exists
    /// within the configured duplicate-day window.
    pub fn check_duplicate_amount(
        &self,
        txn: &HistoricalTransaction,
        history: &TransactionHistory,
    ) -> Option<Anomaly> {
        let vendor = txn.vendor.as_ref()?;
        let window_start = txn.date - Duration::days(self.config.duplicate_days_window as i64);

        let matches = history.by_vendor_and_date_range(vendor, window_start, txn.date);

        // Count transactions with the same amount (excluding the transaction itself).
        let duplicate_count = matches
            .iter()
            .filter(|t| t.id != txn.id && t.amount == txn.amount)
            .count();

        if duplicate_count > 0 {
            debug!(
                txn_id = %txn.id,
                vendor = %vendor,
                amount = %txn.amount,
                duplicates = duplicate_count,
                "Duplicate amount detected"
            );

            Some(Anomaly {
                anomaly_type: AnomalyType::DuplicateAmount,
                severity: 0.7,
                description: format!(
                    "Found {} transaction(s) with the same amount ({}) for vendor '{}' within the last {} day(s)",
                    duplicate_count,
                    txn.amount,
                    vendor,
                    self.config.duplicate_days_window,
                ),
                confidence: 0.9,
                recommendation: format!(
                    "Review recent transactions with '{}' for amount {} — this may be a duplicate entry",
                    vendor, txn.amount,
                ),
            })
        } else {
            None
        }
    }

    /// Check whether the transaction's vendor has not been seen within the
    /// configured unusual-vendor window.
    pub fn check_unusual_vendor(
        &self,
        txn: &HistoricalTransaction,
        history: &TransactionHistory,
    ) -> Option<Anomaly> {
        let vendor = txn.vendor.as_ref()?;

        // Look at history *before* this transaction's date.
        let cutoff = txn.date - Duration::days(self.config.unusual_vendor_days as i64);
        let seen = history
            .by_vendor_and_date_range(vendor, cutoff, txn.date)
            .iter()
            .any(|t| t.id != txn.id);

        if !seen {
            debug!(
                txn_id = %txn.id,
                vendor = %vendor,
                "Unusual vendor detected"
            );

            Some(Anomaly {
                anomaly_type: AnomalyType::UnusualVendor,
                severity: 0.4,
                description: format!(
                    "Vendor '{}' has not appeared in any transaction in the last {} day(s)",
                    vendor, self.config.unusual_vendor_days,
                ),
                confidence: 0.75,
                recommendation: format!(
                    "Verify that '{}' is a legitimate vendor and that this transaction is authorized",
                    vendor,
                ),
            })
        } else {
            None
        }
    }

    /// Check whether an amount is an exact round number (multiple of 1000)
    /// and at or above the configured threshold.
    pub fn check_round_number(&self, amount: Decimal) -> Option<Anomaly> {
        let amount_f64 = amount
            .to_string()
            .parse::<f64>()
            .unwrap_or(0.0);

        let threshold = self.config.round_number_threshold;

        if amount_f64 >= threshold && amount_f64 != 0.0 {
            // "Round" means the amount is an exact multiple of 1000.
            let remainder = amount_f64 % 1000.0;
            if remainder.abs() < f64::EPSILON {
                debug!(amount = amount_f64, "Round-number amount detected");

                return Some(Anomaly {
                    anomaly_type: AnomalyType::RoundNumber,
                    severity: 0.3,
                    description: format!(
                        "Amount ${:.2} is an exact round number (multiple of $1,000) at or above the ${:.2} threshold",
                        amount_f64, threshold,
                    ),
                    confidence: 0.6,
                    recommendation: String::from(
                        "Round-number amounts are more likely to be estimates rather than actual invoices — verify the supporting documentation",
                    ),
                });
            }
        }

        None
    }

    /// Check whether the transaction amount is a statistical outlier relative
    /// to the vendor's historical transaction amounts (mean + N * stddev).
    pub fn check_amount_outlier(
        &self,
        txn: &HistoricalTransaction,
        history: &TransactionHistory,
    ) -> Option<Anomaly> {
        let vendor = txn.vendor.as_ref()?;

        // Collect historical amounts for this vendor (excluding the current txn).
        let historical: Vec<f64> = history
            .by_vendor(vendor)
            .iter()
            .filter(|t| t.id != txn.id)
            .map(|t| t.amount.to_string().parse::<f64>().unwrap_or(0.0))
            .collect();

        // Need at least 2 data points to compute meaningful statistics.
        if historical.len() < 2 {
            return None;
        }

        let n = historical.len() as f64;
        let mean = historical.iter().sum::<f64>() / n;
        let variance = historical.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n;
        let std_dev = variance.sqrt();

        let amount_f64 = txn.amount.to_string().parse::<f64>().unwrap_or(0.0);
        let threshold_value = mean + self.config.outlier_std_dev_threshold * std_dev;

        if amount_f64 > threshold_value && std_dev > f64::EPSILON {
            let z_score = if std_dev > f64::EPSILON {
                (amount_f64 - mean) / std_dev
            } else {
                0.0
            };

            debug!(
                txn_id = %txn.id,
                amount = amount_f64,
                mean = mean,
                std_dev = std_dev,
                z_score = z_score,
                "Amount outlier detected"
            );

            // Scale severity by how far above the threshold we are.
            let severity = ((z_score / 10.0) as f32).min(1.0).max(0.3);

            Some(Anomaly {
                anomaly_type: AnomalyType::AmountOutlier,
                severity,
                description: format!(
                    "Amount ${:.2} is {:.1} standard deviations above the mean (${:.2}) for vendor '{}' (threshold: {:.1}σ)",
                    amount_f64,
                    z_score,
                    mean,
                    vendor,
                    self.config.outlier_std_dev_threshold,
                ),
                confidence: 0.8,
                recommendation: format!(
                    "This amount is unusually large for '{}' — confirm it is correct and properly authorized",
                    vendor,
                ),
            })
        } else {
            None
        }
    }

    /// Check whether the transaction date falls on a weekend (Saturday or Sunday).
    pub fn check_weekend(&self, date: DateTime<Utc>) -> Option<Anomaly> {
        let weekday = date.weekday();
        use chrono::Weekday;

        if weekday == Weekday::Sat || weekday == Weekday::Sun {
            let day_name = match weekday {
                Weekday::Sat => "Saturday",
                Weekday::Sun => "Sunday",
                _ => unreachable!(),
            };

            debug!(date = %date, day = day_name, "Weekend transaction detected");

            Some(Anomaly {
                anomaly_type: AnomalyType::WeekendTransaction,
                severity: 0.2,
                description: format!(
                    "Transaction occurred on {} ({})",
                    day_name,
                    date.format("%Y-%m-%d"),
                ),
                confidence: 1.0,
                recommendation: String::from(
                    "Weekend transactions are unusual for most businesses — verify this was authorized",
                ),
            })
        } else {
            None
        }
    }

    // -----------------------------------------------------------------------
    // LLM-enhanced analysis (optional — falls back to rule-based when disabled)
    // -----------------------------------------------------------------------

    /// Run anomaly analysis augmented by the local Qwen3-4B GGUF model.
    ///
    /// When `llm_enabled` is `false` (or the model is unavailable), this
    /// method returns the same results as [`analyze`](Self::analyze).
    ///
    /// The LLM receives the transaction details together with the rule-based
    /// findings and is asked to identify any additional anomalies that pure
    /// heuristics might miss.
    pub async fn analyze_with_llm(
        &self,
        transaction: &HistoricalTransaction,
        history: &TransactionHistory,
    ) -> Vec<Anomaly> {
        // Always start with rule-based results.
        let mut anomalies = self.analyze(transaction, history);

        if !self.config.llm_enabled {
            debug!("LLM is disabled — returning rule-based results only");
            return anomalies;
        }

        // Attempt LLM-enhanced analysis.
        match self.run_llm_analysis(transaction, history, &anomalies).await {
            Ok(llm_anomalies) => {
                info!(
                    llm_findings = llm_anomalies.len(),
                    "LLM analysis produced additional findings"
                );
                anomalies.extend(llm_anomalies);
            }
            Err(e) => {
                warn!(error = %e, "LLM analysis failed — falling back to rule-based results");
            }
        }

        anomalies
    }

    /// Internal helper that invokes the GGUF model and parses its output.
    async fn run_llm_analysis(
        &self,
        transaction: &HistoricalTransaction,
        _history: &TransactionHistory,
        rule_based_results: &[Anomaly],
    ) -> Result<Vec<Anomaly>, anyhow::Error> {
        let model_path = self
            .config
            .llm_model_path
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("No LLM model path configured"))?;

        // Verify the model file exists.
        if !std::path::Path::new(model_path).exists() {
            anyhow::bail!("LLM model file not found at: {}", model_path);
        }

        let rule_summary = if rule_based_results.is_empty() {
            "None".to_string()
        } else {
            rule_based_results
                .iter()
                .map(|a| format!("- [{}] {}", a.anomaly_type, a.description))
                .collect::<Vec<_>>()
                .join("\n")
        };

        let prompt = format!(
            "Analyze this transaction for anomalies: {description}. \
             Amount: {amount}. Vendor: {vendor}. Date: {date}. \
             Known anomalies from rule-based checks:\n{known}\n\
             Return JSON with additional findings as an array of objects with keys: \
             anomaly_type, severity (0.0-1.0), description, confidence (0.0-1.0), recommendation. \
             If no additional anomalies, return an empty array [].",
            description = transaction.description,
            amount = transaction.amount,
            vendor = transaction.vendor.as_deref().unwrap_or("Unknown"),
            date = transaction.date.format("%Y-%m-%d %H:%M UTC"),
            known = rule_summary,
        );

        debug!(prompt_length = prompt.len(), "Sending prompt to LLM");

        // ----------------------------------------------------------------
        // llama-cpp-rs integration point.
        //
        // The coordinator will add `llama-cpp-rs` to Cargo.toml.  The
        // intended flow is:
        //
        //   use llama_cpp_rs::LlamaModel;
        //   let model = LlamaModel::from_path(model_path)?;
        //   let output = model.generate(&prompt, params)?;
        //   let parsed: Vec<serde_json::Value> = serde_json::from_str(&output)?;
        //
        // For now we return an empty set so the rule-based results stand.
        // ----------------------------------------------------------------
        let _ = prompt; // suppress unused warning until llama-cpp-rs is wired up

        Ok(Vec::new())
    }
}

impl Default for AnomalyDetector {
    fn default() -> Self {
        Self::new(AnomalyConfig::default())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use rust_decimal_macros::dec;

    // -- helpers -------------------------------------------------------------

    fn make_txn(
        id: &str,
        vendor: Option<&str>,
        amount: Decimal,
        date: DateTime<Utc>,
        description: &str,
    ) -> HistoricalTransaction {
        HistoricalTransaction {
            id: id.to_string(),
            vendor: vendor.map(|s| s.to_string()),
            amount,
            date,
            description: description.to_string(),
        }
    }

    fn wednesday_noon() -> DateTime<Utc> {
        // 2026-07-01 is a Wednesday
        Utc.with_ymd_and_hms(2026, 7, 1, 12, 0, 0).unwrap()
    }

    fn saturday_noon() -> DateTime<Utc> {
        // 2026-07-04 is a Saturday
        Utc.with_ymd_and_hms(2026, 7, 4, 12, 0, 0).unwrap()
    }

    fn sunday_noon() -> DateTime<Utc> {
        // 2026-07-05 is a Sunday
        Utc.with_ymd_and_hms(2026, 7, 5, 12, 0, 0).unwrap()
    }

    // -- AnomalyConfig -------------------------------------------------------

    #[test]
    fn test_anomaly_config_defaults() {
        let cfg = AnomalyConfig::default();
        assert_eq!(cfg.duplicate_days_window, 7);
        assert_eq!(cfg.unusual_vendor_days, 90);
        assert!((cfg.round_number_threshold - 5000.0).abs() < f64::EPSILON);
        assert!((cfg.outlier_std_dev_threshold - 3.0).abs() < f64::EPSILON);
        assert!(cfg.enable_weekend_check);
        assert!(cfg.llm_model_path.is_none());
        assert!(!cfg.llm_enabled);
    }

    #[test]
    fn test_anomaly_config_custom() {
        let cfg = AnomalyConfig {
            duplicate_days_window: 14,
            unusual_vendor_days: 60,
            round_number_threshold: 10000.0,
            outlier_std_dev_threshold: 2.5,
            enable_weekend_check: false,
            llm_model_path: Some("/models/qwen.gguf".to_string()),
            llm_enabled: true,
        };
        assert_eq!(cfg.duplicate_days_window, 14);
        assert_eq!(cfg.unusual_vendor_days, 60);
        assert!(!cfg.enable_weekend_check);
        assert!(cfg.llm_enabled);
    }

    // -- AnomalyDetector creation --------------------------------------------

    #[test]
    fn test_anomaly_detector_creation() {
        let cfg = AnomalyConfig {
            duplicate_days_window: 10,
            ..AnomalyConfig::default()
        };
        let detector = AnomalyDetector::new(cfg);
        assert_eq!(detector.config().duplicate_days_window, 10);
    }

    #[test]
    fn test_anomaly_detector_default() {
        let detector = AnomalyDetector::default();
        assert_eq!(detector.config().duplicate_days_window, 7);
        assert!(!detector.config().llm_enabled);
    }

    // -- TransactionHistory --------------------------------------------------

    #[test]
    fn test_transaction_history_empty() {
        let hist = TransactionHistory::new();
        assert!(hist.is_empty());
        assert_eq!(hist.len(), 0);
    }

    #[test]
    fn test_transaction_history_with_transactions() {
        let t1 = make_txn("1", Some("Acme"), dec!(100), wednesday_noon(), "Invoice #1");
        let t2 = make_txn("2", Some("Acme"), dec!(200), wednesday_noon(), "Invoice #2");
        let t3 = make_txn("3", Some("Globex"), dec!(300), wednesday_noon(), "Invoice #3");

        let hist = TransactionHistory::with_transactions(vec![t1, t2, t3]);
        assert_eq!(hist.len(), 3);
        assert!(!hist.is_empty());
        assert!(hist.has_vendor("Acme"));
        assert!(hist.has_vendor("acme")); // case-insensitive
        assert!(!hist.has_vendor("Initech"));
    }

    #[test]
    fn test_transaction_history_by_vendor() {
        let t1 = make_txn("1", Some("Acme"), dec!(100), wednesday_noon(), "A");
        let t2 = make_txn("2", Some("Globex"), dec!(200), wednesday_noon(), "B");
        let t3 = make_txn("3", Some("Acme"), dec!(300), wednesday_noon(), "C");

        let hist = TransactionHistory::with_transactions(vec![t1, t2, t3]);
        let acme = hist.by_vendor("Acme");
        assert_eq!(acme.len(), 2);
        let globex = hist.by_vendor("Globex");
        assert_eq!(globex.len(), 1);
    }

    #[test]
    fn test_transaction_history_by_date_range() {
        let d1 = Utc.with_ymd_and_hms(2026, 6, 1, 12, 0, 0).unwrap();
        let d2 = Utc.with_ymd_and_hms(2026, 6, 15, 12, 0, 0).unwrap();
        let d3 = Utc.with_ymd_and_hms(2026, 7, 1, 12, 0, 0).unwrap();

        let t1 = make_txn("1", Some("A"), dec!(10), d1, "x");
        let t2 = make_txn("2", Some("A"), dec!(20), d2, "y");
        let t3 = make_txn("3", Some("A"), dec!(30), d3, "z");

        let hist = TransactionHistory::with_transactions(vec![t1, t2, t3]);

        let range = hist.by_date_range(
            Utc.with_ymd_and_hms(2026, 6, 10, 0, 0, 0).unwrap(),
            Utc.with_ymd_and_hms(2026, 7, 2, 0, 0, 0).unwrap(),
        );
        assert_eq!(range.len(), 2); // d2 and d3
    }

    #[test]
    fn test_transaction_history_push() {
        let mut hist = TransactionHistory::new();
        assert!(hist.is_empty());
        hist.push(make_txn("1", Some("V"), dec!(50), wednesday_noon(), "d"));
        assert_eq!(hist.len(), 1);
    }

    #[test]
    fn test_transaction_history_add() {
        let mut hist = TransactionHistory::new();
        hist.add(make_txn("1", Some("Acme"), dec!(100), wednesday_noon(), "a"));
        hist.add(make_txn("2", Some("Acme"), dec!(200), wednesday_noon(), "b"));
        assert_eq!(hist.len(), 2);
    }

    #[test]
    fn test_average_amount_for_vendor() {
        let mut hist = TransactionHistory::new();
        hist.add(make_txn("1", Some("Acme"), dec!(100), wednesday_noon(), "a"));
        hist.add(make_txn("2", Some("Acme"), dec!(200), wednesday_noon(), "b"));
        hist.add(make_txn("3", Some("Other"), dec!(999), wednesday_noon(), "c"));

        let avg = hist.average_amount_for_vendor("Acme").unwrap();
        assert_eq!(avg, dec!(150));

        // Case-insensitive
        let avg_ci = hist.average_amount_for_vendor("acme").unwrap();
        assert_eq!(avg_ci, dec!(150));

        // Unknown vendor
        assert!(hist.average_amount_for_vendor("Nobody").is_none());
    }

    #[test]
    fn test_vendor_seen_within_days() {
        let d_old = Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap();
        let d_recent = Utc.with_ymd_and_hms(2026, 6, 25, 12, 0, 0).unwrap();
        let reference = Utc.with_ymd_and_hms(2026, 7, 1, 12, 0, 0).unwrap();

        let t1 = make_txn("1", Some("OldCorp"), dec!(100), d_old, "old");
        let t2 = make_txn("2", Some("NewCorp"), dec!(200), d_recent, "new");

        let hist = TransactionHistory::with_transactions(vec![t1, t2]);

        assert!(hist.vendor_seen_within_days("NewCorp", 90, reference));
        assert!(!hist.vendor_seen_within_days("OldCorp", 90, reference));
    }

    // -- Duplicate amount detection ------------------------------------------

    #[test]
    fn test_duplicate_amount_flagged() {
        let d1 = wednesday_noon();
        let d2 = d1 + Duration::days(5); // 5 days later — within 7-day window

        let t1 = make_txn("t1", Some("Acme"), dec!(1500.00), d1, "Invoice A");
        let t2 = make_txn("t2", Some("Acme"), dec!(1500.00), d2, "Invoice B");

        let hist = TransactionHistory::with_transactions(vec![t1.clone()]);
        let detector = AnomalyDetector::default();

        let result = detector.check_duplicate_amount(&t2, &hist);
        assert!(result.is_some());
        let anomaly = result.unwrap();
        assert_eq!(anomaly.anomaly_type, AnomalyType::DuplicateAmount);
        assert!(anomaly.severity > 0.0);
        assert!(anomaly.confidence > 0.0);
    }

    #[test]
    fn test_duplicate_amount_not_flagged_outside_window() {
        let d1 = wednesday_noon();
        let d2 = d1 + Duration::days(8); // 8 days — outside 7-day window

        let t1 = make_txn("t1", Some("Acme"), dec!(1500.00), d1, "Invoice A");
        let t2 = make_txn("t2", Some("Acme"), dec!(1500.00), d2, "Invoice B");

        let hist = TransactionHistory::with_transactions(vec![t1]);
        let detector = AnomalyDetector::default();

        let result = detector.check_duplicate_amount(&t2, &hist);
        assert!(result.is_none());
    }

    #[test]
    fn test_duplicate_amount_different_vendor_not_flagged() {
        let d1 = wednesday_noon();
        let d2 = d1 + Duration::days(3);

        let t1 = make_txn("t1", Some("Acme"), dec!(1500.00), d1, "A");
        let t2 = make_txn("t2", Some("Globex"), dec!(1500.00), d2, "B");

        let hist = TransactionHistory::with_transactions(vec![t1]);
        let detector = AnomalyDetector::default();

        let result = detector.check_duplicate_amount(&t2, &hist);
        assert!(result.is_none());
    }

    #[test]
    fn test_duplicate_amount_different_amount_not_flagged() {
        let d1 = wednesday_noon();
        let d2 = d1 + Duration::days(2);

        let t1 = make_txn("t1", Some("Acme"), dec!(1500.00), d1, "A");
        let t2 = make_txn("t2", Some("Acme"), dec!(1600.00), d2, "B");

        let hist = TransactionHistory::with_transactions(vec![t1]);
        let detector = AnomalyDetector::default();

        let result = detector.check_duplicate_amount(&t2, &hist);
        assert!(result.is_none());
    }

    #[test]
    fn test_duplicate_no_vendor_returns_none() {
        let txn = make_txn("t1", None, dec!(500), wednesday_noon(), "no vendor");
        let hist = TransactionHistory::new();
        let detector = AnomalyDetector::default();
        assert!(detector.check_duplicate_amount(&txn, &hist).is_none());
    }

    // -- Unusual vendor detection --------------------------------------------

    #[test]
    fn test_unusual_vendor_flagged() {
        let d = wednesday_noon();
        let txn = make_txn("new1", Some("BrandNewCorp"), dec!(500), d, "first time");
        let hist = TransactionHistory::new(); // empty history — vendor never seen
        let detector = AnomalyDetector::default();

        let result = detector.check_unusual_vendor(&txn, &hist);
        assert!(result.is_some());
        assert_eq!(result.unwrap().anomaly_type, AnomalyType::UnusualVendor);
    }

    #[test]
    fn test_known_vendor_not_flagged() {
        let d = wednesday_noon();
        let past = d - Duration::days(30); // 30 days ago — within 90-day window

        let t_old = make_txn("old1", Some("RegularCorp"), dec!(100), past, "past txn");
        let t_new = make_txn("new1", Some("RegularCorp"), dec!(200), d, "current txn");

        let hist = TransactionHistory::with_transactions(vec![t_old]);
        let detector = AnomalyDetector::default();

        let result = detector.check_unusual_vendor(&t_new, &hist);
        assert!(result.is_none());
    }

    #[test]
    fn test_vendor_not_seen_within_window_flagged() {
        let d = wednesday_noon();
        let very_old = d - Duration::days(200); // 200 days ago — outside 90-day window

        let t_old = make_txn("old1", Some("OldCorp"), dec!(100), very_old, "very old");
        let t_new = make_txn("new1", Some("OldCorp"), dec!(200), d, "current");

        let hist = TransactionHistory::with_transactions(vec![t_old]);
        let detector = AnomalyDetector::default();

        let result = detector.check_unusual_vendor(&t_new, &hist);
        assert!(result.is_some());
    }

    #[test]
    fn test_unusual_vendor_no_vendor_returns_none() {
        let txn = make_txn("t1", None, dec!(100), wednesday_noon(), "no vendor");
        let hist = TransactionHistory::new();
        let detector = AnomalyDetector::default();
        assert!(detector.check_unusual_vendor(&txn, &hist).is_none());
    }

    // -- Round number detection ----------------------------------------------

    #[test]
    fn test_round_number_flagged() {
        let detector = AnomalyDetector::default();
        let result = detector.check_round_number(dec!(5000.00));
        assert!(result.is_some());
        assert_eq!(result.unwrap().anomaly_type, AnomalyType::RoundNumber);
    }

    #[test]
    fn test_round_number_large_flagged() {
        let detector = AnomalyDetector::default();
        let result = detector.check_round_number(dec!(10000.00));
        assert!(result.is_some());
    }

    #[test]
    fn test_round_number_just_below_threshold_not_flagged() {
        let detector = AnomalyDetector::default();
        // 4999.99 is not a multiple of 1000, so not flagged
        let result = detector.check_round_number(dec!(4999.99));
        assert!(result.is_none());
    }

    #[test]
    fn test_round_number_below_threshold_not_flagged() {
        let detector = AnomalyDetector::default();
        // 500 is a multiple of 1000? No — 500 % 1000 != 0. So not flagged.
        let result = detector.check_round_number(dec!(500.00));
        assert!(result.is_none());
    }

    #[test]
    fn test_round_number_1000_below_threshold_not_flagged() {
        let detector = AnomalyDetector::default();
        // 1000 is a multiple of 1000 but below the 5000 threshold
        let result = detector.check_round_number(dec!(1000.00));
        assert!(result.is_none());
    }

    #[test]
    fn test_round_number_non_round_above_threshold_not_flagged() {
        let detector = AnomalyDetector::default();
        // 6543.21 is above threshold but not a multiple of 1000
        let result = detector.check_round_number(dec!(6543.21));
        assert!(result.is_none());
    }

    #[test]
    fn test_round_number_zero_not_flagged() {
        let detector = AnomalyDetector::default();
        let result = detector.check_round_number(dec!(0.00));
        assert!(result.is_none());
    }

    #[test]
    fn test_round_number_negative_not_flagged() {
        let detector = AnomalyDetector::default();
        let result = detector.check_round_number(dec!(-5000.00));
        assert!(result.is_none());
    }

    // -- Amount outlier detection --------------------------------------------

    #[test]
    fn test_amount_outlier_flagged() {
        // Historical: 100, 120, 110, 105, 115 — mean ≈ 110, stddev ≈ ~5.5
        // New transaction: 500 — way above mean + 3*stddev (~126.5)
        let base_date = wednesday_noon();
        let vendor = "Acme";

        let txns: Vec<HistoricalTransaction> = vec![
            make_txn("h1", Some(vendor), dec!(100), base_date - Duration::days(60), "a"),
            make_txn("h2", Some(vendor), dec!(120), base_date - Duration::days(50), "b"),
            make_txn("h3", Some(vendor), dec!(110), base_date - Duration::days(40), "c"),
            make_txn("h4", Some(vendor), dec!(105), base_date - Duration::days(30), "d"),
            make_txn("h5", Some(vendor), dec!(115), base_date - Duration::days(20), "e"),
        ];

        let outlier_txn = make_txn("new", Some(vendor), dec!(500), base_date, "outlier");

        let hist = TransactionHistory::with_transactions(txns);
        let detector = AnomalyDetector::default();

        let result = detector.check_amount_outlier(&outlier_txn, &hist);
        assert!(result.is_some());
        let anomaly = result.unwrap();
        assert_eq!(anomaly.anomaly_type, AnomalyType::AmountOutlier);
    }

    #[test]
    fn test_amount_outlier_not_flagged_normal() {
        // Historical: 100, 120, 110, 105, 115 — mean ≈ 110, stddev ≈ ~5.5
        // New transaction: 112 — well within normal range
        let base_date = wednesday_noon();
        let vendor = "Acme";

        let txns: Vec<HistoricalTransaction> = vec![
            make_txn("h1", Some(vendor), dec!(100), base_date - Duration::days(60), "a"),
            make_txn("h2", Some(vendor), dec!(120), base_date - Duration::days(50), "b"),
            make_txn("h3", Some(vendor), dec!(110), base_date - Duration::days(40), "c"),
            make_txn("h4", Some(vendor), dec!(105), base_date - Duration::days(30), "d"),
            make_txn("h5", Some(vendor), dec!(115), base_date - Duration::days(20), "e"),
        ];

        let normal_txn = make_txn("new", Some(vendor), dec!(112), base_date, "normal");

        let hist = TransactionHistory::with_transactions(txns);
        let detector = AnomalyDetector::default();

        let result = detector.check_amount_outlier(&normal_txn, &hist);
        assert!(result.is_none());
    }

    #[test]
    fn test_amount_outlier_insufficient_history() {
        let base_date = wednesday_noon();
        let txn = make_txn("t1", Some("Solo"), dec!(999), base_date, "only one");
        let hist = TransactionHistory::with_transactions(vec![txn.clone()]);
        let detector = AnomalyDetector::default();

        // Only one data point — cannot compute meaningful stats.
        assert!(detector.check_amount_outlier(&txn, &hist).is_none());
    }

    #[test]
    fn test_amount_outlier_no_vendor_returns_none() {
        let txn = make_txn("t1", None, dec!(99999), wednesday_noon(), "no vendor");
        let hist = TransactionHistory::new();
        let detector = AnomalyDetector::default();
        assert!(detector.check_amount_outlier(&txn, &hist).is_none());
    }

    // -- Weekend detection ---------------------------------------------------

    #[test]
    fn test_weekend_saturday_flagged() {
        let detector = AnomalyDetector::default();
        let result = detector.check_weekend(saturday_noon());
        assert!(result.is_some());
        assert_eq!(result.unwrap().anomaly_type, AnomalyType::WeekendTransaction);
    }

    #[test]
    fn test_weekend_sunday_flagged() {
        let detector = AnomalyDetector::default();
        let result = detector.check_weekend(sunday_noon());
        assert!(result.is_some());
        assert_eq!(result.unwrap().anomaly_type, AnomalyType::WeekendTransaction);
    }

    #[test]
    fn test_weekend_wednesday_not_flagged() {
        let detector = AnomalyDetector::default();
        let result = detector.check_weekend(wednesday_noon());
        assert!(result.is_none());
    }

    #[test]
    fn test_weekend_monday_not_flagged() {
        let detector = AnomalyDetector::default();
        // 2026-07-06 is a Monday
        let monday = Utc.with_ymd_and_hms(2026, 7, 6, 12, 0, 0).unwrap();
        assert!(detector.check_weekend(monday).is_none());
    }

    #[test]
    fn test_weekend_friday_not_flagged() {
        let detector = AnomalyDetector::default();
        // 2026-07-03 is a Friday
        let friday = Utc.with_ymd_and_hms(2026, 7, 3, 12, 0, 0).unwrap();
        assert!(detector.check_weekend(friday).is_none());
    }

    // -- Full analyze() integration ------------------------------------------

    #[test]
    fn test_analyze_multiple_anomalies() {
        // Create a scenario that triggers multiple anomaly rules:
        // - Duplicate amount (same vendor, same amount, 3 days apart)
        // - Weekend transaction (Saturday)
        // - Unusual vendor (no prior history besides the duplicate)

        let saturday = saturday_noon();
        let wednesday_before = saturday - Duration::days(3);

        let existing = make_txn(
            "existing",
            Some("ShadyVendor"),
            dec!(2500.00),
            wednesday_before,
            "previous payment",
        );
        let new_txn = make_txn(
            "new",
            Some("ShadyVendor"),
            dec!(2500.00),
            saturday,
            "duplicate payment",
        );

        let hist = TransactionHistory::with_transactions(vec![existing]);
        let detector = AnomalyDetector::default();

        let anomalies = detector.analyze(&new_txn, &hist);

        // Should have at least: DuplicateAmount + WeekendTransaction
        let types: Vec<&AnomalyType> = anomalies.iter().map(|a| &a.anomaly_type).collect();
        assert!(
            types.contains(&&AnomalyType::DuplicateAmount),
            "Expected DuplicateAmount anomaly, got: {:?}",
            types
        );
        assert!(
            types.contains(&&AnomalyType::WeekendTransaction),
            "Expected WeekendTransaction anomaly, got: {:?}",
            types
        );
    }

    #[test]
    fn test_analyze_clean_transaction_no_anomalies() {
        // A perfectly normal transaction: known vendor, normal amount, weekday.
        let base = wednesday_noon();
        let vendor = "RegularCorp";

        let history_txns: Vec<HistoricalTransaction> = vec![
            make_txn("h1", Some(vendor), dec!(100), base - Duration::days(60), "a"),
            make_txn("h2", Some(vendor), dec!(120), base - Duration::days(50), "b"),
            make_txn("h3", Some(vendor), dec!(110), base - Duration::days(40), "c"),
            make_txn("h4", Some(vendor), dec!(105), base - Duration::days(30), "d"),
            make_txn("h5", Some(vendor), dec!(115), base - Duration::days(20), "e"),
        ];

        let normal_txn = make_txn("new", Some(vendor), dec!(112), base, "routine payment");

        let hist = TransactionHistory::with_transactions(history_txns);
        let detector = AnomalyDetector::default();

        let anomalies = detector.analyze(&normal_txn, &hist);
        assert!(
            anomalies.is_empty(),
            "Expected no anomalies for a clean transaction, got: {:?}",
            anomalies.iter().map(|a| &a.anomaly_type).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_analyze_round_number_and_outlier_combined() {
        // A round-number amount that is also an outlier.
        let base = wednesday_noon();
        let vendor = "BigCo";

        let history_txns: Vec<HistoricalTransaction> = vec![
            make_txn("h1", Some(vendor), dec!(100), base - Duration::days(60), "a"),
            make_txn("h2", Some(vendor), dec!(120), base - Duration::days(50), "b"),
            make_txn("h3", Some(vendor), dec!(110), base - Duration::days(40), "c"),
            make_txn("h4", Some(vendor), dec!(105), base - Duration::days(30), "d"),
            make_txn("h5", Some(vendor), dec!(115), base - Duration::days(20), "e"),
        ];

        let big_txn = make_txn("big", Some(vendor), dec!(10000), base, "huge payment");

        let hist = TransactionHistory::with_transactions(history_txns);
        let detector = AnomalyDetector::default();

        let anomalies = detector.analyze(&big_txn, &hist);

        let types: Vec<&AnomalyType> = anomalies.iter().map(|a| &a.anomaly_type).collect();
        assert!(
            types.contains(&&AnomalyType::RoundNumber),
            "Expected RoundNumber, got: {:?}",
            types
        );
        assert!(
            types.contains(&&AnomalyType::AmountOutlier),
            "Expected AmountOutlier, got: {:?}",
            types
        );
    }

    #[test]
    fn test_analyze_weekend_disabled() {
        let config = AnomalyConfig {
            enable_weekend_check: false,
            ..AnomalyConfig::default()
        };
        let detector = AnomalyDetector::new(config);

        let txn = make_txn("t1", Some("V"), dec!(100), saturday_noon(), "weekend");
        let hist = TransactionHistory::with_transactions(vec![
            make_txn("h1", Some("V"), dec!(100), wednesday_noon(), "weekday"),
        ]);

        let anomalies = detector.analyze(&txn, &hist);
        let types: Vec<&AnomalyType> = anomalies.iter().map(|a| &a.anomaly_type).collect();
        assert!(
            !types.contains(&&AnomalyType::WeekendTransaction),
            "Weekend check should be disabled"
        );
    }

    // -- LLM fallback --------------------------------------------------------

    #[tokio::test]
    async fn test_analyze_with_llm_disabled_returns_rule_based() {
        let config = AnomalyConfig {
            llm_enabled: false,
            ..AnomalyConfig::default()
        };
        let detector = AnomalyDetector::new(config);

        let saturday = saturday_noon();
        let txn = make_txn("t1", Some("V"), dec!(100), saturday, "test");
        let hist = TransactionHistory::new();

        let anomalies = detector.analyze_with_llm(&txn, &hist).await;

        // Should still get weekend + unusual vendor from rule-based.
        assert!(!anomalies.is_empty());
        let types: Vec<&AnomalyType> = anomalies.iter().map(|a| &a.anomaly_type).collect();
        assert!(types.contains(&&AnomalyType::WeekendTransaction));
        assert!(types.contains(&&AnomalyType::UnusualVendor));
    }

    #[tokio::test]
    async fn test_analyze_with_llm_enabled_no_model_file() {
        let config = AnomalyConfig {
            llm_enabled: true,
            llm_model_path: Some("/nonexistent/model.gguf".to_string()),
            ..AnomalyConfig::default()
        };
        let detector = AnomalyDetector::new(config);

        let txn = make_txn("t1", Some("V"), dec!(100), wednesday_noon(), "test");
        let hist = TransactionHistory::new();

        // Should gracefully fall back to rule-based results.
        let anomalies = detector.analyze_with_llm(&txn, &hist).await;
        assert!(!anomalies.is_empty()); // unusual vendor at least
    }

    // -- AnomalyType Display -------------------------------------------------

    #[test]
    fn test_anomaly_type_display() {
        assert_eq!(AnomalyType::DuplicateAmount.to_string(), "DuplicateAmount");
        assert_eq!(AnomalyType::UnusualVendor.to_string(), "UnusualVendor");
        assert_eq!(AnomalyType::RoundNumber.to_string(), "RoundNumber");
        assert_eq!(AnomalyType::AmountOutlier.to_string(), "AmountOutlier");
        assert_eq!(
            AnomalyType::WeekendTransaction.to_string(),
            "WeekendTransaction"
        );
        assert_eq!(
            AnomalyType::Custom("Foo".to_string()).to_string(),
            "Custom(Foo)"
        );
    }

    // -- Serialization round-trip --------------------------------------------

    #[test]
    fn test_anomaly_serialization_roundtrip() {
        let anomaly = Anomaly {
            anomaly_type: AnomalyType::DuplicateAmount,
            severity: 0.8,
            description: "test anomaly".to_string(),
            confidence: 0.95,
            recommendation: "review it".to_string(),
        };

        let json = serde_json::to_string(&anomaly).expect("serialize");
        let deserialized: Anomaly = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(deserialized.anomaly_type, AnomalyType::DuplicateAmount);
        assert!((deserialized.severity - 0.8).abs() < f32::EPSILON);
        assert_eq!(deserialized.description, "test anomaly");
    }

    #[test]
    fn test_config_serialization_roundtrip() {
        let config = AnomalyConfig {
            duplicate_days_window: 10,
            unusual_vendor_days: 60,
            round_number_threshold: 2500.0,
            outlier_std_dev_threshold: 2.0,
            enable_weekend_check: true,
            llm_model_path: Some("/models/qwen.gguf".to_string()),
            llm_enabled: true,
        };

        let json = serde_json::to_string(&config).expect("serialize");
        let deserialized: AnomalyConfig = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(deserialized.duplicate_days_window, 10);
        assert_eq!(deserialized.unusual_vendor_days, 60);
        assert!(deserialized.llm_enabled);
    }

    #[test]
    fn test_historical_transaction_serialization() {
        let txn = make_txn("id-1", Some("Vendor"), dec!(123.45), wednesday_noon(), "test");
        let json = serde_json::to_string(&txn).expect("serialize");
        let deserialized: HistoricalTransaction = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(deserialized.id, "id-1");
        assert_eq!(deserialized.vendor, Some("Vendor".to_string()));
        assert_eq!(deserialized.amount, dec!(123.45));
    }
}
