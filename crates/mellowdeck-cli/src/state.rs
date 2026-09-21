use std::{collections::VecDeque, time::Instant};

use mellowdeck_core::CliArtworkPreference;

pub const MIN_WIDTH: u16 = 80;
pub const MIN_HEIGHT: u16 = 24;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LayoutMode {
    Resize,
    Compact,
    #[default]
    Standard,
    Wide,
}

impl LayoutMode {
    pub fn for_size(width: u16, height: u16) -> Self {
        if width < MIN_WIDTH || height < MIN_HEIGHT {
            Self::Resize
        } else if width < 100 {
            Self::Compact
        } else if width < 140 {
            Self::Standard
        } else {
            Self::Wide
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum FocusRegion {
    Sidebar,
    #[default]
    Content,
    Queue,
    Player,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LibraryTab {
    #[default]
    Albums,
    Playlists,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SearchFilter {
    #[default]
    All,
    Tracks,
    Albums,
    Artists,
    Playlists,
}

impl SearchFilter {
    pub const ALL: [Self; 5] =
        [Self::All, Self::Tracks, Self::Albums, Self::Artists, Self::Playlists];
    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Tracks => "Tracks",
            Self::Albums => "Albums",
            Self::Artists => "Artists",
            Self::Playlists => "Playlists",
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum Route {
    #[default]
    Home,
    Search,
    Library,
    Album {
        uri: String,
        title: String,
    },
    Playlist {
        uri: String,
        title: String,
    },
    Artist {
        uri: String,
        title: String,
    },
    Queue,
    Devices,
    Settings,
}

impl Route {
    pub fn label(&self) -> &str {
        match self {
            Self::Home => "Home",
            Self::Search => "Search",
            Self::Library => "Library",
            Self::Album { title, .. }
            | Self::Playlist { title, .. }
            | Self::Artist { title, .. } => title,
            Self::Queue => "Queue",
            Self::Devices => "Devices",
            Self::Settings => "Settings",
        }
    }
    pub fn key(&self) -> String {
        match self {
            Self::Home => "home".into(),
            Self::Search => "search".into(),
            Self::Library => "library".into(),
            Self::Album { uri, .. } => format!("album:{uri}"),
            Self::Playlist { uri, .. } => format!("playlist:{uri}"),
            Self::Artist { uri, .. } => format!("artist:{uri}"),
            Self::Queue => "queue".into(),
            Self::Devices => "devices".into(),
            Self::Settings => "settings".into(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EntityKind {
    Track,
    Album,
    Artist,
    Playlist,
    Device,
    Action,
    Message,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextPosition {
    pub uri: String,
    pub position: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BrowseItem {
    pub id: String,
    pub kind: EntityKind,
    pub title: String,
    pub subtitle: String,
    pub metadata: String,
    pub uri: Option<String>,
    pub external_url: Option<String>,
    pub artwork_url: Option<String>,
    pub artists: Vec<(String, Option<String>)>,
    pub album: Option<(String, String)>,
    pub duration_ms: Option<u64>,
    pub available: bool,
    pub context: Option<ContextPosition>,
    pub restricted: bool,
}

impl BrowseItem {
    pub fn message(id: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            kind: EntityKind::Message,
            title: text.into(),
            subtitle: String::new(),
            metadata: String::new(),
            uri: None,
            external_url: None,
            artwork_url: None,
            artists: Vec::new(),
            album: None,
            duration_ms: None,
            available: false,
            context: None,
            restricted: false,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Section {
    pub title: String,
    pub items: Vec<BrowseItem>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum LoadState {
    #[default]
    Loading,
    Ready,
    Empty(String),
    Partial(String),
    Restricted(String),
    Failed(String),
    Stale(String),
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ListCursor {
    pub selected: usize,
    pub offset: usize,
}

impl ListCursor {
    pub fn move_by(&mut self, delta: isize, len: usize, viewport: usize) {
        if len == 0 {
            self.selected = 0;
            self.offset = 0;
            return;
        }
        self.selected = self.selected.saturating_add_signed(delta).min(len - 1);
        self.reveal(viewport, len);
    }
    pub fn move_to(&mut self, index: usize, len: usize, viewport: usize) {
        self.selected = index.min(len.saturating_sub(1));
        self.reveal(viewport, len);
    }
    pub fn reveal(&mut self, viewport: usize, len: usize) {
        let viewport = viewport.max(1);
        if self.selected < self.offset {
            self.offset = self.selected;
        } else if self.selected >= self.offset.saturating_add(viewport) {
            self.offset = self.selected + 1 - viewport;
        }
        self.offset = self.offset.min(len.saturating_sub(viewport));
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PageState {
    pub route: Route,
    pub title: String,
    pub subtitle: String,
    pub artwork_url: Option<String>,
    pub sections: Vec<Section>,
    pub cursor: ListCursor,
    pub state: LoadState,
    pub filter: String,
    pub library_tab: LibraryTab,
    pub search_filter: SearchFilter,
    pub next_offset: Option<u32>,
    pub loading_more: bool,
    pub generation: u64,
}

impl PageState {
    pub fn loading(route: Route, generation: u64) -> Self {
        let title = route.label().to_owned();
        Self {
            route,
            title,
            subtitle: String::new(),
            artwork_url: None,
            sections: Vec::new(),
            cursor: ListCursor::default(),
            state: LoadState::Loading,
            filter: String::new(),
            library_tab: LibraryTab::Albums,
            search_filter: SearchFilter::All,
            next_offset: None,
            loading_more: false,
            generation,
        }
    }
    pub fn flattened(&self) -> Vec<&BrowseItem> {
        let needle = self.filter.trim().to_lowercase();
        self.sections
            .iter()
            .flat_map(|section| &section.items)
            .filter(|item| {
                (needle.is_empty()
                    || item.title.to_lowercase().contains(&needle)
                    || item.subtitle.to_lowercase().contains(&needle))
                    && match self.route {
                        Route::Library => match self.library_tab {
                            LibraryTab::Albums => item.kind == EntityKind::Album,
                            LibraryTab::Playlists => item.kind == EntityKind::Playlist,
                        },
                        Route::Search => match self.search_filter {
                            SearchFilter::All => true,
                            SearchFilter::Tracks => item.kind == EntityKind::Track,
                            SearchFilter::Albums => item.kind == EntityKind::Album,
                            SearchFilter::Artists => item.kind == EntityKind::Artist,
                            SearchFilter::Playlists => item.kind == EntityKind::Playlist,
                        },
                        _ => true,
                    }
            })
            .collect()
    }
    pub fn selected_item(&self) -> Option<&BrowseItem> {
        self.flattened().get(self.cursor.selected).copied()
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PlaybackState {
    pub track_uri: Option<String>,
    pub context_uri: Option<String>,
    pub title: String,
    pub artist: String,
    pub artists: Vec<(String, Option<String>)>,
    pub album: String,
    pub album_uri: Option<String>,
    pub artwork_url: Option<String>,
    pub device_id: Option<String>,
    pub device_name: Option<String>,
    pub playing: bool,
    pub progress_ms: u64,
    pub duration_ms: u64,
    pub volume: u8,
    pub shuffle: bool,
    pub repeat: u8,
    pub available_actions: Vec<String>,
    pub observed_at: Option<Instant>,
    /// Metadata restored from the CLI sidecar, rather than a live Spotify snapshot.
    pub cached_track: bool,
}

impl PlaybackState {
    pub fn progress_at(&self, now: Instant) -> u64 {
        let elapsed = if self.playing {
            self.observed_at.map_or(0, |seen| {
                u64::try_from(now.saturating_duration_since(seen).as_millis()).unwrap_or(u64::MAX)
            })
        } else {
            0
        };
        if self.duration_ms > 0 {
            self.progress_ms.saturating_add(elapsed).min(self.duration_ms)
        } else {
            self.progress_ms.saturating_add(elapsed)
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Overlay {
    Menu,
    Help { query: String, editing: bool },
    Actions { selected: usize },
    Inspector,
    DevicePicker,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NoticeKind {
    Info,
    Pending,
    Success,
    Error,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Notice {
    pub kind: NoticeKind,
    pub text: String,
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug)]
pub struct AppState {
    pub page: PageState,
    pub history: Vec<PageState>,
    pub focus: FocusRegion,
    pub sidebar: ListCursor,
    /// Primary section retained while browsing a detail page for top-bar highlighting.
    pub primary_section: Route,
    pub queue: ListCursor,
    pub queue_now: Option<BrowseItem>,
    pub queue_upcoming: Vec<BrowseItem>,
    pub queue_state: LoadState,
    pub playback: PlaybackState,
    pub overlay: Option<Overlay>,
    pub text_entry: bool,
    pub search_query: String,
    pub layout: LayoutMode,
    pub content_height: usize,
    /// Number of visible upcoming-queue rows in the rendered queue panel.
    pub queue_height: usize,
    pub generation: u64,
    pub notice: Option<Notice>,
    pub quit: bool,
    pub artwork: CliArtworkPreference,
    pub mouse: bool,
    pub wide_queue: bool,
    /// Device registered by the local Web Playback SDK host, when available.
    pub local_device_id: Option<String>,
    pub selected_device_id: Option<String>,
    /// A device chosen from the Devices page must not be replaced automatically.
    pub device_selected_by_user: bool,
    pub response_log: VecDeque<u64>,
    pub playback_history: Vec<PlaybackState>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            page: PageState::loading(Route::Home, 0),
            history: Vec::new(),
            focus: FocusRegion::Content,
            sidebar: ListCursor::default(),
            primary_section: Route::Home,
            queue: ListCursor::default(),
            queue_now: None,
            queue_upcoming: Vec::new(),
            queue_state: LoadState::Loading,
            playback: PlaybackState { volume: 50, ..PlaybackState::default() },
            overlay: None,
            text_entry: false,
            search_query: String::new(),
            layout: LayoutMode::Standard,
            content_height: 10,
            queue_height: 1,
            generation: 0,
            notice: None,
            quit: false,
            artwork: CliArtworkPreference::Auto,
            mouse: true,
            wide_queue: true,
            local_device_id: None,
            selected_device_id: None,
            device_selected_by_user: false,
            response_log: VecDeque::new(),
            playback_history: Vec::new(),
        }
    }
}

impl AppState {
    pub const NAVIGATION: [Route; 6] =
        [Route::Home, Route::Search, Route::Library, Route::Queue, Route::Devices, Route::Settings];
    pub fn update_layout(&mut self, width: u16, height: u16) {
        self.layout = LayoutMode::for_size(width, height);
        if self.layout == LayoutMode::Compact && self.focus == FocusRegion::Sidebar {
            self.focus = FocusRegion::Content;
        }
        if !self.queue_visible() && self.focus == FocusRegion::Queue {
            self.focus = FocusRegion::Content;
        }
    }
    pub fn queue_visible(&self) -> bool {
        self.layout == LayoutMode::Wide && self.wide_queue && self.page.route != Route::Queue
    }
    pub fn navigate(&mut self, route: Route) -> u64 {
        if self.page.route != route {
            if matches!(route, Route::Home | Route::Search | Route::Library | Route::Settings) {
                self.primary_section = route.clone();
            }
            self.history.push(self.page.clone());
        }
        self.generation = self.generation.saturating_add(1);
        self.page = PageState::loading(route, self.generation);
        self.overlay = None;
        self.text_entry = false;
        self.generation
    }
    pub fn back(&mut self) {
        if let Some(page) = self.history.pop() {
            self.generation = self.generation.saturating_add(1);
            self.page = page;
            self.page.generation = self.generation;
            self.overlay = None;
            self.text_entry = false;
        }
    }
    pub fn visible_focuses(&self) -> Vec<FocusRegion> {
        let mut regions = Vec::new();
        if self.layout != LayoutMode::Compact {
            regions.push(FocusRegion::Sidebar);
        }
        regions.push(FocusRegion::Content);
        if self.queue_visible() {
            regions.push(FocusRegion::Queue);
        }
        regions.push(FocusRegion::Player);
        regions
    }
    pub fn cycle_focus(&mut self, reverse: bool) {
        let regions = self.visible_focuses();
        let position = regions.iter().position(|region| *region == self.focus).unwrap_or(0);
        let next = if reverse {
            position.checked_sub(1).unwrap_or(regions.len() - 1)
        } else {
            (position + 1) % regions.len()
        };
        self.focus = regions[next];
    }
    pub fn selected_item(&self) -> Option<&BrowseItem> {
        match self.focus {
            FocusRegion::Queue => self.queue_upcoming.get(self.queue.selected),
            _ => self.page.selected_item(),
        }
    }
    pub fn accept_page(&mut self, page: PageState) -> bool {
        if page.generation != self.generation || page.route.key() != self.page.route.key() {
            return false;
        }
        self.page = page;
        self.response_log.push_back(self.generation);
        while self.response_log.len() > 16 {
            self.response_log.pop_front();
        }
        true
    }
    pub fn push_playback_history(&mut self) {
        if let Some(uri) = &self.playback.track_uri
            && !self.playback.title.is_empty()
        {
            if self.playback_history.last().and_then(|p| p.track_uri.as_ref()) == Some(uri) {
                return;
            }
            if self.playback_history.len() >= 50 {
                self.playback_history.remove(0);
            }
            self.playback_history.push(self.playback.clone());
        }
    }
    #[must_use]
    pub fn playback_as_browse_item(&self) -> Option<BrowseItem> {
        let uri = self.playback.track_uri.clone()?;
        if self.playback.title.is_empty() {
            return None;
        }
        Some(BrowseItem {
            id: uri.clone(),
            kind: EntityKind::Track,
            title: self.playback.title.clone(),
            subtitle: self.playback.artist.clone(),
            metadata: String::new(),
            uri: Some(uri),
            external_url: None,
            artwork_url: self.playback.artwork_url.clone(),
            artists: self.playback.artists.clone(),
            album: self
                .playback
                .album_uri
                .clone()
                .map(|album_uri| (self.playback.album.clone(), album_uri)),
            duration_ms: if self.playback.duration_ms > 0 {
                Some(self.playback.duration_ms)
            } else {
                None
            },
            available: true,
            context: None,
            restricted: false,
        })
    }
    pub fn apply_track_to_playback(&mut self, item: &BrowseItem) {
        self.playback.track_uri.clone_from(&item.uri);
        self.playback.title.clone_from(&item.title);
        self.playback.artist.clone_from(&item.subtitle);
        self.playback.artists = if item.artists.is_empty() && !item.subtitle.is_empty() {
            vec![(item.subtitle.clone(), None)]
        } else {
            item.artists.clone()
        };
        if let Some((album_name, album_uri)) = &item.album {
            self.playback.album.clone_from(album_name);
            self.playback.album_uri = Some(album_uri.clone());
        } else {
            self.playback.album.clear();
            self.playback.album_uri = None;
        }
        self.playback.artwork_url.clone_from(&item.artwork_url);
        self.playback.duration_ms = item.duration_ms.unwrap_or(0);
        self.playback.progress_ms = 0;
        self.playback.playing = true;
        self.playback.observed_at = Some(Instant::now());
        self.playback.cached_track = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn stale_page_response_cannot_replace_new_navigation() {
        let mut state = AppState::default();
        let old = PageState::loading(Route::Home, 0);
        state.navigate(Route::Library);
        assert!(!state.accept_page(old));
        assert_eq!(state.page.route, Route::Library);
    }

    #[test]
    fn list_cursor_scrolls_to_keep_selection_visible() {
        let mut cursor = ListCursor::default();
        cursor.move_to(12, 30, 5);
        assert_eq!(cursor.selected, 12);
        assert_eq!(cursor.offset, 8);
        cursor.move_by(-10, 30, 5);
        assert_eq!(cursor.selected, 2);
        assert_eq!(cursor.offset, 2);
    }

    #[test]
    fn playback_history_records_and_caps_at_fifty() {
        let mut state = AppState::default();
        state.playback.track_uri = Some("spotify:track:initial".into());
        state.playback.title = "Initial Song".into();

        for i in 0..60 {
            state.push_playback_history();
            state.playback.track_uri = Some(format!("spotify:track:{i}"));
            state.playback.title = format!("Song {i}");
        }
        assert_eq!(state.playback_history.len(), 50);
        assert_eq!(state.playback_history.first().unwrap().title, "Song 9");
        assert_eq!(state.playback_history.last().unwrap().title, "Song 58");
    }

    #[test]
    fn apply_track_to_playback_populates_fields() {
        let mut state = AppState::default();
        let item = BrowseItem {
            id: "track1".into(),
            kind: EntityKind::Track,
            title: "Test Track".into(),
            subtitle: "Test Artist".into(),
            metadata: String::new(),
            uri: Some("spotify:track:123".into()),
            external_url: None,
            artwork_url: Some("https://example.com/art.jpg".into()),
            artists: vec![("Test Artist".into(), None)],
            album: Some(("Test Album".into(), "spotify:album:456".into())),
            duration_ms: Some(210_000),
            available: true,
            context: None,
            restricted: false,
        };
        state.apply_track_to_playback(&item);
        assert_eq!(state.playback.track_uri.as_deref(), Some("spotify:track:123"));
        assert_eq!(state.playback.title, "Test Track");
        assert_eq!(state.playback.artist, "Test Artist");
        assert_eq!(state.playback.album, "Test Album");
        assert_eq!(state.playback.album_uri.as_deref(), Some("spotify:album:456"));
        assert_eq!(state.playback.artwork_url.as_deref(), Some("https://example.com/art.jpg"));
        assert_eq!(state.playback.duration_ms, 210_000);
        assert_eq!(state.playback.progress_ms, 0);
        assert!(state.playback.playing);
    }

    #[test]
    fn playback_history_deduplicates_consecutive_entries() {
        let mut state = AppState::default();
        state.playback.track_uri = Some("spotify:track:song1".into());
        state.playback.title = "Song 1".into();

        state.push_playback_history();
        assert_eq!(state.playback_history.len(), 1);

        // Pushing identical URI again should be a no-op
        state.push_playback_history();
        assert_eq!(state.playback_history.len(), 1);

        state.playback.track_uri = Some("spotify:track:song2".into());
        state.playback.title = "Song 2".into();
        state.push_playback_history();
        assert_eq!(state.playback_history.len(), 2);
    }

    #[test]
    fn progress_at_interpolates_when_duration_is_zero() {
        let start = Instant::now();
        let playback = PlaybackState {
            playing: true,
            duration_ms: 0,
            progress_ms: 1_000,
            observed_at: Some(start),
            ..PlaybackState::default()
        };

        let later = start + Duration::from_millis(500);
        assert!(playback.progress_at(later) >= 1_500);
    }

    #[test]
    fn playback_as_browse_item_creates_valid_item() {
        let mut state = AppState::default();
        assert!(state.playback_as_browse_item().is_none());

        state.playback.track_uri = Some("spotify:track:abc".into());
        state.playback.title = "My Song".into();
        state.playback.artist = "My Artist".into();
        state.playback.duration_ms = 180_000;

        let item = state.playback_as_browse_item().unwrap();
        assert_eq!(item.uri.as_deref(), Some("spotify:track:abc"));
        assert_eq!(item.title, "My Song");
        assert_eq!(item.subtitle, "My Artist");
        assert_eq!(item.duration_ms, Some(180_000));
    }
}
