//! Color palette for the TUI
//!
//! `StarCraft` II-inspired sci-fi palette: dark metal surfaces with cyan glow accents.

use ratatui::{style::Color, widgets::BorderType};

/// Border style used across the app.
pub const BORDER_TYPE: BorderType = BorderType::Double;

// UI Chrome
pub const BORDER: Color = Color::Rgb(58, 94, 132);
pub const SELECTED: Color = Color::Rgb(0, 200, 255);
pub const SURFACE: Color = Color::Rgb(10, 16, 24);
pub const SURFACE_HIGHLIGHT: Color = Color::Rgb(18, 36, 58);

// Text
pub const TEXT_PRIMARY: Color = Color::Rgb(220, 238, 255);
pub const TEXT_DIM: Color = Color::Rgb(140, 170, 200);
pub const TEXT_MUTED: Color = Color::Rgb(100, 122, 150);

// Status (semantic)
pub const STATUS_RUNNING: Color = Color::Rgb(0, 220, 140);
pub const STATUS_STARTING: Color = Color::Rgb(255, 200, 60);

// Diff
pub const DIFF_ADD: Color = Color::Rgb(0, 200, 120);
pub const DIFF_REMOVE: Color = Color::Rgb(255, 90, 90);
pub const DIFF_HUNK: Color = Color::Rgb(0, 170, 255);

// Modals
pub const MODAL_BG: Color = Color::Rgb(8, 12, 18);
pub const MODAL_BORDER_WARNING: Color = Color::Rgb(255, 180, 60);
pub const MODAL_BORDER_ERROR: Color = Color::Rgb(255, 90, 90);
pub const INPUT_BG: Color = Color::Rgb(14, 24, 38);

// Accent (for confirmations)
pub const ACCENT_POSITIVE: Color = Color::Rgb(0, 220, 140);
pub const ACCENT_NEGATIVE: Color = Color::Rgb(255, 90, 90);
pub const ACCENT_WARNING: Color = Color::Rgb(255, 180, 60);
