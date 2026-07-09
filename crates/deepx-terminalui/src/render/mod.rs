//! DeepX rendering abstraction layer.
//!
//! Provides the [`Renderable`] trait and combinator types
//! (Column, Flex, Inset) for building reusable TUI component trees.

use ratatui::layout::Rect;

// ── Insets ──────────────────────────────────────────────────────────────

/// Four-directional insets (padding).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Insets {
    pub left: u16,
    pub top: u16,
    pub right: u16,
    pub bottom: u16,
}

impl Insets {
    /// Specify top, left, bottom, right.
    pub const fn tlbr(top: u16, left: u16, bottom: u16, right: u16) -> Self {
        Self { top, left, bottom, right }
    }

    /// Equal vertical and horizontal insets.
    pub const fn vh(vertical: u16, horizontal: u16) -> Self {
        Self { top: vertical, left: horizontal, bottom: vertical, right: horizontal }
    }

    /// Equal insets on all four sides.
    pub const fn all(n: u16) -> Self {
        Self { top: n, left: n, bottom: n, right: n }
    }
}

// ── RectExt ─────────────────────────────────────────────────────────────

/// Convenience extension methods for `Rect`.
pub trait RectExt {
    /// Shrink rect by the given insets.
    fn inset(&self, insets: Insets) -> Rect;
}

impl RectExt for Rect {
    fn inset(&self, insets: Insets) -> Rect {
        let h = insets.left.saturating_add(insets.right);
        let v = insets.top.saturating_add(insets.bottom);
        Rect {
            x: self.x.saturating_add(insets.left),
            y: self.y.saturating_add(insets.top),
            width: self.width.saturating_sub(h),
            height: self.height.saturating_sub(v),
        }
    }
}

// ── Submodules ──────────────────────────────────────────────────────────

pub mod renderable;
pub mod markdown_render;
