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
