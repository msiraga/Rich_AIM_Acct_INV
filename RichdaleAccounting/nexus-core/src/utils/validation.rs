//! Validation Utilities Module
//!
//! Provides validation functionality for the NexusLedger system.

use std::fmt;
use std::str::FromStr;
use regex::Regex;
use rust_decimal::Decimal;
use chrono::{DateTime, Utc, NaiveDate};
use uuid::Uuid;
use thiserror::Error;

/// Validation error types
#[derive(Debug, Error)]
pub enum ValidationError {
    /// Required field is missing
    #[error("Required field '{0}' is missing")]
    RequiredField(String),
    
    /// Invalid format
    #[error("Invalid format for '{0}': {1}")]
    InvalidFormat(String, String),
    
    /// Value out of range
    #[error("Value for '{0}' is out of range: {1}")]
    OutOfRange(String, String),
    
    /// Value too short
    #[error("Value for '{0}' is too short (minimum: {1})")]
    TooShort(String, usize),
    
    /// Value too long
    #[error("Value for '{0}' is too long (maximum: {1})")]
    TooLong(String, usize),
    
    /// Invalid email format
    #[error("Invalid email format for '{0}'")]
    InvalidEmail(String),
    
    /// Invalid UUID format
    #[error("Invalid UUID format for '{0}'")]
    InvalidUuid(String),
    
    /// Invalid date format
    #[error("Invalid date format for '{0}'")]
    InvalidDate(String),
    
    /// Invalid decimal format
    #[error("Invalid decimal format for '{0}'")]
    InvalidDecimal(String),
    
    /// Custom validation error
    #[error("Validation error: {0}")]
    Custom(String),
}

impl ValidationError {
    /// Create a new required field error
    pub fn required(field: &str) -> Self {
        Self::RequiredField(field.to_string())
    }

    /// Create a new invalid format error
    pub fn invalid_format(field: &str, message: String) -> Self {
        Self::InvalidFormat(field.to_string(), message)
    }

    /// Create a new out of range error
    pub fn out_of_range(field: &str, message: String) -> Self {
        Self::OutOfRange(field.to_string(), message)
    }

    /// Create a new too short error
    pub fn too_short(field: &str, min: usize) -> Self {
        Self::TooShort(field.to_string(), min)
    }

    /// Create a new too long error
    pub fn too_long(field: &str, max: usize) -> Self {
        Self::TooLong(field.to_string(), max)
    }

    /// Create a new invalid email error
    pub fn invalid_email(email: &str) -> Self {
        Self::InvalidEmail(email.to_string())
    }

    /// Create a new invalid UUID error
    pub fn invalid_uuid(uuid: &str) -> Self {
        Self::InvalidUuid(uuid.to_string())
    }

    /// Create a new invalid date error
    pub fn invalid_date(date: &str) -> Self {
        Self::InvalidDate(date.to_string())
    }

    /// Create a new invalid decimal error
    pub fn invalid_decimal(value: &str) -> Self {
        Self::InvalidDecimal(value.to_string())
    }

    /// Create a new custom error
    pub fn custom(message: &str) -> Self {
        Self::Custom(message.to_string())
    }
}

// Display auto-derived by thiserror

/// Result type for validation operations
pub type ValidationResult<T> = Result<T, ValidationError>;

/// Validator for validating data
#[derive(Debug, Clone)]
pub struct Validator;

impl Validator {
    /// Validate that a value is not empty
    pub fn required<T: AsRef<str>>(value: T, field: &str) -> ValidationResult<()> {
        if value.as_ref().trim().is_empty() {
            Err(ValidationError::required(field))
        } else {
            Ok(())
        }
    }

    /// Validate that a value has a minimum length
    pub fn min_length(value: &str, min: usize, field: &str) -> ValidationResult<()> {
        if value.len() < min {
            Err(ValidationError::too_short(field, min))
        } else {
            Ok(())
        }
    }

    /// Validate that a value has a maximum length
    pub fn max_length(value: &str, max: usize, field: &str) -> ValidationResult<()> {
        if value.len() > max {
            Err(ValidationError::too_long(field, max))
        } else {
            Ok(())
        }
    }

    /// Validate that a value has a length within a range
    pub fn length_range(value: &str, min: usize, max: usize, field: &str) -> ValidationResult<()> {
        if value.len() < min {
            Err(ValidationError::too_short(field, min))
        } else if value.len() > max {
            Err(ValidationError::too_long(field, max))
        } else {
            Ok(())
        }
    }

