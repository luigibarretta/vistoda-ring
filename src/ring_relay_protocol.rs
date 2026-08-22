use serde::{Deserialize, Serialize};

use crate::ring_audio::SESSION_SECONDS;

pub const PROTOCOL: &str = "vistoda.pcmu.v1";
pub const FRAME_BYTES: usize = 160;
pub const MAX_MESSAGE_BYTES: usize = 2_048;

#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ClientCommand {
    Ping {},
    Stop {},
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RelayStage {
    Active,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ServerEvent<'a> {
    Session {
        state: &'a str,
        protocol: &'static str,
        session_id: &'a str,
        codec: &'static str,
        sample_rate_hz: u16,
        frame_ms: u8,
        expires_in: u64,
    },
    Ended {
        reason: &'a str,
    },
    Error {
        code: &'a str,
    },
    Pong,
}

pub fn session(state: &str, session_id: &str) -> String {
    encode(&ServerEvent::Session {
        state,
        protocol: PROTOCOL,
        session_id,
        codec: "PCMU",
        sample_rate_hz: 8_000,
        frame_ms: 20,
        expires_in: SESSION_SECONDS,
    })
}

pub fn ended(reason: &str) -> String {
    encode(&ServerEvent::Ended { reason })
}

pub fn error(code: &str) -> String {
    encode(&ServerEvent::Error { code })
}

pub fn pong() -> String {
    encode(&ServerEvent::Pong)
}

pub fn parse(text: &str) -> Option<ClientCommand> {
    if text.len() > MAX_MESSAGE_BYTES {
        return None;
    }
    serde_json::from_str(text).ok()
}

fn encode(event: &ServerEvent<'_>) -> String {
    serde_json::to_string(event)
        .unwrap_or_else(|_| "{\"type\":\"error\",\"code\":\"internal\"}".into())
}

#[cfg(test)]
mod tests {
    use super::{ClientCommand, PROTOCOL, error, parse, session};

    #[test]
    fn client_protocol_is_closed_and_strict() {
        assert_eq!(parse(r#"{"type":"ping"}"#), Some(ClientCommand::Ping {}));
        assert_eq!(parse(r#"{"type":"stop"}"#), Some(ClientCommand::Stop {}));
        assert_eq!(parse(r#"{"type":"stop","extra":1}"#), None);
        assert_eq!(parse(r#"{"type":"unknown"}"#), None);
    }

    #[test]
    fn session_contract_is_explicit_and_secret_free() {
        let payload = session("connecting", "synthetic-session");
        assert!(payload.contains(PROTOCOL));
        assert!(payload.contains("\"sample_rate_hz\":8000"));
        assert!(payload.contains("\"expires_in\":120"));
        assert!(!payload.contains("token"));
        assert_eq!(
            error("session_busy"),
            r#"{"type":"error","code":"session_busy"}"#
        );
    }
}
