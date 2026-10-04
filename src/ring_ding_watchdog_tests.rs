use super::{DEGRADED_WINDOW, DingHealth, DingWatchdog, GRACE, HistoryDing, is_ding_event};

fn ding(id: &str, at: i64) -> HistoryDing {
    HistoryDing {
        event_id: id.into(),
        occurred_at: at,
    }
}

#[test]
fn classifier_accepts_ding_forms_case_insensitively() {
    for label in [
        "ding",
        "DING",
        "Ding",
        "Intercom.Ding",
        "intercom_ding",
        "Doorbell.DING",
    ] {
        assert!(is_ding_event(Some(label), None), "{label}");
    }
    assert!(is_ding_event(None, Some("Ding")));
    assert!(is_ding_event(Some("Door.Unlock"), Some("ding")));
    for label in [
        "Door.Unlock",
        "Reading",
        "Intercom.Dinged",
        "Ding.Answered",
        ".ding",
        "dingdong",
        "",
    ] {
        assert!(!is_ding_event(Some(label), None), "{label}");
    }
    assert!(!is_ding_event(None, Some("intercom_unlock")));
    assert!(!is_ding_event(None, None));
}

#[test]
fn a_history_ding_with_a_nearby_push_ding_is_not_missed() {
    let mut watchdog = DingWatchdog::default();
    watchdog.record_push(1_060);
    assert_eq!(watchdog.observe(&[ding("a", 1_000)], 1_010), 0);
    // Judged only after the grace period, and only once.
    assert_eq!(watchdog.observe(&[ding("a", 1_000)], 1_000 + GRACE), 0);
    assert_eq!(watchdog.observe(&[ding("a", 1_000)], 2_000), 0);
    assert_eq!(watchdog.health(2_000), DingHealth::default());
}

#[test]
fn a_silent_push_is_counted_once_and_degrades_until_a_newer_push() {
    let mut watchdog = DingWatchdog::default();
    // A push ding outside the 90-second window does not match.
    watchdog.record_push(1_000 - 91);
    assert_eq!(watchdog.observe(&[ding("a", 1_000)], 1_000 + GRACE - 1), 0);
    assert_eq!(watchdog.observe(&[ding("a", 1_000)], 1_000 + GRACE), 1);
    assert_eq!(watchdog.observe(&[ding("a", 1_000)], 1_200), 0);
    let degraded = DingHealth {
        push_degraded: true,
        last_missed_ding_at: Some(1_000),
    };
    assert_eq!(watchdog.health(1_200), degraded);
    watchdog.record_push(1_500);
    assert_eq!(
        watchdog.health(1_600),
        DingHealth {
            push_degraded: false,
            last_missed_ding_at: Some(1_000),
        }
    );
}

#[test]
fn degradation_expires_after_a_day_and_stale_history_is_ignored() {
    let mut watchdog = DingWatchdog::default();
    assert_eq!(watchdog.observe(&[ding("a", 1_000)], 1_000 + GRACE), 1);
    assert!(watchdog.health(1_000 + DEGRADED_WINDOW).push_degraded);
    assert!(!watchdog.health(1_001 + DEGRADED_WINDOW).push_degraded);
    // A quiet history page keeps showing an old ding after it was forgotten.
    let later = 1_000 + 3 * 60 * 60;
    assert_eq!(watchdog.observe(&[ding("a", 1_000)], later), 0);
    assert_eq!(
        watchdog.observe(&[ding("b", later - 2 * 60 * 60)], later),
        0
    );
}
