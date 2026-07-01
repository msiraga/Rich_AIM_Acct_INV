//! Monitor Module
//!
//! This module contains monitoring and observability functionality for the NexusLedger system.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use uuid::Uuid;
use chrono::{DateTime, Utc};
use tracing::{info, error, debug, warn};
use serde::{Serialize, Deserialize};
use prometheus::{
    Encoder, Gauge, Histogram, HistogramOpts, IntCounter, IntCounterVec,
    Opts, Registry, TextEncoder,
};
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
    /// Optional Prometheus metrics exporter
    pub prometheus: Option<Arc<PrometheusExporter>>,
}

impl SystemMonitor {
    /// Create a new system monitor
    pub fn new(config: MonitorConfig, orchestrator: Arc<AgentOrchestrator>) -> Self {
        let prometheus = if config.enable_prometheus {
            Some(Arc::new(PrometheusExporter::new()))
        } else {
            None
        };
        Self {
            config,
            orchestrator,
            metrics: Arc::new(RwLock::new(HashMap::new())),
            historical_metrics: Arc::new(RwLock::new(Vec::new())),
            alerts: Arc::new(RwLock::new(Vec::new())),
            prometheus,
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

        // Update Prometheus metrics if enabled
        if let Some(ref exporter) = self.prometheus {
            exporter.update_from_system_status(&system_status);
            debug!("Prometheus metrics updated from system status");
        }

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

    /// Render metrics in Prometheus text exposition format.
    ///
    /// If Prometheus export is enabled, syncs agent gauges from the current
    /// system status and returns all registered metrics as a
    /// Prometheus-compatible string. Returns an empty string if Prometheus
    /// export is disabled.
    pub async fn render_prometheus_metrics(&self) -> String {
        if let Some(ref exporter) = self.prometheus {
            // Sync gauges from current system status before rendering
            let status = self.orchestrator.get_system_status().await;
            exporter.update_from_system_status(&status);
            exporter.export_metrics()
        } else {
            String::new()
        }
    }

    /// Record an HTTP request in Prometheus metrics.
    ///
    /// Increments the request counter with the given `method`, `path`, and
    /// `status` labels, observes the request duration in the histogram, and
    /// increments the error counter if the status code is 4xx or 5xx.
    pub fn record_request(&self, method: &str, path: &str, status: u16, duration: std::time::Duration) {
        if let Some(ref exporter) = self.prometheus {
            exporter.record_request(method, path, status, duration);
        }
    }

    /// Record a completed task in Prometheus metrics.
    ///
    /// Increments the `nexus_tasks_total` counter with `status="completed"`.
    pub fn record_task_completed(&self) {
        if let Some(ref exporter) = self.prometheus {
            exporter.record_task_completed();
        }
    }

    /// Record a failed task in Prometheus metrics.
    ///
    /// Increments the `nexus_tasks_total` counter with `status="failed"`.
    pub fn record_task_failed(&self) {
        if let Some(ref exporter) = self.prometheus {
            exporter.record_task_failed();
        }
    }

    /// Get a reference to the Prometheus exporter, if enabled.
    pub fn prometheus_exporter(&self) -> Option<&PrometheusExporter> {
        self.prometheus.as_deref()
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

// ---------------------------------------------------------------------------
// Prometheus Metrics Exporter
// ---------------------------------------------------------------------------

/// Prometheus metrics exporter for the NexusLedger system.
///
/// Registers and exposes Prometheus-compatible metrics including agent
/// counts, task counters, request counters, and request duration histograms.
///
/// Uses an explicit [`Registry`] rather than the default global registry so
/// that each instance (including test instances) owns its own metric set and
/// does not interfere with other exporters.
///
/// # Registered Metrics
///
/// | Name                                | Type      | Labels                          |
/// |-------------------------------------|-----------|---------------------------------|
/// | `nexus_agents_active`               | Gauge     | —                               |
/// | `nexus_agents_idle`                 | Gauge     | —                               |
/// | `nexus_agents_busy`                 | Gauge     | —                               |
/// | `nexus_agents_error`                | Gauge     | —                               |
/// | `nexus_tasks_total`                 | Counter   | `status` (completed / failed)  |
/// | `nexus_requests_total`              | Counter   | `method`, `path`, `status`     |
/// | `nexus_errors_total`                | Counter   | —                               |
/// | `nexus_request_duration_seconds`    | Histogram | —                               |
#[derive(Debug)]
pub struct PrometheusExporter {
    /// Private registry that owns all registered metrics.
    registry: Registry,
    // --- Agent gauges (no labels) ---
    agents_active: Gauge,
    agents_idle: Gauge,
    agents_busy: Gauge,
    agents_error: Gauge,
    // --- Counters ---
    /// Total tasks by status (`completed` / `failed`).
    tasks_total: IntCounterVec,
    /// Total HTTP requests by method, path, and status code.
    requests_total: IntCounterVec,
    /// Total errors encountered.
    errors_total: IntCounter,
    // --- Histograms ---
    /// HTTP request latency distribution.
    request_duration: Histogram,
}

impl Default for PrometheusExporter {
    fn default() -> Self {
        Self::new()
    }
}

impl PrometheusExporter {
    /// Create a new `PrometheusExporter` with all metrics pre-registered.
    ///
    /// Every metric is constructed via `with_opts` (no default-registry side
    /// effect) and then explicitly registered on the exporter's private
    /// [`Registry`].
    pub fn new() -> Self {
        let registry = Registry::new();

        // -- Agent gauges --
        let agents_active = Gauge::with_opts(
            Opts::new("nexus_agents_active", "Number of active agents")
        ).expect("failed to create nexus_agents_active gauge");
        let agents_idle = Gauge::with_opts(
            Opts::new("nexus_agents_idle", "Number of idle agents")
        ).expect("failed to create nexus_agents_idle gauge");
        let agents_busy = Gauge::with_opts(
            Opts::new("nexus_agents_busy", "Number of busy agents")
        ).expect("failed to create nexus_agents_busy gauge");
        let agents_error = Gauge::with_opts(
            Opts::new("nexus_agents_error", "Number of agents in error state")
        ).expect("failed to create nexus_agents_error gauge");

        registry.register(Box::new(agents_active.clone()))
            .expect("failed to register nexus_agents_active");
        registry.register(Box::new(agents_idle.clone()))
            .expect("failed to register nexus_agents_idle");
        registry.register(Box::new(agents_busy.clone()))
            .expect("failed to register nexus_agents_busy");
        registry.register(Box::new(agents_error.clone()))
            .expect("failed to register nexus_agents_error");

        // -- Counters --
        let tasks_total = IntCounterVec::new(
            Opts::new("nexus_tasks_total", "Total number of tasks processed"),
            &["status"],
        ).expect("failed to create nexus_tasks_total counter vec");
        registry.register(Box::new(tasks_total.clone()))
            .expect("failed to register nexus_tasks_total");
        // Initialize label values so they appear in metrics output even before first increment
        tasks_total.with_label_values(&["completed"]).inc_by(0);
        tasks_total.with_label_values(&["failed"]).inc_by(0);

        let requests_total = IntCounterVec::new(
            Opts::new("nexus_requests_total", "Total number of HTTP requests"),
            &["method", "path", "status"],
        ).expect("failed to create nexus_requests_total counter vec");
        registry.register(Box::new(requests_total.clone()))
            .expect("failed to register nexus_requests_total");
        requests_total.with_label_values(&["GET", "/", "200"]).inc_by(0);

        let errors_total = IntCounter::with_opts(
            Opts::new("nexus_errors_total", "Total number of errors")
        ).expect("failed to create nexus_errors_total counter");
        registry.register(Box::new(errors_total.clone()))
            .expect("failed to register nexus_errors_total");

        // -- Histogram --
        let request_duration = Histogram::with_opts(
            HistogramOpts::new(
                "nexus_request_duration_seconds",
                "HTTP request duration in seconds",
            )
            .buckets(vec![0.01, 0.05, 0.1, 0.5, 1.0, 5.0, 10.0]),
        ).expect("failed to create nexus_request_duration_seconds histogram");
        registry.register(Box::new(request_duration.clone()))
            .expect("failed to register nexus_request_duration_seconds");

        Self {
            registry,
            agents_active,
            agents_idle,
            agents_busy,
            agents_error,
            tasks_total,
            requests_total,
            errors_total,
            request_duration,
        }
    }

    /// Export all registered metrics in Prometheus text exposition format.
    ///
    /// Uses [`TextEncoder`] to produce a string suitable for scraping via an
    /// HTTP `/metrics` endpoint. Returns an empty string if encoding fails.
    pub fn export_metrics(&self) -> String {
        let metric_families = self.registry.gather();
        let encoder = TextEncoder::new();
        let mut buffer = Vec::new();
        if encoder.encode(&metric_families, &mut buffer).is_err() {
            warn!("Failed to encode Prometheus metrics");
            return String::new();
        }
        String::from_utf8_lossy(&buffer).into_owned()
    }

    /// Update agent gauges from the orchestrator's system status.
    ///
    /// Sets the four agent gauges (`active`, `idle`, `busy`, `error`) to the
    /// values reported by [`SystemStatus`]. This method is idempotent —
    /// gauges can be set repeatedly without side effects.
    pub fn update_from_system_status(&self, status: &SystemStatus) {
        self.agents_active.set(status.active_agents as f64);
        self.agents_idle.set(status.idle_agents as f64);
        self.agents_busy.set(status.busy_agents as f64);
        self.agents_error.set(status.error_agents as f64);
    }

    /// Record an HTTP request.
    ///
    /// Increments the `nexus_requests_total` counter with the given `method`,
    /// `path`, and `status` labels, observes the request duration in the
    /// `nexus_request_duration_seconds` histogram, and increments the
    /// `nexus_errors_total` counter when the status code is 4xx or 5xx.
    pub fn record_request(&self, method: &str, path: &str, status: u16, duration: std::time::Duration) {
        let status_str = status.to_string();

        // Increment request counter
        self.requests_total
            .with_label_values(&[method, path, status_str.as_str()])
            .inc();

        // Observe request duration in seconds
        self.request_duration.observe(duration.as_secs_f64());

        // Increment error counter for client/server error responses
        if status >= 400 {
            self.errors_total.inc();
        }
    }

    /// Record a completed task.
    ///
    /// Increments the `nexus_tasks_total` counter with `status="completed"`.
    pub fn record_task_completed(&self) {
        self.tasks_total.with_label_values(&["completed"]).inc();
    }

    /// Record a failed task.
    ///
    /// Increments the `nexus_tasks_total` counter with `status="failed"`.
    pub fn record_task_failed(&self) {
        self.tasks_total.with_label_values(&["failed"]).inc();
    }
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

    // -- Prometheus exporter tests ------------------------------------------

    #[test]
    fn test_prometheus_exporter_creation() {
        let exporter = PrometheusExporter::new();
        let output = exporter.export_metrics();

        // All registered metric names should appear in the text output
        assert!(output.contains("nexus_agents_active"), "missing nexus_agents_active");
        assert!(output.contains("nexus_agents_idle"), "missing nexus_agents_idle");
        assert!(output.contains("nexus_agents_busy"), "missing nexus_agents_busy");
        assert!(output.contains("nexus_agents_error"), "missing nexus_agents_error");
        assert!(output.contains("nexus_tasks_total"), "missing nexus_tasks_total");
        assert!(output.contains("nexus_requests_total"), "missing nexus_requests_total");
        assert!(output.contains("nexus_errors_total"), "missing nexus_errors_total");
        assert!(output.contains("nexus_request_duration_seconds"), "missing nexus_request_duration_seconds");

        // Verify TYPE declarations
        assert!(output.contains("# TYPE nexus_agents_active gauge"));
        assert!(output.contains("# TYPE nexus_tasks_total counter"));
        assert!(output.contains("# TYPE nexus_request_duration_seconds histogram"));
    }

    #[test]
    fn test_prometheus_exporter_default() {
        let exporter = PrometheusExporter::default();
        let output = exporter.export_metrics();
        assert!(!output.is_empty());
    }

    #[test]
    fn test_prometheus_update_from_system_status() {
        let exporter = PrometheusExporter::new();

        let mut status = SystemStatus::default();
        status.active_agents = 5;
        status.idle_agents = 3;
        status.busy_agents = 2;
        status.error_agents = 1;

        exporter.update_from_system_status(&status);

        let output = exporter.export_metrics();

        // Gauges should reflect the values we set
        assert!(output.contains("nexus_agents_active 5"), "active gauge mismatch");
        assert!(output.contains("nexus_agents_idle 3"), "idle gauge mismatch");
        assert!(output.contains("nexus_agents_busy 2"), "busy gauge mismatch");
        assert!(output.contains("nexus_agents_error 1"), "error gauge mismatch");
    }

    #[test]
    fn test_prometheus_update_is_idempotent() {
        let exporter = PrometheusExporter::new();

        let mut status = SystemStatus::default();
        status.active_agents = 10;
        status.idle_agents = 5;

        // Calling twice should not double the value (gauges, not counters)
        exporter.update_from_system_status(&status);
        exporter.update_from_system_status(&status);

        let output = exporter.export_metrics();
        assert!(output.contains("nexus_agents_active 10"), "gauge should still be 10 after second call");
        assert!(output.contains("nexus_agents_idle 5"), "gauge should still be 5 after second call");
    }

    #[test]
    fn test_prometheus_record_request() {
        let exporter = PrometheusExporter::new();

        // Record three requests with different statuses and durations
        exporter.record_request("GET", "/api/transactions", 200, std::time::Duration::from_millis(50));
        exporter.record_request("POST", "/api/transactions", 201, std::time::Duration::from_millis(100));
        exporter.record_request("GET", "/api/transactions", 500, std::time::Duration::from_millis(2000));

        let output = exporter.export_metrics();

        // Request counter should contain the label values
        assert!(output.contains("nexus_requests_total"), "missing nexus_requests_total");
        assert!(output.contains("method=\"GET\""), "missing GET method label");
        assert!(output.contains("method=\"POST\""), "missing POST method label");
        assert!(output.contains("path=\"/api/transactions\""), "missing path label");
        assert!(output.contains("status=\"200\""), "missing status 200 label");
        assert!(output.contains("status=\"500\""), "missing status 500 label");

        // Error counter should be 1 (only the 500 response)
        assert!(output.contains("nexus_errors_total 1"), "error counter should be 1");

        // Histogram should have observed 3 values
        assert!(output.contains("nexus_request_duration_seconds_count 3"), "histogram count should be 3");

        // Histogram buckets should be present
        assert!(output.contains("nexus_request_duration_seconds_bucket"), "missing histogram buckets");
    }

    #[test]
    fn test_prometheus_record_request_no_error_on_success() {
        let exporter = PrometheusExporter::new();

        exporter.record_request("GET", "/api/health", 200, std::time::Duration::from_millis(5));
        exporter.record_request("GET", "/api/health", 204, std::time::Duration::from_millis(3));

        let output = exporter.export_metrics();
        // No 4xx/5xx responses — error counter should remain 0
        assert!(output.contains("nexus_errors_total 0"), "error counter should be 0 for successful requests");
    }

    #[test]
    fn test_prometheus_record_task_completed() {
        let exporter = PrometheusExporter::new();

        exporter.record_task_completed();
        exporter.record_task_completed();
        exporter.record_task_completed();

        let output = exporter.export_metrics();

        // The completed counter should be 3
        assert!(output.contains("nexus_tasks_total"), "missing nexus_tasks_total");
        assert!(output.contains("status=\"completed\""), "missing completed label");
        // Find the line with completed label and verify value is 3
        let completed_line = output.lines()
            .find(|line| line.contains("nexus_tasks_total") && line.contains("completed") && !line.starts_with('#'))
            .expect("should find completed task counter line");
        assert!(completed_line.contains(" 3"), "completed counter should be 3, got: {}", completed_line);
    }

    #[test]
    fn test_prometheus_record_task_failed() {
        let exporter = PrometheusExporter::new();

        exporter.record_task_failed();
        exporter.record_task_failed();

        let output = exporter.export_metrics();

        assert!(output.contains("status=\"failed\""), "missing failed label");
        let failed_line = output.lines()
            .find(|line| line.contains("nexus_tasks_total") && line.contains("failed") && !line.starts_with('#'))
            .expect("should find failed task counter line");
        assert!(failed_line.contains(" 2"), "failed counter should be 2, got: {}", failed_line);
    }

    #[test]
    fn test_prometheus_histogram_buckets() {
        let exporter = PrometheusExporter::new();

        // 5ms  → bucket le="0.01"
        // 40ms → bucket le="0.05"
        // 80ms → bucket le="0.1"
        // 300ms→ bucket le="0.5"
        // 750ms→ bucket le="1"
        // 3s   → bucket le="5"
        // 8s   → bucket le="10"
        exporter.record_request("GET", "/test", 200, std::time::Duration::from_millis(5));
        exporter.record_request("GET", "/test", 200, std::time::Duration::from_millis(40));
        exporter.record_request("GET", "/test", 200, std::time::Duration::from_millis(80));
        exporter.record_request("GET", "/test", 200, std::time::Duration::from_millis(300));
        exporter.record_request("GET", "/test", 200, std::time::Duration::from_millis(750));
        exporter.record_request("GET", "/test", 200, std::time::Duration::from_secs(3));
        exporter.record_request("GET", "/test", 200, std::time::Duration::from_secs(8));

        let output = exporter.export_metrics();

        // Cumulative histogram: each bucket includes all values ≤ its upper bound
        // le="0.01" → 1 (5ms)
        assert!(output.contains("le=\"0.01\"} 1"), "bucket le=0.01 should have 1");
        // le="0.05" → 2 (5ms + 40ms)
        assert!(output.contains("le=\"0.05\"} 2"), "bucket le=0.05 should have 2");
        // le="0.1"  → 3
        assert!(output.contains("le=\"0.1\"} 3"), "bucket le=0.1 should have 3");
        // le="0.5"  → 4
        assert!(output.contains("le=\"0.5\"} 4"), "bucket le=0.5 should have 4");
        // le="1"    → 5
        assert!(output.contains("le=\"1\"} 5"), "bucket le=1 should have 5");
        // le="5"    → 6
        assert!(output.contains("le=\"5\"} 6"), "bucket le=5 should have 6");
        // le="10"   → 7
        assert!(output.contains("le=\"10\"} 7"), "bucket le=10 should have 7");
        // le="+Inf" → 7
        assert!(output.contains("le=\"+Inf\"} 7"), "bucket le=+Inf should have 7");
        // count → 7
        assert!(output.contains("nexus_request_duration_seconds_count 7"), "count should be 7");
    }

    #[tokio::test]
    async fn test_system_monitor_with_prometheus_disabled() {
        let config = MonitorConfig {
            enable_prometheus: false,
            ..MonitorConfig::default()
        };
        let orchestrator = Arc::new(AgentOrchestrator::new());
        let monitor = SystemMonitor::new(config, orchestrator);

        // When prometheus is disabled, render should return empty string
        let output = monitor.render_prometheus_metrics().await;
        assert!(output.is_empty(), "expected empty output when prometheus is disabled");

        // prometheus_exporter should return None
        assert!(monitor.prometheus_exporter().is_none());

        // record_request should be a no-op (no panic)
        monitor.record_request("GET", "/test", 200, std::time::Duration::from_millis(10));
        monitor.record_task_completed();
        monitor.record_task_failed();
    }

    #[tokio::test]
    async fn test_system_monitor_with_prometheus_enabled() {
        let config = MonitorConfig {
            enable_prometheus: true,
            ..MonitorConfig::default()
        };
        let orchestrator = Arc::new(AgentOrchestrator::new());
        let monitor = SystemMonitor::new(config, orchestrator);

        // render_prometheus_metrics should return a non-empty string with metric names
        let output = monitor.render_prometheus_metrics().await;
        assert!(!output.is_empty(), "expected non-empty output when prometheus is enabled");
        assert!(output.contains("nexus_agents_active"), "output should contain nexus_agents_active");
        assert!(output.contains("nexus_tasks_total"), "output should contain nexus_tasks_total");
        assert!(output.contains("nexus_requests_total"), "output should contain nexus_requests_total");

        // prometheus_exporter should return Some
        assert!(monitor.prometheus_exporter().is_some());

        // record_request should update the exporter
        monitor.record_request("GET", "/api/test", 200, std::time::Duration::from_millis(50));
        let output = monitor.render_prometheus_metrics().await;
        assert!(output.contains("/api/test"), "output should contain the recorded request path");
    }

    #[tokio::test]
    async fn test_system_monitor_collect_metrics_updates_prometheus() {
        let config = MonitorConfig {
            enable_prometheus: true,
            ..MonitorConfig::default()
        };
        let orchestrator = Arc::new(AgentOrchestrator::new());
        let monitor = SystemMonitor::new(config, orchestrator);

        // collect_metrics should update prometheus gauges from system status
        monitor.collect_metrics().await.unwrap();

        let output = monitor.render_prometheus_metrics().await;
        // After collection, gauges should have values (even if 0 for a fresh orchestrator)
        assert!(output.contains("nexus_agents_active"), "gauges should be present after collect_metrics");
    }
}
