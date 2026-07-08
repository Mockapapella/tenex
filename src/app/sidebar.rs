use crate::agent::{Agent, VisibleAgentInfo};
use crate::app::AppData;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct SidebarProject {
    pub root: PathBuf,
    pub label: String,
    pub collapsed: bool,
    pub agent_count: usize,
}

#[derive(Debug, Clone)]
pub struct SidebarAgentInfo<'a> {
    pub info: VisibleAgentInfo<'a>,
    pub synthesis_marked: bool,
    pub marked_descendant_count: usize,
}

#[derive(Debug, Clone)]
pub enum SidebarItem<'a> {
    Project(SidebarProject),
    Agent(SidebarAgentInfo<'a>),
}

fn agent_project_root(agent: &Agent) -> &Path {
    agent
        .repo_root
        .as_deref()
        .unwrap_or(agent.worktree_path.as_path())
}

fn project_base_name(root: &Path) -> String {
    root.file_name()
        .and_then(|name| name.to_str())
        .map_or_else(|| root.to_string_lossy().to_string(), str::to_string)
}

fn project_label_for_root(root: &Path, name_counts: &HashMap<String, usize>) -> String {
    let base = project_base_name(root);

    if name_counts.get(&base).copied().unwrap_or(0) <= 1 {
        return base;
    }

    let parent = root
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|name| name.to_str())
        .unwrap_or("");

    if parent.is_empty() {
        return root.to_string_lossy().to_string();
    }

    format!("{parent}/{base}")
}

impl AppData {
    pub(crate) fn sidebar_items(&self) -> Vec<SidebarItem<'_>> {
        let mut child_counts: HashMap<Uuid, usize> = HashMap::new();
        let mut children_map: HashMap<Uuid, Vec<&Agent>> = HashMap::new();
        let marked_descendant_counts = self.marked_synthesis_descendant_counts();

        let mut roots_in_order: Vec<&Agent> = Vec::new();
        for agent in &self.storage.agents {
            if agent.is_root() {
                roots_in_order.push(agent);
            }

            if let Some(parent_id) = agent.parent_id {
                *child_counts.entry(parent_id).or_insert(0) += 1;
                children_map.entry(parent_id).or_default().push(agent);
            }
        }

        let mut project_order: Vec<PathBuf> = Vec::new();
        let mut roots_by_project: HashMap<PathBuf, Vec<&Agent>> = HashMap::new();
        let mut agent_counts_by_project: HashMap<PathBuf, usize> = HashMap::new();

        for agent in &self.storage.agents {
            let root = agent_project_root(agent).to_path_buf();
            *agent_counts_by_project.entry(root).or_insert(0) += 1;
        }

        for root in roots_in_order {
            let project_root = agent_project_root(root).to_path_buf();
            if !roots_by_project.contains_key(&project_root) {
                project_order.push(project_root.clone());
            }
            roots_by_project.entry(project_root).or_default().push(root);
        }

        if let Some(cwd_root) = self.cwd_project_root.clone() {
            if !project_order.contains(&cwd_root) {
                project_order.push(cwd_root.clone());
            }
            agent_counts_by_project.entry(cwd_root).or_insert(0);
        }

        let mut name_counts: HashMap<String, usize> = HashMap::new();
        for project_root in &project_order {
            let base = project_base_name(project_root);
            *name_counts.entry(base).or_insert(0) += 1;
        }

        let mut project_order = project_order
            .into_iter()
            .map(|project_root| {
                let label = project_label_for_root(&project_root, &name_counts);
                let sort_key = label.to_lowercase();
                (sort_key, label, project_root)
            })
            .collect::<Vec<_>>();
        project_order.sort_by(|(a_key, a_label, a_root), (b_key, b_label, b_root)| {
            a_key
                .cmp(b_key)
                .then_with(|| a_label.cmp(b_label))
                .then_with(|| a_root.cmp(b_root))
        });

        let mut result: Vec<SidebarItem<'_>> = Vec::new();

        for (_, label, project_root) in project_order {
            let collapsed = self.ui.collapsed_projects.contains(&project_root);
            let agent_count = agent_counts_by_project
                .get(&project_root)
                .copied()
                .unwrap_or(0);

            result.push(SidebarItem::Project(SidebarProject {
                root: project_root.clone(),
                label,
                collapsed,
                agent_count,
            }));

            if collapsed {
                continue;
            }

            let Some(project_roots) = roots_by_project.get(&project_root) else {
                continue;
            };

            for root_agent in project_roots {
                add_visible_with_info_recursive(
                    root_agent,
                    1,
                    &child_counts,
                    &children_map,
                    &self.synthesis_marks,
                    &marked_descendant_counts,
                    &mut result,
                );
            }
        }

