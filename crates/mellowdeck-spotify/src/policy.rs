use std::time::Duration;

use mellowdeck_core::{AppError, ErrorKind};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HttpMethod {
    Get,
    Put,
    Delete,
    Post,
}

impl HttpMethod {
    pub fn is_idempotent(self) -> bool {
        matches!(self, Self::Get | Self::Put | Self::Delete)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RetryPolicy {
    pub maximum_attempts: u8,
    pub base_delay: Duration,
    pub maximum_delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            maximum_attempts: 4,
            base_delay: Duration::from_millis(250),
            maximum_delay: Duration::from_secs(4),
        }
    }
}

impl RetryPolicy {
    pub fn delay_for(self, attempt: u8, jitter_millis: u64) -> Option<Duration> {
        if attempt == 0 || attempt >= self.maximum_attempts {
            return None;
        }
        let factor = 1_u32.checked_shl(u32::from(attempt - 1)).unwrap_or(u32::MAX);
        Some(
            self.base_delay
                .saturating_mul(factor)
                .min(self.maximum_delay)
                .saturating_add(Duration::from_millis(jitter_millis.min(200))),
        )
    }

    pub fn should_retry(self, method: HttpMethod, status: Option<u16>, attempt: u8) -> bool {
        attempt < self.maximum_attempts
            && method.is_idempotent()
            && status.is_none_or(|code| (500..=599).contains(&code))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResponseAction {
    Accept,
    RefreshAccessTokenOnce,
    Wait(Duration),
    Fail(AppError),
}

pub fn classify_response(
    status: u16,
    retry_after: Option<Duration>,
    already_refreshed: bool,
) -> ResponseAction {
    match status {
        200..=299 => ResponseAction::Accept,
        401 if !already_refreshed => ResponseAction::RefreshAccessTokenOnce,
        401 => ResponseAction::Fail(AppError::new(
            ErrorKind::Authentication,
            "Spotify authorization expired",
        )),
        403 => ResponseAction::Fail(AppError::new(
            ErrorKind::Restricted,
            "Spotify does not permit this action",
        )),
        404 => ResponseAction::Fail(AppError::new(
            ErrorKind::NotFound,
            "Spotify resource was not found",
        )),
        429 => ResponseAction::Wait(retry_after.unwrap_or(Duration::from_secs(1))),
        500..=599 => ResponseAction::Fail(AppError::new(
            ErrorKind::Unavailable,
            "Spotify is temporarily unavailable",
        )),
        _ => ResponseAction::Fail(AppError::new(
            ErrorKind::Unexpected,
            format!("Spotify returned HTTP {status}"),
        )),
    }
}

pub fn redact_log_value(value: &str) -> String {
    const SENSITIVE_KEYS: [&str; 7] = [
        "authorization",
        "access_token",
        "refresh_token",
        "client_secret",
        "code=",
        "state=",
        "verifier",
    ];
    let lowercase = value.to_ascii_lowercase();
    if SENSITIVE_KEYS.iter().any(|key| lowercase.contains(key)) {
        "[REDACTED]".to_owned()
    } else {
        value.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mutations_are_not_blindly_retried() {
        let policy = RetryPolicy::default();
        assert!(policy.should_retry(HttpMethod::Get, Some(503), 1));
        assert!(!policy.should_retry(HttpMethod::Post, Some(503), 1));
        assert!(!policy.should_retry(HttpMethod::Get, Some(400), 1));
    }

    #[test]
    fn rate_limit_respects_retry_after() {
        assert_eq!(
            classify_response(429, Some(Duration::from_secs(9)), false),
            ResponseAction::Wait(Duration::from_secs(9))
        );
    }

    #[test]
    fn sensitive_diagnostics_are_redacted() {
        assert_eq!(redact_log_value("Authorization: Bearer secret"), "[REDACTED]");
        assert_eq!(redact_log_value("request completed"), "request completed");
    }
}
