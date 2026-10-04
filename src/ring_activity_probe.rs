//! Read-only, redacted probe of the Ring feeds that record Intercom unlocks
//! (the doorbot history does not): the per-device notification event types
//! and the event history the official app reads with `capabilities=ringtercom`.

use std::collections::BTreeMap;

use reqwest::Method;
use serde::Serialize;
use serde_json::Value;

use super::{RingClient, unlock_history::ORIGINS};
use crate::error::BridgeError;

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
    "origin",
];
/// Only this feed is a bare list of enum strings worth reporting verbatim.
const ENUM_LIST_SOURCE: &str = "notification_event_types";
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
    /// Redacted vendor error text of a rejected request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl RingClient {
    pub async fn activity_probe(&self) -> Result<Vec<ProbeSummary>, BridgeError> {
        let device = super::controls::only_device(self.discover_intercoms().await?)?;
        let location = device
            .location_id()
            .ok_or_else(|| BridgeError::Protocol("Ring Intercom has no location".into()))?
            .to_owned();
        let root = &self.endpoints.api_root;
        let id = device.id();
        let feeds = [
            (
                "locations_events",
                format!("{}/locations/{location}/events", self.endpoints.client_api),
                vec![("limit".to_owned(), "20".to_owned())],
            ),
            (
                "evm_device_history",
                format!("{root}/evm/v2/history/devices"),
                vec![
                    ("source_ids".to_owned(), id.to_string()),
                    (
                        "capabilities".to_owned(),
                        "offline_event,ringtercom".to_owned(),
                    ),
                    ("limit".to_owned(), "20".to_owned()),
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
                Ok(body) => classified(summarize(source, &body), &body),
                Err(BridgeError::VendorRejected { status, .. }) => ProbeSummary {
                    source,
                    status: Some(status),
                    ..ProbeSummary::default()
                },
                Err(error) => return Err(error),
            });
        }
        summaries.extend(self.notification_settings_probe(id).await?);
        Ok(summaries)
    }
}

/// Adds what the bridge's own history classifier recognizes, so a live probe
/// confirms which `event_type` Ring uses for Intercom dings.
fn classified(mut summary: ProbeSummary, body: &[u8]) -> ProbeSummary {
    if summary.source == "evm_device_history"
        && let Ok(activity) = super::unlock_history::parse_activity(body)
    {
        let count = |items: usize| u32::try_from(items).unwrap_or(u32::MAX);
        let kinds = &mut summary.kinds;
        kinds.insert("classified=ding".into(), count(activity.dings.len()));
        kinds.insert("classified=unlock".into(), count(activity.unlocks.len()));
    }
    summary
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
    walk(
        &value,
        &mut summary,
        0,
        source.starts_with(ENUM_LIST_SOURCE),
    );
    summary
}

fn walk(value: &Value, summary: &mut ProbeSummary, depth: usize, enum_list: bool) {
    if depth > 8 {
        return;
    }
    match value {
        Value::Object(map) => {
            for (key, item) in map {
                if KIND_KEYS.contains(&key.as_str())
                    && let Some(text) = item.as_str().filter(|text| safe_label(text))
                    && (key != "origin" || ORIGINS.contains(&text))
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
                walk(item, summary, depth + 1, false);
            }
        }
        Value::Array(items) => {
            for item in items.iter().take(200) {
                if enum_list
                    && let Some(text) = item.as_str().filter(|text| safe_label(text))
                    && summary.kinds.len() < 64
                {
                    *summary.kinds.entry(format!("item={text}")).or_insert(0) += 1;
                }
                walk(item, summary, depth + 1, enum_list);
            }
        }
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
    use super::{classified, summarize};

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
        let types = summarize("notification_event_types", br#"["ding","unlock"]"#);
        assert_eq!(types.kinds.get("item=unlock"), Some(&1));
        assert_eq!(types.top_level_keys, vec!["array[2]"]);
        let history = summarize(
            "evm_device_history",
            br#"{"items":[{"event_type":"Door.Unlock","origin":"user","tags":["Mario"]},{"origin":"Mario"}]}"#,
        );
        assert_eq!(history.kinds.get("event_type=Door.Unlock"), Some(&1));
        let body = br#"{"items":[{"event_id":"e","event_type":"Intercom.Ding","start_time":"2026-10-04T08:30:00Z"}]}"#;
        let counted = classified(summarize("evm_device_history", body), body);
        assert_eq!(counted.kinds.get("classified=ding"), Some(&1));
        assert_eq!(counted.kinds.get("classified=unlock"), Some(&0));
        assert_eq!(history.kinds.get("origin=user"), Some(&1));
        assert!(
            !serde_json::to_string(&history)
                .unwrap_or_default()
                .contains("Mario")
        );
        assert_eq!(
            summarize("x", b"<html>").top_level_keys,
            vec!["non-json (6 bytes)"]
        );
    }
}
