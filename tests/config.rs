use std::collections::BTreeMap;

use ring_intercom_bridge::{
    BridgeConfig,
    model::{DeviceConfig, DeviceKind},
    ring_recording::RecordingStorageKind,
};

const TOKEN: &[u8] = b"01234567890123456789012345678901";

#[test]
fn traversal_like_aliases_are_rejected() {
    let devices = BTreeMap::from([(
        "../entrance".into(),
        DeviceConfig {
            kind: DeviceKind::RingIntercomAudio,
        },
    )]);
    assert!(BridgeConfig::new("127.0.0.1".into(), 8775, TOKEN.to_vec(), devices).is_err());
}

#[test]
fn recording_display_path_is_bounded_and_absolute() {
    let devices = BTreeMap::from([(
        "entrance".into(),
        DeviceConfig {
            kind: DeviceKind::RingIntercomAudio,
        },
    )]);
    let config = BridgeConfig::new("127.0.0.1".into(), 8775, TOKEN.to_vec(), devices)
        .unwrap_or_else(|error| panic!("config failed: {error}"));
    assert!(
        config
            .clone()
            .with_recording_display("relative".into(), RecordingStorageKind::Custom)
            .is_err()
    );
    assert!(
        config
            .with_recording_display("/media/vistoda-ring".into(), RecordingStorageKind::Media)
            .is_ok()
    );
}

#[test]
fn unsupported_video_devices_are_rejected_during_research() {
    let devices = BTreeMap::from([(
        "entrance".into(),
        DeviceConfig {
            kind: DeviceKind::RingIntercomVideo,
        },
    )]);
    assert!(BridgeConfig::new("127.0.0.1".into(), 8775, TOKEN.to_vec(), devices).is_err());
}

#[test]
fn short_api_tokens_are_rejected() {
    let devices = BTreeMap::from([(
        "entrance".into(),
        DeviceConfig {
            kind: DeviceKind::RingIntercomAudio,
        },
    )]);
    assert!(BridgeConfig::new("127.0.0.1".into(), 8775, b"short".to_vec(), devices).is_err());
}
