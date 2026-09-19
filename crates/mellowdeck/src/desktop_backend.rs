use std::{env, fmt, path::PathBuf, sync::Mutex, thread, time::Duration};

use mellowdeck_core::{
    AppError, CredentialStore, ErrorKind, Result, SettingsState, ThemePreference,
};
use mellowdeck_platform::{AppPaths, WindowsCredentialStore, open_system_browser};
use mellowdeck_spotify::{
    SpotifyArtistDetail, SpotifyAuthenticator, SpotifyDevice, SpotifyDisplayItem, SpotifyHome,
    SpotifyLibrary, SpotifyPlayback, SpotifyQueue, SpotifySection, SpotifyWebApi, TokenSet,
    validate_client_id,
};
use mellowdeck_storage::JsonSettingsStore;
use mellowdeck_ui::{
    ArtistDetail, AuthorizedAccount, DesktopActions, HomeItem, HomeSection, HomeSnapshot,
    LibrarySnapshot, PlaybackCommand, PlaybackDevice, PlaybackSnapshot, QueueSnapshot,
    SearchResultItem, StartupState,
};

const SPOTIFY_DASHBOARD_URL: &str = "https://developer.spotify.com/dashboard";

pub struct DesktopBackend {
    settings_store: JsonSettingsStore,
    settings: Mutex<SettingsState>,
    credentials: WindowsCredentialStore,
    authenticator: SpotifyAuthenticator,
    spotify: SpotifyWebApi,
    access: Mutex<Option<TokenSet>>,
}

impl DesktopBackend {
    pub fn load(paths: AppPaths) -> Result<(Self, StartupState)> {
        let settings_store = JsonSettingsStore::new(paths.settings);
        let (settings, warning) = match settings_store.load() {
            Ok(settings) => (settings, None),
            Err(error) => (SettingsState::default(), Some(error.to_string())),
        };
        let credentials = WindowsCredentialStore::open()?;
        let has_saved_session = credentials.load_refresh_token()?.is_some();
        let startup = StartupState {
            client_id: settings.client_id.clone(),
            has_saved_session,
            warning,
            theme: settings.theme,
        };
        Ok((
            Self {
                settings_store,
                settings: Mutex::new(settings),
                credentials,
                authenticator: SpotifyAuthenticator::new()?,
                spotify: SpotifyWebApi::new()?,
                access: Mutex::new(None),
            },
            startup,
        ))
    }

    fn save_client_id(&self, client_id: &str) -> Result<()> {
        validate_client_id(client_id)?;
        let mut settings = self.settings.lock().map_err(|_| poisoned())?;
        settings.client_id = Some(client_id.to_owned());
        self.settings_store.save(&settings)
    }

    fn access_token(&self) -> Result<String> {
        let access = self.access.lock().map_err(|_| poisoned())?;
        access.as_ref().map(|token| token.access_token().to_owned()).ok_or_else(|| {
            AppError::new(ErrorKind::Authentication, "there is no active Spotify session")
        })
    }
}

impl fmt::Debug for DesktopBackend {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("DesktopBackend").finish_non_exhaustive()
    }
}

impl DesktopActions for DesktopBackend {
    fn authenticate(&self, client_id: &str, restore: bool) -> Result<AuthorizedAccount> {
        tracing::info!(restore, "Spotify authentication started");
        self.save_client_id(client_id)?;
        let session_result = if restore {
            let refresh_token = self.credentials.load_refresh_token()?.ok_or_else(|| {
                AppError::new(ErrorKind::Authentication, "the saved Spotify session is missing")
            })?;
            self.authenticator.refresh(client_id, &refresh_token)
        } else {
            self.authenticator.authorize(client_id, open_system_browser)
        };
        let session = session_result.inspect_err(|error| {
            tracing::warn!(kind = ?error.kind, restore, "Spotify authentication failed");
        })?;
        if let Some(refresh_token) = session.tokens.refresh_token() {
            self.credentials.store_refresh_token(refresh_token)?;
        }
        let account = AuthorizedAccount {
            user_id: session.user_id,
            display_name: session.display_name,
            premium: session.premium,
        };
        *self.access.lock().map_err(|_| poisoned())? = Some(session.tokens);
        tracing::info!(restore, premium = account.premium, "Spotify authentication completed");
        Ok(account)
    }

    fn open_developer_dashboard(&self) -> Result<()> {
        open_system_browser(SPOTIFY_DASHBOARD_URL)
    }