    /// Validate that a value matches a regex pattern
    pub fn pattern(value: &str, pattern: &str, field: &str) -> ValidationResult<()> {
        let regex = Regex::new(pattern)
            .map_err(|e| ValidationError::custom(&format!("Invalid regex pattern '{}': {}", pattern, e)))?;
        
        if !regex.is_match(value) {
            Err(ValidationError::invalid_format(field, format!("Does not match pattern: {}", pattern)))
        } else {
            Ok(())
        }
    }

    /// Validate an email address
    pub fn email(email: &str, field: &str) -> ValidationResult<()> {
        // Simple email validation regex
        let pattern = r"^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$";
        Self::pattern(email, pattern, field)
    }

    /// Validate a UUID
    pub fn uuid(uuid: &str, field: &str) -> ValidationResult<Uuid> {
        Uuid::parse_str(uuid)
            .map_err(|_| ValidationError::invalid_uuid(uuid))
    }

    /// Validate a date string (ISO 8601 format)
    pub fn date(date_str: &str, field: &str) -> ValidationResult<NaiveDate> {
        NaiveDate::parse_from_str(date_str, "%Y-%m-%d")
            .map_err(|_| ValidationError::invalid_date(date_str))
    }

    /// Validate a datetime string (RFC 3339 format)
    pub fn datetime(datetime_str: &str, field: &str) -> ValidationResult<DateTime<Utc>> {
        DateTime::parse_from_rfc3339(datetime_str)
            .map(|dt| dt.with_timezone(&Utc))
            .map_err(|_| ValidationError::invalid_date(datetime_str))
    }

    /// Validate a decimal number
    pub fn decimal(value: &str, field: &str) -> ValidationResult<Decimal> {
        Decimal::from_str(value)
            .map_err(|_| ValidationError::invalid_decimal(value))
    }

    /// Validate that a number is within a range
    pub fn range<T: PartialOrd + fmt::Display + Copy>(value: T, min: T, max: T, field: &str) -> ValidationResult<()> {
        if value < min {
            Err(ValidationError::out_of_range(field, format!("Value must be >= {}", min)))
        } else if value > max {
            Err(ValidationError::out_of_range(field, format!("Value must be <= {}", max)))
        } else {
            Ok(())
        }
    }

    /// Validate that a number is positive
    pub fn positive<T: PartialOrd + fmt::Display + Copy + From<i8>>(value: T, field: &str) -> ValidationResult<()> {
        Self::range(value, T::from(0), T::from(i8::MAX), field)
    }

    /// Validate that a number is non-negative
    pub fn non_negative<T: PartialOrd + fmt::Display + Copy + From<i8>>(value: T, field: &str) -> ValidationResult<()> {
        Self::range(value, T::from(0), T::from(i8::MAX), field)
    }

    /// Validate that a value is one of the allowed values
    pub fn one_of<T: PartialEq + fmt::Display + fmt::Debug>(value: T, allowed: &[T], field: &str) -> ValidationResult<()> {
        if !allowed.contains(&value) {
            Err(ValidationError::invalid_format(
                field,
                format!("Value must be one of: {:?}", allowed)
            ))
        } else {
            Ok(())
        }
    }

    /// Validate a string is alphanumeric
    pub fn alphanumeric(value: &str, field: &str) -> ValidationResult<()> {
        if !value.chars().all(|c| c.is_ascii_alphanumeric()) {
            Err(ValidationError::invalid_format(field, "Value must be alphanumeric".to_string()))
        } else {
            Ok(())
        }
    }

    /// Validate a string is numeric
    pub fn numeric(value: &str, field: &str) -> ValidationResult<()> {
        if !value.chars().all(|c| c.is_ascii_digit()) {
            Err(ValidationError::invalid_format(field, "Value must be numeric".to_string()))
        } else {
            Ok(())
        }
    }

    /// Validate a string is alphabetic
    pub fn alphabetic(value: &str, field: &str) -> ValidationResult<()> {
        if !value.chars().all(|c| c.is_ascii_alphabetic()) {
            Err(ValidationError::invalid_format(field, "Value must be alphabetic".to_string()))
        } else {
            Ok(())
        }
    }

    /// Validate a string contains only whitespace
    pub fn whitespace(value: &str, field: &str) -> ValidationResult<()> {
        if !value.chars().all(|c| c.is_whitespace()) {
            Err(ValidationError::invalid_format(field, "Value must contain only whitespace".to_string()))
        } else {
            Ok(())
        }
    }

    /// Validate a string is uppercase
    pub fn uppercase(value: &str, field: &str) -> ValidationResult<()> {
        if !value.chars().all(|c| c.is_ascii_uppercase()) {
            Err(ValidationError::invalid_format(field, "Value must be uppercase".to_string()))
        } else {
            Ok(())
        }
    }

