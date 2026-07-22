//! This crate parses a terminal byte stream and provides an in-memory
//! representation of the rendered contents.
//!
//! # Overview
//!
//! This is essentially the terminal parser component of a graphical terminal
//! emulator pulled out into a separate crate. Although you can use this crate
//! to build a graphical terminal emulator, it also contains functionality
//! necessary for implementing terminal applications that want to run other
//! terminal applications - programs like `screen` or `tmux` for example.
//!
//! # Synopsis
//!
//! ```text
//! let mut parser = vt100::Parser::new(24, 80, 0);
//!
//! let screen = parser.screen().clone();
//! parser.process(b"this text is \x1b[31mRED\x1b[m");
//! let red = parser.screen().cell(0, 13).unwrap().fgcolor();
//!
//! let screen = parser.screen().clone();
//! parser.process(b"\x1b[3D\x1b[32mGREEN");
//! let full_screen = parser.screen().contents_formatted();
//! let changed_region = parser.screen().contents_diff(&screen);
//! ```

#![warn(clippy::pedantic)]
#![warn(clippy::as_conversions)]
#![warn(clippy::get_unwrap)]
#![allow(clippy::cognitive_complexity)]
#![allow(clippy::missing_const_for_fn)]
#![allow(clippy::similar_names)]
#![allow(clippy::struct_excessive_bools)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::too_many_lines)]
#![allow(clippy::type_complexity)]

mod attrs;
mod callbacks;
mod cell;
mod grid;
mod parser;
mod perform;
mod row;
mod screen;
mod term;
#[cfg(feature = "tui-term")]
mod tui_term;

pub use attrs::Color;
pub use callbacks::Callbacks;
pub use cell::Cell;
pub use parser::Parser;
pub use screen::{MouseProtocolEncoding, MouseProtocolMode, Screen};
