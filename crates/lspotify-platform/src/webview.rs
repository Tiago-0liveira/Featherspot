use std::{borrow::Cow, cell::Cell, fmt};

use lspotify_core::{AppError, ErrorKind, LocalPlayerCommand, Result};
use serde_json::json;
use wry::{
    PermissionKind, PermissionResponse, Rect, WebView, WebViewBuilder, WebViewBuilderExtWindows,
    dpi::{LogicalPosition, LogicalSize},
    http::{Response, header::CONTENT_TYPE},
    raw_window_handle::HasWindowHandle,
};

const PLAYER_HTML: &str = include_str!("../../../assets/web-player/index.html");
const PLAYER_BRIDGE: &str = include_str!("../../../assets/web-player/player-bridge.js");
const PLAYER_PROTOCOL: &str = "lspotify";
/// `WebView2` maps `lspotify://localhost/` to this secure origin. Encrypted Media Extensions,
/// which the Web Playback SDK requires, are unavailable to the opaque origin of inline HTML.
const PLAYER_ORIGIN: &str = "https://lspotify.localhost/";

/// Creates the hardened Wry builder used for the isolated Spotify playback child.
///
/// The caller must attach the returned builder to the native GPUI window handle and retain the
/// resulting webview for the local player's lifetime. Incoming payloads remain untrusted and must
/// be passed through `lspotify_playback::parse_event` at the adapter boundary.
pub fn playback_webview_builder(on_ipc: impl Fn(String) + 'static) -> WebViewBuilder<'static> {
    WebViewBuilder::new()
        .with_https_scheme(true)
        .with_custom_protocol(PLAYER_PROTOCOL.to_owned(), |_, request| {
            let (body, content_type) = match request.uri().path() {
                "/" | "/index.html" => (PLAYER_HTML, "text/html; charset=utf-8"),
                "/player-bridge.js" => (PLAYER_BRIDGE, "text/javascript; charset=utf-8"),
                _ => {
                    return Response::builder()
                        .status(404)
                        .body(Cow::Borrowed(&[][..]))
                        .unwrap_or_default();
                }
            };
            Response::builder()
                .header(CONTENT_TYPE, content_type)
                .body(Cow::Borrowed(body.as_bytes()))
                .unwrap_or_default()
        })
        .with_url(format!("{PLAYER_PROTOCOL}://localhost/index.html"))
        .with_navigation_handler(|url| is_allowed_player_navigation(&url))
        .with_permission_handler(|kind| match kind {
            PermissionKind::Autoplay | PermissionKind::MediaKeySystemAccess => {
                PermissionResponse::Allow
            }
            _ => PermissionResponse::Deny,
        })
        .with_download_started_handler(|_, _| false)
        .with_clipboard(false)
        .with_devtools(false)
        .with_transparent(true)
        .with_ipc_handler(move |request| on_ipc(request.body().clone()))
}

pub struct PlaybackWebView {
    webview: WebView,
    next_command_id: Cell<u64>,
}

impl fmt::Debug for PlaybackWebView {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("PlaybackWebView").finish_non_exhaustive()
    }
}

impl PlaybackWebView {
    /// Sends one typed command to the isolated Web Playback SDK bridge.
    ///
    /// # Errors
    ///
    /// Returns an error when `WebView2` rejects script evaluation.
    pub fn send(&self, command: &LocalPlayerCommand) -> Result<()> {
        let payload = match command {
            LocalPlayerCommand::TokenUpdate { access_token } => {
                json!({"type": "token_update", "access_token": access_token})
            }
            LocalPlayerCommand::Connect => json!({"type": "connect"}),
            LocalPlayerCommand::Disconnect => json!({"type": "disconnect"}),
            LocalPlayerCommand::Play => json!({"type": "play"}),
            LocalPlayerCommand::Pause => json!({"type": "pause"}),
            LocalPlayerCommand::Previous => json!({"type": "previous"}),
            LocalPlayerCommand::Next => json!({"type": "next"}),
            LocalPlayerCommand::Seek { position_ms } => {
                json!({"type": "seek", "position_ms": position_ms})
            }
            LocalPlayerCommand::Volume { value_milli } => {
                json!({"type": "volume", "value_milli": value_milli})
            }
            LocalPlayerCommand::Shuffle { enabled } => {
                json!({"type": "shuffle", "enabled": enabled})
            }
            LocalPlayerCommand::Repeat { mode } => json!({"type": "repeat", "mode": mode}),
            LocalPlayerCommand::Shutdown => json!({"type": "shutdown"}),
        };
        let id = self.next_command_id.get();
        self.next_command_id.set(id.wrapping_add(1));
        let envelope = json!({"version": 1, "id": id, "payload": payload}).to_string();
        let quoted = serde_json::to_string(&envelope)
            .map_err(|error| AppError::new(ErrorKind::Unexpected, error.to_string()))?;
        self.webview
            .evaluate_script(&format!("window.postMessage(JSON.parse({quoted}), '*');"))
            .map_err(|error| webview_error(&error))
    }
}

/// Creates the one-pixel `WebView2` child that owns Spotify's protected local audio pipeline.
///
/// # Errors
///
/// Returns an error when `WebView2` is missing or the child surface cannot be created.
pub fn create_playback_webview<W: HasWindowHandle>(
    window: &W,
    on_ipc: impl Fn(String) + 'static,
) -> Result<PlaybackWebView> {
    let bounds =
        Rect { position: LogicalPosition::new(0, 0).into(), size: LogicalSize::new(1, 1).into() };
    let webview = playback_webview_builder(on_ipc)
        .with_bounds(bounds)
        .build_as_child(window)
        .map_err(|error| webview_error(&error))?;
    Ok(PlaybackWebView { webview, next_command_id: Cell::new(1) })
}

fn webview_error(error: &wry::Error) -> AppError {
    AppError::new(ErrorKind::Unavailable, format!("local WebView2 player failed: {error}"))
}

pub fn is_allowed_player_navigation(url: &str) -> bool {
    url == "about:blank"
        || url.starts_with(PLAYER_ORIGIN)
        || url.starts_with("https://sdk.scdn.co/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn player_navigation_is_restrictive() {
        assert!(is_allowed_player_navigation("about:blank"));
        assert!(is_allowed_player_navigation("https://lspotify.localhost/index.html"));
        assert!(!is_allowed_player_navigation("https://lspotify.localhost.example.com/"));
        assert!(is_allowed_player_navigation("https://sdk.scdn.co/spotify-player.js"));
        assert!(!is_allowed_player_navigation("https://example.com/phishing"));
        assert!(!is_allowed_player_navigation("javascript:alert(1)"));
        assert!(!is_allowed_player_navigation("file:///C:/secret.txt"));
    }
}
