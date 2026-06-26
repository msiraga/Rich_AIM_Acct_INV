//! Agent Memory Module
//!
//! Provides memory and context management for agents.

use serde::{Serialize, Deserialize};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use uuid::Uuid;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use crate::agents::agent_types::AgentType;

/// Struct representing an agent's memory/context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMemory {
    /// Agent ID
    pub agent_id: Uuid,
    /// Agent type
    pub agent_type: AgentType,
    /// Short-term memory (recent context)
    pub short_term_memory: VecDeque<MemoryEntry>,
    /// Long-term memory (persistent knowledge)
    pub long_term_memory: HashMap<String, MemoryEntry>,
    /// Working memory (current task context)
    pub working_memory: HashMap<String, serde_json::Value>,
    /// Maximum short-term memory entries
    pub max_short_term_entries: usize,
    /// Maximum long-term memory entries
    pub max_long_term_entries: usize,
}

impl Default for AgentMemory {
    fn default() -> Self {
        Self {
            agent_id: Uuid::new_v4(),
            agent_type: AgentType::default(),
            short_term_memory: VecDeque::new(),
            long_term_memory: HashMap::new(),
            working_memory: HashMap::new(),
            max_short_term_entries: 100,
            max_long_term_entries: 1000,
        }
    }
}

impl AgentMemory {
    /// Create a new agent memory
    pub fn new(agent_id: Uuid, agent_type: AgentType) -> Self {
        Self {
            agent_id,
            agent_type,
            ..Default::default()
        }
    }

    /// Add an entry to short-term memory
    pub fn add_short_term_memory(&mut self, entry: MemoryEntry) {
        self.short_term_memory.push_back(entry);
        
        // Trim if necessary
        if self.short_term_memory.len() > self.max_short_term_entries {
            self.short_term_memory.pop_front();
        }
    }

    /// Add an entry to long-term memory
    pub fn add_long_term_memory(&mut self, key: &str, entry: MemoryEntry) {
        self.long_term_memory.insert(key.to_string(), entry);
        
        // Trim if necessary
        if self.long_term_memory.len() > self.max_long_term_entries {
            // Remove the oldest entry (simple strategy)
            if let Some(oldest_key) = self.long_term_memory.keys().next().cloned() {
                self.long_term_memory.remove(&oldest_key);
            }
        }
    }

    /// Get a long-term memory entry
    pub fn get_long_term_memory(&self, key: &str) -> Option<&MemoryEntry> {
        self.long_term_memory.get(key)
    }

    /// Remove a long-term memory entry
    pub fn remove_long_term_memory(&mut self, key: &str) -> Option<MemoryEntry> {
        self.long_term_memory.remove(key)
    }

    /// Set working memory value
    pub fn set_working_memory(&mut self, key: &str, value: serde_json::Value) {
        self.working_memory.insert(key.to_string(), value);
    }

    /// Get working memory value
    pub fn get_working_memory(&self, key: &str) -> Option<&serde_json::Value> {
        self.working_memory.get(key)
    }

    /// Clear working memory
    pub fn clear_working_memory(&mut self) {
        self.working_memory.clear();
    }

    /// Clear short-term memory
    pub fn clear_short_term_memory(&mut self) {
        self.short_term_memory.clear();
    }

    /// Get recent short-term memory entries
    pub fn get_recent_short_term_memory(&self, count: usize) -> Vec<&MemoryEntry> {
        let start = if count > self.short_term_memory.len() {
            0
        } else {
            self.short_term_memory.len() - count
        };
        
        self.short_term_memory.range(start..).collect()
    }

    /// Find relevant memory entries based on tags
    pub fn find_by_tags(&self, tags: &[String]) -> Vec<&MemoryEntry> {
        let mut results = Vec::new();
        
        // Search short-term memory
        for entry in &self.short_term_memory {
            if entry.has_any_tags(tags) {
                results.push(entry);
            }
        }
        
        // Search long-term memory
        for entry in self.long_term_memory.values() {
            if entry.has_any_tags(tags) {
                results.push(entry);
            }
        }
        
        results
    }
}

/// A single memory entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    /// Unique identifier
    pub id: Uuid,
    /// Content of the memory
    pub content: String,
    /// Data associated with the memory
    pub data: Option<serde_json::Value>,
    /// Tags for categorization
    pub tags: Vec<String>,
    /// Priority/importance (0.0 - 1.0)
    pub priority: f64,
    /// Timestamp when created
    pub created_at: DateTime<Utc>,
    /// Timestamp when last accessed
    pub last_accessed: DateTime<Utc>,
    /// Number of times accessed
    pub access_count: u32,
}

impl Default for MemoryEntry {
    fn default() -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            content: String::new(),
            data: None,
            tags: Vec::new(),
            priority: 0.5,
            created_at: now,
            last_accessed: now,
            access_count: 0,
        }
    }
}

impl MemoryEntry {
    /// Create a new memory entry
    pub fn new(content: &str) -> Self {
        Self {
            content: content.to_string(),
            ..Default::default()
        }
    }

    /// Create a new memory entry with data
    pub fn with_data(content: &str, data: serde_json::Value) -> Self {
        Self {
            content: content.to_string(),
            data: Some(data),
            ..Default::default()
        }
    }

