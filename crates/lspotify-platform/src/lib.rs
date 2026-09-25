#![forbid(unsafe_code)]

use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::Mutex,
    time::Duration,
};

use lspotify_core::{
    AppError, CredentialStore, ErrorKind, MediaButton, MediaMetadata, MediaSession, Result,
};
mod local_player;

#[cfg(target_os = "windows")]
mod webview;

#[cfg(target_os = "windows")]
mod windows_credentials;

#[cfg(target_os = "windows")]
pub use webview::{
    PlaybackWebView, create_playback_webview, is_allowed_player_navigation,
    playback_webview_builder,
};

pub use local_player::BackgroundLocalPlayer;

#[cfg(target_os = "windows")]
pub use windows_credentials::WindowsCredentialStore;

/// Opens a URL in the user's default browser without blocking the application window.
///
/// # Errors
///
/// Returns an error when the operating system cannot launch the URL handler.
pub fn open_system_browser(url: &str) -> Result<()> {
    open::that_detached(url).map_err(|error| {
        AppError::new(
            ErrorKind::Unavailable,
            format!("default browser could not be opened: {error}"),
        )
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppPaths {
    pub data: PathBuf,
    pub cache: PathBuf,
    pub logs: PathBuf,
    pub settings: PathBuf,
}

impl AppPaths {
    /// Discovers the per-user application locations. Composition roots should use
    /// this instead of reading platform environment variables themselves.
    ///
    /// # Errors
    /// Returns an error if the home or data directory cannot be determined.
    pub fn discover() -> Result<Self> {
        #[cfg(target_os = "windows")]
        let root =
            std::env::var_os("LOCALAPPDATA").map(PathBuf::from).map(|path| path.join("lspotify"));
        #[cfg(target_os = "macos")]
        let root = std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|home| home.join("Library").join("Application Support").join("lspotify"));
        #[cfg(all(unix, not(target_os = "macos")))]
        let root = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share"))
            })
            .map(|path| path.join("lspotify"));
        root.map_or_else(
            || Err(AppError::new(ErrorKind::Storage, "could not discover a user data directory")),
            |root| Ok(Self::under(root)),
        )
    }

    pub fn under(root: impl AsRef<Path>) -> Self {
        let root = root.as_ref();
        Self {
            data: root.join("data"),
            cache: root.join("cache"),
            logs: root.join("logs"),
            settings: root.join("settings.json"),
        }
    }
}

#[derive(Debug, Default)]
pub struct MemoryCredentialStore {
    refresh_token: Mutex<Option<String>>,
}

impl CredentialStore for MemoryCredentialStore {
    fn load_refresh_token(&self) -> Result<Option<String>> {
        self.refresh_token.lock().map_err(|_| poisoned()).map(|token| token.clone())
    }

    fn store_refresh_token(&self, refresh_token: &str) -> Result<()> {
        if refresh_token.is_empty() {
            return Err(AppError::new(ErrorKind::InvalidInput, "refresh token cannot be empty"));
        }
        *self.refresh_token.lock().map_err(|_| poisoned())? = Some(refresh_token.to_owned());
        Ok(())
    }

    fn clear_refresh_token(&self) -> Result<()> {
        *self.refresh_token.lock().map_err(|_| poisoned())? = None;
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct NoopMediaSession {
    metadata: Mutex<Option<MediaMetadata>>,
    state: Mutex<(bool, Duration)>,
    buttons: Mutex<VecDeque<MediaButton>>,
}

impl NoopMediaSession {
    /// Adds a simulated media button event for adapter and application tests.
    ///
    /// # Errors
    ///
    /// Returns an error if another panic has poisoned the internal event queue.
    pub fn push_test_button(&self, button: MediaButton) -> Result<()> {
        self.buttons.lock().map_err(|_| poisoned())?.push_back(button);
        Ok(())
    }
}

impl MediaSession for NoopMediaSession {
    fn publish(&self, metadata: Option<MediaMetadata>) -> Result<()> {
        *self.metadata.lock().map_err(|_| poisoned())? = metadata;
        Ok(())
    }

    fn set_playback(&self, playing: bool, position: Duration) -> Result<()> {
        *self.state.lock().map_err(|_| poisoned())? = (playing, position);
        Ok(())
    }

    fn take_button_event(&self) -> Result<Option<MediaButton>> {
        Ok(self.buttons.lock().map_err(|_| poisoned())?.pop_front())
    }

    fn clear(&self) -> Result<()> {
        *self.metadata.lock().map_err(|_| poisoned())? = None;
        *self.state.lock().map_err(|_| poisoned())? = (false, Duration::ZERO);
        self.buttons.lock().map_err(|_| poisoned())?.clear();
        Ok(())
    }
}

fn poisoned() -> AppError {
    AppError::new(ErrorKind::Unexpected, "platform adapter lock was poisoned")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_credentials_obey_single_account_lifecycle() {
        let store = MemoryCredentialStore::default();
        assert_eq!(store.load_refresh_token().unwrap(), None);
        store.store_refresh_token("secret").unwrap();
        assert_eq!(store.load_refresh_token().unwrap().as_deref(), Some("secret"));
        store.clear_refresh_token().unwrap();
        assert_eq!(store.load_refresh_token().unwrap(), None);
    }

    #[test]
    fn platform_paths_are_kept_under_the_selected_root() {
        let paths = AppPaths::under("portable");
        assert_eq!(paths.cache, Path::new("portable").join("cache"));
        assert_eq!(paths.settings, Path::new("portable").join("settings.json"));
    }

            let output = child.wait_with_output().expect("node execution");
            assert!(
                output.status.success(),
                "node script failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}
