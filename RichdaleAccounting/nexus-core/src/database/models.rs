//! Database Models Module
//!
//! Defines the core data models for the NexusLedger database.

use serde::{Serialize, Deserialize};
use chrono::{DateTime, Utc};
use uuid::Uuid;
use rust_decimal::Decimal;

/// Bounding box for document regions
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BoundingBox {
    /// X coordinate of top-left corner
    pub x: f64,
    /// Y coordinate of top-left corner
    pub y: f64,
    /// Width of the bounding box
    pub width: f64,
    /// Height of the bounding box
    pub height: f64,
}

impl BoundingBox {
    /// Create a new bounding box
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self { x, y, width, height }
    }

    /// Check if a point is inside the bounding box
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && x <= self.x + self.width &&
        y >= self.y && y <= self.y + self.height
    }

    /// Get the center point
    pub fn center(&self) -> (f64, f64) {
        (self.x + self.width / 2.0, self.y + self.height / 2.0)
    }
}

/// Document types
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum DocumentType {
    /// Invoice document
    Invoice,
    /// Receipt document
    Receipt,
    /// Bank statement
    BankStatement,
    /// Check
    Check,
    /// Purchase order
    PurchaseOrder,
    /// Tax form
    TaxForm,
    /// Contract
    Contract,
    /// Other document type
    Other,
}

impl Default for DocumentType {
    fn default() -> Self {
        Self::Other
    }
}

impl DocumentType {
    /// Get document type from string
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "invoice" => Self::Invoice,
            "receipt" => Self::Receipt,
            "bank statement" | "bank_statement" | "statement" => Self::BankStatement,
            "check" => Self::Check,
            "purchase order" | "purchase_order" | "po" => Self::PurchaseOrder,
            "tax form" | "tax_form" | "tax" => Self::TaxForm,
            "contract" => Self::Contract,
            _ => Self::Other,
        }
    }

    /// Convert to string
    pub fn to_str(&self) -> &'static str {
        match self {
            Self::Invoice => "invoice",
            Self::Receipt => "receipt",
            Self::BankStatement => "bank_statement",
            Self::Check => "check",
            Self::PurchaseOrder => "purchase_order",
            Self::TaxForm => "tax_form",
            Self::Contract => "contract",
            Self::Other => "other",
        }
    }
}

/// User model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    /// Unique identifier
    pub id: Uuid,
    /// Username
    pub username: String,
    /// Email address
    pub email: String,
    /// Hashed password
    pub password_hash: String,
    /// Display name
    pub display_name: String,
    /// User role
    pub role: UserRole,
    /// Whether the user is active
    pub is_active: bool,
    /// Last login timestamp
    pub last_login: Option<DateTime<Utc>>,
    /// Created timestamp
    pub created_at: DateTime<Utc>,
    /// Updated timestamp
    pub updated_at: DateTime<Utc>,
}

impl Default for User {
    fn default() -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            username: String::new(),
            email: String::new(),
            password_hash: String::new(),
            display_name: String::new(),
            role: UserRole::default(),
            is_active: true,
            last_login: None,
            created_at: now,
            updated_at: now,
        }
    }
}

impl User {
    /// Create a new user
    pub fn new(username: &str, email: &str, display_name: &str) -> Self {
        Self {
            username: username.to_string(),
            email: email.to_string(),
            display_name: display_name.to_string(),
            ..Default::default()
        }
    }
}

/// User roles
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum UserRole {
    /// Guest with limited access (level 0)
    Guest = 0,
    /// Read-only user (level 1)
    Viewer = 1,
    /// Regular user — CRUD own data (level 2)
    User = 2,
    /// Accounting manager — CRUD all data (level 3)
    Manager = 3,
    /// Administrator with full access (level 4)
    Admin = 4,
}

impl UserRole {
    /// Numeric privilege level (higher = more access).
    pub fn level(&self) -> u8 {
        match self {
            Self::Guest => 0,
            Self::Viewer => 1,
            Self::User => 2,
            Self::Manager => 3,
            Self::Admin => 4,
        }
    }

    /// Check if this role is at least the given minimum.
    pub fn is_at_least(&self, minimum: &UserRole) -> bool {
        self.level() >= minimum.level()
    }

    /// Whether this role can read data (Viewer+).
    pub fn can_read(&self) -> bool {
        self.is_at_least(&UserRole::Viewer)
    }

    /// Whether this role can create/update/delete data (User+).
    pub fn can_write(&self) -> bool {
        self.is_at_least(&UserRole::User)
    }

    /// Whether this role can manage all data (Manager+).
    pub fn can_manage(&self) -> bool {
        self.is_at_least(&UserRole::Manager)
    }

    /// Whether this role can administer the system (Admin).
    pub fn can_administer(&self) -> bool {
        self.is_at_least(&UserRole::Admin)
    }
}

impl Default for UserRole {
    fn default() -> Self {
        Self::User
    }
}

/// Organization model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Organization {
    /// Unique identifier
    pub id: Uuid,
    /// Organization name
    pub name: String,
    /// Organization description
    pub description: String,
    /// Address
    pub address: Address,
    /// Contact information
    pub contact: ContactInfo,
    /// Tax identification number
    pub tax_id: Option<String>,
    /// Currency
    pub currency: String,
    /// Accounting period
    pub accounting_period: AccountingPeriod,
    /// Created timestamp
    pub created_at: DateTime<Utc>,
    /// Updated timestamp
    pub updated_at: DateTime<Utc>,
}

impl Default for Organization {
    fn default() -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            name: String::new(),
            description: String::new(),
            address: Address::default(),
            contact: ContactInfo::default(),
            tax_id: None,
            currency: "USD".to_string(),
            accounting_period: AccountingPeriod::default(),
            created_at: now,
            updated_at: now,
        }
    }
}

