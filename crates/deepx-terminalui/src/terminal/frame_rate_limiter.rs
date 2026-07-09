//! 帧率限制器 —— 基于时间戳的简单限流，确保渲染频率不超过 120 FPS。
//!
//! 移植自 Codex (codex-rs/tui/src/tui/frame_rate_limiter.rs)，移除 watch-delay 逻辑。

use std::time::{Duration, Instant};

/// 最小帧间隔：120 FPS ≈ 8.33 ms
const MIN_FRAME_INTERVAL: Duration = Duration::from_micros(8333);

/// 简单的帧率限制器。
///
/// 记录上一次发出帧的时间戳，通过 `clamp_deadline` 确保下一次帧
/// 不会早于最小间隔。
#[derive(Debug, Clone)]
pub struct FrameRateLimiter {
    last_emitted_at: Option<Instant>,
}

impl FrameRateLimiter {
    /// 创建一个新的帧率限制器，初始无上一帧记录。
    pub fn new() -> Self {
        Self {
            last_emitted_at: None,
        }
    }

    /// 根据当前时间与最小间隔，计算允许下一次帧最早发出的时间点。
    ///
    /// 如果 `last_emitted_at` 为 `None`（从未发出过帧），则直接返回 `now`。
    pub fn clamp_deadline(&self, now: Instant) -> Instant {
        match self.last_emitted_at {
            None => now,
            Some(last) => {
                let next = last + MIN_FRAME_INTERVAL;
                if next > now {
                    next
                } else {
                    now
                }
            }
        }
    }

    /// 标记当前时刻已经发出了一帧，用于更新 `last_emitted_at`。
    pub fn mark_emitted(&mut self) {
        self.last_emitted_at = Some(Instant::now());
    }

    /// 重置限制器，清空上一帧记录。
    pub fn reset(&mut self) {
        self.last_emitted_at = None;
    }
}

impl Default for FrameRateLimiter {
    fn default() -> Self {
        Self::new()
    }
}
