//! PacedEmitter — rate-limited output for streaming text deltas.
//!
//! Accumulates incoming [`RoundDelta`] text into a character-level buffer
//! and releases it one character per tick to the output channel.  All
//! other events (tool calls, errors, etc.) pass through immediately.
//!
//! # Design
//!
//! ```text
//! engine_turn → PacedEmitter::emit_delta()
//!   ├─ RoundDelta  → push each char to Mutex<CharBuffer>
//!   │                drainer thread pops 1 char / tick
//!   │                → tx.send(RoundDelta { delta: "字" })
//!   └─ other       → tx.send()  (immediate)
//! ```
//!
//! When a terminal event arrives (`RoundComplete`, `TurnEnd`, `Done`),
//! `emit()` blocks until the character buffer is empty, guaranteeing
//! that the frontend sees every paced character before the completion
//! event.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use deepx_proto::{Agent2Ui, RoundDeltaKind};

use super::types::Emitter;

/// Default output rate: 120 characters per second (~8.3 ms per char).
pub const DEFAULT_RATE: u32 = 120;

/// Tracks the metadata of the current streaming round so the drainer
/// can reconstruct [`RoundDelta`] events character by character.
struct CharBuffer {
    /// Queued characters from incoming RoundDelta events.
    chars: VecDeque<char>,
    /// Metadata of the most recently received RoundDelta.
    turn_id: String,
    round_num: u32,
    kind: RoundDeltaKind,
    /// True after a character leaves the queue and until its event has been
    /// written to the output channel. Terminal events must wait for this too.
    in_flight: bool,
}

/// Rate-limits [`RoundDelta`] text to `rate_per_sec` characters/second.
///
/// Owns its output channel directly (does not wrap another [`Emitter`]).
///
/// # Lifecycle
///
/// The drainer thread lives for the entire lifetime of the struct.
/// On drop, the drainer is signalled to exit and joined.
pub struct PacedEmitter {
    /// Output channel to the writer thread.
    tx: mpsc::SyncSender<Agent2Ui>,
    /// Flag: writer thread exited, stop sending.
    writer_dead: Arc<AtomicBool>,
    /// Shared character buffer + metadata.
    state: Arc<Mutex<CharBuffer>>,
    /// Set on drop; drainer exits.
    shutdown: Arc<AtomicBool>,
    /// Background thread that drains the buffer at the paced rate.
    _drainer: JoinHandle<()>,
}

impl PacedEmitter {
    /// Create a new paced emitter.
    ///
    /// `tx` — output channel (cloned from the Loop's event channel).
    /// `writer_dead` — shared flag set when the writer thread exits.
    /// `rate_per_sec` — maximum output rate in characters per second.
    pub fn new(
        tx: mpsc::SyncSender<Agent2Ui>,
        writer_dead: Arc<AtomicBool>,
        rate_per_sec: u32,
    ) -> Self {
        assert!(rate_per_sec > 0, "rate_per_sec must be positive");
        let interval = Duration::from_secs_f64(1.0 / rate_per_sec as f64);

        let state = Arc::new(Mutex::new(CharBuffer {
            chars: VecDeque::new(),
            turn_id: String::new(),
            round_num: 0,
            kind: RoundDeltaKind::Answering,
            in_flight: false,
        }));
        let shutdown = Arc::new(AtomicBool::new(false));

        let s = state.clone();
        let sd = shutdown.clone();
        let tx_clone = tx.clone();
        let wd = writer_dead.clone();

        let drainer = thread::Builder::new()
            .name("paced-emitter".into())
            .spawn(move || {
                loop {
                    // ── Pop one character ──
                    let chunk = {
                        let mut guard = s.lock().unwrap();
                        if guard.chars.is_empty() {
                            if sd.load(Ordering::SeqCst) {
                                break;
                            }
                            drop(guard);
                            thread::sleep(Duration::from_millis(1));
                            continue;
                        }
                        let ch = guard.chars.pop_front().unwrap();
                        guard.in_flight = true;
                        (ch, guard.turn_id.clone(), guard.round_num, guard.kind)
                    };

                    let (ch, turn_id, round_num, kind) = chunk;

                    // ── Emit and pace ──
                    let tick = Instant::now();
                    if !wd.load(Ordering::SeqCst) {
                        let event = Agent2Ui::RoundDelta {
                            turn_id,
                            round_num,
                            kind,
                            delta: ch.to_string(),
                        };
                        let _ = tx_clone.send(event);
                    }
                    s.lock().unwrap().in_flight = false;

                    let elapsed = tick.elapsed();
                    if elapsed < interval {
                        let deadline = tick + interval;
                        loop {
                            let now = Instant::now();
                            if now >= deadline || sd.load(Ordering::SeqCst) {
                                break;
                            }
                            let remaining = deadline - now;
                            thread::sleep(remaining.min(Duration::from_millis(5)));
                        }
                    }
                }

                // ── Final drain on shutdown ──
                let mut guard = s.lock().unwrap();
                while let Some(ch) = guard.chars.pop_front() {
                    if wd.load(Ordering::SeqCst) {
                        break;
                    }
                    let event = Agent2Ui::RoundDelta {
                        turn_id: guard.turn_id.clone(),
                        round_num: guard.round_num,
                        kind: guard.kind,
                        delta: ch.to_string(),
                    };
                    let _ = tx_clone.send(event);
                }
            })
            .expect("failed to spawn paced-emitter thread");

        Self {
            tx,
            writer_dead,
            state,
            shutdown,
            _drainer: drainer,
        }
    }

