use std::{path::PathBuf, sync::Arc};

use tokio::sync::OnceCell;

use crate::{error::BridgeError, ring_client::RingClient};

pub struct RingProvider {
    session_file: PathBuf,
    client: OnceCell<Arc<RingClient>>,
}

impl RingProvider {
    pub const fn new(session_file: PathBuf) -> Self {
        Self {
            session_file,
            client: OnceCell::const_new(),
        }
    }

    pub async fn client(&self) -> Result<Arc<RingClient>, BridgeError> {
        self.client
            .get_or_try_init(|| async { RingClient::new(self.session_file.clone()).map(Arc::new) })
            .await
            .cloned()
    }
}
