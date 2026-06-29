//! Payroll Module
//!
//! Handles payroll calculations, processing, and reporting.

use async_trait::async_trait;
use serde::{Serialize, Deserialize};
use std::collections::{HashMap, BTreeMap};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use uuid::Uuid;
use chrono::{DateTime, Utc, NaiveDate, Datelike};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use tracing::{info, error, debug, warn};
use crate::database::financial::{Account, AccountType, EntryType, Transaction, TransactionEntry, TransactionType, TransactionStatus};
use crate::database::error::DatabaseError;
use crate::agents::agent_types::{Agent, AgentConfig, AgentType, AgentStatus};
use crate::agents::task::{Task, TaskResult, TaskPayload};
use crate::agents::error::AgentError;

/// Payroll error types
#[derive(Debug, thiserror::Error)]
pub enum PayrollError {
    /// Employee not found
    #[error("Employee not found: {0}")]
    EmployeeNotFound(String),
    
    /// Invalid pay period
    #[error("Invalid pay period: {0}")]
    InvalidPayPeriod(String),
    
    /// Payroll calculation error
    #[error("Payroll calculation error: {0}")]
    CalculationError(String),
    
    /// Payroll processing error
    #[error("Payroll processing error: {0}")]
    ProcessingError(String),
    
    /// Tax calculation error
    #[error("Tax calculation error: {0}")]
    TaxCalculationError(String),
    
    /// Database error
    #[error("Database error: {0}")]
    DatabaseError(#[from] DatabaseError),
    
    /// Any other payroll error
    #[error("Payroll error: {0}")]
    Other(String),
}

impl PayrollError {
    /// Create a new other error
    pub fn other(msg: &str) -> Self {
        Self::Other(msg.to_string())
    }
}

/// Result type for payroll operations
pub type PayrollResult<T> = Result<T, PayrollError>;

/// Employee model for payroll
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Employee {
    /// Unique identifier
    pub id: Uuid,
    /// Employee number
    pub number: String,
    /// First name
    pub first_name: String,
    /// Last name
    pub last_name: String,
    /// Email
    pub email: String,
    /// Hire date
    pub hire_date: NaiveDate,
    /// Termination date (if applicable)
    pub termination_date: Option<NaiveDate>,
    /// Employment status
    pub status: EmploymentStatus,
    /// Employment type
    pub employment_type: EmploymentType,
    /// Pay rate
    pub pay_rate: Decimal,
    /// Pay frequency
    pub pay_frequency: PayFrequency,
    /// Currency
    pub currency: String,
    /// Tax information
    pub tax_info: TaxInformation,
    /// Direct deposit information
    pub direct_deposit: Option<DirectDeposit>,
    /// Created timestamp
    pub created_at: DateTime<Utc>,
    /// Updated timestamp
    pub updated_at: DateTime<Utc>,
}

impl Default for Employee {
    fn default() -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            number: String::new(),
            first_name: String::new(),
            last_name: String::new(),
            email: String::new(),
            hire_date: now.date_naive(),
            termination_date: None,
            status: EmploymentStatus::default(),
            employment_type: EmploymentType::default(),
            pay_rate: dec!(0),
            pay_frequency: PayFrequency::default(),
            currency: "USD".to_string(),
            tax_info: TaxInformation::default(),
            direct_deposit: None,
            created_at: now,
            updated_at: now,
        }
    }
}

/// Employment status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum EmploymentStatus {
    /// Active employee
    Active,
    /// On leave
    OnLeave,
    /// Terminated
    Terminated,
    /// Retired
    Retired,
}

impl Default for EmploymentStatus {
    fn default() -> Self {
        Self::Active
    }
}

/// Employment type
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum EmploymentType {
    /// Full-time employee
    FullTime,
    /// Part-time employee
    PartTime,
    /// Contractor
    Contractor,
    /// Intern
    Intern,
    /// Temporary
    Temporary,
}

impl Default for EmploymentType {
    fn default() -> Self {
        Self::FullTime
    }
}

