use super::RingReadOnlyClient;
use crate::{
    error::BridgeError,
    ring_http::checked_body,
    ring_protocol::USER_AGENT,
    ring_recording::{EventEnvelope, RawEvent, RecordingEvidence},
};

const EVENTS_LIMIT: usize = 512 * 1024;

impl RingReadOnlyClient {
    pub async fn inspect_recordings(&self) -> Result<RecordingEvidence, BridgeError> {
        let (device, events) = self.recording_context().await?;
        Ok(RecordingEvidence {
            recording_enabled: device.recording_enabled(),
            recordings_visible: device.recordings_visible(),
            location_available: device.location_id().is_some(),
            recent_events: events.len(),
            ready_recordings: events.iter().filter(|event| ready(event)).count(),
        })
    }

    async fn recording_context(
        &self,
    ) -> Result<(super::RingIntercomIdentity, Vec<RawEvent>), BridgeError> {
        let mut devices = self.discover_intercoms().await?;
        if devices.len() != 1 {
            return Err(BridgeError::Protocol("expected one Ring Intercom".into()));
        }
        let device = devices
            .pop()
            .ok_or_else(|| BridgeError::Protocol("Ring Intercom is unavailable".into()))?;
        let location = device
            .location_id()
            .filter(|value| valid_provider_id(value))
            .ok_or_else(|| BridgeError::Protocol("Ring location identity is unavailable".into()))?;
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
            .bearer_auth(super::access_value(&state)?)
            .header("hardware_id", state.session.hardware_id().to_string())
            .header(reqwest::header::USER_AGENT, USER_AGENT)
            .send()
            .await
            .map_err(|error| BridgeError::Transport("recording history", error))?;
        drop(state);
        let body = checked_body(response, "recording history", EVENTS_LIMIT).await?;
        let events = serde_json::from_slice::<EventEnvelope>(&body)?.events;
        if events.len() > 100 {
            return Err(BridgeError::Protocol(
                "Ring event history is too large".into(),
            ));
        }
        Ok((device, events))
    }
}

fn ready(event: &RawEvent) -> bool {
    event.state.as_deref() == Some("completed")
        && matches!(
            event.recording_status.as_deref(),
            Some("audio_ready" | "ready")
        )
}

fn valid_provider_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}
