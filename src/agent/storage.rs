//! Agent persistence layer

use super::Agent;
use crate::config::Config;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use uuid::Uuid;

/// Persisted state for all agents
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Storage {
    /// All tracked agents
    pub agents: Vec<Agent>,

    /// Version of the state file format
    #[serde(default = "default_version")]
    pub version: u32,
}

impl Default for Storage {
    fn default() -> Self {
        Self::new()
    }
}

const fn default_version() -> u32 {
    1
}

impl Storage {
    /// Create a new empty storage
    #[must_use]
    pub const fn new() -> Self {
        Self {
            agents: Vec::new(),
            version: default_version(),
        }
    }

    /// Load state from the default location
    ///
    /// # Errors
    ///
    /// Returns an error if the state file exists but cannot be read or parsed
    pub fn load() -> Result<Self> {
        let path = Config::state_path();
        if path.exists() {
            Self::load_from(&path)
        } else {
            Ok(Self::new())
        }
    }

    /// Load state from a specific path
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read or parsed
    pub fn load_from(path: &Path) -> Result<Self> {
        let contents = fs::read_to_string(path)
            .with_context(|| format!("Failed to read state from {}", path.display()))?;
        let storage: Self = serde_json::from_str(&contents)
            .with_context(|| format!("Failed to parse state from {}", path.display()))?;
        Ok(storage)
    }

    /// Save state to the default location
    ///
    /// # Errors
    ///
    /// Returns an error if the state directory cannot be created or the file cannot be written
    pub fn save(&self) -> Result<()> {
        let path = Config::state_path();
        self.save_to(&path)
    }

