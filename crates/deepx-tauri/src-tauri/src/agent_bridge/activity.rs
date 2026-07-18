use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use deepx_proto::{SessionActivity, SessionActivityState};

#[derive(Clone, Default)]
pub struct SessionActivityTracker {
    inner: Arc<Mutex<TrackerState>>,
}

#[derive(Default)]
struct TrackerState {
    by_seed: HashMap<String, TrackedActivity>,
}

struct TrackedActivity {
    generation: u64,
    activity: SessionActivity,
}

impl SessionActivityTracker {
    pub fn begin(&self, seed: &str) -> (u64, SessionActivity) {
        let mut inner = self.inner.lock().unwrap_or_else(|error| error.into_inner());
        let previous = inner.by_seed.get(seed);
        let generation = previous.map_or(1, |entry| entry.generation.saturating_add(1));
        let seq = previous.map_or(1, |entry| entry.activity.seq.saturating_add(1));
        let activity = SessionActivity {
            seed: seed.to_string(),
            state: SessionActivityState::Starting,
            turn_id: None,
            seq,
            updated_at: now_millis(),
        };
        inner.by_seed.insert(
            seed.to_string(),
            TrackedActivity {
                generation,
                activity: activity.clone(),
            },
        );
        (generation, activity)
    }

    pub fn observe(
        &self,
        seed: &str,
        generation: u64,
        event: &serde_json::Value,
    ) -> Option<SessionActivity> {
        let mut inner = self.inner.lock().unwrap_or_else(|error| error.into_inner());
        let tracked = inner.by_seed.get_mut(seed)?;
        if tracked.generation != generation {
            return None;
        }
        let (state, turn_id) = transition(&tracked.activity, event)?;
        if tracked.activity.state == state && tracked.activity.turn_id == turn_id {
            return None;
        }
        tracked.activity.state = state;
        tracked.activity.turn_id = turn_id;
        tracked.activity.seq = tracked.activity.seq.saturating_add(1);
        tracked.activity.updated_at = now_millis();
        Some(tracked.activity.clone())
    }

    pub fn disconnect(&self, seed: &str, generation: u64) -> Option<SessionActivity> {
        let mut inner = self.inner.lock().unwrap_or_else(|error| error.into_inner());
        let tracked = inner.by_seed.get_mut(seed)?;
        if tracked.generation != generation
            || tracked.activity.state == SessionActivityState::Disconnected
        {
            return None;
        }
        tracked.activity.state = SessionActivityState::Disconnected;
        tracked.activity.turn_id = None;
        tracked.activity.seq = tracked.activity.seq.saturating_add(1);
        tracked.activity.updated_at = now_millis();
        Some(tracked.activity.clone())
    }

    pub fn snapshot(&self) -> Vec<SessionActivity> {
        let inner = self.inner.lock().unwrap_or_else(|error| error.into_inner());
        let mut activities: Vec<_> = inner
            .by_seed
            .values()
            .map(|tracked| tracked.activity.clone())
            .collect();
        activities.sort_by(|left, right| left.seed.cmp(&right.seed));
        activities
    }

    pub fn current(&self, seed: &str, generation: u64) -> Option<SessionActivity> {
        let inner = self.inner.lock().unwrap_or_else(|error| error.into_inner());
        let tracked = inner.by_seed.get(seed)?;
        (tracked.generation == generation).then(|| tracked.activity.clone())
    }
}

fn transition(
    current: &SessionActivity,
    event: &serde_json::Value,
) -> Option<(SessionActivityState, Option<String>)> {
    let event_type = event.get("type")?.as_str()?;
    let current_turn = current.turn_id.clone();
    match event_type {
        "ready" | "done" | "turn_end" | "cancelled" => Some((SessionActivityState::Idle, None)),
        "shutdown_ack" => Some((SessionActivityState::Disconnected, None)),
        "turn_start" => Some((
            SessionActivityState::Working,
            event
                .get("turn_id")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string),
        )),
        "permission_request" | "ask_user" | "plan_submitted" => {
            Some((SessionActivityState::WaitingUser, current_turn))
        }
        "ask_resolved" | "plan_resolved" | "round_delta" | "round_complete" | "tool_results"
        | "tool_exec_delta" | "exec_progress" | "tool_call_preview" | "code_delta" => {
            Some((SessionActivityState::Working, current_turn))
        }
        "compact_start" | "compact_delta" => Some((SessionActivityState::Working, current_turn)),
        "compact_end" if current_turn.is_none() => Some((SessionActivityState::Idle, None)),
        "compact_end" => Some((SessionActivityState::Working, current_turn)),
        _ => None,
    }
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use deepx_proto::SessionActivityState;
    use serde_json::json;

    use super::SessionActivityTracker;

    #[test]
    fn tracks_work_wait_and_idle_transitions_without_stream_spam() {
        let tracker = SessionActivityTracker::default();
        let (generation, starting) = tracker.begin("session-a");
        assert_eq!(starting.state, SessionActivityState::Starting);

        let working = tracker
            .observe(
                "session-a",
                generation,
                &json!({"type":"turn_start","turn_id":"t1"}),
            )
            .unwrap();
        assert_eq!(working.state, SessionActivityState::Working);
        assert_eq!(working.turn_id.as_deref(), Some("t1"));
        assert!(
            tracker
                .observe(
                    "session-a",
                    generation,
                    &json!({"type":"round_delta","turn_id":"t1"}),
                )
                .is_none()
        );

        let waiting = tracker
            .observe(
                "session-a",
                generation,
                &json!({"type":"permission_request"}),
            )
            .unwrap();
        assert_eq!(waiting.state, SessionActivityState::WaitingUser);

        let idle = tracker
            .observe("session-a", generation, &json!({"type":"done"}))
            .unwrap();
        assert_eq!(idle.state, SessionActivityState::Idle);
        assert_eq!(idle.turn_id, None);
        assert!(idle.seq > working.seq);
    }

    #[test]
    fn stale_process_exit_cannot_overwrite_a_new_generation() {
        let tracker = SessionActivityTracker::default();
        let (old_generation, _) = tracker.begin("session-a");
        let (new_generation, _) = tracker.begin("session-a");

        assert!(new_generation > old_generation);
        assert!(tracker.disconnect("session-a", old_generation).is_none());
        assert_eq!(tracker.snapshot()[0].state, SessionActivityState::Starting);
    }
}
