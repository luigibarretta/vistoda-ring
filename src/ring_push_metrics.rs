use std::{
    collections::BTreeMap,
    fmt::Write,
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering},
    },
};

use crate::ring_push_event::RingPushEventKind;

#[derive(Default)]
pub struct RingPushMetrics {
    connected: AtomicBool,
    reauth_required: AtomicBool,
    unlock_history_errors: AtomicU64,
    missed_dings: AtomicU64,
    registrations: AtomicU64,
    reconnects: AtomicU64,
    rotations: AtomicU64,
    state_resets: AtomicU64,
    legacy_record: AtomicBool,
    /// Failures per stage; stages are static strings, so the label set is bounded.
    errors: Mutex<BTreeMap<&'static str, u64>>,
    ding: AtomicU64,
    unlock: AtomicU64,
    ignored: AtomicU64,
    undecryptable: AtomicU64,
    last_event: AtomicI64,
    last_heartbeat: AtomicI64,
}

impl RingPushMetrics {
    pub fn set_connected(&self, value: bool) {
        self.connected.store(value, Ordering::Relaxed);
    }

    pub fn connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    /// Returns whether the state changed, so callers log it once.
    pub fn set_reauth_required(&self, value: bool) -> bool {
        self.reauth_required.swap(value, Ordering::Relaxed) != value
    }

    pub fn reauth_required(&self) -> bool {
        self.reauth_required.load(Ordering::Relaxed)
    }

    pub fn missed_dings(&self, count: u64) {
        self.missed_dings.fetch_add(count, Ordering::Relaxed);
    }

    pub fn unlock_history_failed(&self) {
        self.unlock_history_errors.fetch_add(1, Ordering::Relaxed);
    }

    pub fn registered(&self) {
        self.registrations.fetch_add(1, Ordering::Relaxed);
    }

    pub fn reconnected(&self) {
        self.reconnects.fetch_add(1, Ordering::Relaxed);
    }

    pub fn failed(&self, stage: &'static str) {
        if let Ok(mut errors) = self.errors.lock() {
            *errors.entry(stage).or_default() += 1;
        }
    }

    pub fn rotated(&self) {
        self.rotations.fetch_add(1, Ordering::Relaxed);
    }

    /// The stored registration was unreadable and has been moved aside.
    pub fn state_reset(&self) {
        self.state_resets.fetch_add(1, Ordering::Relaxed);
    }

    pub fn state_was_reset(&self) -> bool {
        self.state_resets.load(Ordering::Relaxed) > 0
    }

    /// Ring refused the app-format device record and holds the legacy one.
    pub fn set_legacy_record(&self, value: bool) {
        self.legacy_record.store(value, Ordering::Relaxed);
    }

    /// A server heartbeat ping or an acknowledgement of ours arrived.
    pub fn heartbeat(&self, at: i64) {
        self.last_heartbeat.store(at, Ordering::Relaxed);
    }

    pub fn ignored(&self) {
        self.ignored.fetch_add(1, Ordering::Relaxed);
    }

    pub fn undecryptable(&self) {
        self.undecryptable.fetch_add(1, Ordering::Relaxed);
    }

    pub fn received(&self, kind: RingPushEventKind, occurred_at: i64) {
        match kind {
            RingPushEventKind::Ding => &self.ding,
            RingPushEventKind::IntercomUnlock => &self.unlock,
        }
        .fetch_add(1, Ordering::Relaxed);
        self.last_event.store(occurred_at, Ordering::Relaxed);
    }

    pub fn render(&self) -> String {
        let mut output = String::with_capacity(1_024);
        gauge(
            &mut output,
            "vistoda_ring_push_connected",
            u64::from(self.connected()),
        );
        gauge(
            &mut output,
            "vistoda_ring_reauth_required",
            u64::from(self.reauth_required()),
        );
        counter(
            &mut output,
            "vistoda_ring_push_registrations_total",
            self.registrations.load(Ordering::Relaxed),
        );
        counter(
            &mut output,
            "vistoda_ring_push_reconnects_total",
            self.reconnects.load(Ordering::Relaxed),
        );
        counter(
            &mut output,
            "vistoda_ring_push_rotations_total",
            self.rotations.load(Ordering::Relaxed),
        );
        counter(
            &mut output,
            "vistoda_ring_push_state_resets_total",
            self.state_resets.load(Ordering::Relaxed),
        );
        gauge(
            &mut output,
            "vistoda_ring_push_legacy_record",
            u64::from(self.legacy_record.load(Ordering::Relaxed)),
        );
        self.render_errors(&mut output);
        let _ = writeln!(output, "# TYPE vistoda_ring_push_events_total counter");
        let _ = writeln!(
            output,
            "vistoda_ring_push_events_total{{event_type=\"ding\"}} {}",
            self.ding.load(Ordering::Relaxed)
        );
        let _ = writeln!(
            output,
            "vistoda_ring_push_events_total{{event_type=\"intercom_unlock\"}} {}",
            self.unlock.load(Ordering::Relaxed)
        );
        counter(
            &mut output,
            "vistoda_ring_push_ignored_total",
            self.ignored.load(Ordering::Relaxed),
        );
        counter(
            &mut output,
            "vistoda_ring_unlock_history_errors_total",
            self.unlock_history_errors.load(Ordering::Relaxed),
        );
        counter(
            &mut output,
            "vistoda_ring_push_missed_dings_total",
            self.missed_dings.load(Ordering::Relaxed),
        );
        counter(
            &mut output,
            "vistoda_ring_push_undecryptable_total",
            self.undecryptable.load(Ordering::Relaxed),
        );
        gauge(
            &mut output,
            "vistoda_ring_push_last_event_timestamp_seconds",
            timestamp(&self.last_event),
        );
        gauge(
            &mut output,
            "vistoda_ring_push_last_heartbeat_timestamp_seconds",
            timestamp(&self.last_heartbeat),
        );
        output
    }

    fn render_errors(&self, output: &mut String) {
        let _ = writeln!(output, "# TYPE vistoda_ring_push_errors_total counter");
        let Ok(errors) = self.errors.lock() else {
            return;
        };
        for (stage, count) in errors.iter() {
            let label: String = stage
                .chars()
                .map(|value| {
                    if value.is_ascii_alphanumeric() {
                        value.to_ascii_lowercase()
                    } else {
                        '_'
                    }
                })
                .collect();
            let _ = writeln!(
                output,
                "vistoda_ring_push_errors_total{{stage=\"{label}\"}} {count}"
            );
        }
    }
}

fn timestamp(value: &AtomicI64) -> u64 {
    u64::try_from(value.load(Ordering::Relaxed).max(0)).unwrap_or_default()
}

fn gauge(output: &mut String, name: &str, value: u64) {
    let _ = writeln!(output, "# TYPE {name} gauge\n{name} {value}");
}

fn counter(output: &mut String, name: &str, value: u64) {
    let _ = writeln!(output, "# TYPE {name} counter\n{name} {value}");
}
