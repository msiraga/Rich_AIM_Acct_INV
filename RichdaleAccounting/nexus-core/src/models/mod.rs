//! Models Module
//!
//! This module contains shared data models for the NexusLedger system.

use serde::{Serialize, Deserialize};
use chrono::{DateTime, NaiveDate, Utc, Datelike};
use uuid::Uuid;

/// Date range for reporting and filtering
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DateRange {
    /// Start date
    pub start: DateTime<Utc>,
    /// End date
    pub end: DateTime<Utc>,
}

impl Default for DateRange {
    fn default() -> Self {
        let now = Utc::now();
        Self {
            start: now,
            end: now,
        }
    }
}

impl DateRange {
    /// Create a new date range
    pub fn new(start: DateTime<Utc>, end: DateTime<Utc>) -> Self {
        Self { start, end }
    }

    /// Create a date range for today
    pub fn today() -> Self {
        let now = Utc::now();
        let start = now.date_naive().and_hms_opt(0, 0, 0).unwrap().and_utc();
        let end = now.date_naive().and_hms_opt(23, 59, 59).unwrap().and_utc();
        Self { start, end }
    }

    /// Create a date range for the current month
    pub fn current_month() -> Self {
        let now = Utc::now();
        let start = NaiveDate::from_ymd_opt(now.date_naive().year(), now.date_naive().month(), 1)
            .unwrap().and_hms_opt(0, 0, 0).unwrap().and_utc();
        let end = NaiveDate::from_ymd_opt(now.date_naive().year(), now.date_naive().month(), now.date_naive().day())
            .unwrap().and_hms_opt(23, 59, 59).unwrap().and_utc();
        Self { start, end }
    }

    /// Create a date range for the current year
    pub fn current_year() -> Self {
        let now = Utc::now();
        let start = NaiveDate::from_ymd_opt(now.date_naive().year(), 1, 1)
            .unwrap().and_hms_opt(0, 0, 0).unwrap().and_utc();
        let end = NaiveDate::from_ymd_opt(now.date_naive().year(), 12, 31)
            .unwrap().and_hms_opt(23, 59, 59).unwrap().and_utc();
        Self { start, end }
    }

    /// Check if a date is within the range
    pub fn contains(&self, date: DateTime<Utc>) -> bool {
        date >= self.start && date <= self.end
    }

    /// Get the duration in days
    pub fn duration_days(&self) -> i64 {
        (self.end - self.start).num_days()
    }
}

/// Filter criteria for querying data
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FilterCriteria {
    /// Date range
    pub date_range: Option<DateRange>,
    /// Search query
    pub query: Option<String>,
    /// Filter by type
    pub filter_type: Option<String>,
    /// Filter by status
    pub filter_status: Option<String>,
    /// Sort by field
    pub sort_by: Option<String>,
    /// Sort direction (asc or desc)
    pub sort_direction: Option<String>,
    /// Page number (1-based)
    pub page: Option<usize>,
    /// Page size
    pub page_size: Option<usize>,
}

impl FilterCriteria {
    /// Create a new filter criteria
    pub fn new() -> Self {
        Self::default()
    }

    /// Set date range
    pub fn with_date_range(mut self, date_range: DateRange) -> Self {
        self.date_range = Some(date_range);
        self
    }

    /// Set search query
    pub fn with_query(mut self, query: &str) -> Self {
        self.query = Some(query.to_string());
        self
    }

    /// Set filter type
    pub fn with_filter_type(mut self, filter_type: &str) -> Self {
        self.filter_type = Some(filter_type.to_string());
        self
    }

    /// Set page
    pub fn with_page(mut self, page: usize) -> Self {
        self.page = Some(page);
        self
    }

    /// Set page size
    pub fn with_page_size(mut self, page_size: usize) -> Self {
        self.page_size = Some(page_size);
        self
    }
}

