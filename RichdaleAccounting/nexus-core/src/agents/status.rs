//! Agent Status Module
//!
//! Provides status monitoring and reporting for agents.

use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use uuid::Uuid;
use chrono::{DateTime, Utc};
use crate::agents::agent_types::{AgentType, AgentStatus};

/// Struct containing status information for an agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStatusInfo {
    /// Agent ID
    pub agent_id: Uuid,
    /// Agent type
    pub agent_type: AgentType,
    /// Current status
    pub status: AgentStatus,
    /// Timestamp of last status change
    pub last_status_change: DateTime<Utc>,
    /// Number of tasks processed
    pub tasks_processed: u64,
    /// Number of tasks failed
    pub tasks_failed: u64,
    /// Number of tasks currently being processed
    pub tasks_in_progress: usize,
    /// Average processing time in milliseconds
    pub avg_processing_time_ms: f64,
    /// Last error message if any
    pub last_error: Option<String>,
    /// Timestamp of last task completion
    pub last_task_completion: Option<DateTime<Utc>>,
}

impl Default for AgentStatusInfo {
    fn default() -> Self {
        Self {
            agent_id: Uuid::new_v4(),
            agent_type: AgentType::default(),
            status: AgentStatus::default(),
            last_status_change: Utc::now(),
            tasks_processed: 0,
            tasks_failed: 0,
            tasks_in_progress: 0,
            avg_processing_time_ms: 0.0,
            last_error: None,
            last_task_completion: None,
        }
    }
}

impl AgentStatusInfo {
    /// Create a new agent status info
    pub fn new(agent_id: Uuid, agent_type: AgentType) -> Self {
        Self {
            agent_id,
            agent_type,
            ..Default::default()
        }
    }

    /// Update the status
    pub fn update_status(&mut self, new_status: AgentStatus) {
        self.status = new_status;
        self.last_status_change = Utc::now();
    }

    /// Record a completed task
    pub fn record_task_completion(&mut self, processing_time_ms: f64) {
        self.tasks_processed += 1;
        self.last_task_completion = Some(Utc::now());
        
        // Update average processing time
        if self.tasks_processed == 1 {
            self.avg_processing_time_ms = processing_time_ms;
        } else {
            let total = self.avg_processing_time_ms * (self.tasks_processed - 1) as f64;
            self.avg_processing_time_ms = (total + processing_time_ms) / self.tasks_processed as f64;
        }
    }

    /// Record a failed task
    pub fn record_task_failure(&mut self, error: &str) {
        self.tasks_failed += 1;
        self.last_error = Some(error.to_string());
    }

    /// Increment tasks in progress
    pub fn increment_tasks_in_progress(&mut self) {
        self.tasks_in_progress += 1;
    }

    /// Decrement tasks in progress
    pub fn decrement_tasks_in_progress(&mut self) {
        if self.tasks_in_progress > 0 {
            self.tasks_in_progress -= 1;
        }
    }

    /// Get the success rate
    pub fn success_rate(&self) -> f64 {
        let total = self.tasks_processed + self.tasks_failed;
        if total == 0 {
            1.0
        } else {
            self.tasks_processed as f64 / total as f64
        }
    }
}

/// Struct containing overall system status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemStatus {
    /// Timestamp of status report
    pub timestamp: DateTime<Utc>,
    /// Total number of agents
    pub total_agents: usize,
    /// Number of active agents
    pub active_agents: usize,
    /// Number of idle agents
    pub idle_agents: usize,
    /// Number of busy agents
    pub busy_agents: usize,
    /// Number of agents with errors
    pub error_agents: usize,
    /// Total tasks processed
    pub total_tasks_processed: u64,
    /// Total tasks failed
    pub total_tasks_failed: u64,
    /// Total tasks in progress
    pub total_tasks_in_progress: usize,
    /// Overall system health score (0.0 - 1.0)
    pub health_score: f64,
    /// Individual agent statuses
    pub agent_statuses: HashMap<Uuid, AgentStatusInfo>,
    /// System warnings
    pub warnings: Vec<String>,
}

impl Default for SystemStatus {
    fn default() -> Self {
        Self {
            timestamp: Utc::now(),
            total_agents: 0,
            active_agents: 0,
            idle_agents: 0,
            busy_agents: 0,
            error_agents: 0,
            total_tasks_processed: 0,
            total_tasks_failed: 0,
            total_tasks_in_progress: 0,
            health_score: 1.0,
            agent_statuses: HashMap::new(),
            warnings: Vec::new(),
        }
    }
}

impl SystemStatus {
    /// Create a new system status
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an agent status
    pub fn add_agent_status(&mut self, status: AgentStatusInfo) {
        self.agent_statuses.insert(status.agent_id, status);
        self.update_counts();
    }

