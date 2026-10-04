//! Intercom unlocks from the event history the official app reads
//! (`evm/v2/history/devices` with `capabilities=ringtercom`). Unlocks made from
//! the Ring app reach neither the doorbot history nor this client's push.

use reqwest::Method;
use serde::Deserialize;
use serde_json::Value;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use super::RingClient;
use crate::error::BridgeError;

const UNLOCK_EVENT_TYPE: &str = "Door.Unlock";
const PAGE_LIMIT: &str = "20";
const BODY_LIMIT: usize = 2 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnlockRecord {
    pub event_id: String,
    pub occurred_at: i64,
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
    start_time: Option<String>,
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

    /// Newest unlocks of one Intercom, in Ring's order.
    pub async fn recent_unlocks(&self, device_id: &str) -> Result<Vec<UnlockRecord>, BridgeError> {
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
        parse_unlocks(&body)
    }
}

/// Unlock items only; malformed items are skipped, a malformed page is an error.
pub fn parse_unlocks(body: &[u8]) -> Result<Vec<UnlockRecord>, BridgeError> {
    let page = serde_json::from_slice::<Page>(body)
        .map_err(|_| BridgeError::Protocol("Ring unlock history is invalid".into()))?;
    Ok(page
        .items
        .into_iter()
        .filter(|item| item.event_type.as_deref() == Some(UNLOCK_EVENT_TYPE))
        .filter_map(|item| {
            let event_id = match item.event_id? {
                Value::String(id) => id,
                Value::Number(id) => id.to_string(),
                _ => return None,
            };
            let occurred_at = OffsetDateTime::parse(item.start_time.as_deref()?, &Rfc3339)
                .ok()?
                .unix_timestamp();
            (!event_id.is_empty() && event_id.len() <= 128).then_some(UnlockRecord {
                event_id,
                occurred_at,
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::{UnlockRecord, parse_unlocks};

    #[test]
    fn keeps_only_well_formed_unlocks() {
        let body = br#"{"items":[
            {"event_id":"e1","event_type":"Door.Unlock","start_time":"2026-10-04T08:34:09.013Z","origin":"user"},
            {"event_id":7,"event_type":"Door.Unlock","start_time":"2026-10-04T08:30:00Z"},
            {"event_id":"e3","event_type":"ding","start_time":"2026-10-04T08:31:00Z"},
            {"event_id":"e4","event_type":"Door.Unlock","start_time":"yesterday"},
            {"event_type":"Door.Unlock","start_time":"2026-10-04T08:32:00Z"}
        ],"pagination_key":null}"#;
        let unlocks = parse_unlocks(body).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            unlocks,
            vec![
                UnlockRecord {
                    event_id: "e1".into(),
                    occurred_at: 1_791_102_849,
                },
                UnlockRecord {
                    event_id: "7".into(),
                    occurred_at: 1_791_102_600,
                },
            ]
        );
        assert!(parse_unlocks(br#""not a page""#).is_err());
        assert_eq!(parse_unlocks(b"{}").map(|items| items.len()).ok(), Some(0));
    }
}
