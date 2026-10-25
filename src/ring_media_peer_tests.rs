//! Offline proof of the Ring media path on the WebRTC stack: `MediaPeer` offers
//! to an in-process stand-in for Ring on loopback, completes ICE, DTLS and SRTP,
//! and exchanges PCMU frames in both directions with the relay framing.
use std::{sync::Arc, time::Duration, time::Instant};

use rtc::{
    interceptor::Registry,
    media::Sample,
    peer_connection::{
        configuration::{
            interceptor_registry::register_default_interceptors,
            setting_engine::SettingEngineBuilder,
        },
        sdp::RTCSessionDescription,
        transport::RTCDtlsRole,
    },
};
use tokio::sync::mpsc;
use webrtc::{
    media_stream::{
        Track,
        track_local::{TrackLocal, static_sample::TrackLocalStaticSample},
        track_remote::{TrackRemote, TrackRemoteEvent},
    },
    peer_connection::{
        PeerConnection, PeerConnectionBuilder, PeerConnectionEventHandler, RTCIceGatheringState,
    },
    runtime::{Runtime, Sender, TokioRuntime, channel},
};

use crate::{
    ring_media_peer::{MediaPeer, STUN_URLS},
    ring_media_setup::{local_track, media_engine},
};

const WAIT: Duration = Duration::from_secs(10);
const CLIENT_BYTE: u8 = 0x55;
const RING_BYTE: u8 = 0x33;

struct RingSide {
    gather: Sender<()>,
    heard: mpsc::UnboundedSender<(u8, Vec<u8>)>,
    runtime: Arc<dyn Runtime>,
}

#[async_trait::async_trait]
impl PeerConnectionEventHandler for RingSide {
    async fn on_ice_gathering_state_change(&self, state: RTCIceGatheringState) {
        if state == RTCIceGatheringState::Complete {
            let _ = self.gather.try_send(());
        }
    }

    async fn on_track(&self, track: Arc<dyn TrackRemote>) {
        let heard = self.heard.clone();
        self.runtime.spawn(Box::pin(async move {
            while let Some(event) = track.poll().await {
                if let TrackRemoteEvent::OnRtpPacket(packet) = event {
                    let _ = heard.send((packet.header.payload_type, packet.payload.to_vec()));
                }
            }
        }));
    }
}

fn check<T, E: std::fmt::Debug>(result: Result<T, E>, what: &str) -> T {
    result.unwrap_or_else(|error| panic!("{what}: {error:?}"))
}

/// Answers `offer` like Ring: PCMU-only sendrecv audio on loopback with the given
/// answering DTLS role. Returns the gathered answer, the answerer and its track.
async fn ring_answer(
    offer: String,
    role: RTCDtlsRole,
    heard: mpsc::UnboundedSender<(u8, Vec<u8>)>,
) -> (String, Arc<dyn PeerConnection>, Arc<TrackLocalStaticSample>) {
    let (codec, mut engine) = check(media_engine(), "ring media engine");
    let registry = check(
        register_default_interceptors(Registry::new(), &mut engine),
        "ring interceptors",
    );
    let runtime: Arc<dyn Runtime> = Arc::new(TokioRuntime);
    let (gather, mut gathered) = channel(1);
    let handler = Arc::new(RingSide {
        gather,
        heard,
        runtime: Arc::clone(&runtime),
    });
    let settings = SettingEngineBuilder::new()
        .with_answering_dtls_role(role)
        .build();
    let ring = check(
        Box::pin(
            PeerConnectionBuilder::new()
                .with_media_engine(engine)
                .with_interceptor_registry(registry)
                .with_setting_engine(settings)
                .with_handler(handler)
                .with_runtime(runtime)
                .with_udp_addrs(vec!["127.0.0.1:0".to_owned()])
                .build(),
        )
        .await,
        "ring peer",
    );
    let ring: Arc<dyn PeerConnection> = Arc::new(ring);
    let track = check(local_track(&codec), "ring track");
    check(
        ring.add_track(Arc::clone(&track) as Arc<dyn TrackLocal>)
            .await,
        "ring add track",
    );
    let offer = check(RTCSessionDescription::offer(offer), "offer parse");
    check(ring.set_remote_description(offer).await, "ring remote SDP");
    let answer = check(ring.create_answer(None).await, "ring answer");
    check(ring.set_local_description(answer).await, "ring local SDP");
    let _ = tokio::time::timeout(WAIT, gathered.recv()).await;
    let answer = ring.local_description().await.map(|value| value.sdp);
    (answer.unwrap_or_default(), ring, track)
}