/// Pay frequency
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum PayFrequency {
    /// Weekly
    Weekly,
    /// Bi-weekly
    BiWeekly,
    /// Semi-monthly
    SemiMonthly,
    /// Monthly
    Monthly,
    /// Annual
    Annual,
}

impl Default for PayFrequency {
    fn default() -> Self {
        Self::BiWeekly
    }
}

/// Tax information for an employee
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TaxInformation {
    /// Social Security Number (or equivalent)
    pub ssn: String,
    /// Federal tax filing status
    pub federal_filing_status: FilingStatus,
    /// Number of federal allowances
    pub federal_allowances: u8,
    /// State tax filing status
    pub state_filing_status: FilingStatus,
    /// Number of state allowances
    pub state_allowances: u8,
    /// Local tax filing status
    pub local_filing_status: FilingStatus,
    /// Number of local allowances
    pub local_allowances: u8,
    /// Exemptions
    pub exemptions: Vec<String>,
}

/// Filing status for tax purposes
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum FilingStatus {
    /// Single
    Single,
    /// Married Filing Jointly
    MarriedJointly,
    /// Married Filing Separately
    MarriedSeparately,
    /// Head of Household
    HeadOfHousehold,
    /// Qualifying Widow(er)
    QualifyingWidow,
}

impl Default for FilingStatus {
    fn default() -> Self {
        Self::Single
    }
}

/// Direct deposit information
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DirectDeposit {
    /// Bank name
    pub bank_name: String,
    /// Account number
    pub account_number: String,
    /// Routing number
    pub routing_number: String,
    /// Account type
    pub account_type: String,
}

/// Pay period
#[derive(Debug, Clone)]
pub struct PayPeriod {
    /// Unique identifier
    pub id: Uuid,
    /// Period start date
    pub start_date: NaiveDate,
    /// Period end date
    pub end_date: NaiveDate,
    /// Pay date
    pub pay_date: NaiveDate,
    /// Period number
    pub period_number: u32,
    /// Year
    pub year: i32,
    /// Status
    pub status: PayPeriodStatus,
}

impl Default for PayPeriod {
    fn default() -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            start_date: now.date_naive(),
            end_date: now.date_naive(),
            pay_date: now.date_naive(),
            period_number: 1,
            year: now.year(),
            status: PayPeriodStatus::default(),
        }
    }
}

/// Pay period status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum PayPeriodStatus {
    /// Open
    Open,
    /// Closed
    Closed,
    /// Processed
    Processed,
    /// Paid
    Paid,
    /// Cancelled
    Cancelled,
}

impl Default for PayPeriodStatus {
    fn default() -> Self {
        Self::Open
    }
}

/// Time entry for payroll
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeEntry {
    /// Unique identifier
    pub id: Uuid,
    /// Employee ID
    pub employee_id: Uuid,
    /// Date
    pub date: NaiveDate,
    /// Hours worked
    pub hours: Decimal,
    /// Regular hours
    pub regular_hours: Decimal,
    /// Overtime hours
    pub overtime_hours: Decimal,
    /// Pay rate override (if applicable)
    pub pay_rate_override: Option<Decimal>,
    /// Notes
    pub notes: String,
}

impl Default for TimeEntry {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4(),
            employee_id: Uuid::new_v4(),
            date: Utc::now().date_naive(),
            hours: dec!(0),
            regular_hours: dec!(0),
            overtime_hours: dec!(0),
            pay_rate_override: None,
            notes: String::new(),
        }
    }
}

/// Payroll calculation result
#[derive(Debug, Clone)]
pub struct PayrollCalculation {
    /// Employee ID
    pub employee_id: Uuid,
    /// Pay period ID
    pub pay_period_id: Uuid,
    /// Regular pay
    pub regular_pay: Decimal,
    /// Overtime pay
    pub overtime_pay: Decimal,
    /// Gross pay
    pub gross_pay: Decimal,
    /// Federal tax withholding
    pub federal_tax: Decimal,
    /// State tax withholding
    pub state_tax: Decimal,
    /// Local tax withholding
    pub local_tax: Decimal,
    /// Social security withholding
    pub social_security: Decimal,
    /// Medicare withholding
    pub medicare: Decimal,
    /// Retirement contribution
    pub retirement: Decimal,
    /// Other deductions
    pub other_deductions: Decimal,
    /// Total deductions
    pub total_deductions: Decimal,
    /// Net pay
    pub net_pay: Decimal,
    /// Employer contributions
    pub employer_contributions: Decimal,
    /// Total employer cost
    pub total_employer_cost: Decimal,
}

