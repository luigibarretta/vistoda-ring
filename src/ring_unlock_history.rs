//! Intercom unlocks and dings from the event history the official app reads
//! (`evm/v2/history/devices` with `capabilities=ringtercom`). Unlocks made from
//! the Ring app reach neither the doorbot history nor this client's push;
//! dings are read only to detect push dings that never arrived.

use reqwest::Method;
use serde::Deserialize;
use serde_json::Value;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use super::RingClient;
use crate::{
    error::BridgeError,
    ring_ding_watchdog::{HistoryDing, is_ding_event},
    ring_push_event::UnlockDetail,
};

const UNLOCK_EVENT_TYPE: &str = "Door.Unlock";
/// Ring's unlock origins (`UnlockOrigin` in the official app).
pub(super) const ORIGINS: &[&str] = &["user", "delivery", "device", "code"];
const ACTOR_LIMIT: usize = 64;
const PAGE_LIMIT: &str = "20";
const BODY_LIMIT: usize = 2 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnlockRecord {
    pub event_id: String,
    pub occurred_at: i64,
    pub detail: UnlockDetail,
}

/// One history page split into the item kinds the bridge acts on.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HistoryActivity {
    pub unlocks: Vec<UnlockRecord>,
    pub dings: Vec<HistoryDing>,
}

#[derive(Deserialize)]
struct Page {
    #[serde(default)]
    items: Vec<Item>,
}

#[derive(Deserialize)]
struct Item {
    event_id: Option<Value>,
    event_type: Option<String>,
    kind: Option<String>,
    start_time: Option<String>,
    origin: Option<String>,
    origin_display_name: Option<String>,
}

impl RingClient {
    /// The selected Intercom's Ring ID, discovering it for an unbound alias.
    pub async fn selected_intercom_id(&self) -> Result<String, BridgeError> {
        if let Some(id) = self.selected_device_id {
            return Ok(id.to_string());
        }
        Ok(
            super::controls::only_device(self.discover_intercoms().await?)?
                .id()
                .to_string(),
        )
    }

    /// Newest unlocks and dings of one Intercom, in Ring's order.
    pub async fn recent_activity(&self, device_id: &str) -> Result<HistoryActivity, BridgeError> {
        let body = self
            .vendor_request(
                Method::GET,
                format!("{}/evm/v2/history/devices", self.endpoints.api_root),
                None,
                vec![
                    ("source_ids".to_owned(), device_id.to_owned()),
                    (
                        "capabilities".to_owned(),
                        "offline_event,ringtercom".to_owned(),
                    ),
                    ("limit".to_owned(), PAGE_LIMIT.to_owned()),
                ],
                "unlock history",
                BODY_LIMIT,
            )
            .await?;
        parse_activity(&body)
    }
}

/// Unlock and ding items; malformed items are skipped, a malformed page is an error.
pub fn parse_activity(body: &[u8]) -> Result<HistoryActivity, BridgeError> {
    let page = serde_json::from_slice::<Page>(body)
        .map_err(|_| BridgeError::Protocol("Ring unlock history is invalid".into()))?;
    let mut activity = HistoryActivity::default();
    for item in page.items {
        let unlock = item.event_type.as_deref() == Some(UNLOCK_EVENT_TYPE);
        let ding = !unlock && is_ding_event(item.event_type.as_deref(), item.kind.as_deref());
        if !unlock && !ding {
            continue;
        }
        let Some((event_id, occurred_at)) =
            identity(item.event_id.as_ref(), item.start_time.as_deref())
        else {
            continue;
        };
        if ding {
            activity.dings.push(HistoryDing {
                event_id,
                occurred_at,
            });
            continue;
        }
        let detail = UnlockDetail {
            origin: item
                .origin
                .filter(|origin| ORIGINS.contains(&origin.as_str())),
            actor: item.origin_display_name.as_deref().and_then(display_name),
        };
        activity.unlocks.push(UnlockRecord {
            event_id,
            occurred_at,
            detail,
        });
    }
    Ok(activity)
}

/// A bounded event ID and the parsed start time, or nothing.
fn identity(event_id: Option<&Value>, start_time: Option<&str>) -> Option<(String, i64)> {
    let event_id = match event_id? {
        Value::String(id) => id.clone(),
        Value::Number(id) => id.to_string(),
        _ => return None,
    };
    let occurred_at = OffsetDateTime::parse(start_time?, &Rfc3339)
        .ok()?
        .unix_timestamp();
    (!event_id.is_empty() && event_id.len() <= 128).then_some((event_id, occurred_at))
}

/// A short printable name, or nothing.
fn display_name(name: &str) -> Option<String> {
    let name = name.trim();
    (!name.is_empty() && name.chars().count() <= ACTOR_LIMIT && !name.chars().any(char::is_control))
        .then(|| name.to_owned())
}

#[cfg(test)]
mod tests {
    use super::{UnlockRecord, parse_activity};
    use crate::{ring_ding_watchdog::HistoryDing, ring_push_event::UnlockDetail};

    #[test]
    fn keeps_only_well_formed_unlocks_and_dings() {
        let body = br#"{"items":[
            {"event_id":"e1","event_type":"Door.Unlock","start_time":"2026-10-04T08:34:09.013Z","origin":"user","origin_display_name":" Luigi "},
            {"event_id":7,"event_type":"Door.Unlock","start_time":"2026-10-04T08:30:00Z","origin":"Mario","origin_display_name":"a\nb"},
            {"event_id":"e3","event_type":"ding","start_time":"2026-10-04T08:31:00Z"},
            {"event_id":"e4","event_type":"Door.Unlock","start_time":"yesterday"},
            {"event_type":"Door.Unlock","start_time":"2026-10-04T08:32:00Z"},
            {"event_id":"e5","event_type":"Intercom.Ding","start_time":"2026-10-04T08:31:00Z"},
            {"event_id":"e6","kind":"ding","start_time":"2026-10-04T08:33:00Z"},
            {"event_id":"e7","event_type":"Intercom.Ding"},
            {"event_id":"e8","event_type":"Motion","start_time":"2026-10-04T08:33:00Z"}
        ],"pagination_key":null}"#;
        let activity = parse_activity(body).unwrap_or_else(|error| panic!("{error}"));
        let ding = |id: &str, at| HistoryDing {
            event_id: id.into(),
            occurred_at: at,
        };
        assert_eq!(
            activity.dings,
            vec![
                ding("e3", 1_791_102_660),
                ding("e5", 1_791_102_660),
                ding("e6", 1_791_102_780),
            ]
        );
        assert_eq!(
            activity.unlocks,
            vec![
                UnlockRecord {
                    event_id: "e1".into(),
                    occurred_at: 1_791_102_849,
                    detail: UnlockDetail {
                        origin: Some("user".into()),
                        actor: Some("Luigi".into()),
                    },
                },
                UnlockRecord {
                    event_id: "7".into(),
                    occurred_at: 1_791_102_600,
                    detail: UnlockDetail::default(),
                },
            ]
        );
        assert!(parse_activity(br#""not a page""#).is_err());
        assert_eq!(
            parse_activity(b"{}").ok(),
            Some(super::HistoryActivity::default())
        );
    }
}
