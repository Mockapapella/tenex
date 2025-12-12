//! Event handling for the TUI

use anyhow::Result;
use ratatui::crossterm::event::{self, Event as CrosstermEvent, KeyEvent, MouseEvent};
use std::time::Duration;

/// Application events
#[derive(Debug, Clone, Copy)]
pub enum Event {
    /// Terminal tick (for animations/updates)
    Tick,
    /// Keyboard input
    Key(KeyEvent),
    /// Mouse input
    Mouse(MouseEvent),
    /// Terminal resize
    Resize(u16, u16),
}

/// Handler that polls for terminal events
#[derive(Debug, Clone, Copy)]
pub struct Handler {
    /// Tick rate in milliseconds
    tick_rate: Duration,
}

impl Handler {
    /// Create a new event handler with the given tick rate
    #[must_use]
    pub const fn new(tick_rate_ms: u64) -> Self {
        Self {
            tick_rate: Duration::from_millis(tick_rate_ms),
        }
    }

    /// Poll for the next event
    ///
    /// # Errors
    ///
    /// Returns an error if polling fails
    pub fn next(&self) -> Result<Event> {
        if event::poll(self.tick_rate)? {
            match event::read()? {
                CrosstermEvent::Key(key) => Ok(Event::Key(key)),
                CrosstermEvent::Mouse(mouse) => Ok(Event::Mouse(mouse)),
                CrosstermEvent::Resize(w, h) => Ok(Event::Resize(w, h)),
                _ => Ok(Event::Tick),
            }
        } else {
            Ok(Event::Tick)
        }
    }

    /// Get the tick rate
    #[must_use]
    pub const fn tick_rate(&self) -> Duration {
        self.tick_rate
    }
}

impl Default for Handler {
    fn default() -> Self {
        Self::new(100)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_handler_new() {
        let handler = Handler::new(50);
        assert_eq!(handler.tick_rate(), Duration::from_millis(50));
    }

    #[test]
    fn test_event_handler_default() {
        let handler = Handler::default();
        assert_eq!(handler.tick_rate(), Duration::from_millis(100));
    }

    #[test]
    fn test_event_debug() {
        let event = Event::Tick;
        assert!(!format!("{event:?}").is_empty());

        let resize = Event::Resize(80, 24);
        assert!(!format!("{resize:?}").is_empty());
    }

    #[test]
    fn test_event_clone() {
        let event = Event::Tick;
        let cloned = Clone::clone(&event);
        assert!(matches!(cloned, Event::Tick));
        assert!(matches!(event, Event::Tick));
    }

    #[test]
    fn test_event_copy() {
        let event = Event::Tick;
        let copied: Event = event;
        assert!(matches!(copied, Event::Tick));
        // Original should still be valid since Event is Copy
        assert!(matches!(event, Event::Tick));
    }

    #[test]
    fn test_handler_debug() {
        let handler = Handler::new(50);
        assert!(!format!("{handler:?}").is_empty());
    }

    #[test]
    fn test_handler_clone() {
        let handler = Handler::new(75);
        let cloned = handler;
        assert_eq!(cloned.tick_rate(), Duration::from_millis(75));
    }

    #[test]
    fn test_event_resize_variant() {
        let resize = Event::Resize(120, 40);
        assert!(matches!(resize, Event::Resize(120, 40)));
    }
}
