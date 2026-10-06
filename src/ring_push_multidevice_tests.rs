use super::RingPushService;
use std::{path::PathBuf, time::Duration};

#[tokio::test]
async fn events_never_cross_intercom_queues() {
    let service = RingPushService::new(
        PathBuf::from("unused-test-path"),
        [Some(42), Some(43)].into_iter(),
    );
    let payload = br#"{"data":{"data":"{\"gcmData\":{\"action\":\"com.ring.push.INTERCOM_UNLOCK_FROM_APP\",\"alarm_meta\":{\"device_zid\":\"43\"}}}"}}"#;
    service
        .handle_message(&["42".into(), "43".into()], payload)
        .await;
    let first = service
        .events(Some(42))
        .unwrap_or_else(|error| panic!("{error}"));
    let second = service
        .events(Some(43))
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(first.wait_after(0, Duration::ZERO).await.is_empty());
    assert_eq!(second.wait_after(0, Duration::ZERO).await.len(), 1);
    assert!(service.events(None).is_err());
    service.handle_message(&["42".into()], payload).await;
    assert_eq!(second.wait_after(0, Duration::ZERO).await.len(), 1);
}

#[tokio::test]
async fn missed_history_dings_degrade_only_their_intercom_and_are_never_published() {
    let service = RingPushService::new(
        PathBuf::from("unused-test-path"),
        [Some(42), Some(43)].into_iter(),
    );
    let devices = ["42".to_owned(), "43".to_owned()];
    let ding = |id: &str, at| crate::ring_ding_watchdog::HistoryDing {
        event_id: id.into(),
        occurred_at: at,
    };
    // A ding before the watcher start is not judged.
    service.watch_dings(&devices, "43", 1_000, &[ding("old", 990)], 1_500);
    // First read, then judged once the short grace has passed.
    service.watch_dings(&devices, "43", 1_000, &[ding("new", 1_100)], 1_480);
    service.watch_dings(&devices, "43", 1_000, &[ding("new", 1_100)], 1_500);
    assert!(
        service
            .metrics()
            .contains("vistoda_ring_push_missed_dings_total 1\n")
    );
    let first = service
        .events(Some(42))
        .unwrap_or_else(|error| panic!("{error}"));
    let second = service
        .events(Some(43))
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(!first.ding_health(1_500).push_degraded);
    let health = second.ding_health(1_500);
    assert!(health.push_degraded);
    assert_eq!(health.last_missed_ding_at, Some(1_100));
    assert!(second.wait_after(0, Duration::ZERO).await.is_empty());
    service.reauth_state(true);
    service.reauth_state(true);
    assert!(
        service
            .metrics()
            .contains("vistoda_ring_reauth_required 1\n")
    );
    service.reauth_state(false);
    assert!(
        service
            .metrics()
            .contains("vistoda_ring_reauth_required 0\n")
    );
}