/// Payroll processor
#[derive(Debug, Clone)]
pub struct PayrollProcessor {
    /// Map of employee ID to employee
    pub employees: Arc<RwLock<BTreeMap<Uuid, Employee>>>,
    /// Map of pay period ID to pay period
    pub pay_periods: Arc<RwLock<BTreeMap<Uuid, PayPeriod>>>,
    /// Map of time entry ID to time entry
    pub time_entries: Arc<RwLock<BTreeMap<Uuid, TimeEntry>>>,
    /// Current pay period
    pub current_pay_period: Arc<Mutex<Option<PayPeriod>>>,
    /// Optional SurrealDB connection for persistence
    pub db: Option<Arc<crate::database::Database>>,
}

impl Default for PayrollProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl PayrollProcessor {
    /// Create a new payroll processor
    pub fn new() -> Self {
        Self {
            employees: Arc::new(RwLock::new(BTreeMap::new())),
            pay_periods: Arc::new(RwLock::new(BTreeMap::new())),
            time_entries: Arc::new(RwLock::new(BTreeMap::new())),
            current_pay_period: Arc::new(Mutex::new(None)),
            db: None,
        }
    }

    /// Initialize the payroll processor
    pub async fn initialize(&mut self) -> PayrollResult<()> {
        info!("Initializing Payroll Processor...");
        
        // Create current pay period
        self.create_current_pay_period().await?;
        
        Ok(())
    }

    /// Create the current pay period
    async fn create_current_pay_period(&mut self) -> PayrollResult<()> {
        let now = Utc::now();
        let today = now.date_naive();
        
        // For simplicity, we'll create a bi-weekly pay period
        // In a real implementation, this would be more sophisticated
        let start_date = today - chrono::Duration::days(14);
        let end_date = today;
        let pay_date = today + chrono::Duration::days(5);
        
        let pay_period = PayPeriod {
            id: Uuid::new_v4(),
            start_date,
            end_date,
            pay_date,
            period_number: 1,
            year: today.year(),
            status: PayPeriodStatus::Open,
        };
        
        self.pay_periods.write().await.insert(pay_period.id, pay_period.clone());
        *self.current_pay_period.lock().await = Some(pay_period);
        
        Ok(())
    }

    /// Create a new employee
    pub async fn create_employee(&mut self, mut employee: Employee) -> PayrollResult<Employee> {
        debug!("Creating employee: {} {}", employee.first_name, employee.last_name);

        // Check if employee number already exists
        let employees = self.employees.read().await;
        if employees.values().any(|e| e.number == employee.number) {
            return Err(PayrollError::other(&format!("Employee number {} already exists", employee.number)));
        }

        drop(employees);

        // Set timestamps
        employee.created_at = Utc::now();
        employee.updated_at = Utc::now();

        // Insert the employee
        self.employees.write().await.insert(employee.id, employee.clone());

        // Persist to SurrealDB if available
        if let Some(ref db) = self.db {
            if let Ok(client) = db.db().await {
                let value = serde_json::to_value(&employee).unwrap_or_default();
                if let Err(e) = client.create::<Vec<serde_json::Value>>("employee").content(value).await {
                    warn!("Failed to persist employee to SurrealDB: {}", e);
                }
            }
        }

        Ok(employee)
    }

    /// Get an employee by ID
    pub async fn get_employee(&self, id: Uuid) -> PayrollResult<Option<Employee>> {
        let employees = self.employees.read().await;
        Ok(employees.get(&id).cloned())
    }

