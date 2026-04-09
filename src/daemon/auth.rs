use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use tracing::warn;

/// Axum middleware that validates `Authorization: Bearer <token>` header.
/// Expects the expected token to be passed as `State<String>`.
pub async fn require_auth(
    State(expected_token): State<String>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let auth_header = request
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok());

    match auth_header {
        Some(header) if header.starts_with("Bearer ") => {
            let token = &header["Bearer ".len()..];
            if constant_time_eq(token.as_bytes(), expected_token.as_bytes()) {
                Ok(next.run(request).await)
            } else {
                warn!("Auth failed: token mismatch");
                Err(StatusCode::UNAUTHORIZED)
            }
        }
        _ => {
            warn!("Auth failed: missing or malformed Authorization header");
            Err(StatusCode::UNAUTHORIZED)
        }
    }
}

/// Constant-time byte comparison to prevent timing attacks
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b.iter()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::constant_time_eq;

    #[test]
    fn equal_tokens_match() {
        assert!(constant_time_eq(b"abc123", b"abc123"));
    }

    #[test]
    fn different_tokens_dont_match() {
        assert!(!constant_time_eq(b"abc123", b"abc124"));
    }

    #[test]
    fn different_lengths_dont_match() {
        assert!(!constant_time_eq(b"abc", b"abcd"));
        assert!(!constant_time_eq(b"", b"a"));
    }

    #[test]
    fn empty_tokens_match() {
        assert!(constant_time_eq(b"", b""));
    }

    #[test]
    fn all_bytes_checked_not_just_first() {
        // Differs only in the last byte
        assert!(!constant_time_eq(b"aaaaaaaaaa1", b"aaaaaaaaaa2"));
    }
}
