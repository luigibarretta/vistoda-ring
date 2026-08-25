use reqwest::Method;
use serde_json::json;

use super::{RingClient, controls::only_device};
use crate::{BridgeError, ring_protocol::API_VERSION};

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
        )
        .await?;
        Ok(device.id().to_string())
    }
}
