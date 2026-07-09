use ratatui::prelude::*;
use crate::render::renderable::Renderable;

/// 四边内边距（Insets）
///
/// 表示矩形区域在左、右、上、下四个方向上需要扣除的空白宽度。
/// 常用于为 InsetRenderable 提供内边距配置。
pub struct Insets {
    /// 左边距宽度
    pub left: u16,
    /// 右边距宽度
    pub right: u16,
    /// 上边距高度
    pub top: u16,
    /// 下边距高度
    pub bottom: u16,
}

impl Insets {
    /// 创建一个各方向内边距相同的新 Insets
    pub const fn all(value: u16) -> Self {
        Self { left: value, right: value, top: value, bottom: value }
    }

    /// 创建一个水平方向相同、垂直方向相同的新 Insets
    ///
    /// ## 参数
    /// - horizontal：左和右的内边距
    /// - vertical：上和下的内边距
    pub const fn symmetric(horizontal: u16, vertical: u16) -> Self {
        Self { left: horizontal, right: horizontal, top: vertical, bottom: vertical }
    }

    /// 创建一个指定各方向内边距的新 Insets
    pub const fn new(left: u16, right: u16, top: u16, bottom: u16) -> Self {
        Self { left, right, top, bottom }
    }
}

/// 内边距渲染器
///
/// 包裹一个 Renderable 子元素，在渲染时先扣除四边内边距，
/// 再在内缩后的可用区域中渲染子元素。
/// 例如，可用于为列表项、面板等组件添加空白边距。
pub struct InsetRenderable<R: Renderable> {
    /// 被包裹的子元素
    inner: R,
    /// 四边内边距
    insets: Insets,
}

impl<R: Renderable> InsetRenderable<R> {
    /// 创建一个新的内边距渲染器
    pub fn new(inner: R, insets: Insets) -> Self {
        Self { inner, insets }
    }
}

impl<R: Renderable> Renderable for InsetRenderable<R> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let inner_area = Rect::new(
            area.x.saturating_add(self.insets.left),
            area.y.saturating_add(self.insets.top),
            area.width.saturating_sub(self.insets.left + self.insets.right),
            area.height.saturating_sub(self.insets.top + self.insets.bottom),
        );
        self.inner.render(inner_area, buf);
    }

    fn min_size(&self) -> Size {
        let inner_size = self.inner.min_size();
        Size::new(
            inner_size.width.saturating_add(self.insets.left + self.insets.right),
            inner_size.height.saturating_add(self.insets.top + self.insets.bottom),
        )
    }
}