    /// Get an employee by number
    pub async fn get_employee_by_number(&self, number: &str) -> PayrollResult<Option<Employee>> {
        let employees = self.employees.read().await;
        Ok(employees.values().find(|e| e.number == number).cloned())
    }

    /// List all employees
    pub async fn list_employees(&self) -> PayrollResult<Vec<Employee>> {
        let employees = self.employees.read().await;
        Ok(employees.values().cloned().collect())
    }

    /// Update an employee
    pub async fn update_employee(&mut self, id: Uuid, mut employee: Employee) -> PayrollResult<Employee> {
        debug!("Updating employee: {}", id);
        
        if employee.id != id {
            return Err(PayrollError::other("Employee ID mismatch"));
        }
        
        // Check if employee exists
        let mut employees = self.employees.write().await;
        if !employees.contains_key(&id) {
            return Err(PayrollError::EmployeeNotFound(id.to_string()));
        }
        
        // Update timestamp
        employee.updated_at = Utc::now();
        
        // Update the employee
        employees.insert(id, employee.clone());
        
        Ok(employee)
    }

    /// Delete an employee
    pub async fn delete_employee(&mut self, id: Uuid) -> PayrollResult<bool> {
        debug!("Deleting employee: {}", id);
        
        let mut employees = self.employees.write().await;
        Ok(employees.remove(&id).is_some())
    }

    /// Record time for an employee
    pub async fn record_time(&mut self, time_entry: TimeEntry) -> PayrollResult<TimeEntry> {
        debug!("Recording time for employee {}", time_entry.employee_id);

        // Validate employee exists
        let employees = self.employees.read().await;
        if !employees.contains_key(&time_entry.employee_id) {
            return Err(PayrollError::EmployeeNotFound(time_entry.employee_id.to_string()));
        }

        drop(employees);

        // Insert the time entry
        self.time_entries.write().await.insert(time_entry.id, time_entry.clone());

        // Persist to SurrealDB if available
        if let Some(ref db) = self.db {
            if let Ok(client) = db.db().await {
                let value = serde_json::to_value(&time_entry).unwrap_or_default();
                if let Err(e) = client.create::<Vec<serde_json::Value>>("time_entry").content(value).await {
                    warn!("Failed to persist time entry to SurrealDB: {}", e);
                }
            }
        }

        Ok(time_entry)
    }

    /// Get time entries for an employee and date range
    pub async fn get_time_entries(
        &self,
        employee_id: Uuid,
        start_date: NaiveDate,
        end_date: NaiveDate,
    ) -> PayrollResult<Vec<TimeEntry>> {
        let time_entries = self.time_entries.read().await;
        Ok(time_entries.values()
            .filter(|e| e.employee_id == employee_id && e.date >= start_date && e.date <= end_date)
            .cloned()
            .collect())
    }

