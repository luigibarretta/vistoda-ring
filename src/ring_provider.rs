use std::{path::PathBuf, sync::Arc};

use tokio::sync::{Mutex, OwnedRwLockWriteGuard, RwLock};

use crate::{error::BridgeError, ring_client::RingClient};

pub struct RingProvider {
    session_file: PathBuf,
    client: Arc<Mutex<Option<Arc<RingClient>>>>,
    device_id: Option<u64>,
    lifecycle: Arc<RwLock<()>>,
}

impl RingProvider {
    pub fn new(session_file: PathBuf) -> Self {
        Self {
            session_file,
            client: Arc::new(Mutex::new(None)),
            device_id: None,
            lifecycle: Arc::new(RwLock::new(())),
        }
    }

    pub fn scoped(&self, device_id: Option<u64>) -> Self {
        Self {
            session_file: self.session_file.clone(),
            client: Arc::clone(&self.client),
            device_id,
            lifecycle: Arc::clone(&self.lifecycle),
        }
    }

    pub async fn client(&self) -> Result<Arc<RingClient>, BridgeError> {
        let guard = Arc::clone(&self.lifecycle).read_owned().await;
        let mut cached = self.client.lock().await;
        if cached.is_none() {
            *cached = Some(Arc::new(RingClient::new(self.session_file.clone())?));
        }
        let client = cached.as_ref().ok_or(BridgeError::UpstreamUnavailable)?;
        Ok(Arc::new(
            client.scoped(self.device_id).with_lifecycle_guard(guard),
        ))
    }

    pub async fn enrollment_guard(&self) -> OwnedRwLockWriteGuard<()> {
        Arc::clone(&self.lifecycle).write_owned().await
    }

    pub async fn reset(&self) {
        self.client.lock().await.take();
    }

    #[cfg(test)]
    pub async fn seed_client(&self, client: RingClient) {
        *self.client.lock().await = Some(Arc::new(client));
    }
}
