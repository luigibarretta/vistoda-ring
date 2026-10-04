//! Poll the Ring event history for Intercom unlocks. Ring does not push
//! unlocks made from the official app to third-party clients, but records
//! them there, so they are published like push events with a short delay.

use std::{
    collections::{HashMap, HashSet, hash_map::Entry},
    sync::Arc,
    time::Duration,
};

use super::RingPushService;
use crate::{
    error::BridgeError, ring_client::UnlockRecord, ring_provider::RingProvider,
    ring_push_event::RingPushEventKind, ring_push_payload::ParsedPushEvent,
    ring_push_support::unix_timestamp,
};

const POLL_SECONDS: i64 = 20;
const POLL_INTERVAL: Duration = Duration::from_secs(POLL_SECONDS.unsigned_abs());
/// After a failing start, unlocks are reported late but at most this old.
const MAX_REPLAY: i64 = 15 * 60;
const MAX_BACKOFF: Duration = Duration::from_mins(5);
/// Ring may index an unlock shortly after a newer one.
const LATE_TOLERANCE: i64 = 120;

/// Reports each history unlock once and never replays history older than
/// the watcher start, so restarts do not repeat notifications.
pub(super) struct UnlockCursor {
    floor: i64,
    seen: HashSet<String>,
}

impl UnlockCursor {
    pub(super) fn new(started_at: i64) -> Self {
        Self {
            floor: started_at,
            seen: HashSet::new(),
        }
    }

    pub(super) fn advance(&mut self, page: Vec<UnlockRecord>) -> Vec<UnlockRecord> {
        let fresh: Vec<UnlockRecord> = page
            .iter()
            .filter(|unlock| {
                !self.seen.contains(&unlock.event_id) && unlock.occurred_at >= self.floor
            })
            .cloned()
            .collect();
        if let Some(newest) = page.iter().map(|unlock| unlock.occurred_at).max() {
            self.floor = self.floor.max(newest - LATE_TOLERANCE);
        }
        self.seen = page.into_iter().map(|unlock| unlock.event_id).collect();
        fresh
    }
}

type UnlockPage = Result<(String, Vec<UnlockRecord>), BridgeError>;

impl RingPushService {
    pub(super) async fn watch_unlocks(self: Arc<Self>, provider: Arc<RingProvider>) {
        let started_at = unix_timestamp();
        let mut cursors = HashMap::new();
        let mut resolved = HashMap::new();
        let mut delay = POLL_INTERVAL;
        loop {
            tokio::time::sleep(delay).await;
            let read = self
                .poll_unlocks(&provider, started_at, &mut cursors, &mut resolved)
                .await;
            delay = if read {
                POLL_INTERVAL
            } else {
                delay.saturating_mul(2).min(MAX_BACKOFF)
            };
        }
    }

    /// Returns whether at least one Intercom was read. A failing Intercom
    /// never blocks the others.
    async fn poll_unlocks(
        &self,
        provider: &RingProvider,
        started_at: i64,
        cursors: &mut HashMap<String, UnlockCursor>,
        resolved: &mut HashMap<Option<u64>, String>,
    ) -> bool {
        let pages = match self.unlock_pages(provider, resolved).await {
            Ok(pages) => pages,
            Err(error) => {
                self.unlock_poll_failed(&error, None);
                return false;
            }
        };
        let devices: Vec<String> = pages
            .iter()
            .filter_map(|page| page.as_ref().ok().map(|(device, _)| device.clone()))
            .collect();
        // Only the first cycle may report unlocks since start (bounded); an
        // Intercom added later starts from now so its history is not replayed.
        let now = unix_timestamp();
        let floor = if cursors.is_empty() {
            started_at.max(now - MAX_REPLAY)
        } else {
            now - POLL_SECONDS
        };
        let mut read = false;
        for (index, page) in pages.into_iter().enumerate() {
            let (device, page) = match page {
                Ok(page) => page,
                Err(error) => {
                    self.unlock_poll_failed(&error, Some(index));
                    continue;
                }
            };
            read = true;
            let cursor = cursors
                .entry(device.clone())
                .or_insert_with(|| UnlockCursor::new(floor));
            for unlock in cursor.advance(page) {
                let event = ParsedPushEvent {
                    device_id: device.clone(),
                    event_type: RingPushEventKind::IntercomUnlock,
                    occurred_at: Some(unlock.occurred_at),
                };
                self.publish(&devices, event, unlock.detail, "history")
                    .await;
            }
        }
        read
    }

    async fn unlock_pages(
        &self,
        provider: &RingProvider,
        resolved: &mut HashMap<Option<u64>, String>,
    ) -> Result<Vec<UnlockPage>, BridgeError> {
        let client = provider.client().await?;
        let mut pages = Vec::new();
        for id in self.events.ids()? {
            if let Entry::Vacant(slot) = resolved.entry(id) {
                match client.scoped(id).selected_intercom_id().await {
                    Ok(device) => {
                        slot.insert(device);
                    }
                    Err(error) => {
                        pages.push(Err(error));
                        continue;
                    }
                }
            }
            if let Some(device) = resolved.get(&id) {
                let page = client.recent_unlocks(device).await;
                pages.push(page.map(|page| (device.clone(), page)));
            }
        }
        drop(client);
        Ok(pages)
    }

    /// Logs the Intercom's position, never its Ring ID.
    fn unlock_poll_failed(&self, error: &BridgeError, intercom: Option<usize>) {
        self.metrics.unlock_history_failed();
        tracing::warn!(error = %error, intercom, "Ring unlock history poll failed");
    }
}

#[cfg(test)]
mod tests {
    use super::UnlockCursor;
    use crate::{ring_client::UnlockRecord, ring_push_event::UnlockDetail};

    fn unlock(id: &str, at: i64) -> UnlockRecord {
        UnlockRecord {
            event_id: id.into(),
            occurred_at: at,
            detail: UnlockDetail::default(),
        }
    }

    #[test]
    fn reports_each_new_unlock_once_without_replaying_history() {
        let mut cursor = UnlockCursor::new(1_000);
        assert!(cursor.advance(vec![unlock("old", 900)]).is_empty());
        let fresh = cursor.advance(vec![unlock("a", 1_010), unlock("old", 900)]);
        assert_eq!(fresh, vec![unlock("a", 1_010)]);
        assert!(cursor.advance(vec![unlock("a", 1_010)]).is_empty());
        // Indexed late but within tolerance of the newest unlock.
        let late = cursor.advance(vec![
            unlock("b", 1_200),
            unlock("c", 1_090),
            unlock("a", 1_010),
        ]);
        assert_eq!(late, vec![unlock("b", 1_200), unlock("c", 1_090)]);
        assert!(cursor.advance(vec![unlock("d", 1_000)]).is_empty());
    }
}
