use axum::http::{HeaderMap, header};
use subtle::ConstantTimeEq;

use crate::error::BridgeError;

pub fn require_bearer(headers: &HeaderMap, expected: &[u8]) -> Result<(), BridgeError> {
    let supplied = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::as_bytes)
        .ok_or(BridgeError::Unauthorized)?;
    if supplied.len() != expected.len() || supplied.ct_eq(expected).unwrap_u8() != 1 {
        return Err(BridgeError::Unauthorized);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use axum::http::{HeaderMap, HeaderValue, header};

    use super::require_bearer;

    #[test]
    fn bearer_auth_requires_an_exact_token() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer 01234567890123456789012345678901"),
        );
        assert!(require_bearer(&headers, b"01234567890123456789012345678901").is_ok());
        assert!(require_bearer(&headers, b"01234567890123456789012345678902").is_err());
    }
}
