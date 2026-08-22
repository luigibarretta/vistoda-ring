use std::{path::PathBuf, sync::Arc};

use crate::{
    error::BridgeError, ring_recording::RecordingItem, ring_recording_store::RecordingStore,
};

pub struct RingRecordings {
    store: RecordingStore,
}

impl RingRecordings {
    pub fn production(root: PathBuf) -> Result<Arc<Self>, BridgeError> {
        Ok(Arc::new(Self {
            store: RecordingStore::new(root)?,
        }))
    }

    pub fn commit(
        &self,
        started_at: i64,
        ended_at: i64,
        media_type: &str,
        media: &[u8],
    ) -> Result<RecordingItem, BridgeError> {
        self.store.commit(started_at, ended_at, media_type, media)
    }

    pub fn list(&self) -> Result<Vec<RecordingItem>, BridgeError> {
        self.store.list()
    }

    pub fn media(&self, id: uuid::Uuid) -> Result<(String, Vec<u8>), BridgeError> {
        self.store.read(id)
    }

    pub fn delete(&self, id: uuid::Uuid) -> Result<(), BridgeError> {
        self.store.delete(id)
    }
}