    /// Update counts based on agent statuses
    pub fn update_counts(&mut self) {
        self.total_agents = self.agent_statuses.len();
        self.active_agents = 0;
        self.idle_agents = 0;
        self.busy_agents = 0;
        self.error_agents = 0;
        self.total_tasks_processed = 0;
        self.total_tasks_failed = 0;
        self.total_tasks_in_progress = 0;

        for status in self.agent_statuses.values() {
            match status.status {
                AgentStatus::Idle => self.idle_agents += 1,
                AgentStatus::Busy => self.busy_agents += 1,
                AgentStatus::Error(_) => self.error_agents += 1,
                AgentStatus::Initializing | AgentStatus::ShuttingDown => self.active_agents += 1,
            }
            
            self.total_tasks_processed += status.tasks_processed;
            self.total_tasks_failed += status.tasks_failed;
            self.total_tasks_in_progress += status.tasks_in_progress;
        }

        self.calculate_health_score();
    }

    /// Calculate the overall health score
    fn calculate_health_score(&mut self) {
        let mut score = 1.0;
        
        // Reduce score based on error agents
        if self.total_agents > 0 {
            let error_ratio = self.error_agents as f64 / self.total_agents as f64;
            score -= error_ratio * 0.4; // Max 40% reduction for errors
        }
        
        // Reduce score based on failure rate
        let total_tasks = self.total_tasks_processed + self.total_tasks_failed;
        if total_tasks > 0 {
            let failure_ratio = self.total_tasks_failed as f64 / total_tasks as f64;
            score -= failure_ratio * 0.3; // Max 30% reduction for failures
        }
        
        // Ensure score is between 0.0 and 1.0
        self.health_score = score.max(0.0).min(1.0);
        
        // Add warnings if health is low
        self.warnings.clear();
        if self.health_score < 0.5 {
            self.warnings.push("System health is critical".to_string());
        } else if self.health_score < 0.8 {
            self.warnings.push("System health is degraded".to_string());
        }
        
        if self.error_agents > 0 {
            self.warnings.push(format!("{} agents in error state", self.error_agents));
        }
    }

    /// Get system status summary
    pub fn summary(&self) -> String {
        format!(
            "System Status: {} agents ({} active, {} idle, {} busy, {} errors) | \
             Tasks: {} processed, {} failed, {} in progress | \
             Health: {:.2}%",
            self.total_agents,
            self.active_agents,
            self.idle_agents,
            self.busy_agents,
            self.error_agents,
            self.total_tasks_processed,
            self.total_tasks_failed,
            self.total_tasks_in_progress,
            self.health_score * 100.0
        )
    }
}

/// Trait for status monitoring
pub trait StatusMonitor {
    /// Get the current status
    fn get_status(&self) -> AgentStatusInfo;
    
    /// Get system-wide status
    fn get_system_status(&self) -> SystemStatus;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_status_info() {
        let mut status = AgentStatusInfo::new(Uuid::new_v4(), AgentType::LedgerAgent);
        assert_eq!(status.tasks_processed, 0);
        assert_eq!(status.success_rate(), 1.0);
        
        status.record_task_completion(100.0);
        assert_eq!(status.tasks_processed, 1);
        assert_eq!(status.avg_processing_time_ms, 100.0);
        
        status.record_task_completion(200.0);
        assert_eq!(status.tasks_processed, 2);
        assert_eq!(status.avg_processing_time_ms, 150.0);
        
        status.record_task_failure("Test error");
        assert_eq!(status.tasks_failed, 1);
        assert_eq!(status.success_rate(), 2.0 / 3.0);
    }

    #[test]
    fn test_system_status() {
        let mut system_status = SystemStatus::new();
        
        let agent1 = AgentStatusInfo::new(Uuid::new_v4(), AgentType::LedgerAgent);
        let mut agent2 = AgentStatusInfo::new(Uuid::new_v4(), AgentType::InvoiceAgent);
        agent2.update_status(AgentStatus::Busy);
        
        system_status.add_agent_status(agent1);
        system_status.add_agent_status(agent2);
        
        assert_eq!(system_status.total_agents, 2);
        assert_eq!(system_status.idle_agents, 1);
        assert_eq!(system_status.busy_agents, 1);
        assert!(system_status.health_score > 0.9);
    }

    #[test]
    fn test_health_score_calculation() {
        let mut system_status = SystemStatus::new();
        
        // Add an error agent
        let mut error_agent = AgentStatusInfo::new(Uuid::new_v4(), AgentType::AuditAgent);
        error_agent.update_status(AgentStatus::Error("Test error".to_string()));
        system_status.add_agent_status(error_agent);
        
        // Add a normal agent
        let normal_agent = AgentStatusInfo::new(Uuid::new_v4(), AgentType::LedgerAgent);
        system_status.add_agent_status(normal_agent);
        
        assert!(system_status.health_score < 1.0);
        assert!(system_status.health_score > 0.5);
    }
}
