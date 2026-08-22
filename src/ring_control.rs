use serde::{Deserialize, Serialize};

use crate::BridgeError;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RingDeviceStatus {
    pub battery: Option<u8>,
    pub online: bool,
    pub doorbell_volume: Option<u8>,
    pub mic_volume: Option<u8>,
    pub voice_volume: Option<u8>,
    pub last_activity: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VolumeUpdate {
    pub doorbell_volume: Option<u8>,
    pub mic_volume: Option<u8>,
    pub voice_volume: Option<u8>,
}

impl VolumeUpdate {
    pub(crate) fn validate(&self) -> Result<(), BridgeError> {
        let supplied = [self.doorbell_volume, self.mic_volume, self.voice_volume]
            .into_iter()
            .flatten()
            .count();
        if supplied != 1 {
            return Err(BridgeError::InvalidRequest(
                "exactly one volume is required".into(),
            ));
        }
        if self.doorbell_volume.is_some_and(|value| value > 8)
            || self.mic_volume.is_some_and(|value| value > 11)
            || self.voice_volume.is_some_and(|value| value > 11)
        {
            return Err(BridgeError::InvalidRequest(
                "volume is outside its range".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Deserialize)]
pub(crate) struct UnlockResponse {
    pub result: UnlockResult,
}

#[derive(Deserialize)]
pub(crate) struct UnlockResult {
    pub code: i32,
}

#[cfg(test)]
mod tests {
    use super::VolumeUpdate;

    #[test]
    fn volume_contract_is_non_empty_and_bounded() {
        assert!(
            VolumeUpdate {
                doorbell_volume: None,
                mic_volume: None,
                voice_volume: None,
            }
            .validate()
            .is_err()
        );
        assert!(
            VolumeUpdate {
                doorbell_volume: Some(9),
                mic_volume: None,
                voice_volume: None,
            }
            .validate()
            .is_err()
        );
    }
}
