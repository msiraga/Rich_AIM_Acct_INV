//! Account classification and categorization module.
//!
//! Provides smart account suggestions using rule-based keyword matching
//! optionally enhanced by a local LLM (Qwen3-4B GGUF via llama-cpp-rs).
//! The rule-based engine is fully functional without the LLM; when the
//! model is available the two signals are merged for higher quality.

use std::collections::HashMap;
use std::fmt;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

// ---------------------------------------------------------------------------
// Data structures
// ---------------------------------------------------------------------------

/// Top-level configuration for the classifier.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassificationConfig {
    /// Filesystem path to the Qwen GGUF model.
    pub llm_model_path: Option<String>,
    /// Whether the LLM path is enabled. Defaults to `false`.
    pub llm_enabled: bool,
    /// Minimum confidence (0.0–1.0) for auto-accepting a suggestion.
    pub confidence_threshold: f32,
    /// Maximum number of suggestions returned by `suggest_account_type`.
    pub max_suggestions: usize,
}

impl Default for ClassificationConfig {
    fn default() -> Self {
        Self {
            llm_model_path: None,
            llm_enabled: false,
            confidence_threshold: 0.7,
            max_suggestions: 3,
        }
    }
}

/// The five fundamental accounting categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AccountCategory {
    Asset,
    Liability,
    Equity,
    Revenue,
    Expense,
}

impl fmt::Display for AccountCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Asset => write!(f, "Asset"),
            Self::Liability => write!(f, "Liability"),
            Self::Equity => write!(f, "Equity"),
            Self::Revenue => write!(f, "Revenue"),
            Self::Expense => write!(f, "Expense"),
        }
    }
}

impl AccountCategory {
    /// Parse a category from a string (case-insensitive).
    pub fn from_str(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "asset" => Some(Self::Asset),
            "liability" => Some(Self::Liability),
            "equity" => Some(Self::Equity),
            "revenue" => Some(Self::Revenue),
            "expense" => Some(Self::Expense),
            _ => None,
        }
    }

    /// Canonical string representation.
    pub fn to_str(self) -> &'static str {
        match self {
            Self::Asset => "Asset",
            Self::Liability => "Liability",
            Self::Equity => "Equity",
            Self::Revenue => "Revenue",
            Self::Expense => "Expense",
        }
    }
}

/// A single account suggestion produced by the classifier.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountSuggestion {
    pub category: AccountCategory,
    /// Human-readable account type, e.g. "Cash", "Accounts Receivable".
    pub account_type: String,
    /// Confidence score in [0.0, 1.0].
    pub confidence: f32,
    /// Explanation of why this suggestion was made.
    pub reasoning: String,
    /// Suggested account number from the default chart of accounts, if any.
    pub account_number: Option<String>,
}

/// An entry in the chart of accounts used for fuzzy matching.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChartOfAccountsEntry {
    pub number: String,
    pub name: String,
    pub category: AccountCategory,
    pub account_type: String,
}

// ---------------------------------------------------------------------------
// Keyword / pattern data
// ---------------------------------------------------------------------------

/// Returns the keyword lists keyed by category.
fn category_keywords() -> HashMap<AccountCategory, Vec<&'static str>> {
    let mut m = HashMap::new();

    m.insert(
        AccountCategory::Asset,
        vec![
            "cash", "bank", "receivable", "inventory", "equipment", "furniture",
            "prepaid", "deposit", "investment", "asset", "petty cash",
        ],
    );

    m.insert(
        AccountCategory::Liability,
        vec![
            "payable", "loan", "mortgage", "tax payable", "accrued", "unearned",
            "debt", "credit card", "payroll liability",
        ],
    );

    m.insert(
        AccountCategory::Equity,
        vec![
            "capital", "equity", "retained earnings", "owner", "drawing",
            "common stock", "dividend",
        ],
    );

    m.insert(
        AccountCategory::Revenue,
        vec![
            "sales", "revenue", "income", "service", "consulting",
            "interest income", "rent income", "commission",
        ],
    );

    m.insert(
        AccountCategory::Expense,
        vec![
            "rent", "utilities", "supplies", "salary", "wage", "payroll",
            "insurance", "depreciation", "advertising", "marketing", "travel",
            "meals", "office", "professional", "legal", "accounting",
            "maintenance", "repair", "fuel", "shipping", "postage",
            "telephone", "internet", "software", "subscription", "training",
            "dues", "license", "permit", "bank fee", "interest expense",
            "bad debt", "charity", "donation",
        ],
    );

    m
}

