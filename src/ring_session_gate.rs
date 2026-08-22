use std::{collections::BTreeMap, sync::Arc, time::Duration};

use tokio::{sync::Mutex, time::Instant};
use uuid::Uuid;

use crate::BridgeError;

const SESSION_COOLDOWN: Duration = Duration::from_secs(10);

#[derive(Clone)]
pub struct SessionPermit {
    pub id: Uuid,
    pub device: String,
}

#[derive(Default)]
struct GateState {
    active: BTreeMap<String, Uuid>,
    cooldowns: BTreeMap<String, Instant>,
}

#[derive(Clone, Default)]
pub struct SessionGate {
    state: Arc<Mutex<GateState>>,
}

impl SessionGate {
    pub async fn reserve(&self, device: String, id: Uuid) -> Result<SessionPermit, BridgeError> {
        let mut state = self.state.lock().await;
        let now = Instant::now();
        state.cooldowns.retain(|_, deadline| *deadline > now);
        if state.active.contains_key(&device) {
            return Err(BridgeError::SessionBusy);
        }
        if state.cooldowns.contains_key(&device) {
            return Err(BridgeError::RateLimited);
        }
        state.active.insert(device.clone(), id);
        drop(state);
        Ok(SessionPermit { id, device })
    }

    pub async fn release(&self, permit: &SessionPermit) {
        let mut state = self.state.lock().await;
        if state.active.get(&permit.device) == Some(&permit.id) {
            state.active.remove(&permit.device);
            state
                .cooldowns
                .insert(permit.device.clone(), Instant::now() + SESSION_COOLDOWN);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SessionGate;
    use uuid::Uuid;

    #[tokio::test]
    async fn reservation_is_exclusive_and_release_starts_cooldown() {
        let gate = SessionGate::default();
        let permit = gate
            .reserve("entrance".into(), Uuid::new_v4())
            .await
            .unwrap_or_else(|error| panic!("reserve failed: {error}"));
        assert!(
            gate.reserve("entrance".into(), Uuid::new_v4())
                .await
                .is_err()
        );
        gate.release(&permit).await;
        gate.release(&permit).await;
        assert!(
            gate.reserve("entrance".into(), Uuid::new_v4())
                .await
                .is_err()
        );
        assert!(gate.reserve("other".into(), Uuid::new_v4()).await.is_ok());
    }
}
