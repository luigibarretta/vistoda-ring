use reqwest::Method;
use serde::Deserialize;
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
const EVENTS_BODY_LIMIT: usize = 512 * 1024;

#[derive(Deserialize)]
struct EventEnvelope {
    #[serde(default)]
    events: Vec<ActivityEvent>,
}

#[derive(Deserialize)]
struct ActivityEvent {
    created_at: String,
}

impl RingClient {
    pub async fn device_status(&self) -> Result<RingDeviceStatus, BridgeError> {
        let device = only_device(self.discover_intercoms().await?)?;
        let (doorbell_volume, mic_volume, voice_volume) = device.volumes();
        let last_activity = self.latest_activity(&device).await.ok().flatten();
        Ok(RingDeviceStatus {
            battery: device.battery(),
            online: device.online(),
            doorbell_volume,
            mic_volume,
            voice_volume,
            last_activity,
        })
    }

    pub async fn unlock(&self) -> Result<(), BridgeError> {
        let device = only_device(self.discover_intercoms().await?)?;
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
        let device = only_device(self.discover_intercoms().await?)?;
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
        self.vendor_request(method, endpoint, body, query, "volume update")
            .await?;
        tracing::info!(setting, value, "Ring Intercom volume updated");
        Ok(())
    }

    async fn vendor_request(
        &self,
        method: Method,
        endpoint: String,
        json_body: Option<Value>,
        query: Vec<(String, String)>,
        operation: &'static str,
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
            return checked_body(response, operation, CONTROL_BODY_LIMIT).await;
        }
        Err(BridgeError::Protocol("control retry was exhausted".into()))
    }

    async fn latest_activity(
        &self,
        device: &RingIntercomIdentity,
    ) -> Result<Option<i64>, BridgeError> {
        let location = device
            .location_id()
            .filter(|value| valid_provider_id(value))
            .ok_or_else(|| BridgeError::Protocol("Ring location is unavailable".into()))?;
        let endpoint = format!(
            "{}/locations/{location}/devices/{}/events?limit=20",
            self.endpoints.client_api,
            device.id()
        );
        let mut state = self.state.lock().await;
        self.ensure_authenticated(&mut state).await?;
        self.ensure_registered(&mut state).await?;
        let response = self
            .http
            .get(endpoint)
            .bearer_auth(access_value(&state)?)
            .header("hardware_id", state.session.hardware_id().to_string())
            .header(reqwest::header::USER_AGENT, USER_AGENT)
            .send()
            .await
            .map_err(|error| BridgeError::Transport("activity history", error))?;
        drop(state);
        let body = checked_body(response, "activity history", EVENTS_BODY_LIMIT).await?;
        let events = serde_json::from_slice::<EventEnvelope>(&body)?.events;
        events
            .iter()
            .map(|event| {
                time::OffsetDateTime::parse(
                    &event.created_at,
                    &time::format_description::well_known::Rfc3339,
                )
                .map(time::OffsetDateTime::unix_timestamp)
                .map_err(|_| BridgeError::Protocol("Ring activity timestamp is invalid".into()))
            })
            .collect::<Result<Vec<_>, _>>()
            .map(|values| values.into_iter().max())
    }
}

fn valid_provider_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn only_device(
    mut devices: Vec<RingIntercomIdentity>,
) -> Result<RingIntercomIdentity, BridgeError> {
    if devices.len() != 1 {
        return Err(BridgeError::Protocol("expected one Ring Intercom".into()));
    }
    devices
        .pop()
        .ok_or_else(|| BridgeError::Protocol("Ring Intercom is unavailable".into()))
}