    /// Add a tag
    pub fn add_tag(&mut self, tag: &str) {
        if !self.tags.contains(&tag.to_string()) {
            self.tags.push(tag.to_string());
        }
    }

    /// Add multiple tags
    pub fn add_tags(&mut self, tags: &[&str]) {
        for tag in tags {
            self.add_tag(tag);
        }
    }

    /// Check if entry has a specific tag
    pub fn has_tag(&self, tag: &str) -> bool {
        self.tags.contains(&tag.to_string())
    }

    /// Check if entry has any of the specified tags
    pub fn has_any_tags(&self, tags: &[String]) -> bool {
        for tag in tags {
            if self.has_tag(tag) {
                return true;
            }
        }
        false
    }

    /// Record an access
    pub fn record_access(&mut self) {
        self.access_count += 1;
        self.last_accessed = Utc::now();
    }

    /// Set priority
    pub fn set_priority(&mut self, priority: f64) {
        self.priority = priority.clamp(0.0, 1.0);
    }
}

/// Global memory manager for all agents
#[derive(Debug, Clone)]
pub struct MemoryManager {
    /// Memory for each agent
    pub agent_memories: Arc<DashMap<Uuid, AgentMemory>>,
    /// Shared memory accessible by all agents
    pub shared_memory: Arc<DashMap<String, MemoryEntry>>,
}

impl Default for MemoryManager {
    fn default() -> Self {
        Self {
            agent_memories: Arc::new(DashMap::new()),
            shared_memory: Arc::new(DashMap::new()),
        }
    }
}

impl MemoryManager {
    /// Create a new memory manager
    pub fn new() -> Self {
        Self::default()
    }

    /// Get or create memory for an agent
    pub fn get_or_create_agent_memory(&self, agent_id: Uuid, agent_type: AgentType) -> AgentMemory {
        if let Some(memory) = self.agent_memories.get(&agent_id) {
            memory.clone()
        } else {
            let memory = AgentMemory::new(agent_id, agent_type);
            self.agent_memories.insert(agent_id, memory.clone());
            memory
        }
    }

    /// Add to shared memory
    pub fn add_shared_memory(&self, key: &str, entry: MemoryEntry) {
        self.shared_memory.insert(key.to_string(), entry);
    }

    /// Get from shared memory
    pub fn get_shared_memory(&self, key: &str) -> Option<MemoryEntry> {
        self.shared_memory.get(key).map(|entry| entry.clone())
    }

    /// Remove from shared memory
    pub fn remove_shared_memory(&self, key: &str) -> Option<MemoryEntry> {
        self.shared_memory.remove(key).map(|(_, entry)| entry)
    }

    /// Find in shared memory by tags
    pub fn find_shared_by_tags(&self, tags: &[String]) -> Vec<MemoryEntry> {
        let mut results = Vec::new();
        for entry in self.shared_memory.iter() {
            if entry.has_any_tags(tags) {
                results.push(entry.clone());
            }
        }
        results
    }

    /// Clear all memory
    pub fn clear_all(&self) {
        self.agent_memories.clear();
        self.shared_memory.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_entry_creation() {
        let entry = MemoryEntry::new("Test content");
        assert_eq!(entry.content, "Test content");
        assert!(entry.tags.is_empty());
    }

    #[test]
    fn test_memory_entry_tags() {
        let mut entry = MemoryEntry::new("Test");
        entry.add_tag("accounting");
        entry.add_tag("finance");
        
        assert!(entry.has_tag("accounting"));
        assert!(entry.has_tag("finance"));
        assert!(!entry.has_tag("other"));
        
        assert!(entry.has_any_tags(&["accounting".to_string(), "other".to_string()]));
        assert!(!entry.has_any_tags(&["other".to_string()]));
    }

    #[test]
    fn test_agent_memory() {
        let mut memory = AgentMemory::new(Uuid::new_v4(), AgentType::LedgerAgent);
        
        // Add short-term memory
        let entry1 = MemoryEntry::new("Recent transaction");
        memory.add_short_term_memory(entry1);
        
        // Add long-term memory
        let entry2 = MemoryEntry::new("Important rule");
        memory.add_long_term_memory("rule1", entry2);
        
        // Add working memory
        memory.set_working_memory("current_task", serde_json::json!({"id": "123"}));
        
        assert_eq!(memory.short_term_memory.len(), 1);
        assert_eq!(memory.long_term_memory.len(), 1);
        assert_eq!(memory.working_memory.len(), 1);
        
        // Test retrieval
        assert!(memory.get_long_term_memory("rule1").is_some());
        assert!(memory.get_working_memory("current_task").is_some());
    }

    #[test]
    fn test_memory_manager() {
        let manager = MemoryManager::new();
        let agent_id = Uuid::new_v4();
        
        // Get or create agent memory
        let memory = manager.get_or_create_agent_memory(agent_id, AgentType::InvoiceAgent);
        assert_eq!(memory.agent_id, agent_id);
        
        // Add shared memory
        let entry = MemoryEntry::new("Shared knowledge");
        manager.add_shared_memory("shared1", entry);
        
        assert!(manager.get_shared_memory("shared1").is_some());
    }
}
