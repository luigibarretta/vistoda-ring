//! Minimal enrollment inventory; raw account metadata never enters the wire model.
use crate::{BridgeError, ring_wire::RingIntercomIdentity};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// Supported public inventory matches the app's maximum configured intercom count.
// Raw vendor discovery still has its independent 512-device parser safety bound.
pub const MAX_INTERCOMS: usize = 32;
const MAX_LOCATIONS: usize = 512;

#[derive(Debug, Serialize)]
pub struct IntercomInventory {
    pub intercoms: Vec<IntercomSummary>,
}

#[derive(Debug, Serialize)]
pub struct IntercomSummary {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    pub device_id: String,
    pub name: String,
    pub location_name: Option<String>,
}

#[derive(Deserialize)]
struct Locations {
    user_locations: Vec<Location>,
}

#[derive(Deserialize)]
struct Location {
    location_id: String,
    name: String,
}

pub fn inventory(
    mut devices: Vec<RingIntercomIdentity>,
    locations: Option<&[u8]>,
) -> Result<IntercomInventory, BridgeError> {
    if devices.len() > MAX_INTERCOMS {
        return Err(BridgeError::Protocol(
            "intercom inventory is too large".into(),
        ));
    }
    let names = locations.and_then(location_names).unwrap_or_default();
    devices.sort_by_key(RingIntercomIdentity::id);
    Ok(IntercomInventory {
        intercoms: devices
            .into_iter()
            .map(|device| IntercomSummary {
                alias: None,
                device_id: device.id().to_string(),
                name: device.description().to_owned(),
                location_name: device.location_id().and_then(|id| names.get(id)).cloned(),
            })
            .collect(),
    })
}

pub fn location_names(body: &[u8]) -> Option<BTreeMap<String, String>> {
    let locations: Locations = serde_json::from_slice(body).ok()?;
    if locations.user_locations.len() > MAX_LOCATIONS {
        return None;
    }
    let mut result = BTreeMap::new();
    for location in locations.user_locations {
        if !safe_text(&location.location_id)
            || !safe_text(&location.name)
            || result.insert(location.location_id, location.name).is_some()
        {
            return None;
        }
    }
    Some(result)
}

fn safe_text(value: &str) -> bool {
    !value.is_empty() && value.len() <= 128 && !value.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use super::{inventory, location_names};
    use crate::ring_wire::parse_devices;

    #[test]
    fn supported_public_inventory_stops_at_32_without_partial_results() {
        for count in [32, 33] {
            let devices: Vec<_> = (1..=count)
                .map(|id| {
                    serde_json::json!({
                        "id":id, "kind":"intercom_handset_audio", "description":"Intercom"
                    })
                })
                .collect();
            let body =
                serde_json::to_vec(&serde_json::json!({"other":devices})).unwrap_or_default();
            let parsed = parse_devices(&body).unwrap_or_else(|error| panic!("{error}"));
            assert_eq!(inventory(parsed, None).is_ok(), count == 32);
        }
    }

    #[test]
    fn oversized_discovery_never_reaches_the_inventory_response() {
        let devices: Vec<_> = (1..=513)
            .map(|id| {
                serde_json::json!({"id":id,
            "kind":"intercom_handset_audio", "description":"Front"})
            })
            .collect();
        let body = serde_json::to_vec(&serde_json::json!({"other":devices})).unwrap_or_default();
        assert!(parse_devices(&body).is_err());
    }

    #[test]
    fn response_allowlists_identity_and_ignores_private_location_fields() {
        let devices = parse_devices(br#"{"other":[{"id":42,"kind":"intercom_handset_audio","description":"Front","location_id":"loc-1"}]}"#)
            .unwrap_or_else(|error| panic!("{error}"));
        let locations = br#"{"user_locations":[{"location_id":"loc-1","name":"Home","address":{"street":"private-address"},"token":"private-token"}]}"#;
        let result = inventory(devices, Some(locations)).unwrap_or_else(|error| panic!("{error}"));
        let value = serde_json::to_value(result).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            value,
            serde_json::json!({"intercoms":[{"device_id":"42","name":"Front","location_name":"Home"}]})
        );
    }

    #[test]
    fn malformed_ambiguous_or_oversized_location_metadata_is_ignored() {
        assert!(
            location_names(br#"{"user_locations":[{"location_id":"x","name":"bad\nname"}]}"#)
                .is_none()
        );
        assert!(location_names(br#"{"user_locations":[{"location_id":"x","name":"A"},{"location_id":"x","name":"B"}]}"#).is_none());
        let value = serde_json::json!({"user_locations":vec![serde_json::json!({"location_id":"x","name":"A"}); 513]});
        assert!(location_names(&serde_json::to_vec(&value).unwrap_or_default()).is_none());
    }
}