/// Address model
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Address {
    /// Street address line 1
    pub street1: String,
    /// Street address line 2
    pub street2: String,
    /// City
    pub city: String,
    /// State/Province
    pub state: String,
    /// Postal code
    pub postal_code: String,
    /// Country
    pub country: String,
}

/// Contact information
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ContactInfo {
    /// Phone number
    pub phone: String,
    /// Email address
    pub email: String,
    /// Website
    pub website: String,
    /// Fax number
    pub fax: String,
}

/// Accounting period
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AccountingPeriod {
    /// Calendar year (January 1 - December 31)
    CalendarYear,
    /// Fiscal year starting in a specific month
    FiscalYear(u8), // Month (1-12)
    /// Custom date range
    Custom { start_month: u8, start_day: u8 },
}

impl Default for AccountingPeriod {
    fn default() -> Self {
        Self::CalendarYear
    }
}

/// Document model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    /// Unique identifier
    pub id: String,
    /// Document name
    pub name: String,
    /// Document type
    pub document_type: DocumentType,
    /// Document content (binary data)
    pub content: Vec<u8>,
    /// Metadata
    pub metadata: serde_json::Value,
    /// Created timestamp
    pub created_at: DateTime<Utc>,
    /// Updated timestamp
    pub updated_at: DateTime<Utc>,
    /// Bounding box for document region (optional)
    pub bounding_box: Option<BoundingBox>,
}

impl Default for Document {
    fn default() -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            name: String::new(),
            document_type: DocumentType::default(),
            content: Vec::new(),
            metadata: serde_json::json!({}),
            created_at: now,
            updated_at: now,
            bounding_box: None,
        }
    }
}

impl Document {
    /// Create a new document
    pub fn new(name: &str, document_type: DocumentType, content: Vec<u8>) -> Self {
        Self {
            name: name.to_string(),
            document_type,
            content,
            ..Default::default()
        }
    }

    /// Get document size in bytes
    pub fn size(&self) -> usize {
        self.content.len()
    }

    /// Check if document is empty
    pub fn is_empty(&self) -> bool {
        self.content.is_empty()
    }
}

/// Audit log entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditLog {
    /// Unique identifier
    pub id: Uuid,
    /// User who performed the action
    pub user_id: Option<Uuid>,
    /// Action performed
    pub action: AuditAction,
    /// Entity type affected
    pub entity_type: String,
    /// Entity ID affected
    pub entity_id: String,
    /// Old values (for updates)
    pub old_values: Option<serde_json::Value>,
    /// New values (for updates and creates)
    pub new_values: Option<serde_json::Value>,
    /// Timestamp
    pub timestamp: DateTime<Utc>,
    /// IP address
    pub ip_address: Option<String>,
    /// User agent
    pub user_agent: Option<String>,
    /// Success status
    pub success: bool,
    /// Error message if any
    pub error_message: Option<String>,
    /// Previous entry hash (SHA-256, hex-encoded) — forms an immutable chain
    pub prev_hash: Option<String>,
    /// Hash of this entry (SHA-256 of canonical JSON)
    pub chain_hash: Option<String>,
}

impl Default for AuditLog {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4(),
            user_id: None,
            action: AuditAction::default(),
            entity_type: String::new(),
            entity_id: String::new(),
            old_values: None,
            new_values: None,
            timestamp: Utc::now(),
            ip_address: None,
            user_agent: None,
            success: true,
            error_message: None,
            prev_hash: None,
            chain_hash: None,
        }
    }
}

/// Audit action types
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum AuditAction {
    /// Create operation
    Create,
    /// Read operation
    Read,
    /// Update operation
    Update,
    /// Delete operation
    Delete,
    /// Login operation
    Login,
    /// Logout operation
    Logout,
    /// Export operation
    Export,
    /// Import operation
    Import,
    /// Custom action
    Custom(String),
}

impl Default for AuditAction {
    fn default() -> Self {
        Self::Create
    }
}

/// Settings model
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Settings {
    /// Settings key
    pub key: String,
    /// Settings value
    pub value: serde_json::Value,
    /// Description
    pub description: String,
    /// Category
    pub category: String,
    /// Updated timestamp
    pub updated_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bounding_box() {
        let bbox = BoundingBox::new(10.0, 20.0, 100.0, 200.0);
        assert!(bbox.contains(50.0, 100.0));
        assert!(!bbox.contains(5.0, 5.0));
        
        let center = bbox.center();
        assert_eq!(center.0, 60.0);
        assert_eq!(center.1, 120.0);
    }

    #[test]
    fn test_document_type_conversion() {
        assert_eq!(DocumentType::from_str("invoice"), DocumentType::Invoice);
        assert_eq!(DocumentType::from_str("INVOICE"), DocumentType::Invoice);
        assert_eq!(DocumentType::from_str("unknown"), DocumentType::Other);
        
        assert_eq!(DocumentType::Invoice.to_str(), "invoice");
    }

    #[test]
    fn test_user_creation() {
        let user = User::new("john_doe", "john@example.com", "John Doe");
        assert_eq!(user.username, "john_doe");
        assert_eq!(user.email, "john@example.com");
        assert_eq!(user.display_name, "John Doe");
    }

    #[test]
    fn test_document_creation() {
        let doc = Document::new("Test Doc", DocumentType::Invoice, vec![1, 2, 3]);
        assert_eq!(doc.name, "Test Doc");
        assert_eq!(doc.document_type, DocumentType::Invoice);
        assert_eq!(doc.size(), 3);
    }

    #[test]
    fn test_audit_log() {
        let log = AuditLog::default();
        assert!(log.success);
        assert!(log.error_message.is_none());
    }
}