    fn load_home(&self) -> Result<HomeSnapshot> {
        tracing::info!("loading Spotify Home collections");
        let access = self.access.lock().map_err(|_| poisoned())?;
        let token = access.as_ref().ok_or_else(|| {
            AppError::new(ErrorKind::Authentication, "there is no active Spotify session")
        })?;
        let home = self.spotify.load_home(token.access_token())?;
        let snapshot = map_home(home);
        let unavailable = [
            &snapshot.recently_played,
            &snapshot.top_artists,
            &snapshot.top_tracks,
            &snapshot.saved_albums,
            &snapshot.playlists,
        ]
        .into_iter()
        .filter(|section| section.warning.is_some())
        .count();
        tracing::info!(unavailable, "Spotify Home collections loaded");
        Ok(snapshot)
    }

    fn load_library(&self) -> Result<LibrarySnapshot> {
        let token = self.access_token()?;
        tracing::info!("loading Spotify library");
        let library = self.spotify.load_library(&token)?;
        Ok(map_library(library))
    }

    fn save_theme(&self, theme: ThemePreference) -> Result<()> {
        let mut settings = self.settings.lock().map_err(|_| poisoned())?;
        settings.theme = theme;
        self.settings_store.save(&settings)
    }

    fn search(&self, query: &str) -> Result<Vec<SearchResultItem>> {
        let access = self.access.lock().map_err(|_| poisoned())?;
        let token = access.as_ref().ok_or_else(|| {
            AppError::new(ErrorKind::Authentication, "there is no active Spotify session")
        })?;
        let results = self.spotify.search(token.access_token(), query)?;
        tracing::info!(result_count = results.len(), "Spotify search completed");
        Ok(results
            .into_iter()
            .map(|item| SearchResultItem {
                title: item.title,
                subtitle: item.subtitle,
                kind: item.kind,
                uri: item.uri,
                artwork_url: item.artwork_url,
            })
            .collect())
    }

    fn load_artist(&self, uri: &str) -> Result<ArtistDetail> {
        let access = self.access.lock().map_err(|_| poisoned())?;
        let token = access.as_ref().ok_or_else(|| {
            AppError::new(ErrorKind::Authentication, "there is no active Spotify session")
        })?;
        tracing::info!("loading Spotify artist page");
        self.spotify.artist(token.access_token(), uri).map(map_artist)
    }

    fn load_album(&self, uri: &str) -> Result<ArtistDetail> {
        let access = self.access.lock().map_err(|_| poisoned())?;
        let token = access.as_ref().ok_or_else(|| {
            AppError::new(ErrorKind::Authentication, "there is no active Spotify session")
        })?;
        tracing::info!("loading Spotify album page");
        self.spotify.album(token.access_token(), uri).map(map_artist)
    }

    fn local_player_access_token(&self) -> Result<String> {
        let access = self.access.lock().map_err(|_| poisoned())?;
        access.as_ref().map(|token| token.access_token().to_owned()).ok_or_else(|| {
            AppError::new(ErrorKind::Authentication, "there is no active Spotify session")
        })
    }

    fn playback_command(&self, command: PlaybackCommand) -> Result<PlaybackSnapshot> {
        let access = self.access.lock().map_err(|_| poisoned())?;
        let token = access.as_ref().ok_or_else(|| {
            AppError::new(ErrorKind::Authentication, "there is no active Spotify session")
        })?;
        tracing::info!(command = command_name(&command), "sending Spotify playback command");
        let command_result = match command {
            PlaybackCommand::PlayUri(uri) => self.spotify.play_uri(token.access_token(), &uri),
            PlaybackCommand::PlayUriOnDevice { uri, device_id } => {
                self.spotify.play_uri_on_device(token.access_token(), &uri, Some(&device_id))
            }
            PlaybackCommand::PlayContext { uri, shuffle } => {
                let result = self.spotify.play_uri(token.access_token(), &uri);
                if result.is_ok() && shuffle {
                    // Set shuffle after playback starts, so the target device is active.
                    let _ = self.spotify.shuffle(token.access_token(), true);
                }
                result
            }
            PlaybackCommand::Resume => self.spotify.resume(token.access_token()),
            PlaybackCommand::Pause => self.spotify.pause(token.access_token()),
            PlaybackCommand::Previous => self.spotify.previous(token.access_token()),
            PlaybackCommand::Next => self.spotify.next(token.access_token()),
            PlaybackCommand::Seek(position) => self.spotify.seek(token.access_token(), position),
            PlaybackCommand::Volume(percent) => self.spotify.volume(token.access_token(), percent),
            PlaybackCommand::Transfer(device) => {
                self.spotify.transfer(token.access_token(), &device)
            }
        };
        command_result.inspect_err(|error| {
            tracing::warn!(kind = ?error.kind, error = %error, "Spotify playback command failed");
        })?;
        // Spotify documents that player command ordering is not guaranteed. Give Connect a brief
        // moment to publish the authoritative state before refreshing the player bar.
        thread::sleep(Duration::from_millis(250));
        self.spotify.playback(token.access_token()).map(map_playback).inspect_err(|error| {
            tracing::warn!(kind = ?error.kind, "Spotify playback refresh failed");
        })
    }

