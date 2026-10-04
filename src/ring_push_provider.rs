use reqwest::Method;
use serde_json::json;

use super::{RingClient, controls::only_device};
use crate::{BridgeError, ring_protocol::API_VERSION};

const PUSH_BODY_LIMIT: usize = 64 * 1024;
/// Notification-settings event type behind the official app's "Unlock Alerts".
const UNLOCK_EVENT_TYPE: &str = "unlock";

impl RingClient {
    pub async fn register_push_token(&self, token: &str) -> Result<(), BridgeError> {
        if token.len() < 32
            || token.len() > 4096
            || !token.bytes().all(|byte| byte.is_ascii_graphic())
        {
            return Err(BridgeError::Protocol("FCM token is invalid".into()));
        }
        self.vendor_request(
            Method::PATCH,
            format!("{}/device", self.endpoints.client_api),
            Some(json!({
                "device": {
                    "metadata": {
                        "api_version": API_VERSION,
                        "device_model": "Vistoda",
                        "pn_dict_version": "2.0.0",
                        "pn_service": "fcm"
                    },
                    "os": "android",
                    "push_notification_token": token
                }
            })),
            Vec::new(),
            "push token registration",
            PUSH_BODY_LIMIT,
        )
        .await?;
        Ok(())
    }

    pub async fn subscribe_push_events(&self) -> Result<String, BridgeError> {
        let device = only_device(self.discover_intercoms().await?)?;
        self.vendor_request(
            Method::POST,
            format!(
                "{}/doorbots/{}/subscribe",
                self.endpoints.client_api,
                device.id()
            ),
            None,
            Vec::new(),
            "push event subscription",
            PUSH_BODY_LIMIT,
        )
        .await?;
        Ok(device.id().to_string())
    }

    /// Opt this client into Intercom unlock pushes. Ring sends
    /// `com.ring.pn.live-event.unlock` only for devices whose notification
    /// settings list the `unlock` event type (the official app's "Unlock
    /// Alerts" toggle); `doorbots/{id}/subscribe` covers dings only.
    /// Returns whether a subscription had to be created. An unreadable GET
    /// falls through to the idempotent POST, whose failure is the real error.
    pub async fn subscribe_unlock_alerts(&self, device_id: &str) -> Result<bool, BridgeError> {
        let event_types = format!(
            "{}/notification_settings/v1/doorbots/{device_id}/event_types",
            self.endpoints.api_root
        );
        let current = self
            .vendor_request(
                Method::GET,
                event_types.clone(),
                None,
                Vec::new(),
                "unlock alert settings",
                PUSH_BODY_LIMIT,
            )
            .await
            .map(|body| subscribed_types(&body))
            .unwrap_or_default();
        if current.iter().any(|kind| kind == UNLOCK_EVENT_TYPE) {
            return Ok(false);
        }
        self.vendor_request(
            Method::POST,
            format!("{event_types}/{UNLOCK_EVENT_TYPE}"),
            None,
            Vec::new(),
            "unlock alert subscription",
            PUSH_BODY_LIMIT,
        )
        .await?;
        Ok(true)
    }
}

/// The app models the response as `List<String>`; anything else reads as
/// "not subscribed", and the idempotent POST repairs it.
fn subscribed_types(body: &[u8]) -> Vec<String> {
    serde_json::from_slice(body).unwrap_or_default()
}
