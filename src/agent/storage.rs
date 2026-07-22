//! Agent persistence layer

use super::{Agent, WorkspaceKind};
use crate::config::Config;
use crate::git;
use anyhow::{Context, Result};
use fs4::fs_std::FileExt as _;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::collections::HashSet;
use std::fs;
#[cfg(target_os = "linux")]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;
use tracing::warn;
use uuid::Uuid;

#[cfg(target_os = "linux")]
const STATE_FILE_MODE: u32 = 0o600;

fn resolve_state_path(path: &Path) -> std::path::PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn backup_state_path(path: &Path) -> std::path::PathBuf {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("state.json");
    path.with_file_name(format!("{name}.bak"))
}

fn lock_state_path(path: &Path) -> std::path::PathBuf {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("state.json");
    path.with_file_name(format!("{name}.lock"))
}

fn temp_state_path(path: &Path) -> std::path::PathBuf {
    let token = Uuid::new_v4();
    let tmp_name = format!(
        ".{}.tmp-{}-{token}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("state.json"),
        std::process::id(),
    );
    path.with_file_name(tmp_name)
}

fn write_temp_state_file(path: &Path, contents: &str) -> Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(target_os = "linux")]
    {
        // Create the temp file with restrictive permissions immediately to avoid a brief
        // window where sensitive contents are world-readable.
        options.mode(STATE_FILE_MODE);
    }

    let mut file = options.open(path).context(format!(
        "Failed to create temp state file {}",
        path.display()
    ))?;

    std::io::Write::write_all(&mut file, contents.as_bytes()).context(format!(
        "Failed to write temp state file {}",
        path.display()
    ))?;
    file.sync_all().context(format!(
        "Failed to sync temp state file to disk {}",
        path.display()
    ))
}

fn set_temp_permissions(path: &Path, existing_permissions: Option<fs::Permissions>) -> Result<()> {
    if let Some(permissions) = existing_permissions {
        fs::set_permissions(path, permissions).context(format!(
            "Failed to set permissions for temp state file {}",
            path.display()
        ))?;
        return Ok(());
    }

    #[cfg(target_os = "linux")]
    fs::set_permissions(path, fs::Permissions::from_mode(STATE_FILE_MODE)).context(format!(
        "Failed to set permissions for temp state file {}",
        path.display()
    ))?;
    Ok(())
}

fn commit_temp_state_file(
    path: &Path,
    tmp_path: &Path,
    existing_permissions: Option<fs::Permissions>,
) -> Result<()> {
    set_temp_permissions(tmp_path, existing_permissions)?;
    fs::rename(tmp_path, path).with_context(|| {
        format!(
            "Failed to replace state file {} with {}",
            path.display(),
            tmp_path.display()
        )
    })?;
    Ok(())
}

fn write_state_atomically(path: &Path, contents: &str) -> Result<()> {
    let existing_permissions = fs::metadata(path)
        .ok()
        .map(|metadata| metadata.permissions());
    let tmp_path = temp_state_path(path);

    let write_result = (|| -> Result<()> {
        write_temp_state_file(&tmp_path, contents)?;
        commit_temp_state_file(path, &tmp_path, existing_permissions)?;
        Ok(())
    })();

    if write_result.is_err() {
        let _ = fs::remove_file(&tmp_path);
    }

    write_result
}

/// Pre-computed visible agent information for efficient rendering
#[derive(Debug, Clone)]
pub struct VisibleAgentInfo<'a> {
    /// Reference to the agent
    pub agent: &'a Agent,
    /// Depth in the tree (0 for root)
    pub depth: usize,
    /// Whether this agent has children
    pub has_children: bool,
    /// Number of children this agent has
    pub child_count: usize,
}

/// Persisted state for all agents
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Storage {
    /// All tracked agents
    pub agents: Vec<Agent>,

    /// Version of the state file format
    #[serde(default = "default_version")]
    pub version: u32,

    /// Unique identifier for this Tenex instance (used for mux session namespacing).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instance_id: Option<String>,

    /// Mux daemon socket name/path used by this Tenex instance.
    ///
    /// Tenex persists this value so agent sessions can survive restarts even if the Tenex binary
    /// (and thus the default mux socket fingerprint) changes across upgrades or rebuilds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mux_socket: Option<String>,

    /// Custom state file path (if set, overrides default location)
    /// When None, uses `Config::state_path()`
    #[serde(skip)]
    pub state_path: Option<std::path::PathBuf>,

    #[serde(skip)]
    last_loaded: Option<StorageSnapshot>,
}

