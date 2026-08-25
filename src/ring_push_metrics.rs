use std::{
    fmt::Write,
    sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering},
};

use crate::ring_push_event::RingPushEventKind;

#[derive(Default)]
pub struct RingPushMetrics {
    connected: AtomicBool,
    registrations: AtomicU64,
    reconnects: AtomicU64,
    errors: AtomicU64,
    ding: AtomicU64,
    unlock: AtomicU64,
    ignored: AtomicU64,
    last_event: AtomicI64,
}

impl RingPushMetrics {
    pub fn set_connected(&self, value: bool) {
        self.connected.store(value, Ordering::Relaxed);
    }

    pub fn connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    pub fn registered(&self) {
        self.registrations.fetch_add(1, Ordering::Relaxed);
    }

    pub fn reconnected(&self) {
        self.reconnects.fetch_add(1, Ordering::Relaxed);
    }

    pub fn failed(&self) {
        self.errors.fetch_add(1, Ordering::Relaxed);
    }

    pub fn ignored(&self) {
        self.ignored.fetch_add(1, Ordering::Relaxed);
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
            "vistoda_ring_push_errors_total",
            self.errors.load(Ordering::Relaxed),
        );
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
        gauge(
            &mut output,
            "vistoda_ring_push_last_event_timestamp_seconds",
            u64::try_from(self.last_event.load(Ordering::Relaxed).max(0)).unwrap_or_default(),
        );
        output
    }
}

fn gauge(output: &mut String, name: &str, value: u64) {
    let _ = writeln!(output, "# TYPE {name} gauge\n{name} {value}");
}

fn counter(output: &mut String, name: &str, value: u64) {
    let _ = writeln!(output, "# TYPE {name} counter\n{name} {value}");
}
