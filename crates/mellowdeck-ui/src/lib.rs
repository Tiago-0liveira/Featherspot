#![forbid(unsafe_code)]

use std::fmt::Debug;

#[cfg(target_os = "windows")]
use std::sync::{Arc, mpsc::Sender};

#[cfg(target_os = "windows")]
use mellowdeck_core::{LocalPlayerCommand, LocalPlayerEvent};
use mellowdeck_core::{Result, ThemePreference, UserId};

mod localization;
mod theme;

#[cfg(target_os = "windows")]
mod gpui_shell;
#[cfg(target_os = "windows")]
mod image_http;

pub use localization::{Catalog, Locale};
pub use theme::{Color, Theme, ThemeTokens};

pub const MINIMUM_WINDOW_WIDTH: u32 = 1_024;
pub const MINIMUM_WINDOW_HEIGHT: u32 = 680;

#[cfg(target_os = "windows")]
pub use gpui_shell::run_desktop_with;

#[cfg(target_os = "windows")]
pub trait LocalPlayerSurface: Debug {
    /// Sends a typed command without exposing `WebView` implementation details to the UI.
    ///
    /// # Errors
    ///
    /// Returns an error when the platform surface rejects the command.
    fn send(&self, command: &LocalPlayerCommand) -> Result<()>;
}

#[cfg(target_os = "windows")]
pub type LocalPlayerFactory = Arc<
    dyn Fn(&gpui::Window, Sender<LocalPlayerEvent>) -> Result<Box<dyn LocalPlayerSurface>>
        + Send
        + Sync,
