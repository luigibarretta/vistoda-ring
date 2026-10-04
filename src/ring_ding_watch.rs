//! Push-silence watchdog wiring: history dings are judged against the push
//! dings each device queue has seen. They are never published as events.

use super::RingPushService;
use crate::ring_ding_watchdog::HistoryDing;

impl RingPushService {
    /// Mirrors `publish` routing: the device queue, plus the legacy alias
    /// queue on single-Intercom accounts. Misses are counted once per device.
    pub(super) fn watch_dings(
        &self,
        devices: &[String],
        device: &str,
        start: i64,
        dings: &[HistoryDing],
        now: i64,
    ) {
        let fresh = || dings.iter().filter(|ding| ding.occurred_at >= start);
        let mut queues = Vec::new();
        if let Ok(queue) = self.events.get(device.parse::<u64>().ok()) {
            queues.push(queue);
        }
        if devices.len() == 1
            && let Ok(queue) = self.events.get(None)
        {
            queues.push(queue);
        }
        for (index, queue) in queues.iter().enumerate() {
            let missed = queue.watch_history_dings(fresh(), now);
            if index == 0 && missed > 0 {
                self.metrics.missed_dings(missed);
                // Counts only: no device ID, event ID or time is logged.
                tracing::warn!(
                    missed,
                    "Ring history recorded a ding that push did not deliver"
                );
            }
        }
    }

    /// Logs a revoked or recovered Ring session once per change; the
    /// `vistoda_ring_reauth_required` gauge carries the current state.
    pub(super) fn reauth_state(&self, required: bool) {
        if !self.metrics.set_reauth_required(required) {
            return;
        }
        if required {
            tracing::warn!(
                "Ring rejected the stored session; re-enroll the account to restore push and history"
            );
        } else {
            tracing::info!("Ring session accepted again");
        }
    }
}
