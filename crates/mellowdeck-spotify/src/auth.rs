use std::{fmt, time::Duration};

use mellowdeck_core::{AppError, ErrorKind, Result, UserId};
use reqwest::{StatusCode, blocking::Client};
use serde::Deserialize;

use crate::{
    API_BASE_URL, CALLBACK_URL, CallbackServer, OAuthAttempt, REQUIRED_SCOPES, validate_client_id,
};

const TOKEN_URL: &str = "https://accounts.spotify.com/api/token";
const AUTHORIZATION_TIMEOUT: Duration = Duration::from_secs(180);

#[derive(Clone)]
pub struct TokenSet {
    access_token: String,
    refresh_token: Option<String>,
    pub expires_in: Duration,
    pub scopes: Vec<String>,
}

impl TokenSet {
    pub fn access_token(&self) -> &str {
        &self.access_token
    }

    pub fn refresh_token(&self) -> Option<&str> {
        self.refresh_token.as_deref()
    }
}

impl fmt::Debug for TokenSet {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TokenSet")
            .field("access_token", &"[redacted]")
            .field("refresh_token", &self.refresh_token.as_ref().map(|_| "[redacted]"))
            .field("expires_in", &self.expires_in)
            .field("scopes", &self.scopes)
            .finish()
    }
}

#[derive(Clone, Debug)]
pub struct AuthorizedSession {
    pub user_id: UserId,
    pub display_name: String,
    pub premium: bool,
    pub tokens: TokenSet,
}

#[derive(Debug)]
pub struct SpotifyAuthenticator {
    client: Client,
}

impl SpotifyAuthenticator {
    /// Builds the HTTPS client used for Spotify authorization and profile discovery.
    ///
    /// # Errors
    ///
    /// Returns an error if the TLS-enabled HTTP client cannot be initialized.
    pub fn new() -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(20))
            .user_agent(concat!("Mellowdeck/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(network_error)?;
        Ok(Self { client })
    }

    /// Runs an interactive Authorization Code with PKCE login.
    ///
    /// `open_browser` is supplied by the platform boundary so this adapter does not own desktop UI.
    ///
    /// # Errors
    ///
    /// Returns a typed error if validation, browser launch, callback handling, token exchange, or
    /// profile discovery fails.
    pub fn authorize(
        &self,
        client_id: &str,
        open_browser: impl FnOnce(&str) -> Result<()>,
    ) -> Result<AuthorizedSession> {
        validate_client_id(client_id)?;
        let callback = CallbackServer::bind(AUTHORIZATION_TIMEOUT)?;
        let attempt = OAuthAttempt::generate();
        let url = attempt.authorization_url(client_id, false)?;
        open_browser(url.as_str())?;
        let code = callback.wait_for_code(&attempt)?;
        let token = self.exchange_code(client_id, &code, &attempt.verifier)?;
        self.load_profile(token)
    }

    /// Restores a session from the refresh token held by the credential store.
    ///
    /// # Errors
    ///
    /// Returns a typed error if Spotify rejects the token or profile discovery fails.
    pub fn refresh(&self, client_id: &str, refresh_token: &str) -> Result<AuthorizedSession> {
        validate_client_id(client_id)?;
        if refresh_token.is_empty() {
            return Err(AppError::new(ErrorKind::Authentication, "stored session is empty"));
        }
        let response = self
            .client
            .post(TOKEN_URL)
            .form(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token),
                ("client_id", client_id),
            ])
            .send()
            .map_err(network_error)?;
        let mut token = decode_token(response)?;
        if token.refresh_token.is_none() {
            token.refresh_token = Some(refresh_token.to_owned());
        }
        if !has_required_scopes(&token.scopes) {
            return Err(AppError::new(
                ErrorKind::Authentication,
                "Mellowdeck needs updated Spotify permissions for built-in playback; connect again",
            ));
        }
        self.load_profile(token)
    }

    fn exchange_code(&self, client_id: &str, code: &str, verifier: &str) -> Result<TokenSet> {
        let response = self
            .client
            .post(TOKEN_URL)
            .form(&[
                ("grant_type", "authorization_code"),
                ("code", code),
                ("redirect_uri", CALLBACK_URL),
                ("client_id", client_id),
                ("code_verifier", verifier),
            ])
            .send()
            .map_err(network_error)?;
        decode_token(response)
    }

    fn load_profile(&self, tokens: TokenSet) -> Result<AuthorizedSession> {
        let response = self
            .client
            .get(format!("{API_BASE_URL}/me"))
            .bearer_auth(tokens.access_token())
            .send()
            .map_err(network_error)?;
        let status = response.status();
        if !status.is_success() {
            return Err(status_error(status, "Spotify profile could not be loaded"));
        }
        let profile: ProfileResponse = response.json().map_err(|error| {
            AppError::new(
                ErrorKind::Network,
                format!("Spotify returned an invalid profile: {error}"),
            )
        })?;
        let user_id = UserId::parse(profile.id.clone()).map_err(|error| {
            AppError::new(
                ErrorKind::InvalidInput,
                format!("Spotify returned an invalid account identifier: {error}"),
            )
        })?;
        let display_name = profile
            .display_name
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| profile.id.clone());
        Ok(AuthorizedSession {
            user_id,
            display_name,
            premium: profile.product.as_deref() == Some("premium"),
            tokens,
        })
    }
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: u64,
    #[serde(default)]
    scope: String,
    token_type: String,
}

