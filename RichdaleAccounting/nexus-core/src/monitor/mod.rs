//! Monitor Module
//!
//! This module contains monitoring and observability functionality for the NexusLedger system.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use uuid::Uuid;
use chrono::{DateTime, Utc};
use tracing::{info, error, debug, warn};
use crate::agents::status::{SystemStatus, AgentStatusInfo};
use crate::agents::orchestrator::AgentOrchestrator;

/// Monitor configuration
#[derive(Debug, Clone)]
pub struct MonitorConfig {
    /// Whether monitoring is enabled
    pub enabled: bool,
    /// Metrics collection interval in seconds
    pub collection_interval: u64,
    /// Metrics retention in hours
    pub retention_hours: u64,
    /// Whether to enable Prometheus metrics
    pub enable_prometheus: bool,
    /// Prometheus metrics port
    pub prometheus_port: u16,
    /// Whether to enable logging
    pub enable_logging: bool,
    /// Log level
    pub log_level: String,
}

impl Default for MonitorConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            collection_interval: 60,
            retention_hours: 24,
            enable_prometheus: false,
            prometheus_port: 9090,
            enable_logging: true,
            log_level: "info".to_string(),
        }
    }
}

impl MonitorConfig {
    /// Create a new monitor configuration
    pub fn new() -> Self {
        Self::default()
    }

