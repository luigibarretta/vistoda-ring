use std::{
    fmt::Write,
    sync::atomic::{AtomicU64, Ordering},
};

#[derive(Default)]
pub struct RelayMetrics {
    accepted_client_frames: AtomicU64,
    dropped_client_frames: AtomicU64,
    forwarded_client_frames: AtomicU64,
    forwarded_ring_frames: AtomicU64,
    forwarded_ring_bytes: AtomicU64,
    dropped_ring_frames: AtomicU64,
}

impl RelayMetrics {
    pub fn client_frame_accepted(&self) {
        self.accepted_client_frames.fetch_add(1, Ordering::Relaxed);
    }

    pub fn client_frame_dropped(&self) {
        self.dropped_client_frames.fetch_add(1, Ordering::Relaxed);
    }

    pub fn client_frame_forwarded(&self) {
        self.forwarded_client_frames.fetch_add(1, Ordering::Relaxed);
    }

    pub fn ring_frame_forwarded(&self, bytes: u64) {
        self.forwarded_ring_frames.fetch_add(1, Ordering::Relaxed);
        self.forwarded_ring_bytes
            .fetch_add(bytes, Ordering::Relaxed);
    }

    pub fn ring_frame_dropped(&self) {
        self.dropped_ring_frames.fetch_add(1, Ordering::Relaxed);
    }

    #[must_use]
    pub fn render(&self) -> String {
        let mut output = String::with_capacity(1_024);
        header(
            &mut output,
            "vistoda_ring_relay_audio_frames_total",
            "Bounded native relay audio frames by stage",
        );
        for (stage, value) in [
            ("client_accepted", &self.accepted_client_frames),
            ("client_forwarded", &self.forwarded_client_frames),
            ("ring_forwarded", &self.forwarded_ring_frames),
        ] {
            let _ = writeln!(
                output,
                "vistoda_ring_relay_audio_frames_total{{stage=\"{stage}\"}} {}",
                value.load(Ordering::Relaxed)
            );
        }
        header(
            &mut output,
            "vistoda_ring_relay_audio_dropped_total",
            "Native relay frames dropped by bounded queues",
        );
        let _ = writeln!(
            output,
            "vistoda_ring_relay_audio_dropped_total{{direction=\"client_to_ring\"}} {}",
            self.dropped_client_frames.load(Ordering::Relaxed)
        );
        let _ = writeln!(
            output,
            "vistoda_ring_relay_audio_dropped_total{{direction=\"ring_to_client\"}} {}",
            self.dropped_ring_frames.load(Ordering::Relaxed)
        );
        header(
            &mut output,
            "vistoda_ring_relay_audio_ring_bytes_total",
            "PCMU payload bytes forwarded from Ring",
        );
        let _ = writeln!(
            output,
            "vistoda_ring_relay_audio_ring_bytes_total {}",
            self.forwarded_ring_bytes.load(Ordering::Relaxed)
        );
        output
    }
}

fn header(output: &mut String, name: &str, help: &str) {
    let _ = writeln!(output, "# HELP {name} {help}");
    let _ = writeln!(output, "# TYPE {name} counter");
}

#[cfg(test)]
mod tests {
    use super::RelayMetrics;

    #[test]
    fn metrics_are_aggregate_and_bounded() {
        let metrics = RelayMetrics::default();
        metrics.client_frame_accepted();
        metrics.client_frame_dropped();
        metrics.client_frame_forwarded();
        metrics.ring_frame_forwarded(160);
        metrics.ring_frame_dropped();
        let output = metrics.render();
        assert!(output.contains("stage=\"client_accepted\"} 1"));
        assert!(output.contains("direction=\"ring_to_client\"} 1"));
        assert!(output.contains("ring_bytes_total 160"));
        assert!(!output.contains("session_id"));
    }
}