/// Paginated result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaginatedResult<T> {
    /// List of items
    pub items: Vec<T>,
    /// Total number of items
    pub total: usize,
    /// Current page (1-based)
    pub page: usize,
    /// Page size
    pub page_size: usize,
    /// Total number of pages
    pub total_pages: usize,
}

impl<T> PaginatedResult<T> {
    /// Create a new paginated result
    pub fn new(items: Vec<T>, total: usize, page: usize, page_size: usize) -> Self {
        let total_pages = if page_size == 0 {
            0
        } else {
            (total + page_size - 1) / page_size
        };
        
        Self {
            items,
            total,
            page,
            page_size,
            total_pages,
        }
    }

    /// Check if there are more pages
    pub fn has_more_pages(&self) -> bool {
        self.page < self.total_pages
    }

    /// Check if there is a previous page
    pub fn has_previous_page(&self) -> bool {
        self.page > 1
    }
}

/// Summary statistics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SummaryStatistics {
    /// Count
    pub count: u64,
    /// Sum
    pub sum: rust_decimal::Decimal,
    /// Average
    pub average: rust_decimal::Decimal,
    /// Minimum
    pub min: rust_decimal::Decimal,
    /// Maximum
    pub max: rust_decimal::Decimal,
}

impl SummaryStatistics {
    /// Create new summary statistics
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a value to the statistics
    pub fn add_value(&mut self, value: rust_decimal::Decimal) {
        self.count += 1;
        self.sum += value;
        
        if self.count == 1 {
            self.min = value;
            self.max = value;
            self.average = value;
        } else {
            if value < self.min {
                self.min = value;
            }
            if value > self.max {
                self.max = value;
            }
            self.average = self.sum / rust_decimal::Decimal::from(self.count);
        }
    }

    /// Merge with another summary
    pub fn merge(&mut self, other: &SummaryStatistics) {
        if other.count == 0 {
            return;
        }
        
        if self.count == 0 {
            *self = other.clone();
            return;
        }
        
        self.count += other.count;
        self.sum += other.sum;
        
        if other.min < self.min {
            self.min = other.min;
        }
        if other.max > self.max {
            self.max = other.max;
        }
        
        self.average = self.sum / rust_decimal::Decimal::from(self.count);
    }
}

/// Notification message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    /// Unique identifier
    pub id: Uuid,
    /// Notification type
    pub notification_type: NotificationType,
    /// Title
    pub title: String,
    /// Message
    pub message: String,
    /// Data
    pub data: Option<serde_json::Value>,
    /// Priority
    pub priority: NotificationPriority,
    /// Timestamp
    pub timestamp: DateTime<Utc>,
    /// Whether the notification has been read
    pub is_read: bool,
}

impl Default for Notification {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4(),
            notification_type: NotificationType::default(),
            title: String::new(),
            message: String::new(),
            data: None,
            priority: NotificationPriority::default(),
            timestamp: Utc::now(),
            is_read: false,
        }
    }
}

impl Notification {
    /// Create a new notification
    pub fn new(notification_type: NotificationType, title: &str, message: &str) -> Self {
        Self {
            notification_type,
            title: title.to_string(),
            message: message.to_string(),
            ..Default::default()
        }
    }

    /// Create an info notification
    pub fn info(title: &str, message: &str) -> Self {
        Self::new(NotificationType::Info, title, message)
    }

    /// Create a warning notification
    pub fn warning(title: &str, message: &str) -> Self {
        let mut notification = Self::new(NotificationType::Warning, title, message);
        notification.priority = NotificationPriority::High;
        notification
    }

    /// Create an error notification
    pub fn error(title: &str, message: &str) -> Self {
        let mut notification = Self::new(NotificationType::Error, title, message);
        notification.priority = NotificationPriority::Critical;
        notification
    }

    /// Mark as read
    pub fn mark_as_read(&mut self) {
        self.is_read = true;
    }
}

/// Notification type
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum NotificationType {
    /// Information notification
    Info,
    /// Warning notification
    Warning,
    /// Error notification
    Error,
    /// Success notification
    Success,
    /// Custom notification type
    Custom(String),
}