#[derive(Debug, Clone)]
struct StorageSnapshot {
    agents_by_id: HashMap<Uuid, Agent>,
    version: u32,
    instance_id: Option<String>,
    mux_socket: Option<String>,
}

impl StorageSnapshot {
    fn capture(storage: &Storage) -> Self {
        let agents_by_id = storage
            .agents
            .iter()
            .cloned()
            .map(|agent| (agent.id, agent))
            .collect();

        Self {
            agents_by_id,
            version: storage.version,
            instance_id: storage.instance_id.clone(),
            mux_socket: storage.mux_socket.clone(),
        }
    }
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
            instance_id: None,
            mux_socket: None,
            state_path: None,
            last_loaded: None,
        }
    }

    /// Create a new empty storage with a custom state file path.
    #[must_use]
    pub const fn with_path(path: std::path::PathBuf) -> Self {
        Self {
            agents: Vec::new(),
            version: 1, // Can't call default_version() in const context
            instance_id: None,
            mux_socket: None,
            state_path: Some(path),
            last_loaded: None,
        }
    }

    fn generate_instance_id() -> String {
        Uuid::new_v4().to_string()[..8].to_string()
    }

    fn normalize_instance_id(value: &str) -> Option<String> {
        let trimmed = value.trim();
        if trimmed.len() != 8 {
            return None;
        }

        let mut normalized = String::with_capacity(8);
        for ch in trimmed.chars() {
            let lower = ch.to_ascii_lowercase();
            if !lower.is_ascii_hexdigit() {
                return None;
            }
            normalized.push(lower);
        }

        Some(normalized)
    }

    /// Get (or generate) this instance's ID.
    ///
    /// If the ID is missing or invalid, a new one is generated and stored in memory.
    /// Persisting it is the caller's responsibility (by saving storage).
    #[must_use]
    pub fn ensure_instance_id(&mut self) -> &str {
        match self
            .instance_id
            .as_deref()
            .and_then(Self::normalize_instance_id)
        {
            Some(normalized) => {
                if self.instance_id.as_deref() != Some(normalized.as_str()) {
                    self.instance_id = Some(normalized);
                }
            }
            None => {
                self.instance_id = Some(Self::generate_instance_id());
            }
        }

        self.instance_id.as_deref().unwrap_or("00000000")
    }

    /// Prefix for mux sessions belonging to this instance.
    #[must_use]
    pub fn instance_session_prefix(&mut self) -> String {
        format!("tenex-{}-", self.ensure_instance_id())
    }

    /// Load state from the default location
    ///
    /// # Errors
    ///
    /// Returns an error if the state file exists but cannot be read or parsed
    pub fn load() -> Result<Self> {
        Self::load_at(&Config::state_path())
    }

    /// Load state from the provided path.
    ///
    /// This performs the same backup fallback behavior as `load` but avoids reading configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if the state file exists but cannot be read or parsed and no usable backup
    /// exists.
    pub fn load_at(configured_path: &Path) -> Result<Self> {
        let path = resolve_state_path(configured_path);
        let backup_path = backup_state_path(&path);

        if path.exists() {
            match Self::load_from(&path) {
                Ok(storage) => Ok(storage),
                Err(err) => {
                    if backup_path.exists() {
                        warn!(
                            error = %err,
                            path = %path.display(),
                            backup_path = %backup_path.display(),
                            "Failed to load state file; attempting backup"
                        );
                        Self::load_from(&backup_path)
                    } else {
                        Err(err)
                    }
                }
            }
        } else if backup_path.exists() {
            // Best-effort recovery for interrupted writes where the state file was moved aside
            // but the new state wasn't written into place.
            match fs::rename(&backup_path, &path) {
                Ok(()) => {
                    warn!(
                        path = %path.display(),
                        backup_path = %backup_path.display(),
                        "Recovered missing state file from backup"
                    );
                    Self::load_from(&path)
                }
                Err(err) => {
                    warn!(
                        error = %err,
                        path = %path.display(),
                        backup_path = %backup_path.display(),
                        "Failed to restore backup state file; reading it in place"
                    );
                    Self::load_from(&backup_path)
                }
            }
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
        let mut storage: Self = serde_json::from_str(&contents)
            .with_context(|| format!("Failed to parse state from {}", path.display()))?;
        storage.last_loaded = Some(StorageSnapshot::capture(&storage));
        Ok(storage)
    }

    /// Ensure `workspace_kind` is consistent with the agent's `worktree_path`.
    ///
    /// Older Tenex versions did not persist `workspace_kind`, so agents created in non-git
    /// directories may deserialize as `GitWorktree` by default. When their working directory is
    /// not inside a git repository, treat them as `PlainDir` so the UI label and behavior remain
    /// stable across restarts.
    pub fn backfill_workspace_kinds(&mut self) -> bool {
        let mut changed = false;

        for agent in &mut self.agents {
            if agent.workspace_kind != WorkspaceKind::GitWorktree {
                continue;
            }

            if !agent.worktree_path.exists() {
                continue;
            }

            if git::is_git_repository(&agent.worktree_path) {
                continue;
            }

            agent.workspace_kind = WorkspaceKind::PlainDir;
            changed = true;
        }

        changed
    }

    /// Remove deprecated short IDs from auto-generated child agent titles.
    ///
    /// Tenex used to append the agent's short id (e.g. `Agent 1 (deadbeef)`) to make tmux window
    /// names unique. The UI now provides enough context, so strip the suffix when it matches the
    /// agent's own short id and the remaining prefix is one of the known auto-generated titles.
    pub fn backfill_child_titles(&mut self) -> bool {
        let mut changed = false;

        for agent in &mut self.agents {
            if agent.parent_id.is_none() {
                continue;
            }

            let short_id = agent.short_id();
            let suffix = format!(" ({short_id})");
            let Some(stripped) = agent.title.strip_suffix(&suffix) else {
                continue;
            };

            let stripped = stripped.trim_end();
            if matches!(
                stripped,
                title if title.starts_with("Agent ")
                    || title.starts_with("Planner ")
                    || title.starts_with("Reviewer ")
            ) {
                agent.title = stripped.to_string();
                changed = true;
            }
        }

        changed
    }

    /// Backfill the repository/workspace root for agents created by older Tenex versions.
    ///
    /// Tenex stores agents in a global state file, so it can load agents created from different
    /// repositories. The UI groups agents by this root, and agent creation uses it to ensure new
    /// worktrees are created in the highlighted repository instead of the process CWD.
    pub fn backfill_repo_roots(&mut self) -> bool {
        let mut changed = false;

        for agent in &mut self.agents {
            if agent.repo_root.is_some() {
                continue;
            }

            let root = if agent.worktree_path.exists() {
                git::repository_workspace_root(&agent.worktree_path)
                    .unwrap_or_else(|_| agent.worktree_path.clone())
            } else {
                agent.worktree_path.clone()
            };

            agent.repo_root = Some(root);
            changed = true;
        }

        changed
    }

    /// Backfill agent conversation IDs for older Tenex state files.
    ///
    /// Tenex uses `conversation_id` to resume supported agent CLIs after restarts/crashes.
    /// Older state files may not have this field populated.
    pub fn backfill_conversation_ids(&mut self) -> bool {
        let mut changed = false;

        for agent in &mut self.agents {
            if agent.is_terminal_agent() {
                continue;
            }

            if let Some(existing) = agent.conversation_id.as_deref()
                && !existing.trim().is_empty()
            {
                continue;
            }

            if crate::conversation::detect_agent_cli(&agent.program)
                == crate::conversation::AgentCli::Claude
            {
                agent.conversation_id = Some(agent.id.to_string());
                changed = true;
            }
        }

        changed
    }

    /// Save state to the configured location (custom path or default)
    ///
    /// # Errors
    ///
    /// Returns an error if the state directory cannot be created or the file cannot be written
    pub fn save(&mut self) -> Result<()> {
        let path = self.state_path.clone().unwrap_or_else(Config::state_path);
        self.save_to(&path)
    }

    /// Save state to a specific path
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be written
    pub fn save_to(&mut self, path: &Path) -> Result<()> {
        let path = resolve_state_path(path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).context(format!(
                "Failed to create state directory {}",
                parent.display()
            ))?;
        }

        let lock_path = lock_state_path(&path);
        let lock_file = fs::OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
            .context(format!("Failed to open state lock {}", lock_path.display()))?;
        lock_file
            .lock_exclusive()
            .context(format!("Failed to lock state {}", lock_path.display()))?;

        let disk = if path.exists()
            && fs::metadata(&path)
                .ok()
                .is_some_and(|metadata| metadata.len() == 0)
        {
            Self::new()
        } else if path.exists() {
            match Self::load_from(&path) {
                Ok(storage) => storage,
                Err(err) => {
                    let backup_path = backup_state_path(&path);
                    if backup_path.exists() {
                        warn!(
                            error = %err,
                            path = %path.display(),
                            backup_path = %backup_path.display(),
                            "Failed to load state file while saving; attempting backup"
                        );
                        Self::load_from(&backup_path)?
                    } else {
                        warn!(
                            error = %err,
                            path = %path.display(),
                            "Failed to load state file while saving; overwriting with in-memory state"
                        );
                        Self::new()
                    }
                }
            }
        } else {
            Self::new()
        };

        let baseline = self
            .last_loaded
            .clone()
            .unwrap_or_else(|| StorageSnapshot::capture(&disk));

        let mut merged = merge_storage_three_way(&baseline, &disk, self);
        merged.apply_local_agent_fields_from(self);
        let contents =
            serde_json::to_string_pretty(&merged).context("Failed to serialize state")?;

        // Write atomically to avoid corrupting the state file if we're interrupted mid-write.
        write_state_atomically(&path, &contents)?;

        let custom_path = self.state_path.clone();
        *self = merged;
        self.state_path = custom_path;
        self.last_loaded = Some(StorageSnapshot::capture(self));

        Ok(())
    }

    pub(crate) fn resolved_state_path(&self) -> std::path::PathBuf {
        let configured = self.state_path.clone().unwrap_or_else(Config::state_path);
        resolve_state_path(&configured)
    }

    pub(crate) fn apply_local_agent_fields_from(&mut self, other: &Self) {
        let collapsed_by_id: HashMap<Uuid, bool> = other
            .agents
            .iter()
            .map(|agent| (agent.id, agent.collapsed))
            .collect();

        for agent in &mut self.agents {
            if let Some(collapsed) = collapsed_by_id.get(&agent.id) {
                agent.collapsed = *collapsed;
            }
        }
    }

    /// Add a new agent
    pub fn add(&mut self, agent: Agent) {
        self.agents.push(agent);
    }

    /// Remove an agent by ID
    pub fn remove(&mut self, id: Uuid) -> Option<Agent> {
        let removed = self
            .agents
            .iter()
            .position(|a| a.id == id)
            .map(|pos| self.agents.remove(pos));

        if removed.is_some() && self.agents.is_empty() {
            self.mux_socket = None;
        }

        removed
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

    /// Set the collapsed state of an agent by ID.
    ///
    /// Returns `true` when the agent existed and was updated.
    pub fn set_collapsed(&mut self, id: Uuid, collapsed: bool) -> bool {
        let Some(agent) = self.get_mut(id) else {
            return false;
        };
        agent.collapsed = collapsed;
        true
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
        self.mux_socket = None;
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

    /// Get visible agents with pre-computed child info for efficient rendering.
    /// This computes child counts in O(n) once, then builds the visible list.
    /// Use this for rendering instead of calling `has_children`/`child_count` per agent.
    #[must_use]
    pub fn visible_agents_with_info(&self) -> Vec<VisibleAgentInfo<'_>> {
        // Build child count map in single O(n) pass
        let mut child_counts: HashMap<Uuid, usize> = HashMap::new();
        for agent in &self.agents {
            if let Some(parent_id) = agent.parent_id {
                *child_counts.entry(parent_id).or_insert(0) += 1;
            }
        }

        // Build children lookup for efficient tree traversal
        let mut children_map: HashMap<Uuid, Vec<&Agent>> = HashMap::new();
        for agent in &self.agents {
            if let Some(parent_id) = agent.parent_id {
                children_map.entry(parent_id).or_default().push(agent);
            }
        }

        // Collect root agents
        let roots: Vec<&Agent> = self.agents.iter().filter(|a| a.is_root()).collect();

        // Build visible list with pre-computed info
        let mut result = Vec::new();
        for root in roots {
            self.add_visible_with_info_recursive(
                root,
                0,
                &child_counts,
                &children_map,
                &mut result,
            );
        }
        result
    }

    /// Helper to recursively add visible agents with pre-computed info
    #[expect(
        clippy::only_used_in_recursion,
        reason = "&self is needed for lifetime 'a to tie result to Storage"
    )]
    fn add_visible_with_info_recursive<'a>(
        &'a self,
        agent: &'a Agent,
        depth: usize,
        child_counts: &HashMap<Uuid, usize>,
        children_map: &HashMap<Uuid, Vec<&'a Agent>>,
        result: &mut Vec<VisibleAgentInfo<'a>>,
    ) {
        let child_count = child_counts.get(&agent.id).copied().unwrap_or(0);
        result.push(VisibleAgentInfo {
            agent,
            depth,
            has_children: child_count > 0,
            child_count,
        });

        if !agent.collapsed
            && let Some(children) = children_map.get(&agent.id)
        {
            for child in children {
                self.add_visible_with_info_recursive(
                    child,
                    depth + 1,
                    child_counts,
                    children_map,
                    result,
                );
            }
        }
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

    /// Reserve multiple consecutive window indices for batch spawning.
    /// Returns the starting index; use indices start..start+count.
    /// This is O(n) once instead of O(n*count) for repeated `next_window_index` calls.
    #[must_use]
    pub fn reserve_window_indices(&self, root_id: Uuid) -> u32 {
        // Window 1 is the root, children start at 2
        let descendants = self.descendants(root_id);
        let max_index = descendants
            .iter()
            .filter_map(|a| a.window_index)
            .max()
            .unwrap_or(1);
        max_index + 1
        // Caller should use indices: start_index, start_index+1, ..., start_index+count-1
    }
}

