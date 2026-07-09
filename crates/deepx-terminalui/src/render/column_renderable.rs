use ratatui::prelude::*;
use crate::render::renderable::Renderable;

/// 垂直列布局渲染器
///
/// 将多个 Renderable 子元素从上到下依次排列，每个子元素占用其最小高度。
/// 行内剩余高度不重新分配。若需弹性拉伸，请使用 FlexRenderable 的垂直模式。
///
/// 子元素之间可配置间距（spacing）。
pub struct ColumnRenderable {
    /// 子元素列表
    children: Vec<Box<dyn Renderable>>,
    /// 子元素之间的垂直间距
    spacing: u16,
}

impl ColumnRenderable {
    /// 创建一个新的列布局渲染器
    ///
    /// ## 参数
    /// - children：子元素列表
    /// - spacing：子元素之间的垂直间距
    pub fn new(children: Vec<Box<dyn Renderable>>, spacing: u16) -> Self {
        Self { children, spacing }
    }

    /// 在末尾添加一个子元素
    pub fn push(&mut self, child: Box<dyn Renderable>) {
        self.children.push(child);
    }
}

impl Renderable for ColumnRenderable {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let mut y = area.y;
        let mut remaining = area.height;

        for (i, child) in self.children.iter().enumerate() {
            if remaining == 0 {
                break;
            }

            let min_h = child.min_size().height;
            let h = min_h.min(remaining);

            child.render(Rect::new(area.x, y, area.width, h), buf);
            y = y.saturating_add(h);
            remaining = remaining.saturating_sub(h);

            // 在非末尾元素之间插入间距
            if i < self.children.len() - 1 && remaining > 0 {
                let gap = self.spacing.min(remaining);
                y = y.saturating_add(gap);
                remaining = remaining.saturating_sub(gap);
            }
        }
    }

    fn min_size(&self) -> Size {
        let w: u16 = self.children.iter().map(|c| c.min_size().width).max().unwrap_or(0);
        let h_no_gap: u16 = self.children.iter().map(|c| c.min_size().height).sum();
        let gaps = if self.children.len() > 1 {
            (self.children.len() - 1) as u16 * self.spacing
        } else {
            0
        };
        Size::new(w, h_no_gap.saturating_add(gaps))
    }
}
