use serde_json::Value;
use zeroize::Zeroizing;

use crate::BridgeError;

pub enum Incoming {
    Answer(String),
    Ice { candidate: String, line: u16 },
    SessionCreated,
    CameraConnected,
    Close { code: i64 },
    Other,
}

pub struct DecodedSignal {
    pub incoming: Incoming,
    pub session_id: Option<Zeroizing<String>>,
}

pub fn decode_signal(
    bytes: &[u8],
    device_id: u64,
    limit: usize,
) -> Result<DecodedSignal, BridgeError> {
    if bytes.len() > limit {
        return Err(protocol("inbound signal exceeds limit"));
    }
    let value: Value = serde_json::from_slice(bytes)?;
    if value
        .pointer("/body/doorbot_id")
        .and_then(Value::as_u64)
        .is_some_and(|id| id != device_id)
    {
        return Ok(decoded(Incoming::Other));
    }
    let method = value.get("method").and_then(Value::as_str).unwrap_or("");
    match method {
        "session_created" | "session_started" => session_created(&value),
        "sdp" => Ok(decoded(Incoming::Answer(required(&value, "/body/sdp")?))),
        "ice" => Ok(decoded(Incoming::Ice {
            candidate: required(&value, "/body/ice")?,
            line: value
                .pointer("/body/mlineindex")
                .and_then(Value::as_u64)
                .and_then(|line| u16::try_from(line).ok())
                .ok_or_else(|| protocol("invalid ICE line"))?,
        })),
        "notification"
            if value.pointer("/body/text").and_then(Value::as_str) == Some("camera_connected") =>
        {
            Ok(decoded(Incoming::CameraConnected))
        }
        "close" => Ok(decoded(Incoming::Close {
            code: value
                .pointer("/body/reason/code")
                .and_then(Value::as_i64)
                .unwrap_or(-1),
        })),
        _ => Ok(decoded(Incoming::Other)),
    }
}

fn session_created(value: &Value) -> Result<DecodedSignal, BridgeError> {
    let id = required(value, "/body/session_id")?;
    if id.len() > 1024 {
        return Err(protocol("session id exceeds limit"));
    }
    Ok(DecodedSignal {
        incoming: Incoming::SessionCreated,
        session_id: Some(Zeroizing::new(id)),
    })
}

const fn decoded(incoming: Incoming) -> DecodedSignal {
    DecodedSignal {
        incoming,
        session_id: None,
    }
}

fn required(value: &Value, pointer: &str) -> Result<String, BridgeError> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| protocol("required signal field is missing"))
}

fn protocol(message: &str) -> BridgeError {
    BridgeError::Protocol(message.into())
}

#[cfg(test)]
mod tests {
    use super::{Incoming, decode_signal};

    #[test]
    fn session_identity_is_retained_but_not_exposed_as_evidence() {
        let input = br#"{"method":"session_created","body":{"doorbot_id":42,"session_id":"synthetic-session"}}"#;
        let decoded =
            decode_signal(input, 42, 1024).unwrap_or_else(|error| panic!("signal failed: {error}"));
        assert!(matches!(decoded.incoming, Incoming::SessionCreated));
        assert_eq!(
            decoded.session_id.as_ref().map(|value| value.as_str()),
            Some("synthetic-session")
        );
    }

    #[test]
    fn signals_for_another_device_are_ignored() {
        let input = br#"{"method":"sdp","body":{"doorbot_id":43,"sdp":"secret-shaped"}}"#;
        let decoded =
            decode_signal(input, 42, 1024).unwrap_or_else(|error| panic!("signal failed: {error}"));
        assert!(matches!(decoded.incoming, Incoming::Other));
    }
}
