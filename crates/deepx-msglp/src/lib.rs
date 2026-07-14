//! deepx-msglp: message-loop driver for the agent child process.
//!
//! The primary production Loop is [`new::loop_core::Loop`] (Ring architecture).
//! It reads [`Ui2Agent`] frames via an mpsc channel fed by a background I/O
//! thread, and writes [`Agent2Ui`] frames via a channel consumed by a background
//! writer thread. It drives the full user-input → gate → tools → response
//! pipeline through a set of pluggable `Engine` implementations.
//!
//! ## Architecture
//!
//! ```text
//! Loop (process-level)
//!  ├─ I/O: cmd_rx, event_tx
//!  ├─ Signal: cancel, phase, pending, writer_dead
//!  ├─ Session: SessionBundle { agent, stats, turn, tool }
//!  └─ Stateless engines: session_eng, input, compact, misc, notify
//! ```
//!
//! See [`new::engine`] for the `Engine` trait and extension points.

pub mod agent;
mod conflict;
mod dashboard;
pub mod lifecycle;
pub mod logger;
mod notification;
#[cfg(windows)]
mod toast_com;
pub mod util;
pub mod new; // Ring-architecture Loop (primary)
