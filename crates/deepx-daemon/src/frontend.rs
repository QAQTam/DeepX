//! Frontend connection management and message routing.
//!
//! Accepts frontend connections, reads FrontendToDaemon frames,
//! routes them to the correct agent, and broadcasts AgentEvents
//! back to subscribed frontends.

use std::collections::{HashMap, HashSet};
use std::io::Write;

use deepx_proto::{FrontendToDaemon, DaemonToFrontend};

use crate::pool::{AgentPool, AgentEvent};

type ConnId = usize;

/// A connected frontend.
struct FrontendConn {
    id: ConnId,
    stream: Box<dyn Write + Send>,
    /// Seeds this frontend is interested in.
    subscriptions: HashSet<String>,
}

/// Manages frontend connections and routes messages.
pub struct FrontendManager {
    next_id: ConnId,
    connections: HashMap<ConnId, FrontendConn>,
    /// Seed → set of frontend IDs subscribed to it.
    subscriptions: HashMap<String, HashSet<ConnId>>,
}

impl FrontendManager {
    pub fn new() -> Self {
        Self {
            next_id: 0,
            connections: HashMap::new(),
            subscriptions: HashMap::new(),
        }
    }

    /// Register a new frontend connection.
    pub fn add(&mut self, stream: Box<dyn Write + Send>) -> ConnId {
        let id = self.next_id;
        self.next_id += 1;
        self.connections.insert(id, FrontendConn {
            id,
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
