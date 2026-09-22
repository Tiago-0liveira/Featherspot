use std::{
    sync::mpsc::{self, Receiver, SyncSender, TryRecvError},
    thread,
    time::Duration,
};

use mellowdeck_core::{AppError, ErrorKind, Result, SpotifyItemKind, SpotifyUri};
use mellowdeck_spotify::{
    SpotifyBrowseItem, SpotifyContentState, SpotifyDetailPage, SpotifyDisplayItem,
    SpotifyEntityKind, SpotifyPlayback, SpotifyTypedQueue, SpotifyWebApi,
};

use crate::{
    Effect,
    state::{BrowseItem, ContextPosition, EntityKind, LoadState, PageState, Route, Section},
};

const PAGE_SIZE: u32 = 50;

#[derive(Debug)]
pub enum ServiceResponse {
    Page(PageState),
    Playback(SpotifyPlayback),
    Queue { now: Option<BrowseItem>, upcoming: Vec<BrowseItem> },
    Command { effect: Effect, result: Result<()> },
}

#[derive(Debug)]
enum Request {
    Effect(Effect),
    Token(String),
    Shutdown,
}

#[derive(Debug)]
pub struct ServiceHandle {
    priority: SyncSender<Request>,
    browse: SyncSender<Request>,
    responses: Receiver<ServiceResponse>,
    worker: Option<thread::JoinHandle<()>>,
}

impl ServiceHandle {
    /// Starts the serialized Spotify background worker.
    ///
    /// # Errors
    /// Returns an error if the HTTP client or worker thread cannot be created.
    pub fn start(access_token: String) -> Result<Self> {
        let api = SpotifyWebApi::new()?;
        let (priority_tx, priority_rx) = mpsc::sync_channel(64);
        let (browse_tx, browse_rx) = mpsc::sync_channel(8);
        let (response_tx, response_rx) = mpsc::sync_channel(64);
        let worker = thread::Builder::new()
            .name("mellowdeck-spotify".into())
            .spawn(move || {
                worker_loop(api, access_token, &priority_rx, &browse_rx, &response_tx);
            })
            .map_err(|error| {
                AppError::new(
                    ErrorKind::Unavailable,
                    format!("could not start Spotify worker: {error}"),
                )
            })?;
        Ok(Self {
            priority: priority_tx,
            browse: browse_tx,
            responses: response_rx,
            worker: Some(worker),
        })
    }

    /// Queues a browse, polling, or playback effect.
    ///
    /// # Errors
    /// Returns an error if the bounded queue is full or the worker stopped.
    pub fn send(&self, effect: Effect) -> Result<()> {
        let priority = matches!(
            effect,
            Effect::TogglePlayback
                | Effect::Previous
                | Effect::Next
                | Effect::Seek(_)
                | Effect::Volume(_)
                | Effect::Shuffle(_)
                | Effect::Repeat(_)
                | Effect::PlayTrack { .. }
                | Effect::PlayContext { .. }
                | Effect::Enqueue(_)
                | Effect::Transfer(_)
                | Effect::RefreshPlayback
                | Effect::RefreshQueue
        );
        if priority {
            self.priority.send(Request::Effect(effect)).map_err(channel_error)
        } else {
            self.browse.try_send(Request::Effect(effect)).map_err(|error| {
                AppError::new(ErrorKind::Unavailable, format!("background queue is busy: {error}"))
            })
        }
    }

    /// Replaces the access token used by subsequent requests.
    ///
    /// # Errors
    /// Returns an error if the worker stopped.
    pub fn update_token(&self, token: String) -> Result<()> {
        self.priority.send(Request::Token(token)).map_err(channel_error)
    }
    pub fn try_recv(&self) -> Option<ServiceResponse> {
        self.responses.try_recv().ok()
    }
}

