//! Materialize immutable physical-ID routes from bounded account discovery.
use crate::{
    BridgeError,
    api::Runtime,
    model::{DeviceConfig, DeviceKind, DeviceSummary, MediaCapabilities},
    ring_device_runtime::build_devices,
    ring_inventory::IntercomInventory,
};
use std::collections::BTreeMap;
use std::{
    sync::{Arc, atomic::Ordering},
    time::Duration,
};

impl Runtime {
    pub(crate) fn start_discovery(self: &Arc<Self>) {
        // A stored notification also covers an enrollment racing worker success.
        // Keep one worker alive between runs instead of racing an AtomicBool reset.
        self.discovery_wakeup.notify_one();
        if self.discovery_started.swap(true, Ordering::AcqRel) {
            return;
        }
        let runtime = Arc::clone(self);
        tokio::spawn(async move {
            loop {
                runtime.discovery_wakeup.notified().await;
                runtime.restore_intercoms(Duration::from_secs(5)).await;
            }
        });
    }

    pub(crate) async fn restore_intercoms(&self, initial_retry: Duration) {
        let mut delay = initial_retry;
        while self.refresh_intercoms().await.is_err() {
            tracing::warn!("Ring route discovery pending; retrying with bounded backoff");
            tokio::time::sleep(delay).await;
            delay = delay.saturating_mul(2).min(Duration::from_mins(5));
        }
    }

    pub(crate) async fn refresh_intercoms(&self) -> Result<IntercomInventory, BridgeError> {
        // Retain the lifecycle read guard through cache publication: a concurrent
        // account replacement cannot publish stale discovery after re-enrollment.
        let client = self.provider.client().await?;
        let mut inventory = client.intercom_inventory().await?;
        self.provision_intercoms(&mut inventory)?;
        drop(client);
        Ok(inventory)
    }

    // `next` is moved into the guarded runtime map, not retained as a local lock.
    #[allow(clippy::significant_drop_tightening)]
    pub(crate) fn provision_intercoms(
        &self,
        inventory: &mut IntercomInventory,
    ) -> Result<(), BridgeError> {
        if inventory.intercoms.len() > crate::ring_inventory::MAX_INTERCOMS {
            return Err(BridgeError::UpstreamUnavailable);
        }
        let mut planned = BTreeMap::new();
        for item in &mut inventory.intercoms {
            let id = item
                .device_id
                .parse::<u64>()
                .map_err(|_| BridgeError::UpstreamUnavailable)?;
            if id == 0 {
                return Err(BridgeError::UpstreamUnavailable);
            }
            let alias = self
                .config
                .devices
                .iter()
                .find(|(_, device)| device.device_id == Some(id))
                .map_or_else(|| format!("intercom-{id}"), |(alias, _)| alias.clone());
            if self
                .config
                .devices
                .get(&alias)
                .is_some_and(|device| device.device_id != Some(id))
            {
                // Never reinterpret a configured alias or guess another target.
                return Err(BridgeError::InvalidRequest(
                    "discovered intercom alias conflicts with configuration".into(),
                ));
            }
            item.alias = Some(alias.clone());
            if planned
                .insert(
                    alias,
                    DeviceConfig {
                        kind: DeviceKind::RingIntercomAudio,
                        device_id: Some(id),
                    },
                )
                .is_some()
            {
                return Err(BridgeError::UpstreamUnavailable);
            }
        }
        // Keep explicitly configured routes (including legacy fail-closed aliases),
        // but prune disappeared auto-discovered devices to bound the cache.
        let mut devices = self
            .device_runtimes
            .write()
            .map_err(|_| BridgeError::UpstreamUnavailable)?;
        let mut next = devices
            .iter()
            .filter(|(alias, _)| self.config.devices.contains_key(*alias))
            .map(|(alias, device)| (alias.clone(), std::sync::Arc::clone(device)))
            .collect::<BTreeMap<_, _>>();
        let mut pending = self.config.clone();
        pending.devices = planned.clone();
        pending
            .devices
            .retain(|alias, _| !devices.contains_key(alias));
        let created = build_devices(&pending, &self.provider, &self.metrics)?;
        for alias in planned.keys() {
            if let Some(device) = devices.get(alias).or_else(|| created.get(alias)) {
                next.insert(alias.clone(), std::sync::Arc::clone(device));
            }
        }
        self.push
            .include_devices(next.values().map(|device| device.device_id))?;
        *devices = next;
        drop(devices);
        Ok(())
    }

    pub(crate) fn device_summaries(&self) -> Result<Vec<DeviceSummary>, BridgeError> {
        let devices = self
            .device_runtimes
            .read()
            .map_err(|_| BridgeError::UpstreamUnavailable)?;
        Ok(devices
            .keys()
            .map(|alias| DeviceSummary {
                alias: alias.clone(),
                kind: DeviceKind::RingIntercomAudio,
                capabilities: MediaCapabilities::verified_audio_recordings(),
            })
            .collect())
    }
}

#[cfg(test)]
#[path = "ring_runtime_inventory_tests.rs"]
mod tests;