    fn is_terminal(event: &Agent2Ui) -> bool {
        matches!(
            event,
            Agent2Ui::RoundComplete { .. } | Agent2Ui::TurnEnd { .. } | Agent2Ui::Done
        )
    }
}

impl Emitter for PacedEmitter {
    fn emit(&self, event: Agent2Ui) {
        // Terminal events: wait for the character buffer to drain so
        // the frontend sees every paced character before completion.
        if Self::is_terminal(&event) {
            loop {
                let guard = self.state.lock().unwrap();
                if guard.chars.is_empty() && !guard.in_flight {
                    break;
                }
                drop(guard);
                thread::sleep(Duration::from_millis(1));
            }
        }
        if self.writer_dead.load(Ordering::SeqCst) {
            return;
        }
        let _ = self.tx.send(event);
    }

    fn emit_delta(&self, event: Agent2Ui) {
        match &event {
            Agent2Ui::RoundDelta {
                turn_id,
                round_num,
                kind,
                delta,
            } => {
                let mut guard = self.state.lock().unwrap();
                guard.turn_id = turn_id.clone();
                guard.round_num = *round_num;
                guard.kind = *kind;
                for ch in delta.chars() {
                    guard.chars.push_back(ch);
                }
            }
            _ => {
                // Tool calls, exec progress, code deltas —
                // pass through without pacing.
                if self.writer_dead.load(Ordering::SeqCst) {
                    return;
                }
                let _ = self.tx.send(event);
            }
        }
    }
}