        result
    }

    pub(crate) fn sidebar_len(&self) -> usize {
        self.sidebar_items().len()
    }

    pub(crate) fn selected_sidebar_item(&self) -> Option<SidebarItem<'_>> {
        self.sidebar_items().get(self.selected).cloned()
    }

    pub(crate) fn selected_project_root(&self) -> Option<PathBuf> {
        match self.selected_sidebar_item()? {
            SidebarItem::Project(project) => Some(project.root),
            SidebarItem::Agent(agent) => Some(agent_project_root(agent.info.agent).to_path_buf()),
        }
    }
}

fn add_visible_with_info_recursive<'a>(
    agent: &'a Agent,
    depth: usize,
    child_counts: &HashMap<Uuid, usize>,
    children_map: &HashMap<Uuid, Vec<&'a Agent>>,
    synthesis_marks: &[Uuid],
    marked_descendant_counts: &HashMap<Uuid, usize>,
    result: &mut Vec<SidebarItem<'a>>,
) {
    let child_count = child_counts.get(&agent.id).copied().unwrap_or(0);
    result.push(SidebarItem::Agent(SidebarAgentInfo {
        info: VisibleAgentInfo {
            agent,
            depth,
            has_children: child_count > 0,
            child_count,
        },
        synthesis_marked: synthesis_marks.contains(&agent.id),
        marked_descendant_count: if agent.collapsed {
            marked_descendant_counts
                .get(&agent.id)
                .copied()
                .unwrap_or(0)
        } else {
            0
        },
    }));

    if !agent.collapsed
        && let Some(children) = children_map.get(&agent.id)
    {
        for child in children {
            add_visible_with_info_recursive(
                child,
                depth + 1,
                child_counts,
                children_map,
                synthesis_marks,
                marked_descendant_counts,
                result,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{ChildConfig, Storage};
    use crate::app::Settings;
    use crate::config::Config;

    fn project_info(item: &SidebarItem<'_>) -> Option<(PathBuf, String, bool, usize)> {
        match item {
            SidebarItem::Project(project) => Some((
                project.root.clone(),
                project.label.clone(),
                project.collapsed,
                project.agent_count,
            )),
            SidebarItem::Agent(_) => None,
        }
    }

    fn agent_title(item: &SidebarItem<'_>) -> Option<String> {
        match item {
            SidebarItem::Agent(agent) => Some(agent.info.agent.title.clone()),
            SidebarItem::Project(_) => None,
        }
    }

    fn agent_info_by_title<'a>(
        items: &'a [SidebarItem<'a>],
        title: &str,
    ) -> Option<&'a SidebarAgentInfo<'a>> {
        items.iter().find_map(|item| match item {
            SidebarItem::Agent(agent) if agent.info.agent.title == title => Some(agent),
            SidebarItem::Agent(_) | SidebarItem::Project(_) => None,
        })
    }

    #[test]
    fn test_sidebar_items_groups_by_repo_root_and_respects_collapsed_projects() {
        let mut app_data = AppData::new(
            Config::default(),
            Storage::new(),
            Settings::default(),
            false,
        );

        let repo_a = PathBuf::from("/tmp/repo-a");
        let repo_b = PathBuf::from("/tmp/repo-b");

        let mut root_a = Agent::new(
            "root-a".to_string(),
            "echo".to_string(),
            "branch-a".to_string(),
            PathBuf::from("/tmp/repo-a-wt"),
        );
        root_a.repo_root = Some(repo_a.clone());
        root_a.collapsed = false;
        let root_a_id = root_a.id;
        app_data.storage.add(root_a);

        let child_a = Agent::new_child(
            "child-a".to_string(),
            "echo".to_string(),
            "branch-a".to_string(),
            PathBuf::from("/tmp/repo-a-wt"),
            ChildConfig {
                parent_id: root_a_id,
                mux_session: "tenex-test".to_string(),
                window_index: 2,
                repo_root: Some(repo_a.clone()),
            },
        );
        app_data.storage.add(child_a);

        let mut root_b = Agent::new(
            "root-b".to_string(),
            "echo".to_string(),
            "branch-b".to_string(),
            PathBuf::from("/tmp/repo-b-wt"),
        );
        root_b.repo_root = Some(repo_b.clone());
        app_data.storage.add(root_b);

        let items = app_data.sidebar_items();
        assert_eq!(items.len(), 5);

        let (root, label, collapsed, agent_count) = project_info(&items[0]).unwrap();
        assert_eq!(root, repo_a);
        assert_eq!(label, "repo-a");
        assert!(!collapsed);
        assert_eq!(agent_count, 2);
        assert!(agent_title(&items[0]).is_none());

        assert!(project_info(&items[1]).is_none());
        assert_eq!(agent_title(&items[1]).as_deref(), Some("root-a"));

        assert!(project_info(&items[2]).is_none());
        assert_eq!(agent_title(&items[2]).as_deref(), Some("child-a"));

        let (root, label, _collapsed, agent_count) = project_info(&items[3]).unwrap();
        assert_eq!(root, repo_b);
        assert_eq!(label, "repo-b");
        assert_eq!(agent_count, 1);

        app_data.ui.collapsed_projects.insert(repo_a);
        let items = app_data.sidebar_items();
        assert_eq!(items.len(), 3);

        let (_root, _label, collapsed, agent_count) = project_info(&items[0]).unwrap();
        assert!(collapsed);
        assert_eq!(agent_count, 2);
    }

    #[test]
    fn test_project_headers_disambiguate_duplicate_base_names() {
        let mut app_data = AppData::new(
            Config::default(),
            Storage::new(),
            Settings::default(),
            false,
        );

        let repo_one = PathBuf::from("/tmp/one/repo");
        let repo_two = PathBuf::from("/tmp/two/repo");

        let mut root_two = Agent::new(
            "two".to_string(),
            "echo".to_string(),
            "branch-two".to_string(),
            PathBuf::from("/tmp/two/repo-wt"),
        );
        root_two.repo_root = Some(repo_two);
        app_data.storage.add(root_two);

        let mut root_one = Agent::new(
            "one".to_string(),
            "echo".to_string(),
            "branch-one".to_string(),
            PathBuf::from("/tmp/one/repo-wt"),
        );
        root_one.repo_root = Some(repo_one);
        app_data.storage.add(root_one);

        let labels: Vec<String> = app_data
            .sidebar_items()
            .into_iter()
            .filter_map(|item| match item {
                SidebarItem::Project(project) => Some(project.label),
                SidebarItem::Agent(_) => None,
            })
            .collect();

        assert_eq!(labels, vec!["one/repo".to_string(), "two/repo".to_string()]);
    }

    #[test]
    fn test_project_label_for_root_uses_full_path_when_parent_empty() {
        let root = std::path::Path::new("/");
        let base = project_base_name(root);
        let mut counts = std::collections::HashMap::new();
        counts.insert(base, 2);

        let label = project_label_for_root(root, &counts);
        assert_eq!(label, "/");
    }

    #[test]
    fn test_sidebar_items_expose_synthesis_mark_state() {
        let mut app_data = AppData::new(
            Config::default(),
            Storage::new(),
            Settings::default(),
            false,
        );

        let mut root = Agent::new(
            "root".to_string(),
            "echo".to_string(),
            "branch".to_string(),
            PathBuf::from("/tmp/repo-wt"),
        );
        root.repo_root = Some(PathBuf::from("/tmp/repo"));
        root.collapsed = false;
        let root_id = root.id;
        let root_session = root.mux_session.clone();
        app_data.storage.add(root);

        let child = Agent::new_child(
            "child".to_string(),
            "echo".to_string(),
            "branch".to_string(),
            PathBuf::from("/tmp/repo-wt"),
            ChildConfig {
                parent_id: root_id,
                mux_session: root_session,
                window_index: 2,
                repo_root: Some(PathBuf::from("/tmp/repo")),
            },
        );
        let child_id = child.id;
        app_data.storage.add(child);
        assert!(app_data.toggle_synthesis_mark(child_id));

        let items = app_data.sidebar_items();
        let child_info = agent_info_by_title(&items, "child").expect("missing child row");
        assert!(child_info.synthesis_marked);
        let root_info = agent_info_by_title(&items, "root").expect("missing root row");
        assert_eq!(root_info.marked_descendant_count, 0);
    }

    #[test]
    fn test_sidebar_items_count_hidden_synthesis_marks_on_collapsed_parent() {
        let mut app_data = AppData::new(
            Config::default(),
            Storage::new(),
            Settings::default(),
            false,
        );

        let mut root = Agent::new(
            "root".to_string(),
            "echo".to_string(),
            "branch".to_string(),
            PathBuf::from("/tmp/repo-wt"),
        );
        root.repo_root = Some(PathBuf::from("/tmp/repo"));
        root.collapsed = false;
        let root_id = root.id;
        let root_session = root.mux_session.clone();
        app_data.storage.add(root);

        let mut child = Agent::new_child(
            "child".to_string(),
            "echo".to_string(),
            "branch".to_string(),
            PathBuf::from("/tmp/repo-wt"),
            ChildConfig {
                parent_id: root_id,
                mux_session: root_session.clone(),
                window_index: 2,
                repo_root: Some(PathBuf::from("/tmp/repo")),
            },
        );
        child.collapsed = false;
        let child_id = child.id;
        app_data.storage.add(child);

        let grandchild = Agent::new_child(
            "grandchild".to_string(),
            "echo".to_string(),
            "branch".to_string(),
            PathBuf::from("/tmp/repo-wt"),
            ChildConfig {
                parent_id: child_id,
                mux_session: root_session,
                window_index: 3,
                repo_root: Some(PathBuf::from("/tmp/repo")),
            },
        );
        let grandchild_id = grandchild.id;
        app_data.storage.add(grandchild);

        assert!(app_data.toggle_synthesis_mark(grandchild_id));
        app_data
            .storage
            .get_mut(child_id)
            .expect("missing child")
            .collapsed = true;

        let items = app_data.sidebar_items();
        assert!(agent_info_by_title(&items, "grandchild").is_none());
        let child_info = agent_info_by_title(&items, "child").expect("missing child row");
        assert_eq!(child_info.marked_descendant_count, 1);
        let root_info = agent_info_by_title(&items, "root").expect("missing root row");
        assert_eq!(root_info.marked_descendant_count, 0);
    }

    #[test]
    fn test_project_headers_are_sorted_alphabetically() {
        let mut app_data = AppData::new(
            Config::default(),
            Storage::new(),
            Settings::default(),
            false,
        );

        let repo_a = PathBuf::from("/tmp/repo-a");
        let repo_b = PathBuf::from("/tmp/repo-b");

        let mut root_b = Agent::new(
            "root-b".to_string(),
            "echo".to_string(),
            "branch-b".to_string(),
            PathBuf::from("/tmp/repo-b-wt"),
        );
        root_b.repo_root = Some(repo_b);
        app_data.storage.add(root_b);

        let mut root_a = Agent::new(
            "root-a".to_string(),
            "echo".to_string(),
            "branch-a".to_string(),
            PathBuf::from("/tmp/repo-a-wt"),
        );
        root_a.repo_root = Some(repo_a);
        app_data.storage.add(root_a);

        let labels: Vec<String> = app_data
            .sidebar_items()
            .into_iter()
            .filter_map(|item| match item {
                SidebarItem::Project(project) => Some(project.label),
                SidebarItem::Agent(_) => None,
            })
            .collect();

        assert_eq!(labels, vec!["repo-a".to_string(), "repo-b".to_string()]);
    }

    #[test]
    fn test_project_header_sort_uses_label_then_root_as_tiebreaker() {
        let mut app_data = AppData::new(
            Config::default(),
            Storage::new(),
            Settings::default(),
            false,
        );

        let first_root = PathBuf::from("/tmp/one/repo");
        let second_root = PathBuf::from("/var/one/repo");

        let mut second_agent = Agent::new(
            "second".to_string(),
            "echo".to_string(),
            "branch-second".to_string(),
            PathBuf::from("/var/one/repo-wt"),
        );
        second_agent.repo_root = Some(second_root.clone());
        app_data.storage.add(second_agent);

        let mut first_agent = Agent::new(
            "first".to_string(),
            "echo".to_string(),
            "branch-first".to_string(),
            PathBuf::from("/tmp/one/repo-wt"),
        );
        first_agent.repo_root = Some(first_root.clone());
        app_data.storage.add(first_agent);

        let roots: Vec<PathBuf> = app_data
            .sidebar_items()
            .into_iter()
            .filter_map(|item| match item {
                SidebarItem::Project(project) => Some(project.root),
                SidebarItem::Agent(_) => None,
            })
            .collect();

        assert_eq!(roots, vec![first_root, second_root]);
    }

    #[test]
    fn test_project_headers_fall_back_to_worktree_path() {
        let mut app_data = AppData::new(
            Config::default(),
            Storage::new(),
            Settings::default(),
            false,
        );

        app_data.storage.add(Agent::new(
            "root".to_string(),
            "echo".to_string(),
            "branch".to_string(),
            PathBuf::from("/tmp/fallback-worktree"),
        ));

        let items = app_data.sidebar_items();
        for (label, expected) in [
            ("fallback-worktree", true),
            ("not-fallback-worktree", false),
        ] {
            assert_eq!(
                matches!(
                    &items[0],
                    SidebarItem::Project(project) if project.label == label
                ),
                expected
            );
        }
    }
}