/// Default chart-of-accounts mapping used for `account_number` suggestions.
/// The entries are (keyword_trigger, account_type_name, account_number).
fn default_account_map() -> Vec<(&'static str, &'static str, &'static str)> {
    vec![
        // Assets
        ("cash", "Cash", "1000"),
        ("petty cash", "Petty Cash", "1010"),
        ("receivable", "Accounts Receivable", "1020"),
        ("inventory", "Inventory", "1030"),
        ("prepaid", "Prepaid Expenses", "1040"),
        ("accumulated depreciation", "Accumulated Depreciation", "1050"),
        ("equipment", "Equipment", "1100"),
        ("furniture", "Furniture & Fixtures", "1200"),
        ("investment", "Investments", "1300"),
        ("deposit", "Deposits", "1400"),
        // Liabilities
        ("payable", "Accounts Payable", "2000"),
        ("accrued", "Accrued Liabilities", "2010"),
        ("payroll tax", "Payroll Tax Payable", "2020"),
        ("sales tax", "Sales Tax Payable", "2030"),
        ("notes payable", "Notes Payable", "2040"),
        ("loan", "Notes Payable", "2040"),
        ("mortgage", "Notes Payable", "2040"),
        ("credit card", "Credit Card Payable", "2050"),
        // Equity
        ("capital", "Owner's Capital", "3000"),
        ("owner", "Owner's Capital", "3000"),
        ("retained earnings", "Retained Earnings", "3010"),
        ("drawing", "Owner's Drawing", "3020"),
        ("common stock", "Common Stock", "3030"),
        ("dividend", "Dividends", "3040"),
        // Revenue
        ("sales", "Sales Revenue", "4000"),
        ("service", "Service Revenue", "4010"),
        ("consulting", "Service Revenue", "4010"),
        ("interest income", "Interest Income", "4020"),
        ("rent income", "Rental Income", "4030"),
        ("commission", "Commission Revenue", "4040"),
        // Expenses
        ("rent", "Rent Expense", "5000"),
        ("utilities", "Utilities Expense", "5010"),
        ("salary", "Salaries Expense", "5020"),
        ("wage", "Salaries Expense", "5020"),
        ("payroll", "Salaries Expense", "5020"),
        ("supplies", "Supplies Expense", "5030"),
        ("insurance", "Insurance Expense", "5040"),
        ("depreciation", "Depreciation Expense", "5050"),
        ("office", "Office Expense", "5060"),
        ("professional", "Professional Services", "5070"),
        ("legal", "Professional Services", "5070"),
        ("accounting", "Professional Services", "5070"),
        ("bank fee", "Bank Fees", "5080"),
        ("travel", "Travel Expense", "5090"),
        ("advertising", "Advertising Expense", "5100"),
        ("marketing", "Marketing Expense", "5110"),
        ("meals", "Meals & Entertainment", "5120"),
        ("maintenance", "Maintenance & Repair", "5130"),
        ("repair", "Maintenance & Repair", "5130"),
        ("fuel", "Vehicle Expense", "5140"),
        ("shipping", "Shipping & Postage", "5150"),
        ("postage", "Shipping & Postage", "5150"),
        ("telephone", "Telephone Expense", "5160"),
        ("internet", "Internet Expense", "5170"),
        ("software", "Software & Subscriptions", "5180"),
        ("subscription", "Software & Subscriptions", "5180"),
        ("training", "Training & Education", "5190"),
        ("dues", "Dues & Licenses", "5200"),
        ("license", "Dues & Licenses", "5200"),
        ("permit", "Dues & Licenses", "5200"),
        ("interest expense", "Interest Expense", "5210"),
        ("bad debt", "Bad Debt Expense", "5220"),
        ("charity", "Charitable Contributions", "5230"),
        ("donation", "Charitable Contributions", "5230"),
    ]
}

// ---------------------------------------------------------------------------
// Core classifier
// ---------------------------------------------------------------------------

/// Smart account classifier combining rule-based keyword matching with an
/// optional LLM-enhanced path.
pub struct AccountClassifier {
    config: ClassificationConfig,
    keywords: HashMap<AccountCategory, Vec<&'static str>>,
    account_map: Vec<(&'static str, &'static str, &'static str)>,
}

impl AccountClassifier {
    /// Create a classifier with the given configuration.
    pub fn new(config: ClassificationConfig) -> Self {
        info!(
            llm_enabled = config.llm_enabled,
            threshold = config.confidence_threshold,
            max_suggestions = config.max_suggestions,
            "AccountClassifier initialised"
        );
        Self {
            config,
            keywords: category_keywords(),
            account_map: default_account_map(),
        }
    }

    // -- Rule-based suggestion ------------------------------------------------