    /// Load configuration from environment variables
    pub fn from_env() -> Self {
        Self {
            enabled: std::env::var("MONITOR_ENABLED")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(true),
            collection_interval: std::env::var("MONITOR_COLLECTION_INTERVAL")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(60),
            retention_hours: std::env::var("MONITOR_RETENTION_HOURS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(24),
            enable_prometheus: std::env::var("MONITOR_ENABLE_PROMETHEUS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(false),
            prometheus_port: std::env::var("MONITOR_PROMETHEUS_PORT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(9090),
            enable_logging: std::env::var("MONITOR_ENABLE_LOGGING")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(true),
            log_level: std::env::var("MONITOR_LOG_LEVEL").unwrap_or_else(|_| "info".to_string()),
        }
    }
}

/// System monitor
#[derive(Debug, Clone)]
pub struct SystemMonitor {
    /// Monitor configuration
    pub config: MonitorConfig,
    /// Agent orchestrator
    pub orchestrator: Arc<AgentOrchestrator>,
    /// Metrics storage
    pub metrics: Arc<RwLock<HashMap<String, Metric>>>,
    /// Historical metrics
    pub historical_metrics: Arc<RwLock<Vec<SystemMetrics>>>,
    /// Alerts
    pub alerts: Arc<RwLock<Vec<Alert>>>,
}

impl SystemMonitor {
    /// Create a new system monitor
    pub fn new(config: MonitorConfig, orchestrator: Arc<AgentOrchestrator>) -> Self {
        Self {
            config,
            orchestrator,
            metrics: Arc::new(RwLock::new(HashMap::new())),
            historical_metrics: Arc::new(RwLock::new(Vec::new())),
            alerts: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Initialize the system monitor
    pub async fn initialize(&mut self) -> Result<(), anyhow::Error> {
        info!("Initializing System Monitor...");
        
        if self.config.enabled {
            info!("Monitoring is enabled");
            info!("Collection interval: {} seconds", self.config.collection_interval);
            info!("Retention: {} hours", self.config.retention_hours);
            
            // Start collecting metrics
            self.start_collecting().await?;
        } else {
            info!("Monitoring is disabled");
        }
        
        Ok(())
    }

    /// Start collecting metrics
    pub async fn start_collecting(&self) -> Result<(), anyhow::Error> {
        info!("Starting metrics collection...");
        
        // In a real implementation, this would start a background task
        // that periodically collects metrics
        
        // Collect initial metrics
        self.collect_metrics().await?;
        
        Ok(())
    }

    /// Collect system metrics
    pub async fn collect_metrics(&self) -> Result<(), anyhow::Error> {
        debug!("Collecting system metrics...");
        
        // Get system status from orchestrator
        let system_status = self.orchestrator.get_system_status().await;
        
        // Create system metrics
        let system_metrics = SystemMetrics {
            timestamp: Utc::now(),
            total_agents: system_status.total_agents,
            active_agents: system_status.active_agents,
            idle_agents: system_status.idle_agents,
            busy_agents: system_status.busy_agents,
            error_agents: system_status.error_agents,
            total_tasks_processed: system_status.total_tasks_processed,
            total_tasks_failed: system_status.total_tasks_failed,
            total_tasks_in_progress: system_status.total_tasks_in_progress,
            health_score: system_status.health_score,
        };
        
        // Store historical metrics
        self.historical_metrics.write().await.push(system_metrics);
        
        // Clean up old metrics
        self.cleanup_old_metrics().await?;
        
        // Check for alerts
        self.check_alerts().await?;
        
        debug!("Metrics collected successfully");
        
        Ok(())
    }

    /// Clean up old metrics
    async fn cleanup_old_metrics(&self) -> Result<(), anyhow::Error> {
        let retention_hours = self.config.retention_hours;
        let cutoff = Utc::now() - chrono::Duration::hours(retention_hours as i64);
        
        let mut historical_metrics = self.historical_metrics.write().await;
        historical_metrics.retain(|m| m.timestamp >= cutoff);
        
        Ok(())
    }

    /// Check for alerts
    async fn check_alerts(&self) -> Result<(), anyhow::Error> {
        let system_status = self.orchestrator.get_system_status().await;
        
        // Check for health score alerts
        if system_status.health_score < 0.5 {
            self.create_alert(Alert {
                id: Uuid::new_v4(),
                alert_type: AlertType::Critical,
                title: "System Health Critical".to_string(),
                message: format!("System health score is critically low: {:.2}%", system_status.health_score * 100.0),
                severity: AlertSeverity::Critical,
                timestamp: Utc::now(),
                is_resolved: false,
            }).await?;
        } else if system_status.health_score < 0.8 {
            self.create_alert(Alert {
                id: Uuid::new_v4(),
                alert_type: AlertType::Warning,
                title: "System Health Degraded".to_string(),
                message: format!("System health score is degraded: {:.2}%", system_status.health_score * 100.0),
                severity: AlertSeverity::High,
                timestamp: Utc::now(),
                is_resolved: false,
            }).await?;
        }
        
        // Check for agent errors
        if system_status.error_agents > 0 {
            self.create_alert(Alert {
                id: Uuid::new_v4(),
                alert_type: AlertType::Error,
                title: "Agents in Error State".to_string(),
                message: format!("{} agents are in error state", system_status.error_agents),
                severity: AlertSeverity::High,
                timestamp: Utc::now(),
                is_resolved: false,
            }).await?;
        }
        
        // Check for task failures
        if system_status.total_tasks_failed > 0 {
            let failure_rate = system_status.total_tasks_failed as f64 / 
                (system_status.total_tasks_processed + system_status.total_tasks_failed) as f64;
            
            if failure_rate > 0.1 { // 10% failure rate
                self.create_alert(Alert {
                    id: Uuid::new_v4(),
                    alert_type: AlertType::Warning,
                    title: "High Task Failure Rate".to_string(),
                    message: format!("Task failure rate is high: {:.2}%", failure_rate * 100.0),
                    severity: AlertSeverity::Medium,
                    timestamp: Utc::now(),
                    is_resolved: false,
                }).await?;
            }
        }
        
        Ok(())
    }

    /// Create an alert
    pub async fn create_alert(&self, alert: Alert) -> Result<(), anyhow::Error> {
        warn!("ALERT: {} - {}", alert.title, alert.message);
        
        let mut alerts = self.alerts.write().await;
        alerts.push(alert);
        
        Ok(())
    }

    /// Get all alerts
    pub async fn get_alerts(&self) -> Vec<Alert> {
        self.alerts.read().await.clone()
    }

    /// Get active alerts
    pub async fn get_active_alerts(&self) -> Vec<Alert> {
        self.alerts.read().await.iter()
            .filter(|a| !a.is_resolved)
            .cloned()
            .collect()
    }

    /// Resolve an alert
    pub async fn resolve_alert(&self, alert_id: Uuid) -> Result<(), anyhow::Error> {
        let mut alerts = self.alerts.write().await;
        if let Some(alert) = alerts.iter_mut().find(|a| a.id == alert_id) {
            alert.is_resolved = true;
            info!("Alert resolved: {}", alert.title);
        }
        Ok(())
    }

    /// Get current system metrics
    pub async fn get_current_metrics(&self) -> SystemMetrics {
        let system_status = self.orchestrator.get_system_status().await;
        
        SystemMetrics {
            timestamp: Utc::now(),
            total_agents: system_status.total_agents,
            active_agents: system_status.active_agents,
            idle_agents: system_status.idle_agents,
            busy_agents: system_status.busy_agents,
            error_agents: system_status.error_agents,
            total_tasks_processed: system_status.total_tasks_processed,
            total_tasks_failed: system_status.total_tasks_failed,
            total_tasks_in_progress: system_status.total_tasks_in_progress,
            health_score: system_status.health_score,
        }
    }

    /// Get historical metrics
    pub async fn get_historical_metrics(&self) -> Vec<SystemMetrics> {
        self.historical_metrics.read().await.clone()
    }

    /// Get metrics by time range
    pub async fn get_metrics_by_range(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Vec<SystemMetrics> {
        self.historical_metrics.read().await.iter()
            .filter(|m| m.timestamp >= start && m.timestamp <= end)
            .cloned()
            .collect()
    }

    /// Get a specific metric
    pub async fn get_metric(&self, name: &str) -> Option<Metric> {
        self.metrics.read().await.get(name).cloned()
    }

    /// Set a metric
    pub async fn set_metric(&self, name: String, metric: Metric) {
        self.metrics.write().await.insert(name, metric);
    }

    /// Increment a counter metric
    pub async fn increment_counter(&self, name: &str, value: f64) {
        let mut metrics = self.metrics.write().await;
        let metric = metrics.entry(name.to_string()).or_insert_with(|| Metric {
            name: name.to_string(),
            metric_type: MetricType::Counter,
            value: 0.0,
            labels: HashMap::new(),
            timestamp: Utc::now(),
        });
        
        metric.value += value;
        metric.timestamp = Utc::now();
    }

    /// Set a gauge metric
    pub async fn set_gauge(&self, name: &str, value: f64) {
        let mut metrics = self.metrics.write().await;
        let metric = metrics.entry(name.to_string()).or_insert_with(|| Metric {
            name: name.to_string(),
            metric_type: MetricType::Gauge,
            value: 0.0,
            labels: HashMap::new(),
            timestamp: Utc::now(),
        });
        
        metric.value = value;
        metric.timestamp = Utc::now();
    }

    /// Record a histogram value
    pub async fn record_histogram(&self, name: &str, value: f64) {
        // In a real implementation, this would record a value in a histogram
        // For now, we'll just update a gauge with the latest value
        self.set_gauge(name, value).await;
    }

    /// Get system health
    pub async fn get_system_health(&self) -> SystemHealth {
        let system_status = self.orchestrator.get_system_status().await;
        
        SystemHealth {
            status: if system_status.health_score >= 0.8 {
                HealthStatus::Healthy
            } else if system_status.health_score >= 0.5 {
                HealthStatus::Degraded
            } else {
                HealthStatus::Critical
            },
            score: system_status.health_score,
            checks: vec![
                HealthCheck {
                    name: "Agent Health".to_string(),
                    status: if system_status.error_agents == 0 {
                        HealthStatus::Healthy
                    } else {
                        HealthStatus::Degraded
                    },
                    message: format!("{} agents in error state", system_status.error_agents),
                },
                HealthCheck {
                    name: "Task Processing".to_string(),
                    status: if system_status.total_tasks_failed == 0 {
                        HealthStatus::Healthy
                    } else {
                        HealthStatus::Degraded
                    },
                    message: format!("{} tasks failed", system_status.total_tasks_failed),
                },
            ],
        }
    }
}

/// Metric types
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MetricType {
    /// Counter metric (monotonically increasing)
    Counter,
    /// Gauge metric (can go up and down)
    Gauge,
    /// Histogram metric
    Histogram,
    /// Summary metric
    Summary,
}

/// Metric data
#[derive(Debug, Clone)]
pub struct Metric {
    /// Metric name
    pub name: String,
    /// Metric type
    pub metric_type: MetricType,
    /// Metric value
    pub value: f64,
    /// Metric labels
    pub labels: HashMap<String, String>,
    /// Timestamp
    pub timestamp: DateTime<Utc>,
}

/// System metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemMetrics {
    /// Timestamp
    pub timestamp: DateTime<Utc>,
    /// Total number of agents
    pub total_agents: usize,
    /// Number of active agents
    pub active_agents: usize,
    /// Number of idle agents
    pub idle_agents: usize,
    /// Number of busy agents
    pub busy_agents: usize,
    /// Number of agents in error
    pub error_agents: usize,
    /// Total tasks processed
    pub total_tasks_processed: u64,
    /// Total tasks failed
    pub total_tasks_failed: u64,
    /// Total tasks in progress
    pub total_tasks_in_progress: usize,
    /// Overall health score
    pub health_score: f64,
}

/// Alert types
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum AlertType {
    /// Information alert
    Info,
    /// Warning alert
    Warning,
    /// Error alert
    Error,
    /// Critical alert
    Critical,
}

/// Alert severity
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum AlertSeverity {
    /// Low severity
    Low,
    /// Medium severity
    Medium,
    /// High severity
    High,
    /// Critical severity
    Critical,
}

/// Alert
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alert {
    /// Unique identifier
    pub id: Uuid,
    /// Alert type
    pub alert_type: AlertType,
    /// Alert title
    pub title: String,
    /// Alert message
    pub message: String,
    /// Severity
    pub severity: AlertSeverity,
    /// Timestamp
    pub timestamp: DateTime<Utc>,
    /// Whether the alert has been resolved
    pub is_resolved: bool,
}

/// Health status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum HealthStatus {
    /// Healthy
    Healthy,
    /// Degraded
    Degraded,
    /// Critical
    Critical,
}

/// Health check
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheck {
    /// Check name
    pub name: String,
    /// Check status
    pub status: HealthStatus,
    /// Check message
    pub message: String,
}

/// System health
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemHealth {
    /// Overall status
    pub status: HealthStatus,
    /// Health score (0.0 - 1.0)
    pub score: f64,
    /// Individual health checks
    pub checks: Vec<HealthCheck>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn test_monitor_config_default() {
        let config = MonitorConfig::default();
        assert!(config.enabled);
        assert_eq!(config.collection_interval, 60);
        assert_eq!(config.retention_hours, 24);
    }

    #[tokio::test]
    async fn test_system_monitor_creation() {
        let config = MonitorConfig::default();
        let orchestrator = Arc::new(AgentOrchestrator::new());
        
        let monitor = SystemMonitor::new(config, orchestrator);
        assert!(monitor.config.enabled);
    }

    #[tokio::test]
    async fn test_metrics() {
        let config = MonitorConfig::default();
        let orchestrator = Arc::new(AgentOrchestrator::new());
        
        let monitor = SystemMonitor::new(config, orchestrator);
        
        // Set a gauge
        monitor.set_gauge("test_gauge", 42.0).await;
        
        // Increment a counter
        monitor.increment_counter("test_counter", 1.0).await;
        monitor.increment_counter("test_counter", 2.0).await;
        
        // Get metrics
        let gauge = monitor.get_metric("test_gauge").await;
        assert!(gauge.is_some());
        assert_eq!(gauge.unwrap().value, 42.0);
        
        let counter = monitor.get_metric("test_counter").await;
        assert!(counter.is_some());
        assert_eq!(counter.unwrap().value, 3.0);
    }

    #[tokio::test]
    async fn test_alerts() {
        let config = MonitorConfig::default();
        let orchestrator = Arc::new(AgentOrchestrator::new());
        
        let monitor = SystemMonitor::new(config, orchestrator);
        
        // Create an alert
        let alert = Alert {
            id: Uuid::new_v4(),
            alert_type: AlertType::Warning,
            title: "Test Alert".to_string(),
            message: "This is a test alert".to_string(),
            severity: AlertSeverity::Medium,
            timestamp: Utc::now(),
            is_resolved: false,
        };
        
        monitor.create_alert(alert.clone()).await.unwrap();
        
        // Get alerts
        let alerts = monitor.get_alerts().await;
        assert_eq!(alerts.len(), 1);
        
        // Get active alerts
        let active_alerts = monitor.get_active_alerts().await;
        assert_eq!(active_alerts.len(), 1);
        
        // Resolve the alert
        monitor.resolve_alert(alert.id).await.unwrap();
        
        let active_alerts = monitor.get_active_alerts().await;
        assert_eq!(active_alerts.len(), 0);
    }

    #[tokio::test]
    async fn test_system_health() {
        let config = MonitorConfig::default();
        let orchestrator = Arc::new(AgentOrchestrator::new());
        
        let monitor = SystemMonitor::new(config, orchestrator);
        
        let health = monitor.get_system_health().await;
        assert!(health.score >= 0.0);
        assert!(health.score <= 1.0);
        assert!(!health.checks.is_empty());
    }
}