impl Default for NotificationType {
    fn default() -> Self {
        Self::Info
    }
}

/// Notification priority
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum NotificationPriority {
    /// Low priority
    Low,
    /// Normal priority
    Normal,
    /// High priority
    High,
    /// Critical priority
    Critical,
}

impl Default for NotificationPriority {
    fn default() -> Self {
        Self::Normal
    }
}

/// Settings configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AppSettings {
    /// Application name
    pub app_name: String,
    /// Application version
    pub app_version: String,
    /// Organization name
    pub organization_name: String,
    /// Currency
    pub currency: String,
    /// Date format
    pub date_format: String,
    /// Number format
    pub number_format: String,
    /// Theme
    pub theme: String,
    /// Language
    pub language: String,
    /// Time zone
    pub time_zone: String,
}

impl AppSettings {
    /// Create new app settings
    pub fn new() -> Self {
        Self {
            app_name: "NexusLedger".to_string(),
            app_version: "0.1.0".to_string(),
            organization_name: "RichdaleAI".to_string(),
            currency: "USD".to_string(),
            date_format: "YYYY-MM-DD".to_string(),
            number_format: "en-US".to_string(),
            theme: "light".to_string(),
            language: "en".to_string(),
            time_zone: "UTC".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_date_range() {
        let now = Utc::now();
        let range = DateRange::new(now, now + chrono::Duration::days(1));
        
        assert!(range.contains(now));
        assert!(range.contains(now + chrono::Duration::hours(12)));
        assert!(!range.contains(now - chrono::Duration::days(1)));
        assert!(!range.contains(now + chrono::Duration::days(2)));
    }

    #[test]
    fn test_filter_criteria() {
        let criteria = FilterCriteria::new()
            .with_query("test")
            .with_filter_type("transaction")
            .with_page(1)
            .with_page_size(10);
        
        assert_eq!(criteria.query, Some("test".to_string()));
        assert_eq!(criteria.filter_type, Some("transaction".to_string()));
        assert_eq!(criteria.page, Some(1));
        assert_eq!(criteria.page_size, Some(10));
    }

    #[test]
    fn test_paginated_result() {
        let items: Vec<i32> = (1..=25).collect();
        let result = PaginatedResult::new(items, 100, 1, 10);
        
        assert_eq!(result.items.len(), 25);
        assert_eq!(result.total, 100);
        assert_eq!(result.page, 1);
        assert_eq!(result.page_size, 10);
        assert_eq!(result.total_pages, 10);
        assert!(result.has_more_pages());
        assert!(!result.has_previous_page());
    }

    #[test]
    fn test_summary_statistics() {
        let mut stats = SummaryStatistics::new();
        
        stats.add_value(dec!(10));
        stats.add_value(dec!(20));
        stats.add_value(dec!(30));
        
        assert_eq!(stats.count, 3);
        assert_eq!(stats.sum, dec!(60));
        assert_eq!(stats.average, dec!(20));
        assert_eq!(stats.min, dec!(10));
        assert_eq!(stats.max, dec!(30));
    }

    #[test]
    fn test_notification() {
        let notification = Notification::info("Test", "This is a test notification");
        assert_eq!(notification.notification_type, NotificationType::Info);
        assert_eq!(notification.title, "Test");
        assert_eq!(notification.message, "This is a test notification");
        assert_eq!(notification.priority, NotificationPriority::Normal);
        
        let warning = Notification::warning("Warning", "This is a warning");
        assert_eq!(warning.priority, NotificationPriority::High);
        
        let error = Notification::error("Error", "This is an error");
        assert_eq!(error.priority, NotificationPriority::Critical);
    }

    #[test]
    fn test_app_settings() {
        let settings = AppSettings::new();
        assert_eq!(settings.app_name, "NexusLedger");
        assert_eq!(settings.app_version, "0.1.0");
        assert_eq!(settings.organization_name, "RichdaleAI");
    }
}