    /// Validate a string is lowercase
    pub fn lowercase(value: &str, field: &str) -> ValidationResult<()> {
        if !value.chars().all(|c| c.is_ascii_lowercase()) {
            Err(ValidationError::invalid_format(field, "Value must be lowercase".to_string()))
        } else {
            Ok(())
        }
    }

    /// Validate a string starts with a prefix
    pub fn starts_with(value: &str, prefix: &str, field: &str) -> ValidationResult<()> {
        if !value.starts_with(prefix) {
            Err(ValidationError::invalid_format(
                field,
                format!("Value must start with '{}'", prefix)
            ))
        } else {
            Ok(())
        }
    }

    /// Validate a string ends with a suffix
    pub fn ends_with(value: &str, suffix: &str, field: &str) -> ValidationResult<()> {
        if !value.ends_with(suffix) {
            Err(ValidationError::invalid_format(
                field,
                format!("Value must end with '{}'", suffix)
            ))
        } else {
            Ok(())
        }
    }

    /// Validate a string contains a substring
    pub fn contains(value: &str, substring: &str, field: &str) -> ValidationResult<()> {
        if !value.contains(substring) {
            Err(ValidationError::invalid_format(
                field,
                format!("Value must contain '{}'", substring)
            ))
        } else {
            Ok(())
        }
    }

    /// Validate a string matches another string
    pub fn equals(value: &str, expected: &str, field: &str) -> ValidationResult<()> {
        if value != expected {
            Err(ValidationError::invalid_format(
                field,
                format!("Value must equal '{}'", expected)
            ))
        } else {
            Ok(())
        }
    }

    /// Validate a string is not equal to another string
    pub fn not_equals(value: &str, unexpected: &str, field: &str) -> ValidationResult<()> {
        if value == unexpected {
            Err(ValidationError::invalid_format(
                field,
                format!("Value must not equal '{}'", unexpected)
            ))
        } else {
            Ok(())
        }
    }

    /// Validate multiple values using a chain of validators
    pub fn validate_all(results: Vec<ValidationResult<()>>) -> ValidationResult<()> {
        for result in results {
            result?;
        }
        Ok(())
    }
}

/// Validation rule
pub struct ValidationRule {
    /// Field name
    pub field: String,
    /// Validator function
    pub validator: Box<dyn Fn(&str) -> ValidationResult<()> + Send + Sync>,
}

impl std::fmt::Debug for ValidationRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ValidationRule")
            .field("field", &self.field)
            .field("validator", &"<closure>")
            .finish()
    }
}

impl Clone for ValidationRule {
    fn clone(&self) -> Self {
        panic!("ValidationRule cannot be cloned because it contains a closure")
    }
}

impl ValidationRule {
    /// Create a new validation rule
    pub fn new<F>(field: String, validator: F) -> Self
    where
        F: Fn(&str) -> ValidationResult<()> + 'static + Send + Sync,
    {
        Self {
            field,
            validator: Box::new(validator),
        }
    }

    /// Validate a value
    pub fn validate(&self, value: &str) -> ValidationResult<()> {
        (self.validator)(value)
    }
}

/// Validation schema
#[derive(Debug, Clone, Default)]
pub struct ValidationSchema {
    /// List of validation rules
    pub rules: Vec<ValidationRule>,
}

impl ValidationSchema {
    /// Create a new validation schema
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a validation rule
    pub fn add_rule(mut self, rule: ValidationRule) -> Self {
        self.rules.push(rule);
        self
    }

    /// Add a required rule
    pub fn required(mut self, field: &str) -> Self {
        let field = field.to_string();
        let f = field.clone();
        self.rules.push(ValidationRule::new(field, move |value| Validator::required(value, &f)));
        self
    }

    /// Add a min length rule
    pub fn min_length(mut self, field: &str, min: usize) -> Self {
        let field = field.to_string();
        let f = field.clone();
        self.rules.push(ValidationRule::new(field, move |value| Validator::min_length(value, min, &f)));
        self
    }

    /// Add a max length rule
    pub fn max_length(mut self, field: &str, max: usize) -> Self {
        let field = field.to_string();
        let f = field.clone();
        self.rules.push(ValidationRule::new(field, move |value| Validator::max_length(value, max, &f)));
        self
    }

    /// Add an email rule
    pub fn email(mut self, field: &str) -> Self {
        let field = field.to_string();
        let f = field.clone();
        self.rules.push(ValidationRule::new(field, move |value| Validator::email(value, &f)));
        self
    }

