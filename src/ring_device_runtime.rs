//! Isolate physical intercom media and recordings while sharing the account session.

use crate::{
    BridgeConfig, BridgeError, ring_audio_manager::RingAudioSessions, ring_metrics::RingMetrics,
    ring_provider::RingProvider, ring_recording_manager::RingRecordings,
};
use std::{collections::BTreeMap, sync::Arc};

pub struct RingDeviceRuntime {
    pub device_id: Option<u64>,
    pub provider: Arc<RingProvider>,
    pub audio: RingAudioSessions,
    pub recordings: Arc<RingRecordings>,
    pub recording_display_dir: String,
}

pub fn build_devices(
    config: &BridgeConfig,
    provider: &RingProvider,
    metrics: &Arc<RingMetrics>,
) -> Result<BTreeMap<String, Arc<RingDeviceRuntime>>, BridgeError> {
    config
        .devices
        .iter()
        .map(|(alias, device)| {
            let provider = Arc::new(provider.scoped(device.device_id));
            // Physical IDs keep recordings stable across alias renames and prevent
            // alias reassignment from exposing a different entrance's archive.
            // Unbound legacy single-device installs retain their existing directory.
            let (directory, display) = device.device_id.map_or_else(
                || {
                    (
                        config.recording_dir.clone(),
                        config.recording_display_dir.clone(),
                    )
                },
                |id| {
                    let name = format!("device-{id}");
                    (
                        config.recording_dir.join(&name),
                        format!("{}/{name}", config.recording_display_dir),
                    )
                },
            );
            let audio = RingAudioSessions::production(Arc::clone(&provider), Arc::clone(metrics));
            Ok((
                alias.clone(),
                Arc::new(RingDeviceRuntime {
                    device_id: device.device_id,
                    provider,
                    audio,
                    recordings: RingRecordings::production(directory)?,
                    recording_display_dir: display,
                }),
            ))
        })
        .collect()
}
