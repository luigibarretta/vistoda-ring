use zeroize::Zeroizing;

use crate::error::BridgeError;

pub async fn checked_body(
    response: reqwest::Response,
    operation: &'static str,
    limit: usize,
) -> Result<Zeroizing<Vec<u8>>, BridgeError> {
    if !response.status().is_success() {
        return Err(BridgeError::VendorRejected {
            operation,
            status: response.status().as_u16(),
        });
    }
    read_bounded(response, operation, limit).await
}

/// Like `checked_body`, but a rejected refresh grant (HTTP 401, or 400 with
/// OAuth `invalid_grant`) becomes `ReauthRequired`. Server errors and other
/// rejections keep their transient classification.
pub async fn checked_refresh_body(
    response: reqwest::Response,
    operation: &'static str,
    limit: usize,
) -> Result<Zeroizing<Vec<u8>>, BridgeError> {
    match response.status() {
        reqwest::StatusCode::UNAUTHORIZED => Err(BridgeError::ReauthRequired),
        reqwest::StatusCode::BAD_REQUEST => {
            let body = read_bounded(response, operation, limit).await;
            if body.is_ok_and(|body| is_invalid_grant(&body)) {
                return Err(BridgeError::ReauthRequired);
            }
            Err(BridgeError::VendorRejected {
                operation,
                status: 400,
            })
        }
        _ => checked_body(response, operation, limit).await,
    }
}

/// RFC 6749 error code of a rejected refresh token; the body is never logged.
pub fn is_invalid_grant(body: &[u8]) -> bool {
    serde_json::from_slice::<serde_json::Value>(body).is_ok_and(|value| {
        value
            .get("error")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|code| code.eq_ignore_ascii_case("invalid_grant"))
    })
}

async fn read_bounded(
    mut response: reqwest::Response,
    operation: &'static str,
    limit: usize,
) -> Result<Zeroizing<Vec<u8>>, BridgeError> {
    let mut body = Zeroizing::new(Vec::new());
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| BridgeError::Transport(operation, error))?
    {
        append_bounded(&mut body, &chunk, operation, limit)?;
    }
    Ok(body)
}

fn append_bounded(
    body: &mut Vec<u8>,
    chunk: &[u8],
    operation: &'static str,
    limit: usize,
) -> Result<(), BridgeError> {
    if body.len().saturating_add(chunk.len()) > limit {
        return Err(BridgeError::Protocol(format!(
            "{operation} response exceeds its limit"
        )));
    }
    body.extend_from_slice(chunk);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{append_bounded, is_invalid_grant};

    #[test]
    fn response_limit_is_enforced_before_append() {
        let mut body = vec![1; 4];
        assert!(append_bounded(&mut body, &[2; 5], "test", 8).is_err());
        assert_eq!(body, vec![1; 4]);
    }

    #[test]
    fn only_the_oauth_invalid_grant_code_requires_reauth() {
        assert!(is_invalid_grant(
            br#"{"error":"invalid_grant","error_description":"token is invalid"}"#
        ));
        assert!(is_invalid_grant(br#"{"error":"INVALID_GRANT"}"#));
        assert!(!is_invalid_grant(br#"{"error":"invalid_request"}"#));
        assert!(!is_invalid_grant(
            br#"{"error_description":"invalid_grant"}"#
        ));
        assert!(!is_invalid_grant(b"invalid_grant"));
    }
}
