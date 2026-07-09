//! Event broker with pause/resume support for crossterm event stream.
//!
//! Wraps crossterm's `event::poll`/`event::read` so that stdin
//! can be released before launching external processes (editors, etc.)
//! and re-acquired after they exit.
//!
//! Simplified from Codex's tui/event_stream.rs.

use std::io;
use std::time::Duration;

use crossterm::event::{self, Event};

/// Wraps the crossterm event stream with pause/resume capability.
///
/// When paused, `next()` returns `Ok(None)` after each timeout
/// instead of reading from stdin, allowing another process to
/// read from stdin safely.
#[derive(Debug)]
pub struct EventBroker {
    paused: bool,
    /// Accumulated pending events while paused (unused currently).
    pending: Vec<Event>,
}

impl EventBroker {
    /// Create a new active event broker.
    pub fn new() -> Self {
        Self {
            paused: false,
            pending: Vec::new(),
        }
    }

    /// Pause event reading. Call before spawning an external process
    /// that reads from stdin.
    pub fn pause(&mut self) {
        self.paused = true;
    }

    /// Resume event reading after the external process exits.
    pub fn resume(&mut self) {
        self.paused = false;
    }

    /// Check if the broker is paused.
    pub fn is_paused(&self) -> bool {
        self.paused
    }

    /// Poll for the next event with a timeout.
    ///
    /// When paused, returns `Ok(None)` after each timeout period
    /// without consuming stdin events.
    ///
    /// When active, delegates to `crossterm::event::poll`.
    pub fn poll(&self, timeout: Duration) -> io::Result<bool> {
        if self.paused {
            // When paused, just wait the timeout and report no events
            std::thread::sleep(timeout.min(Duration::from_millis(100)));
            return Ok(false);
        }
        event::poll(timeout)
    }

    /// Read the next event from stdin.
    ///
    /// When paused, returns a synthetic `Tick`-like event or `None`
    /// depending on the timeout having elapsed.
    ///
    /// Callers should check `is_paused()` before calling this and
    /// handle accordingly.
    pub fn read(&mut self) -> io::Result<Option<Event>> {
        if self.paused {
            // Drain any pending synthetic events first
            if let Some(ev) = self.pending.pop() {
                return Ok(Some(ev));
            }
            return Ok(None);
        }

        // Drain pending real events
        if let Some(ev) = self.pending.pop() {
            return Ok(Some(ev));
        }

        match event::read() {
            Ok(ev) => Ok(Some(ev)),
            Err(e) => Err(e),
        }
    }
}

impl Default for EventBroker {
    fn default() -> Self {
        Self::new()
    }
}
