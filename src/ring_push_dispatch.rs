//! Per-device subscriptions and per-message handling: publish recognized
//! events and log a redacted shape for everything else, so a vendor format
//! change is diagnosable from logs.

use fcm_push_listener::Error as FcmError;

use super::RingPushService;
use crate::{
    error::BridgeError,
    ring_client::RingClient,
    ring_push_payload::{describe_push, parse_push_event},
    ring_push_support::unix_timestamp,
};

/// Vendor error texts are bounded before they reach the log.
const ERROR_TEXT_LIMIT: usize = 200;

pub(super) fn bounded(error: &impl std::fmt::Display) -> String {
    error.to_string().chars().take(ERROR_TEXT_LIMIT).collect()
}

impl RingPushService {
    /// Subscribe every Intercom to ding and unlock pushes; returns their device
    /// IDs. The unlock-alert gauge is 1 only when every Intercom is subscribed.
    pub(super) async fn subscribe_devices(
        &self,
        client: &RingClient,
    ) -> Result<Vec<String>, BridgeError> {
        self.metrics.unlock_alerts(false);
        let mut devices = Vec::new();
        let mut all_unlock_alerts = true;
        for id in self.events.ids()? {
            let scoped = client.scoped(id);
            let device = scoped.subscribe_push_events().await?;
            // Dings keep working without unlock alerts, so a refusal is reported, not fatal.
            match scoped.subscribe_unlock_alerts(&device).await {
                Ok(created) => tracing::info!(created, "Ring unlock alerts are subscribed"),
                Err(error) => {
                    all_unlock_alerts = false;
                    tracing::warn!(error = %error, "Ring unlock alerts could not be subscribed");
                }
            }
            drop(scoped);
            devices.push(device);
        }
        self.metrics
            .unlock_alerts(all_unlock_alerts && !devices.is_empty());
        Ok(devices)
    }

    pub(super) async fn handle_message(&self, device_ids: &[String], body: &[u8]) {
        let Some(event) = parse_push_event(body) else {
            self.metrics.ignored();
            // Category/action names are Ring constants; no IDs or text are logged.
            tracing::info!(shape = %describe_push(body), "Ring push message not recognized");
            return;
        };
        if !device_ids.contains(&event.device_id) {
            self.metrics.ignored();
            tracing::info!(event_type = ?event.event_type, "Ring push event for another device");
            return;
        }
        let occurred_at = event.occurred_at.unwrap_or_else(unix_timestamp);
        let id = event.device_id.parse::<u64>().ok();
        if let Ok(queue) = self.events.get(id) {
            queue.publish(event.event_type, occurred_at).await;
        }
        if device_ids.len() == 1
            && let Ok(queue) = self.events.get(None)
        {
            queue.publish(event.event_type, occurred_at).await;
        }
        self.metrics.received(event.event_type, occurred_at);
        tracing::info!(event_type = ?event.event_type, "Ring push event received");
    }

    /// One bad stanza must not drop the connection or every later event.
    pub(super) fn handle_undecryptable(&self, error: &FcmError) {
        self.metrics.undecryptable();
        tracing::warn!(error_class = %bounded(error), "Ring push message could not be decrypted");
    }
}
