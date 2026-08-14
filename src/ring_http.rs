use zeroize::Zeroizing;

use crate::error::BridgeError;

pub async fn checked_body(
    mut response: reqwest::Response,
    operation: &'static str,
    limit: usize,
) -> Result<Zeroizing<Vec<u8>>, BridgeError> {
    if !response.status().is_success() {
        return Err(BridgeError::VendorRejected {
            operation,
            status: response.status().as_u16(),
        });
    }
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
    use super::append_bounded;

    #[test]
    fn response_limit_is_enforced_before_append() {
        let mut body = vec![1; 4];
        assert!(append_bounded(&mut body, &[2; 5], "test", 8).is_err());
        assert_eq!(body, vec![1; 4]);
    }
}