#[derive(Deserialize)]
struct ProfileResponse {
    id: String,
    display_name: Option<String>,
    product: Option<String>,
}

fn decode_token(response: reqwest::blocking::Response) -> Result<TokenSet> {
    let status = response.status();
    if !status.is_success() {
        return Err(status_error(status, "Spotify did not accept the authorization"));
    }
    let token: TokenResponse = response.json().map_err(|error| {
        AppError::new(
            ErrorKind::Authentication,
            format!("Spotify returned an invalid token response: {error}"),
        )
    })?;
    if !token.token_type.eq_ignore_ascii_case("bearer") || token.access_token.is_empty() {
        return Err(AppError::new(
            ErrorKind::Authentication,
            "Spotify returned an unsupported token",
        ));
    }
    Ok(TokenSet {
        access_token: token.access_token,
        refresh_token: token.refresh_token,
        expires_in: Duration::from_secs(token.expires_in),
        scopes: token.scope.split_ascii_whitespace().map(str::to_owned).collect(),
    })
}

/// Refreshed tokens keep the scopes of the original grant, so sessions created before a scope was
/// added must be authorized again. An empty list means Spotify omitted the field.
fn has_required_scopes(scopes: &[String]) -> bool {
    scopes.is_empty()
        || REQUIRED_SCOPES.iter().all(|required| scopes.iter().any(|scope| scope == required))
}

fn status_error(status: StatusCode, context: &str) -> AppError {
    let kind = match status.as_u16() {
        400 | 401 => ErrorKind::Authentication,
        403 => ErrorKind::Authorization,
        429 => ErrorKind::RateLimited,
        500..=599 => ErrorKind::Unavailable,
        _ => ErrorKind::Network,
    };
    AppError::new(kind, format!("{context} (HTTP {})", status.as_u16()))
}

fn network_error(error: reqwest::Error) -> AppError {
    let message = error.to_string();
    drop(error);
    AppError::new(ErrorKind::Network, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_output_redacts_both_credentials() {
        let tokens = TokenSet {
            access_token: "access-secret".into(),
            refresh_token: Some("refresh-secret".into()),
            expires_in: Duration::from_secs(3600),
            scopes: vec!["streaming".into()],
        };
        let debug = format!("{tokens:?}");
        assert!(!debug.contains("access-secret"));
        assert!(!debug.contains("refresh-secret"));
        assert!(debug.contains("redacted"));
    }

    #[test]
    fn sessions_missing_a_required_scope_are_detected() {
        let all = REQUIRED_SCOPES.iter().map(|scope| (*scope).to_owned()).collect::<Vec<_>>();
        assert!(has_required_scopes(&all));
        let without_email =
            all.iter().filter(|scope| *scope != "user-read-email").cloned().collect::<Vec<_>>();
        assert!(!has_required_scopes(&without_email));
    }

    #[test]
    fn status_codes_have_actionable_error_kinds() {
        assert_eq!(status_error(StatusCode::UNAUTHORIZED, "x").kind, ErrorKind::Authentication);
        assert_eq!(status_error(StatusCode::FORBIDDEN, "x").kind, ErrorKind::Authorization);
        assert_eq!(status_error(StatusCode::TOO_MANY_REQUESTS, "x").kind, ErrorKind::RateLimited);
        assert_eq!(status_error(StatusCode::BAD_GATEWAY, "x").kind, ErrorKind::Unavailable);
    }
}
