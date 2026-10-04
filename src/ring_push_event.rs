use std::{collections::VecDeque, time::Duration};

use serde::Serialize;
use tokio::sync::{Mutex, watch};

const EVENT_LIMIT: usize = 128;
/// Push and history can both report one unlock; HA treats them as one too.
const UNLOCK_DEDUPE_SECONDS: i64 = 15;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RingPushEventKind {
    Ding,
    IntercomUnlock,
}

/// Who unlocked, as recorded in Ring's event history; push events carry none.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct UnlockDetail {
    /// One of Ring's unlock origins: `user`, `device`, `code` or `delivery`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    /// Ring's display name of the person who unlocked, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actor: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RingPushEvent {
    pub sequence: u64,
    pub event_type: RingPushEventKind,
    pub occurred_at: i64,
    #[serde(flatten)]
    pub detail: UnlockDetail,
}

#[derive(Debug, Serialize)]
pub struct RingPushEventBatch {
    pub device_id: String,
    pub cursor_reset: bool,
    pub events: Vec<RingPushEvent>,
    pub next_sequence: u64,
    pub generation: String,
    pub connected: bool,
}

struct EventState {
    next_sequence: u64,
    events: VecDeque<RingPushEvent>,
}

pub struct RingPushEvents {
    state: Mutex<EventState>,
    changed: watch::Sender<u64>,
    generation: String,
}

impl Default for RingPushEvents {
    fn default() -> Self {
        let (changed, _) = watch::channel(0);
        Self {
            state: Mutex::new(EventState {
                next_sequence: 1,
                events: VecDeque::with_capacity(EVENT_LIMIT),
            }),
            changed,
            generation: uuid::Uuid::new_v4().to_string(),
        }
    }
}

impl RingPushEvents {
    pub async fn publish(&self, event_type: RingPushEventKind, occurred_at: i64) -> bool {
        self.publish_detailed(event_type, occurred_at, UnlockDetail::default())
            .await
    }

    /// Returns false when the same unlock was already published.
    pub async fn publish_detailed(
        &self,
        event_type: RingPushEventKind,
        occurred_at: i64,
        detail: UnlockDetail,
    ) -> bool {
        let mut state = self.state.lock().await;
        if event_type == RingPushEventKind::IntercomUnlock
            && state.events.iter().any(|event| {
                event.event_type == event_type
                    && (event.occurred_at - occurred_at).abs() <= UNLOCK_DEDUPE_SECONDS
            })
        {
            return false;
        }
        let sequence = state.next_sequence;
        state.next_sequence = state.next_sequence.saturating_add(1);
        state.events.push_back(RingPushEvent {
            sequence,
            event_type,
            occurred_at,
            detail,
        });
        while state.events.len() > EVENT_LIMIT {
            state.events.pop_front();
        }
        drop(state);
        self.changed.send_replace(sequence);
        true
    }

    pub async fn wait_after(&self, after: u64, wait: Duration) -> Vec<RingPushEvent> {
        let mut receiver = self.changed.subscribe();
        let ready = *receiver.borrow() > after || !self.events_after(after).await.is_empty();
        if !ready && !wait.is_zero() {
            let _ignored = tokio::time::timeout(wait, receiver.changed()).await;
        }
        self.events_after(after).await
    }

    pub async fn latest_sequence(&self) -> u64 {
        self.state.lock().await.next_sequence.saturating_sub(1)
    }

    pub fn generation(&self) -> &str {
        &self.generation
    }

    async fn events_after(&self, after: u64) -> Vec<RingPushEvent> {
        self.state
            .lock()
            .await
            .events
            .iter()
            .filter(|event| event.sequence > after)
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::{RingPushEventKind, RingPushEvents, UnlockDetail};
    use std::time::Duration;

    #[tokio::test]
    async fn cursors_are_monotonic_and_old_events_are_not_replayed() {
        let events = RingPushEvents::default();
        assert!(events.publish(RingPushEventKind::Ding, 10).await);
        assert!(events.publish(RingPushEventKind::IntercomUnlock, 11).await);
        assert!(!events.publish(RingPushEventKind::IntercomUnlock, 20).await);
        assert!(events.publish(RingPushEventKind::Ding, 11).await);
        let batch = events.wait_after(1, Duration::ZERO).await;
        assert_eq!(batch.len(), 2);
        assert_eq!(batch[0].sequence, 2);
        assert_eq!(batch[0].event_type, RingPushEventKind::IntercomUnlock);
        let plain = serde_json::to_value(&batch[0]).unwrap_or_default();
        assert!(plain.get("origin").is_none() && plain.get("actor").is_none());
        let detail = UnlockDetail {
            origin: Some("user".into()),
            actor: Some("Luigi".into()),
        };
        let events = RingPushEvents::default();
        assert!(
            events
                .publish_detailed(RingPushEventKind::IntercomUnlock, 50, detail)
                .await
        );
        let shown = serde_json::to_value(&events.wait_after(0, Duration::ZERO).await[0])
            .unwrap_or_default();
        assert_eq!(shown["origin"], "user");
        assert_eq!(shown["actor"], "Luigi");
    }
}
