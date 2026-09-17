use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use crate::{AlbumId, ArtistId, DeviceId, PlaylistId, SpotifyUri, TrackId, UserId};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Image {
    pub url: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ArtistSummary {
    pub id: ArtistId,
    pub name: String,
    pub external_url: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AlbumSummary {
    pub id: AlbumId,
    pub name: String,
    pub artists: Vec<ArtistSummary>,
    pub artwork: Vec<Image>,
    pub external_url: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Track {
    pub id: TrackId,
    pub uri: SpotifyUri,
    pub name: String,
    pub artists: Vec<ArtistSummary>,
    pub album: AlbumSummary,
    pub duration: Duration,
    pub explicit: bool,
    pub playable: bool,
    pub external_url: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PlaylistSummary {
    pub id: PlaylistId,
    pub name: String,
    pub owner_id: UserId,
    pub artwork: Vec<Image>,
    pub collaborative: bool,
    pub public: Option<bool>,
    pub item_count: u32,
    pub external_url: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Playlist {
    pub summary: PlaylistSummary,
    pub description: String,
    pub snapshot_id: String,
    pub tracks: Page<Track>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub offset: u32,
    pub limit: u32,
    pub total: u32,
    pub next: Option<String>,
}

impl<T> Page<T> {
    pub fn empty(limit: u32) -> Self {
        Self { items: Vec::new(), offset: 0, limit, total: 0, next: None }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct SearchResults {
    pub tracks: Option<Page<Track>>,
    pub albums: Option<Page<AlbumSummary>>,
    pub artists: Option<Page<ArtistSummary>>,
    pub playlists: Option<Page<PlaylistSummary>>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RepeatMode {
    #[default]
    Off,
    Context,
    Track,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Device {
    pub id: DeviceId,
    pub name: String,
    pub kind: String,
    pub active: bool,
    pub restricted: bool,
    pub volume_percent: Option<u8>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct Queue {
    pub now_playing: Option<Track>,
    pub upcoming: Vec<Track>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Cached<T> {
    pub value: T,
    pub fetched_at: SystemTime,
    pub expires_at: SystemTime,
    pub validator: Option<String>,
}

impl<T> Cached<T> {
    pub fn is_fresh_at(&self, now: SystemTime) -> bool {
        self.expires_at > now
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum PlaybackTarget {
    LocalEmbedded,
    SpotifyConnect,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum PlayRequest {
    Track(SpotifyUri),
    Context { context: SpotifyUri, offset: Option<SpotifyUri> },
}
