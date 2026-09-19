use std::{
    sync::{Arc, mpsc::Receiver},
    time::Duration,
};

use gpui::{
    Animation, AnimationExt, AnyElement, AnyView, App, Application, Bounds, Context, Div,
    FocusHandle, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, ObjectFit, Pixels, Render, ScrollDelta, ScrollWheelEvent, SharedString, Stateful,
    Window, WindowBounds, WindowOptions, canvas, deferred, div, img, prelude::*, px, relative, rgb,
    size,
};
use mellowdeck_core::{
    Action, AppState, LocalPlayerCommand, LocalPlayerEvent, Route, SessionState, SpotifyUri,
    ThemePreference,
};

use crate::{
    ArtistDetail, AuthorizedAccount, DesktopActions, HomeSection, HomeSnapshot, LibrarySnapshot,
    LocalPlayerFactory, LocalPlayerSurface, PlaybackCommand, PlaybackDevice, PlaybackSnapshot,
    QueueSnapshot, SearchResultItem, StartupState, image_http::spotify_image_client,
};

// Fixed pastel accents, identical in both themes so brand color stays recognizable.
const PEACH: u32 = 0xF5_B7_9F;
const LAVENDER: u32 = 0xC8_BB_E9;
const SAGE: u32 = 0xAF_C9_B5;
const RED: u32 = 0xD9_3A_47;

/// The themeable surface and text colors. Every screen reads these from the active palette so the
/// whole shell repaints when the user switches between light and dark.
#[derive(Clone, Copy, Debug)]
struct Palette {
    canvas: u32,
    surface: u32,
    ink: u32,
    muted: u32,
    line: u32,
}

impl Palette {
    const fn light() -> Self {
        Self {
            canvas: 0xF2_F1_EF,
            surface: 0xFF_FF_FF,
            ink: 0x1D_1E_22,
            muted: 0x72_72_78,
            line: 0xE3_E1_DE,
        }
    }

    const fn dark() -> Self {
        Self {
            canvas: 0x14_15_1A,
            surface: 0x1E_20_27,
            ink: 0xF2_F1_EF,
            muted: 0x9A_9B_A2,
            line: 0x33_35_3E,
        }
    }

    fn for_preference(preference: ThemePreference) -> Self {
        match preference {
            ThemePreference::InkDark => Self::dark(),
            ThemePreference::System | ThemePreference::PastelLight => Self::light(),
        }
    }
}

/// Device name the embedded Web Playback SDK registers with Spotify Connect.
const LOCAL_PLAYER_NAME: &str = "Mellowdeck";
/// How long a click waits for the embedded player before using Spotify Connect instead.
const LOCAL_PLAYER_START_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone, Debug)]
enum AuthStatus {
    Editing,
    Working { restoring: bool },
    Failed(String),
}

#[derive(Clone, Debug, Default)]
enum HomeStatus {
    #[default]
    NotLoaded,
    Loading,
    Loaded(Box<HomeSnapshot>),
    Failed(String),
}

#[derive(Clone, Debug, Default)]
enum SearchStatus {
    #[default]
    Idle,
    Loading,
    Loaded(Vec<SearchResultItem>),
    Failed(String),
}

#[derive(Clone, Debug, Default)]
enum LoadStatus<T> {
    #[default]
    Idle,
    Loading,
    Loaded(T),
    Failed(String),
}

