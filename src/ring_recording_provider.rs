use reqwest::Url;

use super::RingClient;
use crate::{
    error::BridgeError,
    ring_http::checked_body,
    ring_protocol::USER_AGENT,
    ring_recording::{
        EventEnvelope, ProviderRecording, RawEvent, RecordingEvidence, RecordingUrl,
        parse_created_at, valid_provider_id,
    },
};

const EVENTS_LIMIT: usize = 512 * 1024;
const URL_LIMIT: usize = 32 * 1024;
const MEDIA_LIMIT: usize = 64 * 1024 * 1024;

impl RingClient {
    pub(crate) async fn latest_activity(&self) -> Result<Option<i64>, BridgeError> {
        let (_, events) = self.recording_context().await?;
        events
            .iter()
            .map(|event| parse_created_at(&event.created_at))
            .collect::<Result<Vec<_>, _>>()
            .map(|values| values.into_iter().max())
    }

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

    pub(crate) async fn find_recording_since(
        &self,
        since: i64,
    ) -> Result<Option<ProviderRecording>, BridgeError> {
        let (device, events) = self.recording_context().await?;
        if !device.recording_enabled() || !device.recordings_visible() {
            return Err(BridgeError::RecordingUnavailable);
        }
        events
            .into_iter()
            .filter(ready)
            .map(source)
            .collect::<Result<Vec<_>, _>>()
            .map(|events| {
                events
                    .into_iter()
                    .filter(|event| event.created_at >= since.saturating_sub(15))
                    .max_by_key(|event| event.created_at)
            })
    }

    pub(crate) async fn download_recording(
        &self,
        source: &ProviderRecording,
    ) -> Result<Vec<u8>, BridgeError> {
        if !valid_provider_id(&source.ding_id) {
            return Err(BridgeError::Protocol(
                "Ring recording identity is invalid".into(),
            ));
        }
        let endpoint = format!(
            "{}/dings/{}/recording?disable_redirect=true",
            self.endpoints.client_api, source.ding_id
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
            .map_err(|error| BridgeError::Transport("recording URL", error))?;
        drop(state);
        let body = checked_body(response, "recording URL", URL_LIMIT).await?;
        let target = serde_json::from_slice::<RecordingUrl>(&body)?.url;
        let response = self
            .http
            .get(safe_media_url(&target)?)
            .send()
            .await
            .map_err(|error| BridgeError::Transport("recording download", error))?;
        Ok(checked_body(response, "recording download", MEDIA_LIMIT)
            .await?
            .to_vec())
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

fn source(event: RawEvent) -> Result<ProviderRecording, BridgeError> {
    if !valid_provider_id(&event.ding_id_str) {
        return Err(BridgeError::Protocol(
            "Ring event identity is invalid".into(),
        ));
    }
    Ok(ProviderRecording {
        ding_id: event.ding_id_str,
        created_at: parse_created_at(&event.created_at)?,
    })
}

fn safe_media_url(value: &str) -> Result<Url, BridgeError> {
    let url = Url::parse(value)
        .map_err(|_| BridgeError::Protocol("Ring recording URL is invalid".into()))?;
    let trusted = url.host_str().is_some_and(|host| {
        ["amazonaws.com", "cloudfront.net", "ring.com"]
            .iter()
            .any(|suffix| host == *suffix || host.ends_with(&format!(".{suffix}")))
    });
    if url.scheme() != "https"
        || !trusted
        || url.username() != ""
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(BridgeError::Protocol("Ring recording URL is unsafe".into()));
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::safe_media_url;

    #[test]
    fn media_url_is_https_and_vendor_scoped() {
        assert!(safe_media_url("https://clips.s3.amazonaws.com/call.mp4?signature=x").is_ok());
        assert!(safe_media_url("https://example.com/call.mp4").is_err());
        assert!(safe_media_url("http://clips.s3.amazonaws.com/call.mp4").is_err());
        assert!(safe_media_url("https://user@ring.com/call.mp4").is_err());
    }
}