    /// Return the single best category suggestion using rule-based keyword
    /// matching.  Never fails — always produces a result.
    pub fn suggest_category(
        &self,
        account_name: &str,
        description: Option<&str>,
    ) -> AccountSuggestion {
        let haystack = Self::build_haystack(account_name, description);
        let scores = self.score_categories(&haystack);

        let (category, raw_score) = scores
            .into_iter()
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap_or((AccountCategory::Expense, 0.0));

        let match_count = self.count_matches(&haystack, &category);
        let confidence = if raw_score > 0.0 {
            // Length-weighted: short kw (~4 chars) → ~0.62, long kw (~9) → ~0.77,
            // two keywords (~14) → ~0.92, three+ → capped at 0.95.
            (0.5 + raw_score * 0.03).min(0.95)
        } else {
            0.3 // default low confidence when nothing matches
        };

        let (account_type, account_number) =
            self.resolve_account_type(&haystack, category);

        let reasoning = if match_count > 0 {
            format!(
                "Rule-based: {} keyword match(es) for category {}",
                match_count, category
            )
        } else {
            "No keyword matches — defaulting to Expense with low confidence".into()
        };

        debug!(
            account_name,
            %category,
            confidence,
            "suggest_category result"
        );

        AccountSuggestion {
            category,
            account_type,
            confidence,
            reasoning,
            account_number,
        }
    }

    // -- LLM-enhanced suggestion ---------------------------------------------

    /// LLM-enhanced classification.  Falls back to the rule-based engine when
    /// the LLM is disabled, the model file is missing, or inference fails.
    pub async fn suggest_category_with_llm(
        &self,
        account_name: &str,
        description: Option<&str>,
    ) -> AccountSuggestion {
        if !self.config.llm_enabled {
            debug!("LLM disabled — falling back to rule-based");
            return self.suggest_category(account_name, description);
        }

        let model_path = match &self.config.llm_model_path {
            Some(p) if !p.is_empty() => p.clone(),
            _ => {
                warn!("LLM enabled but no model path configured — falling back");
                return self.suggest_category(account_name, description);
            }
        };

        // Verify model file exists.
        if !std::path::Path::new(&model_path).exists() {
            warn!(%model_path, "LLM model file not found — falling back");
            return self.suggest_category(account_name, description);
        }

        match self.run_llm_classification(&model_path, account_name, description).await {
            Ok(llm_suggestion) => {
                // Merge: if the LLM agrees with the rule-based engine, boost
                // confidence.  Otherwise trust the LLM but cap confidence.
                let rule_suggestion = self.suggest_category(account_name, description);
                let merged = self.merge_suggestions(rule_suggestion, llm_suggestion);
                info!(
                    account_name,
                    category = %merged.category,
                    confidence = merged.confidence,
                    "LLM-enhanced classification complete"
                );
                merged
            }
            Err(e) => {
                warn!(error = %e, "LLM inference failed — falling back to rule-based");
                self.suggest_category(account_name, description)
            }
        }
    }

    // -- Multiple suggestions -------------------------------------------------