impl<T> LoadStatus<T> {
    fn as_loaded(&self) -> Option<&T> {
        if let Self::Loaded(value) = self { Some(value) } else { None }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PlayerPanel {
    Volume,
    Queue,
    Devices,
}

struct MellowdeckShell {
    state: AppState,
    actions: Arc<dyn DesktopActions>,
    account: Option<AuthorizedAccount>,
    client_id: String,
    input_focus: FocusHandle,
    search_focus: FocusHandle,
    auth: AuthStatus,
    /// The user's chosen theme, persisted across launches.
    theme: ThemePreference,
    /// The active color palette derived from `theme`.
    pal: Palette,
    home: HomeStatus,
    library: LoadStatus<LibrarySnapshot>,
    search_query: String,
    search: SearchStatus,
    artist: LoadStatus<ArtistDetail>,
    playback: LoadStatus<PlaybackSnapshot>,
    playback_request_generation: u64,
    queue: LoadStatus<QueueSnapshot>,
    devices: LoadStatus<Vec<PlaybackDevice>>,
    player_panel: Option<PlayerPanel>,
    scratch_origin: Option<(gpui::Pixels, u64)>,
    scratch_preview_ms: Option<u64>,
    playback_clock_started: bool,
    local_player: Option<Box<dyn LocalPlayerSurface>>,
    local_events: Receiver<LocalPlayerEvent>,
    local_device_id: Option<String>,
    local_player_status: Option<String>,
    local_player_failed: bool,
    local_track_uri: Option<SpotifyUri>,
    pending_local_uri: Option<String>,
    /// Optimistic volume shown while the user is adjusting it, before Spotify confirms.
    volume_display: Option<u8>,
    /// Bumped on each volume tweak; a delayed task clears `volume_display` when it is unchanged.
    volume_interaction_token: u64,
    /// Bumped on each volume tweak; a delayed task sends the Web API volume when it is unchanged.
    volume_send_token: u64,
    /// Bumped whenever the volume popup is hovered or unhovered, to gate the close grace period.
    volume_hover_token: u64,
    /// Screen bounds of the waveform strip, captured during paint for click/drag seeking.
    waveform_bounds: Option<Bounds<Pixels>>,
    /// Time under the pointer while hovering the waveform, for the preview line.
    waveform_hover_ms: Option<u64>,
    /// Screen bounds of the volume slider track, captured during paint for click/drag setting.
    volume_slider_bounds: Option<Bounds<Pixels>>,
    /// Whether the pointer is currently dragging the volume slider.
    volume_dragging: bool,
    /// The volume to restore when unmuting after a double-click mute.
    volume_before_mute: Option<u8>,
}

impl MellowdeckShell {
    fn new(
        startup: StartupState,
        actions: Arc<dyn DesktopActions>,
        local_player: Option<Box<dyn LocalPlayerSurface>>,
        local_events: Receiver<LocalPlayerEvent>,
        local_player_error: Option<String>,
        cx: &mut Context<Self>,
    ) -> Self {
        let client_id = startup.client_id.unwrap_or_default();
        let mut shell = Self {
            state: AppState::default(),
            actions,
            account: None,
            client_id,
            input_focus: cx.focus_handle(),
            search_focus: cx.focus_handle(),
            auth: startup.warning.map_or(AuthStatus::Editing, AuthStatus::Failed),
            theme: startup.theme,
            pal: Palette::for_preference(startup.theme),
            home: HomeStatus::NotLoaded,
            library: LoadStatus::Idle,
            search_query: String::new(),
            search: SearchStatus::Idle,
            artist: LoadStatus::Idle,
            playback: LoadStatus::Idle,
            playback_request_generation: 0,
            queue: LoadStatus::Idle,
            devices: LoadStatus::Idle,
            player_panel: None,
            scratch_origin: None,
            scratch_preview_ms: None,
            playback_clock_started: false,
            local_player,
            local_events,
            local_device_id: None,
            local_player_status: local_player_error,
            local_player_failed: false,
            local_track_uri: None,
            pending_local_uri: None,
            volume_display: None,
            volume_interaction_token: 0,
            volume_send_token: 0,
            volume_hover_token: 0,
            waveform_bounds: None,
            waveform_hover_ms: None,
            volume_slider_bounds: None,
            volume_dragging: false,
            volume_before_mute: None,
        };
        Self::start_local_event_loop(cx);
        if startup.has_saved_session && !shell.client_id.is_empty() {
            shell.start_authentication(true, cx);
        }
        shell
    }

    fn start_authentication(&mut self, restore: bool, cx: &mut Context<Self>) {
        if matches!(self.auth, AuthStatus::Working { .. }) {
            return;
        }
        let client_id = self.client_id.trim().to_owned();
        if !(16..=64).contains(&client_id.len())
            || !client_id.bytes().all(|byte| byte.is_ascii_alphanumeric())
        {
            self.auth = AuthStatus::Failed(
                "Enter the public Client ID from your Spotify developer app.".into(),
            );
            cx.notify();
            return;
        }

        self.auth = AuthStatus::Working { restoring: restore };
        self.state.session = SessionState::Authorizing;
        let actions = Arc::clone(&self.actions);
        let task = cx
            .background_executor()
            .spawn(async move { actions.authenticate(&client_id, restore) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(account) => {
                        this.state.session = SessionState::SignedIn {
                            user_id: account.user_id.clone(),
                            display_name: account.display_name.clone(),
                            premium: account.premium,
                        };
                        this.state.reduce(Action::Navigate(Route::Home));
                        this.account = Some(account);
                        this.auth = AuthStatus::Editing;
                        this.start_home_load(cx);
                        this.load_playback(cx);
                        this.start_playback_clock(cx);
                        this.connect_local_player(cx);
                    }
                    Err(error) => {
                        this.state.session = SessionState::SignedOut;
                        this.state.navigation.current = Route::Onboarding;
                        this.auth = AuthStatus::Failed(error.to_string());
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn start_home_load(&mut self, cx: &mut Context<Self>) {
        if matches!(self.home, HomeStatus::Loading) {
            return;
        }
        self.home = HomeStatus::Loading;
        let actions = Arc::clone(&self.actions);
        let task = cx.background_executor().spawn(async move { actions.load_home() });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.home = match result {
                    Ok(home) => HomeStatus::Loaded(Box::new(home)),
                    Err(error) => HomeStatus::Failed(error.to_string()),
                };
                cx.notify();
            });
        })
        .detach();
    }

    fn start_library_load(&mut self, cx: &mut Context<Self>) {
        if matches!(self.library, LoadStatus::Loading) {
            return;
        }
        self.library = LoadStatus::Loading;
        let actions = Arc::clone(&self.actions);
        let task = cx.background_executor().spawn(async move { actions.load_library() });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.library = match result {
                    Ok(library) => LoadStatus::Loaded(library),
                    Err(error) => LoadStatus::Failed(error.to_string()),
                };
                cx.notify();
            });
        })
        .detach();
    }

    /// Switches the theme, repaints immediately, and persists the choice in the background.
    fn set_theme(&mut self, preference: ThemePreference, cx: &mut Context<Self>) {
        self.theme = preference;
        self.pal = Palette::for_preference(preference);
        let actions = Arc::clone(&self.actions);
        cx.background_executor()
            .spawn(async move {
                let _ = actions.save_theme(preference);
            })
            .detach();
        cx.notify();
    }

    fn start_search(&mut self, cx: &mut Context<Self>) {
        if matches!(self.search, SearchStatus::Loading) {
            return;
        }
        let query = self.search_query.trim().to_owned();
        if query.is_empty() {
            self.search = SearchStatus::Failed("Enter something to search for.".into());
            cx.notify();
            return;
        }
        self.search = SearchStatus::Loading;
        let actions = Arc::clone(&self.actions);
        let task = cx.background_executor().spawn(async move { actions.search(&query) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.search = match result {
                    Ok(results) => SearchStatus::Loaded(results),
                    Err(error) => SearchStatus::Failed(error.to_string()),
                };
                cx.notify();
            });
        })
        .detach();
    }

    fn load_playback(&mut self, cx: &mut Context<Self>) {
        self.playback = LoadStatus::Loading;
        let actions = Arc::clone(&self.actions);
        let task = cx.background_executor().spawn(async move { actions.load_playback() });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.playback = match result {
                    Ok(playback) => LoadStatus::Loaded(playback),
                    Err(error) => LoadStatus::Failed(error.to_string()),
                };
                cx.notify();
            });
        })
        .detach();
    }

    fn refresh_playback(&mut self, cx: &mut Context<Self>) {
        if matches!(self.playback, LoadStatus::Loading) {
            return;
        }
        let actions = Arc::clone(&self.actions);
        let task = cx.background_executor().spawn(async move { actions.load_playback() });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.playback = match result {
                    Ok(playback) => LoadStatus::Loaded(playback),
                    Err(error) => LoadStatus::Failed(error.to_string()),
                };
                cx.notify();
            });
        })
        .detach();
    }

    fn start_playback_clock(&mut self, cx: &mut Context<Self>) {
        if self.playback_clock_started {
            return;
        }
        self.playback_clock_started = true;
        cx.spawn(async move |this, cx| {
            let mut seconds = 0_u8;
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let alive = this.update(cx, |this, cx| {
                    seconds = seconds.wrapping_add(1);
                    if let LoadStatus::Loaded(playback) = &mut this.playback
                        && playback.playing
                    {
                        playback.progress_ms =
                            playback.progress_ms.saturating_add(1_000).min(playback.duration_ms);
                        cx.notify();
                    }
                    if seconds.is_multiple_of(15) {
                        this.refresh_playback(cx);
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    fn send_playback(&mut self, command: PlaybackCommand, cx: &mut Context<Self>) {
        if matches!(command, PlaybackCommand::Transfer(_)) {
            self.player_panel = None;
        }
        self.playback_request_generation = self.playback_request_generation.wrapping_add(1);
        let generation = self.playback_request_generation;
        if !matches!(self.playback, LoadStatus::Loaded(_)) {
            self.playback = LoadStatus::Loading;
        }
        let actions = Arc::clone(&self.actions);
        let task = cx.background_executor().spawn(async move { actions.playback_command(command) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                if generation != this.playback_request_generation {
                    return;
                }
                this.playback = match result {
                    Ok(playback) => LoadStatus::Loaded(playback),
                    Err(error) => LoadStatus::Failed(error.to_string()),
                };
                cx.notify();
            });
        })
        .detach();
    }

    fn activate_media(&mut self, uri: &str, cx: &mut Context<Self>) {
        // Artists and albums open a detail page rather than starting playback.
        if uri.starts_with("spotify:artist:") {
            self.open_detail(uri.to_owned(), Route::Artists, false, cx);
            return;
        }
        if uri.starts_with("spotify:album:") {
            self.open_detail(uri.to_owned(), Route::Albums, true, cx);
            return;
        }

        // Respect a session that is already audible elsewhere; otherwise play inside Mellowdeck.
        let command = if self.another_device_is_playing() {
            PlaybackCommand::PlayUri(uri.to_owned())
        } else if let Some(device_id) = &self.local_device_id {
            PlaybackCommand::PlayUriOnDevice { uri: uri.to_owned(), device_id: device_id.clone() }
        } else if self.local_player.is_some() && !self.local_player_failed {
            self.wait_for_local_player(uri.to_owned(), cx);
            return;
        } else {
            PlaybackCommand::PlayUri(uri.to_owned())
        };
        self.send_playback(command, cx);
    }

    /// Loads an artist or album detail page into the shared detail slot and shows it.
    fn open_detail(&mut self, uri: String, route: Route, is_album: bool, cx: &mut Context<Self>) {
        self.state.reduce(Action::Navigate(route));
        self.artist = LoadStatus::Loading;
        let actions = Arc::clone(&self.actions);
        let task = cx.background_executor().spawn(async move {
            if is_album { actions.load_album(&uri) } else { actions.load_artist(&uri) }
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.artist = match result {
                    Ok(detail) => LoadStatus::Loaded(detail),
                    Err(error) => LoadStatus::Failed(error.to_string()),
                };
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn another_device_is_playing(&self) -> bool {
        self.playback.as_loaded().is_some_and(|playback| {
            playback.playing
                && playback.device_name.as_deref().is_some_and(|name| name != LOCAL_PLAYER_NAME)
        })
    }

    fn wait_for_local_player(&mut self, uri: String, cx: &mut Context<Self>) {
        self.pending_local_uri = Some(uri.clone());
        self.local_player_status = Some("Waiting for Mellowdeck player…".into());
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(LOCAL_PLAYER_START_TIMEOUT).await;
            let _ = this.update(cx, |this, cx| {
                if this.pending_local_uri.as_deref() == Some(uri.as_str()) {
                    this.local_player_status =
                        Some("Mellowdeck player did not start; using Spotify Connect".into());
                    this.play_pending_on_connect(cx);
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    fn play_pending_on_connect(&mut self, cx: &mut Context<Self>) {
        if let Some(uri) = self.pending_local_uri.take() {
            self.send_playback(PlaybackCommand::PlayUri(uri), cx);
        }
    }

    fn connect_local_player(&mut self, cx: &mut Context<Self>) {
        let Some(player) = self.local_player.as_ref() else {
            return;
        };
        let result = self.actions.local_player_access_token().and_then(|access_token| {
            player.send(&LocalPlayerCommand::TokenUpdate { access_token })?;
            player.send(&LocalPlayerCommand::Connect)
        });
        self.local_player_status = match result {
            Ok(()) => Some("Starting Mellowdeck local player…".into()),
            Err(error) => Some(error.to_string()),
        };
        cx.notify();
    }

    fn start_local_event_loop(cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(100)).await;
                let alive = this.update(cx, |this, cx| {
                    let events = this.local_events.try_iter().collect::<Vec<_>>();
                    if !events.is_empty() {
                        for event in events {
                            this.handle_local_event(event, cx);
                        }
                        cx.notify();
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    fn handle_local_event(&mut self, event: LocalPlayerEvent, cx: &mut Context<Self>) {
        match event {
            LocalPlayerEvent::Ready { device_id } => {
                let device_id = device_id.to_string();
                self.local_device_id = Some(device_id.clone());
                self.local_player_failed = false;
                self.local_player_status = Some("Mellowdeck player ready".into());
                if let Some(uri) = self.pending_local_uri.take() {
                    self.send_playback(PlaybackCommand::PlayUriOnDevice { uri, device_id }, cx);
                }
            }
            LocalPlayerEvent::Unavailable => {
                self.local_device_id = None;
                self.local_player_failed = true;
                self.local_player_status = Some("Local playback is unavailable in WebView2".into());
                self.play_pending_on_connect(cx);
            }
            LocalPlayerEvent::StateChanged { playing, position_ms, duration_ms, track_uri } => {
                if let LoadStatus::Loaded(playback) = &mut self.playback {
                    playback.playing = playing;
                    playback.progress_ms = position_ms;
                    playback.duration_ms = duration_ms;
                }
                // The SDK reports timing only; fetch title and artwork when the track changes.
                if track_uri != self.local_track_uri {
                    self.local_track_uri = track_uri;
                    self.refresh_playback(cx);
                }
            }
            LocalPlayerEvent::AuthenticationError(message)
            | LocalPlayerEvent::AccountError(message) => {
                self.local_device_id = None;
                self.local_player_failed = true;
                self.local_player_status = Some(message);
                self.play_pending_on_connect(cx);
            }
            LocalPlayerEvent::PlaybackError(message) => {
                self.local_player_status = Some(message);
            }
        }
    }

    fn start_scratching(
        &mut self,
        event: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let LoadStatus::Loaded(playback) = &self.playback
            && playback.duration_ms > 0
        {
            self.scratch_origin = Some((event.position.x, playback.progress_ms));
            self.scratch_preview_ms = Some(playback.progress_ms);
            cx.notify();
        }
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss, clippy::cast_sign_loss)]
    fn move_scratch(
        &mut self,
        event: &MouseMoveEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((origin_x, origin_ms)) = self.scratch_origin else {
            return;
        };
        let duration_ms = match &self.playback {
            LoadStatus::Loaded(playback) => playback.duration_ms,
            _ => return,
        };
        let delta_pixels = (event.position.x - origin_x) / px(1.0);
        let delta_ms = f64::from(delta_pixels) * 250.0;
        let target = (origin_ms as f64 + delta_ms).clamp(0.0, duration_ms as f64) as u64;
        self.scratch_preview_ms = Some(target);
        cx.notify();
    }

    fn finish_scratch(
        &mut self,
        _event: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.scratch_origin.take().is_some()
            && let Some(target) = self.scratch_preview_ms.take()
        {
            self.send_playback(PlaybackCommand::Seek(target), cx);
        }
    }

    /// The volume the player bar should display: the user's in-flight value, else Spotify's.
    fn current_volume(&self) -> u8 {
        self.volume_display
            .or_else(|| self.playback.as_loaded().and_then(|playback| playback.volume_percent))
            .unwrap_or(50)
    }

    /// Whether Mellowdeck's embedded Web Playback SDK is the active Spotify Connect device.
    fn local_player_is_active(&self) -> bool {
        self.playback
            .as_loaded()
            .and_then(|playback| playback.device_name.as_deref())
            .is_some_and(|name| name == LOCAL_PLAYER_NAME)
    }

    /// Applies a new volume: optimistic locally, direct to the embedded player, or debounced to the
    /// Web API. Only the latest value is ever sent, so rapid scrolling never floods Spotify.
    fn set_volume(&mut self, percent: u8, cx: &mut Context<Self>) {
        let percent = percent.min(100);
        self.volume_display = Some(percent);
        if let LoadStatus::Loaded(playback) = &mut self.playback {
            playback.volume_percent = Some(percent);
        }

        // Let the periodic refresh resume owning the value once the user settles.
        self.volume_interaction_token = self.volume_interaction_token.wrapping_add(1);
        let interaction_token = self.volume_interaction_token;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(1_500)).await;
            let _ = this.update(cx, |this, cx| {
                if this.volume_interaction_token == interaction_token {
                    this.volume_display = None;
                    cx.notify();
                }
            });
        })
        .detach();

        if self.local_player_is_active() {
            if let Some(player) = &self.local_player {
                let _ = player
                    .send(&LocalPlayerCommand::Volume { value_milli: u16::from(percent) * 10 });
            }
        } else {
            self.volume_send_token = self.volume_send_token.wrapping_add(1);
            let send_token = self.volume_send_token;
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(Duration::from_millis(200)).await;
                let _ = this.update(cx, |this, cx| {
                    if this.volume_send_token == send_token {
                        this.send_playback(PlaybackCommand::Volume(percent), cx);
                    }
                });
            })
            .detach();
        }
        cx.notify();
    }

    /// Nudges the volume by whole 5% steps, from mouse-wheel notches over the volume controls.
    fn adjust_volume(&mut self, notches: i32, cx: &mut Context<Self>) {
        if notches == 0 {
            return;
        }
        let current = i32::from(self.current_volume());
        let next = (current + notches * 5).clamp(0, 100);
        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
        self.set_volume(next as u8, cx);
    }

    /// Converts a scroll event over the volume controls into a volume change.
    fn scroll_volume(
        &mut self,
        event: &ScrollWheelEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let amount = match event.delta {
            ScrollDelta::Lines(point) => point.y,
            ScrollDelta::Pixels(point) => f32::from(point.y),
        };
        // One 5% step per scroll gesture, in whichever direction the wheel moved.
        let notches = match amount.partial_cmp(&0.0) {
            Some(std::cmp::Ordering::Greater) => 1,
            Some(std::cmp::Ordering::Less) => -1,
            _ => 0,
        };
        self.adjust_volume(notches, cx);
    }

    /// Opens the volume popup and keeps it open while the pointer is over the icon or the popup.
    fn hover_volume(&mut self, hovering: bool, cx: &mut Context<Self>) {
        self.volume_hover_token = self.volume_hover_token.wrapping_add(1);
        if hovering {
            self.player_panel = Some(PlayerPanel::Volume);
            cx.notify();
        } else {
            // Grace period so moving between the icon and the popup does not close it.
            let token = self.volume_hover_token;
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(Duration::from_millis(250)).await;
                let _ = this.update(cx, |this, cx| {
                    if this.volume_hover_token == token
                        && this.player_panel == Some(PlayerPanel::Volume)
                    {
                        this.player_panel = None;
                        cx.notify();
                    }
                });
            })
            .detach();
        }
    }

    /// Toggles mute: silences and remembers the level, or restores the remembered level.
    fn toggle_mute(&mut self, cx: &mut Context<Self>) {
        if let Some(previous) = self.volume_before_mute.take() {
            self.set_volume(previous, cx);
        } else {
            let current = self.current_volume();
            self.volume_before_mute = Some(if current == 0 { 50 } else { current });
            self.set_volume(0, cx);
        }
    }

    /// Maps a vertical screen position on the slider track to a volume percent (top is loudest).
    #[allow(clippy::cast_precision_loss, clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    fn volume_from_slider_y(&self, y: Pixels) -> Option<u8> {
        let bounds = self.volume_slider_bounds?;
        let height = f32::from(bounds.size.height);
        if height <= 0.0 {
            return None;
        }
        let offset = f32::from(y - bounds.origin.y);
        let fraction = (1.0 - offset / height).clamp(0.0, 1.0);
        Some((fraction * 100.0).round() as u8)
    }

    fn volume_slider_down(
        &mut self,
        event: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(percent) = self.volume_from_slider_y(event.position.y) {
            self.volume_dragging = true;
            self.volume_before_mute = None;
            self.set_volume(percent, cx);
        }
    }

    fn volume_slider_move(
        &mut self,
        event: &MouseMoveEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.volume_dragging
            && let Some(percent) = self.volume_from_slider_y(event.position.y)
        {
            self.set_volume(percent, cx);
        }
    }

    fn volume_slider_up(
        &mut self,
        _event: &MouseUpEvent,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        self.volume_dragging = false;
    }

    /// Maps a horizontal screen position to a fraction of the current track, using the waveform
    /// bounds captured during paint.
    #[allow(clippy::cast_precision_loss)]
    fn waveform_fraction_at(&self, x: Pixels) -> Option<f32> {
        let bounds = self.waveform_bounds?;
        seek_ratio(f32::from(x), f32::from(bounds.origin.x), f32::from(bounds.size.width))
    }

    #[allow(clippy::cast_precision_loss, clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    fn fraction_to_ms(&self, fraction: f32) -> Option<u64> {
        let duration = self.playback.as_loaded().map(|playback| playback.duration_ms)?;
        if duration == 0 {
            return None;
        }
        Some((f64::from(fraction) * duration as f64).round() as u64)
    }

    fn waveform_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(fraction) = self.waveform_fraction_at(event.position.x)
            && let Some(target) = self.fraction_to_ms(fraction)
        {
            self.scratch_origin = Some((event.position.x, target));
            self.scratch_preview_ms = Some(target);
            cx.notify();
        }
    }

    fn waveform_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(fraction) = self.waveform_fraction_at(event.position.x) else {
            return;
        };
        let Some(target) = self.fraction_to_ms(fraction) else {
            return;
        };
        if self.scratch_origin.is_some() {
            // Dragging: scrub the playhead itself.
            self.scratch_preview_ms = Some(target);
        }
        // Always show the hover preview line under the pointer.
        self.waveform_hover_ms = Some(target);
        cx.notify();
    }

    fn waveform_mouse_up(
        &mut self,
        _event: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.scratch_origin.take().is_some()
            && let Some(target) = self.scratch_preview_ms.take()
        {
            self.send_playback(PlaybackCommand::Seek(target), cx);
        }
    }

    fn clear_waveform_hover(&mut self, cx: &mut Context<Self>) {
        if self.waveform_hover_ms.take().is_some() {
            cx.notify();
        }
    }

    fn toggle_player_panel(&mut self, panel: PlayerPanel, cx: &mut Context<Self>) {
        self.player_panel = if self.player_panel == Some(panel) { None } else { Some(panel) };
        match panel {
            PlayerPanel::Queue if !matches!(self.queue, LoadStatus::Loading) => {
                self.load_queue(cx);
            }
            PlayerPanel::Devices if !matches!(self.devices, LoadStatus::Loading) => {
                self.load_devices(cx);
            }
            PlayerPanel::Volume | PlayerPanel::Queue | PlayerPanel::Devices => cx.notify(),
        }
    }

    fn load_queue(&mut self, cx: &mut Context<Self>) {
        self.queue = LoadStatus::Loading;
        let actions = Arc::clone(&self.actions);
        let task = cx.background_executor().spawn(async move { actions.load_queue() });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.queue = match result {
                    Ok(queue) => LoadStatus::Loaded(queue),
                    Err(error) => LoadStatus::Failed(error.to_string()),
                };
                cx.notify();
            });
        })
        .detach();
    }

    fn load_devices(&mut self, cx: &mut Context<Self>) {
        self.devices = LoadStatus::Loading;
        let actions = Arc::clone(&self.actions);
        let task = cx.background_executor().spawn(async move { actions.load_devices() });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.devices = match result {
                    Ok(devices) => LoadStatus::Loaded(devices),
                    Err(error) => LoadStatus::Failed(error.to_string()),
                };
                cx.notify();
            });
        })
        .detach();
    }

    fn handle_search_key(
        &mut self,
        event: &KeyDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = event.keystroke.key.as_str();
        if event.keystroke.modifiers.secondary() && key.eq_ignore_ascii_case("v") {
            if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                self.search_query = text.chars().take(120).collect();
                self.search = SearchStatus::Idle;
                cx.notify();
            }
            cx.stop_propagation();
            return;
        }
        match key {
            "backspace" => {
                self.search_query.pop();
                self.search = SearchStatus::Idle;
            }
            "enter" => self.start_search(cx),
            // Space arrives with an empty `key_char` on some platforms, so insert it explicitly.
            "space" if self.search_query.chars().count() < 120 => {
                self.search_query.push(' ');
                self.search = SearchStatus::Idle;
            }
            _ if !event.keystroke.modifiers.modified()
                && self.search_query.chars().count() < 120 =>
            {
                if let Some(value) = event.keystroke.key_char.as_deref() {
                    self.search_query.push_str(value);
                    self.search = SearchStatus::Idle;
                }
            }
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }

    fn handle_client_id_key(
        &mut self,
        event: &KeyDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = event.keystroke.key.as_str();
        if event.keystroke.modifiers.secondary() && key.eq_ignore_ascii_case("v") {
            if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                self.client_id =
                    text.chars().filter(char::is_ascii_alphanumeric).take(64).collect();
                self.auth = AuthStatus::Editing;
                cx.notify();
            }
            cx.stop_propagation();
            return;
        }
        match key {
            "backspace" => {
                self.client_id.pop();
                self.auth = AuthStatus::Editing;
            }
            "enter" => self.start_authentication(false, cx),
            _ if !event.keystroke.modifiers.modified() && self.client_id.len() < 64 => {
                if let Some(value) = event.keystroke.key_char.as_deref() {
                    self.client_id.extend(value.chars().filter(char::is_ascii_alphanumeric));
                    self.client_id.truncate(64);
                    self.auth = AuthStatus::Editing;
                }
            }
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }

    fn open_dashboard(&mut self, cx: &mut Context<Self>) {
        if let Err(error) = self.actions.open_developer_dashboard() {
            self.auth = AuthStatus::Failed(error.to_string());
        }
        cx.notify();
    }

    #[allow(clippy::too_many_lines)] // Declarative GPUI layout reads most clearly as one surface.
    fn auth_screen(&self, window: &Window, cx: &Context<Self>) -> AnyElement {
        let focused = self.input_focus.is_focused(window);
        let focus_handle = self.input_focus.clone();
        let is_working = matches!(self.auth, AuthStatus::Working { .. });
        let status = match &self.auth {
            AuthStatus::Editing => None,
            AuthStatus::Working { restoring: true } => {
                Some(("Restoring your Spotify session…", SAGE))
            }
            AuthStatus::Working { restoring: false } => {
                Some(("Waiting for Spotify in your browser…", LAVENDER))
            }
            AuthStatus::Failed(message) => Some((message.as_str(), PEACH)),
        };

        div()
            .flex()
            .size_full()
            .bg(rgb(self.pal.canvas))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .justify_between()
                    .w(px(390.0))
                    .m_5()
                    .p_8()
                    .rounded_xl()
                    .bg(rgb(0x25_27_34))
                    .text_color(rgb(0xFF_F9_F2))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(div().size(px(34.0)).rounded_full().bg(rgb(PEACH)))
                            .child(
                                div()
                                    .text_xl()
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .child("Mellowdeck"),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_4()
                            .child(
                                div()
                                    .text_3xl()
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .child("Your music, with room to breathe."),
                            )
                            .child(div().text_base().text_color(rgb(0xC9_C7_C4)).child("A lightweight Spotify desktop client with a quiet, tactile interface and an optional turntable-inspired player."))
                            .child(
                                div()
                                    .flex()
                                    .gap_3()
                                    .child(div().size(px(12.0)).rounded_full().bg(rgb(PEACH)))
                                    .child(div().size(px(12.0)).rounded_full().bg(rgb(LAVENDER)))
                                    .child(div().size(px(12.0)).rounded_full().bg(rgb(SAGE))),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(0x9C_9B_A1))
                            .child("No telemetry · Tokens stay on this computer"),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .justify_center()
                    .flex_1()
                    .max_w(px(630.0))
                    .px_12()
                    .gap_5()
                    .child(
                        div()
                            .text_3xl()
                            .font_weight(gpui::FontWeight::BOLD)
                            .child("Connect Spotify"),
                    )
                    .child(div().text_base().text_color(rgb(self.pal.muted)).child("Mellowdeck needs your app's public Client ID. Never paste a Client Secret or password here."))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .child("Spotify Client ID"),
                            )
                            .child(
                                div()
                                    .id("client-id-input")
                                    .track_focus(&self.input_focus)
                                    .on_key_down(cx.listener(Self::handle_client_id_key))
                                    .on_click(move |_, window, _| focus_handle.focus(window))
                                    .cursor_text()
                                    .flex()
                                    .items_center()
                                    .h(px(48.0))
                                    .px_4()
                                    .rounded_lg()
                                    .bg(rgb(self.pal.surface))
                                    .border_1()
                                    .border_color(rgb(if focused { RED } else { self.pal.line }))
                                    .shadow_sm()
                                    .text_color(rgb(if self.client_id.is_empty() && !focused {
                                        0xA0_A0_A5
                                    } else {
                                        self.pal.ink
                                    }))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .child(if self.client_id.is_empty() && !focused {
                                                "Paste your Client ID".to_owned()
                                            } else {
                                                self.client_id.clone()
                                            })
                                            .when(focused, |element| {
                                                element.child(
                                                    div()
                                                        .w(px(2.0))
                                                        .h(px(20.0))
                                                        .bg(rgb(self.pal.ink)),
                                                )
                                            }),
                                    ),
                            )
                            .child(div().text_xs().text_color(rgb(self.pal.muted)).child("Register this redirect URI exactly: http://127.0.0.1:43821/callback")),
                    )
                    .when_some(status, |element, (message, color)| {
                        element.child(
                            div()
                                .p_3()
                                .rounded_lg()
                                .bg(rgb(color))
                                .text_sm()
                                .text_color(rgb(self.pal.ink))
                                .child(message.to_owned()),
                        )
                    })
                    .child(
                        div()
                            .flex()
                            .gap_3()
                            .child(
                                div()
                                    .id("spotify-login")
                                    .cursor_pointer()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.start_authentication(false, cx);
                                    }))
                                    .px_5()
                                    .py_3()
                                    .rounded_full()
                                    .bg(rgb(if is_working { 0xC7_C5_C2 } else { self.pal.ink }))
                                    .text_color(rgb(self.pal.surface))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .child(if is_working {
                                        "Connecting…"
                                    } else {
                                        "Log in with Spotify"
                                    }),
                            )
                            .child(
                                div()
                                    .id("spotify-dashboard")
                                    .cursor_pointer()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.open_dashboard(cx);
                                    }))
                                    .px_5()
                                    .py_3()
                                    .rounded_full()
                                    .border_1()
                                    .border_color(rgb(self.pal.line))
                                    .bg(rgb(self.pal.surface))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .child("Open developer dashboard"),
                            ),
                    )
                    .child(div().text_sm().text_color(rgb(self.pal.muted)).child("Spotify currently requires the owner of a Development Mode app to have Premium. Premium is also required for playback inside Mellowdeck.")),
            )
            .into_any_element()
    }

    fn rail_button(
        &self,
        id: &'static str,
        symbol: &'static str,
        route: Route,
        cx: &Context<Self>,
    ) -> AnyElement {
        let selected = self.state.navigation.current == route;
        div()
            .id(id)
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| {
                // Leaving the shared detail slot idle makes these tabs show their list, not a
                // stale artist/album page.
                if matches!(route, Route::Artists | Route::Albums) {
                    this.artist = LoadStatus::Idle;
                }
                // The Library tab fetches its albums and playlists the first time it opens.
                if route == Route::Albums && !matches!(this.library, LoadStatus::Loaded(_)) {
                    this.start_library_load(cx);
                }
                this.state.reduce(Action::Navigate(route.clone()));
                cx.notify();
            }))
            .flex()
            .items_center()
            .justify_center()
            .size(px(42.0))
            .rounded_full()
            .bg(rgb(if selected { RED } else { self.pal.surface }))
            .text_color(rgb(if selected { self.pal.surface } else { self.pal.ink }))
            .font_weight(gpui::FontWeight::BOLD)
            .child(symbol)
            .into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::unused_self)]
    fn listening_card(
        &self,
        title: &str,
        subtitle: &str,
        accent: u32,
        glyph: &str,
        artwork_url: Option<&str>,
        uri: Option<&str>,
        cx: &Context<Self>,
    ) -> AnyElement {
        let play_uri = uri.map(str::to_owned);
        let artwork = artwork_url.map_or_else(
            || {
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .size_full()
                    .text_3xl()
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(rgb(self.pal.ink))
                    .child(glyph.to_owned())
                    .into_any_element()
            },
            |url| {
                img(url.to_owned())
                    .size_full()
                    .object_fit(gpui::ObjectFit::Cover)
                    .into_any_element()
            },
        );
        // The glyph is unique per home section, so it keeps two cards distinct even when their top
        // item happens to share a title.
        div()
            .id(SharedString::from(format!("listening-card-{glyph}-{title}")))
            .cursor_pointer()
            .hover(|style| style.opacity(0.86))
            .on_click(cx.listener(move |this, _, _, cx| {
                if let Some(uri) = &play_uri {
                    this.activate_media(uri, cx);
                }
            }))
            .flex()
            .flex_col()
            .flex_1()
            .gap_2()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .h(px(168.0))
                    .rounded_xl()
                    .bg(rgb(accent))
                    .overflow_hidden()
                    .child(artwork),
            )
            .child(div().font_weight(gpui::FontWeight::SEMIBOLD).child(title.to_owned()))
            .child(div().text_sm().text_color(rgb(self.pal.muted)).child(subtitle.to_owned()))
            .into_any_element()
    }

    fn artist_row(&self, name: &str, listeners: &str, color: u32) -> AnyElement {
        div()
            .flex()
            .items_center()
            .gap_3()
            .child(div().size(px(46.0)).rounded_full().bg(rgb(color)))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(name.to_owned()),
                    )
                    .child(
                        div().text_xs().text_color(rgb(self.pal.muted)).child(listeners.to_owned()),
                    ),
            )
            .child(div().ml_auto().text_color(rgb(self.pal.muted)).child("›"))
            .into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::unused_self)]
    fn media_row(
        &self,
        title: &str,
        subtitle: &str,
        artwork_url: Option<&str>,
        uri: Option<&str>,
        color: u32,
        cx: &Context<Self>,
    ) -> AnyElement {
        let play_uri = uri.map(str::to_owned);
        let artwork = artwork_url.map_or_else(
            || div().size_full().bg(rgb(color)).into_any_element(),
            |url| {
                img(url.to_owned())
                    .size_full()
                    .object_fit(gpui::ObjectFit::Cover)
                    .into_any_element()
            },
        );
        // Key the row by its URI (unique) rather than its title, which can repeat across sections
        // and would otherwise give two rows the same ElementId and misroute clicks.
        let row_key = uri.unwrap_or(title);
        div()
            .id(SharedString::from(format!("media-row-{row_key}")))
            .cursor_pointer()
            .p_1()
            .rounded_lg()
            .hover(|style| style.bg(rgb(self.pal.surface)))
            .on_click(cx.listener(move |this, _, _, cx| {
                if let Some(uri) = &play_uri {
                    this.activate_media(uri, cx);
                }
            }))
            .flex()
            .items_center()
            .gap_3()
            .child(
                div().size(px(52.0)).rounded_lg().overflow_hidden().bg(rgb(color)).child(artwork),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(title.to_owned()),
                    )
                    .child(
                        div().text_xs().text_color(rgb(self.pal.muted)).child(subtitle.to_owned()),
                    ),
            )
            .child(div().ml_auto().text_color(rgb(self.pal.muted)).child("▶"))
            .into_any_element()
    }

    fn top_artist_rows(&self, cx: &Context<Self>) -> Vec<AnyElement> {
        match &self.home {
            HomeStatus::Loaded(home) if !home.top_artists.items.is_empty() => home
                .top_artists
                .items
                .iter()
                .take(3)
                .enumerate()
                .map(|(index, artist)| {
                    let subtitle = if artist.subtitle.is_empty() {
                        "Top artist"
                    } else {
                        artist.subtitle.as_str()
                    };
                    self.media_row(
                        &artist.title,
                        subtitle,
                        artist.artwork_url.as_deref(),
                        artist.uri.as_deref(),
                        [SAGE, LAVENDER, PEACH][index % 3],
                        cx,
                    )
                })
                .collect(),
            HomeStatus::Loaded(home) => vec![self.artist_row(
                "No top artists yet",
                home.top_artists.warning.as_deref().unwrap_or("Spotify returned no items"),
                SAGE,
            )],
            HomeStatus::Loading | HomeStatus::NotLoaded => vec![self.artist_row(
                "Loading your artists…",
                "Fetching personal Spotify data",
                LAVENDER,
            )],
            HomeStatus::Failed(message) => {
                vec![self.artist_row("Home sync failed", message, PEACH)]
            }
        }
    }

    fn home_sync_label(&self) -> &str {
        match self.home {
            HomeStatus::NotLoaded | HomeStatus::Loading => "Syncing with Spotify…",
            HomeStatus::Loaded(_) => "Spotify data loaded",
            HomeStatus::Failed(_) => "Sync needs attention",
        }
    }

    fn home_section(
        &self,
        select: impl FnOnce(&HomeSnapshot) -> &HomeSection,
    ) -> Option<&HomeSection> {
        match &self.home {
            HomeStatus::Loaded(home) => Some(select(home)),
            _ => None,
        }
    }

    fn section_card(
        &self,
        section: Option<&HomeSection>,
        fallback_title: &str,
        fallback_subtitle: &str,
        accent: u32,
        glyph: &str,
        cx: &Context<Self>,
    ) -> AnyElement {
        let item = section.and_then(|section| section.items.first());
        let title = item.map_or(fallback_title, |item| item.title.as_str());
        let subtitle = item.map_or_else(
            || section.and_then(|section| section.warning.as_deref()).unwrap_or(fallback_subtitle),
            |item| item.subtitle.as_str(),
        );
        self.listening_card(
            title,
            subtitle,
            accent,
            glyph,
            item.and_then(|item| item.artwork_url.as_deref()),
            item.and_then(|item| item.uri.as_deref()),
            cx,
        )
    }

    fn search_overlay(&self, window: &Window, cx: &Context<Self>) -> AnyElement {
        let focused = self.search_focus.is_focused(window);
        let focus_handle = self.search_focus.clone();
        let results = match &self.search {
            SearchStatus::Loaded(items) => items
                .iter()
                .map(|item| {
                    self.media_row(
                        &item.title,
                        &format!("{} · {}", item.kind, item.subtitle),
                        item.artwork_url.as_deref(),
                        item.uri.as_deref(),
                        LAVENDER,
                        cx,
                    )
                })
                .collect::<Vec<_>>(),
            _ => Vec::new(),
        };
        let message = match &self.search {
            SearchStatus::Idle => "Search tracks, albums, artists, and playlists.".to_owned(),
            SearchStatus::Loading => "Searching Spotify…".to_owned(),
            SearchStatus::Loaded(items) if items.is_empty() => "No results found.".to_owned(),
            SearchStatus::Loaded(items) => format!("{} results", items.len()),
            SearchStatus::Failed(error) => error.clone(),
        };
        div()
            .absolute()
            .top(px(82.0))
            .bottom(px(92.0))
            .left(px(0.0))
            .right(px(0.0))
            .flex()
            .flex_col()
            .gap_4()
            .p_8()
            .bg(rgb(self.pal.canvas))
            // Capture the mouse so clicks do not fall through to the home cards behind this overlay.
            .occlude()
            .child(div().text_3xl().font_weight(gpui::FontWeight::BOLD).child("Search"))
            .child(
                div()
                    .flex()
                    .gap_3()
                    .child(
                        div()
                            .id("catalog-search-input")
                            .track_focus(&self.search_focus)
                            .on_key_down(cx.listener(Self::handle_search_key))
                            .on_click(move |_, window, _| focus_handle.focus(window))
                            .cursor_text()
                            .flex()
                            .items_center()
                            .flex_1()
                            .h(px(48.0))
                            .px_4()
                            .rounded_lg()
                            .bg(rgb(self.pal.surface))
                            .border_1()
                            .border_color(rgb(if focused { RED } else { self.pal.line }))
                            .text_color(rgb(if self.search_query.is_empty() && !focused {
                                self.pal.muted
                            } else {
                                self.pal.ink
                            }))
                            // Show the placeholder only while unfocused; once focused, show the
                            // (possibly empty) query so the caret sits right after the last char.
                            .child(if self.search_query.is_empty() && !focused {
                                "What do you want to listen to?".to_owned()
                            } else {
                                self.search_query.clone()
                            })
                            .when(focused, |element| {
                                element.child(div().w(px(2.0)).h(px(20.0)).bg(rgb(self.pal.ink)))
                            }),
                    )
                    .child(
                        div()
                            .id("catalog-search-submit")
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| this.start_search(cx)))
                            .flex()
                            .items_center()
                            .px_6()
                            .rounded_full()
                            .bg(rgb(self.pal.ink))
                            .text_color(rgb(self.pal.surface))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child("Search"),
                    ),
            )
            .child(div().text_sm().text_color(rgb(self.pal.muted)).child(message))
            // Results scroll independently so a long list never hides behind the player bar.
            .child(
                div()
                    .id("search-results-scroll")
                    .flex_1()
                    .overflow_y_scroll()
                    .child(div().flex().flex_col().gap_3().pb_4().children(results)),
            )
            .into_any_element()
    }

    /// Builds the hero header for a detail page: a faint banner behind a portrait, the name, some
    /// metadata, and Play / Shuffle buttons that start the whole album or artist.
    #[allow(clippy::too_many_lines)]
    fn detail_header(&self, detail: &ArtistDetail, cx: &Context<Self>) -> AnyElement {
        let pal = self.pal;
        let portrait_image = detail.artwork_url.as_deref().map_or_else(
            || div().size_full().bg(rgb(LAVENDER)).into_any_element(),
            |url| {
                img(url.to_owned())
                    .size_full()
                    .object_fit(gpui::ObjectFit::Cover)
                    .into_any_element()
            },
        );
        // Albums show a rounded square cover; artists a circular portrait.
        let portrait = div()
            .size(px(128.0))
            .when(detail.is_album, gpui::Styled::rounded_xl)
            .when(!detail.is_album, gpui::Styled::rounded_full)
            .overflow_hidden()
            .bg(rgb(LAVENDER))
            .shadow_md()
            .child(portrait_image);

        // A metadata line that always names the type and, for albums, the track count.
        let meta = if detail.is_album {
            format!("{} · {} tracks", detail.description, detail.songs.len())
        } else {
            detail.description.clone()
        };

        let play_uri = detail.uri.clone();
        let shuffle_uri = detail.uri.clone();
        let play_button = div()
            .id("detail-play")
            .cursor_pointer()
            .hover(|style| style.opacity(0.85))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.send_playback(
                    PlaybackCommand::PlayContext { uri: play_uri.clone(), shuffle: false },
                    cx,
                );
            }))
            .flex()
            .items_center()
            .gap_2()
            .px_5()
            .py_2()
            .rounded_full()
            .bg(rgb(RED))
            .text_color(rgb(0xFF_FF_FF))
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .child("▶")
            .child("Play");
        let shuffle_button = div()
            .id("detail-shuffle")
            .cursor_pointer()
            .hover(|style| style.opacity(0.85))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.send_playback(
                    PlaybackCommand::PlayContext { uri: shuffle_uri.clone(), shuffle: true },
                    cx,
                );
            }))
            .flex()
            .items_center()
            .gap_2()
            .px_5()
            .py_2()
            .rounded_full()
            .bg(rgb(pal.surface))
            .border_1()
            .border_color(rgb(pal.line))
            .text_color(rgb(pal.ink))
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .child("🔀")
            .child("Shuffle");

        let content =
            div().relative().flex().items_center().gap_5().size_full().p_6().child(portrait).child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(rgb(pal.muted))
                            .child(if detail.is_album { "ALBUM" } else { "ARTIST" }),
                    )
                    .child(
                        div()
                            .text_3xl()
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(rgb(pal.ink))
                            .child(detail.name.clone()),
                    )
                    .child(div().text_sm().text_color(rgb(pal.muted)).child(meta))
                    .child(div().flex().gap_3().mt_1().child(play_button).child(shuffle_button)),
            );

        div()
            .relative()
            .h(px(212.0))
            .rounded_xl()
            .overflow_hidden()
            .bg(rgb(LAVENDER))
            .when_some(detail.banner_url.clone(), |element, banner| {
                element.child(
                    img(banner)
                        .absolute()
                        .size_full()
                        .object_fit(gpui::ObjectFit::Cover)
                        .opacity(0.30),
                )
            })
            .child(content)
            .into_any_element()
    }

    /// Builds one detail column: a heading over a stack of media rows.
    fn detail_column(heading: &str, rows: Vec<AnyElement>) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(320.0))
            .gap_2()
            .child(div().text_xl().font_weight(gpui::FontWeight::BOLD).child(heading.to_owned()))
            .child(div().flex().flex_col().gap_2().children(rows))
            .into_any_element()
    }

    fn artist_overlay(&self, cx: &Context<Self>) -> AnyElement {
        let (header, message, is_album, songs, albums) = match &self.artist {
            LoadStatus::Loaded(detail) => {
                let songs = detail
                    .songs
                    .iter()
                    .map(|song| {
                        self.media_row(
                            &song.title,
                            &song.subtitle,
                            song.artwork_url.as_deref(),
                            song.uri.as_deref(),
                            PEACH,
                            cx,
                        )
                    })
                    .collect::<Vec<_>>();
                let albums = detail
                    .albums
                    .iter()
                    .map(|album| {
                        self.media_row(
                            &album.title,
                            &album.subtitle,
                            album.artwork_url.as_deref(),
                            album.uri.as_deref(),
                            SAGE,
                            cx,
                        )
                    })
                    .collect::<Vec<_>>();
                (
                    self.detail_header(detail, cx),
                    detail.warning.clone(),
                    detail.is_album,
                    songs,
                    albums,
                )
            }
            LoadStatus::Loading => (
                div()
                    .text_2xl()
                    .font_weight(gpui::FontWeight::BOLD)
                    .child("Loading…")
                    .into_any_element(),
                None,
                false,
                Vec::new(),
                Vec::new(),
            ),
            LoadStatus::Failed(error) => (
                div()
                    .text_2xl()
                    .font_weight(gpui::FontWeight::BOLD)
                    .child("Unavailable")
                    .into_any_element(),
                Some(error.clone()),
                false,
                Vec::new(),
                Vec::new(),
            ),
            LoadStatus::Idle => unreachable!("detail overlay is not rendered while idle"),
        };

        // Lay the two result columns side by side; they wrap to a single column when the window is
        // too narrow to hold both.
        let mut columns = div().flex().flex_wrap().gap_6();
        if !songs.is_empty() {
            let heading = if is_album { "Tracks" } else { "Popular results" };
            columns = columns.child(Self::detail_column(heading, songs));
        }
        if !albums.is_empty() {
            columns = columns.child(Self::detail_column("Albums & singles", albums));
        }

        div()
            .id("artist-detail-scroll")
            .absolute()
            .top(px(82.0))
            .bottom(px(92.0))
            .left(px(0.0))
            .right(px(0.0))
            .flex()
            .flex_col()
            .gap_5()
            .p_8()
            .overflow_y_scroll()
            .bg(rgb(self.pal.canvas))
            .occlude()
            .child(header)
            .when_some(message, |element, message| {
                element.child(div().text_sm().text_color(rgb(RED)).child(message))
            })
            .child(columns)
            .into_any_element()
    }

    fn route_overlay(&self, window: &Window, cx: &Context<Self>) -> Option<AnyElement> {
        // Artist and album detail pages share one slot and overlay their originating tab.
        if matches!(self.state.navigation.current, Route::Artists | Route::Albums)
            && !matches!(self.artist, LoadStatus::Idle)
        {
            return Some(self.artist_overlay(cx));
        }
        let (title, description, section) = match self.state.navigation.current {
            Route::Home => return None,
            Route::Search => return Some(self.search_overlay(window, cx)),
            Route::Albums => return Some(self.library_overlay(cx)),
            Route::Settings => return Some(self.settings_overlay(cx)),
            Route::LikedSongs => {
                ("Liked Songs", "Your full saved-track table is being connected next.", None)
            }
            Route::Artists => (
                "Top Artists",
                "Artists from your medium-term Spotify listening history.",
                self.home_section(|home| &home.top_artists),
            ),
            Route::Playlist(_) | Route::Onboarding => {
                ("Playlist", "Playlist details have not been loaded yet.", None)
            }
        };
        let rows = section.map_or_else(Vec::new, |section| {
            section
                .items
                .iter()
                .map(|item| {
                    self.media_row(
                        &item.title,
                        &item.subtitle,
                        item.artwork_url.as_deref(),
                        item.uri.as_deref(),
                        LAVENDER,
                        cx,
                    )
                })
                .collect()
        });
        Some(
            div()
                .absolute()
                .top(px(82.0))
                .bottom(px(92.0))
                .left(px(0.0))
                .right(px(0.0))
                .flex()
                .flex_col()
                .gap_5()
                .p_8()
                .bg(rgb(self.pal.canvas))
                // Capture the mouse so clicks do not fall through to the home cards behind.
                .occlude()
                .child(div().text_3xl().font_weight(gpui::FontWeight::BOLD).child(title))
                .child(div().text_base().text_color(rgb(self.pal.muted)).child(description))
                .child(
                    div()
                        .id("refresh-home-data")
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _, _, cx| this.start_home_load(cx)))
                        .w(px(150.0))
                        .px_5()
                        .py_3()
                        .rounded_full()
                        .bg(rgb(self.pal.ink))
                        .text_color(rgb(self.pal.surface))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child("Refresh Spotify"),
                )
                // The list scrolls so long results never hide behind the player bar.
                .child(
                    div()
                        .id("route-list-scroll")
                        .flex_1()
                        .overflow_y_scroll()
                        .child(div().flex().flex_col().gap_2().pb_4().children(rows)),
                )
                .into_any_element(),
        )
    }

    /// A page-level scaffold shared by the Library and Settings overlays: a scrolling, mouse-
    /// capturing surface with a big title and subtitle.
    fn overlay_page(&self, id: &'static str, title: &str, subtitle: &str) -> Stateful<Div> {
        div()
            .id(id)
            .absolute()
            .top(px(82.0))
            .bottom(px(92.0))
            .left(px(0.0))
            .right(px(0.0))
            .flex()
            .flex_col()
            .gap_5()
            .p_8()
            .overflow_y_scroll()
            .bg(rgb(self.pal.canvas))
            .occlude()
            .child(div().text_3xl().font_weight(gpui::FontWeight::BOLD).child(title.to_owned()))
            .child(div().text_base().text_color(rgb(self.pal.muted)).child(subtitle.to_owned()))
    }

    /// A titled grid of artwork cards for one library collection (albums or playlists).
    fn library_grid(&self, heading: &str, section: &HomeSection, cx: &Context<Self>) -> AnyElement {
        let cards = section
            .items
            .iter()
            .map(|item| {
                self.listening_card(
                    &item.title,
                    &item.subtitle,
                    LAVENDER,
                    "♪",
                    item.artwork_url.as_deref(),
                    item.uri.as_deref(),
                    cx,
                )
            })
            .collect::<Vec<_>>();
        let body: AnyElement = if cards.is_empty() {
            let message = section.warning.as_deref().unwrap_or("Nothing saved here yet.");
            div()
                .text_sm()
                .text_color(rgb(self.pal.muted))
                .child(message.to_owned())
                .into_any_element()
        } else {
            // Fixed-width cards that wrap onto as many rows as the width allows.
            let mut grid = div().flex().flex_wrap().gap_4();
            for card in cards {
                grid = grid.child(div().w(px(180.0)).child(card));
            }
            grid.into_any_element()
        };
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(div().text_xl().font_weight(gpui::FontWeight::BOLD).child(heading.to_owned()))
            .child(body)
            .into_any_element()
    }

    /// The Library tab: every saved album and playlist, as artwork grids.
    fn library_overlay(&self, cx: &Context<Self>) -> AnyElement {
        let page = self.overlay_page(
            "library-scroll",
            "Your Library",
            "Every album and playlist you saved on Spotify.",
        );
        match &self.library {
            LoadStatus::Loaded(library) => page
                .child(self.library_grid("Albums", &library.albums, cx))
                .child(self.library_grid("Playlists", &library.playlists, cx))
                .child(div().h(px(8.0))),
            LoadStatus::Loading | LoadStatus::Idle => page.child(
                div().text_sm().text_color(rgb(self.pal.muted)).child("Loading your library…"),
            ),
            LoadStatus::Failed(error) => page.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(div().text_sm().text_color(rgb(RED)).child(error.clone()))
                    .child(
                        div()
                            .id("library-retry")
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| this.start_library_load(cx)))
                            .w(px(120.0))
                            .px_5()
                            .py_3()
                            .rounded_full()
                            .bg(rgb(self.pal.ink))
                            .text_color(rgb(self.pal.surface))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child("Retry"),
                    ),
            ),
        }
        .into_any_element()
    }

    /// One selectable theme option in Settings.
    fn theme_option(
        &self,
        id: &'static str,
        label: &str,
        preference: ThemePreference,
        cx: &Context<Self>,
    ) -> AnyElement {
        // The default `System` preference renders as light, so highlight the Light option for it.
        let selected = self.theme == preference
            || (self.theme == ThemePreference::System
                && preference == ThemePreference::PastelLight);
        div()
            .id(id)
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| this.set_theme(preference, cx)))
            .flex()
            .items_center()
            .justify_center()
            .px_6()
            .py_3()
            .rounded_full()
            .bg(rgb(if selected { self.pal.ink } else { self.pal.surface }))
            .border_1()
            .border_color(rgb(if selected { self.pal.ink } else { self.pal.line }))
            .text_color(rgb(if selected { self.pal.surface } else { self.pal.ink }))
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .child(label.to_owned())
            .into_any_element()
    }

    /// The Settings tab. Currently hosts the appearance (theme) switcher.
    fn settings_overlay(&self, cx: &Context<Self>) -> AnyElement {
        self.overlay_page(
            "settings-scroll",
            "Settings",
            "Personalize how Mellowdeck looks and behaves.",
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_3()
                .child(div().text_xl().font_weight(gpui::FontWeight::BOLD).child("Appearance"))
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(self.pal.muted))
                        .child("Choose a light or dark theme for the whole app."),
                )
                .child(
                    div()
                        .flex()
                        .gap_3()
                        .child(self.theme_option(
                            "theme-light",
                            "Light",
                            ThemePreference::PastelLight,
                            cx,
                        ))
                        .child(self.theme_option(
                            "theme-dark",
                            "Dark",
                            ThemePreference::InkDark,
                            cx,
                        )),
                ),
        )
        .into_any_element()
    }

    #[allow(clippy::unused_self)]
    fn playback_control(
        &self,
        label: &'static str,
        command: PlaybackCommand,
        primary: bool,
        cx: &Context<Self>,
    ) -> AnyElement {
        let directional = !primary;
        let accessible_label = match command {
            PlaybackCommand::Previous => "Previous track",
            PlaybackCommand::Next => "Next track",
            PlaybackCommand::Pause => "Pause",
            PlaybackCommand::Resume => "Play",
            PlaybackCommand::Seek(_) => "Seek",
            _ => "Playback control",
        };
        div()
            .id(SharedString::from(format!("playback-control-{label}")))
            .cursor_pointer()
            .tooltip(text_tooltip(accessible_label))
            .hover(
                move |style| {
                    if directional { style.bg(rgb(0xB9_2D_3A)) } else { style.opacity(0.72) }
                },
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.send_playback(command.clone(), cx);
            }))
            .flex()
            .items_center()
            .justify_center()
            .size(px(if primary { 38.0 } else { 34.0 }))
            .rounded_full()
            .bg(rgb(if primary { self.pal.ink } else { RED }))
            .text_color(rgb(self.pal.surface))
            .active(|style| style.opacity(0.72))
            .child(label)
            .into_any_element()
    }

    #[allow(clippy::too_many_lines)]
    fn player_popup(&self, cx: &Context<Self>) -> Option<AnyElement> {
        let panel = self.player_panel?;
        let content = match panel {
            PlayerPanel::Volume => self.volume_slider(cx),
            PlayerPanel::Queue => {
                let rows = match &self.queue {
                    LoadStatus::Loaded(queue) => {
                        let mut rows = Vec::new();
                        if let Some(item) = &queue.current {
                            rows.push(self.media_row(
                                &item.title,
                                &format!("Now playing · {}", item.subtitle),
                                item.artwork_url.as_deref(),
                                item.uri.as_deref(),
                                SAGE,
                                cx,
                            ));
                        }
                        rows.extend(queue.upcoming.iter().take(5).map(|item| {
                            self.media_row(
                                &item.title,
                                &item.subtitle,
                                item.artwork_url.as_deref(),
                                item.uri.as_deref(),
                                LAVENDER,
                                cx,
                            )
                        }));
                        if rows.is_empty() {
                            rows.push(self.artist_row(
                                "Queue is empty",
                                "Start something on Spotify",
                                PEACH,
                            ));
                        }
                        rows
                    }
                    LoadStatus::Failed(error) => {
                        vec![self.artist_row("Queue unavailable", error, PEACH)]
                    }
                    LoadStatus::Idle | LoadStatus::Loading => {
                        vec![self.artist_row("Loading queue…", "Spotify Connect", SAGE)]
                    }
                };
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(div().font_weight(gpui::FontWeight::SEMIBOLD).child("Up next"))
                    .children(rows)
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(self.pal.muted))
                            .child("Spotify does not provide queue reorder or removal."),
                    )
                    .into_any_element()
            }
            PlayerPanel::Devices => {
                let rows = match &self.devices {
                    LoadStatus::Loaded(devices) if devices.is_empty() => {
                        vec![self.artist_row("No devices found", "Open Spotify on a device", PEACH)]
                    }
                    LoadStatus::Loaded(devices) => devices
                        .iter()
                        .map(|device| {
                            let id = device.id.clone();
                            let subtitle = format!(
                                "{}{}{}",
                                device.kind,
                                if device.active { " · active" } else { "" },
                                if device.restricted { " · restricted" } else { "" }
                            );
                            div()
                                .id(SharedString::from(format!("playback-device-{id}")))
                                .cursor_pointer()
                                .rounded_lg()
                                .hover(|style| style.bg(rgb(self.pal.canvas)))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.send_playback(PlaybackCommand::Transfer(id.clone()), cx);
                                }))
                                .child(self.artist_row(&device.name, &subtitle, SAGE))
                                .into_any_element()
                        })
                        .collect(),
                    LoadStatus::Failed(error) => {
                        vec![self.artist_row("Devices unavailable", error, PEACH)]
                    }
                    LoadStatus::Idle | LoadStatus::Loading => {
                        vec![self.artist_row("Finding devices…", "Spotify Connect", SAGE)]
                    }
                };
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(div().font_weight(gpui::FontWeight::SEMIBOLD).child("Play on a device"))
                    // If the built-in player could not start, say so unmistakably instead of
                    // listing it as a device that will never work.
                    .when(self.local_player_failed, |element| {
                        element.child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .p_3()
                                .rounded_lg()
                                .bg(rgb(RED))
                                .text_color(rgb(self.pal.surface))
                                .child(
                                    div()
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .child("Built-in player unavailable"),
                                )
                                .child(div().text_xs().child(
                                    self.local_player_status.clone().unwrap_or_else(|| {
                                        "Play on another Spotify device instead.".to_owned()
                                    }),
                                )),
                        )
                    })
                    .children(rows)
                    .into_any_element()
            }
        };
        // The volume popup floats above its icon and stays open on hover; the queue and device
        // popups are wider, click-opened, and close when the pointer clicks outside them.
        let shell = div()
            .id("player-popup")
            .occlude()
            .absolute()
            .bottom(px(96.0))
            .p_4()
            .rounded_xl()
            .bg(rgb(self.pal.surface))
            .border_1()
            .border_color(rgb(self.pal.line))
            .shadow_md()
            .child(content);
        let shell = match panel {
            PlayerPanel::Volume => shell
                .right(px(104.0))
                .on_hover(cx.listener(|this, hovering: &bool, _, cx| {
                    this.hover_volume(*hovering, cx);
                }))
                .on_scroll_wheel(cx.listener(Self::scroll_volume)),
            PlayerPanel::Queue | PlayerPanel::Devices => shell
                .right(px(12.0))
                .w(px(340.0))
                .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                    this.player_panel = None;
                    cx.notify();
                })),
        };
        Some(shell.into_any_element())
    }

    /// A round, consistently styled icon button used across the player bar.
    fn icon_button(
        &self,
        id: impl Into<SharedString>,
        glyph: &str,
        active: bool,
        color: u32,
    ) -> Stateful<Div> {
        let pal = self.pal;
        div()
            .id(id.into())
            .flex()
            .items_center()
            .justify_center()
            .size(px(36.0))
            .rounded_full()
            .bg(rgb(if active { pal.canvas } else { pal.surface }))
            .text_color(rgb(color))
            .cursor_pointer()
            .hover(move |style| style.bg(rgb(pal.canvas)))
            .active(|style| style.opacity(0.7))
            .child(glyph.to_owned())
    }

    /// The title the player bar should show when nothing useful is playing.
    fn playback_placeholder_title(&self) -> String {
        match &self.playback {
            LoadStatus::Failed(_) => "Playback unavailable".to_owned(),
            LoadStatus::Loading => "Updating playback…".to_owned(),
            LoadStatus::Idle | LoadStatus::Loaded(_) => "Nothing playing".to_owned(),
        }
    }

    /// A fixed-slot cover that always has a neutral visual fallback. The image is layered over
    /// the placeholder, so a missing URL or a failed asynchronous image load never leaves a
    /// broken-image artifact in the player.
    fn player_artwork(&self, artwork_url: Option<&str>) -> AnyElement {
        let placeholder = div()
            .absolute()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgb(LAVENDER))
            .text_2xl()
            .text_color(rgb(self.pal.muted))
            .child("♫");
        let cover = div().relative().size_full().child(placeholder);
        match artwork_url {
            Some(url) => cover
                .child(img(url.to_owned()).size_full().object_fit(ObjectFit::Cover))
                .into_any_element(),
            None => cover.into_any_element(),
        }
    }
    /// Left card: cover art, the track title, and each artist as a clickable link.
    fn now_playing_card(&self, cx: &Context<Self>) -> AnyElement {
        let playback = self.playback.as_loaded();
        let title = playback
            .filter(|item| !item.title.is_empty())
            .map_or_else(|| self.playback_placeholder_title(), |item| item.title.clone());
        let artwork = self.player_artwork(playback.and_then(|item| item.artwork_url.as_deref()));

        // Render each artist as its own clickable chip; fall back to the joined subtitle.
        let artists = playback.map(|item| item.artists.clone()).unwrap_or_default();
        let artist_line = if artists.is_empty() {
            let subtitle = playback.filter(|item| !item.subtitle.is_empty()).map_or_else(
                || match &self.playback {
                    LoadStatus::Failed(error) => error.clone(),
                    _ => "Choose a track or Spotify device".to_owned(),
                },
                |item| item.subtitle.clone(),
            );
            div()
                .min_w(px(0.0))
                .truncate()
                .text_sm()
                .text_color(rgb(self.pal.muted))
                .child(subtitle)
                .into_any_element()
        } else {
            let mut row = div()
                .flex()
                .min_w(px(0.0))
                .truncate()
                .items_center()
                .text_sm()
                .text_color(rgb(self.pal.muted));
            for (index, (name, uri)) in artists.into_iter().enumerate() {
                if index > 0 {
                    row = row.child(div().child(", "));
                }
                if let Some(uri) = uri {
                    row = row.child(
                        div()
                            .id(("now-playing-artist", index))
                            .cursor_pointer()
                            .hover(|style| style.text_color(rgb(self.pal.ink)))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.activate_media(&uri, cx);
                            }))
                            .child(name),
                    );
                } else {
                    row = row.child(div().child(name));
                }
            }
            row.into_any_element()
        };

        div()
            .flex()
            .items_center()
            .gap_4()
            .w(px(300.0))
            .min_w(px(300.0))
            .child(
                div()
                    .size(px(56.0))
                    .flex_none()
                    .rounded_lg()
                    .overflow_hidden()
                    .bg(rgb(LAVENDER))
                    .shadow_sm()
                    .child(artwork),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w(px(0.0))
                    .gap_1()
                    .overflow_hidden()
                    .child(self.marquee_title(title))
                    .child(artist_line),
            )
            .into_any_element()
    }

    /// Renders the now-playing title within the metadata region without allowing it to run
    /// beneath the fixed artwork.
    fn marquee_title(&self, title: String) -> AnyElement {
        div()
            .w_full()
            .min_w(px(0.0))
            .truncate()
            .text_base()
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .text_color(rgb(self.pal.ink))
            .child(title)
            .into_any_element()
    }
    /// The waveform strip: a track-shaped bar chart that fills to the playhead, pulses while
    /// playing, and seeks on click, drag, and hover.
    #[allow(clippy::cast_precision_loss)]
    fn waveform(&self, cx: &Context<Self>) -> AnyElement {
        let playback = self.playback.as_loaded();
        let playing = playback.is_some_and(|item| item.playing);
        let duration_ms = playback.map_or(0, |item| item.duration_ms);
        let displayed_progress_ms = self
            .scratch_preview_ms
            .or_else(|| playback.map(|item| item.progress_ms))
            .unwrap_or_default();
        let progress = if duration_ms == 0 {
            0.0
        } else {
            (displayed_progress_ms as f32 / duration_ms as f32).clamp(0.0, 1.0)
        };
        let seed = playback.map_or(0, |item| waveform_seed(&item.title, item.duration_ms));
        let bars = waveform_bars(seed, WAVEFORM_BARS);
        let bar_height = 34.0_f32;
        let rest_color = self.pal.line;

        let row = div().flex().items_center().gap(px(2.0)).absolute().size_full();
        let row = if playing {
            let animated_bars = bars.clone();
            row.with_animation(
                "waveform-pulse",
                Animation::new(Duration::from_millis(1_400)).repeat(),
                move |row, delta| {
                    row.children(waveform_bar_elements(
                        &animated_bars,
                        progress,
                        bar_height,
                        Some(delta),
                        rest_color,
                    ))
                },
            )
            .into_any_element()
        } else {
            row.children(waveform_bar_elements(&bars, progress, bar_height, None, rest_color))
                .into_any_element()
        };

        let bounds_probe = {
            let handle = cx.weak_entity();
            canvas(
                move |bounds, _window, cx| {
                    let _ = handle.update(cx, |this, _| this.waveform_bounds = Some(bounds));
                },
                |_bounds, (), _window, _cx| {},
            )
            .absolute()
            .size_full()
        };

        // Hover preview: a thin line under the pointer with the time it would seek to.
        let hover_line = self.waveform_hover_ms.filter(|_| duration_ms > 0).map(|hover_ms| {
            let fraction = (hover_ms as f32 / duration_ms as f32).clamp(0.0, 1.0);
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .left(relative(fraction))
                .flex()
                .flex_col()
                .items_center()
                .child(div().w(px(1.5)).h_full().bg(rgb(self.pal.muted)))
                .child(
                    div()
                        .absolute()
                        .top(px(-14.0))
                        .text_xs()
                        .text_color(rgb(self.pal.ink))
                        .child(format_time(hover_ms)),
                )
        });

        div()
            .id("waveform")
            .relative()
            .flex_1()
            .h(px(bar_height))
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, cx.listener(Self::waveform_mouse_down))
            .on_mouse_move(cx.listener(Self::waveform_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::waveform_mouse_up))
            .on_hover(cx.listener(|this, hovering: &bool, _, cx| {
                if !hovering {
                    this.clear_waveform_hover(cx);
                }
            }))
            .child(bounds_probe)
            .child(row)
            .when_some(hover_line, gpui::ParentElement::child)
            .into_any_element()
    }

    /// Middle card: the half-peeking vinyl disc, transport controls, waveform, and times.
    #[allow(clippy::cast_precision_loss, clippy::too_many_lines)]
    fn transport_card(&self, cx: &Context<Self>) -> AnyElement {
        let playback = self.playback.as_loaded();
        let playing = playback.is_some_and(|item| item.playing);
        let duration_ms = playback.map_or(0, |item| item.duration_ms);
        let displayed_progress_ms = self
            .scratch_preview_ms
            .or_else(|| playback.map(|item| item.progress_ms))
            .unwrap_or_default();
        let toggle = if playing { PlaybackCommand::Pause } else { PlaybackCommand::Resume };

        // The vinyl disc peeks out from behind the card's left edge, spinning while playing.
        let marker = div().absolute().size(px(6.0)).rounded_full().bg(rgb(self.pal.surface));
        let marker = if playing {
            marker
                .with_animation(
                    "vinyl-spin",
                    Animation::new(Duration::from_millis(1_800)).repeat(),
                    |marker, delta| {
                        let radians = delta * std::f32::consts::TAU;
                        marker
                            .left(px(33.0 + radians.cos() * 24.0))
                            .top(px(33.0 + radians.sin() * 24.0))
                    },
                )
                .into_any_element()
        } else {
            marker.left(px(33.0)).top(px(9.0)).into_any_element()
        };
        let disc = div()
            .id("vinyl-scrubber")
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, cx.listener(Self::start_scratching))
            .on_mouse_move(cx.listener(Self::move_scratch))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::finish_scratch))
            .absolute()
            .left(px(-6.0))
            .top(px(2.0))
            .flex()
            .items_center()
            .justify_center()
            .size(px(72.0))
            .rounded_full()
            .bg(rgb(0x20_21_27))
            .border_1()
            .border_color(rgb(0x49_49_50))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(42.0))
                    .rounded_full()
                    .border_1()
                    .border_color(rgb(0x4F_50_58))
                    .child(div().size(px(18.0)).rounded_full().bg(rgb(PEACH))),
            )
            .child(marker);

        div()
            .id("transport-card")
            .relative()
            .flex()
            .items_center()
            .flex_1()
            .min_w(px(320.0))
            .gap_4()
            .pl(px(80.0))
            .child(disc)
            // Transport controls sit just to the right of the vinyl.
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(self.playback_control("⏮", PlaybackCommand::Previous, false, cx))
                    .child(self.playback_control(if playing { "⏸" } else { "▶" }, toggle, true, cx))
                    .child(self.playback_control("⏭", PlaybackCommand::Next, false, cx)),
            )
            // Waveform and times take the flexible middle.
            .child(
                div()
                    .flex()
                    .items_center()
                    .flex_1()
                    .gap_3()
                    .child(
                        div()
                            .w(px(34.0))
                            .text_xs()
                            .text_color(rgb(self.pal.muted))
                            .child(format_time(displayed_progress_ms)),
                    )
                    .child(self.waveform(cx))
                    .child(
                        div()
                            .w(px(34.0))
                            .text_xs()
                            .text_color(rgb(self.pal.muted))
                            .child(format_time(duration_ms)),
                    ),
            )
            .into_any_element()
    }

    /// The vertical volume slider shown inside the volume popup.
    #[allow(clippy::cast_precision_loss)]
    fn volume_slider(&self, cx: &Context<Self>) -> AnyElement {
        let volume = self.current_volume();
        let fraction = f32::from(volume) / 100.0;
        let track = {
            let handle = cx.weak_entity();
            div()
                .id("volume-track")
                .relative()
                .w(px(8.0))
                .h(px(140.0))
                .rounded_full()
                .bg(rgb(self.pal.line))
                .cursor_pointer()
                .on_mouse_down(MouseButton::Left, cx.listener(Self::volume_slider_down))
                .on_mouse_move(cx.listener(Self::volume_slider_move))
                .on_mouse_up(MouseButton::Left, cx.listener(Self::volume_slider_up))
                .child(
                    canvas(
                        move |bounds, _window, cx| {
                            let _ = handle
                                .update(cx, |this, _| this.volume_slider_bounds = Some(bounds));
                        },
                        |_bounds, (), _window, _cx| {},
                    )
                    .absolute()
                    .size_full(),
                )
                // Filled portion grows from the bottom.
                .child(
                    div()
                        .absolute()
                        .bottom_0()
                        .left_0()
                        .right_0()
                        .h(relative(fraction))
                        .rounded_full()
                        .bg(rgb(RED)),
                )
                // Thumb.
                .child(
                    div()
                        .absolute()
                        .left(px(-3.0))
                        .bottom(relative(fraction))
                        .mb(px(-7.0))
                        .size(px(14.0))
                        .rounded_full()
                        .bg(rgb(self.pal.surface))
                        .border_1()
                        .border_color(rgb(self.pal.ink)),
                )
        };
        div()
            .flex()
            .flex_col()
            .items_center()
            .gap_3()
            .child(
                div().text_xs().font_weight(gpui::FontWeight::SEMIBOLD).child(format!("{volume}%")),
            )
            .child(div().flex().justify_center().w(px(24.0)).child(track))
            .into_any_element()
    }

    /// The volume / queue / device icon buttons shown at the right of the transport card.
    fn output_controls(&self, cx: &Context<Self>) -> AnyElement {
        let volume = self.current_volume();
        let volume_glyph = if volume == 0 {
            "🔇"
        } else if volume < 50 {
            "🔉"
        } else {
            "🔊"
        };
        // When audio is coming from another device, the device icon changes shape and colour so it
        // is obvious the sound is not on this computer.
        let remote = self.another_device_is_playing();
        let device_glyph = if remote { "📡" } else { "🖥" };
        let device_color = if remote { RED } else { self.pal.ink };

        let volume_button = self
            .icon_button(
                "player-volume",
                volume_glyph,
                self.player_panel == Some(PlayerPanel::Volume),
                self.pal.ink,
            )
            .tooltip(text_tooltip("Volume"))
            .on_hover(cx.listener(|this, hovering: &bool, _, cx| {
                this.hover_volume(*hovering, cx);
            }))
            .on_scroll_wheel(cx.listener(Self::scroll_volume))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    if event.click_count >= 2 {
                        this.toggle_mute(cx);
                    }
                }),
            );

        let queue_button = self
            .icon_button(
                "player-queue",
                "≡",
                self.player_panel == Some(PlayerPanel::Queue),
                self.pal.ink,
            )
            .tooltip(text_tooltip("Queue"))
            .on_click(cx.listener(|this, _, _, cx| {
                this.toggle_player_panel(PlayerPanel::Queue, cx);
            }));

        let device_button = self
            .icon_button(
                "player-device",
                device_glyph,
                self.player_panel == Some(PlayerPanel::Devices),
                device_color,
            )
            .tooltip(text_tooltip(if remote { "Playing on another device" } else { "Devices" }))
            .on_click(cx.listener(|this, _, _, cx| {
                this.toggle_player_panel(PlayerPanel::Devices, cx);
            }));

        div()
            .flex()
            .items_center()
            .gap_2()
            .child(volume_button)
            .child(queue_button)
            .child(device_button)
            .into_any_element()
    }

    /// The full-width bottom bar: now-playing flush on the left, transport in the middle, and the
    /// volume / queue / device controls on the right.
    fn player_bar(&self, cx: &Context<Self>) -> AnyElement {
        div()
            .id("player-bar")
            .flex()
            .items_center()
            .gap_4()
            .w_full()
            .h(px(84.0))
            .px_5()
            .bg(rgb(self.pal.surface))
            .border_t_1()
            .border_color(rgb(self.pal.line))
            .child(self.now_playing_card(cx))
            .child(self.transport_card(cx))
            .child(self.output_controls(cx))
            .into_any_element()
    }

    #[allow(clippy::too_many_lines)] // Declarative GPUI layout reads most clearly as one surface.
    fn home_screen(&self, window: &Window, cx: &Context<Self>) -> AnyElement {
        let display_name =
            self.account.as_ref().map_or("listener", |account| account.display_name.as_str());
        div()
            .flex()
            .size_full()
            .bg(rgb(self.pal.canvas))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_3()
                    .w(px(72.0))
                    .py_5()
                    .child(div().size(px(32.0)).rounded_full().bg(rgb(RED)))
                    .child(div().h(px(72.0)))
                    .child(self.rail_button("nav-home", "⌂", Route::Home, cx))
                    .child(self.rail_button("nav-search", "⌕", Route::Search, cx))
                    .child(self.rail_button("nav-liked", "♥", Route::LikedSongs, cx))
                    .child(self.rail_button("nav-library", "▤", Route::Albums, cx))
                    .child(self.rail_button("nav-artists", "♬", Route::Artists, cx))
                    .child(div().flex_1())
                    .child(self.rail_button("nav-settings", "⚙", Route::Settings, cx)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .relative()
                    .overflow_hidden()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .h(px(82.0))
                            .px_5()
                            // The title is positioned against both edges, rather than after the
                            // variable-width actions, so it remains centered in the full header.
                            .child(
                                div()
                                    .absolute()
                                    .left_0()
                                    .right_0()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(
                                        div()
                                            .w(relative(0.55))
                                            .truncate()
                                            .text_center()
                                            .text_2xl()
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .child(format!("Hello, {display_name}!")),
                                    ),
                            )
                            .child(
                                div()
                                    .id("header-search")
                                    .cursor_pointer()
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.state.reduce(Action::Navigate(Route::Search));
                                        this.search_focus.focus(window);
                                        cx.notify();
                                    }))
                                    .ml_auto()
                                    .px_4()
                                    .py_2()
                                    .rounded_full()
                                    .bg(rgb(self.pal.surface))
                                    .text_sm()
                                    .text_color(rgb(self.pal.muted))
                                    .child("⌕  Search music…"),
                            )
                            .child(div().ml_3().size(px(38.0)).rounded_full().bg(rgb(SAGE))),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .px_5()
                            .gap_4()
                            .overflow_hidden()
                            .child(
                                div()
                                    .flex()
                                    .gap_5()
                                    .h(px(220.0))
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .justify_center()
                                            .w(relative(0.58))
                                            .min_w(px(470.0))
                                            .p_6()
                                            .gap_3()
                                            .rounded_xl()
                                            .bg(rgb(PEACH))
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .font_weight(gpui::FontWeight::BOLD)
                                                    .child("WELCOME TO MELLOWDECK"),
                                            )
                                            .child(
                                                div()
                                                    .text_2xl()
                                                    .font_weight(gpui::FontWeight::BOLD)
                                                    .child("Turn up the music, keep the interface quiet."),
                                            )
                                            .child(div().text_sm().text_color(rgb(0x53_3C_37)).child("Your personal listening data is now loaded directly from Spotify."))
                                            .child(div().flex().items_center().justify_center().w(px(105.0)).py_2().rounded_full().bg(rgb(self.pal.surface)).font_weight(gpui::FontWeight::SEMIBOLD).child("Connected")),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .flex_1()
                                            .gap_3()
                                            .child(
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .child(
                                                        div()
                                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                                            .child("Your top artists"),
                                                    )
                                                    .child(
                                                        div()
                                                            .ml_auto()
                                                            .text_xs()
                                                            .text_color(rgb(self.pal.muted))
                                                            .child(self.home_sync_label().to_owned()),
                                                    ),
                                            )
                                            .children(self.top_artist_rows(cx)),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .child(
                                        div()
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .child("Listening highlights"),
                                    )
                                    .child(
                                        div()
                                            .ml_auto()
                                            .text_xs()
                                            .text_color(rgb(self.pal.muted))
                                            .child("Live from Spotify"),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .gap_4()
                                    .child(self.section_card(
                                        self.home_section(|home| &home.recently_played),
                                        "Recently played",
                                        "Loading from Spotify…",
                                        0xE6_CE_A0,
                                        "RECENT",
                                        cx,
                                    ))
                                    .child(self.section_card(
                                        self.home_section(|home| &home.top_tracks),
                                        "Top tracks",
                                        "Loading from Spotify…",
                                        LAVENDER,
                                        "TOP",
                                        cx,
                                    ))
                                    .child(self.section_card(
                                        self.home_section(|home| &home.saved_albums),
                                        "Saved albums",
                                        "Loading from Spotify…",
                                        SAGE,
                                        "ALBUMS",
                                        cx,
                                    ))
                                    .child(self.section_card(
                                        self.home_section(|home| &home.playlists),
                                        "Playlists",
                                        "Loading from Spotify…",
                                        0xD6_B6_C9,
                                        "MIXES",
                                        cx,
                                    )),
                            )
                            .child(div().flex_1()),
                    )
                    .when_some(self.route_overlay(window, cx), gpui::ParentElement::child)
                    .child(self.player_bar(cx))
                    // Popups float above everything, including route overlays, and swallow the
                    // mouse so hovering, clicking, and scrolling never reach the cards behind them.
                    .when_some(self.player_popup(cx), |element, popup| {
                        element.child(deferred(popup).with_priority(1))
                    }),
            )
            .into_any_element()
    }
}

