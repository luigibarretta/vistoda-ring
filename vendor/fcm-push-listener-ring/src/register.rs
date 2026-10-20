use crate::{fcm, firebase, gcm, Error};
use serde::Deserialize;
use serde::Serialize;
use uuid::Uuid;

#[derive(Clone, Serialize, Deserialize)]
pub struct Registration {
    pub fcm_token: String,
    pub gcm: gcm::Session,
    pub keys: fcm::WebPushKeys,
}

/// GCM often answers a fresh check-in with a transient `PHONE_REGISTRATION_ERROR`;
/// retry on the same check-in instead of creating a new device every time.
const TOKEN_ATTEMPTS: u32 = 5;

async fn request_token_with_retry(
    http: &reqwest::Client,
    session: &gcm::Session,
    app_id: &str,
) -> Result<String, Error> {
    use rand::Rng;

    let mut attempt = 1;
    loop {
        match session.request_token(http, app_id).await {
            Ok(token) => return Ok(token),
            Err(error) if attempt < TOKEN_ATTEMPTS => {
                log::debug!("GCM registration attempt {attempt} failed: {error}");
                let pause = rand::rng().random_range(1_000..=2_000);
                tokio::time::sleep(std::time::Duration::from_millis(pause)).await;
                attempt += 1;
            }
            Err(error) => return Err(error),
        }
    }
}

pub async fn register(
    http: &reqwest::Client,
    firebase_app_id: &str,
    firebase_project_id: &str,
    firebase_api_key: &str,
    vapid_key: Option<&str>,
) -> Result<Registration, Error> {
    log::debug!("Checking in to GCM");
    let gcm_session = gcm::Session::create(http).await?;

    let id = Uuid::new_v4();
    let gcm_app_id = format!("wp:receiver.push.com#{id}");

    log::debug!("Registering to GCM");
    let gcm_token = request_token_with_retry(http, &gcm_session, &gcm_app_id).await?;

    log::debug!("Getting Firebase installation token");
    let firebase_installation_token = firebase::InstallationAuthToken::request(
        http,
        firebase_app_id,
        firebase_project_id,
        firebase_api_key,
    )
    .await?;

    log::debug!("Calling FCM register");
    let fcm_register_result = fcm::Registration::request(
        http,
        firebase_project_id,
        firebase_api_key,
        vapid_key,
        &firebase_installation_token.value,
        &gcm_token,
    )
    .await?;

    log::debug!("Registration complete");

    Ok(Registration {
        gcm: gcm_session,
        fcm_token: fcm_register_result.fcm_token,
        keys: fcm_register_result.keys,
    })
}
