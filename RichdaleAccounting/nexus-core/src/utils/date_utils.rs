//! Date Utilities Module
//!
//! Provides utility functions for working with dates and times.

use chrono::{DateTime, Utc, NaiveDate, Datelike, Duration};
use std::fmt;

/// Date range for filtering and reporting
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateRange {
    /// Start date
    pub start: NaiveDate,
    /// End date
    pub end: NaiveDate,
}

impl DateRange {
    /// Create a new date range
    pub fn new(start: NaiveDate, end: NaiveDate) -> Self {
        Self { start, end }
    }

    /// Create a date range for today
    pub fn today() -> Self {
        let today = Utc::now().date_naive();
        Self { start: today, end: today }
    }

    /// Create a date range for the current week
    pub fn current_week() -> Self {
        let today = Utc::now().date_naive();
        let start = today - Duration::days(today.weekday().num_days_from_sunday() as i64);
        let end = start + Duration::days(6);
        Self { start, end }
    }

    /// Create a date range for the current month
    pub fn current_month() -> Self {
        let today = Utc::now().date_naive();
        let start = today.with_day(1).unwrap();
        let end = today.with_day(today.day()).unwrap();
        Self { start, end }
    }

    /// Create a date range for the current year
    pub fn current_year() -> Self {
        let today = Utc::now().date_naive();
        let start = today.with_month(1).unwrap().with_day(1).unwrap();
        let end = today.with_month(12).unwrap().with_day(31).unwrap();
        Self { start, end }
    }

    /// Create a date range for the previous period
    pub fn previous(&self) -> Self {
        let duration = self.end - self.start;
        let start = self.start - duration;
        let end = self.end - duration;
        Self { start, end }
    }

    /// Check if a date is within the range
    pub fn contains(&self, date: NaiveDate) -> bool {
        date >= self.start && date <= self.end
    }

    /// Check if a datetime is within the range
    pub fn contains_datetime(&self, datetime: DateTime<Utc>) -> bool {
        let date = datetime.date_naive();
        date >= self.start && date <= self.end
    }

    /// Get the number of days in the range
    pub fn days(&self) -> i64 {
        (self.end - self.start).num_days() + 1
    }

    /// Get the start as a datetime at midnight
    pub fn start_datetime(&self) -> DateTime<Utc> {
        self.start.and_hms_opt(0, 0, 0).unwrap()
    }

    /// Get the end as a datetime at midnight
    pub fn end_datetime(&self) -> DateTime<Utc> {
        self.end.and_hms_opt(23, 59, 59).unwrap()
    }
}

impl fmt::Display for DateRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?} to {:?}", self.start, self.end)
    }
}

/// Date error types
#[derive(Debug, thiserror::Error)]
pub enum DateError {
    /// Invalid date format
    #[error("Invalid date format: {0}")]
    InvalidFormat(String),
    
    /// Date out of range
    #[error("Date out of range: {0}")]
    OutOfRange(String),
    
    /// Invalid date range
    #[error("Invalid date range: {0}")]
    InvalidRange(String),
    
    /// Parse error
    #[error("Parse error: {0}")]
    ParseError(String),
}

impl DateError {
    /// Create a new invalid format error
    pub fn invalid_format(msg: &str) -> Self {
        Self::InvalidFormat(msg.to_string())
    }

    /// Create a new out of range error
    pub fn out_of_range(msg: &str) -> Self {
        Self::OutOfRange(msg.to_string())
    }

    /// Create a new invalid range error
    pub fn invalid_range(msg: &str) -> Self {
        Self::InvalidRange(msg.to_string())
    }
}

/// Date utilities
pub struct DateUtils;

impl DateUtils {
    /// Parse a date string in various formats
    pub fn parse_date(date_str: &str) -> Result<NaiveDate, DateError> {
        // Try ISO 8601 format first (YYYY-MM-DD)
        if let Ok(date) = NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
            return Ok(date);
        }
        
        // Try US format (MM/DD/YYYY)
        if let Ok(date) = NaiveDate::parse_from_str(date_str, "%m/%d/%Y") {
            return Ok(date);
        }
        
        // Try European format (DD/MM/YYYY)
        if let Ok(date) = NaiveDate::parse_from_str(date_str, "%d/%m/%Y") {
            return Ok(date);
        }
        
