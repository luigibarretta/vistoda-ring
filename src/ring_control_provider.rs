use reqwest::Method;
use serde_json::{Value, json};
use zeroize::Zeroizing;

use super::{RingClient, access_value, invalidate_auth};
use crate::{
    BridgeError,
    ring_control::{RingDeviceStatus, UnlockResponse, VolumeUpdate},
    ring_http::checked_body,
    ring_protocol::USER_AGENT,
    ring_wire::RingIntercomIdentity,
};

const CONTROL_BODY_LIMIT: usize = 64 * 1024;

impl RingClient {
    pub(crate) fn with_lifecycle_guard(
        mut self,
        guard: tokio::sync::OwnedRwLockReadGuard<()>,
    ) -> Self {
        self.lifecycle_guard = Some(std::sync::Arc::new(guard));
        self
    }
    pub(crate) fn filter_devices(
        &self,
        devices: Vec<RingIntercomIdentity>,
    ) -> Vec<RingIntercomIdentity> {
        devices
            .into_iter()
            .filter(|device| self.selected_device_id.is_none_or(|id| device.id() == id))
            .collect()
    }
    pub(crate) fn scoped(&self, device_id: Option<u64>) -> Self {
        let mut client = self.clone();
        client.selected_device_id = device_id;
        client
    }

    pub async fn device_status(&self) -> Result<RingDeviceStatus, BridgeError> {
        let device = only_device(self.discover_intercoms().await?)?;
        let (doorbell_volume, mic_volume, voice_volume) = device.volumes();
        let last_activity = self.latest_activity(&device).await.ok().flatten();
        Ok(RingDeviceStatus {
            device_id: device.id().to_string(),
            battery: device.battery(),
            online: device.online(),
            doorbell_volume,
            mic_volume,
            voice_volume,
            last_activity,
        })
    }

    pub async fn unlock(&self) -> Result<(), BridgeError> {
        self.unlock_expected(None).await
    }

    pub async fn unlock_expected(
        &self,
        expected_device_id: Option<&str>,
    ) -> Result<(), BridgeError> {
        let device = self.verified_device(expected_device_id).await?;
        let endpoint = format!(
            "{}/commands/v1/devices/{}/device_rpc",
            self.endpoints.api_root,
            device.id()
        );
        let body = self
            .vendor_request(
                Method::PUT,
                endpoint,
                Some(json!({
                    "command_name": "device_rpc",
                    "request": {
                        "id": uuid::Uuid::new_v4(),
                        "jsonrpc": "2.0",
                        "method": "unlock_door",
                        "params": {"door_id": 0, "user_id": -1}
                    }
                })),
                Vec::new(),
                "door unlock",
                CONTROL_BODY_LIMIT,
            )
            .await?;
        let response = serde_json::from_slice::<UnlockResponse>(&body)?;
        if response.result.code != 0 {
            return Err(BridgeError::Protocol("Ring rejected door unlock".into()));
        }
        tracing::info!("Ring Intercom door unlock accepted");
        Ok(())
    }

    pub async fn update_volume(&self, update: &VolumeUpdate) -> Result<(), BridgeError> {
        update.validate()?;
        let device = self
            .verified_device(update.expected_device_id.as_deref())
            .await?;
        let (method, endpoint, body, query, setting, value) =
            if let Some(value) = update.doorbell_volume {
                (
                    Method::PUT,
                    format!("{}/doorbots/{}", self.endpoints.client_api, device.id()),
                    None,
                    vec![(
                        "doorbot[settings][doorbell_volume]".into(),
                        value.to_string(),
                    )],
                    "doorbell",
                    value,
                )
            } else if let Some(value) = update.mic_volume {
                (
                    Method::PATCH,
                    format!(
                        "{}/devices/v1/devices/{}/settings",
                        self.endpoints.api_root,
                        device.id()
                    ),
                    Some(json!({"volume_settings": {"mic_volume": value}})),
                    Vec::new(),
                    "microphone",
                    value,
                )
            } else if let Some(value) = update.voice_volume {
                (
                    Method::PATCH,
                    format!(
                        "{}/devices/v1/devices/{}/settings",
                        self.endpoints.api_root,
                        device.id()
                    ),
                    Some(json!({"volume_settings": {"voice_volume": value}})),
                    Vec::new(),
                    "voice",
                    value,
                )
            } else {
                return Err(BridgeError::InvalidRequest(
                    "a volume value is required".into(),
                ));
            };
        self.vendor_request(
            method,
            endpoint,
            body,
            query,
            "volume update",
            CONTROL_BODY_LIMIT,
        )
        .await?;
        tracing::info!(setting, value, "Ring Intercom volume updated");
        Ok(())
    }

    pub(crate) async fn vendor_request(
        &self,
        method: Method,
        endpoint: String,
        json_body: Option<Value>,
        query: Vec<(String, String)>,
        operation: &'static str,
        response_limit: usize,
    ) -> Result<Zeroizing<Vec<u8>>, BridgeError> {
        let mut endpoint = reqwest::Url::parse(&endpoint)
            .map_err(|_| BridgeError::Protocol("control URL is invalid".into()))?;
        if !query.is_empty() {
            endpoint.query_pairs_mut().extend_pairs(&query);
        }
        let mut state = self.state.lock().await;
        for attempt in 0..=1 {
            self.ensure_authenticated(&mut state).await?;
            self.ensure_registered(&mut state).await?;
            let mut request = self
                .http
                .request(method.clone(), endpoint.clone())
                .bearer_auth(access_value(&state)?)
                .header("hardware_id", state.session.hardware_id().to_string())
                .header(reqwest::header::USER_AGENT, USER_AGENT);
            if let Some(body) = &json_body {
                request = request.json(body);
            }
            let response = request
                .send()
                .await
                .map_err(|error| BridgeError::Transport(operation, error))?;
            if response.status() == reqwest::StatusCode::UNAUTHORIZED && attempt == 0 {
                invalidate_auth(&mut state);
                continue;
            }
            drop(state);
            return checked_body(response, operation, response_limit).await;
        }
        Err(BridgeError::Protocol("control retry was exhausted".into()))
    }

    async fn latest_activity(
        &self,
        device: &RingIntercomIdentity,
    ) -> Result<Option<i64>, BridgeError> {
        Ok(self
            .activity_for_device(device, 20, None)
            .await?
            .0
            .into_iter()
            .map(|event| event.occurred_at)
            .max())
    }
}

pub(super) fn only_device(
    mut devices: Vec<RingIntercomIdentity>,
) -> Result<RingIntercomIdentity, BridgeError> {
    if devices.len() != 1 {
        return Err(BridgeError::Protocol("expected one Ring Intercom".into()));
    }
    devices
        .pop()
        .ok_or_else(|| BridgeError::Protocol("Ring Intercom is unavailable".into()))
}