impl Drop for ServiceHandle {
    fn drop(&mut self) {
        let _ = self.priority.send(Request::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[allow(clippy::needless_pass_by_value)]
fn worker_loop(
    api: SpotifyWebApi,
    mut token: String,
    priority: &Receiver<Request>,
    browse: &Receiver<Request>,
    responses: &SyncSender<ServiceResponse>,
) {
    loop {
        let request = match priority.try_recv() {
            Ok(request) => request,
            Err(TryRecvError::Disconnected) => break,
            Err(TryRecvError::Empty) => match browse.recv_timeout(Duration::from_millis(50)) {
                Ok(request) => request,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            },
        };
        match request {
            Request::Shutdown => break,
            Request::Token(value) => token = value,
            Request::Effect(effect) => {
                let response = perform(&api, &token, effect);
                if responses.send(response).is_err() {
                    break;
                }
            }
        }
    }
}

fn perform(api: &SpotifyWebApi, token: &str, effect: Effect) -> ServiceResponse {
    match effect.clone() {
        Effect::LoadPage { route, generation, offset, query } => {
            match load_page(api, token, route, generation, offset, &query) {
                Ok(page) => ServiceResponse::Page(page),
                Err(error) => ServiceResponse::Command { effect, result: Err(error) },
            }
        }
        Effect::RefreshPlayback => match api.playback(token) {
            Ok(playback) => ServiceResponse::Playback(playback),
            Err(error) => ServiceResponse::Command { effect, result: Err(error) },
        },
        Effect::RefreshQueue => match api.typed_queue(token) {
            Ok(queue) => queue_response(queue),
            Err(error) => ServiceResponse::Command { effect, result: Err(error) },
        },
        Effect::TogglePlayback => {
            let result = api.playback(token).and_then(|snapshot| {
                if snapshot.playing { api.pause(token) } else { api.resume(token) }
            });
            ServiceResponse::Command { effect, result }
        }
        Effect::Previous => ServiceResponse::Command { effect, result: api.previous(token) },
        Effect::Next => ServiceResponse::Command { effect, result: api.next(token) },
        Effect::Seek(delta) => {
            let result = api.playback(token).and_then(|snapshot| {
                let current = i64::try_from(snapshot.progress_ms).unwrap_or(i64::MAX);
                api.seek(token, current.saturating_add(delta).max(0).cast_unsigned())
            });
            ServiceResponse::Command { effect, result }
        }
        Effect::Volume(value) => {
            ServiceResponse::Command { effect, result: api.volume(token, value) }
        }
        Effect::Shuffle(value) => {
            ServiceResponse::Command { effect, result: api.shuffle(token, value) }
        }
        Effect::Repeat(value) => {
            let mode = match value {
                1 => "context",
                2 => "track",
                _ => "off",
            };
            ServiceResponse::Command { effect, result: api.repeat(token, mode) }
        }
        Effect::PlayTrack { uri, device_id } => {
            let result = play_track_with_retry(api, token, &uri, device_id.as_deref());
            ServiceResponse::Command { effect, result }
        }
        Effect::PlayContext { uri, position, device_id } => {
            let result = play_context_with_retry(api, token, &uri, position, device_id.as_deref());
            ServiceResponse::Command { effect, result }
        }
        Effect::Enqueue(uri) => {
            ServiceResponse::Command { effect, result: api.enqueue(token, &uri) }
        }
        Effect::Transfer(id) => {
            ServiceResponse::Command { effect, result: api.transfer(token, &id) }
        }
        Effect::OpenExternal(_) | Effect::SaveSettings => {
            ServiceResponse::Command { effect, result: Ok(()) }
        }
    }
}

#[allow(clippy::too_many_lines, clippy::needless_pass_by_value)]
fn load_page(
    api: &SpotifyWebApi,
    token: &str,
    route: Route,
    generation: u64,
    offset: u32,
    query: &str,
) -> Result<PageState> {
    let mut page = PageState::loading(route.clone(), generation);
    match &route {
        Route::Home => {
            let home = api.load_home(token)?;
            page.title = "Home".into();
            page.subtitle = "Your listening, kept in distinct collections".into();
            page.sections = vec![
                display_section("Recently played", home.recently_played.items),
                display_section("Top tracks", home.top_tracks.items),
                display_section("Top artists", home.top_artists.items),
                display_section("Saved albums", home.saved_albums.items),
                display_section("Playlists", home.playlists.items),
            ];
            let warnings = [
                home.recently_played.warning,
                home.top_tracks.warning,
                home.top_artists.warning,
                home.saved_albums.warning,
                home.playlists.warning,
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
            page.state = if warnings.is_empty() {
                ready_or_empty(&page)
            } else {
                LoadState::Partial(warnings.join(" · "))
            };
        }
        Route::Library => {
            let (albums, playlists) = std::thread::scope(|s| {
                let albums_h = s.spawn(|| api.saved_albums_page(token, offset, PAGE_SIZE));
                let playlists_h = s.spawn(|| api.playlists_page(token, offset, PAGE_SIZE));
                (
                    albums_h.join().unwrap_or_else(|_| {
                        Err(mellowdeck_core::AppError::new(
                            mellowdeck_core::ErrorKind::Unavailable,
                            "album fetch worker panicked",
                        ))
                    }),
                    playlists_h.join().unwrap_or_else(|_| {
                        Err(mellowdeck_core::AppError::new(
                            mellowdeck_core::ErrorKind::Unavailable,
                            "playlist fetch worker panicked",
                        ))
                    }),
                )
            });
            let albums = albums?;
            let playlists = playlists?;
            page.title = "Library".into();
            page.subtitle = "Albums and playlists".into();
            page.next_offset = albums.next_offset.or(playlists.next_offset);
            page.sections = vec![
                typed_section("Albums", albums.items, None),
                typed_section("Playlists", playlists.items, None),
            ];
            page.state = ready_or_empty(&page);
        }
        Route::Search => {
            page.title = "Search".into();
            page.subtitle = "Press Enter to search Spotify".into();
            if query.trim().is_empty() {
                page.state = LoadState::Empty("Type a query, then press Enter.".into());
            } else {
                let results = api.search_page(token, query, offset, PAGE_SIZE)?;
                page.next_offset = results.next_offset;
                page.sections = vec![typed_section("Results", results.items, None)];
                page.state = ready_or_empty(&page);
            }
        }
        Route::Album { uri, .. } => apply_detail(&mut page, api.album_page(token, uri)?),
        Route::Playlist { uri, .. } => {
            apply_detail(&mut page, api.playlist_page(token, uri, offset, PAGE_SIZE)?);
        }
        Route::Artist { uri, .. } => apply_detail(&mut page, api.artist_page(token, uri)?),
        Route::Queue => {
            let queue = api.queue(token)?;
            page.title = "Queue".into();
            page.subtitle = "Now playing and server-ordered upcoming tracks".into();
            let mut sections = vec![Section {
                title: "Now playing".into(),
                items: queue
                    .current
                    .into_iter()
                    .map(|item| display_item(item, Some(EntityKind::Track)))
                    .collect(),
            }];
            sections.push(Section {
                title: "Up next".into(),
                items: queue
                    .upcoming
                    .into_iter()
                    .map(|item| display_item(item, Some(EntityKind::Track)))
                    .collect(),
            });
            page.sections = sections;
            page.state = ready_or_empty(&page);
        }
        Route::Devices => {
            page.title = "Devices".into();
            page.subtitle = "Selection is used by subsequent playback commands".into();
            page.sections = vec![Section {
                title: "Spotify Connect".into(),
                items: api
                    .devices(token)?
                    .into_iter()
                    .map(|device| BrowseItem {
                        id: device.id.clone(),
                        kind: EntityKind::Device,
                        title: format!(
                            "{}{}",
                            device.name,
                            if device.active { " · active" } else { "" }
                        ),
                        subtitle: if device.restricted {
                            format!("{} · restricted", device.kind)
                        } else {
                            device.kind
                        },
                        metadata: if device.restricted {
                            "Spotify reports that this device cannot accept remote commands.".into()
                        } else {
                            "Enter selects this device for playback.".into()
                        },
                        uri: Some(device.id),
                        external_url: None,
                        artwork_url: None,
                        artists: Vec::new(),
                        album: None,
                        duration_ms: None,
                        available: !device.restricted,
                        context: None,
                        restricted: device.restricted,
                    })
                    .collect(),
            }];
            page.state = ready_or_empty(&page);
        }
        Route::Settings => {
            page.title = "Settings".into();
            page.subtitle = "CLI appearance and interaction".into();
            page.sections = vec![Section {
                title: "Terminal".into(),
                items: vec![
                    setting_item("artwork", "Artwork: Auto / Blocks / Off"),
                    setting_item("mouse", "Mouse input: enabled"),
                    setting_item("wide-queue", "Wide-screen queue: enabled"),
                    setting_item("side-player-height", "Side player max height: 28 rows"),
                    setting_item("side-player-width", "Side player min width: 100 cols"),
                    setting_item("stacked-queue-height", "Stacked queue min height: 38 rows"),
                    setting_item("wide-breakpoint", "Wide layout min width: 140 cols"),
                ],
            }];
            page.state = LoadState::Ready;
        }
    }
    Ok(page)
}

fn apply_detail(page: &mut PageState, detail: SpotifyDetailPage) {
    page.subtitle = detail_line(&detail);
    page.title.clone_from(&detail.title);
    page.artwork_url.clone_from(&detail.artwork_url);
    let context = matches!(
        detail.kind,
        mellowdeck_spotify::SpotifyDetailKind::Album
            | mellowdeck_spotify::SpotifyDetailKind::Playlist
    )
    .then_some(detail.uri.clone());
    let primary = match detail.kind {
        mellowdeck_spotify::SpotifyDetailKind::Artist => "Songs (search-derived)",
        _ => "Tracks",
    };
    page.uri = Some(detail.uri.clone());
    page.external_url = detail
        .external_url
        .or_else(|| detail.uri.parse::<SpotifyUri>().ok().map(|parsed| parsed.web_url()));
    page.next_offset = detail.items.next_offset;
    page.sections = vec![typed_section(primary, detail.items.items, context.as_deref())];
    if !detail.releases.is_empty() {
        page.sections.push(typed_section("Releases", detail.releases, None));
    }
    page.state = match detail.state {
        SpotifyContentState::Available => ready_or_empty(page),
        SpotifyContentState::Empty => LoadState::Empty("This collection is empty.".into()),
        SpotifyContentState::Restricted(message) => LoadState::Restricted(message),
        SpotifyContentState::Partial(message) => LoadState::Partial(message),
    };
}

fn detail_line(detail: &SpotifyDetailPage) -> String {
    let mut parts = Vec::new();
    if let Some(owner) = &detail.owner {
        parts.push(format!("by {owner}"));
    }
    if !detail.description.is_empty() {
        parts.push(detail.description.clone());
    }
    if let Some(release) = &detail.release {
        parts.push(release.clone());
    }
    parts.push(format!("{} tracks", detail.total));
    parts.join(" · ")
}

fn ready_or_empty(page: &PageState) -> LoadState {
    if page.sections.iter().all(|section| section.items.is_empty()) {
        LoadState::Empty("Nothing to show.".into())
    } else {
        LoadState::Ready
    }
}

fn display_section(title: &str, items: Vec<SpotifyDisplayItem>) -> Section {
    Section {
        title: title.into(),
        items: items.into_iter().map(|item| display_item(item, None)).collect(),
    }
}

fn display_item(item: SpotifyDisplayItem, fallback: Option<EntityKind>) -> BrowseItem {
    let kind = item.uri.as_deref().and_then(|uri| uri.parse::<SpotifyUri>().ok()).map_or(
        fallback.unwrap_or(EntityKind::Message),
        |uri| match uri.kind() {
            SpotifyItemKind::Track => EntityKind::Track,
            SpotifyItemKind::Album => EntityKind::Album,
            SpotifyItemKind::Artist => EntityKind::Artist,
            SpotifyItemKind::Playlist => EntityKind::Playlist,
        },
    );
    BrowseItem {
        id: item.uri.clone().unwrap_or_else(|| item.title.clone()),
        kind,
        title: item.title,
        subtitle: item.subtitle,
        metadata: String::new(),
        uri: item.uri,
        external_url: None,
        artwork_url: item.artwork_url,
        artists: Vec::new(),
        album: None,
        duration_ms: None,
        available: true,
        context: None,
        restricted: false,
    }
}

fn typed_section(title: &str, items: Vec<SpotifyBrowseItem>, context: Option<&str>) -> Section {
    Section {
        title: title.into(),
        items: items.into_iter().map(|item| typed_item(item, context)).collect(),
    }
}

fn typed_item(item: SpotifyBrowseItem, context: Option<&str>) -> BrowseItem {
    let position = item.position;
    BrowseItem {
        id: item.id,
        kind: match item.kind {
            SpotifyEntityKind::Track => EntityKind::Track,
            SpotifyEntityKind::Album => EntityKind::Album,
            SpotifyEntityKind::Artist => EntityKind::Artist,
            SpotifyEntityKind::Playlist => EntityKind::Playlist,
        },
        title: item.title,
        subtitle: item.subtitle,
        metadata: item.metadata,
        uri: item.uri,
        external_url: item.external_url,
        artwork_url: item.artwork_url,
        artists: item.artists,
        album: item.album,
        duration_ms: item.duration_ms,
        available: item.available,
        context: context
            .zip(position)
            .map(|(uri, position)| ContextPosition { uri: uri.to_owned(), position }),
        restricted: false,
    }
}

fn queue_response(queue: SpotifyTypedQueue) -> ServiceResponse {
    ServiceResponse::Queue {
        now: queue.current.map(|item| typed_item(item, None)),
        upcoming: queue.upcoming.into_iter().map(|item| typed_item(item, None)).collect(),
    }
}

fn setting_item(id: &str, title: &str) -> BrowseItem {
    let mut item = BrowseItem::message(id, title);
    item.kind = EntityKind::Action;
    item.available = true;
    item
}

fn channel_error<T: std::fmt::Display>(error: T) -> AppError {
    AppError::new(ErrorKind::Unavailable, format!("background worker stopped: {error}"))
}

/// Maps a repeat effect numeric value to Spotify's repeat mode string.
#[must_use]
pub const fn repeat_mode(value: u8) -> &'static str {
    match value {
        1 => "context",
        2 => "track",
        _ => "off",
    }
}

fn play_track_with_retry(
    api: &SpotifyWebApi,
    token: &str,
    uri: &str,
    device_id: Option<&str>,
) -> Result<()> {
    let initial = api.play_uri_on_device(token, uri, device_id);
    if let Err(ref error) = initial
        && error.kind == ErrorKind::Unavailable
    {
        let target_device = match device_id {
            Some(id) => Some(id.to_owned()),
            None => resolve_fallback_device(api, token),
        };
        if let Some(ref target) = target_device {
            let _ = api.transfer(token, target);
            thread::sleep(Duration::from_millis(250));
            if api.play_uri_on_device(token, uri, Some(target)).is_ok() {
                return Ok(());
            }
        }
        thread::sleep(Duration::from_millis(150));
        if let Ok(playback) = api.playback(token)
            && playback.playing
        {
            return Ok(());
        }
    }
    initial
}

fn play_context_with_retry(
    api: &SpotifyWebApi,
    token: &str,
    uri: &str,
    position: usize,
    device_id: Option<&str>,
) -> Result<()> {
    let initial = api.play_context_at(token, uri, position, device_id);
    if let Err(ref error) = initial
        && error.kind == ErrorKind::Unavailable
    {
        let target_device = match device_id {
            Some(id) => Some(id.to_owned()),
            None => resolve_fallback_device(api, token),
        };
        if let Some(ref target) = target_device {
            let _ = api.transfer(token, target);
            thread::sleep(Duration::from_millis(250));
            if api.play_context_at(token, uri, position, Some(target)).is_ok() {
                return Ok(());
            }
        }
        thread::sleep(Duration::from_millis(150));
        if let Ok(playback) = api.playback(token)
            && playback.playing
        {
            return Ok(());
        }
    }
    initial
}

fn resolve_fallback_device(api: &SpotifyWebApi, token: &str) -> Option<String> {
    let devices = api.devices(token).ok()?;
    devices
        .iter()
        .find(|d| d.name.eq_ignore_ascii_case("mellowdeck") && !d.restricted)
        .or_else(|| devices.iter().find(|d| d.active && !d.restricted))
        .or_else(|| devices.iter().find(|d| !d.restricted))
        .map(|d| d.id.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeat_mode_maps_values_and_out_of_bounds_to_off() {
        assert_eq!(repeat_mode(0), "off");
        assert_eq!(repeat_mode(1), "context");
        assert_eq!(repeat_mode(2), "track");
        assert_eq!(repeat_mode(3), "off");
        assert_eq!(repeat_mode(4), "off");
        assert_eq!(repeat_mode(255), "off");
    }

    #[test]
    fn repeat_effect_pattern_matching_does_not_panic_and_maps_properly() {
        for value in 0..=255 {
            let mode = match value {
                1 => "context",
                2 => "track",
                _ => "off",
            };
            assert_eq!(mode, repeat_mode(value));
            match value {
                1 => assert_eq!(mode, "context"),
                2 => assert_eq!(mode, "track"),
                _ => assert_eq!(mode, "off"),
            }
        }
    }

    #[test]
    fn apply_detail_stores_context_metadata_and_omits_action_section() {
        let mut page = PageState::loading(
            Route::Album { uri: "spotify:album:1".into(), title: "OK Computer".into() },
            1,
        );
        let detail = SpotifyDetailPage {
            title: "OK Computer".into(),
            kind: mellowdeck_spotify::SpotifyDetailKind::Album,
            uri: "spotify:album:1".into(),
            external_url: Some("https://open.spotify.com/album/1".into()),
            artwork_url: None,
            owner: Some("Radiohead".into()),
            description: String::new(),
            release: Some("1997".into()),
            total: 12,
            items: mellowdeck_spotify::SpotifyPage {
                items: vec![],
                offset: 0,
                limit: 50,
                total: 0,
                next_offset: None,
            },
            releases: vec![],
            state: SpotifyContentState::Available,
        };
        apply_detail(&mut page, detail);
        assert_eq!(page.uri.as_deref(), Some("spotify:album:1"));
        assert_eq!(page.external_url.as_deref(), Some("https://open.spotify.com/album/1"));
        assert!(!page.sections.iter().any(|s| s.title == "Actions"));
        assert_eq!(page.sections[0].title, "Tracks");
    }
}