fn merge_storage_three_way(baseline: &StorageSnapshot, disk: &Storage, ours: &Storage) -> Storage {
    let mut merged = disk.clone();
    merged.state_path = None;
    merged.last_loaded = None;

    if ours.version != baseline.version {
        merged.version = ours.version;
    }

    if ours.instance_id != baseline.instance_id {
        merged.instance_id.clone_from(&ours.instance_id);
    }

    if ours.mux_socket != baseline.mux_socket {
        merged.mux_socket.clone_from(&ours.mux_socket);
    }

    let mut ours_by_id: HashMap<Uuid, &Agent> = HashMap::new();
    for agent in &ours.agents {
        ours_by_id.insert(agent.id, agent);
    }

    let deleted_ids: HashSet<Uuid> = baseline
        .agents_by_id
        .keys()
        .copied()
        .filter(|id| !ours_by_id.contains_key(id))
        .collect();
    if !deleted_ids.is_empty() {
        merged
            .agents
            .retain(|agent| !deleted_ids.contains(&agent.id));
    }

    let mut merged_index_by_id: HashMap<Uuid, usize> = HashMap::new();
    for (idx, agent) in merged.agents.iter().enumerate() {
        merged_index_by_id.insert(agent.id, idx);
    }

    for (id, ours_agent) in &ours_by_id {
        let Some(baseline_agent) = baseline.agents_by_id.get(id) else {
            continue;
        };
        let Some(index) = merged_index_by_id.get(id).copied() else {
            continue;
        };

        let disk_agent = &mut merged.agents[index];
        apply_agent_changes(disk_agent, baseline_agent, ours_agent);
    }

    for agent in &ours.agents {
        if baseline.agents_by_id.contains_key(&agent.id) {
            continue;
        }
        if merged_index_by_id.contains_key(&agent.id) {
            continue;
        }

        merged_index_by_id.insert(agent.id, merged.agents.len());
        merged.agents.push(agent.clone());
    }

    if merged.agents.is_empty() {
        merged.mux_socket = None;
    }

    merged
}

