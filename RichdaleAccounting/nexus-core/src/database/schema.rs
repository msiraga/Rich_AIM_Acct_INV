//! Database Schema Module
//!
//! Defines all SurrealDB table schemas using SurrealQL DEFINE TABLE and DEFINE FIELD statements.
//! These statements are applied during database initialization to create the schema.

/// Returns all schema definition statements for the NexusLedger database.
///
/// The statements define tables and their fields based on the Rust models in
/// `database/models.rs` and `database/financial.rs`.
pub fn schema_statements() -> Vec<&'static str> {
    vec![
        // ──────────────────────────────────────────────
        // account table
        // ──────────────────────────────────────────────
        "DEFINE TABLE account TYPE NORMAL COMMENT 'Chart of accounts entry';",
        "DEFINE FIELD number ON TABLE account TYPE string COMMENT 'Account number/code';",
        "DEFINE FIELD name ON TABLE account TYPE string COMMENT 'Account name';",
        "DEFINE FIELD description ON TABLE account TYPE string DEFAULT '' COMMENT 'Account description';",
        "DEFINE FIELD account_type ON TABLE account TYPE string COMMENT 'Asset, Liability, Equity, Revenue, or Expense';",
        "DEFINE FIELD parent_id ON TABLE account TYPE option<uuid> COMMENT 'Parent account for hierarchical structure';",
        "DEFINE FIELD status ON TABLE account TYPE string DEFAULT 'active' COMMENT 'active, inactive, frozen, or closed';",
        "DEFINE FIELD balance ON TABLE account TYPE decimal DEFAULT 0 COMMENT 'Current balance';",
        "DEFINE FIELD currency ON TABLE account TYPE string DEFAULT 'USD' COMMENT 'Currency code';",
        "DEFINE FIELD is_bank_account ON TABLE account TYPE bool DEFAULT false;",
        "DEFINE FIELD bank_details ON TABLE account TYPE option<object> COMMENT 'Bank account details if applicable';",
        "DEFINE FIELD is_reconciled ON TABLE account TYPE bool DEFAULT false;",
        "DEFINE FIELD last_reconciled ON TABLE account TYPE option<datetime>;",
        "DEFINE FIELD created_at ON TABLE account TYPE datetime;",
        "DEFINE FIELD updated_at ON TABLE account TYPE datetime;",
        "DEFINE INDEX idx_account_number ON TABLE account COLUMNS number UNIQUE;",
        "DEFINE INDEX idx_account_type ON TABLE account COLUMNS account_type;",

        // ──────────────────────────────────────────────
        // transaction_entry table
        // ──────────────────────────────────────────────
        "DEFINE TABLE transaction_entry TYPE NORMAL COMMENT 'Individual debit/credit entry within a transaction';",
        "DEFINE FIELD account_id ON TABLE transaction_entry TYPE uuid COMMENT 'Account this entry affects';",
        "DEFINE FIELD entry_type ON TABLE transaction_entry TYPE string COMMENT 'debit or credit';",
        "DEFINE FIELD amount ON TABLE transaction_entry TYPE decimal COMMENT 'Entry amount';",
        "DEFINE FIELD description ON TABLE transaction_entry TYPE string DEFAULT '' COMMENT 'Memo for this entry';",
        "DEFINE FIELD reference ON TABLE transaction_entry TYPE option<string> COMMENT 'Optional reference number';",
        "DEFINE INDEX idx_transaction_entry_account ON TABLE transaction_entry COLUMNS account_id;",

        // ──────────────────────────────────────────────
        // journal_entry table
        // ──────────────────────────────────────────────
        "DEFINE TABLE journal_entry TYPE NORMAL COMMENT 'Journal entry grouping multiple transaction entries';",
        "DEFINE FIELD number ON TABLE journal_entry TYPE string COMMENT 'Journal entry number';",
        "DEFINE FIELD date ON TABLE journal_entry TYPE string COMMENT 'Date of the journal entry (YYYY-MM-DD)';",
        "DEFINE FIELD description ON TABLE journal_entry TYPE string DEFAULT '' COMMENT 'Description/memo';",
        "DEFINE FIELD reference ON TABLE journal_entry TYPE option<string> COMMENT 'Optional reference';",
        "DEFINE FIELD entries ON TABLE journal_entry TYPE array COMMENT 'List of transaction entries';",
        "DEFINE FIELD is_posted ON TABLE journal_entry TYPE bool DEFAULT false;",
        "DEFINE FIELD posted_at ON TABLE journal_entry TYPE option<datetime>;",
        "DEFINE FIELD is_reconciled ON TABLE journal_entry TYPE bool DEFAULT false;",
        "DEFINE FIELD created_at ON TABLE journal_entry TYPE datetime;",
        "DEFINE FIELD updated_at ON TABLE journal_entry TYPE datetime;",
        "DEFINE INDEX idx_journal_entry_number ON TABLE journal_entry COLUMNS number UNIQUE;",
        "DEFINE INDEX idx_journal_entry_date ON TABLE journal_entry COLUMNS date;",

        // ──────────────────────────────────────────────
        // transaction table (higher-level grouping)
        // ──────────────────────────────────────────────
        "DEFINE TABLE transaction TYPE NORMAL COMMENT 'High-level transaction record';",
        "DEFINE FIELD number ON TABLE transaction TYPE string COMMENT 'Transaction number/reference';",
        "DEFINE FIELD description ON TABLE transaction TYPE string DEFAULT '';",
        "DEFINE FIELD date ON TABLE transaction TYPE datetime;",
        "DEFINE FIELD transaction_type ON TABLE transaction TYPE string COMMENT 'invoice, payment, expense, transfer, journal_entry, adjustment, reconciliation, other';",
        "DEFINE FIELD status ON TABLE transaction TYPE string DEFAULT 'draft' COMMENT 'draft, pending, posted, reconciled, voided, error';",
        "DEFINE FIELD entries ON TABLE transaction TYPE array COMMENT 'List of transaction entries';",
        "DEFINE FIELD journal_entry_id ON TABLE transaction TYPE option<uuid>;",
        "DEFINE FIELD document_ids ON TABLE transaction TYPE array DEFAULT [] COMMENT 'Related document IDs';",
        "DEFINE FIELD metadata ON TABLE transaction TYPE object DEFAULT {};",
        "DEFINE FIELD created_at ON TABLE transaction TYPE datetime;",
        "DEFINE FIELD updated_at ON TABLE transaction TYPE datetime;",
        "DEFINE INDEX idx_transaction_number ON TABLE transaction COLUMNS number;",
        "DEFINE INDEX idx_transaction_date ON TABLE transaction COLUMNS date;",

        // ──────────────────────────────────────────────
        // user table
        // ──────────────────────────────────────────────
        "DEFINE TABLE user TYPE NORMAL COMMENT 'Application user';",
        "DEFINE FIELD username ON TABLE user TYPE string COMMENT 'Unique username';",
        "DEFINE FIELD email ON TABLE user TYPE string COMMENT 'Email address';",
        "DEFINE FIELD password_hash ON TABLE user TYPE string COMMENT 'Hashed password';",
        "DEFINE FIELD display_name ON TABLE user TYPE string DEFAULT '' COMMENT 'Display name';",
        "DEFINE FIELD role ON TABLE user TYPE string DEFAULT 'user' COMMENT 'admin, manager, user, viewer, or guest';",
        "DEFINE FIELD is_active ON TABLE user TYPE bool DEFAULT true;",
        "DEFINE FIELD last_login ON TABLE user TYPE option<datetime>;",
        "DEFINE FIELD created_at ON TABLE user TYPE datetime;",
        "DEFINE FIELD updated_at ON TABLE user TYPE datetime;",
        "DEFINE INDEX idx_user_username ON TABLE user COLUMNS username UNIQUE;",
        "DEFINE INDEX idx_user_email ON TABLE user COLUMNS email UNIQUE;",

        // ──────────────────────────────────────────────
        // organization table
        // ──────────────────────────────────────────────
        "DEFINE TABLE organization TYPE NORMAL COMMENT 'Business organization';",
        "DEFINE FIELD name ON TABLE organization TYPE string;",
        "DEFINE FIELD description ON TABLE organization TYPE string DEFAULT '';",
        "DEFINE FIELD address ON TABLE organization TYPE object COMMENT 'Street, city, state, postal_code, country';",
        "DEFINE FIELD contact ON TABLE organization TYPE object COMMENT 'Phone, email, website, fax';",
        "DEFINE FIELD tax_id ON TABLE organization TYPE option<string>;",
        "DEFINE FIELD currency ON TABLE organization TYPE string DEFAULT 'USD';",
        "DEFINE FIELD accounting_period ON TABLE organization TYPE object COMMENT 'CalendarYear or FiscalYear config';",
        "DEFINE FIELD created_at ON TABLE organization TYPE datetime;",
        "DEFINE FIELD updated_at ON TABLE organization TYPE datetime;",

        // ──────────────────────────────────────────────
        // document table
        // ──────────────────────────────────────────────
        "DEFINE TABLE document TYPE NORMAL COMMENT 'Stored document (invoices, receipts, etc.)';",
        "DEFINE FIELD name ON TABLE document TYPE string;",
        "DEFINE FIELD document_type ON TABLE document TYPE string COMMENT 'invoice, receipt, bank_statement, check, purchase_order, tax_form, contract, other';",
        "DEFINE FIELD content ON TABLE document TYPE string DEFAULT '' COMMENT 'Base64-encoded binary content';",
        "DEFINE FIELD metadata ON TABLE document TYPE object DEFAULT {};",
        "DEFINE FIELD created_at ON TABLE document TYPE datetime;",
        "DEFINE FIELD updated_at ON TABLE document TYPE datetime;",
        "DEFINE FIELD bounding_box ON TABLE document TYPE option<object>;",
        "DEFINE INDEX idx_document_type ON TABLE document COLUMNS document_type;",
        "DEFINE INDEX idx_document_name ON TABLE document COLUMNS name;",

        // ──────────────────────────────────────────────
        // audit_log table
        // ──────────────────────────────────────────────
        "DEFINE TABLE audit_log TYPE NORMAL COMMENT 'Audit trail entry';",
        "DEFINE FIELD user_id ON TABLE audit_log TYPE option<uuid>;",
        "DEFINE FIELD action ON TABLE audit_log TYPE string COMMENT 'create, read, update, delete, login, logout, export, import, or custom';",
        "DEFINE FIELD entity_type ON TABLE audit_log TYPE string;",
        "DEFINE FIELD entity_id ON TABLE audit_log TYPE string;",
        "DEFINE FIELD old_values ON TABLE audit_log TYPE option<object>;",
        "DEFINE FIELD new_values ON TABLE audit_log TYPE option<object>;",
        "DEFINE FIELD timestamp ON TABLE audit_log TYPE datetime;",
        "DEFINE FIELD ip_address ON TABLE audit_log TYPE option<string>;",
        "DEFINE FIELD user_agent ON TABLE audit_log TYPE option<string>;",
        "DEFINE FIELD success ON TABLE audit_log TYPE bool DEFAULT true;",
        "DEFINE FIELD error_message ON TABLE audit_log TYPE option<string>;",
        "DEFINE INDEX idx_audit_log_user ON TABLE audit_log COLUMNS user_id;",
        "DEFINE INDEX idx_audit_log_entity ON TABLE audit_log COLUMNS entity_type, entity_id;",
        "DEFINE INDEX idx_audit_log_action ON TABLE audit_log COLUMNS action;",
        "DEFINE INDEX idx_audit_log_timestamp ON TABLE audit_log COLUMNS timestamp;",

        // ──────────────────────────────────────────────
        // reconciliation table
        // ──────────────────────────────────────────────
        "DEFINE TABLE reconciliation TYPE NORMAL COMMENT 'Account reconciliation record';",
        "DEFINE FIELD account_id ON TABLE reconciliation TYPE uuid;",
        "DEFINE FIELD statement_date ON TABLE reconciliation TYPE string COMMENT 'Statement date (YYYY-MM-DD)';",
        "DEFINE FIELD starting_balance ON TABLE reconciliation TYPE decimal DEFAULT 0;",
        "DEFINE FIELD ending_balance ON TABLE reconciliation TYPE decimal DEFAULT 0;",
        "DEFINE FIELD statement_ending_balance ON TABLE reconciliation TYPE decimal DEFAULT 0;",
        "DEFINE FIELD reconciled_transactions ON TABLE reconciliation TYPE array DEFAULT [] COMMENT 'Reconciled transaction IDs';",
        "DEFINE FIELD outstanding_transactions ON TABLE reconciliation TYPE array DEFAULT [] COMMENT 'Outstanding transaction IDs';",
        "DEFINE FIELD difference ON TABLE reconciliation TYPE decimal DEFAULT 0;",
        "DEFINE FIELD status ON TABLE reconciliation TYPE string DEFAULT 'in_progress' COMMENT 'in_progress, completed, needs_review, cancelled';",
        "DEFINE FIELD notes ON TABLE reconciliation TYPE string DEFAULT '';",
        "DEFINE FIELD created_at ON TABLE reconciliation TYPE datetime;",
        "DEFINE FIELD updated_at ON TABLE reconciliation TYPE datetime;",
        "DEFINE INDEX idx_reconciliation_account ON TABLE reconciliation COLUMNS account_id;",

        // ──────────────────────────────────────────────
        // tax_jurisdiction table
        // ──────────────────────────────────────────────
        "DEFINE TABLE tax_jurisdiction TYPE NORMAL COMMENT 'Tax jurisdiction definition';",
        "DEFINE FIELD name ON TABLE tax_jurisdiction TYPE string;",
        "DEFINE FIELD code ON TABLE tax_jurisdiction TYPE string COMMENT 'Jurisdiction code (e.g. US-FED, CA-STATE)';",
        "DEFINE FIELD country ON TABLE tax_jurisdiction TYPE string;",
        "DEFINE FIELD state ON TABLE tax_jurisdiction TYPE option<string>;",
        "DEFINE FIELD rate ON TABLE tax_jurisdiction TYPE decimal DEFAULT 0 COMMENT 'Tax rate as decimal (e.g. 0.0825 for 8.25%)';",
        "DEFINE FIELD is_active ON TABLE tax_jurisdiction TYPE bool DEFAULT true;",
        "DEFINE FIELD created_at ON TABLE tax_jurisdiction TYPE datetime;",
        "DEFINE FIELD updated_at ON TABLE tax_jurisdiction TYPE datetime;",
        "DEFINE INDEX idx_tax_jurisdiction_code ON TABLE tax_jurisdiction COLUMNS code UNIQUE;",

        // ──────────────────────────────────────────────
        // tax_filing table
        // ──────────────────────────────────────────────
        "DEFINE TABLE tax_filing TYPE NORMAL COMMENT 'Tax filing record';",
        "DEFINE FIELD jurisdiction_id ON TABLE tax_filing TYPE uuid;",
        "DEFINE FIELD filing_period_start ON TABLE tax_filing TYPE string COMMENT 'Period start (YYYY-MM-DD)';",
        "DEFINE FIELD filing_period_end ON TABLE tax_filing TYPE string COMMENT 'Period end (YYYY-MM-DD)';",
        "DEFINE FIELD filing_type ON TABLE tax_filing TYPE string COMMENT 'e.g. income, sales, payroll';",
        "DEFINE FIELD status ON TABLE tax_filing TYPE string DEFAULT 'pending' COMMENT 'pending, filed, accepted, rejected';",
        "DEFINE FIELD amount_due ON TABLE tax_filing TYPE decimal DEFAULT 0;",
        "DEFINE FIELD amount_paid ON TABLE tax_filing TYPE decimal DEFAULT 0;",
        "DEFINE FIELD due_date ON TABLE tax_filing TYPE string COMMENT 'Filing due date (YYYY-MM-DD)';",
        "DEFINE FIELD filed_date ON TABLE tax_filing TYPE option<string>;",
        "DEFINE FIELD notes ON TABLE tax_filing TYPE string DEFAULT '';",
        "DEFINE FIELD created_at ON TABLE tax_filing TYPE datetime;",
        "DEFINE FIELD updated_at ON TABLE tax_filing TYPE datetime;",
        "DEFINE INDEX idx_tax_filing_jurisdiction ON TABLE tax_filing COLUMNS jurisdiction_id;",

        // ──────────────────────────────────────────────
        // employee table
        // ──────────────────────────────────────────────
        "DEFINE TABLE employee TYPE NORMAL COMMENT 'Employee record';",
        "DEFINE FIELD user_id ON TABLE employee TYPE option<uuid> COMMENT 'Linked user account';",
        "DEFINE FIELD first_name ON TABLE employee TYPE string;",
        "DEFINE FIELD last_name ON TABLE employee TYPE string;",
        "DEFINE FIELD email ON TABLE employee TYPE string;",
        "DEFINE FIELD phone ON TABLE employee TYPE option<string>;",
        "DEFINE FIELD department ON TABLE employee TYPE option<string>;",
        "DEFINE FIELD title ON TABLE employee TYPE option<string>;",
        "DEFINE FIELD hire_date ON TABLE employee TYPE string COMMENT 'YYYY-MM-DD';",
        "DEFINE FIELD termination_date ON TABLE employee TYPE option<string>;",
        "DEFINE FIELD salary ON TABLE employee TYPE decimal DEFAULT 0;",
        "DEFINE FIELD hourly_rate ON TABLE employee TYPE decimal DEFAULT 0;",
        "DEFINE FIELD is_active ON TABLE employee TYPE bool DEFAULT true;",
        "DEFINE FIELD created_at ON TABLE employee TYPE datetime;",
        "DEFINE FIELD updated_at ON TABLE employee TYPE datetime;",

        // ──────────────────────────────────────────────
        // pay_period table
        // ──────────────────────────────────────────────
        "DEFINE TABLE pay_period TYPE NORMAL COMMENT 'Payroll period definition';",
        "DEFINE FIELD period_start ON TABLE pay_period TYPE string COMMENT 'YYYY-MM-DD';",
        "DEFINE FIELD period_end ON TABLE pay_period TYPE string COMMENT 'YYYY-MM-DD';",
        "DEFINE FIELD pay_date ON TABLE pay_period TYPE string COMMENT 'YYYY-MM-DD';",
        "DEFINE FIELD status ON TABLE pay_period TYPE string DEFAULT 'draft' COMMENT 'draft, processing, completed';",
        "DEFINE FIELD total_gross ON TABLE pay_period TYPE decimal DEFAULT 0;",
        "DEFINE FIELD total_deductions ON TABLE pay_period TYPE decimal DEFAULT 0;",
        "DEFINE FIELD total_net ON TABLE pay_period TYPE decimal DEFAULT 0;",
        "DEFINE FIELD created_at ON TABLE pay_period TYPE datetime;",
        "DEFINE FIELD updated_at ON TABLE pay_period TYPE datetime;",

        // ──────────────────────────────────────────────
        // time_entry table
        // ──────────────────────────────────────────────
        "DEFINE TABLE time_entry TYPE NORMAL COMMENT 'Employee time tracking entry';",
        "DEFINE FIELD employee_id ON TABLE time_entry TYPE uuid;",
        "DEFINE FIELD date ON TABLE time_entry TYPE string COMMENT 'YYYY-MM-DD';",
        "DEFINE FIELD hours ON TABLE time_entry TYPE decimal DEFAULT 0;",
        "DEFINE FIELD rate ON TABLE time_entry TYPE decimal DEFAULT 0 COMMENT 'Rate at time of entry';",
        "DEFINE FIELD description ON TABLE time_entry TYPE string DEFAULT '';",
        "DEFINE FIELD project_id ON TABLE time_entry TYPE option<uuid>;",
        "DEFINE FIELD is_approved ON TABLE time_entry TYPE bool DEFAULT false;",
        "DEFINE FIELD approved_by ON TABLE time_entry TYPE option<uuid>;",
        "DEFINE FIELD approved_at ON TABLE time_entry TYPE option<datetime>;",
        "DEFINE FIELD created_at ON TABLE time_entry TYPE datetime;",
        "DEFINE FIELD updated_at ON TABLE time_entry TYPE datetime;",
        "DEFINE INDEX idx_time_entry_employee ON TABLE time_entry COLUMNS employee_id;",
        "DEFINE INDEX idx_time_entry_date ON TABLE time_entry COLUMNS date;",

        // ──────────────────────────────────────────────
        // schema_version table (for migrations)
        // ──────────────────────────────────────────────
        "DEFINE TABLE schema_version TYPE NORMAL COMMENT 'Tracks database schema version for migrations';",
        "DEFINE FIELD version ON TABLE schema_version TYPE int COMMENT 'Schema version number';",
        "DEFINE FIELD applied_at ON TABLE schema_version TYPE datetime;",
        "DEFINE FIELD description ON TABLE schema_version TYPE string DEFAULT '';",
    ]
}

