//! Frontend connection management and message routing.
//!
//! Accepts frontend connections, reads FrontendToDaemon frames,
//! routes them to the correct agent, and broadcasts AgentEvents
//! back to subscribed frontends.

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Write;

use deepx_proto::{FrontendToDaemon, DaemonToFrontend};

use crate::pool::{AgentPool, AgentEvent};

type ConnId = usize;

/// A connected frontend.
struct FrontendConn {
    stream: Box<dyn Write + Send>,
    /// Seeds this frontend is interested in.
    subscriptions: HashSet<String>,
}

/// Maximum events buffered per seed (ring buffer capacity).
const RING_BUFFER_CAP: usize = 256;

/// Manages frontend connections and routes messages.
pub struct FrontendManager {
    next_id: ConnId,
    connections: HashMap<ConnId, FrontendConn>,
    /// Seed → set of frontend IDs subscribed to it.
    subscriptions: HashMap<String, HashSet<ConnId>>,
    /// Per-seed ring buffer of recent events: (seq_id, event).
    ring_buffer: HashMap<String, VecDeque<(u64, AgentEvent)>>,
    /// Monotonically increasing sequence number for each event.
    seq_id: u64,
}

impl FrontendManager {
    pub fn new() -> Self {
        Self {
            next_id: 0,
            connections: HashMap::new(),
            subscriptions: HashMap::new(),
            ring_buffer: HashMap::new(),
            seq_id: 0,
        }
    }

    /// Register a new frontend connection.
    pub fn add(&mut self, stream: Box<dyn Write + Send>) -> ConnId {
        let id = self.next_id;
        self.next_id += 1;
        self.connections.insert(id, FrontendConn {
            stream,
            subscriptions: HashSet::new(),
        });
        log::info!("[FRONTEND] connection {} added", id);
        id
    }

    /// Remove a frontend connection.
    pub fn remove(&mut self, id: ConnId) {
        if let Some(conn) = self.connections.remove(&id) {
            for seed in &conn.subscriptions {
                if let Some(subs) = self.subscriptions.get_mut(seed) {
                    subs.remove(&id);
                }
            }
            log::info!("[FRONTEND] connection {} removed", id);
        }
    }

    /// Subscribe a frontend to a session seed.
    pub fn subscribe(&mut self, conn_id: ConnId, seed: &str) {
        if let Some(conn) = self.connections.get_mut(&conn_id) {
            conn.subscriptions.insert(seed.to_string());
        }
        self.subscriptions
            .entry(seed.to_string())
            .or_default()
            .insert(conn_id);
    }

    /// Broadcast an AgentEvent to all frontends subscribed to its seed.
    pub fn broadcast(&mut self, event: &AgentEvent) {
        // Push into per-seed ring buffer with sequence id.
        let entry = (self.seq_id, event.clone());
        self.seq_id += 1;
        let buf = self
            .ring_buffer
            .entry(event.seed.clone())
            .or_default();
        if buf.len() >= RING_BUFFER_CAP {
            buf.pop_front();
        }
        buf.push_back(entry);

        let frame = DaemonToFrontend {
            seed: event.seed.clone(),
            event: event.event.clone(),
        };
        let payload = match serde_json::to_vec(&frame) {
            Ok(p) => p,
            Err(_) => return,
        };
        let len = payload.len() as u32;

        if let Some(subs) = self.subscriptions.get(&event.seed) {
            let mut dead: Vec<ConnId> = Vec::new();
            for &conn_id in subs {
                if let Some(conn) = self.connections.get_mut(&conn_id) {
                    let mut buf = Vec::with_capacity(4 + payload.len());
                    buf.extend_from_slice(&len.to_le_bytes());
                    buf.extend_from_slice(&payload);
                    if conn.stream.write_all(&buf).is_err() {
                        dead.push(conn_id);
                    }
                }
            }
            for id in dead {
                self.remove(id);
            }
        }
    }

    /// Replay all buffered events for a seed to a specific frontend connection.
    /// Used to catch up a late-joining frontend.
    pub fn replay_buffered(&mut self, conn_id: ConnId, seed: &str) {
        let conn = match self.connections.get_mut(&conn_id) {
            Some(c) => c,
            None => return,
        };
        let buf = match self.ring_buffer.get(seed) {
            Some(b) => b,
            None => return,
        };
        for (_seq_id, event) in buf {
            let frame = DaemonToFrontend {
                seed: event.seed.clone(),
                event: event.event.clone(),
            };
            let payload = match serde_json::to_vec(&frame) {
                Ok(p) => p,
                Err(_) => continue,
            };
            let len = payload.len() as u32;
            let mut wire = Vec::with_capacity(4 + payload.len());
            wire.extend_from_slice(&len.to_le_bytes());
            wire.extend_from_slice(&payload);
            if conn.stream.write_all(&wire).is_err() {
                // Connection dead; caller will clean up.
                return;
            }
        }
    }

    /// Process an incoming frame from a frontend: route to agent pool.
    pub fn handle_frame(
        &mut self,
        conn_id: ConnId,
        frame: FrontendToDaemon,
        pool: &AgentPool,
    ) -> Result<(), String> {
        // Auto-subscribe to the seed this frontend is talking to
        self.subscribe(conn_id, &frame.seed);

        // Route frame to agent
        pool.send_to_agent(&frame.seed, &frame.frame)
    }
}