>;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct StartupState {
    pub client_id: Option<String>,
    pub has_saved_session: bool,
    pub warning: Option<String>,
    /// The user's saved theme preference, applied to the shell at startup.
    pub theme: ThemePreference,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthorizedAccount {
    pub user_id: UserId,
    pub display_name: String,
    pub premium: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct HomeItem {
    pub title: String,
    pub subtitle: String,
    pub uri: Option<String>,
    pub artwork_url: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct HomeSection {
    pub items: Vec<HomeItem>,
    pub warning: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct HomeSnapshot {
    pub recently_played: HomeSection,
    pub top_artists: HomeSection,
    pub top_tracks: HomeSection,
    pub saved_albums: HomeSection,
    pub playlists: HomeSection,
}

/// The user's full library: every saved album and playlist, shown on the Library tab.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LibrarySnapshot {
    pub albums: HomeSection,
    pub playlists: HomeSection,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchResultItem {
    pub title: String,
    pub subtitle: String,
    pub kind: String,
    pub uri: Option<String>,
    pub artwork_url: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PlaybackSnapshot {
    pub title: String,
    pub subtitle: String,
    /// The track's artists as `(display name, artist URI)` pairs, in Spotify's order.
    pub artists: Vec<(String, Option<String>)>,
    pub artwork_url: Option<String>,
    pub playing: bool,
    pub progress_ms: u64,
    pub duration_ms: u64,
    pub volume_percent: Option<u8>,
    pub device_name: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlaybackDevice {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub active: bool,
    pub restricted: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct QueueSnapshot {
    pub current: Option<HomeItem>,
    pub upcoming: Vec<HomeItem>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ArtistDetail {
    pub name: String,
    pub description: String,
    pub uri: String,
    pub artwork_url: Option<String>,
    /// A wide hero image for the page header, when one is available.
    pub banner_url: Option<String>,
    /// True when this detail describes an album (tracks) rather than an artist.
    pub is_album: bool,
    pub songs: Vec<HomeItem>,
    pub albums: Vec<HomeItem>,
    pub warning: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PlaybackCommand {
    PlayUri(String),
    PlayUriOnDevice { uri: String, device_id: String },
    /// Plays an album/artist/playlist context, optionally shuffled.
    PlayContext { uri: String, shuffle: bool },
    Resume,
    Pause,
    Previous,
    Next,
    Seek(u64),
    Volume(u8),
    Transfer(String),
}

pub trait DesktopActions: Debug + Send + Sync {
    /// Saves the Client ID and either restores or starts a Spotify PKCE session.
    ///
    /// # Errors
    ///
    /// Returns a user-displayable error when settings, credentials, networking, or authorization
    /// fails.
    fn authenticate(&self, client_id: &str, restore: bool) -> Result<AuthorizedAccount>;

    /// Opens the Spotify developer dashboard in the system browser.
    ///
    /// # Errors
    ///
    /// Returns an error when the platform cannot launch the browser.
    fn open_developer_dashboard(&self) -> Result<()>;

    /// Loads the personalized collections used by the Home screen.
    ///
    /// # Errors
    ///
    /// Returns an error when there is no active session or Spotify cannot be reached.
    fn load_home(&self) -> Result<HomeSnapshot>;

    /// Loads the user's full saved library (albums and playlists) for the Library tab.
    ///
    /// # Errors
    ///
    /// Returns an error when there is no active session or Spotify cannot be reached.
    fn load_library(&self) -> Result<LibrarySnapshot>;

    /// Persists the user's theme preference.
    ///
    /// # Errors
    ///
    /// Returns an error when the settings file cannot be written.
    fn save_theme(&self, theme: ThemePreference) -> Result<()>;

    /// Searches Spotify catalog content for the signed-in user.
    ///
    /// # Errors
    ///
    /// Returns an error when the query is invalid, the session has expired, or Spotify cannot be
    /// reached.
    fn search(&self, query: &str) -> Result<Vec<SearchResultItem>>;

    /// Loads an artist page with releases and supported song results.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid URI, expired session, or unavailable Spotify endpoint.
    fn load_artist(&self, uri: &str) -> Result<ArtistDetail>;

    /// Loads an album page with its track list.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid URI, expired session, or unavailable Spotify endpoint.
    fn load_album(&self, uri: &str) -> Result<ArtistDetail>;

    /// Returns the short-lived access token required by the isolated local playback surface.
    ///
    /// # Errors
    ///
    /// Returns an error when there is no active session.
    fn local_player_access_token(&self) -> Result<String>;

    /// Executes a Spotify Connect command and returns the reconciled playback state.
    ///
    /// # Errors
    ///
    /// Returns an error when no compatible active device exists or Spotify rejects the command.
    fn playback_command(&self, command: PlaybackCommand) -> Result<PlaybackSnapshot>;

    /// Loads the current Spotify Connect playback state.
    ///
    /// # Errors
    ///
    /// Returns an error when the session expires or Spotify cannot be reached.
    fn load_playback(&self) -> Result<PlaybackSnapshot>;

    /// Loads current Spotify Connect devices.
    ///
    /// # Errors
    ///
    /// Returns an error when the session expires or Spotify cannot be reached.
    fn load_devices(&self) -> Result<Vec<PlaybackDevice>>;

    /// Loads the current Spotify queue.
    ///
    /// # Errors
    ///
    /// Returns an error when the session expires or Spotify cannot be reached.
    fn load_queue(&self) -> Result<QueueSnapshot>;
}

#[cfg(not(target_os = "windows"))]
pub fn run_desktop_with(_startup: StartupState, _actions: std::sync::Arc<dyn DesktopActions>) {
    let catalog = Catalog::load(Locale::EnUs);
    println!("{}", catalog.message("app-foundation-ready"));
    println!("The complete desktop shell is currently available on Windows only.");
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Primitive {
    Button,
    IconButton,
    Link,
    Input,
    SearchField,
    Slider,
    Toggle,
    Select,
    Tooltip,
    Popover,
    ContextMenu,
    Dialog,
    Toast,
    Banner,
    Tabs,
    NavigationItem,
    ArtworkCard,
    TrackRow,
    Skeleton,
    EmptyState,
    FocusRing,
    ShortcutHint,
    VirtualList,
}
