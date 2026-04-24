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
        let mut poll = |duration: Duration| event::poll(duration).map_err(Into::into);
        let mut read = || event::read().map_err(Into::into);
        self.next_with(&mut poll, &mut read)
    }

    fn next_with(
        &self,
        poll: &mut dyn FnMut(Duration) -> Result<bool>,
        read: &mut dyn FnMut() -> Result<CrosstermEvent>,
    ) -> Result<Event> {
        if poll(self.tick_rate)? {
            match read()? {
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

    fn read_focus_gained() -> CrosstermEvent {
        CrosstermEvent::FocusGained
    }

    fn is_tick(event: Event) -> bool {
        matches!(event, Event::Tick)
    }

    fn is_key(event: Event) -> bool {
        matches!(event, Event::Key(_))
    }

    fn is_mouse(event: Event) -> bool {
        matches!(event, Event::Mouse(_))
    }

    fn resize_dims(event: Event) -> Option<(u16, u16)> {
        match event {
            Event::Resize(w, h) => Some((w, h)),
            _ => None,
        }
    }

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
        assert!(is_tick(cloned));
        assert!(is_tick(event));
        assert!(!is_tick(Event::Resize(10, 10)));
    }

    #[test]
    fn test_event_copy() {
        let event = Event::Tick;
        let copied: Event = event;
        assert!(is_tick(copied));
        // Original should still be valid since Event is Copy
        assert!(is_tick(event));
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
        assert_eq!(resize_dims(resize), Some((120, 40)));
        assert_eq!(resize_dims(Event::Tick), None);
    }

    #[test]
    fn test_event_handler_next_with_returns_tick_when_poll_false() {
        let handler = Handler::new(50);
        let mut poll = |_| -> Result<bool> { Ok(false) };
        let mut read = || Ok(read_focus_gained());
        let _ = read().unwrap();
        let event = handler.next_with(&mut poll, &mut read).unwrap();
        assert!(is_tick(event));
    }

    #[test]
    fn test_event_handler_next_with_handles_key_event() {
        let handler = Handler::new(50);
        let mut poll = |_| -> Result<bool> { Ok(true) };
        let mut read = || -> Result<CrosstermEvent> {
            Ok(CrosstermEvent::Key(KeyEvent::new(
                event::KeyCode::Char('a'),
                event::KeyModifiers::NONE,
            )))
        };
        let event = handler.next_with(&mut poll, &mut read).unwrap();
        assert!(is_key(event));
        assert!(!is_key(Event::Tick));
    }

    #[test]
    fn test_event_handler_next_with_handles_mouse_event() {
        let handler = Handler::new(50);
        let mouse = MouseEvent {
            kind: event::MouseEventKind::Moved,
            column: 10,
            row: 5,
            modifiers: event::KeyModifiers::NONE,
        };
        let mut poll = |_| -> Result<bool> { Ok(true) };
        let mut read = || -> Result<CrosstermEvent> { Ok(CrosstermEvent::Mouse(mouse)) };
        let event = handler.next_with(&mut poll, &mut read).unwrap();
        assert!(is_mouse(event));
        assert!(!is_mouse(Event::Tick));
    }

    #[test]
    fn test_event_handler_next_with_handles_resize_event() {
        let handler = Handler::new(50);
        let mut poll = |_| -> Result<bool> { Ok(true) };
        let mut read = || -> Result<CrosstermEvent> { Ok(CrosstermEvent::Resize(80, 24)) };
        let event = handler.next_with(&mut poll, &mut read).unwrap();
        assert_eq!(resize_dims(event), Some((80, 24)));
    }

    #[test]
    fn test_event_handler_next_with_maps_unhandled_event_to_tick() {
        let handler = Handler::new(50);
        let mut poll = |_| -> Result<bool> { Ok(true) };
        let mut read = || Ok(read_focus_gained());
        let event = handler.next_with(&mut poll, &mut read).unwrap();
        assert!(is_tick(event));
    }

    #[test]
    fn test_event_handler_next_with_propagates_poll_error() {
        let handler = Handler::new(50);
        let mut poll = |_| -> Result<bool> { Err(anyhow::anyhow!("poll failed")) };
        let mut read = || Ok(read_focus_gained());
        let _ = read().unwrap();
        let err = handler.next_with(&mut poll, &mut read).unwrap_err();
        assert!(err.to_string().contains("poll failed"));
    }

    #[test]
    fn test_event_handler_next_with_propagates_read_error() {
        let handler = Handler::new(50);
        let mut poll = |_| -> Result<bool> { Ok(true) };
        let mut read = || -> Result<CrosstermEvent> { Err(anyhow::anyhow!("read failed")) };
        let err = handler.next_with(&mut poll, &mut read).unwrap_err();
        assert!(err.to_string().contains("read failed"));
    }

    #[test]
    fn test_event_handler_next_smoke_does_not_block() {
        let handler = Handler::new(0);
        let _ = handler.next();
    }
}