    /// Calculate payroll for an employee
    pub async fn calculate_payroll(
        &self,
        employee_id: Uuid,
        pay_period_id: Uuid,
    ) -> PayrollResult<PayrollCalculation> {
        debug!("Calculating payroll for employee {} and pay period {}", employee_id, pay_period_id);
        
        // Get employee
        let employee = self.get_employee(employee_id).await?;
        let employee = employee.ok_or_else(|| PayrollError::EmployeeNotFound(employee_id.to_string()))?;
        
        // Get pay period
        let pay_periods = self.pay_periods.read().await;
        let pay_period = pay_periods.get(&pay_period_id)
            .ok_or_else(|| PayrollError::InvalidPayPeriod(pay_period_id.to_string()))?;
        
        // Get time entries for the pay period
        let time_entries = self.get_time_entries(employee_id, pay_period.start_date, pay_period.end_date).await?;
        
        // Calculate regular and overtime hours
        let mut regular_hours = dec!(0);
        let mut overtime_hours = dec!(0);
        
        for entry in &time_entries {
            regular_hours += entry.regular_hours;
            overtime_hours += entry.overtime_hours;
        }
        
        // Calculate pay
        let regular_pay = regular_hours * employee.pay_rate;
        let overtime_pay = overtime_hours * (employee.pay_rate * dec!(1.5));
        let gross_pay = regular_pay + overtime_pay;
        
        // Calculate taxes (simplified for example)
        let federal_tax = self.calculate_federal_tax(&employee, gross_pay).await?;
        let state_tax = self.calculate_state_tax(&employee, gross_pay).await?;
        let local_tax = self.calculate_local_tax(&employee, gross_pay).await?;
        let social_security = self.calculate_social_security(gross_pay).await?;
        let medicare = self.calculate_medicare(gross_pay).await?;
        
        // Calculate retirement (simplified)
        let retirement = gross_pay * dec!(0.05);
        
        // Total deductions
        let total_deductions = federal_tax + state_tax + local_tax + social_security + medicare + retirement;
        
        // Net pay
        let net_pay = gross_pay - total_deductions;
        
        // Employer contributions (simplified)
        let employer_social_security = social_security;
        let employer_medicare = medicare;
        let employer_retirement = retirement;
        let employer_contributions = employer_social_security + employer_medicare + employer_retirement;
        
        // Total employer cost
        let total_employer_cost = gross_pay + employer_contributions;
        
        Ok(PayrollCalculation {
            employee_id,
            pay_period_id,
            regular_pay,
            overtime_pay,
            gross_pay,
            federal_tax,
            state_tax,
            local_tax,
            social_security,
            medicare,
            retirement,
            other_deductions: dec!(0),
            total_deductions,
            net_pay,
            employer_contributions,
            total_employer_cost,
        })
    }

    /// Calculate federal tax (simplified)
    async fn calculate_federal_tax(&self, employee: &Employee, gross_pay: Decimal) -> PayrollResult<Decimal> {
        // Simplified federal tax calculation
        // In a real implementation, this would use tax tables and withholding formulas
        let rate = match employee.tax_info.federal_filing_status {
            FilingStatus::Single => dec!(0.15),
            FilingStatus::MarriedJointly => dec!(0.12),
            FilingStatus::MarriedSeparately => dec!(0.15),
            FilingStatus::HeadOfHousehold => dec!(0.13),
            FilingStatus::QualifyingWidow => dec!(0.12),
        };
        
        // Adjust for allowances
        let allowance_adjustment = dec!(100) * Decimal::from(employee.tax_info.federal_allowances as i64);
        let taxable_amount = gross_pay - allowance_adjustment;
        
        Ok(taxable_amount * rate)
    }

    /// Calculate state tax (simplified)
    async fn calculate_state_tax(&self, employee: &Employee, gross_pay: Decimal) -> PayrollResult<Decimal> {
        // Simplified state tax calculation
        let rate = match employee.tax_info.state_filing_status {
            FilingStatus::Single => dec!(0.05),
            FilingStatus::MarriedJointly => dec!(0.04),
            FilingStatus::MarriedSeparately => dec!(0.05),
            FilingStatus::HeadOfHousehold => dec!(0.045),
            FilingStatus::QualifyingWidow => dec!(0.04),
        };
        
        // Adjust for allowances
        let allowance_adjustment = dec!(50) * Decimal::from(employee.tax_info.state_allowances as i64);
        let taxable_amount = gross_pay - allowance_adjustment;
        
        Ok(taxable_amount * rate)
    }

    /// Calculate local tax (simplified)
    async fn calculate_local_tax(&self, employee: &Employee, gross_pay: Decimal) -> PayrollResult<Decimal> {
        // Simplified local tax calculation
        let rate = match employee.tax_info.local_filing_status {
            FilingStatus::Single => dec!(0.01),
            FilingStatus::MarriedJointly => dec!(0.008),
            FilingStatus::MarriedSeparately => dec!(0.01),
            FilingStatus::HeadOfHousehold => dec!(0.009),
            FilingStatus::QualifyingWidow => dec!(0.008),
        };
        
        Ok(gross_pay * rate)
    }

