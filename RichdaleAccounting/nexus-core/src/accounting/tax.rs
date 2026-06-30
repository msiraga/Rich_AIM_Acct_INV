//! Tax Module
//!
//! Handles tax calculations, filings, and compliance.

use async_trait::async_trait;
use serde::{Serialize, Deserialize};
use std::collections::{HashMap, BTreeMap};
use std::fmt;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use uuid::Uuid;
use chrono::{DateTime, Utc, NaiveDate};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use tracing::{info, error, debug, warn};
use crate::database::financial::{Account, AccountType, EntryType, Transaction, TransactionEntry, TransactionType, TransactionStatus};
use crate::database::error::DatabaseError;
use crate::agents::agent_types::{Agent, AgentConfig, AgentType, AgentStatus};
use crate::agents::task::{Task, TaskResult, TaskPayload};
use crate::agents::error::AgentError;

/// Tax error types
#[derive(Debug, thiserror::Error)]
pub enum TaxError {
    /// Invalid tax rate
    #[error("Invalid tax rate: {0}")]
    InvalidTaxRate(String),
    
    /// Tax calculation error
    #[error("Tax calculation error: {0}")]
    CalculationError(String),
    
    /// Tax jurisdiction not found
    #[error("Tax jurisdiction not found: {0}")]
    JurisdictionNotFound(String),
    
    /// Tax period not valid
    #[error("Invalid tax period: {0}")]
    InvalidTaxPeriod(String),
    
    /// Filing error
    #[error("Filing error: {0}")]
    FilingError(String),
    
    /// Database error
    #[error("Database error: {0}")]
    DatabaseError(#[from] DatabaseError),
    
    /// Any other tax error
    #[error("Tax error: {0}")]
    Other(String),
}

impl TaxError {
    /// Create a new other error
    pub fn other(msg: &str) -> Self {
        Self::Other(msg.to_string())
    }
}

/// Result type for tax operations
pub type TaxResult<T> = Result<T, TaxError>;

/// Tax jurisdiction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaxJurisdiction {
    /// Unique identifier
    pub id: Uuid,
    /// Jurisdiction name
    pub name: String,
    /// Jurisdiction code
    pub code: String,
    /// Jurisdiction type
    pub jurisdiction_type: JurisdictionType,
    /// Tax rates (flat, for simple jurisdictions)
    pub tax_rates: HashMap<TaxType, Decimal>,
    /// Progressive tax brackets (for income tax in multi-bracket jurisdictions)
    pub tax_brackets: Vec<TaxBracket>,
    /// Filing frequency
    pub filing_frequency: FilingFrequency,
    /// Effective date
    pub effective_date: NaiveDate,
    /// Expiration date
    pub expiration_date: Option<NaiveDate>,
    /// Whether the jurisdiction is active
    pub is_active: bool,
    /// Created timestamp
    pub created_at: DateTime<Utc>,
    /// Updated timestamp
    pub updated_at: DateTime<Utc>,
}

impl Default for TaxJurisdiction {
    fn default() -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            name: String::new(),
            code: String::new(),
            jurisdiction_type: JurisdictionType::default(),
            tax_rates: HashMap::new(),
            tax_brackets: Vec::new(),
            filing_frequency: FilingFrequency::default(),
            effective_date: now.date_naive(),
            expiration_date: None,
            is_active: true,
            created_at: now,
            updated_at: now,
        }
    }
}

/// Progressive tax bracket
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaxBracket {
    pub lower_bound: Decimal,
    pub upper_bound: Option<Decimal>, // None = unlimited
    pub rate: Decimal,
}

/// Jurisdiction type
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum JurisdictionType {
    /// Federal jurisdiction
    Federal,
    /// State/Province jurisdiction
    State,
    /// Local/City jurisdiction
    Local,
    /// International jurisdiction
    International,
}

impl Default for JurisdictionType {
    fn default() -> Self {
        Self::Federal
    }
}

