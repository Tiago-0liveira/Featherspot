use std::{future::Future, pin::Pin, time::Duration};

use crate::{
    AlbumId, AlbumSummary, AppError, ArtistId, Cached, Device, DeviceId, Page, PlayRequest,
    Playlist, PlaylistId, PlaylistSummary, Queue, RepeatMode, Result, SearchResults, SpotifyUri,
    Track, TrackId,
};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountCapabilities {
    pub product: ProductTier,
    pub embedded_playback: CapabilityAvailability,
    pub library_mutation: CapabilityAvailability,
    pub playlist_mutation: CapabilityAvailability,
    pub connect_control: CapabilityAvailability,
    pub restrictions: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProductTier {
    Premium,
    Other,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CapabilityAvailability {
    Available,
    Unavailable,
    Unknown,
}

pub trait SessionService: Send + Sync {
    fn authorize<'a>(&'a self, client_id: &'a str) -> BoxFuture<'a, Result<AccountCapabilities>>;
    fn refresh(&self) -> BoxFuture<'_, Result<()>>;
    fn logout(&self) -> BoxFuture<'_, Result<()>>;
    fn capabilities(&self) -> BoxFuture<'_, Result<AccountCapabilities>>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SearchType {
    Track,
    Album,
    Artist,
    Playlist,
}

pub trait CatalogService: Send + Sync {
    fn search<'a>(
        &'a self,
        query: &'a str,
        kinds: &'a [SearchType],
        offset: u32,
        limit: u32,
    ) -> BoxFuture<'a, Result<SearchResults>>;
    fn track(&self, id: TrackId) -> BoxFuture<'_, Result<Track>>;
    fn album(&self, id: AlbumId) -> BoxFuture<'_, Result<AlbumSummary>>;
    fn playlist(&self, id: PlaylistId, offset: u32, limit: u32) -> BoxFuture<'_, Result<Playlist>>;
}

pub trait LibraryService: Send + Sync {
    fn liked_tracks(&self, offset: u32, limit: u32) -> BoxFuture<'_, Result<Page<Track>>>;
    fn saved_albums(&self, offset: u32, limit: u32) -> BoxFuture<'_, Result<Page<AlbumSummary>>>;
    fn save(&self, uris: Vec<SpotifyUri>) -> BoxFuture<'_, Result<()>>;
    fn remove(&self, uris: Vec<SpotifyUri>) -> BoxFuture<'_, Result<()>>;
    fn contains(&self, uris: Vec<SpotifyUri>) -> BoxFuture<'_, Result<Vec<bool>>>;
    fn follow_artists(&self, _ids: Vec<ArtistId>) -> BoxFuture<'_, Result<()>> {
        Box::pin(std::future::ready(Ok(())))
    }
    fn unfollow_artists(&self, _ids: Vec<ArtistId>) -> BoxFuture<'_, Result<()>> {
        Box::pin(std::future::ready(Ok(())))
    }
    fn is_following_artists(&self, ids: Vec<ArtistId>) -> BoxFuture<'_, Result<Vec<bool>>> {
        let count = ids.len();
        Box::pin(std::future::ready(Ok(vec![false; count])))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlaylistMetadata {
    pub name: String,
    pub description: String,
    pub public: Option<bool>,
    pub collaborative: bool,
}

pub trait PlaylistService: Send + Sync {
    fn list(&self, offset: u32, limit: u32) -> BoxFuture<'_, Result<Page<PlaylistSummary>>>;
    fn create(&self, metadata: PlaylistMetadata) -> BoxFuture<'_, Result<Playlist>>;
    fn update(&self, id: PlaylistId, metadata: PlaylistMetadata) -> BoxFuture<'_, Result<()>>;
    fn add(
        &self,
        id: PlaylistId,
        uris: Vec<SpotifyUri>,
        position: Option<u32>,
    ) -> BoxFuture<'_, Result<String>>;
    fn remove(
        &self,
        id: PlaylistId,
        uris: Vec<SpotifyUri>,
        snapshot: String,
    ) -> BoxFuture<'_, Result<String>>;
    fn reorder(
        &self,
        id: PlaylistId,
        from: u32,
        to: u32,
        length: u32,
        snapshot: String,
    ) -> BoxFuture<'_, Result<String>>;
    fn follow(&self, id: PlaylistId, public: bool) -> BoxFuture<'_, Result<()>>;
    fn unfollow(&self, id: PlaylistId) -> BoxFuture<'_, Result<()>>;
}