fn assert_bridge_offer(offer: &str) {
    assert_eq!(offer.matches("m=").count(), 1, "audio only: {offer}");
    assert!(
        offer.contains("m=audio 9 UDP/TLS/RTP/SAVPF 0\r\n"),
        "{offer}"
    );
    assert!(offer.contains("a=rtpmap:0 PCMU/8000"), "{offer}");
    assert!(offer.contains("a=sendrecv"), "{offer}");
    assert!(offer.contains("a=setup:actpass"), "{offer}");
    assert!(offer.contains("a=fingerprint:sha-256 "), "{offer}");
    assert!(offer.contains("a=rtcp-mux"), "{offer}");
    assert!(
        offer.contains("a=candidate:"),
        "gathered host candidate: {offer}"
    );
    assert!(!offer.contains("opus") && !offer.contains("m=application"));
}

async fn call(role: RTCDtlsRole, setup: &str, trickle: bool) {
    let (ring_audio, mut relayed) = mpsc::channel(64);
    let peer = check(
        MediaPeer::new_loopback(Some(ring_audio), None).await,
        "bridge peer",
    );
    let offer = check(peer.offer().await, "bridge offer");
    assert_bridge_offer(&offer);
    let (heard_tx, mut heard) = mpsc::unbounded_channel();
    let (answer, ring, ring_track) = ring_answer(offer, role, heard_tx).await;
    assert!(answer.contains(setup), "{setup} expected: {answer}");
    assert!(answer.contains("a=rtpmap:0 PCMU/8000"), "{answer}");
    if trickle {
        let (body, candidates): (Vec<&str>, Vec<&str>) = answer
            .split("\r\n")
            .partition(|line| !line.starts_with("a=candidate:"));
        check(peer.accept_answer(body.join("\r\n")).await, "answer");
        assert!(!candidates.is_empty(), "ring answer has candidates");
        for line in candidates {
            check(peer.add_ice(line[2..].to_owned(), 0).await, "trickle ICE");
        }
    } else {
        check(peer.accept_answer(answer).await, "answer");
    }
    check(peer.wait_connected(WAIT).await, "DTLS/SRTP connected");

    let (client, client_audio) = mpsc::channel(8);
    peer.start_audio(client_audio, Duration::from_secs(5), None);
    let feeder = tokio::spawn(async move {
        while client.send(vec![CLIENT_BYTE; 160]).await.is_ok() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    });
    let ssrc = ring_track
        .ssrcs()
        .await
        .first()
        .copied()
        .unwrap_or_default();
    let speaker = tokio::spawn(async move {
        for _ in 0..150 {
            let sample = Sample {
                data: vec![RING_BYTE; 160].into(),
                duration: Duration::from_millis(20),
                ..Sample::new(Instant::now())
            };
            let _ = ring_track
                .sample_writer(ssrc, 0)
                .write_sample(&sample)
                .await;
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    });

    let from_ring = tokio::time::timeout(WAIT, relayed.recv()).await;
    assert_eq!(
        check(from_ring, "Ring audio relayed"),
        Some(vec![RING_BYTE; 160])
    );
    let to_ring = check(
        tokio::time::timeout(WAIT, async {
            loop {
                match heard.recv().await {
                    Some((0, payload)) if payload == vec![CLIENT_BYTE; 160] => break true,
                    Some(_) => {}
                    None => break false,
                }
            }
        })
        .await,
        "client audio reached Ring",
    );
    assert!(to_ring, "client PCMU frame with payload type 0");
    let snapshot = peer.snapshot().await;
    assert!(snapshot.connected && snapshot.received_packets > 0);
    assert_eq!(snapshot.received_bytes % 160, 0);
    assert_eq!(snapshot.codec.as_deref(), Some("audio/PCMU"));
    feeder.abort();
    speaker.abort();
    check(peer.close().await, "bridge close");
    check(ring.close().await, "ring close");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ring_as_dtls_client_exchanges_pcmu_both_ways() {
    call(RTCDtlsRole::Client, "a=setup:active", false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ring_as_dtls_server_with_trickled_candidates_exchanges_pcmu() {
    call(RTCDtlsRole::Server, "a=setup:passive", true).await;
}

#[test]
fn production_ice_servers_are_unchanged() {
    assert_eq!(
        STUN_URLS,
        [
            "stun:stun.kinesisvideo.us-east-1.amazonaws.com:443",
            "stun:stun.l.google.com:19302",
        ]
    );
}
