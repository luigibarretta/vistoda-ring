use reqwest::Method;

use super::RingClient;
use crate::{
    BridgeError,
    ring_history::{
        RingHistoryIdentity, RingHistoryPage, parse_activity, parse_location, validate_cursor,
    },
};

const HISTORY_BODY_LIMIT: usize = 512 * 1024;
const LOCATION_BODY_LIMIT: usize = 256 * 1024;

impl RingClient {
    pub async fn intercom_inventory(
        &self,
    ) -> Result<crate::ring_inventory::IntercomInventory, BridgeError> {
        let devices = self.discover_intercoms().await?;
        let locations = self
            .vendor_request(
                Method::GET,
                format!("{}/devices/v1/locations", self.endpoints.api_root),
                None,
                Vec::new(),
                "location discovery",
                LOCATION_BODY_LIMIT,
            )
            .await
            .ok();
        crate::ring_inventory::inventory(devices, locations.as_ref().map(|body| body.as_slice()))
    }

    pub async fn history(
        &self,
        limit: u8,
        cursor: Option<&str>,
    ) -> Result<RingHistoryPage, BridgeError> {
        self.history_expected(limit, cursor, None).await
    }

    pub async fn history_expected(
        &self,
        limit: u8,
        cursor: Option<&str>,
        expected: Option<&str>,
    ) -> Result<RingHistoryPage, BridgeError> {
        if !(1..=50).contains(&limit) {
            return Err(BridgeError::InvalidRequest(
                "history limit must be between 1 and 50".into(),
            ));
        }
        if let Some(value) = cursor {
            validate_cursor(value)?;
        }
        let device = self.verified_device(expected).await?;
        self.history_for_device(&device, limit, cursor).await
    }

    pub(crate) async fn history_for_device(
        &self,
        device: &crate::ring_wire::RingIntercomIdentity,
        limit: u8,
        cursor: Option<&str>,
    ) -> Result<RingHistoryPage, BridgeError> {
        let location_id = device
            .location_id()
            .filter(|value| valid_provider_id(value))
            .ok_or_else(|| BridgeError::Protocol("Ring location is unavailable".into()))?;
        let locations = self
            .vendor_request(
                Method::GET,
                format!("{}/devices/v1/locations", self.endpoints.api_root),
                None,
                Vec::new(),
                "location discovery",
                LOCATION_BODY_LIMIT,
            )
            .await?;
        let (location_name, city) = parse_location(&locations, location_id)?;
        let (events, next_cursor) = self.activity_for_device(device, limit, cursor).await?;
        Ok(RingHistoryPage {
            identity: RingHistoryIdentity {
                device_id: device.id().to_string(),
                device_name: device.description().to_owned(),
                location_name,
                city,
            },
            events,
            next_cursor,
        })
    }

    pub(crate) async fn activity_for_device(
        &self,
        device: &crate::ring_wire::RingIntercomIdentity,
        limit: u8,
        cursor: Option<&str>,
    ) -> Result<(Vec<crate::ring_history::RingHistoryEvent>, Option<String>), BridgeError> {
        let mut query = vec![("limit".into(), limit.to_string())];
        if let Some(value) = cursor {
            query.push(("older_than".into(), value.into()));
        }
        let activity = self
            .vendor_request(
                Method::GET,
                format!(
                    "{}/doorbots/{}/history",
                    self.endpoints.client_api,
                    device.id()
                ),
                None,
                query,
                "activity history",
                HISTORY_BODY_LIMIT,
            )
            .await?;
        parse_activity(&activity, limit)
    }
}

fn valid_provider_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}
