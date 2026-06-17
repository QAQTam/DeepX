# next-gui: 原生 AI 聊天渲染引擎设计草案

## 设计原则

1. **一次排版，永久缓存** — 文本布局结果存在 GPU buffer，内容不变则永远不重排
2. **增量更新** — 流式到达的每个 token 只追加 glyph，不重建整行
3. **零帧开销** — 无事件时 CPU 完全休眠，GPU 保持最后一帧
4. **文本即一等公民** — markdown 解析 → 富文本 tree → GPU 直接消费，无中间 widget 抽象

## 无框架假设

不使用任何 GUI 框架。直接对 GPU 编程：

- 后端：wgpu（跨平台 Vulkan/Metal/DX12）
- 字体：skrifa（解析塑形）+ harfrust（复杂文字塑形）
- 窗口：winit（只负责窗口创建和输入事件）
- 文本布局：parley 或 cosmic-text（富文本排版）
- Markdown：pulldown-cmark（AST 解析）
- 语法高亮：syntect 或 tree-sitter-highlight

## 架构总览

```
┌──────────────────────────────────────────────┐
│                   你的代码                     │
│                                              │
│  app.send_message("你好")                    │
│  app.on_event(Event::Token("世界"))           │
│  不调用任何 widget API                         │
│                                              │
└───────────────────┬──────────────────────────┘
                    │
┌───────────────────▼──────────────────────────┐
│              ng::Engine                       │
│                                               │
│  ┌─────────┐  ┌──────────┐  ┌─────────────┐  │
│  │ Block   │  │ Layout   │  │ GpuAtlas    │  │
│  │ Tree    │──│ Cache    │──│              │  │
│  │ (AST)   │  │ (排版缓存)│  │ (纹理图集)   │  │
│  └─────────┘  └──────────┘  └─────────────┘  │
│                                               │
│  ┌──────────────────────────────────────────┐ │
│  │              wgpu 渲染管线                 │ │
│  │  vertex buffer (glyph 位置)               │ │
│  │  index buffer  (三角形索引)               │ │
│  │  texture atlas (字形光栅)                  │ │
│  └──────────────────────────────────────────┘ │
└──────────────────────────────────────────────┘
```

## 核心数据结构

### Block — 内容的不可变单元

```rust
/// 一个不可变的渲染单元。
/// 一旦创建，其内容永不改变。
/// 位置和可见性可变，但内容不可变。
struct Block {
    id: BlockId,
    kind: BlockKind,
    /// 排版结果（只创建一次）
    layout: Option<Arc<Layout>>,
    /// 在 GPU buffer 中的位置
    vertex_range: Option<Range<u32>>,
    /// 当前可见矩形（viewport 坐标）
    visible_rect: Option<Rect>,
}

enum BlockKind {
    /// 流式追加中的文本（只追加，不重建）
    StreamingText(StreamingBuffer),
    /// 已完成的富文本
    RichText(RichTextDoc),
    /// 代码块
    CodeBlock { lang: String, text: String },
    /// 工具调用卡片
    ToolCard { name: String, args: String, status: ToolStatus },
}

/// 流式文本缓冲：只追加，不分配
struct StreamingBuffer {
    /// 已完成的行（不可变）
    sealed_lines: Vec<Arc<str>>,
    /// 当前正在写入的行
    draft: String,
}
```

### Layout — 仅排版一次

```rust
/// 文本的排版结果，创建后不可变
struct Layout {
    /// 每个 glyph 的屏幕位置
    glyphs: Vec<GlyphPosition>,
    /// 总尺寸
    size: Size,
    /// 行信息（用于光标定位、选择等）
    lines: Vec<LineInfo>,
}

struct GlyphPosition {
    glyph_id: u16,      // 字体中的字形索引
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    /// 在字体纹理图集中的位置
    atlas_rect: Rect,
}
```

### GpuAtlas — 字形纹理图集

```rust
struct GpuAtlas {
    /// wgpu 纹理
    texture: wgpu::Texture,
    /// 每个字形的 UV 坐标
    glyph_uvs: HashMap<GlyphKey, Rect>,
}

// 全局单例，跨所有 Block 共享
static GPU_ATLAS: Lazy<GpuAtlas> = ...;
```

## 流式渲染流程

### 场景：用户发消息，AI 逐 token 回复

