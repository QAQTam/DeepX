//! Custom terminal with double-buffering and diff-based rendering.
//!
//! Wraps ratatui's [`Terminal`] with a second buffer to track the
//! previous frame, reducing terminal output to only changed cells.
//!
//! Simplified from Codex's custom_terminal.rs for ratatui 0.30.

use std::io::{self, Write};

use crossterm::cursor::MoveTo;
use crossterm::QueueableCommand;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

/// A terminal wrapper with double-buffered, diff-based rendering.
pub struct CustomTerminal<W: Write> {
    inner: Terminal<CrosstermBackend<W>>,
    prev_buffer: Buffer,
    last_size: Rect,
}

impl<W: Write> CustomTerminal<W> {
    pub fn new(terminal: Terminal<CrosstermBackend<W>>) -> Self {
        let size = terminal.size().unwrap_or(ratatui::layout::Size::new(80, 24));
        let rect = Rect::new(0, 0, size.width, size.height);
        Self {
            inner: terminal,
            prev_buffer: Buffer::empty(rect),
            last_size: rect,
        }
    }

    pub fn size_rect(&self) -> io::Result<Rect> {
        let s = self.inner.size()?;
        Ok(Rect::new(0, 0, s.width, s.height))
    }

    pub fn terminal_mut(&mut self) -> &mut Terminal<CrosstermBackend<W>> {
        &mut self.inner
    }

    pub fn backend(&self) -> &CrosstermBackend<W> {
        self.inner.backend()
    }

    pub fn backend_mut(&mut self) -> &mut CrosstermBackend<W> {
        self.inner.backend_mut()
    }

    /// Draw a frame with the diff-based approach.
    /// `f` receives the area rect and a buffer to render into.
    pub fn draw<F>(&mut self, f: F) -> io::Result<()>
    where
        F: FnOnce(Rect, &mut Buffer),
    {
        let size = self.inner.size()?;
        let area = Rect::new(0, 0, size.width, size.height);
        let mut cur_buf = Buffer::empty(area);

        // Render into current buffer
        f(area, &mut cur_buf);

        // Diff and flush — pass backend explicitly to avoid borrow conflict
        {
            let backend = self.inner.backend_mut();
            diff_buffers(backend, &cur_buf, &self.prev_buffer, area)?;
        }

        self.prev_buffer = cur_buf;
        self.last_size = area;
        Ok(())
    }

    pub fn clear(&mut self) -> io::Result<()> {
        use crossterm::execute;
        use crossterm::terminal::{Clear, ClearType};
        execute!(self.inner.backend_mut(), Clear(ClearType::All))?;
        let size = self.inner.size()?;
        let rect = Rect::new(0, 0, size.width, size.height);
        self.prev_buffer = Buffer::empty(rect);
        Ok(())
    }

    /// Standard Frame-based draw, delegating to ratatui.
    pub fn draw_frame<F>(&mut self, f: F) -> io::Result<()>
    where
        F: FnOnce(&mut ratatui::Frame<'_>),
    {
        let _ = self.inner.draw(f)?;
        Ok(())
    }
}

/// Write only cells that differ between `cur` and `prev` buffers to `backend`.
fn diff_buffers<W: Write>(
    backend: &mut CrosstermBackend<W>,
    cur: &Buffer,
    prev: &Buffer,
    area: Rect,
) -> io::Result<()> {
    let mut last_x: Option<u16> = None;
    let mut last_y: Option<u16> = None;

    for y in 0..area.height {
        for x in 0..area.width {
            let cur_cell = cur.cell((x, y));
            let prev_cell = prev.cell((x, y));

            let changed = match (cur_cell, prev_cell) {
                (Some(c), Some(p)) => c.symbol() != p.symbol() || c.style() != p.style(),
                (Some(_), None) => true,
                _ => false,
            };

            if changed {
                if last_x != Some(x) || last_y != Some(y) {
                    backend.queue(MoveTo(x, y))?;
                    last_x = Some(x);
                    last_y = Some(y);
                }
                if let Some(cell) = cur_cell {
                    let sym = cell.symbol();
                    if sym == " " {
                        backend.queue(crossterm::style::Print(" "))?;
                    } else {
                        backend.queue(crossterm::style::Print(sym.to_string()))?;
                    }
                }
            }
        }
    }

    backend.flush()?;
    Ok(())
}