/// Tax type
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum TaxType {
    /// Income tax
    Income,
    /// Sales tax
    Sales,
    /// Payroll tax
    Payroll,
    /// Property tax
    Property,
    /// Value-added tax (VAT)
    VAT,
    /// Goods and Services Tax (GST)
    GST,
    /// Other tax type
    Other(String),
}

impl Default for TaxType {
    fn default() -> Self {
        Self::Income
    }
}

impl fmt::Display for TaxType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TaxType::Income => write!(f, "Income"),
            TaxType::Sales => write!(f, "Sales"),
            TaxType::Payroll => write!(f, "Payroll"),
            TaxType::Property => write!(f, "Property"),
            TaxType::VAT => write!(f, "VAT"),
            TaxType::GST => write!(f, "GST"),
            TaxType::Other(s) => write!(f, "Other({})", s),
        }
    }
}

/// Filing frequency
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum FilingFrequency {
    /// Annual filing
    Annual,
    /// Quarterly filing
    Quarterly,
    /// Monthly filing
    Monthly,
    /// Weekly filing
    Weekly,
    /// Bi-weekly filing
    BiWeekly,
}

impl Default for FilingFrequency {
    fn default() -> Self {
        Self::Annual
    }
}

/// Tax calculation result
#[derive(Debug, Clone, Serialize)]
pub struct TaxCalculation {
    /// Tax jurisdiction
    pub jurisdiction: TaxJurisdiction,
    /// Tax type
    pub tax_type: TaxType,
    /// Taxable amount
    pub taxable_amount: Decimal,
    /// Tax rate
    pub tax_rate: Decimal,
    /// Tax amount
    pub tax_amount: Decimal,
    /// Calculation date
    pub calculation_date: DateTime<Utc>,
    /// Period start date
    pub period_start: NaiveDate,
    /// Period end date
    pub period_end: NaiveDate,
}

/// Tax filing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaxFiling {
    /// Unique identifier
    pub id: Uuid,
    /// Tax jurisdiction ID
    pub jurisdiction_id: Uuid,
    /// Tax type
    pub tax_type: TaxType,
    /// Filing period start
    pub period_start: NaiveDate,
    /// Filing period end
    pub period_end: NaiveDate,
    /// Taxable amount
    pub taxable_amount: Decimal,
    /// Tax amount
    pub tax_amount: Decimal,
    /// Filing status
    pub status: FilingStatus,
    /// Filing date
    pub filing_date: Option<DateTime<Utc>>,
    /// Due date
    pub due_date: Option<DateTime<Utc>>,
    /// Payment date
    pub payment_date: Option<DateTime<Utc>>,
    /// Payment amount
    pub payment_amount: Option<Decimal>,
    /// Confirmation number
    pub confirmation_number: Option<String>,
    /// Notes
    pub notes: String,
    /// Created timestamp
    pub created_at: DateTime<Utc>,
    /// Updated timestamp
    pub updated_at: DateTime<Utc>,
}

impl Default for TaxFiling {
    fn default() -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            jurisdiction_id: Uuid::new_v4(),
            tax_type: TaxType::default(),
            period_start: now.date_naive(),
            period_end: now.date_naive(),
            taxable_amount: dec!(0),
            tax_amount: dec!(0),
            status: FilingStatus::default(),
            filing_date: None,
            due_date: None,
            payment_date: None,
            payment_amount: None,
            confirmation_number: None,
            notes: String::new(),
            created_at: now,
            updated_at: now,
        }
    }
}

/// Filing status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum FilingStatus {
    /// Not started
    NotStarted,
    /// In progress
    InProgress,
    /// Ready to file
    ReadyToFile,
    /// Filed
    Filed,
    /// Paid
    Paid,
    /// Cancelled
    Cancelled,
    /// Overdue
    Overdue,
}

impl Default for FilingStatus {
    fn default() -> Self {
        Self::NotStarted
    }
}

