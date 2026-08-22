use std::{
    fmt::Write,
    sync::atomic::{AtomicI64, AtomicU64, Ordering},
};

use crate::ring_audio::{AudioMode, SessionEndReason};

const ICE_BUCKETS_MS: [u64; 8] = [100, 250, 500, 1_000, 2_000, 4_000, 8_000, 20_000];
const DURATION_BUCKETS_MS: [u64; 7] = [1_000, 5_000, 15_000, 30_000, 60_000, 120_000, 180_000];

struct Histogram<const N: usize> {
    bounds_ms: [u64; N],
    buckets: [AtomicU64; N],
    count: AtomicU64,
    sum_ms: AtomicU64,
}

impl<const N: usize> Histogram<N> {
    fn new(bounds_ms: [u64; N]) -> Self {
        Self {
            bounds_ms,
            buckets: std::array::from_fn(|_| AtomicU64::new(0)),
            count: AtomicU64::new(0),
            sum_ms: AtomicU64::new(0),
        }
    }

    fn observe(&self, value_ms: u64) {
        self.count.fetch_add(1, Ordering::Relaxed);
        self.sum_ms.fetch_add(value_ms, Ordering::Relaxed);
        for (bound, bucket) in self.bounds_ms.iter().zip(&self.buckets) {
            if value_ms <= *bound {
                bucket.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    fn render(&self, output: &mut String, name: &str, help: &str) {
        metric_header(output, name, help, "histogram");
        for (bound, bucket) in self.bounds_ms.iter().zip(&self.buckets) {
            let _ = writeln!(
                output,
                "{name}_bucket{{le=\"{}\"}} {}",
                seconds(*bound),
                bucket.load(Ordering::Relaxed)
            );
        }
        let count = self.count.load(Ordering::Relaxed);
        let _ = writeln!(output, "{name}_bucket{{le=\"+Inf\"}} {count}");
        let _ = writeln!(
            output,
            "{name}_sum {}",
            seconds(self.sum_ms.load(Ordering::Relaxed))
        );
        let _ = writeln!(output, "{name}_count {count}");
    }
}

pub struct RingMetrics {
    active: AtomicI64,
    started_listen: AtomicU64,
    started_talk: AtomicU64,
    ended: [AtomicU64; SessionEndReason::COUNT],
    ice: Histogram<8>,
    duration: Histogram<7>,
}

impl Default for RingMetrics {
    fn default() -> Self {
        Self {
            active: AtomicI64::new(0),
            started_listen: AtomicU64::new(0),
            started_talk: AtomicU64::new(0),
            ended: std::array::from_fn(|_| AtomicU64::new(0)),
            ice: Histogram::new(ICE_BUCKETS_MS),
            duration: Histogram::new(DURATION_BUCKETS_MS),
        }
    }
}

impl RingMetrics {
    pub fn reserved(&self) {
        self.active.fetch_add(1, Ordering::Relaxed);
    }

    pub fn started(&self, mode: AudioMode, ice_ms: Option<u32>) {
        match mode {
            AudioMode::Listen => &self.started_listen,
            AudioMode::Talk => &self.started_talk,
        }
        .fetch_add(1, Ordering::Relaxed);
        if let Some(value) = ice_ms {
            self.ice.observe(u64::from(value));
        }
    }

    pub fn ended(&self, reason: SessionEndReason, duration_ms: u64) {
        self.active.fetch_sub(1, Ordering::Relaxed);
        self.ended[reason as usize].fetch_add(1, Ordering::Relaxed);
        self.duration.observe(duration_ms);
    }

    #[must_use]
    pub fn render(&self) -> String {
        let mut output = String::with_capacity(2_048);
        metric_header(
            &mut output,
            "vistoda_ring_audio_sessions_active",
            "Current Ring audio sessions including negotiation",
            "gauge",
        );
        let _ = writeln!(
            output,
            "vistoda_ring_audio_sessions_active {}",
            self.active.load(Ordering::Relaxed)
        );
        metric_header(
            &mut output,
            "vistoda_ring_audio_sessions_started_total",
            "Successfully negotiated Ring audio sessions",
            "counter",
        );
        let _ = writeln!(
            output,
            "vistoda_ring_audio_sessions_started_total{{mode=\"listen\"}} {}",
            self.started_listen.load(Ordering::Relaxed)
        );
        let _ = writeln!(
            output,
            "vistoda_ring_audio_sessions_started_total{{mode=\"talk\"}} {}",
            self.started_talk.load(Ordering::Relaxed)
        );
        metric_header(
            &mut output,
            "vistoda_ring_audio_sessions_ended_total",
            "Ring audio session terminations by bounded reason",
            "counter",
        );
        for reason in SessionEndReason::ALL {
            let _ = writeln!(
                output,
                "vistoda_ring_audio_sessions_ended_total{{reason=\"{}\"}} {}",
                reason.as_str(),
                self.ended[reason as usize].load(Ordering::Relaxed)
            );
        }
        self.ice.render(
            &mut output,
            "vistoda_ring_audio_ice_gathering_seconds",
            "Browser ICE gathering duration for successful Ring sessions",
        );
        self.duration.render(
            &mut output,
            "vistoda_ring_audio_session_duration_seconds",
            "Ring audio session duration including negotiation",
        );
        output
    }
}

fn metric_header(output: &mut String, name: &str, help: &str, kind: &str) {
    let _ = writeln!(output, "# HELP {name} {help}");
    let _ = writeln!(output, "# TYPE {name} {kind}");
}

fn seconds(milliseconds: u64) -> String {
    format!("{}.{:03}", milliseconds / 1_000, milliseconds % 1_000)
}

#[cfg(test)]
mod tests {
    use super::RingMetrics;
    use crate::ring_audio::{AudioMode, SessionEndReason};

    #[test]
    fn prometheus_output_is_aggregate_and_bounded() {
        let metrics = RingMetrics::default();
        metrics.reserved();
        metrics.started(AudioMode::Listen, Some(253));
        metrics.ended(SessionEndReason::UserStop, 1_250);
        let output = metrics.render();
        assert!(output.contains("sessions_active 0"));
        assert!(output.contains("mode=\"listen\"} 1"));
        assert!(output.contains("reason=\"user_stop\"} 1"));
        assert!(output.contains("ice_gathering_seconds_count 1"));
        assert!(!output.contains("session_id"));
    }
}
