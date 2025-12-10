//! Color palette for the TUI
//!
//! Modern color palette - cohesive, muted colors for a clean look

use ratatui::style::Color;

// UI Chrome
pub const BORDER: Color = Color::Rgb(100, 110, 130);
pub const SELECTED: Color = Color::Rgb(100, 180, 220);
pub const SURFACE: Color = Color::Rgb(30, 32, 40);
pub const SURFACE_HIGHLIGHT: Color = Color::Rgb(50, 55, 70);

// Text
pub const TEXT_PRIMARY: Color = Color::Rgb(220, 220, 230);
pub const TEXT_DIM: Color = Color::Rgb(130, 135, 150);
pub const TEXT_MUTED: Color = Color::Rgb(90, 95, 110);

// Status (semantic)
pub const STATUS_RUNNING: Color = Color::Rgb(120, 180, 120);
pub const STATUS_STARTING: Color = Color::Rgb(200, 180, 100);

// Diff
pub const DIFF_ADD: Color = Color::Rgb(120, 180, 120);
pub const DIFF_REMOVE: Color = Color::Rgb(200, 100, 100);
pub const DIFF_HUNK: Color = Color::Rgb(100, 140, 200);

// Modals
pub const MODAL_BG: Color = Color::Rgb(25, 27, 35);
pub const MODAL_BORDER_WARNING: Color = Color::Rgb(200, 160, 80);
pub const MODAL_BORDER_ERROR: Color = Color::Rgb(200, 100, 100);
pub const INPUT_BG: Color = Color::Rgb(35, 40, 50);

// Accent (for confirmations)
pub const ACCENT_POSITIVE: Color = Color::Rgb(120, 180, 120);
pub const ACCENT_NEGATIVE: Color = Color::Rgb(200, 100, 100);
pub const ACCENT_WARNING: Color = Color::Rgb(200, 160, 80);
