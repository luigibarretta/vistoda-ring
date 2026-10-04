//! Read-only diagnosis of Ring's notification settings API, which rejects
//! this client with HTTP 400 although the official app uses the same Bearer
//! client for it. The same GET is sent with the header sets the official app
//! always adds (Ring Android 3.113.0 interceptors), cumulatively, and each
//! attempt reports its status and a redacted error text.

use reqwest::header::USER_AGENT as USER_AGENT_HEADER;
use serde_json::Value;
use zeroize::Zeroizing;

use super::{ProbeSummary, RingClient, access_value, activity_probe::summarize};
use crate::{error::BridgeError, ring_protocol::USER_AGENT};

const BODY_LIMIT: usize = 64 * 1024;
const ERROR_LIMIT: usize = 120;
const APP_USER_AGENT: &str = "android:com.ringapp:3.113.0(70497080)";

type Headers = Vec<(&'static str, String)>;

fn variants(hardware_id: &str) -> Vec<(&'static str, Headers)> {
    let mut headers: Headers = Vec::new();
    let mut variants = vec![("notification_event_types:plain", headers.clone())];
    headers.push(("Content-Type", "application/json".into()));
    variants.push(("notification_event_types:content_type", headers.clone()));
    headers.push(("X-API-LANG", "en".into()));
    headers.push(("X-Ring-UserAgent-LocaleLanguage", "en".into()));
    variants.push(("notification_event_types:language", headers.clone()));
    headers.extend([
        ("User-Agent", APP_USER_AGENT.into()),
        ("App_Brand", "ring".into()),
        ("X-Ring-UserAgent-AppBrand", "ring".into()),
        ("X-Ring-UserAgent-App", "com.ringapp".into()),
        ("X-Ring-UserAgent-AppVersion", "3.113.0".into()),
        ("X-Ring-UserAgent-Tool", "Retrofit OkHttp".into()),
        ("X-Ring-UserAgent-ToolVersion", "3.0.0 + 4.12.0".into()),
        ("X-Ring-UserAgent-DeviceHardwareId", hardware_id.into()),
    ]);
    variants.push(("notification_event_types:app_identity", headers.clone()));
    headers.extend([
        ("X-Ring-UserAgent-DeviceOS", "Android".into()),
        (
            "X-Ring-UserAgent-DeviceOSVersion",
            "Android SDK: 34 (14)".into(),
        ),
        ("X-Ring-UserAgent-DeviceModel", "Vistoda".into()),
        ("X-Ring-UserAgent-DeviceLanguage", "en-US".into()),
        ("X-Ring-UserAgent-LocaleTimezone", "UTC/GMT +00:00".into()),
    ]);
    variants.push(("notification_event_types:device", headers));
    variants
}

impl RingClient {
    pub(super) async fn notification_settings_probe(
        &self,
        device_id: u64,
    ) -> Result<Vec<ProbeSummary>, BridgeError> {
        let url = format!(
            "{}/notification_settings/v1/doorbots/{device_id}/event_types",
            self.endpoints.api_root
        );
        let mut state = self.state.lock().await;
        self.ensure_authenticated(&mut state).await?;
        self.ensure_registered(&mut state).await?;
        let hardware_id = state.session.hardware_id().to_string();
        let access = Zeroizing::new(access_value(&state)?.to_owned());
        drop(state);
        let mut summaries = Vec::new();
        for (source, headers) in variants(&hardware_id) {
            let agent = headers
                .iter()
                .find(|(name, _)| *name == "User-Agent")
                .map_or(USER_AGENT, |(_, value)| value.as_str());
            let mut request = self
                .http
                .get(&url)
                .bearer_auth(access.as_str())
                .header("hardware_id", &hardware_id)
                .header(USER_AGENT_HEADER, agent);
            for (name, value) in headers.iter().filter(|(name, _)| *name != "User-Agent") {
                request = request.header(*name, value);
            }
            let mut response = request
                .send()
                .await
                .map_err(|error| BridgeError::Transport("notification settings probe", error))?;
            let status = response.status().as_u16();
            let mut body = Zeroizing::new(Vec::new());
            while let Ok(Some(chunk)) = response.chunk().await {
                if body.len() + chunk.len() > BODY_LIMIT {
                    break;
                }
                body.extend_from_slice(&chunk);
            }
            summaries.push(if response_ok(status) {
                summarize(source, &body)
            } else {
                ProbeSummary {
                    source,
                    status: Some(status),
                    error: Some(error_text(&body)),
                    ..ProbeSummary::default()
                }
            });
        }
        Ok(summaries)
    }
}

const fn response_ok(status: u16) -> bool {
    status >= 200 && status < 300
}

/// The vendor's error message with digits masked and only plain characters
/// kept, so identifiers in the text cannot leak.
pub fn error_text(body: &[u8]) -> String {
    let text = serde_json::from_slice::<Value>(body).map_or_else(
        |_| String::from_utf8_lossy(body).into_owned(),
        |value| {
            ["error", "message", "error_description", "errors", "detail"]
                .iter()
                .find_map(|key| value.get(key).map(Value::to_string))
                .unwrap_or_else(|| value.to_string())
        },
    );
    text.chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || " _-.:,'".contains(*ch))
        .map(|ch| if ch.is_ascii_digit() { '#' } else { ch })
        .take(ERROR_LIMIT)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{error_text, variants};

    #[test]
    fn error_text_masks_digits_and_markup() {
        assert_eq!(
            error_text(br#"{"error":"doorbot 12345 <b>missing</b> X-API-LANG"}"#),
            "doorbot ##### bmissingb X-API-LANG"
        );
        assert_eq!(error_text(b"Bad Request 400"), "Bad Request ###");
    }

    #[test]
    fn variants_are_cumulative_and_keep_one_user_agent() {
        let sets = variants("hw");
        assert_eq!(sets.len(), 5);
        assert!(
            sets.windows(2)
                .all(|pair| pair[0].1.len() < pair[1].1.len())
        );
        let last = &sets[4].1;
        assert_eq!(
            last.iter()
                .filter(|(name, _)| *name == "User-Agent")
                .count(),
            1
        );
    }
}
