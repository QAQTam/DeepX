//! 帧请求器 —— 管理渲染帧的请求与合并，配合 FrameRateLimiter 实现 120 FPS 限流。
//!
//! 移植自 Codex (codex-rs/tui/src/tui/frame_requester.rs)，简化如下：
//! - 移除 watch-delay 逻辑
//! - 移除 app-server 专用类型
//! - 使用 `Arc<AtomicBool>` 标志替代 tokio broadcast channel
//! - 主事件循环通过 `poll()` 检查是否需要绘制

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use super::frame_rate_limiter::FrameRateLimiter;

/// 帧请求器，线程安全，可在任意上下文中请求重绘。
///
/// # 设计思路
/// - 任何代码均可调用 `request_frame()` 标记需要帧
/// - 合并重复请求：连续多次 `request_frame()` 只触发一次绘制
/// - 受 FrameRateLimiter 限制，最多 120 FPS
/// - 主事件循环周期调用 `poll()`，若返回 `Some(now)` 则执行 `draw()`
#[derive(Debug, Clone)]
pub struct FrameRequester {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    /// 原子标志：是否需要新帧
    frame_needed: AtomicBool,
    /// 帧率限制器（需 Mutex 保护以实现内部可变性）
    limiter: Mutex<FrameRateLimiter>,
}

impl FrameRequester {
    /// 创建一个新的帧请求器。
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Inner {
                frame_needed: AtomicBool::new(false),
                limiter: Mutex::new(FrameRateLimiter::new()),
            }),
        }
    }

    /// 请求一帧渲染。
    ///
    /// 多次调用会被合并——只有在下一次 `poll()` 时才会触发一次绘制。
    /// 此方法无阻塞，可在任意线程安全地调用。
    pub fn request_frame(&self) {
        self.inner.frame_needed.store(true, Ordering::Release);
    }

    /// 由主事件循环调用，检查是否需要绘制新帧。
    ///
    /// # 返回值
    /// - `None`：当前无需绘制
    /// - `Some(deadline)`：需要绘制，但尚未超过帧率限制；`deadline` 是允许
    ///   绘制的最早时刻，调用方可 sleep 到该时刻后重试 `poll()`
    ///   若 `deadline == now` 则应立即执行绘制
    pub fn poll(&self) -> Option<Instant> {
        if !self.inner.frame_needed.load(Ordering::Acquire) {
            return None;
        }

        let now = Instant::now();
        let mut limiter = self.inner.limiter.lock().unwrap();
        let deadline = limiter.clamp_deadline(now);

        if deadline <= now {
            // 准予绘制：清除标志，更新限流器
            self.inner.frame_needed.store(false, Ordering::Release);
            limiter.mark_emitted();
            Some(now)
        } else {
            // 尚未到允许绘制的时间，返回 deadline 让调用方等待
            Some(deadline)
        }
    }

    /// 检查当前是否有待处理的帧请求（不涉及限流判断）。
    pub fn is_frame_needed(&self) -> bool {
        self.inner.frame_needed.load(Ordering::Acquire)
    }

    /// 重置内部状态：清空待处理标志并重置限流器。
    pub fn reset(&self) {
        self.inner.frame_needed.store(false, Ordering::Release);
        let mut limiter = self.inner.limiter.lock().unwrap();
        limiter.reset();
    }
}

impl Default for FrameRequester {
    fn default() -> Self {
        Self::new()
    }
}