/// Tax calculator
#[derive(Debug, Clone)]
pub struct TaxCalculator {
    /// Map of jurisdiction ID to jurisdiction
    pub jurisdictions: Arc<RwLock<BTreeMap<Uuid, TaxJurisdiction>>>,
    /// Map of jurisdiction code to jurisdiction ID
    pub jurisdictions_by_code: Arc<RwLock<HashMap<String, Uuid>>>,
    /// Map of tax filing ID to filing
    pub filings: Arc<RwLock<BTreeMap<Uuid, TaxFiling>>>,
    /// Optional SurrealDB connection for persistence
    pub db: Option<Arc<crate::database::Database>>,
}

impl Default for TaxCalculator {
    fn default() -> Self {
        Self::new()
    }
}

impl TaxCalculator {
    /// Create a new tax calculator
    pub fn new() -> Self {
        Self {
            jurisdictions: Arc::new(RwLock::new(BTreeMap::new())),
            jurisdictions_by_code: Arc::new(RwLock::new(HashMap::new())),
            filings: Arc::new(RwLock::new(BTreeMap::new())),
            db: None,
        }
    }

    /// Initialize the tax calculator with default jurisdictions
    pub async fn initialize(&mut self) -> TaxResult<()> {
        info!("Initializing Tax Calculator...");
        
        // Create default jurisdictions
        self.create_default_jurisdictions().await?;
        
        Ok(())
    }

    /// Create default jurisdictions
    async fn create_default_jurisdictions(&mut self) -> TaxResult<()> {
        // US Federal
        let mut federal = TaxJurisdiction::default();
        federal.name = "United States Federal".to_string();
        federal.code = "US-FED".to_string();
        federal.jurisdiction_type = JurisdictionType::Federal;
        federal.tax_rates.insert(TaxType::Income, dec!(0.21));
        federal.tax_rates.insert(TaxType::Payroll, dec!(0.153));
        federal.filing_frequency = FilingFrequency::Annual;
        self.create_jurisdiction(federal).await?;
        
        // California State
        let mut california = TaxJurisdiction::default();
        california.name = "California State".to_string();
        california.code = "US-CA".to_string();
        california.jurisdiction_type = JurisdictionType::State;
        california.tax_rates.insert(TaxType::Income, dec!(0.093));
        california.tax_rates.insert(TaxType::Sales, dec!(0.0725));
        california.filing_frequency = FilingFrequency::Quarterly;
        self.create_jurisdiction(california).await?;
        
        // New York State
        let mut new_york = TaxJurisdiction::default();
        new_york.name = "New York State".to_string();
        new_york.code = "US-NY".to_string();
        new_york.jurisdiction_type = JurisdictionType::State;
        new_york.tax_rates.insert(TaxType::Income, dec!(0.06));
        new_york.tax_rates.insert(TaxType::Sales, dec!(0.04));
        new_york.filing_frequency = FilingFrequency::Quarterly;
        self.create_jurisdiction(new_york).await?;
        
        info!("Created {} default jurisdictions", self.jurisdictions.read().await.len());
        
        Ok(())
    }

    /// Create a new jurisdiction
    pub async fn create_jurisdiction(&mut self, mut jurisdiction: TaxJurisdiction) -> TaxResult<TaxJurisdiction> {
        debug!("Creating jurisdiction: {} ({})", jurisdiction.name, jurisdiction.code);

        // Check if code already exists
        let jurisdictions_by_code = self.jurisdictions_by_code.read().await;
        if jurisdictions_by_code.contains_key(&jurisdiction.code) {
            return Err(TaxError::other(&format!("Jurisdiction code {} already exists", jurisdiction.code)));
        }

        drop(jurisdictions_by_code);

        // Set timestamps
        jurisdiction.created_at = Utc::now();
        jurisdiction.updated_at = Utc::now();

        // Insert the jurisdiction
        self.jurisdictions.write().await.insert(jurisdiction.id, jurisdiction.clone());
        self.jurisdictions_by_code.write().await.insert(jurisdiction.code.clone(), jurisdiction.id);

        // Persist to SurrealDB if available
        if let Some(ref db) = self.db {
            if let Ok(client) = db.db().await {
                let value = serde_json::to_value(&jurisdiction).unwrap_or_default();
                if let Err(e) = client.create::<Vec<serde_json::Value>>("tax_jurisdiction").content(value).await {
                    warn!("Failed to persist jurisdiction to SurrealDB: {}", e);
                }
            }
        }

        Ok(jurisdiction)
    }