    /// Calculate social security tax
    async fn calculate_social_security(&self, gross_pay: Decimal) -> PayrollResult<Decimal> {
        // Social security rate is 6.2% up to the wage base limit
        const RATE: Decimal = dec!(0.062);
        const WAGE_BASE_LIMIT: Decimal = dec!(160200); // 2023 limit
        
        let taxable_amount = gross_pay.min(WAGE_BASE_LIMIT);
        Ok(taxable_amount * RATE)
    }

    /// Calculate medicare tax
    async fn calculate_medicare(&self, gross_pay: Decimal) -> PayrollResult<Decimal> {
        // Medicare rate is 1.45% with no wage base limit
        const RATE: Decimal = dec!(0.0145);
        
        // Additional Medicare tax of 0.9% for wages over $200,000
        const ADDITIONAL_RATE: Decimal = dec!(0.009);
        const ADDITIONAL_THRESHOLD: Decimal = dec!(200000);
        
        let mut tax = gross_pay * RATE;
        
        if gross_pay > ADDITIONAL_THRESHOLD {
            let additional_taxable = gross_pay - ADDITIONAL_THRESHOLD;
            tax += additional_taxable * ADDITIONAL_RATE;
        }
        
        Ok(tax)
    }

    /// Process payroll for a pay period
    pub async fn process_payroll(&self, pay_period_id: Uuid) -> PayrollResult<Vec<PayrollCalculation>> {
        info!("Processing payroll for pay period {}", pay_period_id);
        
        // Get all active employees
        let employees = self.list_employees().await?;
        let active_employees: Vec<Employee> = employees.into_iter()
            .filter(|e| e.status == EmploymentStatus::Active)
            .collect();
        
        let mut calculations = Vec::new();
        
        for employee in active_employees {
            let calculation = self.calculate_payroll(employee.id, pay_period_id).await?;
            calculations.push(calculation);
        }
        
        // Update pay period status
        let mut pay_periods = self.pay_periods.write().await;
        if let Some(pay_period) = pay_periods.get_mut(&pay_period_id) {
            pay_period.status = PayPeriodStatus::Processed;
        }
        
        Ok(calculations)
    }

    /// Generate payroll transactions
    pub async fn generate_payroll_transactions(
        &self,
        pay_period_id: Uuid,
        calculations: Vec<PayrollCalculation>,
    ) -> PayrollResult<Vec<Transaction>> {
        info!("Generating payroll transactions for pay period {}", pay_period_id);
        
        let mut transactions = Vec::new();
        
        for calculation in calculations {
            // Create a transaction for each payroll calculation
            let mut entries = Vec::new();
            
            // Debit: Salaries Expense (gross pay)
            entries.push(TransactionEntry::new(
                Uuid::new_v4(), // In real implementation, this would be the salaries expense account
                EntryType::Debit,
                calculation.gross_pay,
                &format!("Gross pay for employee {}", calculation.employee_id),
            ));
            
            // Credit: Federal Tax Payable
            entries.push(TransactionEntry::new(
                Uuid::new_v4(), // Federal tax payable account
                EntryType::Credit,
                calculation.federal_tax,
                &format!("Federal tax for employee {}", calculation.employee_id),
            ));
            
            // Credit: State Tax Payable
            entries.push(TransactionEntry::new(
                Uuid::new_v4(), // State tax payable account
                EntryType::Credit,
                calculation.state_tax,
                &format!("State tax for employee {}", calculation.employee_id),
            ));
            
            // Credit: Local Tax Payable
            entries.push(TransactionEntry::new(
                Uuid::new_v4(), // Local tax payable account
                EntryType::Credit,
                calculation.local_tax,
                &format!("Local tax for employee {}", calculation.employee_id),
            ));
            
            // Credit: Social Security Payable
            entries.push(TransactionEntry::new(
                Uuid::new_v4(), // Social security payable account
                EntryType::Credit,
                calculation.social_security,
                &format!("Social security for employee {}", calculation.employee_id),
            ));
            
            // Credit: Medicare Payable
            entries.push(TransactionEntry::new(
                Uuid::new_v4(), // Medicare payable account
                EntryType::Credit,
                calculation.medicare,
                &format!("Medicare for employee {}", calculation.employee_id),
            ));
            
            // Credit: Retirement Payable
            entries.push(TransactionEntry::new(
                Uuid::new_v4(), // Retirement payable account
                EntryType::Credit,
                calculation.retirement,
                &format!("Retirement for employee {}", calculation.employee_id),
            ));
            
            // Credit: Cash (net pay)
            entries.push(TransactionEntry::new(
                Uuid::new_v4(), // Cash account
                EntryType::Credit,
                calculation.net_pay,
                &format!("Net pay for employee {}", calculation.employee_id),
            ));
            
            // Create the transaction
            let transaction = Transaction::new(
                format!("Payroll for employee {} - Period {}", calculation.employee_id, pay_period_id),
                Utc::now(),
                entries,
            );
            
            transactions.push(transaction);
        }
        
        Ok(transactions)
    }
}

