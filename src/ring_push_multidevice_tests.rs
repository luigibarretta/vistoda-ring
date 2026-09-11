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