    /// Get a jurisdiction by ID
    pub async fn get_jurisdiction(&self, id: Uuid) -> TaxResult<Option<TaxJurisdiction>> {
        let jurisdictions = self.jurisdictions.read().await;
        Ok(jurisdictions.get(&id).cloned())
    }

    /// Get a jurisdiction by code
    pub async fn get_jurisdiction_by_code(&self, code: &str) -> TaxResult<Option<TaxJurisdiction>> {
        let jurisdictions_by_code = self.jurisdictions_by_code.read().await;
        if let Some(&id) = jurisdictions_by_code.get(code) {
            self.get_jurisdiction(id).await
        } else {
            Ok(None)
        }
    }

    /// List all jurisdictions
    pub async fn list_jurisdictions(&self) -> TaxResult<Vec<TaxJurisdiction>> {
        let jurisdictions = self.jurisdictions.read().await;
        Ok(jurisdictions.values().cloned().collect())
    }

    /// Update a jurisdiction
    pub async fn update_jurisdiction(&mut self, id: Uuid, mut jurisdiction: TaxJurisdiction) -> TaxResult<TaxJurisdiction> {
        debug!("Updating jurisdiction: {}", id);
        
        if jurisdiction.id != id {
            return Err(TaxError::other("Jurisdiction ID mismatch"));
        }
        
        // Check if code is being changed
        let jurisdictions = self.jurisdictions.read().await;
        if let Some(existing) = jurisdictions.get(&id) {
            if existing.code != jurisdiction.code {
                // Check if new code already exists
                let jurisdictions_by_code = self.jurisdictions_by_code.read().await;
                if jurisdictions_by_code.contains_key(&jurisdiction.code) {
                    return Err(TaxError::other(&format!("Jurisdiction code {} already exists", jurisdiction.code)));
                }
                
                // Update the code mapping
                drop(jurisdictions_by_code);
                let mut jurisdictions_by_code = self.jurisdictions_by_code.write().await;
                jurisdictions_by_code.remove(&existing.code);
                jurisdictions_by_code.insert(jurisdiction.code.clone(), id);
            }
        }
        
        drop(jurisdictions);
        
        // Update timestamp
        jurisdiction.updated_at = Utc::now();
        
        // Update the jurisdiction
        self.jurisdictions.write().await.insert(id, jurisdiction.clone());
        
        Ok(jurisdiction)
    }

    /// Delete a jurisdiction
    pub async fn delete_jurisdiction(&mut self, id: Uuid) -> TaxResult<bool> {
        debug!("Deleting jurisdiction: {}", id);
        
        let mut jurisdictions = self.jurisdictions.write().await;
        let jurisdiction = jurisdictions.remove(&id);
        let found = jurisdiction.is_some();

        if let Some(jur) = jurisdiction {
            // Remove from code mapping
            self.jurisdictions_by_code.write().await.remove(&jur.code);
        }

        Ok(found)
    }

