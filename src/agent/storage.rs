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
            fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create state directory {}", parent.display())
            })?;
        }
        let contents = serde_json::to_string_pretty(self).context("Failed to serialize state")?;
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
    pub const fn len(&self) -> usize {
        self.agents.len()
    }

    /// Check if there are no agents
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.agents.is_empty()
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

    // === Hierarchy Methods ===

    /// Get all root agents (agents without a parent)
    #[must_use]
    pub fn root_agents(&self) -> Vec<&Agent> {
        self.agents.iter().filter(|a| a.is_root()).collect()
    }

    /// Get all children of a specific agent
    #[must_use]
    pub fn children(&self, parent_id: Uuid) -> Vec<&Agent> {
        self.agents
            .iter()
            .filter(|a| a.parent_id == Some(parent_id))
            .collect()
    }

    /// Check if an agent has any children
    #[must_use]
    pub fn has_children(&self, agent_id: Uuid) -> bool {
        self.agents.iter().any(|a| a.parent_id == Some(agent_id))
    }

    /// Get the count of children for an agent
    #[must_use]
    pub fn child_count(&self, agent_id: Uuid) -> usize {
        self.agents
            .iter()
            .filter(|a| a.parent_id == Some(agent_id))
            .count()
    }

    /// Get the root ancestor of an agent (follows parent chain to the top)
    #[must_use]
    pub fn root_ancestor(&self, agent_id: Uuid) -> Option<&Agent> {
        let agent = self.get(agent_id)?;
        if agent.is_root() {
            return Some(agent);
        }

        // Follow parent chain
        let mut current = agent;
        while let Some(parent_id) = current.parent_id {
            if let Some(parent) = self.get(parent_id) {
                current = parent;
            } else {
                break;
            }
        }
        Some(current)
    }

    /// Get all descendants of an agent (children, grandchildren, etc.)
    #[must_use]
    pub fn descendants(&self, agent_id: Uuid) -> Vec<&Agent> {
        let mut result = Vec::new();
        self.collect_descendants(agent_id, &mut result);
        result
    }

    /// Helper to recursively collect descendants
    fn collect_descendants<'a>(&'a self, agent_id: Uuid, result: &mut Vec<&'a Agent>) {
        for child in self.children(agent_id) {
            result.push(child);
            self.collect_descendants(child.id, result);
        }
    }

    /// Get all descendant IDs (useful for removal operations)
    #[must_use]
    pub fn descendant_ids(&self, agent_id: Uuid) -> Vec<Uuid> {
        self.descendants(agent_id).iter().map(|a| a.id).collect()
    }

    /// Get the depth of an agent in the tree (root = 0)
    #[must_use]
    pub fn depth(&self, agent_id: Uuid) -> usize {
        let Some(agent) = self.get(agent_id) else {
            return 0;
        };

        let mut depth = 0;
        let mut current = agent;
        while let Some(parent_id) = current.parent_id {
            depth += 1;
            if let Some(parent) = self.get(parent_id) {
                current = parent;
            } else {
                break;
            }
        }
        depth
    }

    /// Get visible agents (respecting collapsed state) in display order
    /// Returns a list of (agent, depth) tuples for rendering
    #[must_use]
    pub fn visible_agents(&self) -> Vec<(&Agent, usize)> {
        let mut result = Vec::new();
        for root in self.root_agents() {
            self.add_visible_recursive(root, 0, &mut result);
        }
        result
    }

    /// Helper to recursively add visible agents
    fn add_visible_recursive<'a>(
        &'a self,
        agent: &'a Agent,
        depth: usize,
        result: &mut Vec<(&'a Agent, usize)>,
    ) {
        result.push((agent, depth));
        if !agent.collapsed {
            for child in self.children(agent.id) {
                self.add_visible_recursive(child, depth + 1, result);
            }
        }
    }

    /// Get the visible agent at a specific index
    #[must_use]
    pub fn visible_agent_at(&self, index: usize) -> Option<&Agent> {
        self.visible_agents().get(index).map(|(agent, _)| *agent)
    }

    /// Get the number of visible agents
    #[must_use]
    pub fn visible_count(&self) -> usize {
        self.visible_agents().len()
    }

    /// Find the visible index for a given agent ID
    #[must_use]
    pub fn visible_index_of(&self, agent_id: Uuid) -> Option<usize> {
        self.visible_agents()
            .iter()
            .position(|(agent, _)| agent.id == agent_id)
    }

    /// Remove an agent and all its descendants
    pub fn remove_with_descendants(&mut self, agent_id: Uuid) -> Vec<Agent> {
        // First collect all IDs to remove
        let mut ids_to_remove = self.descendant_ids(agent_id);
        ids_to_remove.push(agent_id);

        // Remove them all
        let mut removed = Vec::new();
        for id in ids_to_remove {
            if let Some(agent) = self.remove(id) {
                removed.push(agent);
            }
        }
        removed
    }

    /// Get the next available window index for a root agent's session
    #[must_use]
    pub fn next_window_index(&self, root_id: Uuid) -> u32 {
        // Window 1 is the root, children start at 2
        let descendants = self.descendants(root_id);
        let max_index = descendants
            .iter()
            .filter_map(|a| a.window_index)
            .max()
            .unwrap_or(1);
        max_index + 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{ChildConfig, Status};
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn create_test_agent(title: &str) -> Agent {
        Agent::new(
            title.to_string(),
            "claude".to_string(),
            format!("tenex/{title}"),
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
    fn test_remove_agent() -> Result<(), Box<dyn std::error::Error>> {
        let mut storage = Storage::new();
        let agent = create_test_agent("test");
        let id = agent.id;

        storage.add(agent);
        let removed = storage.remove(id);

        assert!(removed.is_some());
        assert_eq!(removed.ok_or("Agent not found")?.id, id);
        assert!(storage.is_empty());
        Ok(())
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
    fn test_get_mut() -> Result<(), Box<dyn std::error::Error>> {
        let mut storage = Storage::new();
        let agent = create_test_agent("test");
        let id = agent.id;

        storage.add(agent);

        if let Some(agent) = storage.get_mut(id) {
            agent.set_status(Status::Running);
        }

        assert_eq!(
            storage.get(id).ok_or("Agent not found")?.status,
            Status::Running
        );
        Ok(())
    }

    #[test]
    fn test_get_by_index() -> Result<(), Box<dyn std::error::Error>> {
        let mut storage = Storage::new();
        storage.add(create_test_agent("first"));
        storage.add(create_test_agent("second"));

        assert_eq!(
            storage.get_by_index(0).ok_or("Agent not found")?.title,
            "first"
        );
        assert_eq!(
            storage.get_by_index(1).ok_or("Agent not found")?.title,
            "second"
        );
        assert!(storage.get_by_index(2).is_none());
        Ok(())
    }

    #[test]
    fn test_get_by_index_mut() -> Result<(), Box<dyn std::error::Error>> {
        let mut storage = Storage::new();
        storage.add(create_test_agent("test"));

        if let Some(agent) = storage.get_by_index_mut(0) {
            agent.title = "modified".to_string();
        }

        assert_eq!(
            storage.get_by_index(0).ok_or("Agent not found")?.title,
            "modified"
        );
        Ok(())
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
    fn test_save_and_load() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let state_path = temp_dir.path().join("state.json");

        let mut storage = Storage::new();
        storage.add(create_test_agent("test1"));
        storage.add(create_test_agent("test2"));

        storage.save_to(&state_path)?;
        let loaded = Storage::load_from(&state_path)?;

        assert_eq!(storage.len(), loaded.len());
        assert_eq!(storage.agents[0].title, loaded.agents[0].title);
        assert_eq!(storage.agents[1].title, loaded.agents[1].title);
        Ok(())
    }

    #[test]
    fn test_load_nonexistent_returns_empty() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let state_path = temp_dir.path().join("nonexistent.json");

        assert!(Storage::load_from(&state_path).is_err());
        Ok(())
    }

    #[test]
    fn test_default_trait() {
        let storage = Storage::default();
        assert!(storage.is_empty());
        assert_eq!(storage.version, 1);
    }

    // === Hierarchy Tests ===

    fn create_child_agent(parent: &Agent, title: &str, window_index: u32) -> Agent {
        Agent::new_child(
            title.to_string(),
            "claude".to_string(),
            parent.branch.clone(),
            parent.worktree_path.clone(),
            None,
            ChildConfig {
                parent_id: parent.id,
                tmux_session: parent.tmux_session.clone(),
                window_index,
            },
        )
    }

    #[test]
    fn test_root_agents() {
        let mut storage = Storage::new();
        let root1 = create_test_agent("root1");
        let root2 = create_test_agent("root2");
        let child = create_child_agent(&root1, "child", 2);

        storage.add(root1);
        storage.add(root2);
        storage.add(child);

        let root_list = storage.root_agents();
        assert_eq!(root_list.len(), 2);
        assert!(root_list.iter().all(|a| a.is_root()));
    }

    #[test]
    fn test_children() {
        let mut storage = Storage::new();
        let root = create_test_agent("root");
        let root_id = root.id;
        let child1 = create_child_agent(&root, "child1", 2);
        let child2 = create_child_agent(&root, "child2", 3);

        storage.add(root);
        storage.add(child1);
        storage.add(child2);

        let children = storage.children(root_id);
        assert_eq!(children.len(), 2);
    }

    #[test]
    fn test_has_children() {
        let mut storage = Storage::new();
        let root = create_test_agent("root");
        let root_id = root.id;
        let child = create_child_agent(&root, "child", 2);

        storage.add(root);
        assert!(!storage.has_children(root_id));

        storage.add(child);
        assert!(storage.has_children(root_id));
    }

    #[test]
    fn test_child_count() {
        let mut storage = Storage::new();
        let root = create_test_agent("root");
        let root_id = root.id;

        storage.add(root.clone());
        assert_eq!(storage.child_count(root_id), 0);

        storage.add(create_child_agent(&root, "child1", 2));
        storage.add(create_child_agent(&root, "child2", 3));
        assert_eq!(storage.child_count(root_id), 2);
    }

    #[test]
    fn test_root_ancestor() -> Result<(), Box<dyn std::error::Error>> {
        let mut storage = Storage::new();
        let root = create_test_agent("root");
        let root_id = root.id;
        let child = create_child_agent(&root, "child", 2);
        let child_id = child.id;
        let grandchild = Agent::new_child(
            "grandchild".to_string(),
            "claude".to_string(),
            root.branch.clone(),
            root.worktree_path.clone(),
            None,
            ChildConfig {
                parent_id: child.id,
                tmux_session: root.tmux_session.clone(),
                window_index: 3,
            },
        );
        let grandchild_id = grandchild.id;

        storage.add(root);
        storage.add(child);
        storage.add(grandchild);

        // Root's ancestor is itself
        assert_eq!(
            storage.root_ancestor(root_id).ok_or("Agent not found")?.id,
            root_id
        );
        // Child's ancestor is root
        assert_eq!(
            storage.root_ancestor(child_id).ok_or("Agent not found")?.id,
            root_id
        );
        // Grandchild's ancestor is also root
        assert_eq!(
            storage
                .root_ancestor(grandchild_id)
                .ok_or("Agent not found")?
                .id,
            root_id
        );
        Ok(())
    }

    #[test]
    fn test_descendants() {
        let mut storage = Storage::new();
        let root = create_test_agent("root");
        let root_id = root.id;
        let child1 = create_child_agent(&root, "child1", 2);
        let child2 = create_child_agent(&root, "child2", 3);
        let grandchild = Agent::new_child(
            "grandchild".to_string(),
            "claude".to_string(),
            root.branch.clone(),
            root.worktree_path.clone(),
            None,
            ChildConfig {
                parent_id: child1.id,
                tmux_session: root.tmux_session.clone(),
                window_index: 4,
            },
        );

        storage.add(root);
        storage.add(child1);
        storage.add(child2);
        storage.add(grandchild);

        let descendants = storage.descendants(root_id);
        assert_eq!(descendants.len(), 3);
    }

    #[test]
    fn test_depth() {
        let mut storage = Storage::new();
        let root = create_test_agent("root");
        let root_id = root.id;
        let child = create_child_agent(&root, "child", 2);
        let child_id = child.id;
        let grandchild = Agent::new_child(
            "grandchild".to_string(),
            "claude".to_string(),
            root.branch.clone(),
            root.worktree_path.clone(),
            None,
            ChildConfig {
                parent_id: child.id,
                tmux_session: root.tmux_session.clone(),
                window_index: 3,
            },
        );
        let grandchild_id = grandchild.id;

        storage.add(root);
        storage.add(child);
        storage.add(grandchild);

        assert_eq!(storage.depth(root_id), 0);
        assert_eq!(storage.depth(child_id), 1);
        assert_eq!(storage.depth(grandchild_id), 2);
    }

    #[test]
    fn test_visible_agents_all_collapsed() {
        let mut storage = Storage::new();
        let root = create_test_agent("root");
        let root_id = root.id;
        let child = create_child_agent(&root, "child", 2);

        storage.add(root);
        storage.add(child);

        // By default, collapsed is true, so only root is visible
        let visible = storage.visible_agents();
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].0.id, root_id);
    }

    #[test]
    fn test_visible_agents_expanded() {
        let mut storage = Storage::new();
        let mut root = create_test_agent("root");
        root.collapsed = false; // Expand root
        let root_id = root.id;
        let child = create_child_agent(&root, "child", 2);
        let child_id = child.id;

        storage.add(root);
        storage.add(child);

        let visible = storage.visible_agents();
        assert_eq!(visible.len(), 2);
        assert_eq!(visible[0].0.id, root_id);
        assert_eq!(visible[0].1, 0); // depth 0
        assert_eq!(visible[1].0.id, child_id);
        assert_eq!(visible[1].1, 1); // depth 1
    }

    #[test]
    fn test_visible_agent_at() -> Result<(), Box<dyn std::error::Error>> {
        let mut storage = Storage::new();
        let mut root = create_test_agent("root");
        root.collapsed = false;
        let child = create_child_agent(&root, "child", 2);

        storage.add(root);
        storage.add(child);

        assert_eq!(
            storage.visible_agent_at(0).ok_or("Agent not found")?.title,
            "root"
        );
        assert_eq!(
            storage.visible_agent_at(1).ok_or("Agent not found")?.title,
            "child"
        );
        assert!(storage.visible_agent_at(2).is_none());
        Ok(())
    }

    #[test]
    fn test_visible_count() {
        let mut storage = Storage::new();
        let mut root = create_test_agent("root");
        root.collapsed = false;
        let child = create_child_agent(&root, "child", 2);

        storage.add(root);
        storage.add(child);

        assert_eq!(storage.visible_count(), 2);
    }

    #[test]
    fn test_remove_with_descendants() {
        let mut storage = Storage::new();
        let root = create_test_agent("root");
        let root_id = root.id;
        let child1 = create_child_agent(&root, "child1", 2);
        let child2 = create_child_agent(&root, "child2", 3);
        let grandchild = Agent::new_child(
            "grandchild".to_string(),
            "claude".to_string(),
            root.branch.clone(),
            root.worktree_path.clone(),
            None,
            ChildConfig {
                parent_id: child1.id,
                tmux_session: root.tmux_session.clone(),
                window_index: 4,
            },
        );

        storage.add(root);
        storage.add(child1);
        storage.add(child2);
        storage.add(grandchild);

        assert_eq!(storage.len(), 4);

        let removed = storage.remove_with_descendants(root_id);
        assert_eq!(removed.len(), 4);
        assert!(storage.is_empty());
    }

    #[test]
    fn test_next_window_index() {
        let mut storage = Storage::new();
        let root = create_test_agent("root");
        let root_id = root.id;

        storage.add(root.clone());
        // No children yet, next index should be 2 (window 1 is root)
        assert_eq!(storage.next_window_index(root_id), 2);

        storage.add(create_child_agent(&root, "child1", 2));
        assert_eq!(storage.next_window_index(root_id), 3);

        storage.add(create_child_agent(&root, "child2", 3));
        assert_eq!(storage.next_window_index(root_id), 4);
    }
}