    fn load_playback(&self) -> Result<PlaybackSnapshot> {
        let access = self.access.lock().map_err(|_| poisoned())?;
        let token = access.as_ref().ok_or_else(|| {
            AppError::new(ErrorKind::Authentication, "there is no active Spotify session")
        })?;
        self.spotify.playback(token.access_token()).map(map_playback)
    }

    fn load_devices(&self) -> Result<Vec<PlaybackDevice>> {
        let access = self.access.lock().map_err(|_| poisoned())?;
        let token = access.as_ref().ok_or_else(|| {
            AppError::new(ErrorKind::Authentication, "there is no active Spotify session")
        })?;
        self.spotify
            .devices(token.access_token())
            .map(|devices| devices.into_iter().map(map_device).collect())
    }

    fn load_queue(&self) -> Result<QueueSnapshot> {
        let access = self.access.lock().map_err(|_| poisoned())?;
        let token = access.as_ref().ok_or_else(|| {
            AppError::new(ErrorKind::Authentication, "there is no active Spotify session")
        })?;
        self.spotify.queue(token.access_token()).map(map_queue)
    }
}

pub fn app_paths() -> AppPaths {
    let root = env::var_os("LOCALAPPDATA")
        .map_or_else(|| PathBuf::from("."), PathBuf::from)
        .join("Mellowdeck");
    AppPaths::under(root)
}

fn map_home(home: SpotifyHome) -> HomeSnapshot {
    HomeSnapshot {
        recently_played: map_section(home.recently_played),
        top_artists: map_section(home.top_artists),
        top_tracks: map_section(home.top_tracks),
        saved_albums: map_section(home.saved_albums),
        playlists: map_section(home.playlists),
    }
}

fn map_library(library: SpotifyLibrary) -> LibrarySnapshot {
    LibrarySnapshot {
        albums: map_section(library.albums),
        playlists: map_section(library.playlists),
    }
}

fn map_section(section: SpotifySection) -> HomeSection {
    HomeSection {
        items: section.items.into_iter().map(map_item).collect(),
        warning: section.warning,
    }
}

fn map_item(item: SpotifyDisplayItem) -> HomeItem {
    HomeItem {
        title: item.title,
        subtitle: item.subtitle,
        uri: item.uri,
        artwork_url: item.artwork_url,
    }
}

fn map_playback(playback: SpotifyPlayback) -> PlaybackSnapshot {
    PlaybackSnapshot {
        title: playback.title,
        subtitle: playback.subtitle,
        artists: playback.artists,
        artwork_url: playback.artwork_url,
        playing: playback.playing,
        progress_ms: playback.progress_ms,
        duration_ms: playback.duration_ms,
        volume_percent: playback.volume_percent,
        device_name: playback.device_name,
    }
}

fn map_device(device: SpotifyDevice) -> PlaybackDevice {
    PlaybackDevice {
        id: device.id,
        name: device.name,
        kind: device.kind,
        active: device.active,
        restricted: device.restricted,
    }
}

fn map_queue(queue: SpotifyQueue) -> QueueSnapshot {
    QueueSnapshot {
        current: queue.current.map(map_item),
        upcoming: queue.upcoming.into_iter().map(map_item).collect(),
    }
}

fn map_artist(artist: SpotifyArtistDetail) -> ArtistDetail {
    ArtistDetail {
        name: artist.name,
        description: artist.description,
        uri: artist.uri,
        artwork_url: artist.artwork_url,
        banner_url: artist.banner_url,
        is_album: artist.is_album,
        songs: artist.songs.into_iter().map(map_item).collect(),
        albums: artist.albums.into_iter().map(map_item).collect(),
        warning: artist.warning,
    }
}

fn command_name(command: &PlaybackCommand) -> &'static str {
    match command {
        PlaybackCommand::PlayUri(_) => "play_uri",
        PlaybackCommand::PlayUriOnDevice { .. } => "play_uri_on_device",
        PlaybackCommand::PlayContext { .. } => "play_context",
        PlaybackCommand::Resume => "resume",
        PlaybackCommand::Pause => "pause",
        PlaybackCommand::Previous => "previous",
        PlaybackCommand::Next => "next",
        PlaybackCommand::Seek(_) => "seek",
        PlaybackCommand::Volume(_) => "volume",
        PlaybackCommand::Transfer(_) => "transfer",
    }
}

fn poisoned() -> AppError {
    AppError::new(ErrorKind::Unexpected, "desktop session lock was poisoned")
}
