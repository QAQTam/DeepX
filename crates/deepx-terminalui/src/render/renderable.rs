//! `Renderable` trait and combinator types.
//!
//! Core trait [`Renderable`] provides `render` + `desired_height`.
//! Combinators include:
//! - [`ColumnRenderable`] — stacks children vertically
//! - [`FlexRenderable`] — flex-box vertical layout
//! - [`InsetRenderable`] — wraps a child with padding

use std::sync::Arc;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use crate::render::{Insets, RectExt};

// ── Renderable trait ────────────────────────────────────────────────────

/// Core trait for renderable TUI components.
pub trait Renderable {
    /// Render self to the buffer within the given area.
    fn render(&self, area: Rect, buf: &mut Buffer);

    /// Expected height (in rows) at the given width.
    fn desired_height(&self, width: u16) -> u16;
}

// ── RenderableItem ──────────────────────────────────────────────────────

/// Wrapper supporting both owned and borrowed `Renderable` values.
pub enum RenderableItem<'a> {
    Owned(Box<dyn Renderable + 'a>),
    Borrowed(&'a dyn Renderable),
}

impl<'a> Renderable for RenderableItem<'a> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        match self {
            Self::Owned(child) => child.render(area, buf),
            Self::Borrowed(child) => child.render(area, buf),
        }
    }

    fn desired_height(&self, width: u16) -> u16 {
        match self {
            Self::Owned(child) => child.desired_height(width),
            Self::Borrowed(child) => child.desired_height(width),
        }
    }
}

impl<'a> From<Box<dyn Renderable + 'a>> for RenderableItem<'a> {
    fn from(v: Box<dyn Renderable + 'a>) -> Self {
        Self::Owned(v)
    }
}

/// Helper to convert any `Renderable` into a `RenderableItem`.
pub fn to_item<'a>(r: impl Renderable + 'a) -> RenderableItem<'a> {
    RenderableItem::Owned(Box::new(r))
}

// ── Standard type implementations ───────────────────────────────────────

impl Renderable for () {
    fn render(&self, _area: Rect, _buf: &mut Buffer) {}
    fn desired_height(&self, _width: u16) -> u16 {
        0
    }
}

/// Render a string slice by building a Paragraph and rendering it.
fn render_str(s: &str, area: Rect, buf: &mut Buffer) {
    let p = Paragraph::new(s);
    p.render(area, buf);
}

impl Renderable for &str {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        render_str(self, area, buf);
    }
    fn desired_height(&self, _width: u16) -> u16 {
        1
    }
}

impl Renderable for String {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        render_str(self.as_str(), area, buf);
    }
    fn desired_height(&self, _width: u16) -> u16 {
        1
    }
}

impl<'a> Renderable for Span<'a> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let line = Line::from(self.clone());
        Paragraph::new(line).render(area, buf);
    }
    fn desired_height(&self, _width: u16) -> u16 {
        1
    }
}

impl<'a> Renderable for Line<'a> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        Paragraph::new(self.clone()).render(area, buf);
    }
    fn desired_height(&self, _width: u16) -> u16 {
        1
    }
}

impl<'a> Renderable for Paragraph<'a> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        Widget::render(self, area, buf);
    }
    fn desired_height(&self, width: u16) -> u16 {
        self.line_count(width) as u16
    }
}

impl<R: Renderable> Renderable for Option<R> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        if let Some(r) = self {
            r.render(area, buf);
        }
    }
    fn desired_height(&self, width: u16) -> u16 {
        self.as_ref().map_or(0, |r| r.desired_height(width))
    }
}

impl<R: Renderable> Renderable for Arc<R> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        self.as_ref().render(area, buf);
    }
    fn desired_height(&self, width: u16) -> u16 {
        self.as_ref().desired_height(width)
    }
}

// ── ColumnRenderable ────────────────────────────────────────────────────

/// Stacks children vertically.
pub struct ColumnRenderable<'a> {
    children: Vec<RenderableItem<'a>>,
}