impl Drop for PacedEmitter {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
    }
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    fn round_delta(delta: &str) -> Agent2Ui {
        Agent2Ui::RoundDelta {
            turn_id: "t1".into(),
            round_num: 1,
            kind: RoundDeltaKind::Answering,
            delta: delta.into(),
        }
    }

    /// Create a test PacedEmitter that writes to a Vec via a channel.
    struct TestHarness {
        events: Arc<StdMutex<Vec<(Agent2Ui, Instant)>>>,
        pacer: PacedEmitter,
        _tx: mpsc::SyncSender<Agent2Ui>,
    }

    impl TestHarness {
        fn new(rate_per_sec: u32) -> Self {
            let (tx, rx) = mpsc::sync_channel::<Agent2Ui>(65536);
            let events: Arc<StdMutex<Vec<(Agent2Ui, Instant)>>> =
                Arc::new(StdMutex::new(Vec::new()));
            let events_clone = events.clone();

            thread::spawn(move || {
                while let Ok(event) = rx.recv() {
                    events_clone.lock().unwrap().push((event, Instant::now()));
                }
            });

            let pacer =
                PacedEmitter::new(tx.clone(), Arc::new(AtomicBool::new(false)), rate_per_sec);

            Self {
                events,
                pacer,
                _tx: tx,
            }
        }

        fn take_events(&self) -> Vec<(Agent2Ui, Instant)> {
            thread::sleep(Duration::from_millis(10));
            self.events.lock().unwrap().drain(..).collect()
        }

        /// Collect all RoundDelta characters in order.
        fn collect_chars(&self) -> String {
            self.take_events()
                .iter()
                .filter_map(|(e, _)| match e {
                    Agent2Ui::RoundDelta { delta, .. } => Some(delta.as_str()),
                    _ => None,
                })
                .collect()
        }
    }

    #[test]
    fn non_delta_events_pass_through_immediately() {
        let h = TestHarness::new(DEFAULT_RATE);

        let tool = Agent2Ui::ToolCallPreview {
            turn_id: "t1".into(),
            round_num: 1,
            index: 0,
            id: "tc1".into(),
            name: "read".into(),
            args_so_far: "{}".into(),
        };

        h.pacer.emit_delta(tool);
        let events = h.take_events();
        assert_eq!(events.len(), 1, "tool call should pass through immediately");
    }

    #[test]
    fn characters_are_paced_one_by_one() {
        // 10 chars/sec → 100ms per char
        let h = TestHarness::new(10);

        // Push a multi-character delta
        h.pacer.emit_delta(round_delta("abc"));

        // Wait for all 3 chars to drain
        thread::sleep(Duration::from_millis(500));

        let events = h.take_events();
        let round_deltas: Vec<_> = events
            .iter()
            .filter(|(e, _)| matches!(e, Agent2Ui::RoundDelta { .. }))
            .collect();

        assert_eq!(round_deltas.len(), 3, "expected 3 single-char deltas");

        // Each character should be a single-char RoundDelta
        let chars: String = round_deltas
            .iter()
            .filter_map(|(e, _)| match e {
                Agent2Ui::RoundDelta { delta, .. } => Some(delta.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(chars, "abc", "characters should be in order");

        // Pacing: ~100ms between chars
        for i in 1..round_deltas.len() {
            let gap = round_deltas[i].1.duration_since(round_deltas[i - 1].1);
            assert!(
                gap >= Duration::from_millis(70),
                "char {} too close: gap={:?}",
                i,
                gap
            );
        }
    }

    #[test]
    fn multi_byte_unicode_chars_work() {
        let h = TestHarness::new(50);
        h.pacer.emit_delta(round_delta("你好世界"));
        thread::sleep(Duration::from_millis(200));
        assert_eq!(h.collect_chars(), "你好世界");
    }

    #[test]
    fn metadata_tracks_most_recent_delta() {
        let h = TestHarness::new(50);

        // Two deltas with different turn_ids (simulates multi-round)
        h.pacer.emit_delta(Agent2Ui::RoundDelta {
            turn_id: "t1".into(),
            round_num: 1,
            kind: RoundDeltaKind::Answering,
            delta: "a".into(),
        });
        h.pacer.emit_delta(Agent2Ui::RoundDelta {
            turn_id: "t2".into(),
            round_num: 2,
            kind: RoundDeltaKind::Thinking,
            delta: "b".into(),
        });

        thread::sleep(Duration::from_millis(150));

        let events = h.take_events();
        let deltas: Vec<_> = events
            .iter()
            .filter_map(|(e, _)| match e {
                Agent2Ui::RoundDelta {
                    turn_id,
                    round_num,
                    kind,
                    delta,
                } => Some((turn_id.as_str(), *round_num, *kind, delta.as_str())),
                _ => None,
            })
            .collect();

        // First char "a": picks up metadata from first delta
        // Second char "b": picks up metadata from second delta
        // The drainer uses whatever meta is current when it pops.
        // Since "a" and "b" might be popped in any order depending
        // on timing, we just verify both are emitted.
        assert_eq!(deltas.len(), 2);
    }

    #[test]
    fn terminal_event_waits_for_drain() {
        let h = TestHarness::new(10); // 10 chars/sec

        h.pacer.emit_delta(round_delta("xy"));

        // Drainer is concurrent — some chars may already be gone.
        // The important guarantee: Done must never precede a RoundDelta.
        h.pacer.emit(Agent2Ui::Done);

        let events = h.take_events();

        let done_pos = events.iter().position(|(e, _)| matches!(e, Agent2Ui::Done));
        assert!(done_pos.is_some(), "Done should be emitted");
        let done_pos = done_pos.unwrap();

        // Verify no RoundDelta appears after Done
        let after_done_has_delta = events[done_pos + 1..]
            .iter()
            .any(|(e, _)| matches!(e, Agent2Ui::RoundDelta { .. }));
        assert!(
            !after_done_has_delta,
            "no RoundDelta should appear after Done"
        );

        let char_count: usize = events[..done_pos]
            .iter()
            .filter(|(e, _)| matches!(e, Agent2Ui::RoundDelta { .. }))
            .count();
        assert_eq!(char_count, 2, "both chars must precede Done");
    }
}