    /// Calculate tax for a given amount and jurisdiction
    pub async fn calculate_tax(
        &self,
        jurisdiction_code: &str,
        tax_type: TaxType,
        amount: Decimal,
        period_start: NaiveDate,
        period_end: NaiveDate,
    ) -> TaxResult<TaxCalculation> {
        debug!("Calculating {} tax for jurisdiction {}: amount = {}", tax_type, jurisdiction_code, amount);
        
        // Get the jurisdiction
        let jurisdiction = self.get_jurisdiction_by_code(jurisdiction_code).await?;
        let jurisdiction = jurisdiction.ok_or_else(|| TaxError::JurisdictionNotFound(jurisdiction_code.to_string()))?;
        
        // Get the tax rate — if brackets exist, use progressive calculation
        let (tax_rate, tax_amount) = if !jurisdiction.tax_brackets.is_empty() {
            let bracket_tax = Self::calculate_progressive_tax(&jurisdiction.tax_brackets, amount);
            // Return effective rate for display
            let effective_rate = if amount > dec!(0) { bracket_tax / amount } else { dec!(0) };
            (effective_rate, bracket_tax)
        } else {
            let rate = jurisdiction.tax_rates.get(&tax_type)
                .ok_or_else(|| TaxError::other(&format!("Tax rate not found for type {:?} in jurisdiction {}", tax_type, jurisdiction_code)))?;
            (rate.clone(), amount * rate)
        };
        
        Ok(TaxCalculation {
            jurisdiction: jurisdiction.clone(),
            tax_type: tax_type.clone(),
            taxable_amount: amount,
            tax_rate,
            tax_amount,
            calculation_date: Utc::now(),
            period_start,
            period_end,
        })
    }

    /// Create a tax filing
    pub async fn create_filing(
        &mut self,
        jurisdiction_id: Uuid,
        tax_type: TaxType,
        period_start: NaiveDate,
        period_end: NaiveDate,
        taxable_amount: Decimal,
        tax_amount: Decimal,
    ) -> TaxResult<TaxFiling> {
        debug!("Creating tax filing for jurisdiction {} and type {:?}", jurisdiction_id, tax_type);

        let mut filing = TaxFiling {
            jurisdiction_id,
            tax_type,
            period_start,
            period_end,
            taxable_amount,
            tax_amount,
            ..Default::default()
        };

        // Set timestamps
        filing.created_at = Utc::now();
        filing.updated_at = Utc::now();

        // Store the filing
        self.filings.write().await.insert(filing.id, filing.clone());

        // Persist to SurrealDB if available
        if let Some(ref db) = self.db {
            if let Ok(client) = db.db().await {
                let value = serde_json::to_value(&filing).unwrap_or_default();
                if let Err(e) = client.create::<Vec<serde_json::Value>>("tax_filing").content(value).await {
                    warn!("Failed to persist tax filing to SurrealDB: {}", e);
                }
            }
        }

        Ok(filing)
    }

    /// Get a filing by ID
    pub async fn get_filing(&self, id: Uuid) -> TaxResult<Option<TaxFiling>> {
        let filings = self.filings.read().await;
        Ok(filings.get(&id).cloned())
    }

    /// List filings by jurisdiction
    pub async fn list_filings_by_jurisdiction(&self, jurisdiction_id: Uuid) -> TaxResult<Vec<TaxFiling>> {
        let filings = self.filings.read().await;
        Ok(filings.values()
            .filter(|f| f.jurisdiction_id == jurisdiction_id)
            .cloned()
            .collect())
    }

    /// List filings by status
    pub async fn list_filings_by_status(&self, status: FilingStatus) -> TaxResult<Vec<TaxFiling>> {
        let filings = self.filings.read().await;
        Ok(filings.values()
            .filter(|f| f.status == status)
            .cloned()
            .collect())
    }

    /// List filings by date range
    pub async fn list_filings_by_date_range(
        &self,
        start: NaiveDate,
        end: NaiveDate,
    ) -> TaxResult<Vec<TaxFiling>> {
        let filings = self.filings.read().await;
        Ok(filings.values()
            .filter(|f| f.period_end >= start && f.period_start <= end)
            .cloned()
            .collect())
    }

