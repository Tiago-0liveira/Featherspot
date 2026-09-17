use std::time::Duration;

use mellowdeck_core::{AppError, ErrorKind, Result, SpotifyItemKind, SpotifyUri};
use reqwest::{StatusCode, blocking::Client};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::API_BASE_URL;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SpotifyDisplayItem {
    pub title: String,
    pub subtitle: String,
    pub uri: Option<String>,
    pub artwork_url: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpotifySearchItem {
    pub title: String,
    pub subtitle: String,
    pub kind: String,
    pub uri: Option<String>,
    pub artwork_url: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SpotifySection {
    pub items: Vec<SpotifyDisplayItem>,
    pub warning: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SpotifyHome {
    pub recently_played: SpotifySection,
    pub top_artists: SpotifySection,
    pub top_tracks: SpotifySection,
    pub saved_albums: SpotifySection,
    pub playlists: SpotifySection,
}

/// The user's full saved library: every saved album and every playlist, in one payload.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SpotifyLibrary {
    pub albums: SpotifySection,
    pub playlists: SpotifySection,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SpotifyPlayback {
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
pub struct SpotifyDevice {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub active: bool,
    pub restricted: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SpotifyQueue {
    pub current: Option<SpotifyDisplayItem>,
    pub upcoming: Vec<SpotifyDisplayItem>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SpotifyArtistDetail {
    pub name: String,
    pub description: String,
    pub uri: String,
    pub artwork_url: Option<String>,
    pub banner_url: Option<String>,
    pub is_album: bool,
    pub songs: Vec<SpotifyDisplayItem>,
    pub albums: Vec<SpotifyDisplayItem>,
    pub warning: Option<String>,
}

#[derive(Debug)]
pub struct SpotifyWebApi {
    client: Client,
}

impl SpotifyWebApi {
    /// Builds the authenticated Spotify Web API client.
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

    /// Loads each personalized Home collection independently.
    ///
    /// A restricted endpoint becomes a warning on its section so one capability change does not
    /// blank the entire Home page.
    ///
    /// # Errors
    ///
    /// Returns an error only when the supplied access token is empty.
    pub fn load_home(&self, access_token: &str) -> Result<SpotifyHome> {
        if access_token.is_empty() {
            return Err(AppError::new(ErrorKind::Authentication, "the active session is empty"));
        }
        Ok(SpotifyHome {
            recently_played: section(self.recently_played(access_token)),
            top_artists: section(self.top_artists(access_token)),
            top_tracks: section(self.top_tracks(access_token)),
            saved_albums: section(self.saved_albums(access_token, 4)),
            playlists: section(self.playlists(access_token, 4)),
        })
    }

    /// Loads the full saved library: saved albums and playlists, each up to 50 items.
    ///
    /// # Errors
    ///
    /// Returns an error only when the supplied access token is empty.
    pub fn load_library(&self, access_token: &str) -> Result<SpotifyLibrary> {
        if access_token.is_empty() {
            return Err(AppError::new(ErrorKind::Authentication, "the active session is empty"));
        }
        Ok(SpotifyLibrary {
            albums: section(self.saved_albums(access_token, 50)),
            playlists: section(self.playlists(access_token, 50)),
        })
    }

    /// Sets Spotify Connect shuffle state on the active device.
    ///
    /// # Errors
    ///
    /// Returns an error when Spotify rejects the command.
    pub fn shuffle(&self, token: &str, state: bool) -> Result<()> {
        Self::send_empty(
            self.client
                .put(format!("{API_BASE_URL}/me/player/shuffle"))
                .query(&[("state", if state { "true" } else { "false" })]),
            token,
        )
    }

    /// Searches the supported v1 music entity types.
    ///
    /// # Errors
    ///
    /// Returns an error for an empty query, expired authorization, endpoint restrictions, rate
    /// limiting, network failures, or malformed responses.
    pub fn search(&self, access_token: &str, query: &str) -> Result<Vec<SpotifySearchItem>> {
        let query = query.trim();
        if query.is_empty() {
            return Err(AppError::new(ErrorKind::InvalidInput, "enter something to search for"));
        }
        let response: SearchResponse = self.get(
            "/search",
            access_token,
            &[("q", query), ("type", "track,album,artist,playlist"), ("limit", "5")],
        )?;
        let mut results = Vec::new();
        results.extend(response.tracks.into_iter().flat_map(|page| {
            page.items.into_iter().map(|track| SpotifySearchItem {
                artwork_url: track.album.as_ref().and_then(|album| first_image(&album.images)),
                uri: track.uri.clone(),
                subtitle: artist_names(&track.artists),
                title: track.name,
                kind: "Track".into(),
            })
        }));
        results.extend(response.albums.into_iter().flat_map(|page| {
            page.items.into_iter().map(|album| SpotifySearchItem {
                artwork_url: first_image(&album.images),
                uri: album.uri.clone(),
                subtitle: artist_names(&album.artists),
                title: album.name,
                kind: "Album".into(),
            })
        }));
        results.extend(response.artists.into_iter().flat_map(|page| {
            page.items.into_iter().map(|artist| SpotifySearchItem {
                artwork_url: first_image(&artist.images),
                uri: artist.uri.clone(),
                subtitle: artist.genres.into_iter().take(2).collect::<Vec<_>>().join(" · "),
                title: artist.name,
                kind: "Artist".into(),
            })
        }));
        results.extend(response.playlists.into_iter().flat_map(|page| {
            page.items.into_iter().flatten().map(|playlist| SpotifySearchItem {
                artwork_url: first_image(&playlist.images),
                uri: playlist.uri.clone(),
                subtitle: playlist.items.map_or_else(
                    || "Spotify playlist".into(),
                    |items| format!("{} items", items.total),
                ),
                title: playlist.name,
                kind: "Playlist".into(),
            })
        }));
        Ok(results)
    }

    /// Loads an artist profile, releases, and supported song search results.
    ///
    /// Spotify removed the dedicated artist-top-tracks endpoint from Development Mode in 2026,
    /// so songs are sourced from catalog search relevance instead.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid artist URI or when the artist profile cannot be loaded.
    pub fn artist(&self, access_token: &str, uri: &str) -> Result<SpotifyArtistDetail> {
        let parsed = uri
            .parse::<SpotifyUri>()
            .map_err(|_| AppError::new(ErrorKind::InvalidInput, "invalid Spotify artist URI"))?;
        if parsed.kind() != SpotifyItemKind::Artist {
            return Err(AppError::new(ErrorKind::InvalidInput, "expected a Spotify artist URI"));
        }
        let artist: ArtistObject =
            self.get(&format!("/artists/{}", parsed.id()), access_token, &[])?;
        let albums = self.get::<SearchAlbumPage>(
            &format!("/artists/{}/albums", parsed.id()),
            access_token,
            &[("include_groups", "album,single"), ("limit", "10")],
        );
        let query = format!("artist:\"{}\"", artist.name.replace('"', ""));
        let songs = self.get::<SearchResponse>(
            "/search",
            access_token,
            &[("q", query.as_str()), ("type", "track"), ("limit", "10")],
        );
        let mut warnings = Vec::new();
        let albums = match albums {
            Ok(page) => page
                .items
                .into_iter()
                .map(|album| SpotifyDisplayItem {
                    artwork_url: first_image(&album.images),
                    uri: album.uri.clone(),
                    subtitle: artist_names(&album.artists),
                    title: album.name,
                })
                .collect(),
            Err(error) => {
                warnings.push(error.to_string());
                Vec::new()
            }
        };
        let songs = match songs {
            Ok(response) => response
                .tracks
                .into_iter()
                .flat_map(|page| page.items.into_iter().map(track_item))
                .collect(),
            Err(error) => {
                warnings.push(error.to_string());
                Vec::new()
            }
        };
        let artwork_url = first_image(&artist.images);
        Ok(SpotifyArtistDetail {
            description: if artist.genres.is_empty() {
                "Artist on Spotify".into()
            } else {
                artist.genres.join(" · ")
            },
            uri: artist.uri.unwrap_or_else(|| uri.to_owned()),
            banner_url: artwork_url.clone(),
            artwork_url,
            is_album: false,
            name: artist.name,
            songs,
            albums,
            warning: (!warnings.is_empty()).then(|| warnings.join(" · ")),
        })
    }

    /// Loads an album page: its cover, artists, and full track list.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid album URI or when the album cannot be loaded.
    pub fn album(&self, access_token: &str, uri: &str) -> Result<SpotifyArtistDetail> {
        let parsed = uri
            .parse::<SpotifyUri>()
            .map_err(|_| AppError::new(ErrorKind::InvalidInput, "invalid Spotify album URI"))?;
        if parsed.kind() != SpotifyItemKind::Album {
            return Err(AppError::new(ErrorKind::InvalidInput, "expected a Spotify album URI"));
        }
        let album: AlbumDetailObject =
            self.get(&format!("/albums/{}", parsed.id()), access_token, &[("limit", "50")])?;
        let cover = first_image(&album.images);
        // Album track objects omit their own artwork, so reuse the album cover for every row.
        let songs = album
            .tracks
            .map(|page| page.items)
            .unwrap_or_default()
            .into_iter()
            .map(|track| SpotifyDisplayItem {
                artwork_url: cover.clone(),
                uri: track.uri,
                title: track.name,
                subtitle: artist_names(&track.artists),
            })
            .collect();
        let description = {
            let artists = artist_names(&album.artists);
            match album.release_date.as_deref().and_then(|date| date.get(0..4)) {
                Some(year) if !artists.is_empty() => format!("{artists} · {year}"),
                Some(year) => year.to_owned(),
                None => artists,
            }
        };
        Ok(SpotifyArtistDetail {
            description,
            uri: album.uri.unwrap_or_else(|| uri.to_owned()),
            banner_url: cover.clone(),
            artwork_url: cover,
            is_album: true,
            name: album.name,
            songs,
            albums: Vec::new(),
            warning: None,
        })
    }

    /// Gets the current Spotify Connect playback state.
    ///
    /// # Errors
    ///
    /// Returns an error for expired authorization, restrictions, rate limits, network failures, or
    /// malformed responses.
    pub fn playback(&self, token: &str) -> Result<SpotifyPlayback> {
        let response = self
            .client
            .get(format!("{API_BASE_URL}/me/player"))
            .bearer_auth(token)
            .send()
            .map_err(network_error)?;
        if response.status() == StatusCode::NO_CONTENT {
            return Ok(SpotifyPlayback::default());
        }
        let status = response.status();
        if !status.is_success() {
            return Err(status_error(status));
        }
        let state: PlaybackResponse = response.json().map_err(|error| invalid_data(&error))?;
        let item = state.item;
        Ok(SpotifyPlayback {
            title: item.as_ref().map_or_else(String::new, |track| track.name.clone()),
            subtitle: item.as_ref().map_or_else(String::new, |track| artist_names(&track.artists)),
            artists: item.as_ref().map_or_else(Vec::new, |track| {
                track
                    .artists
                    .iter()
                    .map(|artist| (artist.name.clone(), artist.uri.clone()))
                    .collect()
            }),
            artwork_url: item
                .as_ref()
                .and_then(|track| track.album.as_ref())
                .and_then(|album| first_image(&album.images)),
            playing: state.is_playing,
            progress_ms: state.progress_ms.unwrap_or_default(),
            duration_ms: item.and_then(|track| track.duration_ms).unwrap_or_default(),
            volume_percent: state.device.as_ref().and_then(|device| device.volume_percent),
            device_name: state.device.map(|device| device.name),
        })
    }

    /// Gets available Spotify Connect devices.
    ///
    /// # Errors
    ///
    /// Returns an error for authorization, restrictions, rate limits, networking, or invalid data.
    pub fn devices(&self, token: &str) -> Result<Vec<SpotifyDevice>> {
        let response: DevicesResponse = self.get("/me/player/devices", token, &[])?;
        Ok(response
            .devices
            .into_iter()
            .filter_map(|device| {
                device.id.map(|id| SpotifyDevice {
                    id,
                    name: device.name,
                    kind: device.kind,
                    active: device.is_active,
                    restricted: device.is_restricted,
                })
            })
            .collect())
    }

    /// Gets the readable Spotify queue.
    ///
    /// # Errors
    ///
    /// Returns an error for authorization, restrictions, rate limits, networking, or invalid data.
    pub fn queue(&self, token: &str) -> Result<SpotifyQueue> {
        let response: QueueResponse = self.get("/me/player/queue", token, &[])?;
        Ok(SpotifyQueue {
            current: response.currently_playing.map(track_item),
            upcoming: response.queue.into_iter().map(track_item).collect(),
        })
    }

    /// Starts a track or context URI on the active Spotify Connect device.
    ///
    /// # Errors
    ///
    /// Returns an error when the URI is unsupported or Spotify rejects the command.
    pub fn play_uri(&self, token: &str, uri: &str) -> Result<()> {
        self.play_uri_on_device(token, uri, None)
    }

    /// Starts a URI on a specific Spotify Connect device.
    ///
    /// # Errors
    ///
    /// Returns an error when the URI is unsupported or Spotify rejects the command.
    pub fn play_uri_on_device(
        &self,
        token: &str,
        uri: &str,
        device_id: Option<&str>,
    ) -> Result<()> {
        let body = if uri.starts_with("spotify:track:") {
            PlayBody { uris: Some(vec![uri]), context_uri: None }
        } else if uri.starts_with("spotify:album:")
            || uri.starts_with("spotify:artist:")
            || uri.starts_with("spotify:playlist:")
        {
            PlayBody { uris: None, context_uri: Some(uri) }
        } else {
            return Err(AppError::new(ErrorKind::InvalidInput, "unsupported Spotify play URI"));
        };
        let mut request = self.client.put(format!("{API_BASE_URL}/me/player/play"));
        if let Some(device_id) = device_id {
            request = request.query(&[("device_id", device_id)]);
        }
        Self::send_command(request, token, &body)
    }

    /// Resumes the active Spotify Connect device.
    ///
    /// # Errors
    ///
    /// Returns an error when Spotify rejects the command.
    pub fn resume(&self, token: &str) -> Result<()> {
        Self::send_empty(self.client.put(format!("{API_BASE_URL}/me/player/play")), token)
    }

    /// Pauses the active Spotify Connect device.
    ///
    /// # Errors
    ///
    /// Returns an error when Spotify rejects the command.
    pub fn pause(&self, token: &str) -> Result<()> {
        Self::send_empty(self.client.put(format!("{API_BASE_URL}/me/player/pause")), token)
    }

    /// Skips to the next queue item.
    ///
    /// # Errors
    ///
    /// Returns an error when Spotify rejects the command.
    pub fn next(&self, token: &str) -> Result<()> {
        Self::send_empty(self.client.post(format!("{API_BASE_URL}/me/player/next")), token)
    }

    /// Skips to the previous queue item.
    ///
    /// # Errors
    ///
    /// Returns an error when Spotify rejects the command.
    pub fn previous(&self, token: &str) -> Result<()> {
        Self::send_empty(self.client.post(format!("{API_BASE_URL}/me/player/previous")), token)
    }

    /// Seeks the active item.
    ///
    /// # Errors
    ///
    /// Returns an error when Spotify rejects the command.
    pub fn seek(&self, token: &str, position_ms: u64) -> Result<()> {
        Self::send_empty(
            self.client
                .put(format!("{API_BASE_URL}/me/player/seek"))
                .query(&[("position_ms", position_ms)]),
            token,
        )
    }

    /// Sets active-device volume.
    ///
    /// # Errors
    ///
    /// Returns an error when Spotify rejects the command.
    pub fn volume(&self, token: &str, percent: u8) -> Result<()> {
        Self::send_empty(
            self.client
                .put(format!("{API_BASE_URL}/me/player/volume"))
                .query(&[("volume_percent", percent.min(100))]),
            token,
        )
    }

    /// Sets the repeat mode for the active Spotify Connect device.
    ///
    /// # Errors
    ///
    /// Returns an error for an unsupported mode or when Spotify rejects the command.
    pub fn repeat(&self, token: &str, mode: &str) -> Result<()> {
        if !matches!(mode, "off" | "context" | "track") {
            return Err(AppError::new(ErrorKind::InvalidInput, "unsupported repeat mode"));
        }
        Self::send_empty(
            self.client.put(format!("{API_BASE_URL}/me/player/repeat")).query(&[("state", mode)]),
            token,
        )
    }

    /// Adds one validated track to the active Spotify Connect queue.
    ///
    /// # Errors
    ///
    /// Returns an error when `uri` is not a Spotify track URI or Spotify rejects the command.
    pub fn enqueue(&self, token: &str, uri: &str) -> Result<()> {
        let parsed = uri.parse::<SpotifyUri>().map_err(|_| {
            AppError::new(ErrorKind::InvalidInput, "invalid Spotify URI for queue insertion")
        })?;
        if parsed.kind() != SpotifyItemKind::Track {
            return Err(AppError::new(
                ErrorKind::InvalidInput,
                "only tracks can be added to queue",
            ));
        }
        Self::send_empty(
            self.client.post(format!("{API_BASE_URL}/me/player/queue")).query(&[("uri", uri)]),
            token,
        )
    }

    /// Transfers Spotify playback to one selected device.
    ///
    /// # Errors
    ///
    /// Returns an error when the identifier is empty or Spotify rejects the command.
    pub fn transfer(&self, token: &str, device_id: &str) -> Result<()> {
        if device_id.is_empty() {
            return Err(AppError::new(ErrorKind::InvalidInput, "device identifier is empty"));
        }
        Self::send_command(
            self.client.put(format!("{API_BASE_URL}/me/player")),
            token,
            &TransferBody { device_ids: [device_id], play: false },
        )
    }

    fn recently_played(&self, token: &str) -> Result<Vec<SpotifyDisplayItem>> {
        let page: HistoryPage = self.get("/me/player/recently-played", token, &[("limit", "4")])?;
        Ok(page.items.into_iter().map(|item| track_item(item.track)).collect())
    }

    fn top_artists(&self, token: &str) -> Result<Vec<SpotifyDisplayItem>> {
        let page: ArtistPage =
            self.get("/me/top/artists", token, &[("limit", "4"), ("time_range", "medium_term")])?;
        Ok(page
            .items
            .into_iter()
            .map(|artist| SpotifyDisplayItem {
                artwork_url: first_image(&artist.images),
                uri: artist.uri.clone(),
                subtitle: artist.genres.into_iter().take(2).collect::<Vec<_>>().join(" · "),
                title: artist.name,
            })
            .collect())
    }

    fn top_tracks(&self, token: &str) -> Result<Vec<SpotifyDisplayItem>> {
        let page: TrackPage =
            self.get("/me/top/tracks", token, &[("limit", "4"), ("time_range", "medium_term")])?;
        Ok(page.items.into_iter().map(track_item).collect())
    }

    fn saved_albums(&self, token: &str, limit: u32) -> Result<Vec<SpotifyDisplayItem>> {
        let page: AlbumPage =
            self.get("/me/albums", token, &[("limit", limit.to_string().as_str())])?;
        Ok(page
            .items
            .into_iter()
            .map(|item| SpotifyDisplayItem {
                artwork_url: first_image(&item.album.images),
                uri: item.album.uri.clone(),
                subtitle: artist_names(&item.album.artists),
                title: item.album.name,
            })
            .collect())
    }

    fn playlists(&self, token: &str, limit: u32) -> Result<Vec<SpotifyDisplayItem>> {
        let page: PlaylistPage =
            self.get("/me/playlists", token, &[("limit", limit.to_string().as_str())])?;
        Ok(page
            .items
            .into_iter()
            .map(|playlist| SpotifyDisplayItem {
                artwork_url: first_image(&playlist.images),
                uri: playlist.uri.clone(),
                subtitle: playlist
                    .items
                    .map_or_else(|| "Playlist".into(), |items| format!("{} items", items.total)),
                title: playlist.name,
            })
            .collect())
    }

    fn send_empty(request: reqwest::blocking::RequestBuilder, token: &str) -> Result<()> {
        // Spotify's front end rejects body-less PUT/POST requests with HTTP 411 unless the
        // request states an explicit zero length.
        let response = request
            .bearer_auth(token)
            .header(reqwest::header::CONTENT_LENGTH, "0")
            .body(Vec::new())
            .send()
            .map_err(network_error)?;
        if response.status().is_success() { Ok(()) } else { Err(status_error(response.status())) }
    }

    fn send_command(
        request: reqwest::blocking::RequestBuilder,
        token: &str,
        body: &impl Serialize,
    ) -> Result<()> {
        let response = request.bearer_auth(token).json(body).send().map_err(network_error)?;
        if response.status().is_success() { Ok(()) } else { Err(status_error(response.status())) }
    }

    fn get<T: DeserializeOwned>(
        &self,
        path: &str,
        token: &str,
        query: &[(&str, &str)],
    ) -> Result<T> {
        let response = self
            .client
            .get(format!("{API_BASE_URL}{path}"))
            .bearer_auth(token)
            .query(query)
            .send()
            .map_err(network_error)?;
        let status = response.status();
        if !status.is_success() {
            return Err(status_error(status));
        }
        response.json().map_err(|error| {
            AppError::new(ErrorKind::Network, format!("Spotify returned invalid data: {error}"))
        })
    }
}

fn section(result: Result<Vec<SpotifyDisplayItem>>) -> SpotifySection {
    match result {
        Ok(items) => SpotifySection { items, warning: None },
        Err(error) => SpotifySection { items: Vec::new(), warning: Some(error.to_string()) },
    }
}

fn track_item(track: TrackObject) -> SpotifyDisplayItem {
    SpotifyDisplayItem {
        artwork_url: track.album.as_ref().and_then(|album| first_image(&album.images)),
        uri: track.uri,
        title: track.name,
        subtitle: artist_names(&track.artists),
    }
}

fn artist_names(artists: &[ArtistObject]) -> String {
    artists.iter().map(|artist| artist.name.as_str()).collect::<Vec<_>>().join(", ")
}

fn first_image(images: &[ImageObject]) -> Option<String> {
    images.first().map(|image| image.url.clone())
}

fn status_error(status: StatusCode) -> AppError {
    let (kind, message) = match status.as_u16() {
        401 => (ErrorKind::Authentication, "Spotify session expired"),
        403 => {
            (ErrorKind::Restricted, "This action is unavailable for this Spotify account or app")
        }
        404 => (
            ErrorKind::Unavailable,
            "No active Spotify device or requested item was found; open Spotify on a device first",
        ),
        429 => (ErrorKind::RateLimited, "Spotify request limit reached; try again shortly"),
        500..=599 => (ErrorKind::Unavailable, "Spotify is temporarily unavailable"),
        _ => (ErrorKind::Network, "Spotify could not complete this request"),
    };
    AppError::new(kind, format!("{message} (HTTP {})", status.as_u16()))
}

fn network_error(error: reqwest::Error) -> AppError {
    let message = error.to_string();
    drop(error);
    AppError::new(ErrorKind::Network, message)
}

fn invalid_data(error: &reqwest::Error) -> AppError {
    AppError::new(ErrorKind::Network, format!("Spotify returned invalid data: {error}"))
}

#[derive(Serialize)]
struct PlayBody<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    uris: Option<Vec<&'a str>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    context_uri: Option<&'a str>,
}

#[derive(Serialize)]
struct TransferBody<'a> {
    device_ids: [&'a str; 1],
    play: bool,
}

#[derive(Deserialize)]
struct PlaybackResponse {
    #[serde(default)]
    is_playing: bool,
    progress_ms: Option<u64>,
    item: Option<TrackObject>,
    device: Option<DeviceObject>,
}

#[derive(Deserialize)]
struct DevicesResponse {
    #[serde(default)]
    devices: Vec<DeviceObject>,
}

#[derive(Deserialize)]
struct DeviceObject {
    id: Option<String>,
    name: String,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    is_active: bool,
    #[serde(default)]
    is_restricted: bool,
    volume_percent: Option<u8>,
}

#[derive(Deserialize)]
struct QueueResponse {
    currently_playing: Option<TrackObject>,
    #[serde(default)]
    queue: Vec<TrackObject>,
}

#[derive(Deserialize)]
struct HistoryPage {
    #[serde(default)]
    items: Vec<HistoryItem>,
}

#[derive(Deserialize)]
struct HistoryItem {
    track: TrackObject,
}

#[derive(Deserialize)]
struct TrackPage {
    #[serde(default)]
    items: Vec<TrackObject>,
}

#[derive(Deserialize)]
struct TrackObject {
    name: String,
    uri: Option<String>,
    album: Option<AlbumObject>,
    duration_ms: Option<u64>,
    #[serde(default)]
    artists: Vec<ArtistObject>,
}

#[derive(Deserialize)]
struct ArtistPage {
    #[serde(default)]
    items: Vec<ArtistObject>,
}

#[derive(Deserialize)]
struct ArtistObject {
    name: String,
    uri: Option<String>,
    #[serde(default)]
    images: Vec<ImageObject>,
    #[serde(default)]
    genres: Vec<String>,
}

#[derive(Deserialize)]
struct AlbumPage {
    #[serde(default)]
    items: Vec<SavedAlbum>,
}

#[derive(Deserialize)]
struct SavedAlbum {
    album: AlbumObject,
}

#[derive(Deserialize)]
struct AlbumObject {
    name: String,
    uri: Option<String>,
    #[serde(default)]
    images: Vec<ImageObject>,
    #[serde(default)]
    artists: Vec<ArtistObject>,
}

#[derive(Deserialize)]
struct AlbumDetailObject {
    name: String,
    uri: Option<String>,
    release_date: Option<String>,
    #[serde(default)]
    images: Vec<ImageObject>,
    #[serde(default)]
    artists: Vec<ArtistObject>,
    tracks: Option<TrackPage>,
}

#[derive(Deserialize)]
struct PlaylistPage {
    #[serde(default)]
    items: Vec<PlaylistObject>,
}

#[derive(Deserialize)]
struct PlaylistObject {
    name: String,
    uri: Option<String>,
    #[serde(default)]
    images: Vec<ImageObject>,
    items: Option<PlaylistItems>,
}

#[derive(Deserialize)]
struct ImageObject {
    url: String,
}

#[derive(Deserialize)]
struct PlaylistItems {
    total: u32,
}

#[derive(Deserialize)]
struct SearchResponse {
    tracks: Option<TrackPage>,
    albums: Option<SearchAlbumPage>,
    artists: Option<ArtistPage>,
    playlists: Option<SearchPlaylistPage>,
}

#[derive(Deserialize)]
struct SearchAlbumPage {
    #[serde(default)]
    items: Vec<AlbumObject>,
}

#[derive(Deserialize)]
struct SearchPlaylistPage {
    #[serde(default)]
    items: Vec<Option<PlaylistObject>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_playlist_shape_uses_items_count() {
        let page: PlaylistPage =
            serde_json::from_str(r#"{"items":[{"name":"Focus","items":{"total":12}}]}"#).unwrap();
        assert_eq!(page.items[0].items.as_ref().unwrap().total, 12);
    }

    #[test]
    fn removed_artist_fields_are_optional() {
        let page: ArtistPage = serde_json::from_str(r#"{"items":[{"name":"Rosalía"}]}"#).unwrap();
        assert!(page.items[0].genres.is_empty());
    }

    #[test]
    fn album_detail_shape_reads_tracks_and_release_date() {
        let album: AlbumDetailObject = serde_json::from_str(
            r#"{
                "name":"Motomami",
                "uri":"spotify:album:abc",
                "release_date":"2022-03-18",
                "artists":[{"name":"Rosalía"}],
                "tracks":{"items":[
                    {"name":"Saoko","uri":"spotify:track:1"},
                    {"name":"Candy","uri":"spotify:track:2"}
                ]}
            }"#,
        )
        .unwrap();
        assert_eq!(album.name, "Motomami");
        assert_eq!(album.release_date.as_deref(), Some("2022-03-18"));
        assert_eq!(album.tracks.unwrap().items.len(), 2);
    }

    #[test]
    fn search_tolerates_unavailable_playlist_entries() {
        let response: SearchResponse = serde_json::from_str(
            r#"{"tracks":{"items":[]},"playlists":{"items":[null,{"name":"Mix","items":null}]}}"#,
        )
        .unwrap();
        assert_eq!(response.playlists.unwrap().items.len(), 2);
    }

    #[test]
    fn playback_shape_keeps_device_and_track_timing() {
        let response: PlaybackResponse = serde_json::from_str(
            r#"{
                "is_playing":true,
                "progress_ms":42000,
                "device":{"id":"speaker","name":"Office","type":"Speaker","volume_percent":65},
                "item":{"name":"Song","duration_ms":180000,"artists":[{"name":"Artist"}]}
            }"#,
        )
        .unwrap();
        assert!(response.is_playing);
        assert_eq!(response.progress_ms, Some(42_000));
        assert_eq!(response.device.unwrap().volume_percent, Some(65));
        assert_eq!(response.item.unwrap().duration_ms, Some(180_000));
    }

    #[test]
    fn queue_shape_tolerates_episode_like_items() {
        let response: QueueResponse = serde_json::from_str(
            r#"{"currently_playing":null,"queue":[{"name":"Episode","uri":"spotify:episode:abc"}]}"#,
        )
        .unwrap();
        assert_eq!(response.queue.len(), 1);
        assert!(response.queue[0].artists.is_empty());
    }
}