    /// Save state to a specific path
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be written
    pub fn save_to(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create state directory {}", parent.display()))?;
        }
        let contents = serde_json::to_string_pretty(self)
            .context("Failed to serialize state")?;
        fs::write(path, contents)
            .with_context(|| format!("Failed to write state to {}", path.display()))?;
        Ok(())
    }

    /// Add a new agent
    pub fn add(&mut self, agent: Agent) {
        self.agents.push(agent);
    }

    /// Remove an agent by ID
    pub fn remove(&mut self, id: Uuid) -> Option<Agent> {
        if let Some(pos) = self.agents.iter().position(|a| a.id == id) {
            Some(self.agents.remove(pos))
        } else {
            None
        }
    }

    /// Get an agent by ID
    #[must_use]
    pub fn get(&self, id: Uuid) -> Option<&Agent> {
        self.agents.iter().find(|a| a.id == id)
    }

    /// Get a mutable reference to an agent by ID
    pub fn get_mut(&mut self, id: Uuid) -> Option<&mut Agent> {
        self.agents.iter_mut().find(|a| a.id == id)
    }

    /// Get an agent by index
    #[must_use]
    pub fn get_by_index(&self, index: usize) -> Option<&Agent> {
        self.agents.get(index)
    }

    /// Get a mutable reference to an agent by index
    pub fn get_by_index_mut(&mut self, index: usize) -> Option<&mut Agent> {
        self.agents.get_mut(index)
    }

    /// Find an agent by short ID (first 8 chars)
    #[must_use]
    pub fn find_by_short_id(&self, short_id: &str) -> Option<&Agent> {
        self.agents
            .iter()
            .find(|a| a.id.to_string().starts_with(short_id))
    }

    /// Get the number of agents
    #[must_use]
    pub fn len(&self) -> usize {
        self.agents.len()
    }

    /// Check if there are no agents
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.agents.is_empty()
    }

    /// Get all alive agents (not stopped)
    #[must_use]
    pub fn alive_agents(&self) -> Vec<&Agent> {
        self.agents.iter().filter(|a| a.is_alive()).collect()
    }

    /// Clear all agents
    pub fn clear(&mut self) {
        self.agents.clear();
    }

    /// Get an iterator over all agents
    pub fn iter(&self) -> impl Iterator<Item = &Agent> {
        self.agents.iter()
    }

    /// Get a mutable iterator over all agents
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Agent> {
        self.agents.iter_mut()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::Status;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn create_test_agent(title: &str) -> Agent {
        Agent::new(
            title.to_string(),
            "claude".to_string(),
            format!("muster/{title}"),
            PathBuf::from("/tmp/worktree"),
            None,
        )
    }

    #[test]
    fn test_new_storage() {
        let storage = Storage::new();
        assert!(storage.is_empty());
        assert_eq!(storage.len(), 0);
        assert_eq!(storage.version, 1);
    }

    #[test]
    fn test_add_agent() {
        let mut storage = Storage::new();
        let agent = create_test_agent("test");

        storage.add(agent);

        assert_eq!(storage.len(), 1);
        assert!(!storage.is_empty());
    }

    #[test]
    fn test_remove_agent() {
        let mut storage = Storage::new();
        let agent = create_test_agent("test");
        let id = agent.id;

        storage.add(agent);
        let removed = storage.remove(id);

        assert!(removed.is_some());
        assert_eq!(removed.unwrap().id, id);
        assert!(storage.is_empty());
    }

    #[test]
    fn test_remove_nonexistent() {
        let mut storage = Storage::new();
        let removed = storage.remove(Uuid::new_v4());
        assert!(removed.is_none());
    }

    #[test]
    fn test_get_agent() {
        let mut storage = Storage::new();
        let agent = create_test_agent("test");
        let id = agent.id;

        storage.add(agent);

        assert!(storage.get(id).is_some());
        assert!(storage.get(Uuid::new_v4()).is_none());
    }

    #[test]
    fn test_get_mut() {
        let mut storage = Storage::new();
        let agent = create_test_agent("test");
        let id = agent.id;

        storage.add(agent);

        if let Some(agent) = storage.get_mut(id) {
            agent.set_status(Status::Running);
        }

        assert_eq!(storage.get(id).unwrap().status, Status::Running);
    }

    #[test]
    fn test_get_by_index() {
        let mut storage = Storage::new();
        storage.add(create_test_agent("first"));
        storage.add(create_test_agent("second"));

        assert_eq!(storage.get_by_index(0).unwrap().title, "first");
        assert_eq!(storage.get_by_index(1).unwrap().title, "second");
        assert!(storage.get_by_index(2).is_none());
    }

    #[test]
    fn test_get_by_index_mut() {
        let mut storage = Storage::new();
        storage.add(create_test_agent("test"));

        if let Some(agent) = storage.get_by_index_mut(0) {
            agent.title = "modified".to_string();
        }

        assert_eq!(storage.get_by_index(0).unwrap().title, "modified");
    }

    #[test]
    fn test_find_by_short_id() {
        let mut storage = Storage::new();
        let agent = create_test_agent("test");
        let short_id = agent.short_id();

        storage.add(agent);

        assert!(storage.find_by_short_id(&short_id).is_some());
        assert!(storage.find_by_short_id("nonexistent").is_none());
    }

    #[test]
    fn test_alive_agents() {
        let mut storage = Storage::new();

        let mut agent1 = create_test_agent("alive");
        agent1.set_status(Status::Running);

        let mut agent2 = create_test_agent("dead");
        agent2.set_status(Status::Stopped);

        storage.add(agent1);
        storage.add(agent2);

        let alive = storage.alive_agents();
        assert_eq!(alive.len(), 1);
        assert_eq!(alive[0].title, "alive");
    }

    #[test]
    fn test_clear() {
        let mut storage = Storage::new();
        storage.add(create_test_agent("test1"));
        storage.add(create_test_agent("test2"));

        storage.clear();

        assert!(storage.is_empty());
    }

    #[test]
    fn test_iter() {
        let mut storage = Storage::new();
        storage.add(create_test_agent("test1"));
        storage.add(create_test_agent("test2"));

        let titles: Vec<_> = storage.iter().map(|a| a.title.as_str()).collect();
        assert_eq!(titles, vec!["test1", "test2"]);
    }

    #[test]
    fn test_iter_mut() {
        let mut storage = Storage::new();
        storage.add(create_test_agent("test1"));
        storage.add(create_test_agent("test2"));

        for agent in storage.iter_mut() {
            agent.set_status(Status::Running);
        }

        assert!(storage.iter().all(|a| a.status == Status::Running));
    }

    #[test]
    fn test_save_and_load() {
        let temp_dir = TempDir::new().unwrap();
        let state_path = temp_dir.path().join("state.json");

        let mut storage = Storage::new();
        storage.add(create_test_agent("test1"));
        storage.add(create_test_agent("test2"));

        storage.save_to(&state_path).unwrap();
        let loaded = Storage::load_from(&state_path).unwrap();

        assert_eq!(storage.len(), loaded.len());
        assert_eq!(storage.agents[0].title, loaded.agents[0].title);
        assert_eq!(storage.agents[1].title, loaded.agents[1].title);
    }

    #[test]
    fn test_load_nonexistent_returns_empty() {
        let temp_dir = TempDir::new().unwrap();
        let state_path = temp_dir.path().join("nonexistent.json");

        assert!(Storage::load_from(&state_path).is_err());
    }

    #[test]
    fn test_default_trait() {
        let storage = Storage::default();
        assert!(storage.is_empty());
        assert_eq!(storage.version, 1);
    }
}
