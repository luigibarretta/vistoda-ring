use super::{AudioCallGrant, RingClient};
use crate::{
    BridgeError,
    ring_camera::{self, CameraInventory},
};

impl RingClient {
    pub async fn camera_inventory(&self) -> Result<CameraInventory, BridgeError> {
        let mut inventory = ring_camera::parse(&self.discovery_body().await?)?;
        inventory.cameras.retain(|camera| {
            self.selected_device_id
                .is_none_or(|id| camera.device_id == id.to_string())
        });
        if let Ok(body) = self
            .vendor_request(
                reqwest::Method::GET,
                format!("{}/devices/v1/locations", self.endpoints.api_root),
                None,
                Vec::new(),
                "camera locations",
                256 * 1024,
            )
            .await
        {
            inventory.locations(&body);
        }
        Ok(inventory)
    }

    pub(crate) async fn prepare_camera_call(
        &self,
        expected: &str,
    ) -> Result<AudioCallGrant, BridgeError> {
        let id = crate::ring_expected_device::parse_id(expected)?;
        if self
            .selected_device_id
            .is_some_and(|selected| selected != id)
        {
            return Err(crate::ring_expected_device::mismatch());
        }
        let inventory = ring_camera::parse(&self.discovery_body().await?)?;
        if !inventory
            .cameras
            .iter()
            .any(|camera| camera.device_id == expected)
        {
            return Err(BridgeError::DeviceNotFound);
        }
        self.stream_grant(id).await
    }
}
