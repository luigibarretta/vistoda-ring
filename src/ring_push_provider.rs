//! Push token registration and Intercom ding subscription.
//!
//! The device record mirrors the official Ring Android app (3.113.0
//! `HostDevice`): payload dictionary 2.4.0, the app brand, the hardware ID and
//! the notification status, sent with the app identity headers Ring enforces
//! on its notification APIs. Ring re-creates the device record on every
//! session registration, so the token is re-sent after each one.

use reqwest::Method;
use serde_json::{Value, json};
use zeroize::Zeroizing;

use super::{ClientState, RingClient, access_value, controls::only_device, invalidate_auth};
use crate::{
    BridgeError,
    ring_http::checked_body,
    ring_protocol::{API_VERSION, USER_AGENT},
};

const PUSH_BODY_LIMIT: usize = 64 * 1024;
/// Payload dictionary of the official app (`PayloadVersion.TWO_PHASE_5`).
const PN_DICT_VERSION: &str = "2.4.0";

fn valid_token(token: &str) -> bool {
    (32..=4096).contains(&token.len()) && token.bytes().all(|byte| byte.is_ascii_graphic())
}

/// The record every client library sent until 2026; known to be accepted.
pub(super) fn legacy_device_body(token: &str) -> Value {
    json!({
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
    })
}

const fn unauthorized(error: &BridgeError) -> bool {
    matches!(error, BridgeError::VendorRejected { status: 401, .. })
}

/// The `PATCH clients_api/device` body for one FCM token.
pub(super) fn push_device_body(hardware_id: &str, token: &str) -> Value {
    json!({
        "device": {
            "os": "android",
            "app_brand": "ring",
            "hardware_id": hardware_id,
            "push_notification_token": token,
            "metadata": {
                "api_version": API_VERSION,
                "app_brand": "ring",
                "device_model": "Vistoda",
                "os_notification_status": "enabled",
                "pn_dict_version": PN_DICT_VERSION,
                "pn_service": "fcm"
            }
        }
    })
}

impl RingClient {
    pub async fn register_push_token(&self, token: &str) -> Result<(), BridgeError> {
        if !valid_token(token) {
            return Err(BridgeError::Protocol("FCM token is invalid".into()));
        }
        let mut state = self.state.lock().await;
        self.ensure_authenticated(&mut state).await?;
        self.ensure_registered(&mut state).await?;
        state.push_token = Some(Zeroizing::new(token.to_owned()));
        let first = self.patch_push_device(&state).await;
        if !first.as_ref().is_err_and(unauthorized) {
            drop(state);
            return first;
        }
        // One re-authentication, as for every other Ring request.
        invalidate_auth(&mut state);
        self.ensure_authenticated(&mut state).await?;
        self.ensure_registered(&mut state).await?;
        let result = self.patch_push_device(&state).await;
        drop(state);
        result
    }

    /// Sends the remembered token; a no-op before the first registration.
    /// Ring rejecting the app-format record (a 4xx other than 401) falls back
    /// to the legacy record, so push never ends up unregistered.
    pub(super) async fn patch_push_device(&self, state: &ClientState) -> Result<(), BridgeError> {
        let Some(token) = state.push_token.as_ref() else {
            return Ok(());
        };
        let hardware_id = state.session.hardware_id().to_string();
        let modern = push_device_body(&hardware_id, token);
        match self.send_device(state, &hardware_id, &modern).await {
            Err(BridgeError::VendorRejected { status, .. })
                if (400..500).contains(&status) && status != 401 =>
            {
                tracing::warn!(
                    status,
                    "Ring rejected the app-format push record; using the legacy record"
                );
                self.send_device(state, &hardware_id, &legacy_device_body(token))
                    .await
            }
            result => result,
        }
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

    async fn send_device(
        &self,
        state: &ClientState,
        hardware_id: &str,
        body: &Value,
    ) -> Result<(), BridgeError> {
        let response = self
            .http
            .patch(format!("{}/device", self.endpoints.client_api))
            .bearer_auth(access_value(state)?)
            .header("hardware_id", hardware_id)
            .header(reqwest::header::USER_AGENT, USER_AGENT)
            .header("App_Brand", "ring")
            .header("X-Ring-UserAgent-AppBrand", "ring")
            .header("X-Ring-UserAgent-App", "com.ringapp")
            .header("X-Ring-UserAgent-DeviceHardwareId", hardware_id)
            .json(body)
            .send()
            .await
            .map_err(|error| BridgeError::Transport("push token registration", error))?;
        checked_body(response, "push token registration", PUSH_BODY_LIMIT).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{legacy_device_body, push_device_body, valid_token};

    #[test]
    fn device_body_matches_the_official_app_record() {
        let body = push_device_body("hw-1", "token");
        let device = &body["device"];
        assert_eq!(device["app_brand"], "ring");
        assert_eq!(device["hardware_id"], "hw-1");
        assert_eq!(device["push_notification_token"], "token");
        assert_eq!(device["metadata"]["pn_dict_version"], "2.4.0");
        assert_eq!(device["metadata"]["os_notification_status"], "enabled");
        assert_eq!(device["metadata"]["pn_service"], "fcm");
    }

    #[test]
    fn legacy_body_keeps_the_known_accepted_record() {
        let body = legacy_device_body("token");
        assert_eq!(body["device"]["metadata"]["pn_dict_version"], "2.0.0");
        assert!(body["device"].get("app_brand").is_none());
    }

    #[test]
    fn tokens_are_bounded_printable_ascii() {
        assert!(valid_token(&"a".repeat(32)));
        assert!(!valid_token(&"a".repeat(31)));
        assert!(!valid_token(&format!("{} ", "a".repeat(40))));
        assert!(!valid_token(&"a".repeat(4097)));
    }
}