impl<'a> ColumnRenderable<'a> {
    pub fn new() -> Self {
        Self {
            children: Vec::new(),
        }
    }

    pub fn push(&mut self, child: impl Into<RenderableItem<'a>>) {
        self.children.push(child.into());
    }
}

impl Renderable for ColumnRenderable<'_> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let mut y = area.y;
        let max_y = area.y.saturating_add(area.height);
        for child in &self.children {
            if y >= max_y {
                break;
            }
            let h = child
                .desired_height(area.width)
                .min(max_y.saturating_sub(y));
            let child_area = Rect {
                x: area.x,
                y,
                width: area.width,
                height: h,
            };
            child.render(child_area, buf);
            y = y.saturating_add(h);
        }
    }

    fn desired_height(&self, width: u16) -> u16 {
        self.children.iter().map(|c| c.desired_height(width)).sum()
    }
}

// ── FlexRenderable ──────────────────────────────────────────────────────

struct FlexChild<'a> {
    flex: i32,
    child: RenderableItem<'a>,
}

/// Flex-box vertical layout.
///
/// Children with `flex > 0` share remaining space proportionally.
/// Children with `flex == 0` take their `desired_height`.
pub struct FlexRenderable<'a> {
    children: Vec<FlexChild<'a>>,
}

impl<'a> FlexRenderable<'a> {
    pub fn new() -> Self {
        Self {
            children: Vec::new(),
        }
    }

    pub fn push(&mut self, flex: i32, child: impl Into<RenderableItem<'a>>) {
        self.children.push(FlexChild {
            flex,
            child: child.into(),
        });
    }

    fn allocate(&self, area: Rect) -> Vec<Rect> {
        let n = self.children.len();
        if n == 0 {
            return Vec::new();
        }

        let mut sizes = vec![0u16; n];
        let mut allocated = 0u16;
        let max_h = area.height;

        // 1. Fixed-height children (flex == 0)
        let mut flex_list: Vec<(usize, u16, u16)> = Vec::new();
        for (i, fc) in self.children.iter().enumerate() {
            if fc.flex <= 0 {
                let h = fc
                    .child
                    .desired_height(area.width)
                    .min(max_h.saturating_sub(allocated));
                sizes[i] = h;
                allocated = allocated.saturating_add(h);
            } else {
                flex_list.push((i, fc.flex as u16, fc.child.desired_height(area.width)));
            }
        }

        // 2. Distribute remaining space to flex children
        if !flex_list.is_empty() {
            let free = max_h.saturating_sub(allocated);
            let total_flex: u16 = flex_list.iter().map(|(_, f, _)| *f).sum();
            let last_idx = flex_list.last().map(|(i, _, _)| *i);

            let mut flex_allocated = 0u16;
            for (i, f, desired) in &flex_list {
                let share = if Some(*i) == last_idx {
                    free.saturating_sub(flex_allocated)
                } else if total_flex > 0 {
                    (free as u32 * *f as u32 / total_flex as u32) as u16
                } else {
                    0
                };
                let h = (*desired).min(share);
                sizes[*i] = h;
                flex_allocated = flex_allocated.saturating_add(h);
            }
        }

        let mut rects = Vec::with_capacity(n);
        let mut y = area.y;
        for h in sizes {
            rects.push(Rect {
                x: area.x,
                y,
                width: area.width,
                height: h,
            });
            y = y.saturating_add(h);
        }
        rects
    }
}

impl Renderable for FlexRenderable<'_> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let rects = self.allocate(area);
        for (fc, r) in self.children.iter().zip(rects.iter()) {
            fc.child.render(*r, buf);
        }
    }

    fn desired_height(&self, width: u16) -> u16 {
        self.children
            .iter()
            .map(|fc| fc.child.desired_height(width))
            .sum()
    }
}

// ── RowRenderable ───────────────────────────────────────────────────────

/// Lays children out horizontally, left to right.
///
/// Children with `flex > 0` share remaining width proportionally.
/// Children with `flex == 0` take their `desired_width` (approximated via desired_height).
pub struct RowRenderable<'a> {
    children: Vec<RowChild<'a>>,
}