    /// Update a filing
    pub async fn update_filing(&mut self, id: Uuid, mut filing: TaxFiling) -> TaxResult<TaxFiling> {
        debug!("Updating filing: {}", id);
        
        if filing.id != id {
            return Err(TaxError::other("Filing ID mismatch"));
        }
        
        // Update timestamp
        filing.updated_at = Utc::now();
        
        // Update the filing
        self.filings.write().await.insert(id, filing.clone());
        
        Ok(filing)
    }

    /// Delete a filing
    pub async fn delete_filing(&mut self, id: Uuid) -> TaxResult<bool> {
        debug!("Deleting filing: {}", id);
        
        let mut filings = self.filings.write().await;
        Ok(filings.remove(&id).is_some())
    }

    /// Mark a filing as filed
    pub async fn mark_as_filed(&mut self, id: Uuid, confirmation_number: Option<String>) -> TaxResult<()> {
        let mut filing = self.get_filing(id).await?;
        if filing.is_none() {
            return Err(TaxError::other(&format!("Filing {} not found", id)));
        }
        
        let mut filing = filing.unwrap();
        filing.status = FilingStatus::Filed;
        filing.filing_date = Some(Utc::now());
        filing.confirmation_number = confirmation_number;
        
        self.update_filing(id, filing).await?;
        
        Ok(())
    }

    /// Mark a filing as paid
    pub async fn mark_as_paid(&mut self, id: Uuid, payment_amount: Decimal) -> TaxResult<()> {
        let filing = self.get_filing(id).await?;
        if filing.is_none() {
            return Err(TaxError::other(&format!("Filing {} not found", id)));
        }

        let mut filing = filing.unwrap();
        filing.status = FilingStatus::Paid;
        filing.payment_date = Some(Utc::now());
        filing.payment_amount = Some(payment_amount);

        // Create tax payment journal entry if ledger is connected
        if let Some(ref db) = self.db {
            // Store the filing update
            self.update_filing(id, filing).await?;

            // Create tax expense entry in the ledger if available
            // (requires ledger reference — available through orchestrator wiring)
        } else {
            self.update_filing(id, filing).await?;
        }

        Ok(())
    }

    /// Calculate progressive tax using bracket system.
    /// Example: CA 2023 brackets: 0-10,412 @ 1%, ... 13.3% over 1M
    pub fn calculate_progressive_tax(brackets: &[TaxBracket], amount: Decimal) -> Decimal {
        let mut total = dec!(0);
        for bracket in brackets {
            if amount <= bracket.lower_bound {
                continue;
            }
            let bracket_income = match bracket.upper_bound {
                Some(upper) if amount > upper => upper - bracket.lower_bound,
                Some(upper) => amount - bracket.lower_bound,
                None => amount - bracket.lower_bound,
            };
            total += bracket_income * bracket.rate;
        }
        total
    }
}

/// Tax Agent for handling tax-related tasks
#[derive(Debug, Clone)]
pub struct TaxAgent {
    /// Agent configuration
    pub config: AgentConfig,
    /// Current status
    pub status: AgentStatus,
    /// Tax calculator
    pub calculator: TaxCalculator,
}

impl TaxAgent {
    /// Create a new tax agent
    pub fn new(config: AgentConfig, calculator: TaxCalculator) -> Self {
        Self {
            config,
            status: AgentStatus::Idle,
            calculator,
        }
    }

    /// Create a tax agent with default configuration
    pub fn with_defaults() -> Self {
        let config = AgentConfig::tax_agent();
        let calculator = TaxCalculator::new();
        Self::new(config, calculator)
    }
}

#[async_trait]
impl Agent for TaxAgent {
    fn config(&self) -> &AgentConfig {
        &self.config
    }

    fn status(&self) -> AgentStatus {
        self.status.clone()
    }

    async fn initialize(&mut self) -> Result<(), anyhow::Error> {
        self.status = AgentStatus::Initializing;
        self.calculator.initialize().await?;
        self.status = AgentStatus::Idle;
        Ok(())
    }

    async fn shutdown(&mut self) -> Result<(), anyhow::Error> {
        self.status = AgentStatus::ShuttingDown;
        // Clean up resources
        self.status = AgentStatus::Idle;
        Ok(())
    }