fn format_time(milliseconds: u64) -> String {
    let seconds = milliseconds / 1_000;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

/// Number of bars drawn in the waveform strip.
const WAVEFORM_BARS: usize = 56;

/// A small dark tooltip that shows a label. Built lazily by `text_tooltip`.
struct TextTooltip(SharedString);

impl Render for TextTooltip {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        // Tooltips stay dark-on-light in both themes so they read as an overlay, not a surface.
        div()
            .px_2()
            .py_1()
            .rounded_md()
            .bg(rgb(0x1D_1E_22))
            .text_color(rgb(0xFF_FF_FF))
            .text_xs()
            .child(self.0.clone())
    }
}

/// Builds a `tooltip` callback that shows the given static label.
fn text_tooltip(label: &'static str) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    move |_window, cx| cx.new(|_| TextTooltip(SharedString::from(label))).into()
}

/// Derives a stable seed for a track's waveform from its title and duration, so the same song
/// always draws the same shape without needing Spotify's (now unavailable) audio analysis.
fn waveform_seed(title: &str, duration_ms: u64) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in title.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash ^ duration_ms
}

/// Converts a pointer's screen x coordinate into a seek fraction for the actual interactive
/// track rectangle. This intentionally does not use the player/container width.
fn seek_ratio(pointer_x: f32, track_left: f32, track_width: f32) -> Option<f32> {
    if !track_width.is_finite()
        || track_width <= 0.0
        || !pointer_x.is_finite()
        || !track_left.is_finite()
    {
        return None;
    }
    Some(((pointer_x - track_left) / track_width).clamp(0.0, 1.0))
}
/// Generates smoothed, music-like bar heights in `0.18..=1.0` from a seed.
#[allow(clippy::cast_precision_loss)]
fn waveform_bars(seed: u64, count: usize) -> Vec<f32> {
    // A small LCG gives deterministic pseudo-random values per bar.
    let mut state = seed | 1;
    let mut raw = Vec::with_capacity(count);
    for _ in 0..count {
        state =
            state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        // Take 24 high bits and normalise to 0.0..1.0.
        let bits = (state >> 40) as u32;
        raw.push(bits as f32 / 16_777_216.0);
    }

    // Smooth each bar with its neighbours so the shape reads like a waveform, not noise, and
    // taper the ends with a gentle envelope.
    (0..count)
        .map(|index| {
            let previous = raw[index.saturating_sub(1)];
            let current = raw[index];
            let next = raw[(index + 1).min(count - 1)];
            let smoothed = previous.mul_add(0.25, current.mul_add(0.5, next * 0.25));
            let position = index as f32 / (count.max(2) - 1) as f32;
            let envelope = (position * std::f32::consts::PI).sin().mul_add(0.35, 0.65);
            (0.18 + smoothed * envelope * 0.82).clamp(0.18, 1.0)
        })
        .collect()
}