/// Payroll Agent for handling payroll-related tasks
#[derive(Debug, Clone)]
pub struct PayrollAgent {
    /// Agent configuration
    pub config: AgentConfig,
    /// Current status
    pub status: AgentStatus,
    /// Payroll processor
    pub processor: PayrollProcessor,
}

impl PayrollAgent {
    /// Create a new payroll agent
    pub fn new(config: AgentConfig, processor: PayrollProcessor) -> Self {
        Self {
            config,
            status: AgentStatus::Idle,
            processor,
        }
    }

    /// Create a payroll agent with default configuration
    pub fn with_defaults() -> Self {
        let config = AgentConfig::payroll_agent();
        let processor = PayrollProcessor::new();
        Self::new(config, processor)
    }
}

#[async_trait]
impl Agent for PayrollAgent {
    fn config(&self) -> &AgentConfig {
        &self.config
    }

    fn status(&self) -> AgentStatus {
        self.status.clone()
    }

    async fn initialize(&mut self) -> Result<(), anyhow::Error> {
        self.status = AgentStatus::Initializing;
        self.processor.initialize().await?;
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
            crate::agents::task::TaskType::CalculatePayroll => {
                self.process_calculate_payroll(task).await
            }
            _ => {
                Err(AgentError::TaskProcessingFailed(
                    format!("PayrollAgent cannot handle task type: {:?}", task.task_type)
                ).into())
            }
        }
    }

    fn agent_type(&self) -> AgentType {
        self.config.agent_type.clone()
    }
}