struct RowChild<'a> {
    flex: i32,
    child: RenderableItem<'a>,
    min_width: u16,
}

impl<'a> RowRenderable<'a> {
    pub fn new() -> Self {
        Self { children: Vec::new() }
    }

    /// Add a child. `flex > 0` = flexible, `flex == 0` = fixed.
    /// `min_width` is the minimum width for flex children.
    pub fn push(&mut self, flex: i32, min_width: u16, child: impl Into<RenderableItem<'a>>) {
        self.children.push(RowChild { flex, min_width, child: child.into() });
    }

    fn allocate(&self, area: Rect) -> Vec<Rect> {
        let n = self.children.len();
        if n == 0 { return Vec::new(); }

        let mut widths = vec![0u16; n];
        let mut allocated = 0u16;
        let max_w = area.width;

        // 1. Fixed-width children
        let mut flex_list: Vec<(usize, u16, u16)> = Vec::new();
        for (i, rc) in self.children.iter().enumerate() {
            if rc.flex <= 0 {
                let w = rc.child.desired_height(1).min(max_w.saturating_sub(allocated));
                widths[i] = w;
                allocated = allocated.saturating_add(w);
            } else {
                flex_list.push((i, rc.flex as u16, rc.min_width));
            }
        }

        // 2. Flex children share remaining
        if !flex_list.is_empty() {
            let free = max_w.saturating_sub(allocated);
            let total_flex: u16 = flex_list.iter().map(|(_, f, _)| *f).sum();
            let last_idx = flex_list.last().map(|(i, _, _)| *i);
            let mut fa = 0u16;
            for (i, f, min_w) in &flex_list {
                let share = if Some(*i) == last_idx {
                    free.saturating_sub(fa)
                } else if total_flex > 0 {
                    (free as u32 * *f as u32 / total_flex as u32) as u16
                } else { 0 };
                widths[*i] = (*min_w).max(share);
                fa = fa.saturating_add(widths[*i]);
            }
        }

        let mut rects = Vec::with_capacity(n);
        let mut x = area.x;
        for w in widths {
            rects.push(Rect { x, y: area.y, width: w, height: area.height });
            x = x.saturating_add(w);
        }
        rects
    }
}

impl Renderable for RowRenderable<'_> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let rects = self.allocate(area);
        for (rc, r) in self.children.iter().zip(rects.iter()) {
            rc.child.render(*r, buf);
        }
    }

    fn desired_height(&self, width: u16) -> u16 {
        self.children.iter()
            .map(|rc| rc.child.desired_height(width))
            .max()
            .unwrap_or(0)
    }
}

// ── InsetRenderable ─────────────────────────────────────────────────────

/// Wraps a child with padding.
pub struct InsetRenderable<'a> {
    child: RenderableItem<'a>,
    insets: Insets,
}

impl<'a> InsetRenderable<'a> {
    pub fn new(child: RenderableItem<'a>, insets: Insets) -> Self {
        Self { child, insets }
    }
}

impl Renderable for InsetRenderable<'_> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let inner = area.inset(self.insets);
        self.child.render(inner, buf);
    }

    fn desired_height(&self, width: u16) -> u16 {
        let inner_w =
            width.saturating_sub(self.insets.left.saturating_add(self.insets.right));
        self.child
            .desired_height(inner_w)
            .saturating_add(self.insets.top)
            .saturating_add(self.insets.bottom)
    }
}

// ── RenderableExt ──────────────────────────────────────────────────────

/// Extension methods for all `Renderable` types.
pub trait RenderableExt<'a>: Renderable + Sized + 'a {
    /// Wrap self with padding insets.
    fn inset(self, insets: Insets) -> RenderableItem<'a> {
        RenderableItem::Owned(Box::new(InsetRenderable::new(
            RenderableItem::Owned(Box::new(self)),
            insets,
        )))
    }
}

impl<'a, R: Renderable + 'a> RenderableExt<'a> for R {}
