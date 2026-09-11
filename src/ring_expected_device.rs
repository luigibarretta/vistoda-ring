//! Parse lossless caller pins and resolve immutable runtime bindings.
use crate::{BridgeError, ring_device_runtime::RingDeviceRuntime};
use std::sync::Arc;

impl crate::Runtime {
    pub(crate) fn audio_target(
        &self,
        alias: &str,
        expected: Option<&str>,
    ) -> Result<(Arc<RingDeviceRuntime>, u64), BridgeError> {
        let route = self.device(alias)?;
        let id = route.expected_id(expected)?;
        if route.device_id.is_some() {
            return Ok((route, id));
        }
        drop(route);
        let devices = self
            .device_runtimes
            .read()
            .map_err(|_| BridgeError::UpstreamUnavailable)?;
        let target = devices
            .values()
            .find(|target| target.device_id == Some(id))
            .cloned()
            .ok_or(BridgeError::DeviceNotFound)?;
        drop(devices);
        Ok((target, id))
    }
}

pub fn parse_id(value: &str) -> Result<u64, BridgeError> {
    if value.is_empty() || value.len() > 20 || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(mismatch());
    }
    value
        .parse::<u64>()
        .ok()
        .filter(|id| *id > 0 && id.to_string() == value)
        .ok_or_else(mismatch)
}

#[cfg(test)]
#[path = "ring_expected_device_tests.rs"]
mod tests;

pub fn mismatch() -> BridgeError {
    BridgeError::InvalidRequest("Ring physical device binding changed or is missing".into())
}

impl RingDeviceRuntime {
    pub fn expected_id(&self, value: Option<&str>) -> Result<u64, BridgeError> {
        let expected = value.map(parse_id).transpose()?;
        match (self.device_id, expected) {
            (Some(actual), Some(expected)) if actual != expected => Err(mismatch()),
            (Some(actual), _) => Ok(actual),
            (None, Some(expected)) => Ok(expected),
            (None, None) => Err(mismatch()),
        }
    }
}
