use std::{collections::VecDeque, time::Duration};

use serde::Serialize;
use tokio::sync::{Mutex, watch};

const EVENT_LIMIT: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RingPushEventKind {
    Ding,
    IntercomUnlock,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RingPushEvent {
    pub sequence: u64,
    pub event_type: RingPushEventKind,
    pub occurred_at: i64,
}

#[derive(Debug, Serialize)]
pub struct RingPushEventBatch {
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
    pub async fn publish(&self, event_type: RingPushEventKind, occurred_at: i64) {
        let mut state = self.state.lock().await;
        let sequence = state.next_sequence;
        state.next_sequence = state.next_sequence.saturating_add(1);
        state.events.push_back(RingPushEvent {
            sequence,
            event_type,
            occurred_at,
        });
        while state.events.len() > EVENT_LIMIT {
            state.events.pop_front();
        }
        drop(state);
        self.changed.send_replace(sequence);
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
    use super::{RingPushEventKind, RingPushEvents};
    use std::time::Duration;

    #[tokio::test]
    async fn cursors_are_monotonic_and_old_events_are_not_replayed() {
        let events = RingPushEvents::default();
        events.publish(RingPushEventKind::Ding, 10).await;
        events.publish(RingPushEventKind::IntercomUnlock, 11).await;
        let batch = events.wait_after(1, Duration::ZERO).await;
        assert_eq!(batch.len(), 1);
        assert_eq!(batch[0].sequence, 2);
        assert_eq!(batch[0].event_type, RingPushEventKind::IntercomUnlock);
    }
}
