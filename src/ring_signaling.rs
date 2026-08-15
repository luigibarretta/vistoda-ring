use futures_util::{SinkExt, StreamExt};
use reqwest::Url;
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{Message, client::IntoClientRequest},
};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::{
    error::BridgeError,
    ring_protocol::{SIGNALING_ORIGIN, USER_AGENT},
    ring_signal_wire::decode_signal,
};

const MAX_SIGNAL_BYTES: usize = 256 * 1024;
type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

pub struct Signaling {
    socket: Socket,
    dialog_id: Uuid,
    device_id: u64,
    session_id: Option<Zeroizing<String>>,
}

pub use crate::ring_signal_wire::Incoming;

impl Signaling {
    pub async fn connect(ticket: &str, device_id: u64) -> Result<Self, BridgeError> {
        let mut url = Url::parse(SIGNALING_ORIGIN)
            .map_err(|_| protocol("invalid compiled signaling origin"))?;
        url.query_pairs_mut()
            .append_pair("api_version", "4.0")
            .append_pair("auth_type", "ring_solutions")
            .append_pair("client_id", &format!("ring_site-{}", Uuid::new_v4()))
            .append_pair("token", ticket);
        let mut request = url
            .as_str()
            .into_client_request()
            .map_err(|_| protocol("invalid signaling request"))?;
        request.headers_mut().insert(
            "User-Agent",
            USER_AGENT
                .parse()
                .map_err(|_| protocol("invalid user agent"))?,
        );
        let (socket, response) = connect_async(request)
            .await
            .map_err(|_| protocol("signaling connection failed"))?;
        if response.status().as_u16() != 101 {
            return Err(protocol("signaling upgrade was rejected"));
        }
        Ok(Self {
            socket,
            dialog_id: Uuid::new_v4(),
            device_id,
            session_id: None,
        })
    }

    pub async fn offer(&mut self, sdp: &str) -> Result<(), BridgeError> {
        self.send(json!({
            "method": "live_view",
            "dialog_id": self.dialog_id,
            "body": {
                "doorbot_id": self.device_id,
                "stream_options": {"audio_enabled": true, "video_enabled": false},
                "sdp": sdp,
                "type": "offer"
            }
        }))
        .await
    }

    pub async fn next(&mut self) -> Result<Option<Incoming>, BridgeError> {
        while let Some(message) = self.socket.next().await {
            let message = message.map_err(|_| protocol("signaling read failed"))?;
            match message {
                Message::Text(text) => return self.parse(text.as_bytes()),
                Message::Binary(bytes) => return self.parse(&bytes),
                Message::Ping(bytes) => self
                    .socket
                    .send(Message::Pong(bytes))
                    .await
                    .map_err(|_| protocol("signaling pong failed"))?,
                Message::Close(_) => return Ok(None),
                Message::Pong(_) | Message::Frame(_) => {}
            }
        }
        Ok(None)
    }

    pub async fn activate(&mut self) -> Result<(), BridgeError> {
        self.session_message("activate_session", json!({})).await?;
        self.session_message(
            "stream_options",
            json!({
                "audio_enabled": true,
                "video_enabled": false
            }),
        )
        .await
    }

    pub async fn camera_options(&mut self) -> Result<(), BridgeError> {
        self.session_message("camera_options", json!({"stealth_mode": false}))
            .await
    }

    pub async fn ping(&mut self) -> Result<(), BridgeError> {
        if self.session_id.is_some() {
            self.session_message("ping", json!({})).await?;
        }
        Ok(())
    }

    pub async fn close(mut self) -> Result<(), BridgeError> {
        let _ = self
            .send(json!({
                "method": "close",
                "reason": {"code": 0, "text": ""}
            }))
            .await;
        self.socket
            .close(None)
            .await
            .map_err(|_| protocol("signaling close failed"))
    }

    async fn session_message(&mut self, method: &str, extra: Value) -> Result<(), BridgeError> {
        let Some(session_id) = self.session_id.as_deref() else {
            return Err(protocol("signaling session is unavailable"));
        };
        let mut body = json!({
            "doorbot_id": self.device_id,
            "session_id": session_id
        });
        let Value::Object(target) = &mut body else {
            unreachable!()
        };
        let Value::Object(extra) = extra else {
            return Err(protocol("invalid signal body"));
        };
        target.extend(extra);
        self.send(json!({"method": method, "dialog_id": self.dialog_id, "body": body}))
            .await
    }

    async fn send(&mut self, value: Value) -> Result<(), BridgeError> {
        let encoded = serde_json::to_string(&value)?;
        if encoded.len() > MAX_SIGNAL_BYTES {
            return Err(protocol("outbound signal exceeds limit"));
        }
        self.socket
            .send(Message::Text(encoded.into()))
            .await
            .map_err(|_| protocol("signaling write failed"))
    }

    fn parse(&mut self, bytes: &[u8]) -> Result<Option<Incoming>, BridgeError> {
        let decoded = decode_signal(bytes, self.device_id, MAX_SIGNAL_BYTES)?;
        if let Some(session_id) = decoded.session_id {
            self.session_id = Some(session_id);
        }
        Ok(Some(decoded.incoming))
    }
}

fn protocol(message: &str) -> BridgeError {
    BridgeError::Protocol(message.into())
}
