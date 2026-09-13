use super::*;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;

const OFFER: &str = "v=0\r\nm=audio 9 UDP/TLS/RTP/SAVPF 0\r\na=sendrecv\r\nm=video 9 UDP/TLS/RTP/SAVPF 96\r\na=rtpmap:96 H264/90000\r\na=fmtp:96 packetization-mode=1;profile-level-id=42e01f\r\na=recvonly\r\na=rtcp-mux\r\na=rtcp-fb:96 nack pli\r\n";

#[tokio::test]
async fn intercom_signaling_keeps_video_disabled_by_default() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let address = listener.local_addr().unwrap_or_else(|e| panic!("{e}"));
    let mock = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap_or_else(|e| panic!("{e}"));
        let mut socket = tokio_tungstenite::accept_async(stream)
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        let offer = socket
            .next()
            .await
            .unwrap_or_else(|| panic!("missing offer"))
            .unwrap_or_else(|e| panic!("{e}"));
        let value: Value = serde_json::from_str(offer.to_text().unwrap_or_default())
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(value["body"]["stream_options"]["video_enabled"], false);
        assert_eq!(value["body"]["doorbot_id"], 42);
        socket.send(Message::Text(json!({"method":"session_created", "body":{"doorbot_id":42,"session_id":"synthetic-session"}}).to_string().into())).await.unwrap_or_else(|e| panic!("{e}"));
        let mut checked = false;
        while let Some(Ok(message)) = socket.next().await {
            let Ok(text) = message.to_text() else {
                continue;
            };
            let value: Value = serde_json::from_str(text).unwrap_or_else(|e| panic!("{e}"));
            if value["method"] == "stream_options" {
                assert_eq!(value["body"]["video_enabled"], false);
                checked = true;
            }
            if value["method"] == "close" {
                break;
            }
        }
        assert!(checked);
    });
    let mut signaling = Signaling::test_socket(&format!("ws://{address}"), 42).await;
    signaling
        .offer("synthetic-audio-offer")
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert!(matches!(
        signaling.next().await,
        Ok(Some(Incoming::SessionCreated))
    ));
    signaling.activate().await.unwrap_or_else(|e| panic!("{e}"));
    signaling.close().await.unwrap_or_else(|e| panic!("{e}"));
    mock.await.unwrap_or_else(|e| panic!("{e}"));
}

#[tokio::test]
async fn direct_camera_signaling_preserves_h264_sdp_and_ice_without_pcm_relay() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let address = listener.local_addr().unwrap_or_else(|e| panic!("{e}"));
    let answer = OFFER.replace("a=recvonly", "a=sendonly");
    let expected_answer = answer.clone();
    let mock = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap_or_else(|e| panic!("{e}"));
        let mut socket = tokio_tungstenite::accept_async(stream)
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        let offer = socket
            .next()
            .await
            .unwrap_or_else(|| panic!("missing offer"))
            .unwrap_or_else(|e| panic!("{e}"));
        let offer: Value = serde_json::from_str(offer.to_text().unwrap_or_default())
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(offer["method"], "live_view");
        assert_eq!(offer["body"]["doorbot_id"], 51);
        assert_eq!(offer["body"]["stream_options"]["video_enabled"], true);
        assert_eq!(offer["body"]["sdp"], OFFER);
        for value in [
            json!({"method":"session_created", "body":{"doorbot_id":51,"session_id":"synthetic-session"}}),
            json!({"method":"sdp", "body":{"doorbot_id":52,"sdp":"wrong-device"}}),
            json!({"method":"sdp", "body":{"doorbot_id":51,"sdp":answer}}),
            json!({"method":"ice", "body":{"doorbot_id":51,"ice":"candidate:synthetic-video","mlineindex":1}}),
            json!({"method":"notification", "body":{"doorbot_id":51,"text":"camera_connected"}}),
        ] {
            socket
                .send(Message::Text(value.to_string().into()))
                .await
                .unwrap_or_else(|e| panic!("{e}"));
        }
        let mut activated = false;
        while let Some(Ok(message)) = socket.next().await {
            let Ok(text) = message.to_text() else {
                continue;
            };
            let value: Value = serde_json::from_str(text).unwrap_or_else(|e| panic!("{e}"));
            if value["method"] == "stream_options" {
                assert_eq!(value["body"]["video_enabled"], true);
                assert_eq!(value["body"]["doorbot_id"], 51);
                activated = true;
            }
            if value["method"] == "close" {
                break;
            }
        }
        assert!(activated);
    });
    let mut signaling = Signaling::test_socket(&format!("ws://{address}"), 51).await;
    signaling.set_video(true);
    signaling
        .offer(OFFER)
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let worker =
        ProductionSessionRunner::camera(Arc::new(RingProvider::new("unused-synthetic".into())));
    let result = worker
        .collect(&mut signaling)
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    drop(worker);
    assert_eq!(result.answer_sdp, expected_answer);
    assert_eq!(result.ice_candidates.len(), 1);
    assert_eq!(result.ice_candidates[0].sdp_mline_index, 1);
    assert_eq!(
        result.ice_candidates[0].candidate,
        "candidate:synthetic-video"
    );
    signaling.close().await.unwrap_or_else(|e| panic!("{e}"));
    mock.await.unwrap_or_else(|e| panic!("{e}"));
}
