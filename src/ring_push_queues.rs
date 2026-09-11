//! Short-lived queue map locks never span network or event waits.
use crate::{BridgeError, ring_push_event::RingPushEvents};
use std::{
    collections::BTreeMap,
    sync::{Arc, RwLock},
};

pub struct RingPushQueues(RwLock<BTreeMap<Option<u64>, Arc<RingPushEvents>>>);

impl RingPushQueues {
    pub fn new(devices: impl Iterator<Item = Option<u64>>) -> Self {
        Self(RwLock::new(
            devices
                .map(|id| (id, Arc::new(RingPushEvents::default())))
                .collect(),
        ))
    }

    pub fn include(&self, devices: impl Iterator<Item = Option<u64>>) -> Result<bool, BridgeError> {
        let mut queues = self
            .0
            .write()
            .map_err(|_| BridgeError::UpstreamUnavailable)?;
        let mut next = BTreeMap::new();
        for id in devices {
            next.insert(
                id,
                queues
                    .get(&id)
                    .cloned()
                    .unwrap_or_else(|| Arc::new(RingPushEvents::default())),
            );
        }
        let changed = !queues.keys().eq(next.keys());
        *queues = next;
        drop(queues);
        Ok(changed)
    }

    pub fn get(&self, id: Option<u64>) -> Result<Arc<RingPushEvents>, BridgeError> {
        self.0
            .read()
            .map_err(|_| BridgeError::UpstreamUnavailable)?
            .get(&id)
            .cloned()
            .ok_or(BridgeError::DeviceNotFound)
    }

    pub fn ids(&self) -> Result<Vec<Option<u64>>, BridgeError> {
        let queues = self
            .0
            .read()
            .map_err(|_| BridgeError::UpstreamUnavailable)?;
        // An unbound legacy alias cannot be resolved on a multi-intercom account.
        // Bound subscriptions already feed its queue only when exactly one exists.
        let bound = queues.keys().any(Option::is_some);
        Ok(queues
            .keys()
            .filter(|id| !bound || id.is_some())
            .copied()
            .collect())
    }
}
