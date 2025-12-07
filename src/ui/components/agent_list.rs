//! Agent list widget

use crate::agent::{Agent, Status};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem},
};

/// Widget for displaying the agent list
#[derive(Debug)]
pub struct Widget<'a> {
    agents: &'a [Agent],
    selected: usize,
    title: String,
}

impl<'a> Widget<'a> {
    /// Create a new agent list widget
    #[must_use]
    pub fn new(agents: &'a [Agent], selected: usize) -> Self {
        Self {
            agents,
            selected,
            title: format!(" Agents ({}) ", agents.len()),
        }
    }

    /// Set a custom title
    #[must_use]
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// Convert to a List widget
    #[must_use]
    pub fn to_list(&self) -> List<'a> {
        let items: Vec<ListItem<'_>> = self
            .agents
            .iter()
            .enumerate()
            .map(|(i, agent)| self.render_item(i, agent))
            .collect();

        List::new(items)
            .block(
                Block::default()
                    .title(self.title.clone())
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
            )
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
    }

    fn render_item(&self, index: usize, agent: &Agent) -> ListItem<'a> {
        let status_color = status_to_color(agent.status);

        let style = if index == self.selected {
            Style::default()
                .fg(Color::Black)
                .bg(Color::White)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };

        let content = Line::from(vec![
            Span::styled(
                format!("{} ", agent.status.symbol()),
                Style::default().fg(status_color),
            ),
            Span::styled(agent.title.clone(), style),
            Span::styled(
                format!(" ({})", agent.age_string()),
                Style::default().fg(Color::DarkGray),
            ),
        ]);

        ListItem::new(content).style(style)
    }
}

/// Convert agent status to a display color
#[must_use]
pub const fn status_to_color(status: Status) -> Color {
    match status {
        Status::Starting => Color::Yellow,
        Status::Running => Color::Green,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn create_test_agent(title: &str, status: Status) -> Agent {
        let mut agent = Agent::new(
            title.to_string(),
            "claude".to_string(),
            format!("tenex/{title}"),
            PathBuf::from("/tmp"),
            None,
        );
        agent.set_status(status);
        agent
    }

    #[test]
    fn test_agent_list_widget_new() {
        let agents = vec![
            create_test_agent("agent1", Status::Running),
            create_test_agent("agent2", Status::Starting),
        ];

        let widget = Widget::new(&agents, 0);
        assert!(widget.title.contains('2'));
    }

    #[test]
    fn test_agent_list_widget_title() {
        let agents = vec![create_test_agent("test", Status::Running)];
        let widget = Widget::new(&agents, 0).title("Custom Title");
        assert_eq!(widget.title, "Custom Title");
    }

    #[test]
    fn test_status_to_color() {
        assert_eq!(status_to_color(Status::Starting), Color::Yellow);
        assert_eq!(status_to_color(Status::Running), Color::Green);
    }

    #[test]
    fn test_to_list() {
        let agents = vec![create_test_agent("test", Status::Running)];
        let widget = Widget::new(&agents, 0);
        let _list = widget.to_list();
    }
}