/// Builds the waveform's bar elements. `pulse` carries the current animation phase while playing.
#[allow(clippy::cast_precision_loss, clippy::cast_sign_loss, clippy::cast_possible_truncation)]
fn waveform_bar_elements(
    bars: &[f32],
    progress: f32,
    height: f32,
    pulse: Option<f32>,
    rest_color: u32,
) -> Vec<AnyElement> {
    let count = bars.len();
    let playhead = if count == 0 { 0 } else { (progress * (count - 1) as f32).round() as usize };
    bars.iter()
        .enumerate()
        .map(|(index, &value)| {
            let mut scale = value;
            if let Some(delta) = pulse {
                // Bars near the playhead gently bounce while playing.
                let distance = index.abs_diff(playhead);
                if distance <= 6 {
                    let proximity = 1.0 - distance as f32 / 6.0;
                    let wave = (delta * std::f32::consts::TAU + index as f32 * 0.6).sin();
                    scale = (scale + wave * 0.18 * proximity).clamp(0.12, 1.0);
                }
            }
            let color = match index.cmp(&playhead) {
                std::cmp::Ordering::Less => RED,
                std::cmp::Ordering::Equal => 0xFF_6B_5E,
                std::cmp::Ordering::Greater => rest_color,
            };
            div().flex_1().h(px(height * scale)).rounded_full().bg(rgb(color)).into_any_element()
        })
        .collect()
}

