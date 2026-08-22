use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use rtc::media::Sample;
use tokio::sync::{Notify, mpsc};
use webrtc::{
    media_stream::{Track, track_local::static_sample::TrackLocalStaticSample},
    rtp_transceiver::RtpSender,
};

use crate::{BridgeError, ring_media_handler::PeerStats, ring_relay_metrics::RelayMetrics};

const FRAME_BYTES: usize = 160;
const FRAME_DURATION: Duration = Duration::from_millis(20);

#[allow(clippy::too_many_arguments)]
pub fn spawn(
    track: Arc<TrackLocalStaticSample>,
    sender: Arc<dyn RtpSender>,
    connected: Arc<Notify>,
    stopped: Arc<AtomicBool>,
    stats: Arc<PeerStats>,
    mut receiver: mpsc::Receiver<Vec<u8>>,
    deadline: Duration,
    metrics: Option<Arc<RelayMetrics>>,
) {
    tokio::spawn(async move {
        let notified = connected.notified();
        tokio::pin!(notified);
        if !stats.is_connected() && tokio::time::timeout(deadline, &mut notified).await.is_err() {
            return;
        }
        let Ok(payload_type) = negotiated_payload_type(&sender).await else {
            return;
        };
        let Some(ssrc) = track.ssrcs().await.first().copied() else {
            return;
        };
        let end = tokio::time::Instant::now() + deadline;
        let mut interval = tokio::time::interval(FRAME_DURATION);
        while tokio::time::Instant::now() < end && !stopped.load(Ordering::Relaxed) {
            interval.tick().await;
            let frame = newest_frame(&mut receiver).unwrap_or_else(silence);
            let silent = frame.iter().all(|byte| *byte == 0xff);
            let result = track
                .sample_writer(ssrc, payload_type)
                .write_sample(&Sample {
                    data: frame.into(),
                    duration: FRAME_DURATION,
                    ..Default::default()
                })
                .await;
            if result.is_ok() {
                if silent {
                    stats.sent_silence();
                }
                if let Some(metrics) = &metrics {
                    metrics.client_frame_forwarded();
                }
            }
        }
    });
}

fn newest_frame(receiver: &mut mpsc::Receiver<Vec<u8>>) -> Option<Vec<u8>> {
    let mut newest = None;
    while let Ok(frame) = receiver.try_recv() {
        if frame.len() == FRAME_BYTES {
            newest = Some(frame);
        }
    }
    newest
}

fn silence() -> Vec<u8> {
    vec![0xff; FRAME_BYTES]
}

async fn negotiated_payload_type(sender: &Arc<dyn RtpSender>) -> Result<u8, BridgeError> {
    sender
        .get_parameters()
        .await
        .map_err(|_| protocol("audio negotiation unavailable"))?
        .rtp_parameters
        .codecs
        .first()
        .map(|codec| codec.payload_type)
        .ok_or_else(|| protocol("audio codec was not negotiated"))
}

fn protocol(message: &str) -> BridgeError {
    BridgeError::Protocol(message.into())
}

#[cfg(test)]
mod tests {
    use super::{FRAME_BYTES, newest_frame, silence};
    use tokio::sync::mpsc;

    #[test]
    fn silence_is_one_pcmu_frame() {
        assert_eq!(silence(), vec![0xff; FRAME_BYTES]);
    }

    #[tokio::test]
    async fn newest_complete_frame_wins() {
        let (sender, mut receiver) = mpsc::channel(3);
        sender
            .try_send(vec![1; FRAME_BYTES])
            .unwrap_or_else(|error| panic!("send: {error}"));
        sender
            .try_send(vec![2; 3])
            .unwrap_or_else(|error| panic!("send: {error}"));
        sender
            .try_send(vec![3; FRAME_BYTES])
            .unwrap_or_else(|error| panic!("send: {error}"));
        assert_eq!(newest_frame(&mut receiver), Some(vec![3; FRAME_BYTES]));
    }
}