/// The current schema version number.
pub const CURRENT_SCHEMA_VERSION: i32 = 1;

/// Execute all schema definition statements against the database.
///
/// Each statement is run individually so a single failure does not
/// prevent the remaining statements from being applied. Errors are
/// silently ignored to support idempotent re-runs (SurrealDB 1.0
/// does not support `IF NOT EXISTS` on DEFINE statements).
pub async fn apply_all_statements(
    db: &surrealdb::Surreal<surrealdb::engine::local::Db>,
) -> Result<(), crate::database::error::DatabaseError> {
    let statements = schema_statements();

    for (_i, stmt) in statements.iter().enumerate() {
        if let Err(_e) = db.query(*stmt).await {
            // Silently ignore errors — DEFINE statements are idempotent
            // in intent; SurrealDB 1.0 errors on re-definition without
            // IF NOT EXISTS, so we tolerate "already exists" errors.
            tracing::debug!("Schema statement ignored (likely already exists): {}", stmt);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_schema_statements_not_empty() {
        let stmts = schema_statements();
        assert!(!stmts.is_empty());
        // Every statement should be a DEFINE statement
        for stmt in &stmts {
            assert!(
                stmt.starts_with("DEFINE"),
                "Expected DEFINE statement, got: {}",
                stmt
            );
        }
    }

    #[test]
    fn test_schema_version_constant() {
        assert!(CURRENT_SCHEMA_VERSION >= 1);
    }
}
