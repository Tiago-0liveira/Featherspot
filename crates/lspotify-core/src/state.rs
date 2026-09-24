use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::{Device, DeviceId, ErrorKind, PlaylistId, RepeatMode, Track, UserId};

pub const SETTINGS_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AppState {
    pub session: SessionState,
    pub navigation: NavigationState,
    pub home: HomeState,
    pub library: LibraryState,
    pub search: SearchState,
    pub playlist_editor: PlaylistEditorState,
    pub playback: PlaybackState,
    pub connectivity: ConnectivityState,
    pub settings: SettingsState,
    pub notifications: NotificationState,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum SessionState {
    #[default]
    SignedOut,
    Authorizing,
    SignedIn {
        user_id: UserId,
        display_name: String,
        premium: bool,
    },
    ReauthorizationRequired,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NavigationState {
    pub current: Route,
    pub back: Vec<Route>,
    pub forward: Vec<Route>,
}

impl Default for NavigationState {
    fn default() -> Self {
        Self { current: Route::Onboarding, back: Vec::new(), forward: Vec::new() }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Route {
    Onboarding,
    Home,
    Search,
    LikedSongs,
    Albums,
    Artists,
    Playlist(PlaylistId),
    Settings,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct HomeState {
    pub loading: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LibraryState {
    pub loading: bool,
    pub liked_track_count: u32,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SearchState {
    pub query: String,
    pub request_generation: u64,
    pub loading: bool,
    pub recent_queries: Vec<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PlaylistEditorState {
    pub playlist_id: Option<PlaylistId>,
    pub pending_mutations: usize,
    pub conflict: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PlaybackState {
    pub target: Option<PlaybackTargetState>,
    pub item: Option<Track>,
    pub playing: bool,
    pub authoritative_position: Duration,
    pub observed_at: Option<Instant>,
    pub duration: Duration,
    pub volume_percent: u8,
    pub shuffle: bool,
    pub repeat: RepeatMode,
    pub restrictions: Vec<String>,
    pub devices: Vec<Device>,
}

impl PlaybackState {
    pub fn interpolated_position(&self, now: Instant) -> Duration {
        let elapsed = if self.playing {
            self.observed_at
                .map_or(Duration::ZERO, |observed| now.saturating_duration_since(observed))
        } else {
            Duration::ZERO
        };
        self.authoritative_position.saturating_add(elapsed).min(self.duration)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PlaybackTargetState {
    LocalEmbedded { healthy: bool },
    SpotifyConnect(DeviceId),
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum ConnectivityState {
    #[default]
    Online,
    Offline,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemePreference {
    #[default]
    System,
    PastelLight,
    InkDark,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalePreference {
    #[default]
    System,
    EnUs,
    PtPt,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FrameRateLimit {
    #[default]
    Adaptive,
    Fps30,
    Fps60,
    Fps120,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CliArtworkPreference {
    #[default]
    Auto,
    Blocks,
    Off,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct CliSettings {
    pub artwork: CliArtworkPreference,
    pub mouse: bool,
    pub wide_queue: bool,
    pub side_player_max_height: u16,
    pub side_player_min_width: u16,
    pub stacked_queue_min_height: u16,
    pub wide_breakpoint_width: u16,
    pub artwork_under_overlays: bool,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub keybindings: BTreeMap<String, Vec<String>>,
}

impl Default for CliSettings {
    fn default() -> Self {
        Self {
            artwork: CliArtworkPreference::Auto,
            mouse: true,
            wide_queue: true,
            side_player_max_height: 28,
            side_player_min_width: 100,
            stacked_queue_min_height: 38,
            wide_breakpoint_width: 140,
            artwork_under_overlays: false,
            keybindings: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct SettingsState {
    pub schema_version: u32,
    pub theme: ThemePreference,
    pub locale: LocalePreference,
    pub frame_rate: FrameRateLimit,
    pub cache_limit_mb: u32,
    pub client_id: Option<String>,
    /// Terminal-only preferences. Optional/defaulted for settings written by older releases.
    pub cli: Option<CliSettings>,
}

impl Default for SettingsState {
    fn default() -> Self {
        Self {
            schema_version: SETTINGS_SCHEMA_VERSION,
            theme: ThemePreference::System,
            locale: LocalePreference::System,
            frame_rate: FrameRateLimit::Adaptive,
            cache_limit_mb: 500,
            client_id: None,
            cli: None,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct NotificationState {
    pub next_id: u64,
    pub items: Vec<Notification>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Notification {
    pub id: u64,
    pub message: String,
    pub kind: NotificationKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NotificationKind {
    Info,
    Success,
    Warning,
    Error(ErrorKind),
}

#[derive(Debug)]
pub enum Action {
    Navigate(Route),
    NavigateBack,
    NavigateForward,
    SearchChanged(String),
    SearchStarted { generation: u64 },
    SearchFinished { generation: u64 },
    ConnectivityChanged(ConnectivityState),
    PlaybackObserved { position: Duration, duration: Duration, playing: bool, at: Instant },
    VolumeChanged(u8),
    Notify { message: String, kind: NotificationKind },
    DismissNotification(u64),
    LoggedOut,
}

impl AppState {
    pub fn reduce(&mut self, action: Action) {
        match action {
            Action::Navigate(route) if route != self.navigation.current => {
                self.navigation.back.push(self.navigation.current.clone());
                if self.navigation.back.len() > 50 {
                    self.navigation.back.remove(0);
                }
                self.navigation.current = route;
                self.navigation.forward.clear();
            }
            Action::NavigateBack => {
                if let Some(route) = self.navigation.back.pop() {
                    self.navigation.forward.push(self.navigation.current.clone());
                    if self.navigation.forward.len() > 50 {
                        self.navigation.forward.remove(0);
                    }
                    self.navigation.current = route;
                }
            }
            Action::NavigateForward => {
                if let Some(route) = self.navigation.forward.pop() {
                    self.navigation.back.push(self.navigation.current.clone());
                    if self.navigation.back.len() > 50 {
                        self.navigation.back.remove(0);
                    }
                    self.navigation.current = route;
                }
            }
            Action::SearchChanged(query) => {
                self.search.query = query;
                self.search.request_generation = self.search.request_generation.saturating_add(1);
            }
            Action::SearchStarted { generation }
                if generation == self.search.request_generation =>
            {
                self.search.loading = true;
            }
            Action::SearchFinished { generation }
                if generation == self.search.request_generation =>
            {
                self.search.loading = false;
                let query = self.search.query.trim();
                if !query.is_empty()
                    && self.search.recent_queries.first().map(String::as_str) != Some(query)
                {
                    self.search.recent_queries.insert(0, query.to_owned());
                    self.search.recent_queries.truncate(10);
                }
            }
            Action::Navigate(_) | Action::SearchStarted { .. } | Action::SearchFinished { .. } => {}
            Action::ConnectivityChanged(connectivity) => self.connectivity = connectivity,
            Action::PlaybackObserved { position, duration, playing, at } => {
                self.playback.authoritative_position = position.min(duration);
                self.playback.duration = duration;
                self.playback.playing = playing;
                self.playback.observed_at = Some(at);
            }
            Action::VolumeChanged(volume) => self.playback.volume_percent = volume.min(100),
            Action::Notify { message, kind } => {
                let id = self.notifications.next_id;
                self.notifications.next_id = id.saturating_add(1);
                self.notifications.items.push(Notification { id, message, kind });
                if self.notifications.items.len() > 50 {
                    self.notifications.items.remove(0);
                }
            }
            Action::DismissNotification(id) => {
                self.notifications.items.retain(|notification| notification.id != id);
            }
            Action::LoggedOut => {
                let settings = self.settings.clone();
                *self = Self::default();
                self.settings = settings;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_history_is_reversible() {
        let mut state = AppState::default();
        state.reduce(Action::Navigate(Route::Home));
        state.reduce(Action::Navigate(Route::Search));
        state.reduce(Action::NavigateBack);
        assert_eq!(state.navigation.current, Route::Home);
        state.reduce(Action::NavigateForward);
        assert_eq!(state.navigation.current, Route::Search);
    }

    #[test]
    fn obsolete_search_completion_is_ignored() {
        let mut state = AppState::default();
        state.reduce(Action::SearchChanged("first".into()));
        let old = state.search.request_generation;
        state.reduce(Action::SearchChanged("second".into()));
        state.reduce(Action::SearchStarted { generation: state.search.request_generation });
        state.reduce(Action::SearchFinished { generation: old });
        assert!(state.search.loading);
    }

    #[test]
    fn playback_progress_interpolates_and_clamps() {
        let observed = Instant::now();
        let mut state = PlaybackState {
            authoritative_position: Duration::from_secs(9),
            duration: Duration::from_secs(10),
            playing: true,
            observed_at: Some(observed),
            ..PlaybackState::default()
        };
        assert_eq!(
            state.interpolated_position(observed + Duration::from_secs(2)),
            Duration::from_secs(10)
        );
        state.playing = false;
        assert_eq!(
            state.interpolated_position(observed + Duration::from_secs(2)),
            Duration::from_secs(9)
        );
    }

    #[test]
    fn logout_preserves_local_settings_only() {
        let mut state = AppState::default();
        state.settings.theme = ThemePreference::InkDark;
        state.navigation.current = Route::Home;
        state.reduce(Action::LoggedOut);
        assert_eq!(state.settings.theme, ThemePreference::InkDark);
        assert_eq!(state.navigation.current, Route::Onboarding);
    }

    #[test]
    fn navigation_back_stack_capped_at_fifty() {
        let mut state = AppState::default();
        for i in 1..=60 {
            let route = Route::Playlist(PlaylistId::parse(format!("p{i}")).unwrap());
            state.reduce(Action::Navigate(route));
        }
        assert_eq!(state.navigation.back.len(), 50);
        let expected_oldest = Route::Playlist(PlaylistId::parse("p10").unwrap());
        let expected_newest = Route::Playlist(PlaylistId::parse("p59").unwrap());
        let expected_current = Route::Playlist(PlaylistId::parse("p60").unwrap());
        assert_eq!(state.navigation.back.first(), Some(&expected_oldest));
        assert_eq!(state.navigation.back.last(), Some(&expected_newest));
        assert_eq!(state.navigation.current, expected_current);
    }

    #[test]
    fn navigation_forward_stack_capped_at_fifty() {
        let mut state = AppState::default();
        state.navigation.back = (1..=60)
            .map(|i| Route::Playlist(PlaylistId::parse(format!("p{i}")).unwrap()))
            .collect();
        state.navigation.current = Route::Playlist(PlaylistId::parse("p61").unwrap());

        for _ in 0..60 {
            state.reduce(Action::NavigateBack);
        }
        assert_eq!(state.navigation.forward.len(), 50);
        let expected_current = Route::Playlist(PlaylistId::parse("p1").unwrap());
        assert_eq!(state.navigation.current, expected_current);

        for _ in 0..60 {
            state.reduce(Action::NavigateForward);
        }
        assert_eq!(state.navigation.back.len(), 50);
        assert!(state.navigation.forward.is_empty());
    }

    #[test]
    fn notifications_capped_at_fifty() {
        let mut state = AppState::default();
        for i in 1..=60 {
            state.reduce(Action::Notify {
                message: format!("Notification {i}"),
                kind: NotificationKind::Info,
            });
        }
        assert_eq!(state.notifications.items.len(), 50);
        assert_eq!(state.notifications.items.first().unwrap().message, "Notification 11");
        assert_eq!(state.notifications.items.last().unwrap().message, "Notification 60");
        assert_eq!(state.notifications.next_id, 60);
    }

    #[test]
    fn default_cli_settings_has_artwork_under_overlays_disabled() {
        let settings = CliSettings::default();
        assert!(!settings.artwork_under_overlays);
        assert!(settings.keybindings.is_empty());
    }

    #[test]
    fn older_settings_json_deserializes_without_keybindings() {
        let json = r#"{"artwork":"auto","mouse":true,"wide_queue":true}"#;
        let cli: CliSettings = serde_json::from_str(json).unwrap();
        assert!(cli.keybindings.is_empty());
    }

    #[test]
    fn settings_round_trip_preserves_keybinding_overrides() {
        let mut cli = CliSettings::default();
        cli.keybindings.insert("quit".into(), vec!["Ctrl+X".into(), "Esc".into()]);
        cli.keybindings.insert("toggle_playback".into(), vec!["p".into()]);

        let serialized = serde_json::to_string(&cli).unwrap();
        assert!(serialized.contains("Ctrl+X"));
        assert!(serialized.contains("toggle_playback"));

        let deserialized: CliSettings = serde_json::from_str(&serialized).unwrap();
        assert_eq!(cli, deserialized);
    }
}