impl PayrollAgent {
    /// Process a calculate payroll task
    async fn process_calculate_payroll(&self, task: Task) -> Result<Task, anyhow::Error> {
        // Status tracking deferred - requires interior mutability

        let start_time = std::time::Instant::now();
        
        // In a real implementation, we would extract payroll calculation parameters from the task
        // For now, we'll perform a mock calculation
        
        // Get the current pay period
        let current_pay_period = self.processor.current_pay_period.lock().await;
        let pay_period_id = current_pay_period.as_ref().map(|p| p.id).unwrap_or(Uuid::new_v4());
        
        // Process payroll
        let calculations = self.processor.process_payroll(pay_period_id).await?;
        
        // Generate transactions
        let transactions = self.processor.generate_payroll_transactions(pay_period_id, calculations).await?;
        
        // Create success result
        let result = TaskResult::success_with_data(
            "Payroll calculated successfully",
            TaskPayload::Json(serde_json::to_value(transactions).unwrap())
        );
        
        let processing_time = start_time.elapsed().as_millis() as f64;

        // Status tracking deferred - requires interior mutability

        Ok(task.complete(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[tokio::test]
    async fn test_payroll_processor_creation() {
        let processor = PayrollProcessor::new();
        assert!(processor.employees.read().await.is_empty());
    }

    #[tokio::test]
    async fn test_payroll_processor_initialization() {
        let mut processor = PayrollProcessor::new();
        processor.initialize().await.unwrap();
        
        assert!(processor.current_pay_period.lock().await.is_some());
    }

    #[tokio::test]
    async fn test_employee_operations() {
        let mut processor = PayrollProcessor::new();
        
        // Create an employee
        let mut employee = Employee::default();
        employee.first_name = "John".to_string();
        employee.last_name = "Doe".to_string();
        employee.number = "EMP001".to_string();
        employee.pay_rate = dec!(25);
        
        let created = processor.create_employee(employee).await.unwrap();
        assert_eq!(created.number, "EMP001");
        
        // Get employee by ID
        let found = processor.get_employee(created.id).await.unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().number, "EMP001");
        
        // Get employee by number
        let found = processor.get_employee_by_number("EMP001").await.unwrap();
        assert!(found.is_some());
        
        // List employees
        let employees = processor.list_employees().await.unwrap();
        assert_eq!(employees.len(), 1);
        
        // Delete employee
        let deleted = processor.delete_employee(created.id).await.unwrap();
        assert!(deleted);
    }

    #[tokio::test]
    async fn test_time_entry_operations() {
        let mut processor = PayrollProcessor::new();
        
        // Create an employee first
        let mut employee = Employee::default();
        employee.first_name = "Jane".to_string();
        employee.last_name = "Smith".to_string();
        employee.number = "EMP002".to_string();
        let employee = processor.create_employee(employee).await.unwrap();
        
        // Record time
        let time_entry = TimeEntry {
            id: Uuid::new_v4(),
            employee_id: employee.id,
            date: Utc::now().date_naive(),
            hours: dec!(8),
            regular_hours: dec!(8),
            overtime_hours: dec!(0),
            pay_rate_override: None,
            notes: "Regular work day".to_string(),
        };
        
        let recorded = processor.record_time(time_entry).await.unwrap();
        assert_eq!(recorded.hours, dec!(8));
        
        // Get time entries
        let entries = processor.get_time_entries(
            employee.id,
            Utc::now().date_naive(),
            Utc::now().date_naive(),
        ).await.unwrap();
        assert_eq!(entries.len(), 1);
    }

    #[tokio::test]
    async fn test_payroll_calculation() {
        let mut processor = PayrollProcessor::new();
        processor.initialize().await.unwrap();
        
        // Create an employee
        let mut employee = Employee::default();
        employee.first_name = "John".to_string();
        employee.last_name = "Doe".to_string();
        employee.number = "EMP003".to_string();
        employee.pay_rate = dec!(25);
        employee.tax_info.federal_filing_status = FilingStatus::Single;
        employee.tax_info.federal_allowances = 1;
        employee.tax_info.state_filing_status = FilingStatus::Single;
        employee.tax_info.state_allowances = 1;
        employee.tax_info.local_filing_status = FilingStatus::Single;
        employee.tax_info.local_allowances = 1;
        let employee = processor.create_employee(employee).await.unwrap();
        
        // Get current pay period
        let pay_period_id = {
            let current_pay_period = processor.current_pay_period.lock().await;
            current_pay_period.as_ref().map(|p| p.id).unwrap()
        };
        
        // Record time
        let time_entry = TimeEntry {
            id: Uuid::new_v4(),
            employee_id: employee.id,
            date: Utc::now().date_naive(),
            hours: dec!(40),
            regular_hours: dec!(40),
            overtime_hours: dec!(0),
            pay_rate_override: None,
            notes: "Regular work week".to_string(),
        };
        processor.record_time(time_entry).await.unwrap();
        
        // Calculate payroll
        let calculation = processor.calculate_payroll(employee.id, pay_period_id).await.unwrap();
        
        assert_eq!(calculation.regular_pay, dec!(1000)); // 40 hours * $25/hour
        assert_eq!(calculation.overtime_pay, dec!(0));
        assert!(calculation.gross_pay > dec!(0));
        assert!(calculation.net_pay < calculation.gross_pay);
    }

    #[tokio::test]
    async fn test_payroll_agent() {
        let agent = PayrollAgent::with_defaults();
        assert_eq!(agent.config.agent_type, AgentType::PayrollAgent);
        assert_eq!(agent.status, AgentStatus::Idle);
    }
}
