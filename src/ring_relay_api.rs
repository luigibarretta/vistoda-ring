use std::{sync::Arc, time::Duration};

use axum::{
    Router,
    extract::{
        FromRequestParts, Path, Request, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    response::{IntoResponse, Response},
    routing::get,
};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::{mpsc, oneshot};

use crate::{
    BridgeError,
    api::Runtime,
    auth::require_bearer,
    ring_audio::SessionEndReason,
    ring_relay_protocol::{
        ClientCommand, FRAME_BYTES, MAX_MESSAGE_BYTES, RelayStage, ended, error, parse, pong,
        session,
    },
    ring_relay_worker::RelayWorker,
};

const AUDIO_QUEUE: usize = 32;
const CLIENT_QUEUE: usize = 8;
const STOP_TIMEOUT: Duration = Duration::from_secs(5);

pub fn routes() -> Router<Arc<Runtime>> {
    Router::new().route("/v1/devices/{device}/audio/relay", get(upgrade_relay))
}

async fn upgrade_relay(
    State(runtime): State<Arc<Runtime>>,
    Path(device): Path<String>,
    request: Request,
) -> Result<Response, BridgeError> {
    let (mut parts, _body) = request.into_parts();
    require_bearer(&parts.headers, &runtime.config.api_token)?;
    if !runtime.config.devices.contains_key(&device) {
        return Err(BridgeError::DeviceNotFound);
    }
    let websocket = match WebSocketUpgrade::from_request_parts(&mut parts, &runtime).await {
        Ok(value) => value,
        Err(error) => return Ok(error.into_response()),
    };
    Ok(websocket
        .max_message_size(MAX_MESSAGE_BYTES)
        .max_frame_size(MAX_MESSAGE_BYTES)
        .on_upgrade(move |socket| serve(runtime, device, socket)))
}

async fn serve(runtime: Arc<Runtime>, device: String, socket: WebSocket) {
    let reservation = match runtime.audio.reserve_relay(device).await {
        Ok(value) => value,
        Err(BridgeError::SessionBusy) => {
            reject(socket, "session_busy").await;
            return;
        }
        Err(BridgeError::RateLimited) => {
            reject(socket, "cooldown").await;
            return;
        }
        Err(_) => {
            reject(socket, "unavailable").await;
            return;
        }
    };
    let session_id = reservation.id.to_string();
    let (mut sink, mut source) = socket.split();
    if sink
        .send(Message::Text(session("connecting", &session_id).into()))
        .await
        .is_err()
    {
        runtime
            .audio
            .finish_relay(reservation, SessionEndReason::ConnectionEnded)
            .await;
        return;
    }
    let (ring_sender, mut ring_receiver) = mpsc::channel(AUDIO_QUEUE);
    let (client_sender, client_receiver) = mpsc::channel(CLIENT_QUEUE);
    let (stage_sender, mut stage_receiver) = mpsc::channel(2);
    let (cancel_sender, cancel_receiver) = oneshot::channel();
    let worker = RelayWorker::new(
        Arc::clone(&runtime.provider),
        Arc::clone(&runtime.relay_metrics),
    );
    let mut task =
        tokio::spawn(worker.run(ring_sender, client_receiver, stage_sender, cancel_receiver));
    let mut cancel_sender = Some(cancel_sender);
    let mut task_done = false;
    let mut reason = SessionEndReason::ConnectionEnded;
    loop {
        tokio::select! {
            Some(frame) = ring_receiver.recv() => {
                if sink.send(Message::Binary(frame.into())).await.is_err() { break }
            }
            Some(stage) = stage_receiver.recv() => {
                if stage == RelayStage::Active {
                    runtime.audio.relay_started();
                    if sink.send(Message::Text(session("active", &session_id).into()))
                        .await.is_err() { break }
                }
            }
            message = source.next() => {
                match handle_client(message, &client_sender, &runtime) {
                    ClientResult::Continue => {}
                    ClientResult::Reply(payload) => {
                        if sink.send(Message::Text(payload.into())).await.is_err() { break }
                    }
                    ClientResult::Stop(stop_reason) => {
                        reason = stop_reason;
                        break;
                    }
                }
            }
            result = &mut task => {
                reason = result.unwrap_or(SessionEndReason::StartupFailed);
                task_done = true;
                break;
            }
        }
    }
    if !task_done {
        if let Some(sender) = cancel_sender.take() {
            let _ = sender.send(());
        }
        match tokio::time::timeout(STOP_TIMEOUT, &mut task).await {
            Ok(Ok(worker_reason)) if reason == SessionEndReason::ConnectionEnded => {
                reason = worker_reason;
            }
            Ok(_) => {}
            Err(_) => {
                task.abort();
                reason = SessionEndReason::ConnectionEnded;
            }
        }
    }
    runtime.audio.finish_relay(reservation, reason).await;
    let _ = sink
        .send(Message::Text(ended(reason.as_str()).into()))
        .await;
    let _ = sink.close().await;
}

enum ClientResult {
    Continue,
    Reply(String),
    Stop(SessionEndReason),
}

fn handle_client(
    message: Option<Result<Message, axum::Error>>,
    audio: &mpsc::Sender<Vec<u8>>,
    runtime: &Runtime,
) -> ClientResult {
    match message {
        Some(Ok(Message::Binary(frame))) if frame.len() == FRAME_BYTES => {
            match audio.try_send(frame.to_vec()) {
                Ok(()) => runtime.relay_metrics.client_frame_accepted(),
                Err(_) => runtime.relay_metrics.client_frame_dropped(),
            }
            ClientResult::Continue
        }
        Some(Ok(Message::Text(text))) => match parse(&text) {
            Some(ClientCommand::Ping {}) => ClientResult::Reply(pong()),
            Some(ClientCommand::Stop {}) => ClientResult::Stop(SessionEndReason::UserStop),
            None => ClientResult::Reply(error("invalid_command")),
        },
        Some(Ok(Message::Ping(_) | Message::Pong(_))) => ClientResult::Continue,
        Some(Ok(Message::Close(_))) | None => ClientResult::Stop(SessionEndReason::ConnectionEnded),
        Some(Ok(Message::Binary(_)) | Err(_)) => {
            ClientResult::Stop(SessionEndReason::ConnectionEnded)
        }
    }
}

async fn reject(mut socket: WebSocket, code: &str) {
    let _ = socket.send(Message::Text(error(code).into())).await;
    let _ = socket.close().await;
}
