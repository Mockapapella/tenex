//! Text input mode key handling
//!
//! Handles key events for modes that involve text input:
//! - `Creating` (new agent name)
//! - `Prompting` (new agent with prompt)
//! - `ChildPrompt` (task for children)
//! - `Broadcasting` (message to leaves)
//! - `ReconnectPrompt` (reconnect with edited prompt)
//! - `TerminalPrompt` (terminal startup command)
//! - `SynthesisPrompt` (extra synthesis instructions)

use crate::app::App;
use crate::state::AppMode;
use anyhow::Result;
use ratatui::crossterm::event::{KeyCode, KeyModifiers};

/// Handle key events in text input modes
pub fn handle_text_input_mode(app: &mut App, code: KeyCode, modifiers: KeyModifiers) -> Result<()> {
    match &app.mode {
        AppMode::Creating(_) => crate::action::dispatch_creating_mode(app, code, modifiers)?,
        AppMode::Prompting(_) => crate::action::dispatch_prompting_mode(app, code, modifiers)?,
        AppMode::ChildPrompt(_) => crate::action::dispatch_child_prompt_mode(app, code, modifiers)?,
        AppMode::Broadcasting(_) => {
            crate::action::dispatch_broadcasting_mode(app, code, modifiers)?;
        }
        AppMode::ReconnectPrompt(_) => {
            crate::action::dispatch_reconnect_prompt_mode(app, code, modifiers)?;
        }
        AppMode::TerminalPrompt(_) => {
            crate::action::dispatch_terminal_prompt_mode(app, code, modifiers)?;
        }
        AppMode::CustomAgentCommand(_) => {
            crate::action::dispatch_custom_agent_command_mode(app, code, modifiers)?;
        }
        AppMode::SynthesisPrompt(_) => {
            crate::action::dispatch_synthesis_prompt_mode(app, code, modifiers)?;
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::Storage;
    use crate::agent::WorkspaceKind;
    use crate::app::Settings;
    use crate::config::Config;
    use crate::state::{
        AppMode, BroadcastingMode, ChildPromptMode, CreatingMode, CustomAgentCommandMode,
        PromptingMode, ReconnectPromptMode, SynthesisPromptMode, TerminalPromptMode,
    };
    use tempfile::NamedTempFile;
    use tempfile::TempDir;

    fn create_test_app() -> Result<(App, NamedTempFile), std::io::Error> {
        let temp_file = NamedTempFile::new()?;
        let storage = Storage::with_path(temp_file.path().to_path_buf());
        Ok((
            App::new(Config::default(), storage, Settings::default(), false),
            temp_file,
        ))
    }

    #[test]
    fn test_handle_text_input_char() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.apply_mode(CreatingMode.into());

        handle_text_input_mode(&mut app, KeyCode::Char('a'), KeyModifiers::NONE)?;
        assert_eq!(app.data.input.buffer, "a");

        handle_text_input_mode(&mut app, KeyCode::Char('b'), KeyModifiers::NONE)?;
        assert_eq!(app.data.input.buffer, "ab");

        Ok(())
    }

    #[test]
    fn test_handle_text_input_backspace() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.apply_mode(CreatingMode.into());
        app.data.input.buffer = "test".to_string();
        app.data.input.cursor = 4;

        handle_text_input_mode(&mut app, KeyCode::Backspace, KeyModifiers::NONE)?;
        assert_eq!(app.data.input.buffer, "tes");

        Ok(())
    }

    #[test]
    fn test_handle_text_input_delete() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.apply_mode(CreatingMode.into());
        app.data.input.buffer = "test".to_string();
        app.data.input.cursor = 0;

        handle_text_input_mode(&mut app, KeyCode::Delete, KeyModifiers::NONE)?;
        assert_eq!(app.data.input.buffer, "est");

        Ok(())
    }

    #[test]
    fn test_handle_text_input_cursor_movement() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.apply_mode(CreatingMode.into());
        app.data.input.buffer = "test".to_string();
        app.data.input.cursor = 2;

        handle_text_input_mode(&mut app, KeyCode::Left, KeyModifiers::NONE)?;
        assert_eq!(app.data.input.cursor, 1);

        handle_text_input_mode(&mut app, KeyCode::Right, KeyModifiers::NONE)?;
        assert_eq!(app.data.input.cursor, 2);

        handle_text_input_mode(&mut app, KeyCode::Home, KeyModifiers::NONE)?;
        assert_eq!(app.data.input.cursor, 0);

        handle_text_input_mode(&mut app, KeyCode::End, KeyModifiers::NONE)?;
        assert_eq!(app.data.input.cursor, 4);

        Ok(())
    }

    #[test]
    fn test_handle_text_input_escape() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.apply_mode(CreatingMode.into());
        app.data.input.buffer = "test".to_string();

        handle_text_input_mode(&mut app, KeyCode::Esc, KeyModifiers::NONE)?;
        assert_eq!(app.mode, AppMode::normal());

        Ok(())
    }

    #[test]
    fn test_handle_escape_reconnect_prompt_clears_conflict()
    -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.apply_mode(ReconnectPromptMode.into());
        app.data.spawn.worktree_conflict = Some(crate::app::WorktreeConflictInfo {
            title: "test".to_string(),
            branch: "test".to_string(),
            worktree_path: std::path::PathBuf::from("/tmp"),
            prompt: None,
            existing_branch: None,
            existing_commit: None,
            current_branch: "main".to_string(),
            current_commit: "abc1234".to_string(),
            swarm_child_count: None,
        });

        handle_text_input_mode(&mut app, KeyCode::Esc, KeyModifiers::NONE)?;

        assert!(app.data.spawn.worktree_conflict.is_none());
        assert_eq!(app.mode, AppMode::normal());

        Ok(())
    }

    #[test]
    fn test_handle_enter_empty_creating_exits() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.apply_mode(CreatingMode.into());
        app.data.input.buffer = String::new();

        handle_text_input_mode(&mut app, KeyCode::Enter, KeyModifiers::NONE)?;
        assert_eq!(app.mode, AppMode::normal());

        Ok(())
    }

    #[test]
    fn test_handle_alt_enter_inserts_newline() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.apply_mode(CreatingMode.into());
        app.data.input.buffer = "test".to_string();
        app.data.input.cursor = 4;

        handle_text_input_mode(&mut app, KeyCode::Enter, KeyModifiers::ALT)?;
        assert!(app.data.input.buffer.contains('\n'));

        Ok(())
    }

    #[test]
    fn test_text_input_actions_covered_for_all_modes() -> Result<(), Box<dyn std::error::Error>> {
        let modes: [AppMode; 8] = [
            CreatingMode.into(),
            PromptingMode.into(),
            ChildPromptMode.into(),
            BroadcastingMode.into(),
            ReconnectPromptMode.into(),
            TerminalPromptMode.into(),
            CustomAgentCommandMode.into(),
            SynthesisPromptMode.into(),
        ];

        for mode in modes {
            let is_reconnect_prompt = matches!(mode, AppMode::ReconnectPrompt(_));
            let (mut app, _temp) = create_test_app()?;
            app.apply_mode(mode);
            app.data.input.buffer = "hello world".to_string();
            app.data.input.cursor = app.data.input.buffer.len();

            // Delete word, then clear line.
            handle_text_input_mode(&mut app, KeyCode::Char('w'), KeyModifiers::CONTROL)?;
            handle_text_input_mode(&mut app, KeyCode::Char('u'), KeyModifiers::CONTROL)?;
            assert!(app.data.input.buffer.is_empty());

            // Insert, backspace, delete, and cursor movement.
            handle_text_input_mode(&mut app, KeyCode::Char('a'), KeyModifiers::NONE)?;
            handle_text_input_mode(&mut app, KeyCode::Backspace, KeyModifiers::NONE)?;
            handle_text_input_mode(&mut app, KeyCode::Char('b'), KeyModifiers::NONE)?;
            handle_text_input_mode(&mut app, KeyCode::Delete, KeyModifiers::NONE)?;
            handle_text_input_mode(&mut app, KeyCode::Left, KeyModifiers::NONE)?;
            handle_text_input_mode(&mut app, KeyCode::Right, KeyModifiers::NONE)?;
            handle_text_input_mode(&mut app, KeyCode::Up, KeyModifiers::NONE)?;
            handle_text_input_mode(&mut app, KeyCode::Down, KeyModifiers::NONE)?;
            handle_text_input_mode(&mut app, KeyCode::Home, KeyModifiers::NONE)?;
            handle_text_input_mode(&mut app, KeyCode::End, KeyModifiers::NONE)?;
            handle_text_input_mode(&mut app, KeyCode::F(12), KeyModifiers::NONE)?;

            // Cancel (Esc) returns to Normal and clears any ReconnectPrompt conflict.
            if is_reconnect_prompt {
                app.apply_mode(ReconnectPromptMode.into());
                app.data.spawn.worktree_conflict = Some(crate::app::WorktreeConflictInfo {
                    title: "test".to_string(),
                    branch: "test".to_string(),
                    worktree_path: std::path::PathBuf::from("/tmp"),
                    prompt: None,
                    existing_branch: None,
                    existing_commit: None,
                    current_branch: "main".to_string(),
                    current_commit: "abc1234".to_string(),
                    swarm_child_count: None,
                });
            }

            handle_text_input_mode(&mut app, KeyCode::Esc, KeyModifiers::NONE)?;
            assert_eq!(app.mode, AppMode::normal());
            assert!(app.data.spawn.worktree_conflict.is_none());
        }

        Ok(())
    }

    #[test]
    fn test_text_input_submit_error_paths() -> Result<(), Box<dyn std::error::Error>> {
        struct RestoreCwd(std::path::PathBuf);

        impl Drop for RestoreCwd {
            fn drop(&mut self) {
                let _ = std::env::set_current_dir(&self.0);
            }
        }

        let original_dir = std::env::current_dir()?;
        let _guard = RestoreCwd(original_dir);
        let non_git_dir = TempDir::new()?;
        std::env::set_current_dir(non_git_dir.path())?;

        // CreatingMode should succeed by creating a plain-directory agent.
        {
            let (mut app, _temp) = create_test_app()?;
            app.apply_mode(CreatingMode.into());
            app.data.input.buffer = "agent".to_string();
            app.data.input.cursor = app.data.input.buffer.len();
            handle_text_input_mode(&mut app, KeyCode::Enter, KeyModifiers::NONE)?;
            assert_eq!(app.mode, AppMode::normal());
            let agent = app.data.storage.iter().next().ok_or("Missing agent")?;
            assert_eq!(agent.workspace_kind, WorkspaceKind::PlainDir);
            let _ = crate::mux::SessionManager::new().kill(&agent.mux_session);
        }

        // PromptingMode should also succeed outside git.
        {
            let (mut app, _temp) = create_test_app()?;
            app.apply_mode(PromptingMode.into());
            app.data.input.buffer = "prompt".to_string();
            app.data.input.cursor = app.data.input.buffer.len();
            handle_text_input_mode(&mut app, KeyCode::Enter, KeyModifiers::NONE)?;
            assert_eq!(app.mode, AppMode::normal());
            let agent = app.data.storage.iter().next().ok_or("Missing agent")?;
            assert_eq!(agent.workspace_kind, WorkspaceKind::PlainDir);
            let _ = crate::mux::SessionManager::new().kill(&agent.mux_session);
        }

        // ChildPromptMode: force an error by pointing to a missing parent agent.
        {
            let (mut app, _temp) = create_test_app()?;
            app.data.spawn.spawning_under = Some(uuid::Uuid::new_v4());
            app.apply_mode(ChildPromptMode.into());
            app.data.input.buffer = "task".to_string();
            app.data.input.cursor = app.data.input.buffer.len();
            handle_text_input_mode(&mut app, KeyCode::Enter, KeyModifiers::NONE)?;
            assert!(matches!(app.mode, AppMode::ErrorModal(_)));
        }

        // BroadcastingMode: errors without a selected agent.
        {
            let (mut app, _temp) = create_test_app()?;
            app.apply_mode(BroadcastingMode.into());
            app.data.input.buffer = "msg".to_string();
            app.data.input.cursor = app.data.input.buffer.len();
            handle_text_input_mode(&mut app, KeyCode::Enter, KeyModifiers::NONE)?;
            assert!(matches!(app.mode, AppMode::ErrorModal(_)));
        }

        // ReconnectPromptMode: errors without conflict info.
        {
            let (mut app, _temp) = create_test_app()?;
            app.apply_mode(ReconnectPromptMode.into());
            app.data.input.buffer = "new prompt".to_string();
            app.data.input.cursor = app.data.input.buffer.len();
            handle_text_input_mode(&mut app, KeyCode::Enter, KeyModifiers::NONE)?;
            assert!(matches!(app.mode, AppMode::ErrorModal(_)));
        }

        // TerminalPromptMode: errors without a selected agent.
        {
            let (mut app, _temp) = create_test_app()?;
            app.apply_mode(TerminalPromptMode.into());
            app.data.input.buffer = "echo hi".to_string();
            app.data.input.cursor = app.data.input.buffer.len();
            handle_text_input_mode(&mut app, KeyCode::Enter, KeyModifiers::NONE)?;
            assert!(matches!(app.mode, AppMode::ErrorModal(_)));
        }

        // Custom agent command: empty input should stay in mode and set a status message.
        {
            let (mut app, _temp) = create_test_app()?;
            app.apply_mode(CustomAgentCommandMode.into());
            app.data.input.buffer = "   ".to_string();
            app.data.input.cursor = app.data.input.buffer.len();
            handle_text_input_mode(&mut app, KeyCode::Enter, KeyModifiers::NONE)?;
            assert_eq!(app.mode, CustomAgentCommandMode.into());
        }

        Ok(())
    }
}
