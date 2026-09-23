use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::RngCore as _;
use sha2::{Digest as _, Sha256};
use url::Url;

use lspotify_core::{AppError, ErrorKind, Result};

pub const AUTHORIZE_URL: &str = "https://accounts.spotify.com/authorize";
pub const CALLBACK_HOST: &str = "127.0.0.1";
pub const CALLBACK_PORT: u16 = 43_821;
pub const CALLBACK_URL: &str = "http://127.0.0.1:43821/callback";

pub const REQUIRED_SCOPES: &[&str] = &[
    "user-read-private",
    // Required by the Web Playback SDK; lspotify never reads or stores the address.
    "user-read-email",
    "user-library-read",
    "user-library-modify",
    "user-follow-read",
    "user-follow-modify",
    "playlist-read-private",
    "playlist-read-collaborative",
    "playlist-modify-private",
    "playlist-modify-public",
    "user-read-playback-state",
    "user-read-currently-playing",
    "user-modify-playback-state",
    "user-read-recently-played",
    "user-top-read",
    "streaming",
];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OAuthAttempt {
    pub verifier: String,
    pub challenge: String,
    pub state: String,
}

impl OAuthAttempt {
    pub fn generate() -> Self {
        let mut verifier_bytes = [0_u8; 64];
        let mut state_bytes = [0_u8; 32];
        rand::rng().fill_bytes(&mut verifier_bytes);
        rand::rng().fill_bytes(&mut state_bytes);
        let verifier = URL_SAFE_NO_PAD.encode(verifier_bytes);
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let state = URL_SAFE_NO_PAD.encode(state_bytes);
        Self { verifier, challenge, state }
    }

    /// Builds the Spotify authorization URL for this PKCE attempt.
    ///
    /// # Errors
    ///
    /// Returns an error when the public client identifier is invalid or the authorization base URL
    /// cannot be represented.
    pub fn authorization_url(&self, client_id: &str, show_dialog: bool) -> Result<Url> {
        validate_client_id(client_id)?;
        let mut url = Url::parse(AUTHORIZE_URL)
            .map_err(|error| AppError::new(ErrorKind::Unexpected, error.to_string()))?;
        url.query_pairs_mut()
            .append_pair("client_id", client_id)
            .append_pair("response_type", "code")
            .append_pair("redirect_uri", CALLBACK_URL)
            .append_pair("code_challenge_method", "S256")
            .append_pair("code_challenge", &self.challenge)
            .append_pair("state", &self.state)
            .append_pair("scope", &REQUIRED_SCOPES.join(" "))
            .append_pair("show_dialog", if show_dialog { "true" } else { "false" });
        Ok(url)
    }

    /// Validates the exact loopback callback and extracts its authorization code.
    ///
    /// # Errors
    ///
    /// Returns an error when the origin or path differs, state does not match, consent was denied,
    /// or no authorization code is present.
    pub fn validate_callback(&self, callback: &Url) -> Result<String> {
        if callback.scheme() != "http"
            || callback.host_str() != Some(CALLBACK_HOST)
            || callback.port_or_known_default() != Some(CALLBACK_PORT)
            || callback.path() != "/callback"
        {
            return Err(AppError::new(ErrorKind::InvalidInput, "unexpected OAuth callback URL"));
        }
        let mut state = None;
        let mut code = None;
        let mut denied = None;
        for (key, value) in callback.query_pairs() {
            match key.as_ref() {
                "state" => state = Some(value.into_owned()),
                "code" => code = Some(value.into_owned()),
                "error" => denied = Some(value.into_owned()),
                _ => {}
            }
        }
        if state.as_deref() != Some(self.state.as_str()) {
            return Err(AppError::new(ErrorKind::Authentication, "OAuth state mismatch"));
        }
        if let Some(reason) = denied {
            return Err(AppError::new(
                ErrorKind::Authorization,
                format!("Spotify authorization denied: {reason}"),
            ));
        }
        code.filter(|value| !value.is_empty()).ok_or_else(|| {
            AppError::new(ErrorKind::Authentication, "OAuth callback did not include a code")
        })
    }
}

/// Validates the public Spotify application identifier accepted during onboarding.
///
/// # Errors
///
/// Returns an error when the identifier has an invalid length or character.
pub fn validate_client_id(client_id: &str) -> Result<()> {
    let valid_length = (16..=64).contains(&client_id.len());
    let valid_characters = client_id.bytes().all(|byte| byte.is_ascii_alphanumeric());
    if valid_length && valid_characters {
        Ok(())
    } else {
        Err(AppError::new(ErrorKind::InvalidInput, "Spotify Client ID is not valid"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attempt() -> OAuthAttempt {
        OAuthAttempt {
            verifier: "v".repeat(64),
            challenge: "challenge".into(),
            state: "expected".into(),
        }
    }

    #[test]
    fn generated_attempt_has_pkce_safe_values() {
        let generated = OAuthAttempt::generate();
        assert!(generated.verifier.len() >= 43);
        assert!(!generated.verifier.contains('='));
        assert!(!generated.challenge.contains('='));
        assert_ne!(OAuthAttempt::generate().state, generated.state);
    }

    #[test]
    fn authorization_requests_web_playback_sdk_scopes() {
        let url = attempt().authorization_url("1234567890abcdef", false).unwrap();
        let scope = url.query_pairs().find(|(key, _)| key == "scope").unwrap().1;
        for required in ["streaming", "user-read-email", "user-read-private"] {
            assert!(scope.split(' ').any(|granted| granted == required));
        }
    }

    #[test]
    fn callback_requires_exact_loopback_and_matching_state() {
        let valid = Url::parse("http://127.0.0.1:43821/callback?code=abc&state=expected").unwrap();
        assert_eq!(attempt().validate_callback(&valid).unwrap(), "abc");
        let mismatch = Url::parse("http://127.0.0.1:43821/callback?code=abc&state=wrong").unwrap();
        assert_eq!(
            attempt().validate_callback(&mismatch).unwrap_err().kind,
            ErrorKind::Authentication
        );
        let hostile =
            Url::parse("http://localhost:43821/callback?code=abc&state=expected").unwrap();
        assert!(attempt().validate_callback(&hostile).is_err());
    }
}