fn apply_agent_changes(target: &mut Agent, baseline: &Agent, ours: &Agent) {
    if ours.title != baseline.title {
        target.title.clone_from(&ours.title);
    }
    if ours.program != baseline.program {
        target.program.clone_from(&ours.program);
    }
    if ours.conversation_id != baseline.conversation_id {
        target.conversation_id.clone_from(&ours.conversation_id);
    }
    if ours.status != baseline.status {
        target.status = ours.status;
    }
    if ours.branch != baseline.branch {
        target.branch.clone_from(&ours.branch);
    }
    if ours.worktree_path != baseline.worktree_path {
        target.worktree_path.clone_from(&ours.worktree_path);
    }
    if ours.repo_root != baseline.repo_root {
        target.repo_root.clone_from(&ours.repo_root);
    }
    if ours.workspace_kind != baseline.workspace_kind {
        target.workspace_kind = ours.workspace_kind;
    }
    if ours.runtime != baseline.runtime {
        target.runtime = ours.runtime;
    }
    if ours.runtime_scope != baseline.runtime_scope {
        target.runtime_scope.clone_from(&ours.runtime_scope);
    }
    if ours.mux_session != baseline.mux_session {
        target.mux_session.clone_from(&ours.mux_session);
    }
    if ours.created_at != baseline.created_at {
        target.created_at = ours.created_at;
    }
    if ours.updated_at != baseline.updated_at {
        target.updated_at = ours.updated_at;
    }
    if ours.parent_id != baseline.parent_id {
        target.parent_id = ours.parent_id;
    }
    if ours.window_index != baseline.window_index {
        target.window_index = ours.window_index;
    }
    if ours.is_terminal != baseline.is_terminal {
        target.is_terminal = ours.is_terminal;
    }
}
