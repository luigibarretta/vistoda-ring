//! Read-only view of the Intercom unlock settings that the official Ring app
//! manages under "Unlock Settings".
//!
//! The values come from `settings.intercom_settings` of the Intercom entry in
//! the device discovery response the bridge already fetches, so no extra
//! vendor request is needed. The Ring app models that object with the fields
//! `ring_to_open` (boolean, the "Unlock Type" choice between Direct Unlock and
//! Ring-to-Open) and `config` (a JSON document serialized as a string whose
//! `analog.unlock_duration` holds the unlock duration in milliseconds; only
//! analog building systems expose it). Anything missing, of the wrong type or
//! out of range is reported as unknown instead of failing discovery.

use serde::Serialize;
use serde_json::Value;

/// Largest serialized `config` document that is inspected.
const MAX_CONFIG_BYTES: usize = 16 * 1024;
/// Largest unlock duration accepted from Ring (ten minutes, in milliseconds).
const MAX_UNLOCK_DURATION_MS: u64 = 600_000;

/// How a remote unlock from the Ring account reaches the door.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RingUnlockMode {
    /// The Intercom releases the door at any time on an unlock command.
    Direct,
    /// The building system only opens during an active call: someone must
    /// press the call button at the entrance panel.
    RingToOpen,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct RingUnlockSettings {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<RingUnlockMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ring_to_open_enabled: Option<bool>,
    /// Whole seconds the door stays unlocked; 0 means less than one second.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_seconds: Option<u16>,
}

/// Parses `settings.intercom_settings`; returns `None` when nothing is known.
pub(crate) fn parse_unlock_settings(value: Option<&Value>) -> Option<RingUnlockSettings> {
    let settings = value?.as_object()?;
    let ring_to_open = settings.get("ring_to_open").and_then(Value::as_bool);
    let parsed = RingUnlockSettings {
        mode: ring_to_open.map(|enabled| {
            if enabled {
                RingUnlockMode::RingToOpen
            } else {
                RingUnlockMode::Direct
            }
        }),
        ring_to_open_enabled: ring_to_open,
        duration_seconds: settings.get("config").and_then(duration_seconds),
    };
    (parsed != RingUnlockSettings::default()).then_some(parsed)
}

fn duration_seconds(config: &Value) -> Option<u16> {
    let document;
    let config = match config {
        Value::String(text) if text.len() <= MAX_CONFIG_BYTES => {
            document = serde_json::from_str::<Value>(text).ok()?;
            &document
        }
        Value::Object(_) => config,
        _ => return None,
    };
    let millis = config.get("analog")?.get("unlock_duration")?.as_u64()?;
    if millis > MAX_UNLOCK_DURATION_MS {
        return None;
    }
    u16::try_from(millis / 1000).ok()
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::{RingUnlockMode, RingUnlockSettings, parse_unlock_settings};

    // Synthetic shape of the Ring app `IntercomSettings` DTO (3.113.0):
    // predecessor/config are strings, ring_to_open is a boolean, unlock_mode
    // an integer and the analog unlock duration is stored in milliseconds.
    fn intercom_settings(ring_to_open: Value, config: Value) -> Value {
        let mut settings = json!({
            "predecessor": "{\"make\":\"Synthetic\",\"model\":\"Synthetic\"}",
            "intercom_type": "SYNTHETIC",
            "unlock_mode": 0,
            "replication": 1
        });
        settings["config"] = config;
        settings["ring_to_open"] = ring_to_open;
        settings
    }

    #[test]
    fn analog_ring_to_open_with_duration_is_reported() {
        let value = intercom_settings(
            json!(true),
            json!("{\"analog\":{\"unlock_duration\":3000},\"video\":{\"en\":false}}"),
        );
        assert_eq!(
            parse_unlock_settings(Some(&value)),
            Some(RingUnlockSettings {
                mode: Some(RingUnlockMode::RingToOpen),
                ring_to_open_enabled: Some(true),
                duration_seconds: Some(3),
            })
        );
    }

    #[test]
    fn digital_direct_unlock_has_no_duration() {
        let value = intercom_settings(json!(false), json!("{\"digital\":{}}"));
        let parsed = parse_unlock_settings(Some(&value));
        assert_eq!(
            parsed,
            Some(RingUnlockSettings {
                mode: Some(RingUnlockMode::Direct),
                ring_to_open_enabled: Some(false),
                duration_seconds: None,
            })
        );
        let rendered = serde_json::to_value(parsed).unwrap_or_default();
        assert_eq!(
            rendered,
            json!({"mode": "direct", "ring_to_open_enabled": false})
        );
    }

    #[test]
    fn sub_second_and_object_config_are_bounded() {
        let short = intercom_settings(Value::Null, json!({"analog": {"unlock_duration": 500}}));
        assert_eq!(
            parse_unlock_settings(Some(&short)),
            Some(RingUnlockSettings {
                duration_seconds: Some(0),
                ..RingUnlockSettings::default()
            })
        );
        let long = intercom_settings(json!(true), json!({"analog": {"unlock_duration": 600_001}}));
        let parsed = parse_unlock_settings(Some(&long)).unwrap_or_default();
        assert_eq!(parsed.duration_seconds, None);
        assert_eq!(parsed.mode, Some(RingUnlockMode::RingToOpen));
    }

    #[test]
    fn malformed_or_missing_values_are_unknown() {
        for value in [
            Value::Null,
            json!([]),
            json!("ring_to_open"),
            json!({}),
            intercom_settings(json!("true"), json!("not json")),
            intercom_settings(json!(1), json!({"analog": {"unlock_duration": -5}})),
            intercom_settings(json!(null), json!({"analog": {"unlock_duration": 2.5}})),
            intercom_settings(json!(null), json!({"analog": {"unlock_duration": "3000"}})),
            intercom_settings(json!(null), json!(42)),
        ] {
            assert_eq!(parse_unlock_settings(Some(&value)), None, "{value}");
        }
        assert_eq!(parse_unlock_settings(None), None);
        let oversized = format!(
            "{{\"analog\":{{\"unlock_duration\":3000}},\"pad\":\"{}\"}}",
            "x".repeat(16 * 1024)
        );
        let value = intercom_settings(Value::Null, Value::String(oversized));
        assert_eq!(parse_unlock_settings(Some(&value)), None);
    }
}