pub trait PlaybackService: Send + Sync {
    fn play(&self, request: PlayRequest, target: Option<DeviceId>) -> BoxFuture<'_, Result<()>>;
    fn pause(&self) -> BoxFuture<'_, Result<()>>;
    fn resume(&self) -> BoxFuture<'_, Result<()>>;
    fn previous(&self) -> BoxFuture<'_, Result<()>>;
    fn next(&self) -> BoxFuture<'_, Result<()>>;
    fn seek(&self, position: Duration) -> BoxFuture<'_, Result<()>>;
    fn shuffle(&self, enabled: bool) -> BoxFuture<'_, Result<()>>;
    fn repeat(&self, mode: RepeatMode) -> BoxFuture<'_, Result<()>>;
    fn volume(&self, percent: u8) -> BoxFuture<'_, Result<()>>;
    fn queue(&self) -> BoxFuture<'_, Result<Queue>>;
    fn enqueue(&self, uri: SpotifyUri) -> BoxFuture<'_, Result<()>>;
    fn devices(&self) -> BoxFuture<'_, Result<Vec<Device>>>;
    fn transfer(&self, device: DeviceId, start_playing: bool) -> BoxFuture<'_, Result<()>>;
}

pub trait CacheRepository: Send + Sync {
    fn read(&self, key: String) -> BoxFuture<'_, Result<Option<Cached<Vec<u8>>>>>;
    fn write(&self, key: String, value: Cached<Vec<u8>>) -> BoxFuture<'_, Result<()>>;
    fn invalidate(&self, prefix: String) -> BoxFuture<'_, Result<usize>>;
    fn size_bytes(&self) -> BoxFuture<'_, Result<u64>>;
    fn trim_to(&self, maximum_bytes: u64) -> BoxFuture<'_, Result<u64>>;
    fn clear(&self) -> BoxFuture<'_, Result<()>>;
}

pub trait CredentialStore: Send + Sync {
    /// Loads the refresh token for the single supported account.
    ///
    /// # Errors
    ///
    /// Returns an error when the platform credential vault cannot be read.
    fn load_refresh_token(&self) -> Result<Option<String>>;
    /// Replaces the stored refresh token.
    ///
    /// # Errors
    ///
    /// Returns an error when the value is invalid or the platform vault cannot be written.
    fn store_refresh_token(&self, refresh_token: &str) -> Result<()>;
    /// Removes any stored refresh token.
    ///
    /// # Errors
    ///
    /// Returns an error when the platform vault cannot remove the credential.
    fn clear_refresh_token(&self) -> Result<()>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MediaMetadata {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub artwork_url: Option<String>,
    pub duration: Duration,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MediaButton {
    Play,
    Pause,
    Next,
    Previous,
}

pub trait MediaSession: Send + Sync {
    /// Replaces the metadata exposed to the operating system.
    ///
    /// # Errors
    ///
    /// Returns an error when the operating-system media session rejects the update.
    fn publish(&self, metadata: Option<MediaMetadata>) -> Result<()>;
    /// Updates the operating-system playback state and timeline.
    ///
    /// # Errors
    ///
    /// Returns an error when the operating-system media session rejects the update.
    fn set_playback(&self, playing: bool, position: Duration) -> Result<()>;
    /// Takes the next hardware media-button event, if one is pending.
    ///
    /// # Errors
    ///
    /// Returns an error when the operating-system event source cannot be queried.
    fn take_button_event(&self) -> Result<Option<MediaButton>>;
    /// Clears all metadata and releases the active operating-system media session.
    ///
    /// # Errors
    ///
    /// Returns an error when the operating-system media session cannot be cleared.
    fn clear(&self) -> Result<()>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LocalPlayerCommand {
    TokenUpdate { access_token: String },
    Connect,
    Disconnect,
    Play,
    Pause,
    Seek { position_ms: u64 },
    Volume { value_milli: u16 },
    Shutdown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LocalPlayerEvent {
    Ready {
        device_id: DeviceId,
    },
    Unavailable,
    StateChanged {
        playing: bool,
        position_ms: u64,
        duration_ms: u64,
        track_uri: Option<SpotifyUri>,
        title: Option<String>,
        artist: Option<String>,
        album: Option<String>,
        artwork_url: Option<String>,
    },
    AuthenticationError(String),
    PlaybackError(String),
    AccountError(String),
}

pub trait LocalPlayerHost: Send + Sync {
    fn create(&self) -> BoxFuture<'_, Result<()>>;
    fn send(&self, command: LocalPlayerCommand) -> BoxFuture<'_, Result<()>>;
    fn next_event(&self) -> BoxFuture<'_, Result<LocalPlayerEvent>>;
    fn destroy(&self) -> BoxFuture<'_, Result<()>>;
}

pub fn unavailable(feature: &str) -> AppError {
    AppError::new(crate::ErrorKind::Unavailable, format!("{feature} is unavailable"))
}
