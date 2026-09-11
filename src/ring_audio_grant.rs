use super::{
    AUTH_BODY_LIMIT, AudioCallGrant, BridgeError, RingClient, USER_AGENT, access_value,
    checked_body,
};

impl RingClient {
    pub async fn prepare_audio_call(&self) -> Result<AudioCallGrant, BridgeError> {
        self.prepare_audio_call_expected(None).await
    }

    pub async fn prepare_audio_call_expected(
        &self,
        expected: Option<&str>,
    ) -> Result<AudioCallGrant, BridgeError> {
        let device_id = self.verified_device(expected).await?.id();
        let mut state = self.state.lock().await;
        self.ensure_authenticated(&mut state).await?;
        self.ensure_registered(&mut state).await?;
        let response = self
            .http
            .post(&self.endpoints.stream_ticket)
            .bearer_auth(access_value(&state)?)
            .header("hardware_id", state.session.hardware_id().to_string())
            .header(reqwest::header::USER_AGENT, USER_AGENT)
            .send()
            .await
            .map_err(|error| BridgeError::Transport("stream ticket", error))?;
        drop(state);
        let body = checked_body(response, "stream ticket", AUTH_BODY_LIMIT).await?;
        Ok(AudioCallGrant {
            device_id,
            ticket: crate::ring_wire::parse_ticket(&body)?,
        })
    }

    pub(crate) async fn verified_device(
        &self,
        expected: Option<&str>,
    ) -> Result<super::RingIntercomIdentity, BridgeError> {
        let expected = expected
            .map(crate::ring_expected_device::parse_id)
            .transpose()?;
        if self
            .selected_device_id
            .zip(expected)
            .is_some_and(|(actual, expected)| actual != expected)
        {
            return Err(crate::ring_expected_device::mismatch());
        }
        let device = super::controls::only_device(self.discover_intercoms().await?)?;
        if expected.is_some_and(|expected| expected != device.id()) {
            return Err(crate::ring_expected_device::mismatch());
        }
        Ok(device)
    }
}
