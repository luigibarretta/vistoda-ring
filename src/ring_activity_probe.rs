//! Read-only, redacted probe of Ring activity feeds that might record app
//! unlocks (which the doorbot history and FCM push do not deliver).

use std::collections::BTreeMap;

use reqwest::Method;
use serde::Serialize;
use serde_json::Value;

use super::RingClient;
use crate::error::BridgeError;

/// Host used by the official app for location event and monitoring feeds.
const APP_API_ROOT: &str = "https://prd-api-us.prd.rings.solutions";
const PROBE_BODY_LIMIT: usize = 2 * 1024 * 1024;
const KIND_KEYS: &[&str] = &[
    "kind",
    "event_type",
    "type",
    "action",
    "category",
    "subtype",
    "event_kind",
    "eventType",
];
const TIME_KEYS: &[&str] = &[
    "created_at",
    "timestamp",
    "time",
    "occurred_at",
    "start_time",
];

/// What one feed contains, without IDs, names or free text.
#[derive(Debug, Default, PartialEq, Eq, Serialize)]
pub struct ProbeSummary {
    pub source: &'static str,
    pub status: Option<u16>,
    pub top_level_keys: Vec<String>,
    pub kinds: BTreeMap<String, u32>,
    pub newest: Option<String>,
}

impl RingClient {
    pub async fn activity_probe(&self) -> Result<Vec<ProbeSummary>, BridgeError> {
        let device = super::controls::only_device(self.discover_intercoms().await?)?;
        let location = device
            .location_id()
            .ok_or_else(|| BridgeError::Protocol("Ring Intercom has no location".into()))?
            .to_owned();
        let feeds = [
            (
                "locations_events",
                format!("{}/locations/{location}/events", self.endpoints.client_api),
                vec![("limit".to_owned(), "20".to_owned())],
            ),
            (
                "evm_location_history",
                format!("{APP_API_ROOT}/evm/v2/history/locations/{location}?ringtercom"),
                Vec::new(),
            ),
            (
                "rs_history",
                format!("{APP_API_ROOT}/api/v1/rs/history"),
                vec![
                    ("accountId".to_owned(), location.clone()),
                    ("limit".to_owned(), "20".to_owned()),
                    ("maxLevel".to_owned(), "50".to_owned()),
                ],
            ),
        ];
        let mut summaries = Vec::new();
        for (source, url, query) in feeds {
            let result = self
                .vendor_request(
                    Method::GET,
                    url,
                    None,
                    query,
                    "activity probe",
                    PROBE_BODY_LIMIT,
                )
                .await;
            summaries.push(match result {
                Ok(body) => summarize(source, &body),
                Err(BridgeError::VendorRejected { status, .. }) => ProbeSummary {
                    source,
                    status: Some(status),
                    ..ProbeSummary::default()
                },
                Err(error) => return Err(error),
            });
        }
        Ok(summaries)
    }
}

pub fn summarize(source: &'static str, body: &[u8]) -> ProbeSummary {
    let mut summary = ProbeSummary {
        source,
        status: Some(200),
        ..ProbeSummary::default()
    };
    let Ok(value) = serde_json::from_slice::<Value>(body) else {
        summary.top_level_keys = vec![format!("non-json ({} bytes)", body.len())];
        return summary;
    };
    summary.top_level_keys = match &value {
        Value::Object(map) => map.keys().take(20).cloned().collect(),
        Value::Array(items) => vec![format!("array[{}]", items.len())],
        _ => vec!["scalar".to_owned()],
    };
    walk(&value, &mut summary, 0);
    summary
}

fn walk(value: &Value, summary: &mut ProbeSummary, depth: usize) {
    if depth > 8 {
        return;
    }
    match value {
        Value::Object(map) => {
            for (key, item) in map {
                if KIND_KEYS.contains(&key.as_str())
                    && let Some(text) = item.as_str().filter(|text| safe_label(text))
                    && summary.kinds.len() < 64
                {
                    *summary.kinds.entry(format!("{key}={text}")).or_insert(0) += 1;
                }
                if TIME_KEYS.contains(&key.as_str()) {
                    let stamp = item
                        .as_str()
                        .map(ToOwned::to_owned)
                        .or_else(|| item.as_i64().map(|number| number.to_string()));
                    if let Some(stamp) = stamp.filter(|stamp| stamp.len() <= 40)
                        && summary.newest.as_ref().is_none_or(|newest| &stamp > newest)
                    {
                        summary.newest = Some(stamp);
                    }
                }
                walk(item, summary, depth + 1);
            }
        }
        Value::Array(items) => items
            .iter()
            .take(200)
            .for_each(|item| walk(item, summary, depth + 1)),
        _ => {}
    }
}

/// Enum-like vendor labels only: short identifiers without spaces or digits runs.
fn safe_label(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 64
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphabetic() || matches!(byte, b'_' | b'-' | b'.'))
}

#[cfg(test)]
mod tests {
    use super::summarize;

    #[test]
    fn summarizes_kinds_and_newest_time_without_identifiers() {
        let body = br#"{"events":[{"kind":"ding","created_at":"2026-10-04T08:30:00Z","doorbot_id":42,"name":"Mario"},{"kind":"intercom_unlock","created_at":"2026-10-04T08:33:00Z","doorbot":{"description":"Portone"}}],"meta":{"pagination_key":"123"}}"#;
        let summary = summarize("locations_events", body);
        assert_eq!(summary.kinds.get("kind=intercom_unlock"), Some(&1));
        assert_eq!(summary.kinds.get("kind=ding"), Some(&1));
        assert_eq!(summary.newest.as_deref(), Some("2026-10-04T08:33:00Z"));
        assert_eq!(summary.top_level_keys, vec!["events", "meta"]);
        let rendered = serde_json::to_string(&summary).unwrap_or_default();
        assert!(
            !rendered.contains("Mario")
                && !rendered.contains("Portone")
                && !rendered.contains("42")
        );
        assert_eq!(
            summarize("x", b"<html>").top_level_keys,
            vec!["non-json (6 bytes)"]
        );
    }
}