    /// Return the top-N account type suggestions ranked by confidence.
    pub fn suggest_account_type(&self, account_name: &str) -> Vec<AccountSuggestion> {
        let haystack = account_name.to_lowercase();
        let scores = self.score_categories(&haystack);
        let mut suggestions: Vec<AccountSuggestion> = scores
            .into_iter()
            .filter(|(_, score)| *score > 0.0)
            .map(|(category, raw_score)| {
                let match_count = self.count_matches(&haystack, &category);
                let confidence = (0.5 + raw_score * 0.03).min(0.95);
                let (account_type, account_number) =
                    self.resolve_account_type(&haystack, category);
                AccountSuggestion {
                    category,
                    account_type,
                    confidence,
                    reasoning: format!(
                        "Rule-based: {} keyword match(es) for category {}",
                        match_count, category
                    ),
                    account_number,
                }
            })
            .collect();

        // Sort descending by confidence.
        suggestions.sort_by(|a, b| {
            b.confidence
                .partial_cmp(&a.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        suggestions.truncate(self.config.max_suggestions);

        // If nothing matched at all, return a single low-confidence Expense.
        if suggestions.is_empty() {
            let (account_type, account_number) =
                self.resolve_account_type(&haystack, AccountCategory::Expense);
            suggestions.push(AccountSuggestion {
                category: AccountCategory::Expense,
                account_type,
                confidence: 0.3,
                reasoning: "No keyword matches — defaulting to Expense".into(),
                account_number,
            });
        }

        suggestions
    }

    // -- Fuzzy matching against existing chart of accounts --------------------

    /// Find the best matching entry in the provided chart of accounts using
    /// substring matching and Levenshtein distance.
    pub fn match_existing_account(
        &self,
        account_name: &str,
        chart: &[ChartOfAccountsEntry],
    ) -> Option<ChartOfAccountsEntry> {
        if chart.is_empty() {
            return None;
        }

        let needle = account_name.to_lowercase();

        let mut best: Option<(f32, &ChartOfAccountsEntry)> = None;

        for entry in chart {
            let candidate = entry.name.to_lowercase();

            // Exact match.
            if needle == candidate {
                return Some(entry.clone());
            }

            // Substring match (either direction).
            let substring_score = if needle.contains(&candidate) || candidate.contains(&needle) {
                let ratio = needle.len().min(candidate.len()) as f32
                    / needle.len().max(candidate.len()) as f32;
                0.6 + 0.3 * ratio // 0.6 – 0.9
            } else {
                0.0
            };

            // Word-overlap score: fraction of significant words shared
            // between the two strings (handles re-ordered words like
            // "Operating Cash" ↔ "Cash - Operating").
            let needle_words: Vec<&str> = needle
                .split(|c: char| !c.is_alphanumeric())
                .filter(|w| w.len() > 1)
                .collect();
            let candidate_words: Vec<&str> = candidate
                .split(|c: char| !c.is_alphanumeric())
                .filter(|w| w.len() > 1)
                .collect();
            let word_overlap_score = if !needle_words.is_empty() && !candidate_words.is_empty() {
                let shared = needle_words
                    .iter()
                    .filter(|w| candidate_words.contains(w))
                    .count();
                let max_words = needle_words.len().max(candidate_words.len()) as f32;
                if shared > 0 {
                    (shared as f32 / max_words) * 0.85
                } else {
                    0.0
                }
            } else {
                0.0
            };

            // Levenshtein similarity.
            let lev_dist = levenshtein_distance(&needle, &candidate);
            let max_len = needle.len().max(candidate.len()) as f32;
            let lev_score = if max_len > 0.0 {
                1.0 - (lev_dist as f32 / max_len)
            } else {
                0.0
            };

            // Blend: prefer substring when available, then word overlap,
            // else Levenshtein.
            let combined = if substring_score > 0.0 {
                substring_score.max(word_overlap_score).max(lev_score * 0.8)
            } else if word_overlap_score > 0.0 {
                word_overlap_score.max(lev_score * 0.8)
            } else {
                lev_score * 0.8
            };

            // Only consider if above a reasonable threshold.
            if combined >= 0.4 {
                if best.map_or(true, |(best_score, _)| combined > best_score) {
                    best = Some((combined, entry));
                }
            }
        }

        best.map(|(_, entry)| entry.clone())
    }

    // -- Transaction description classification -------------------------------

    /// Classify an account from a free-form transaction description such as
    /// "Paid rent for office space" or "Received payment from client".
    pub fn classify_transaction_description(
        &self,
        description: &str,
    ) -> AccountSuggestion {
        self.suggest_category(description, None)
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    /// Build the combined search text from name and optional description.
    fn build_haystack(account_name: &str, description: Option<&str>) -> String {
        let mut h = account_name.to_lowercase();
        if let Some(desc) = description {
            h.push(' ');
            h.push_str(&desc.to_lowercase());
        }
        h
    }

    /// Score every category against the haystack.
    ///
    /// Returns `(category, weighted_score)` where the score is the sum of
    /// matched keyword lengths.  This gives more specific (longer) keywords
    /// greater weight — e.g. "equipment" (9) outranks "office" (6) when both
    /// appear in a description like "Bought new office equipment".
    fn score_categories(&self, haystack: &str) -> Vec<(AccountCategory, f32)> {
        self.keywords
            .iter()
            .map(|(&cat, keywords)| {
                let score: f32 = keywords
                    .iter()
                    .filter(|kw| haystack.contains(**kw))
                    .map(|kw| kw.len() as f32)
                    .sum();
                (cat, score)
            })
            .collect()
    }

    /// Count how many keywords matched for a given category (for display
    /// in reasoning strings).
    fn count_matches(&self, haystack: &str, category: &AccountCategory) -> usize {
        self.keywords
            .get(category)
            .map(|kws| kws.iter().filter(|kw| haystack.contains(**kw)).count())
            .unwrap_or(0)
    }

    /// Given a haystack and a decided category, pick the most specific
    /// account type and number from the default chart.
    fn resolve_account_type(
        &self,
        haystack: &str,
        category: AccountCategory,
    ) -> (String, Option<String>) {
        // Walk the account map looking for the most specific (longest)
        // keyword that appears in the haystack AND belongs to the category.
        let mut best: Option<(&str, &str, usize)> = None; // (type, number, kw_len)

        // We need the keywords for this category to gate matches.
        let cat_kws: &[&str] = self
            .keywords
            .get(&category)
            .map(|v| v.as_slice())
            .unwrap_or(&[]);

        for &(kw, acct_type, acct_num) in &self.account_map {
            // Only consider entries whose trigger keyword appears in the haystack.
            if !haystack.contains(kw) {
                continue;
            }
            // Additionally, the trigger keyword should be associated with the
            // chosen category (either it IS a category keyword or is a more
            // specific sub-keyword).
            let relevant = cat_kws.iter().any(|ckw| {
                kw.contains(ckw) || ckw.contains(kw)
            });
            if !relevant {
                continue;
            }
            let kw_len = kw.len();
            if best.map_or(true, |(_, _, blen)| kw_len > blen) {
                best = Some((acct_type, acct_num, kw_len));
            }
        }

        match best {
            Some((acct_type, acct_num, _)) => {
                (acct_type.to_string(), Some(acct_num.to_string()))
            }
            None => {
                // Fallback generic name based on category.
                let generic = match category {
                    AccountCategory::Asset => "Other Asset",
                    AccountCategory::Liability => "Other Liability",
                    AccountCategory::Equity => "Other Equity",
                    AccountCategory::Revenue => "Other Revenue",
                    AccountCategory::Expense => "Other Expense",
                };
                (generic.to_string(), None)
            }
        }
    }

    /// Merge a rule-based suggestion with an LLM suggestion.
    fn merge_suggestions(
        &self,
        rule: AccountSuggestion,
        llm: AccountSuggestion,
    ) -> AccountSuggestion {
        if rule.category == llm.category {
            // Agreement — boost confidence (average, capped at 0.98).
            let merged_confidence =
                ((rule.confidence + llm.confidence) / 2.0).min(0.98);
            AccountSuggestion {
                category: rule.category,
                account_type: llm.account_type.clone(),
                confidence: merged_confidence,
                reasoning: format!(
                    "Rule-based + LLM agree on {}. Rule: {}; LLM: {}",
                    rule.category, rule.reasoning, llm.reasoning
                ),
                account_number: llm.account_number.or(rule.account_number),
            }
        } else {
            // Disagreement — prefer LLM but lower confidence.
            AccountSuggestion {
                category: llm.category,
                account_type: llm.account_type.clone(),
                confidence: (llm.confidence * 0.8).min(0.9),
                reasoning: format!(
                    "LLM disagrees with rule-based. LLM chose {} ({}), rule chose {} ({}). Trusting LLM.",
                    llm.category, llm.account_type, rule.category, rule.account_type
                ),
                account_number: llm.account_number.or(rule.account_number),
            }
        }
    }

    /// Run the Qwen GGUF model for classification.  Returns an
    /// `AccountSuggestion` on success or an error on any failure path.
    #[cfg(feature = "llm")]
    async fn run_llm_classification(
        &self,
        model_path: &str,
        account_name: &str,
        description: Option<&str>,
    ) -> Result<AccountSuggestion> {
        let desc_text = description.unwrap_or("N/A");
        let prompt = format!(
            "Classify this account: '{}'. Description: '{}'. Choose from: \
             Asset, Liability, Equity, Revenue, Expense. Return JSON: \
             {{\"category\": \"...\", \"account_type\": \"...\", \
             \"confidence\": 0.0, \"reasoning\": \"...\"}}",
            account_name, desc_text
        );

        debug!(prompt = %prompt, "Sending classification prompt to LLM");

        let model = llama_cpp_rs::LlamaModel::from_file(
            model_path,
            llama_cpp_rs::LlamaModelParams::default(),
        )
        .context("Failed to load GGUF model")?;

        let mut session = model
            .create_session(512)
            .context("Failed to create LLM session")?;

        session
            .advance_dialogue(&prompt)
            .context("LLM inference failed")?;

        let mut response = String::new();
        while let Some(token) = session.next_token() {
            if session.is_eog_token(token) {
                break;
            }
            response.push_str(&token.to_string());
        }

        debug!(response = %response, "LLM raw response");

        let json_str = extract_json_object(&response)
            .unwrap_or_else(|| response.trim().to_string());

        let parsed: LlmClassificationResponse = serde_json::from_str(&json_str)
            .context(format!(
                "Failed to parse LLM JSON response: {}",
                json_str
            ))?;

        let category = AccountCategory::from_str(&parsed.category)
            .unwrap_or(AccountCategory::Expense);

        Ok(AccountSuggestion {
            category,
            account_type: parsed.account_type,
            confidence: parsed.confidence.clamp(0.0, 1.0),
            reasoning: parsed.reasoning,
            account_number: None,
        })
    }

    /// Stub when the `llm` feature is not enabled.
    #[cfg(not(feature = "llm"))]
    async fn run_llm_classification(
        &self,
        _model_path: &str,
        _account_name: &str,
        _description: Option<&str>,
    ) -> Result<AccountSuggestion> {
        Err(anyhow::anyhow!(
            "LLM classification requires the `llm` feature (llama_cpp_rs). \
             Enable with: cargo build --features llm"
        ))
    }
}

impl Default for AccountClassifier {
    fn default() -> Self {
        Self::new(ClassificationConfig::default())
    }
}

// ---------------------------------------------------------------------------
// LLM response shape (only used when `llm` feature is enabled)
// ---------------------------------------------------------------------------

#[cfg(feature = "llm")]
#[derive(Debug, Deserialize)]
struct LlmClassificationResponse {
    category: String,
    account_type: String,
    confidence: f32,
    reasoning: String,
}

// ---------------------------------------------------------------------------
// Utility: JSON extraction from noisy LLM output (only used with `llm`)
// ---------------------------------------------------------------------------

/// Attempt to extract the first `{...}` JSON object from a string that may
/// contain surrounding prose or markdown fences.
fn extract_json_object(text: &str) -> Option<String> {
    let start = text.find('{')?;
    let mut depth: i32 = 0;
    for (i, ch) in text[start..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(text[start..start + i + 1].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Utility: Levenshtein distance
// ---------------------------------------------------------------------------

/// Compute the Levenshtein edit distance between two strings.
fn levenshtein_distance(a: &str, b: &str) -> usize {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let a_len = a_chars.len();
    let b_len = b_chars.len();

    if a_len == 0 {
        return b_len;
    }
    if b_len == 0 {
        return a_len;
    }

    // Single-row optimisation.
    let mut prev: Vec<usize> = (0..=b_len).collect();
    let mut curr = vec![0usize; b_len + 1];

    for i in 1..=a_len {
        curr[0] = i;
        for j in 1..=b_len {
            let cost = if a_chars[i - 1] == b_chars[j - 1] { 0 } else { 1 };
            curr[j] = (prev[j] + 1)
                .min(curr[j - 1] + 1)
                .min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[b_len]
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn classifier() -> AccountClassifier {
        AccountClassifier::default()
    }

    fn sample_chart() -> Vec<ChartOfAccountsEntry> {
        vec![
            ChartOfAccountsEntry {
                number: "1000".into(),
                name: "Cash - Operating".into(),
                category: AccountCategory::Asset,
                account_type: "Cash".into(),
            },
            ChartOfAccountsEntry {
                number: "1020".into(),
                name: "Accounts Receivable".into(),
                category: AccountCategory::Asset,
                account_type: "Accounts Receivable".into(),
            },
            ChartOfAccountsEntry {
                number: "2000".into(),
                name: "Accounts Payable".into(),
                category: AccountCategory::Liability,
                account_type: "Accounts Payable".into(),
            },
            ChartOfAccountsEntry {
                number: "4000".into(),
                name: "Sales Revenue".into(),
                category: AccountCategory::Revenue,
                account_type: "Sales Revenue".into(),
            },
            ChartOfAccountsEntry {
                number: "5000".into(),
                name: "Rent Expense".into(),
                category: AccountCategory::Expense,
                account_type: "Rent Expense".into(),
            },
            ChartOfAccountsEntry {
                number: "5060".into(),
                name: "Office Expense".into(),
                category: AccountCategory::Expense,
                account_type: "Office Expense".into(),
            },
        ]
    }

    // -- Config defaults -----------------------------------------------------

    #[test]
    fn test_config_defaults() {
        let cfg = ClassificationConfig::default();
        assert!(!cfg.llm_enabled);
        assert!(cfg.llm_model_path.is_none());
        assert!((cfg.confidence_threshold - 0.7).abs() < f32::EPSILON);
        assert_eq!(cfg.max_suggestions, 3);
    }

    // -- AccountCategory -----------------------------------------------------

    #[test]
    fn test_category_display_and_roundtrip() {
        for cat in [
            AccountCategory::Asset,
            AccountCategory::Liability,
            AccountCategory::Equity,
            AccountCategory::Revenue,
            AccountCategory::Expense,
        ] {
            let s = cat.to_str();
            let parsed = AccountCategory::from_str(s).unwrap();
            assert_eq!(cat, parsed);
            assert_eq!(format!("{}", cat), s);
        }
    }

    #[test]
    fn test_category_from_str_case_insensitive() {
        assert_eq!(AccountCategory::from_str("ASSET"), Some(AccountCategory::Asset));
        assert_eq!(AccountCategory::from_str("expense"), Some(AccountCategory::Expense));
        assert_eq!(AccountCategory::from_str(" Revenue "), Some(AccountCategory::Revenue));
        assert_eq!(AccountCategory::from_str("bogus"), None);
    }

    // -- Rule-based categorization -------------------------------------------

    #[test]
    fn test_office_rent_is_expense() {
        let s = classifier().suggest_category("Office Rent", None);
        assert_eq!(s.category, AccountCategory::Expense);
        assert!(s.account_type.to_lowercase().contains("rent") || s.account_type.contains("Office"));
        assert!(s.confidence > 0.5, "confidence was {}", s.confidence);
    }

    #[test]
    fn test_cash_account_is_asset() {
        let s = classifier().suggest_category("Cash Account", None);
        assert_eq!(s.category, AccountCategory::Asset);
        assert_eq!(s.account_type, "Cash");
        assert_eq!(s.account_number.as_deref(), Some("1000"));
    }

    #[test]
    fn test_accounts_payable_is_liability() {
        let s = classifier().suggest_category("Accounts Payable", None);
        assert_eq!(s.category, AccountCategory::Liability);
        assert_eq!(s.account_type, "Accounts Payable");
        assert_eq!(s.account_number.as_deref(), Some("2000"));
    }

    #[test]
    fn test_sales_revenue_is_revenue() {
        let s = classifier().suggest_category("Sales Revenue", None);
        assert_eq!(s.category, AccountCategory::Revenue);
        assert_eq!(s.account_type, "Sales Revenue");
        assert_eq!(s.account_number.as_deref(), Some("4000"));
    }

    #[test]
    fn test_common_stock_is_equity() {
        let s = classifier().suggest_category("Common Stock", None);
        assert_eq!(s.category, AccountCategory::Equity);
    }

    #[test]
    fn test_unknown_account_defaults_expense_low_confidence() {
        let s = classifier().suggest_category("Unknown Account", None);
        assert_eq!(s.category, AccountCategory::Expense);
        assert!(s.confidence <= 0.35, "confidence was {}", s.confidence);
    }

    #[test]
    fn test_description_boosts_confidence() {
        let without = classifier().suggest_category("Checking", None);
        let with = classifier().suggest_category("Checking", Some("Bank account for daily operations"));
        // The description adds "bank" which is an asset keyword.
        assert!(
            with.confidence >= without.confidence,
            "with={} without={}",
            with.confidence,
            without.confidence
        );
    }

    #[test]
    fn test_multiple_keyword_matches_higher_confidence() {
        // "Petty Cash" matches both "petty cash" and "cash"
        let s = classifier().suggest_category("Petty Cash", None);
        assert_eq!(s.category, AccountCategory::Asset);
        assert!(s.confidence >= 0.6, "confidence was {}", s.confidence);
    }

    // -- Multiple suggestions ------------------------------------------------

    #[test]
    fn test_suggest_account_type_returns_multiple() {
        let suggestions = classifier().suggest_account_type("Office Supplies Expense");
        assert!(!suggestions.is_empty());
        // First suggestion should be the strongest.
        assert!(suggestions[0].confidence >= suggestions.last().unwrap().confidence);
    }

    #[test]
    fn test_suggest_account_type_max_respected() {
        let cfg = ClassificationConfig {
            max_suggestions: 2,
            ..ClassificationConfig::default()
        };
        let c = AccountClassifier::new(cfg);
        let suggestions = c.suggest_account_type("Office Rent Expense");
        assert!(suggestions.len() <= 2);
    }

    #[test]
    fn test_suggest_account_type_unknown_returns_default() {
        let suggestions = classifier().suggest_account_type("xyzzy");
        assert!(!suggestions.is_empty());
        assert_eq!(suggestions[0].category, AccountCategory::Expense);
        assert!(suggestions[0].confidence <= 0.35);
    }

    // -- Fuzzy matching ------------------------------------------------------

    #[test]
    fn test_match_existing_account_exact() {
        let chart = sample_chart();
        let m = classifier().match_existing_account("Accounts Payable", &chart);
        assert!(m.is_some());
        assert_eq!(m.unwrap().number, "2000");
    }

    #[test]
    fn test_match_existing_account_substring() {
        let chart = sample_chart();
        let m = classifier().match_existing_account("Operating Cash", &chart);
        assert!(m.is_some());
        assert_eq!(m.unwrap().number, "1000");
    }

    #[test]
    fn test_match_existing_account_fuzzy() {
        let chart = sample_chart();
        let m = classifier().match_existing_account("Rent Exp", &chart);
        assert!(m.is_some());
        assert_eq!(m.unwrap().number, "5000");
    }

    #[test]
    fn test_match_existing_account_empty_chart() {
        let m = classifier().match_existing_account("Cash", &[]);
        assert!(m.is_none());
    }

    #[test]
    fn test_match_existing_account_no_match() {
        let chart = sample_chart();
        let m = classifier().match_existing_account("Zzzzzzzzzz", &chart);
        assert!(m.is_none());
    }

    // -- Transaction description classification ------------------------------

    #[test]
    fn test_classify_transaction_rent() {
        let s = classifier().classify_transaction_description("Paid rent for office");
        assert_eq!(s.category, AccountCategory::Expense);
        assert!(s.confidence > 0.4);
    }

    #[test]
    fn test_classify_transaction_payment_received() {
        let s = classifier().classify_transaction_description(
            "Received payment from client for consulting services",
        );
        assert_eq!(s.category, AccountCategory::Revenue);
    }

    #[test]
    fn test_classify_transaction_equipment_purchase() {
        let s =
            classifier().classify_transaction_description("Bought new office equipment");
        assert_eq!(s.category, AccountCategory::Asset);
    }

    #[test]
    fn test_classify_transaction_loan_payment() {
        let s =
            classifier().classify_transaction_description("Monthly loan payment");
        assert_eq!(s.category, AccountCategory::Liability);
    }

    // -- LLM fallback -------------------------------------------------------

    #[tokio::test]
    async fn test_llm_disabled_falls_back() {
        let c = classifier();
        let s = c
            .suggest_category_with_llm("Cash Account", None)
            .await;
        assert_eq!(s.category, AccountCategory::Asset);
    }

    #[tokio::test]
    async fn test_llm_enabled_missing_model_falls_back() {
        let cfg = ClassificationConfig {
            llm_enabled: true,
            llm_model_path: Some("/nonexistent/model.gguf".into()),
            ..ClassificationConfig::default()
        };
        let c = AccountClassifier::new(cfg);
        let s = c
            .suggest_category_with_llm("Cash Account", None)
            .await;
        assert_eq!(s.category, AccountCategory::Asset);
    }

    #[tokio::test]
    async fn test_llm_enabled_no_path_falls_back() {
        let cfg = ClassificationConfig {
            llm_enabled: true,
            llm_model_path: None,
            ..ClassificationConfig::default()
        };
        let c = AccountClassifier::new(cfg);
        let s = c
            .suggest_category_with_llm("Sales Revenue", None)
            .await;
        assert_eq!(s.category, AccountCategory::Revenue);
    }

    // -- Utility functions ---------------------------------------------------

    #[test]
    fn test_levenshtein_identical() {
        assert_eq!(levenshtein_distance("hello", "hello"), 0);
    }

    #[test]
    fn test_levenshtein_empty() {
        assert_eq!(levenshtein_distance("", "abc"), 3);
        assert_eq!(levenshtein_distance("abc", ""), 3);
    }

    #[test]
    fn test_levenshtein_typical() {
        assert_eq!(levenshtein_distance("kitten", "sitting"), 3);
    }

    #[test]
    fn test_extract_json_object_plain() {
        let input = r#"{"category":"Asset","account_type":"Cash","confidence":0.9,"reasoning":"cash"}"#;
        let extracted = extract_json_object(input).unwrap();
        assert!(extracted.starts_with('{'));
        assert!(extracted.ends_with('}'));
    }

    #[test]
    fn test_extract_json_object_with_prose() {
        let input = r#"Here is the result: {"category":"Expense"} and more text."#;
        let extracted = extract_json_object(input).unwrap();
        assert_eq!(extracted, r#"{"category":"Expense"}"#);
    }

    #[test]
    fn test_extract_json_object_markdown_fence() {
        let input = "```json\n{\"category\":\"Revenue\"}\n```";
        let extracted = extract_json_object(input).unwrap();
        assert_eq!(extracted, r#"{"category":"Revenue"}"#);
    }

    #[test]
    fn test_extract_json_object_none() {
        assert!(extract_json_object("no json here").is_none());
    }

    // -- Account number suggestions ------------------------------------------

    #[test]
    fn test_petty_cash_account_number() {
        let s = classifier().suggest_category("Petty Cash", None);
        assert_eq!(s.account_number.as_deref(), Some("1010"));
    }

    #[test]
    fn test_equipment_account_number() {
        let s = classifier().suggest_category("Equipment", None);
        assert_eq!(s.category, AccountCategory::Asset);
        assert_eq!(s.account_number.as_deref(), Some("1100"));
    }

    #[test]
    fn test_insurance_expense_account_number() {
        let s = classifier().suggest_category("Insurance", None);
        assert_eq!(s.category, AccountCategory::Expense);
        assert_eq!(s.account_number.as_deref(), Some("5040"));
    }

    #[test]
    fn test_retained_earnings_is_equity() {
        let s = classifier().suggest_category("Retained Earnings", None);
        assert_eq!(s.category, AccountCategory::Equity);
        assert_eq!(s.account_number.as_deref(), Some("3010"));
    }

    // -- Serde roundtrip -----------------------------------------------------

    #[test]
    fn test_suggestion_serde_roundtrip() {
        let s = AccountSuggestion {
            category: AccountCategory::Asset,
            account_type: "Cash".into(),
            confidence: 0.85,
            reasoning: "test".into(),
            account_number: Some("1000".into()),
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: AccountSuggestion = serde_json::from_str(&json).unwrap();
        assert_eq!(back.category, AccountCategory::Asset);
        assert_eq!(back.account_type, "Cash");
        assert!((back.confidence - 0.85).abs() < f32::EPSILON);
    }

    #[test]
    fn test_chart_entry_serde_roundtrip() {
        let e = ChartOfAccountsEntry {
            number: "1000".into(),
            name: "Cash".into(),
            category: AccountCategory::Asset,
            account_type: "Cash".into(),
        };
        let json = serde_json::to_string(&e).unwrap();
        let back: ChartOfAccountsEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(back.number, "1000");
        assert_eq!(back.category, AccountCategory::Asset);
    }
}
