#![forbid(unsafe_code)]

#[cfg(target_os = "windows")]
mod desktop_backend;
#[cfg(target_os = "windows")]
mod logging;

#[cfg(target_os = "windows")]
#[derive(Debug)]
struct DesktopLocalPlayer(mellowdeck_platform::PlaybackWebView);

#[cfg(target_os = "windows")]
impl mellowdeck_ui::LocalPlayerSurface for DesktopLocalPlayer {
    fn send(&self, command: &mellowdeck_core::LocalPlayerCommand) -> mellowdeck_core::Result<()> {
        self.0.send(command)
    }
}

#[cfg(target_os = "windows")]
fn main() {
    use std::sync::Arc;

    let paths = desktop_backend::app_paths();
    let _logging_guard = match logging::initialize(&paths.logs) {
        Ok(guard) => Some(guard),
        Err(error) => {
            eprintln!("Mellowdeck could not initialize file logging: {error}");
            None
        }
    };
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "Mellowdeck starting");
    let local_player_factory: mellowdeck_ui::LocalPlayerFactory = Arc::new(|window, events| {
        let player = mellowdeck_platform::create_playback_webview(window, move |body| {
            match mellowdeck_playback::parse_event(&body) {
                Ok(envelope) => {
                    let event = match envelope.payload {
                        mellowdeck_playback::BridgeEvent::Ready { device_id } => {
                            tracing::info!("local player ready");
                            mellowdeck_core::LocalPlayerEvent::Ready { device_id }
                        }
                        mellowdeck_playback::BridgeEvent::Unavailable { reason } => {
                            tracing::warn!(?reason, "local player unavailable");
                            mellowdeck_core::LocalPlayerEvent::Unavailable
                        }
                        mellowdeck_playback::BridgeEvent::StateChanged {
                            playing,
                            position_ms,
                            duration_ms,
                            track_uri,
                            title,
                            artist,
                            album,
                            artwork_url,
                        } => mellowdeck_core::LocalPlayerEvent::StateChanged {
                            playing,
                            position_ms,
                            duration_ms,
                            track_uri,
                            title,
                            artist,
                            album,
                            artwork_url,
                        },
                        mellowdeck_playback::BridgeEvent::AuthenticationError { message } => {
                            tracing::warn!(%message, "local player reported AuthenticationError");
                            mellowdeck_core::LocalPlayerEvent::AuthenticationError(message)
                        }
                        mellowdeck_playback::BridgeEvent::PlaybackError { message } => {
                            tracing::warn!(%message, "local player reported PlaybackError");
                            mellowdeck_core::LocalPlayerEvent::PlaybackError(message)
                        }
                        mellowdeck_playback::BridgeEvent::AccountError { message } => {
                            tracing::warn!(%message, "local player reported AccountError");
                            mellowdeck_core::LocalPlayerEvent::AccountError(message)
                        }
                    };
                    let _ = events.send(event);
                }
                Err(error) => tracing::warn!(kind = ?error.kind, "ignored local player message"),
            }
        })?;
        Ok(Box::new(DesktopLocalPlayer(player)))
    });
    match desktop_backend::DesktopBackend::load(paths) {
        Ok((backend, startup)) => {
            mellowdeck_ui::run_desktop_with(startup, Arc::new(backend), local_player_factory);
        }
        Err(error) => {
            tracing::error!(error = %error, "desktop initialization failed");
            eprintln!("Mellowdeck could not initialize secure storage: {error}");
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn main() {
    #[derive(Debug)]
    struct Unsupported;

    impl mellowdeck_ui::DesktopActions for Unsupported {
        fn authenticate(
            &self,
            _client_id: &str,
            _restore: bool,
        ) -> mellowdeck_core::Result<mellowdeck_ui::AuthorizedAccount> {
            Err(mellowdeck_core::AppError::new(
                mellowdeck_core::ErrorKind::Unavailable,
                "the desktop application is currently available on Windows only",
            ))
        }

        fn open_developer_dashboard(&self) -> mellowdeck_core::Result<()> {
            Err(mellowdeck_core::AppError::new(
                mellowdeck_core::ErrorKind::Unavailable,
                "the desktop application is currently available on Windows only",
            ))
        }

        fn load_home(&self) -> mellowdeck_core::Result<mellowdeck_ui::HomeSnapshot> {
            Err(mellowdeck_core::AppError::new(
                mellowdeck_core::ErrorKind::Unavailable,
                "the desktop application is currently available on Windows only",
            ))
        }

        fn load_library(&self) -> mellowdeck_core::Result<mellowdeck_ui::LibrarySnapshot> {
            unsupported()
        }

        fn save_theme(
            &self,
            _theme: mellowdeck_core::ThemePreference,
        ) -> mellowdeck_core::Result<()> {
            unsupported()
        }

        fn search(
            &self,
            _query: &str,
        ) -> mellowdeck_core::Result<Vec<mellowdeck_ui::SearchResultItem>> {
            Err(mellowdeck_core::AppError::new(
                mellowdeck_core::ErrorKind::Unavailable,
                "the desktop application is currently available on Windows only",
            ))
        }

        fn load_artist(&self, _uri: &str) -> mellowdeck_core::Result<mellowdeck_ui::ArtistDetail> {
            unsupported()
        }

        fn load_album(&self, _uri: &str) -> mellowdeck_core::Result<mellowdeck_ui::ArtistDetail> {
            unsupported()
        }

        fn local_player_access_token(&self) -> mellowdeck_core::Result<String> {
            unsupported()
        }

        fn playback_command(
            &self,
            _command: mellowdeck_ui::PlaybackCommand,
        ) -> mellowdeck_core::Result<mellowdeck_ui::PlaybackSnapshot> {
            unsupported()
        }

        fn load_playback(&self) -> mellowdeck_core::Result<mellowdeck_ui::PlaybackSnapshot> {
            unsupported()
        }

        fn load_devices(&self) -> mellowdeck_core::Result<Vec<mellowdeck_ui::PlaybackDevice>> {
            unsupported()
        }

        fn load_queue(&self) -> mellowdeck_core::Result<mellowdeck_ui::QueueSnapshot> {
            unsupported()
        }
    }

    fn unsupported<T>() -> mellowdeck_core::Result<T> {
        Err(mellowdeck_core::AppError::new(
            mellowdeck_core::ErrorKind::Unavailable,
            "the desktop application is currently available on Windows only",
        ))
    }

    mellowdeck_ui::run_desktop_with(Default::default(), std::sync::Arc::new(Unsupported));
}
