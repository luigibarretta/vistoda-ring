use serde::{Deserialize, Serialize};

use crate::BridgeError;

const MAX_TEXT_BYTES: usize = 128;
const MAX_HISTORY_EVENTS: usize = 50;
const MAX_LOCATIONS: usize = 512;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RingHistoryIdentity {
    pub device_name: String,
    pub location_name: String,
    pub city: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RingHistoryEventType {
    Unlock,
    LiveView,
    Ding,
    Motion,
    Activity,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RingHistoryEvent {
    pub event_id: String,
    pub event_type: RingHistoryEventType,
    pub occurred_at: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RingHistoryPage {
    pub identity: RingHistoryIdentity,
    pub events: Vec<RingHistoryEvent>,
    pub next_cursor: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct LocationEnvelope {
    #[serde(default)]
    user_locations: Vec<RawLocation>,
}

#[derive(Deserialize)]
struct RawLocation {
    location_id: String,
    name: String,
    #[serde(default)]
    address: RawAddress,
}

#[derive(Default, Deserialize)]
struct RawAddress {
    #[serde(default)]
    city: Option<String>,
}

#[derive(Deserialize)]
struct RawActivity {
    created_at: String,
    #[serde(default, deserialize_with = "deserialize_optional_id")]
    id: Option<String>,
    #[serde(default)]
    ding_id_str: Option<String>,
    #[serde(default)]
    ding_id: Option<u64>,
    #[serde(default)]
    kind: Option<String>,
}

pub(crate) fn parse_location(
    body: &[u8],
    location_id: &str,
) -> Result<(String, Option<String>), BridgeError> {
    let locations = serde_json::from_slice::<LocationEnvelope>(body)?.user_locations;
    if locations.len() > MAX_LOCATIONS {
        return Err(BridgeError::Protocol(
            "Ring location inventory is too large".into(),
        ));
    }
    let location = locations
        .into_iter()
        .find(|item| item.location_id == location_id)
        .ok_or_else(|| BridgeError::Protocol("Ring location is unavailable".into()))?;
    validate_text(&location.name, "Ring location name")?;
    let city = location.address.city.filter(|value| !value.is_empty());
    if let Some(city) = city.as_deref() {
        validate_text(city, "Ring location city")?;
    }
    Ok((location.name, city))
}

pub(crate) fn parse_activity(
    body: &[u8],
    limit: u8,
) -> Result<(Vec<RingHistoryEvent>, Option<String>), BridgeError> {
    let raw = serde_json::from_slice::<Vec<RawActivity>>(body)?;
    if raw.len() > MAX_HISTORY_EVENTS || raw.len() > usize::from(limit) {
        return Err(BridgeError::Protocol(
            "Ring activity page is too large".into(),
        ));
    }
    let page_is_full = raw.len() == usize::from(limit);
    let mut events = Vec::with_capacity(raw.len());
    for (index, item) in raw.into_iter().enumerate() {
        let kind = item.kind.unwrap_or_else(|| "activity".into());
        validate_text(&kind, "Ring activity kind")?;
        let occurred_at = time::OffsetDateTime::parse(
            &item.created_at,
            &time::format_description::well_known::Rfc3339,
        )
        .map(time::OffsetDateTime::unix_timestamp)
        .map_err(|_| BridgeError::Protocol("Ring activity timestamp is invalid".into()))?;
        let event_id = item
            .id
            .or(item.ding_id_str)
            .or_else(|| item.ding_id.map(|value| value.to_string()))
            .unwrap_or_else(|| format!("event:{occurred_at}:{index}"));
        validate_text(&event_id, "Ring activity identifier")?;
        events.push(RingHistoryEvent {
            event_id,
            event_type: event_type(&kind),
            occurred_at,
        });
    }
    let next_cursor = page_is_full
        .then(|| events.last().map(|event| event.event_id.clone()))
        .flatten()
        .filter(|cursor| cursor.bytes().all(|byte| byte.is_ascii_digit()));
    Ok((events, next_cursor))
}

pub(crate) fn validate_cursor(value: &str) -> Result<(), BridgeError> {
    if value.is_empty() || value.len() > 32 || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(BridgeError::InvalidRequest(
            "history cursor is invalid".into(),
        ));
    }
    Ok(())
}

fn deserialize_optional_id<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Id {
        String(String),
        Number(u64),
    }
    Option::<Id>::deserialize(deserializer).map(|value| {
        value.map(|id| match id {
            Id::String(value) => value,
            Id::Number(value) => value.to_string(),
        })
    })
}

fn event_type(kind: &str) -> RingHistoryEventType {
    match kind {
        "key_access" | "unlock" | "intercom_unlock" => RingHistoryEventType::Unlock,
        "on_demand" | "live_view" => RingHistoryEventType::LiveView,
        "ding" => RingHistoryEventType::Ding,
        "motion" => RingHistoryEventType::Motion,
        _ => RingHistoryEventType::Activity,
    }
}

fn validate_text(value: &str, label: &str) -> Result<(), BridgeError> {
    if value.is_empty() || value.len() > MAX_TEXT_BYTES || value.chars().any(char::is_control) {
        return Err(BridgeError::Protocol(format!("{label} is invalid")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{RingHistoryEventType, parse_activity, parse_location};

    #[test]
    fn parses_identity_and_bounded_activity_page() {
        let (name, city) = parse_location(
            br#"{"user_locations":[{"location_id":"loc-1","name":"Home","address":{"city":"Casoria"}}]}"#,
            "loc-1",
        )
        .unwrap_or_else(|error| panic!("location: {error}"));
        assert_eq!(name, "Home");
        assert_eq!(city.as_deref(), Some("Casoria"));
        let (events, cursor) = parse_activity(
            br#"[{"id":7330963245622279024,"created_at":"2026-09-10T17:30:00Z","kind":"key_access"},{"id":"7323267080901445808","created_at":"2026-09-10T17:20:00Z","kind":"on_demand"}]"#,
            2,
        )
        .unwrap_or_else(|error| panic!("activity: {error}"));
        assert_eq!(events[0].event_type, RingHistoryEventType::Unlock);
        assert_eq!(events[1].event_type, RingHistoryEventType::LiveView);
        assert_eq!(cursor.as_deref(), Some("7323267080901445808"));
    }
}
