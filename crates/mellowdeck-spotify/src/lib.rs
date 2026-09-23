#![forbid(unsafe_code)]

mod auth;
mod callback;
mod home;
mod oauth;
mod policy;

pub use callback::CallbackServer;
pub use home::{
    SpotifyArtistDetail, SpotifyBrowseItem, SpotifyContentState, SpotifyDetailKind,
    SpotifyDetailPage, SpotifyDevice, SpotifyDisplayItem, SpotifyEntityKind, SpotifyHome,
    SpotifyLibrary, SpotifyLibraryService, SpotifyPage, SpotifyPlayback, SpotifyQueue,
    SpotifySearchItem, SpotifySection, SpotifyTypedQueue, SpotifyWebApi,
};
pub use oauth::{
    AUTHORIZE_URL, CALLBACK_HOST, CALLBACK_PORT, CALLBACK_URL, OAuthAttempt, REQUIRED_SCOPES,
    validate_client_id,
};
pub use policy::{HttpMethod, ResponseAction, RetryPolicy, classify_response, redact_log_value};

pub const API_BASE_URL: &str = "https://api.spotify.com/v1";
pub use auth::{AuthorizedSession, SpotifyAuthenticator, TokenSet};