impl Render for MellowdeckShell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().font_family("Segoe UI").text_color(rgb(self.pal.ink)).child(
            if self.account.is_some() {
                self.home_screen(window, cx)
            } else {
                self.auth_screen(window, cx)
            },
        )
    }
}

impl Drop for MellowdeckShell {
    fn drop(&mut self) {
        if let Some(player) = &self.local_player {
            let _ = player.send(&LocalPlayerCommand::Shutdown);
        }
    }
}

pub fn run_desktop_with(
    startup: StartupState,
    actions: Arc<dyn DesktopActions>,
    local_player_factory: LocalPlayerFactory,
) {
    Application::new().with_http_client(spotify_image_client()).run(move |cx: &mut App| {
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        let bounds = Bounds::centered(None, size(px(1_180.0), px(760.0)), cx);
        let result = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(1_024.0), px(680.0))),
                ..WindowOptions::default()
            },
            |window, cx| {
                window.set_window_title("Mellowdeck");
                let (event_sender, event_receiver) = std::sync::mpsc::channel();
                let (local_player, local_player_error) =
                    match local_player_factory(window, event_sender) {
                        Ok(player) => (Some(player), None),
                        Err(error) => (None, Some(error.to_string())),
                    };
                cx.new(|cx| {
                    MellowdeckShell::new(
                        startup,
                        actions,
                        local_player,
                        event_receiver,
                        local_player_error,
                        cx,
                    )
                })
            },
        );
        match result {
            Ok(_) => cx.activate(true),
            Err(error) => {
                eprintln!("Mellowdeck could not create its main window: {error}");
                cx.quit();
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{WAVEFORM_BARS, format_time, seek_ratio, waveform_bars, waveform_seed};

    #[test]
    fn format_time_pads_seconds() {
        assert_eq!(format_time(0), "0:00");
        assert_eq!(format_time(9_000), "0:09");
        assert_eq!(format_time(83_000), "1:23");
    }

    #[test]
    fn seek_ratio_uses_track_geometry_and_clamps_to_the_track() {
        assert_eq!(seek_ratio(120.0, 120.0, 200.0), Some(0.0));
        assert_eq!(seek_ratio(170.0, 120.0, 200.0), Some(0.25));
        assert_eq!(seek_ratio(220.0, 120.0, 200.0), Some(0.5));
        assert_eq!(seek_ratio(270.0, 120.0, 200.0), Some(0.75));
        assert_eq!(seek_ratio(320.0, 120.0, 200.0), Some(1.0));
        assert_eq!(seek_ratio(20.0, 120.0, 200.0), Some(0.0));
        assert_eq!(seek_ratio(400.0, 120.0, 200.0), Some(1.0));
        assert_eq!(seek_ratio(120.0, 120.0, 0.0), None);
    }
    #[test]
    fn waveform_seed_is_stable_and_track_specific() {
        assert_eq!(waveform_seed("Song", 180_000), waveform_seed("Song", 180_000));
        assert_ne!(waveform_seed("Song", 180_000), waveform_seed("Other", 180_000));
        assert_ne!(waveform_seed("Song", 180_000), waveform_seed("Song", 200_000));
    }

    #[test]
    fn waveform_bars_are_deterministic_and_bounded() {
        let first = waveform_bars(waveform_seed("Track", 210_000), WAVEFORM_BARS);
        let second = waveform_bars(waveform_seed("Track", 210_000), WAVEFORM_BARS);
        assert_eq!(first, second);
        assert_eq!(first.len(), WAVEFORM_BARS);
        for value in first {
            assert!((0.18..=1.0).contains(&value), "bar height {value} out of range");
        }
    }
}