        // Try RFC 3339 format
        if let Ok(datetime) = DateTime::parse_from_rfc3339(date_str) {
            return Ok(datetime.date_naive());
        }
        
        Err(DateError::invalid_format(date_str))
    }

    /// Format a date in ISO 8601 format
    pub fn format_iso(date: NaiveDate) -> String {
        date.format("%Y-%m-%d").to_string()
    }

    /// Format a date in US format
    pub fn format_us(date: NaiveDate) -> String {
        date.format("%m/%d/%Y").to_string()
    }

    /// Format a date in European format
    pub fn format_european(date: NaiveDate) -> String {
        date.format("%d/%m/%Y").to_string()
    }

    /// Format a date in a readable format
    pub fn format_readable(date: NaiveDate) -> String {
        date.format("%B %d, %Y").to_string()
    }

    /// Get the current date
    pub fn today() -> NaiveDate {
        Utc::now().date_naive()
    }

    /// Get the current datetime
    pub fn now() -> DateTime<Utc> {
        Utc::now()
    }

    /// Get the first day of the current month
    pub fn first_day_of_month() -> NaiveDate {
        let today = Self::today();
        today.with_day(1).unwrap()
    }

    /// Get the last day of the current month
    pub fn last_day_of_month() -> NaiveDate {
        let today = Self::today();
        let next_month = today.with_month(today.month() + 1).unwrap_or(today.with_year(today.year() + 1).unwrap().with_month(1).unwrap());
        next_month.with_day(1).unwrap() - Duration::days(1)
    }

    /// Get the first day of the current year
    pub fn first_day_of_year() -> NaiveDate {
        let today = Self::today();
        today.with_month(1).unwrap().with_day(1).unwrap()
    }

    /// Get the last day of the current year
    pub fn last_day_of_year() -> NaiveDate {
        let today = Self::today();
        today.with_month(12).unwrap().with_day(31).unwrap()
    }

    /// Get the current quarter
    pub fn current_quarter() -> u32 {
        let today = Self::today();
        (today.month() - 1) / 3 + 1
    }

    /// Get the first day of the current quarter
    pub fn first_day_of_quarter() -> NaiveDate {
        let today = Self::today();
        let quarter = Self::current_quarter();
        let month = (quarter - 1) * 3 + 1;
        today.with_month(month).unwrap().with_day(1).unwrap()
    }

    /// Get the last day of the current quarter
    pub fn last_day_of_quarter() -> NaiveDate {
        let today = Self::today();
        let quarter = Self::current_quarter();
        let month = quarter * 3;
        today.with_month(month).unwrap().with_day(1).unwrap() - Duration::days(1)
    }

    /// Get the number of days between two dates
    pub fn days_between(start: NaiveDate, end: NaiveDate) -> i64 {
        (end - start).num_days()
    }

    /// Add days to a date
    pub fn add_days(date: NaiveDate, days: i64) -> NaiveDate {
        date + Duration::days(days)
    }

    /// Subtract days from a date
    pub fn subtract_days(date: NaiveDate, days: i64) -> NaiveDate {
        date - Duration::days(days)
    }

    /// Add months to a date
    pub fn add_months(date: NaiveDate, months: i64) -> NaiveDate {
        let mut result = date;
        let mut remaining_months = months;
        
        while remaining_months != 0 {
            let new_month = result.month() as i64 + remaining_months;
            let new_year = result.year() + (new_month - 1) / 12;
            let new_month = ((new_month - 1) % 12 + 1) as u32;
            
            result = result.with_year(new_year).unwrap().with_month(new_month).unwrap();
            
            // If the day doesn't exist in the new month, use the last day of the month
            if result.day() > result.with_day(1).unwrap().with_month(new_month + 1).unwrap_or(result.with_year(new_year + 1).unwrap().with_month(1).unwrap()).pred_opt().unwrap_or(result).day() {
                result = result.with_day(1).unwrap().with_month(new_month + 1).unwrap_or(result.with_year(new_year + 1).unwrap().with_month(1).unwrap()).pred_opt().unwrap_or(result);
            }
            
            remaining_months = 0;
        }
        
        result
    }

    /// Subtract months from a date
    pub fn subtract_months(date: NaiveDate, months: i64) -> NaiveDate {
        Self::add_months(date, -months)
    }

    /// Add years to a date
    pub fn add_years(date: NaiveDate, years: i32) -> NaiveDate {
        date.with_year(date.year() + years).unwrap_or(date)
    }

    /// Subtract years from a date
    pub fn subtract_years(date: NaiveDate, years: i32) -> NaiveDate {
        Self::add_years(date, -years)
    }

    /// Get the day of the week name
    pub fn day_of_week_name(date: NaiveDate) -> String {
        match date.weekday() {
            chrono::Weekday::Mon => "Monday".to_string(),
            chrono::Weekday::Tue => "Tuesday".to_string(),
            chrono::Weekday::Wed => "Wednesday".to_string(),
            chrono::Weekday::Thu => "Thursday".to_string(),
            chrono::Weekday::Fri => "Friday".to_string(),
            chrono::Weekday::Sat => "Saturday".to_string(),
            chrono::Weekday::Sun => "Sunday".to_string(),
        }
    }

    /// Get the month name
    pub fn month_name(date: NaiveDate) -> String {
        match date.month() {
            1 => "January".to_string(),
            2 => "February".to_string(),
            3 => "March".to_string(),
            4 => "April".to_string(),
            5 => "May".to_string(),
            6 => "June".to_string(),
            7 => "July".to_string(),
            8 => "August".to_string(),
            9 => "September".to_string(),
            10 => "October".to_string(),
            11 => "November".to_string(),
            12 => "December".to_string(),
            _ => "Unknown".to_string(),
        }
    }

    /// Check if a year is a leap year
    pub fn is_leap_year(year: i32) -> bool {
        (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
    }

    /// Get the number of days in a month
    pub fn days_in_month(year: i32, month: u32) -> u32 {
        match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 => {
                if Self::is_leap_year(year) {
                    29
                } else {
                    28
                }
            }
            _ => 0,
        }
    }

    /// Validate a date
    pub fn validate_date(year: i32, month: u32, day: u32) -> Result<NaiveDate, DateError> {
        if month < 1 || month > 12 {
            return Err(DateError::out_of_range(format!("Month must be between 1 and 12, got {}", month)));
        }
        
        let max_days = Self::days_in_month(year, month);
        if day < 1 || day > max_days {
            return Err(DateError::out_of_range(format!("Day must be between 1 and {}, got {}", max_days, day)));
        }
        
        NaiveDate::from_ymd_opt(year, month, day)
            .ok_or_else(|| DateError::invalid_range(format!("Invalid date: {}-{}-{}", year, month, day)))
    }

    /// Get the age in years between two dates
    pub fn age_in_years(birth_date: NaiveDate, reference_date: NaiveDate) -> u32 {
        let mut age = reference_date.year() - birth_date.year();
        
        // Check if birthday has occurred this year
        let birth_month_day = (birth_date.month(), birth_date.day());
        let reference_month_day = (reference_date.month(), reference_date.day());
        
        if reference_month_day < birth_month_day {
            age -= 1;
        }
        
        age as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_date_range() {
        let start = NaiveDate::from_ymd_opt(2023, 1, 1).unwrap();
        let end = NaiveDate::from_ymd_opt(2023, 1, 31).unwrap();
        let range = DateRange::new(start, end);
        
        assert!(range.contains(NaiveDate::from_ymd_opt(2023, 1, 15).unwrap()));
        assert!(range.contains(NaiveDate::from_ymd_opt(2023, 1, 1).unwrap()));
        assert!(range.contains(NaiveDate::from_ymd_opt(2023, 1, 31).unwrap()));
        assert!(!range.contains(NaiveDate::from_ymd_opt(2022, 12, 31).unwrap()));
        assert!(!range.contains(NaiveDate::from_ymd_opt(2023, 2, 1).unwrap()));
    }

    #[test]
    fn test_date_range_today() {
        let range = DateRange::today();
        let today = Utc::now().date_naive();
        
        assert_eq!(range.start, today);
        assert_eq!(range.end, today);
    }

    #[test]
    fn test_date_range_days() {
        let start = NaiveDate::from_ymd_opt(2023, 1, 1).unwrap();
        let end = NaiveDate::from_ymd_opt(2023, 1, 31).unwrap();
        let range = DateRange::new(start, end);
        
        assert_eq!(range.days(), 31);
    }

    #[test]
    fn test_parse_date() {
        // ISO format
        let date = DateUtils::parse_date("2023-01-15").unwrap();
        assert_eq!(date.year(), 2023);
        assert_eq!(date.month(), 1);
        assert_eq!(date.day(), 15);
        
        // US format
        let date = DateUtils::parse_date("01/15/2023").unwrap();
        assert_eq!(date.year(), 2023);
        assert_eq!(date.month(), 1);
        assert_eq!(date.day(), 15);
        
        // European format
        let date = DateUtils::parse_date("15/01/2023").unwrap();
        assert_eq!(date.year(), 2023);
        assert_eq!(date.month(), 1);
        assert_eq!(date.day(), 15);
    }

    #[test]
    fn test_format_date() {
        let date = NaiveDate::from_ymd_opt(2023, 1, 15).unwrap();
        
        assert_eq!(DateUtils::format_iso(date), "2023-01-15");
        assert_eq!(DateUtils::format_us(date), "01/15/2023");
        assert_eq!(DateUtils::format_european(date), "15/01/2023");
        assert_eq!(DateUtils::format_readable(date), "January 15, 2023");
    }

    #[test]
    fn test_date_arithmetic() {
        let date = NaiveDate::from_ymd_opt(2023, 1, 15).unwrap();
        
        assert_eq!(DateUtils::add_days(date, 5), NaiveDate::from_ymd_opt(2023, 1, 20).unwrap());
        assert_eq!(DateUtils::subtract_days(date, 5), NaiveDate::from_ymd_opt(2023, 1, 10).unwrap());
        
        assert_eq!(DateUtils::add_months(date, 1), NaiveDate::from_ymd_opt(2023, 2, 15).unwrap());
        assert_eq!(DateUtils::subtract_months(date, 1), NaiveDate::from_ymd_opt(2022, 12, 15).unwrap());
        
        assert_eq!(DateUtils::add_years(date, 1), NaiveDate::from_ymd_opt(2024, 1, 15).unwrap());
        assert_eq!(DateUtils::subtract_years(date, 1), NaiveDate::from_ymd_opt(2022, 1, 15).unwrap());
    }

    #[test]
    fn test_days_between() {
        let start = NaiveDate::from_ymd_opt(2023, 1, 1).unwrap();
        let end = NaiveDate::from_ymd_opt(2023, 1, 31).unwrap();
        
        assert_eq!(DateUtils::days_between(start, end), 30);
    }

    #[test]
    fn test_leap_year() {
        assert!(!DateUtils::is_leap_year(2023));
        assert!(DateUtils::is_leap_year(2024));
        assert!(!DateUtils::is_leap_year(1900));
        assert!(DateUtils::is_leap_year(2000));
    }

    #[test]
    fn test_days_in_month() {
        assert_eq!(DateUtils::days_in_month(2023, 1), 31);
        assert_eq!(DateUtils::days_in_month(2023, 2), 28);
        assert_eq!(DateUtils::days_in_month(2024, 2), 29);
        assert_eq!(DateUtils::days_in_month(2023, 4), 30);
    }

    #[test]
    fn test_validate_date() {
        assert!(DateUtils::validate_date(2023, 1, 15).is_ok());
        assert!(DateUtils::validate_date(2023, 2, 29).is_err()); // Not a leap year
        assert!(DateUtils::validate_date(2024, 2, 29).is_ok()); // Leap year
        assert!(DateUtils::validate_date(2023, 13, 1).is_err()); // Invalid month
        assert!(DateUtils::validate_date(2023, 1, 32).is_err()); // Invalid day
    }

    #[test]
    fn test_age_in_years() {
        let birth_date = NaiveDate::from_ymd_opt(1990, 6, 15).unwrap();
        let reference_date = NaiveDate::from_ymd_opt(2023, 6, 14).unwrap();
        
        assert_eq!(DateUtils::age_in_years(birth_date, reference_date), 32);
        
        let reference_date = NaiveDate::from_ymd_opt(2023, 6, 15).unwrap();
        assert_eq!(DateUtils::age_in_years(birth_date, reference_date), 33);
    }
}
