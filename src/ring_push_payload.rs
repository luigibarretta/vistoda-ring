use serde_json::{Map, Value};

use crate::ring_push_event::RingPushEventKind;

const INTERCOM_DING: &str = "com.ring.pn.live-event.intercom";
const INTERCOM_UNLOCK: &str = "com.ring.push.INTERCOM_UNLOCK_FROM_APP";
const PAYLOAD_LIMIT: usize = 128 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub struct ParsedPushEvent {
    pub device_id: String,
    pub event_type: RingPushEventKind,
    pub occurred_at: Option<i64>,
}

#[must_use]
pub fn parse_push_event(input: &[u8]) -> Option<ParsedPushEvent> {
    if input.is_empty() || input.len() > PAYLOAD_LIMIT {
        return None;
    }
    let envelope = serde_json::from_slice::<Value>(input).ok()?;
    let fields = envelope.get("data")?.as_object()?;
    let message = normalize_fields(fields);
    parse_v2(&message).or_else(|| parse_legacy(&message))
}

fn normalize_fields(fields: &Map<String, Value>) -> Map<String, Value> {
    fields
        .iter()
        .map(|(key, value)| {
            let parsed = value
                .as_str()
                .and_then(|raw| serde_json::from_str(raw).ok())
                .unwrap_or_else(|| value.clone());
            (key.clone(), parsed)
        })
        .collect()
}

fn parse_v2(message: &Map<String, Value>) -> Option<ParsedPushEvent> {
    let category = message.get("android_config")?.get("category")?.as_str()?;
    if category != INTERCOM_DING {
        return None;
    }
    let data = message.get("data")?;
    Some(ParsedPushEvent {
        device_id: provider_id(data.get("device")?.get("id")?)?,
        event_type: RingPushEventKind::Ding,
        occurred_at: message
            .get("analytics")
            .and_then(|value| value.get("triggered_at"))
            .and_then(Value::as_i64)
            .map(milliseconds_to_seconds),
    })
}

fn parse_legacy(message: &Map<String, Value>) -> Option<ParsedPushEvent> {
    let gcm = message.get("data")?.get("gcmData")?;
    if gcm.get("action")?.as_str()? != INTERCOM_UNLOCK {
        return None;
    }
    Some(ParsedPushEvent {
        device_id: provider_id(gcm.get("alarm_meta")?.get("device_zid")?)?,
        event_type: RingPushEventKind::IntercomUnlock,
        occurred_at: None,
    })
}

fn provider_id(value: &Value) -> Option<String> {
    let value = value
        .as_str()
        .map(ToOwned::to_owned)
        .or_else(|| value.as_u64().map(|number| number.to_string()))?;
    if value.is_empty() || value.len() > 128 || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some(value)
}

const fn milliseconds_to_seconds(value: i64) -> i64 {
    if value > 10_000_000_000 {
        value / 1_000
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::parse_push_event;
    use crate::ring_push_event::RingPushEventKind;

    #[test]
    fn parses_string_encoded_v2_intercom_ding() {
        let payload = br#"{"data":{"android_config":"{\"category\":\"com.ring.pn.live-event.intercom\"}","analytics":"{\"triggered_at\":1787600000123}","data":"{\"device\":{\"id\":12345}}"}}"#;
        let event = parse_push_event(payload).unwrap_or_else(|| panic!("ding was not parsed"));
        assert_eq!(event.event_type, RingPushEventKind::Ding);
        assert_eq!(event.device_id, "12345");
        assert_eq!(event.occurred_at, Some(1_787_600_000));
    }

    #[test]
    fn parses_legacy_unlock_and_rejects_other_devices() {
        let payload = br#"{"data":{"data":"{\"gcmData\":{\"action\":\"com.ring.push.INTERCOM_UNLOCK_FROM_APP\",\"alarm_meta\":{\"device_zid\":\"987\"}}}"}}"#;
        let event = parse_push_event(payload).unwrap_or_else(|| panic!("unlock was not parsed"));
        assert_eq!(event.event_type, RingPushEventKind::IntercomUnlock);
        assert_eq!(event.device_id, "987");
        assert!(parse_push_event(br#"{"data":{"data":"{}"}}"#).is_none());
    }
}