    /// Add a pattern rule
    pub fn pattern(mut self, field: &str, pattern: &str) -> Self {
        let field = field.to_string();
        let pattern = pattern.to_string();
        let f = field.clone();
        let p = pattern.clone();
        self.rules.push(ValidationRule::new(field, move |value| Validator::pattern(value, &p, &f)));
        self
    }

    /// Validate a value against the schema
    pub fn validate(&self, value: &str) -> ValidationResult<()> {
        let mut errors = Vec::new();
        
        for rule in &self.rules {
            if let Err(e) = rule.validate(value) {
                errors.push(e);
            }
        }
        
        if errors.is_empty() {
            Ok(())
        } else if errors.len() == 1 {
            Err(errors.into_iter().next().unwrap())
        } else {
            // Combine multiple errors into a single error message
            let messages: Vec<String> = errors.into_iter().map(|e| e.to_string()).collect();
            Err(ValidationError::custom(&messages.join("; ")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_required_validation() {
        assert!(Validator::required("", "name").is_err());
        assert!(Validator::required(" ", "name").is_err());
        assert!(Validator::required("John", "name").is_ok());
    }

    #[test]
    fn test_length_validation() {
        assert!(Validator::min_length("short", 10, "name").is_err());
        assert!(Validator::min_length("long enough", 10, "name").is_ok());
        
        assert!(Validator::max_length("short", 10, "name").is_ok());
        assert!(Validator::max_length("way too long", 10, "name").is_err());
        
        assert!(Validator::length_range("short", 10, 20, "name").is_err());
        assert!(Validator::length_range("just right", 10, 20, "name").is_ok());
        assert!(Validator::length_range("this string is way too long for the range", 10, 20, "name").is_err());
    }

    #[test]
    fn test_email_validation() {
        assert!(Validator::email("test@example.com", "email").is_ok());
        assert!(Validator::email("invalid-email", "email").is_err());
        assert!(Validator::email("test@.com", "email").is_err());
    }

    #[test]
    fn test_uuid_validation() {
        let uuid = Uuid::new_v4();
        assert!(Validator::uuid(&uuid.to_string(), "id").is_ok());
        assert!(Validator::uuid("invalid-uuid", "id").is_err());
    }

    #[test]
    fn test_date_validation() {
        assert!(Validator::date("2023-01-15", "date").is_ok());
        assert!(Validator::date("invalid-date", "date").is_err());
    }

    #[test]
    fn test_decimal_validation() {
        assert!(Validator::decimal("123.45", "amount").is_ok());
        assert!(Validator::decimal("invalid", "amount").is_err());
    }

    #[test]
    fn test_range_validation() {
        assert!(Validator::range(5, 1, 10, "value").is_ok());
        assert!(Validator::range(0, 1, 10, "value").is_err());
        assert!(Validator::range(11, 1, 10, "value").is_err());
    }

    #[test]
    fn test_pattern_validation() {
        assert!(Validator::pattern("123", r"^\d+$", "value").is_ok());
        assert!(Validator::pattern("abc", r"^\d+$", "value").is_err());
    }

    #[test]
    fn test_one_of_validation() {
        assert!(Validator::one_of("a", &["a", "b", "c"], "value").is_ok());
        assert!(Validator::one_of("d", &["a", "b", "c"], "value").is_err());
    }

    #[test]
    fn test_string_validation() {
        assert!(Validator::alphanumeric("abc123", "value").is_ok());
        assert!(Validator::alphanumeric("abc-123", "value").is_err());
        
        assert!(Validator::numeric("123", "value").is_ok());
        assert!(Validator::numeric("abc", "value").is_err());
        
        assert!(Validator::alphabetic("abc", "value").is_ok());
        assert!(Validator::alphabetic("abc123", "value").is_err());
    }

    #[test]
    fn test_validation_schema() {
        let schema = ValidationSchema::new()
            .required("name")
            .min_length("name", 3)
            .max_length("name", 50);

        assert!(schema.validate("").is_err());
        assert!(schema.validate("ab").is_err());
        assert!(schema.validate("John").is_ok());
    }

    #[test]
    fn test_validate_all() {
        let results = vec![
            Validator::required("John", "name"),
            Validator::email("test@example.com", "email"),
            Validator::range(5, 1, 10, "value"),
        ];
        
        assert!(Validator::validate_all(results).is_ok());
        
        let results = vec![
            Validator::required("", "name"),
            Validator::email("test@example.com", "email"),
        ];
        
        assert!(Validator::validate_all(results).is_err());
    }
}
