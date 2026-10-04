use std::time::Duration;

pub fn fcm_client() -> Result<reqwest::Client, crate::ring_push_worker::PushError> {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|error| {
            crate::ring_push_worker::PushError::Fcm("HTTP client setup", error.to_string())
        })
}

pub fn unix_timestamp() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}
