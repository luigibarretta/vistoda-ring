//! Per-device subscriptions and per-message handling: publish recognized
//! events and log a redacted shape for everything else, so a vendor format
//! change is diagnosable from logs.

use fcm_push_listener::Error as FcmError;

use super::RingPushService;
use crate::{
    error::BridgeError,
    ring_client::RingClient,
    ring_push_payload::{ParsedPushEvent, describe_push, parse_push_event},
    ring_push_support::unix_timestamp,
};

/// Vendor error texts are bounded before they reach the log.
const ERROR_TEXT_LIMIT: usize = 200;

pub(super) fn bounded(error: &impl std::fmt::Display) -> String {
    error.to_string().chars().take(ERROR_TEXT_LIMIT).collect()
}

impl RingPushService {
    /// Subscribe every Intercom to ding pushes; returns their device IDs.
    pub(super) async fn subscribe_devices(
        &self,
        client: &RingClient,
    ) -> Result<Vec<String>, BridgeError> {
        let mut devices = Vec::new();
        for id in self.events.ids()? {
            devices.push(client.scoped(id).subscribe_push_events().await?);
        }
        Ok(devices)
    }

    pub(super) async fn handle_message(&self, device_ids: &[String], body: &[u8]) {
        let Some(event) = parse_push_event(body) else {
            self.metrics.ignored();
            // Category/action names are Ring constants; no IDs or text are logged.
            tracing::info!(shape = %describe_push(body), "Ring push message not recognized");
            return;
        };
        self.publish(device_ids, event, "push").await;
    }

    /// Publish one event from push or history; queues drop a repeated unlock.
    pub(super) async fn publish(
        &self,
        device_ids: &[String],
        event: ParsedPushEvent,
        path: &'static str,
    ) {
        if !device_ids.contains(&event.device_id) {
            self.metrics.ignored();
            tracing::info!(event_type = ?event.event_type, path, "Ring event for another device");
            return;
        }
        let occurred_at = event.occurred_at.unwrap_or_else(unix_timestamp);
        let id = event.device_id.parse::<u64>().ok();
        let mut published = match self.events.get(id) {
            Ok(queue) => queue.publish(event.event_type, occurred_at).await,
            Err(_) => false,
        };
        if device_ids.len() == 1
            && let Ok(queue) = self.events.get(None)
        {
            published |= queue.publish(event.event_type, occurred_at).await;
        }
        if !published {
            tracing::info!(event_type = ?event.event_type, path, "Ring event already published");
            return;
        }
        self.metrics.received(event.event_type, occurred_at);
        tracing::info!(event_type = ?event.event_type, path, "Ring push event received");
    }

    /// One bad stanza must not drop the connection or every later event.
    pub(super) fn handle_undecryptable(&self, error: &FcmError) {
        self.metrics.undecryptable();
        tracing::warn!(error_class = %bounded(error), "Ring push message could not be decrypted");
    }
}
