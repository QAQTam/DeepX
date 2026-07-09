use ratatui::prelude::*;
use crate::render::renderable::Renderable;

/// 布局方向枚举
///
/// 用于 FlexRenderable 指定子元素的排列方向。
pub enum Direction {
    /// 水平方向：子元素从左到右依次排列
    Horizontal,
    /// 垂直方向：子元素从上到下依次排列
    Vertical,
}

/// 弹性布局渲染器（Flexbox 风格的简易实现）
///
/// 将多个 Renderable 子元素按水平或垂直方向排列。
/// 空间分配策略：
/// - 若可用空间大于所有子元素最小宽度/高度之和，则在满足最小尺寸后平均分配剩余空间
/// - 若可用空间不足，则按最小尺寸比例压缩各子元素
/// - 若所有子元素最小尺寸均为 0，则平均分配全部空间
pub struct FlexRenderable {
    /// 子元素列表
    children: Vec<Box<dyn Renderable>>,
    /// 排列方向
    direction: Direction,
}

impl FlexRenderable {
    /// 创建一个新的弹性布局渲染器
    ///
    /// ## 参数
    /// - children：子元素列表
    /// - direction：排列方向
    pub fn new(children: Vec<Box<dyn Renderable>>, direction: Direction) -> Self {
        Self { children, direction }
    }

    /// 在末尾添加一个子元素
    pub fn push(&mut self, child: Box<dyn Renderable>) {
        self.children.push(child);
    }
}

impl Renderable for FlexRenderable {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        if self.children.is_empty() {
            return;
        }

        match self.direction {
            Direction::Horizontal => {
                let total_min: u16 = self.children.iter().map(|c| c.min_size().width).sum();
                let count = self.children.len() as u16;

                if total_min == 0 {
                    // 所有子元素都没有最小宽度 → 均分
                    let each = area.width / count;
                    let mut x = area.x;
                    for child in &self.children {
                        child.render(Rect::new(x, area.y, each, area.height), buf);
                        x = x.saturating_add(each);
                    }
                } else if total_min >= area.width {
                    // 空间不足 → 按最小宽度比例压缩
                    let mut x = area.x;
                    for child in &self.children {
                        let min_w = child.min_size().width;
                        let w = min_w * area.width / total_min;
                        child.render(Rect::new(x, area.y, w, area.height), buf);
                        x = x.saturating_add(w);
                    }
                } else {
                    // 空间充裕 → 最小宽度 + 均分剩余
                    let extra = (area.width - total_min) / count;
                    let mut x = area.x;
                    for child in &self.children {
                        let w = child.min_size().width + extra;
                        child.render(Rect::new(x, area.y, w, area.height), buf);
                        x = x.saturating_add(w);
                    }
                }
            }
            Direction::Vertical => {
                let total_min: u16 = self.children.iter().map(|c| c.min_size().height).sum();
                let count = self.children.len() as u16;

                if total_min == 0 {
                    let each = area.height / count;
                    let mut y = area.y;
                    for child in &self.children {
                        child.render(Rect::new(area.x, y, area.width, each), buf);
                        y = y.saturating_add(each);
                    }
                } else if total_min >= area.height {
                    let mut y = area.y;
                    for child in &self.children {
                        let min_h = child.min_size().height;
                        let h = min_h * area.height / total_min;
                        child.render(Rect::new(area.x, y, area.width, h), buf);
                        y = y.saturating_add(h);
                    }
                } else {
                    let extra = (area.height - total_min) / count;
                    let mut y = area.y;
                    for child in &self.children {
                        let h = child.min_size().height + extra;
                        child.render(Rect::new(area.x, y, area.width, h), buf);
                        y = y.saturating_add(h);
                    }
                }
            }
        }
    }

    fn min_size(&self) -> Size {
        match self.direction {
            Direction::Horizontal => {
                let w: u16 = self.children.iter().map(|c| c.min_size().width).sum();
                let h: u16 = self.children.iter().map(|c| c.min_size().height).max().unwrap_or(0);
                Size::new(w, h)
            }
            Direction::Vertical => {
                let w: u16 = self.children.iter().map(|c| c.min_size().width).max().unwrap_or(0);
                let h: u16 = self.children.iter().map(|c| c.min_size().height).sum();
                Size::new(w, h)
            }
        }
    }
}