```
RoundDelta("你")
  │
  ├─ 1. StreamingBuffer::push("你")
  │     draft = "你"
  │
  ├─ 2. LayoutCache::layout_streaming(&buffer)
  │     sealed_lines → 复用已有排版
  │     draft → 只排版 "你" 这行
  │     → Layout { glyphs: [你的位置], ... }
  │
  ├─ 3. GpuAtlas::ensure("你")
  │     如果字形 "你" 不在图集中 → 光栅化 → 上传
  │     如果已在 → 直接返回 UV
  │
  ├─ 4. GPU::update_vertex_range(block.vertex_range)
  │     更新顶点缓冲区中 "你" 的位置 + UV
  │
  └─ 5. GPU::present()
        直接呈现，不经过 CPU 帧循环

RoundDelta("好")
  │
  ├─ 1. StreamingBuffer::push("好")
  │     draft = "你好"
  │
  ├─ 2. LayoutCache 检测到只有 draft 行变化
  │     只重新排版 draft 行
  │     "你" 的 glyph 位置不变 → 不更新
  │     "好" 的 glyph 新分配
  │
  ├─ 3-5. 同上，只更新 "好" 的顶点

RoundComplete { blocks: [Text("你好世界")] }
  │
  ├─ 1. StreamingBuffer → RichTextDoc("你好世界")
  │
  ├─ 2. pulldown-cmark::Parser::new("你好世界") → MdAst
  │     只解析一次！
  │
  ├─ 3. LayoutCache::layout_rich_text(&doc)
  │     全文排版一次 → 存入 Arc<Layout>
  │
  ├─ 4. GPU: 替换整个 block 的顶点缓冲区
  │
  └─ 5. GPU::present()

后续每帧:
  │
  ├─ 没有新事件 → CPU 完全空闲
  └─ GPU 持有上一帧的顶点缓冲 → 显示器继续刷新
```

## 帧循环

```rust
struct Engine {
    blocks: Vec<Block>,
    pending_events: VecDeque<Event>,
    gpu: GpuState,
    atlas: GpuAtlas,
    layout_cache: LayoutCache,
}

impl Engine {
    fn run(mut self) {
        loop {
            // 1. 处理输入事件（如果有）
            while let Some(event) = self.pending_events.pop_front() {
                match event {
                    Event::Token { block_id, text } => {
                        self.streaming_append(block_id, &text);
                    }
                    Event::RoundComplete { block_id, doc } => {
                        self.finalize_block(block_id, doc);
                    }
                    Event::Scroll { delta } => {
                        self.gpu.update_scroll_offset(delta);
                    }
                    Event::Resize { width, height } => {
                        // 只重新计算位置，不重新排版
                        self.relayout_viewport(width, height);
                    }
                }
            }

            // 2. 渲染
            self.gpu.render(&self.blocks);

            // 3. 如果没有事件，等待 vsync 或新事件
            //    （不轮询，不空转）
            let next_event = winit::wait_for_event_or_vsync();
            if let Some(event) = next_event {
                self.pending_events.push_back(event);
            }
        }
    }
}
```

## 与现有框架的本质差异

| | React/Solid (Web) | egui | next-gui |
|---|---|---|---|
| 排版时机 | DOM 自动 | 每帧 | 内容变化时 |
| Markdown 解析 | 每帧（React）或手动缓存 | 每帧（无缓存） | 只解析一次 |
| 空闲 CPU 占用 | ~5%（浏览器后台） | 120ms 周期轮询 | 0%（vsync 等待） |
| 内存模型 | Virtual DOM diff | 每帧全量 | GPU 持久化 buffer |
| 渲染 | 浏览器绘制 | vello_cpu → wgpu | wgpu 直写 |

## Rust 适配清单

### 必须的 crate

| crate | 用途 | 已有/需适配 |
|---|---|---|
| winit | 窗口创建、输入事件 | ✅ 直接用 |
| wgpu | GPU API | ✅ 直接用 |
| skrifa | 字体解析 | ✅ egui 已依赖 |
| harfrust | 复杂文字塑形（阿拉伯语、天城文等） | ✅ egui 已依赖 |
| pulldown-cmark | markdown 解析 | ✅ 已集成 |
| syntect | 语法高亮 | ✅ TUI 已用 |

### 需要自己写的组件

| 组件 | 估算行数 | 说明 |
|---|---|---|
| `Block` / `BlockTree` | ~300 | 不可变内容树 |
| `LayoutCache` | ~500 | 排版缓存，增量失效 |
| `GpuAtlas` | ~400 | 字形纹理图集管理 |
| `wgpu 渲染管线` | ~600 | vertex/index buffer，glyph 着色器 |
| `StreamingBuffer` | ~200 | 追加文本缓冲 |
| `EventLoop` | ~200 | winit + vsync 等待 |
| 输入系统（键盘/鼠标/滚轮） | ~300 | 复用 winit 事件 |
| 主题/样式 | ~200 | 颜色、间距、字体大小 |

**总计 ~2,700 行** 可以跑起 demo。

### 不需要写的

- ❌ `widget` 抽象层 — 直接用 `Block` 表示一切
- ❌ `layout()` 每帧重算 — 只在内容变化时算
- ❌ `Tree diff` / `Virtual DOM` — 没有树，只有 GPU buffer
- ❌ `CSS 解析` — 样式用 Rust 代码表达
- ❌ `字体加载器` — 复用 skrifa + fontdb

## 第一阶段目标

跑起来的最小闭环：

```
winit 窗口 800x600
  ├─ 硬编码渲染 "Hello, DeepX!"
  ├─ 支持 wgpu 纹理图集（英文字母 + 中文基本字）
  ├─ 支持单行文本追加（模拟流式输出）
  └─ 按 Space 键追加 token
```

约 800 行代码，不依赖任何 GUI 框架。