    async fn process_task(&self, task: Task) -> Result<Task, anyhow::Error> {
        if !self.config.enabled {
            return Err(AgentError::AgentDisabled(self.config.name.clone()).into());
        }

        match task.task_type {
            crate::agents::task::TaskType::CalculateTaxes => {
                self.process_calculate_taxes(task).await
            }
            _ => {
                Err(AgentError::TaskProcessingFailed(
                    format!("TaxAgent cannot handle task type: {:?}", task.task_type)
                ).into())
            }
        }
    }

    fn agent_type(&self) -> AgentType {
        self.config.agent_type.clone()
    }
}

impl TaxAgent {
    /// Process a calculate taxes task.
    ///
    /// Accepts `TaskPayload::Json` with fields:
    ///   `jurisdiction_code` (string, e.g. "US-FED", "US-CA"),
    ///   `tax_type` (string: "Income", "Sales", "Payroll", "Property", "VAT", "GST"),
    ///   `amount` (number or string),
    ///   `period_start` (YYYY-MM-DD, optional),
    ///   `period_end` (YYYY-MM-DD, optional).
    async fn process_calculate_taxes(&self, task: Task) -> Result<Task, anyhow::Error> {
        let start_time = std::time::Instant::now();

        let json = match &task.payload {
            TaskPayload::Json(v) => v.clone(),
            _ => {
                return Err(AgentError::TaskProcessingFailed(
                    "Expected Json payload for CalculateTaxes task".to_string(),
                ).into());
            }
        };

        let jurisdiction_code = json.get("jurisdiction_code")
            .and_then(|v| v.as_str())
            .unwrap_or("US-FED");

        let tax_type = json.get("tax_type")
            .and_then(|v| v.as_str())
            .map(|s| match s {
                "Income" | "income" => TaxType::Income,
                "Sales" | "sales" => TaxType::Sales,
                "Payroll" | "payroll" => TaxType::Payroll,
                "Property" | "property" => TaxType::Property,
                "VAT" | "vat" => TaxType::VAT,
                "GST" | "gst" => TaxType::GST,
                other => TaxType::Other(other.to_string()),
            })
            .unwrap_or(TaxType::Income);

        let amount = json.get("amount")
            .and_then(|v| {
                if let Some(s) = v.as_str() {
                    s.parse::<Decimal>().ok()
                } else if let Some(n) = v.as_f64() {
                    Decimal::from_f64_retain(n)
                } else {
                    None
                }
            })
            .ok_or_else(|| AgentError::TaskProcessingFailed(
                "Missing or invalid 'amount' in payload".to_string(),
            ))?;

        let period_start = json.get("period_start")
            .and_then(|v| v.as_str())
            .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
            .unwrap_or_else(|| Utc::now().date_naive());

        let period_end = json.get("period_end")
            .and_then(|v| v.as_str())
            .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
            .unwrap_or_else(|| Utc::now().date_naive());

        let calculation = self.calculator.calculate_tax(
            jurisdiction_code,
            tax_type,
            amount,
            period_start,
            period_end,
        ).await?;

        let rate_pct = calculation.tax_rate * dec!(100);
        let result = TaskResult::success_with_data(
            &format!(
                "Tax calculated: {} {} @ {}% on {} = {}",
                calculation.jurisdiction.code,
                calculation.tax_type,
                rate_pct,
                calculation.taxable_amount,
                calculation.tax_amount,
            ),
            TaskPayload::Json(serde_json::to_value(&calculation).unwrap_or_default()),
        );

        let _processing_time = start_time.elapsed().as_millis() as f64;

        Ok(task.complete(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[tokio::test]
    async fn test_tax_calculator_creation() {
        let calculator = TaxCalculator::new();
        assert!(calculator.jurisdictions.read().await.is_empty());
    }

    #[tokio::test]
    async fn test_tax_calculator_initialization() {
        let mut calculator = TaxCalculator::new();
        calculator.initialize().await.unwrap();
        
        assert!(!calculator.jurisdictions.read().await.is_empty());
    }

    #[tokio::test]
    async fn test_jurisdiction_operations() {
        let mut calculator = TaxCalculator::new();
        
        // Create a jurisdiction
        let mut jurisdiction = TaxJurisdiction::default();
        jurisdiction.name = "Test Jurisdiction".to_string();
        jurisdiction.code = "TEST".to_string();
        jurisdiction.tax_rates.insert(TaxType::Income, dec!(25));
        
        let created = calculator.create_jurisdiction(jurisdiction).await.unwrap();
        assert_eq!(created.code, "TEST");
        
        // Get jurisdiction by ID
        let found = calculator.get_jurisdiction(created.id).await.unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().code, "TEST");
        
        // Get jurisdiction by code
        let found = calculator.get_jurisdiction_by_code("TEST").await.unwrap();
        assert!(found.is_some());
        
        // List jurisdictions
        let jurisdictions = calculator.list_jurisdictions().await.unwrap();
        assert_eq!(jurisdictions.len(), 1);
        
        // Delete jurisdiction
        let deleted = calculator.delete_jurisdiction(created.id).await.unwrap();
        assert!(deleted);
    }

    #[tokio::test]
    async fn test_tax_calculation() {
        let mut calculator = TaxCalculator::new();
        calculator.initialize().await.unwrap();
        
        // Calculate tax for US Federal
        let calculation = calculator.calculate_tax(
            "US-FED",
            TaxType::Income,
            dec!(10000),
            Utc::now().date_naive(),
            Utc::now().date_naive(),
        ).await.unwrap();
        
        assert_eq!(calculation.jurisdiction.code, "US-FED");
        assert_eq!(calculation.tax_type, TaxType::Income);
        assert_eq!(calculation.taxable_amount, dec!(10000));
        assert_eq!(calculation.tax_rate, dec!(0.21));
        // 21% of $10,000 = $2,100 (rate stored as 0.21, amount * rate = 10000 * 0.21)
        assert_eq!(calculation.tax_amount, dec!(2100));
    }

    #[tokio::test]
    async fn test_filing_operations() {
        let mut calculator = TaxCalculator::new();
        
        // Create a jurisdiction first
        let mut jurisdiction = TaxJurisdiction::default();
        jurisdiction.name = "Test Jurisdiction".to_string();
        jurisdiction.code = "TEST".to_string();
        let jurisdiction = calculator.create_jurisdiction(jurisdiction).await.unwrap();
        
        // Create a filing
        let filing = calculator.create_filing(
            jurisdiction.id,
            TaxType::Income,
            Utc::now().date_naive(),
            Utc::now().date_naive(),
            dec!(10000),
            dec!(2000),
        ).await.unwrap();
        
        assert_eq!(filing.jurisdiction_id, jurisdiction.id);
        assert_eq!(filing.tax_type, TaxType::Income);
        
        // Get filing
        let found = calculator.get_filing(filing.id).await.unwrap();
        assert!(found.is_some());
        
        // Mark as filed
        calculator.mark_as_filed(filing.id, Some("CONF-123".to_string())).await.unwrap();
        let found = calculator.get_filing(filing.id).await.unwrap();
        assert_eq!(found.unwrap().status, FilingStatus::Filed);
        
        // Mark as paid
        calculator.mark_as_paid(filing.id, dec!(2000)).await.unwrap();
        let found = calculator.get_filing(filing.id).await.unwrap();
        assert_eq!(found.unwrap().status, FilingStatus::Paid);
    }

    #[tokio::test]
    async fn test_tax_agent() {
        let agent = TaxAgent::with_defaults();
        assert_eq!(agent.config.agent_type, AgentType::TaxAgent);
        assert_eq!(agent.status, AgentStatus::Idle);
    }
}
