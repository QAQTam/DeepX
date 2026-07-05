//! Frontend connection management and message routing.
//!
//! Accepts frontend connections, reads FrontendToDaemon frames,
//! routes them to the correct agent, and broadcasts AgentEvents
//! back to subscribed frontends.

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Write;

use deepx_proto::{FrontendToDaemon, DaemonToFrontend, Ui2Agent, Agent2Ui, TurnData};

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

/// Cached session state for snapshot construction.
#[derive(Debug, Clone, Default)]
struct SessionCache {
    turns: Vec<TurnData>,
    tokens_used: u32,
    context_limit: u32,
}

/// Manages frontend connections and routes messages.
pub struct FrontendManager {
    next_id: ConnId,
    connections: HashMap<ConnId, FrontendConn>,
    /// Seed → set of frontend IDs subscribed to it.
    subscriptions: HashMap<String, HashSet<ConnId>>,
    /// Per-seed ring buffer of recent events: (seq_id, Agent2Ui event).
    ring_buffer: HashMap<String, VecDeque<(u64, Agent2Ui)>>,
    /// Per-seed cached session state for snapshot.
    session_cache: HashMap<String, SessionCache>,
    /// Monotonically increasing sequence number for each event.
    seq_id: u64,
    /// Pending reconnects: conn_id → (seed, last_seq). Delivered when cache is populated.
    pending_reconnects: HashMap<ConnId, (String, u64)>,
}

impl FrontendManager {
    pub fn new() -> Self {
        Self {
            next_id: 0,
            connections: HashMap::new(),
            subscriptions: HashMap::new(),
            ring_buffer: HashMap::new(),
            session_cache: HashMap::new(),
            seq_id: 0,
            pending_reconnects: HashMap::new(),
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

    /// Update the session cache from relevant agent events.
    fn update_cache(&mut self, seed: &str, event: &Agent2Ui) {
        let cache = self.session_cache.entry(seed.to_string()).or_default();
        match event {
            Agent2Ui::SessionRestored { turns, tokens_used, .. } => {
                cache.turns = turns.clone();
                cache.tokens_used = *tokens_used;
                // Deliver any pending reconnects now that we have session data
                self.flush_pending_reconnects(seed);
            }
            Agent2Ui::Dashboard { usage, context_limit, .. } => {
                if let Some(u) = usage {
                    cache.tokens_used = u.total_tokens;
                }
                cache.context_limit = *context_limit;
            }
            _ => {}
        }
    }

    fn flush_pending_reconnects(&mut self, seed: &str) {
        let reconnects: Vec<(ConnId, u64)> = self.pending_reconnects.iter()
            .filter(|(_, (s, _))| s == seed)
            .map(|(&conn_id, (_, last_seq))| (conn_id, *last_seq))
            .collect();
        for (conn_id, last_seq) in reconnects {
            self.pending_reconnects.remove(&conn_id);
            self.send_snapshot(conn_id, seed, last_seq);
        }
    }

    /// Broadcast an AgentEvent to all frontends subscribed to its seed.
    pub fn broadcast(&mut self, event: &AgentEvent) {
        // Update session cache from relevant events
        self.update_cache(&event.seed, &event.event);

        // Push into per-seed ring buffer with sequence id.
        let entry = (self.seq_id, event.event.clone());
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

    /// Build and send a Snapshot to a reconnecting frontend.
    /// Contains cached session state + buffered events since last_seq.
    pub fn send_snapshot(&mut self, conn_id: ConnId, seed: &str, last_seq: u64) {
        let conn = match self.connections.get_mut(&conn_id) {
            Some(c) => c,
            None => return,
        };

        let cache = self.session_cache.get(seed).cloned().unwrap_or_default();

        // Collect buffered events with seq_id > last_seq
        let mut buffered_events: Vec<Agent2Ui> = Vec::new();
        let mut has_more = false;
        let current_seq = self.seq_id;

        if let Some(buf) = self.ring_buffer.get(seed) {
            for (seq, ev) in buf {
                if *seq > last_seq {
                    buffered_events.push(ev.clone());
                }
            }
            // If the earliest buffered seq_id > last_seq + 1, we lost events
            if let Some((earliest_seq, _)) = buf.front() {
                if *earliest_seq > last_seq + 1 {
                    has_more = true;
                }
            }
        }

        let snapshot = Agent2Ui::Snapshot {
            seed: seed.to_string(),
            turns: cache.turns,
            tokens_used: cache.tokens_used,
            context_limit: cache.context_limit,
            buffered_events,
            seq_id: current_seq,
            has_more,
        };

        let frame = DaemonToFrontend {
            seed: seed.to_string(),
            event: snapshot,
        };

        let payload = match serde_json::to_vec(&frame) {
            Ok(p) => p,
            Err(_) => return,
        };
        let len = payload.len() as u32;
        let mut wire = Vec::with_capacity(4 + payload.len());
        wire.extend_from_slice(&len.to_le_bytes());
        wire.extend_from_slice(&payload);
        let _ = conn.stream.write_all(&wire);
    }

    /// Get all active connection IDs.
    pub fn conn_ids(&self) -> Vec<ConnId> {
        self.connections.keys().copied().collect()
    }

    /// Send a daemon-level event directly to a specific frontend (not routed via seed).
    pub fn send_control(&mut self, conn_id: ConnId, event: Agent2Ui) {
        let frame = DaemonToFrontend {
            seed: String::new(),
            event,
        };
        let payload = match serde_json::to_vec(&frame) {
            Ok(p) => p,
            Err(_) => return,
        };
        let len = payload.len() as u32;
        if let Some(conn) = self.connections.get_mut(&conn_id) {
            let mut buf = Vec::with_capacity(4 + payload.len());
            buf.extend_from_slice(&len.to_le_bytes());
            buf.extend_from_slice(&payload);
            let _ = conn.stream.write_all(&buf);
        }
    }

    /// Process an incoming frame from a frontend: route to agent pool.
    /// Intercepts daemon-level commands (Subscribe, Reconnect) before forwarding.
    pub fn handle_frame(
        &mut self,
        conn_id: ConnId,
        frame: FrontendToDaemon,
        pool: &AgentPool,
    ) -> Result<(), String> {
        // Auto-subscribe to the seed this frontend is talking to
        self.subscribe(conn_id, &frame.seed);

        // Intercept daemon-level commands
        match &frame.frame {
            Ui2Agent::Subscribe { .. } => {
                // Subscription already handled above — nothing more to do
                return Ok(());
            }
            Ui2Agent::Reconnect { seed, last_seq } => {
                // Don't send an empty snapshot — wait for the agent to emit SessionRestored first.
                // Save the reconnect request for later delivery when cache is populated.
                self.pending_reconnects.insert(conn_id, (seed.clone(), *last_seq));
                log::info!("[FRONTEND] reconnect seed={}, last_seq={} — deferred until agent emits SessionRestored",
                    &seed[..seed.floor_char_boundary(seed.len().min(8))], last_seq);
                return Ok(());
            }
            _ => {}
        }

        // Route regular frames to agent
        pool.send_to_agent(&frame.seed, &frame.frame)
    }
}
