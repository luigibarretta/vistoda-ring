use std::{collections::BTreeMap, path::PathBuf, sync::Arc, time::Duration};

use tokio::sync::Mutex;
use uuid::Uuid;

use crate::{
    error::BridgeError,
    ring_client::RingReadOnlyClient,
    ring_recording::{
        RecordingImport, RecordingImportRequest, RecordingImportState, RecordingItem,
    },
    ring_recording_store::RecordingStore,
};

const FIRST_POLL_DELAY: Duration = Duration::from_secs(15);
const POLL_INTERVAL: Duration = Duration::from_secs(5);
const POLL_ATTEMPTS: usize = 36;

pub struct RingRecordings {
    session_file: PathBuf,
    store: RecordingStore,
    jobs: Mutex<BTreeMap<Uuid, RecordingImport>>,
    active: Mutex<bool>,
}

impl RingRecordings {
    pub fn production(session_file: PathBuf, root: PathBuf) -> Result<Arc<Self>, BridgeError> {
        Ok(Arc::new(Self {
            session_file,
            store: RecordingStore::new(root)?,
            jobs: Mutex::new(BTreeMap::new()),
            active: Mutex::new(false),
        }))
    }

    pub async fn start(
        self: &Arc<Self>,
        request: RecordingImportRequest,
    ) -> Result<RecordingImport, BridgeError> {
        validate_trigger(request.triggered_at)?;
        if let Some(recording) = self.store.find_trigger(request.triggered_at)? {
            return Ok(completed_import(
                request.triggered_at,
                recording.recording_id,
            ));
        }
        let duplicate = self
            .jobs
            .lock()
            .await
            .values()
            .find(|job| job.triggered_at.abs_diff(request.triggered_at) <= 5)
            .cloned();
        if let Some(job) = duplicate {
            return Ok(job);
        }
        let mut active = self.active.lock().await;
        if *active {
            return Err(BridgeError::RecordingBusy);
        }
        *active = true;
        drop(active);
        let job = RecordingImport {
            import_id: Uuid::new_v4(),
            triggered_at: request.triggered_at,
            state: RecordingImportState::Pending,
            recording_id: None,
        };
        self.jobs.lock().await.insert(job.import_id, job.clone());
        let manager = Arc::clone(self);
        let task_job = job.clone();
        tokio::spawn(async move { manager.run(task_job).await });
        Ok(job)
    }

    pub async fn status(&self, id: Uuid) -> Result<RecordingImport, BridgeError> {
        self.jobs
            .lock()
            .await
            .get(&id)
            .cloned()
            .ok_or(BridgeError::RecordingNotFound)
    }

    pub fn list(&self) -> Result<Vec<RecordingItem>, BridgeError> {
        self.store.list()
    }

    pub fn media(&self, id: Uuid) -> Result<Vec<u8>, BridgeError> {
        self.store.read(id)
    }

    pub fn delete(&self, id: Uuid) -> Result<(), BridgeError> {
        self.store.delete(id)
    }

    async fn run(self: Arc<Self>, job: RecordingImport) {
        tokio::time::sleep(FIRST_POLL_DELAY).await;
        let result = self.poll_and_commit(&job).await;
        let (state, recording_id) = match result {
            Ok(Some(recording)) => (RecordingImportState::Complete, Some(recording.recording_id)),
            Ok(None) => (RecordingImportState::Expired, None),
            Err(BridgeError::RecordingUnavailable) => (RecordingImportState::Unavailable, None),
            Err(error) => {
                tracing::warn!(error = %error, "Ring recording import failed");
                (RecordingImportState::Failed, None)
            }
        };
        if let Some(current) = self.jobs.lock().await.get_mut(&job.import_id) {
            current.state = state;
            current.recording_id = recording_id;
        }
        *self.active.lock().await = false;
    }

    async fn poll_and_commit(
        &self,
        job: &RecordingImport,
    ) -> Result<Option<RecordingItem>, BridgeError> {
        let client = RingReadOnlyClient::new(self.session_file.clone())?;
        for attempt in 0..POLL_ATTEMPTS {
            if let Some(source) = client.find_recording_since(job.triggered_at).await? {
                let media = client.download_recording(&source).await?;
                return self
                    .store
                    .commit(job.triggered_at, source.created_at, &media)
                    .map(Some);
            }
            if attempt + 1 < POLL_ATTEMPTS {
                tokio::time::sleep(POLL_INTERVAL).await;
            }
        }
        Ok(None)
    }
}

fn validate_trigger(triggered_at: i64) -> Result<(), BridgeError> {
    let current = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| BridgeError::InvalidRequest("system clock is invalid".into()))?
        .as_secs();
    let current = i64::try_from(current).unwrap_or(i64::MAX);
    if triggered_at < current.saturating_sub(3600) || triggered_at > current.saturating_add(60) {
        return Err(BridgeError::InvalidRequest(
            "triggered_at must be within the last hour".into(),
        ));
    }
    Ok(())
}

fn completed_import(triggered_at: i64, recording_id: Uuid) -> RecordingImport {
    RecordingImport {
        import_id: Uuid::new_v4(),
        triggered_at,
        state: RecordingImportState::Complete,
        recording_id: Some(recording_id),
    }
}
