#![forbid(unsafe_code)]

use std::{
    env,
    io::{self, Write as _},
    path::PathBuf,
    time::{Duration, Instant},
};

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use mellowdeck_cli::{
    AppState, Effect, HitMap, LoadState, Notice, NoticeKind, PlaybackState, Route,
    artwork::ArtworkManager,
    dispatch_key, dispatch_mouse,
    render::render_with_artwork,
    service::{ServiceHandle, ServiceResponse},
    session_state::{CliSessionState, CliSessionStore, PersistedTrack},
};
use mellowdeck_core::{
    AppError, CliSettings, CredentialStore, ErrorKind, LocalPlayerCommand, LocalPlayerEvent, Result,
};
#[cfg(not(target_os = "windows"))]
use mellowdeck_platform::MemoryCredentialStore;
#[cfg(target_os = "windows")]
use mellowdeck_platform::WindowsCredentialStore;
use mellowdeck_platform::{AppPaths, BackgroundLocalPlayer, open_system_browser};
use mellowdeck_spotify::{SpotifyAuthenticator, TokenSet, validate_client_id};
use mellowdeck_storage::JsonSettingsStore;
use ratatui::{Terminal, backend::CrosstermBackend};

struct Session {
    settings: JsonSettingsStore,
    client_id: Option<String>,
    credentials: Box<dyn CredentialStore>,
    auth: SpotifyAuthenticator,
    cli_session: CliSessionStore,
    token: Option<TokenSet>,
}

impl Session {
    fn load() -> Result<Self> {
        let paths = AppPaths::discover()?;
        let settings = JsonSettingsStore::new(paths.settings);
        let saved = settings.load()?;
        Ok(Self {
            cli_session: CliSessionStore::new(&paths.data),
            settings,
            client_id: saved.client_id,
            credentials: credential_store()?,
            auth: SpotifyAuthenticator::new()?,
            token: None,
        })
    }

    fn authenticate(&mut self) -> Result<String> {
        let client_id = if let Some(value) = self.client_id.clone() {
            value
        } else {
            print!("Spotify Client ID (create one at developer.spotify.com/dashboard): ");
            io::stdout().flush().map_err(io_error)?;
            let mut value = String::new();
            io::stdin().read_line(&mut value).map_err(io_error)?;
            let value = value.trim().to_owned();
            validate_client_id(&value)?;
            let mut settings = self.settings.load()?;
            settings.client_id = Some(value.clone());
            self.settings.save(&settings)?;
            self.client_id = Some(value.clone());
            value
        };
        let restored = self
            .credentials
            .load_refresh_token()?
            .map(|refresh| self.auth.refresh(&client_id, &refresh));
        let authorized = match restored {
            Some(Ok(session)) => session,
            Some(Err(_)) | None => self.auth.authorize(&client_id, open_system_browser)?,
        };
        if let Some(refresh) = authorized.tokens.refresh_token() {
            self.credentials.store_refresh_token(refresh)?;
        }
        let name = authorized.display_name;
        self.token = Some(authorized.tokens);
        Ok(name)
    }

    fn access_token(&self) -> Result<String> {
        self.token
            .as_ref()
            .map(|token| token.access_token().to_owned())
            .ok_or_else(|| AppError::new(ErrorKind::Authentication, "sign in is required"))
    }

    fn refresh_access_token(&mut self) -> Result<String> {
        let client_id = self.client_id.as_deref().ok_or_else(|| {
            AppError::new(ErrorKind::Authentication, "Spotify client ID is missing")
        })?;
        let refresh = self.credentials.load_refresh_token()?.ok_or_else(|| {
            AppError::new(ErrorKind::Authentication, "Spotify refresh token is missing")
        })?;
        let authorized = self.auth.refresh(client_id, &refresh)?;
        if let Some(updated) = authorized.tokens.refresh_token() {
            self.credentials.store_refresh_token(updated)?;
        }
        let access = authorized.tokens.access_token().to_owned();
        self.token = Some(authorized.tokens);
        Ok(access)
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("mellowdeck-cli: {error}");
    }
}

fn run() -> Result<()> {
    let mut session = Session::load()?;
    println!("Mellowdeck CLI — sign in to Spotify in your browser.");
    let account = session.authenticate()?;
    let mut terminal = TerminalGuard::new()?;
    let saved = session.settings.load()?;
    let cli = saved.cli.unwrap_or_default();
    let cached_session = session.cli_session.load();
    let mut state = AppState {
        artwork: cli.artwork,
        mouse: cli.mouse,
        wide_queue: cli.wide_queue,
        playback: PlaybackState { volume: cached_session.volume, ..PlaybackState::default() },
        ..AppState::default()
    };
    restore_cached_track(&mut state, cached_session.last_track);
    state.notice =
        Some(Notice { kind: NoticeKind::Success, text: format!("Signed in as {account}") });
    let mut artwork = ArtworkManager::new(state.artwork);
    let access_token = session.access_token()?;
    let worker = ServiceHandle::start(access_token.clone())?;
    let local_player = BackgroundLocalPlayer::start();
    if let Err(error) = local_player
        .send(LocalPlayerCommand::TokenUpdate { access_token })
        .and_then(|()| local_player.send(LocalPlayerCommand::Connect))
    {
        state.notice = Some(Notice {
            kind: NoticeKind::Info,
            text: format!(
                "Local playback engine could not start ({error}); Spotify Connect is still available."
            ),
        });
    }
    let mut hits = HitMap::default();
    send_effects(
        &worker,
        &local_player,
        vec![
            Effect::LoadPage { route: Route::Home, generation: 0, offset: 0, query: String::new() },
            Effect::RefreshPlayback,
            Effect::RefreshQueue,
        ],
        &mut state,
    )?;
    let mut playback_polled = Instant::now();

    while !state.quit {
        artwork.set_mode(state.artwork);
        terminal.draw(|frame| render_with_artwork(frame, &mut state, &mut hits, &mut artwork))?;
        while let Some(response) = worker.try_recv() {
            handle_response(response, &worker, &local_player, &mut session, &mut state)?;
        }
        handle_local_player_events(&local_player, &worker, &mut state)?;
        let interval =
            if state.playback.playing { Duration::from_secs(5) } else { Duration::from_secs(15) };
        if playback_polled.elapsed() >= interval {
            worker.send(Effect::RefreshPlayback)?;
            playback_polled = Instant::now();
        }
        if event::poll(Duration::from_millis(50)).map_err(io_error)? {
            let effects = match event::read().map_err(io_error)? {
                Event::Key(key) if key.kind == KeyEventKind::Press => dispatch_key(&mut state, key),
                Event::Mouse(mouse) => dispatch_mouse(&mut state, &mut hits, mouse, Instant::now()),
                Event::Resize(width, height) => {
                    state.update_layout(width, height);
                    artwork.clear_placement();
                    Vec::new()
                }
                _ => Vec::new(),
            };
            let save_settings = effects.iter().any(|effect| matches!(effect, Effect::SaveSettings));
            send_effects(&worker, &local_player, effects, &mut state)?;
            if save_settings {
                save_cli_settings(&session, &state)?;
            }
            request_next_page_if_needed(&worker, &mut state)?;
        }
    }
    artwork.clear_placement();
    let settings =
        CliSettings { artwork: state.artwork, mouse: state.mouse, wide_queue: state.wide_queue };
    let mut saved = session.settings.load()?;
    saved.cli = Some(settings);
    session.settings.save(&saved)?;
    if let Err(error) = session.cli_session.save(&session_snapshot(&state)) {
        tracing::warn!(%error, "failed to persist CLI session state");
    }
    // Do not wait for the player-host process: terminal restoration must never depend on it.
    let _ = local_player.send(LocalPlayerCommand::Shutdown);
    Ok(())
}

fn send_effects(
    worker: &ServiceHandle,
    local_player: &BackgroundLocalPlayer,
    effects: Vec<Effect>,
    state: &mut AppState,
) -> Result<()> {
    for effect in effects {
        match effect {
            Effect::OpenExternal(url) => open_system_browser(&url)?,
            Effect::SaveSettings => {}
            Effect::TogglePlayback if local_player_selected(state) => {
                let command = if state.playback.playing {
                    LocalPlayerCommand::Pause
                } else {
                    LocalPlayerCommand::Play
                };
                send_local_command(local_player, command, state);
            }
            Effect::Seek(delta) if local_player_selected(state) => {
                let current =
                    i64::try_from(state.playback.progress_at(Instant::now())).unwrap_or(i64::MAX);
                let position_ms = current.saturating_add(delta).max(0).cast_unsigned();
                send_local_command(local_player, LocalPlayerCommand::Seek { position_ms }, state);
            }
            Effect::Volume(value) if local_player_selected(state) => {
                send_local_command(
                    local_player,
                    LocalPlayerCommand::Volume { value_milli: u16::from(value) * 10 },
                    state,
                );
            }
            effect => {
                if let Err(error) = worker.send(effect) {
                    state.notice =
                        Some(Notice { kind: NoticeKind::Error, text: error.to_string() });
                }
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_lines)]
fn handle_response(
    response: ServiceResponse,
    worker: &ServiceHandle,
    local_player: &BackgroundLocalPlayer,
    session: &mut Session,
    state: &mut AppState,
) -> Result<()> {
    match response {
        ServiceResponse::Page(mut page) => {
            if page.generation != state.generation || page.route.key() != state.page.route.key() {
                return Ok(());
            }
            if state.page.loading_more {
                for incoming in page.sections.drain(..) {
                    if let Some(existing) = state
                        .page
                        .sections
                        .iter_mut()
                        .find(|section| section.title == incoming.title)
                    {
                        existing.items.extend(incoming.items);
                    } else {
                        state.page.sections.push(incoming);
                    }
                }
                state.page.next_offset = page.next_offset;
                state.page.loading_more = false;
                state.page.state = page.state;
            } else {
                if page.route == Route::Settings {
                    update_settings_rows(&mut page, state);
                }
                page.filter.clone_from(&state.page.filter);
                page.library_tab = state.page.library_tab;
                page.search_filter = state.page.search_filter;
                state.accept_page(page);
            }
        }
        ServiceResponse::Playback(playback) => {
            if playback.track_uri.is_some() {
                state.playback = PlaybackState {
                    track_uri: playback.track_uri,
                    context_uri: playback.context_uri,
                    title: playback.title,
                    artist: playback.subtitle,
                    artists: playback.artists,
                    album: playback.album,
                    album_uri: playback.album_uri,
                    artwork_url: playback.artwork_url,
                    device_id: playback.device_id,
                    device_name: playback.device_name,
                    playing: playback.playing,
                    progress_ms: playback.progress_ms.min(playback.duration_ms),
                    duration_ms: playback.duration_ms,
                    volume: playback.volume_percent.unwrap_or(state.playback.volume),
                    shuffle: playback.shuffle,
                    repeat: match playback.repeat.as_str() {
                        "context" => 1,
                        "track" => 2,
                        _ => 0,
                    },
                    available_actions: playback.actions,
                    observed_at: Some(Instant::now()),
                    cached_track: false,
                };
                if let Err(error) = session.cli_session.save(&session_snapshot(state)) {
                    tracing::warn!(%error, "failed to persist CLI session state");
                }
            }
            select_automatic_device(state);
        }
        ServiceResponse::Queue { now, upcoming } => {
            state.queue_now = now;
            state.queue_upcoming = upcoming;
            state.queue_state = if state.queue_now.is_none() && state.queue_upcoming.is_empty() {
                LoadState::Empty("Queue is empty".into())
            } else {
                LoadState::Ready
            };
        }
        ServiceResponse::Command { effect, result } => match result {
            Ok(()) => {
                state.notice =
                    Some(Notice { kind: NoticeKind::Success, text: command_success(&effect) });
                if matches!(effect, Effect::Volume(_)) {
                    if let Err(error) = session.cli_session.save(&session_snapshot(state)) {
                        tracing::warn!(%error, "failed to persist CLI session state");
                    }
                }
                worker.send(Effect::RefreshPlayback)?;
                if matches!(
                    effect,
                    Effect::Enqueue(_)
                        | Effect::PlayTrack { .. }
                        | Effect::PlayContext { .. }
                        | Effect::Next
                        | Effect::Previous
                ) {
                    worker.send(Effect::RefreshQueue)?;
                }
            }
            Err(error) if error.kind == ErrorKind::Authentication => {
                match session.refresh_access_token() {
                    Ok(token) => {
                        worker.update_token(token.clone())?;
                        if let Err(error) = local_player
                            .send(LocalPlayerCommand::TokenUpdate { access_token: token })
                        {
                            let was_selected = local_player_selected(state);
                            state.local_device_id = None;
                            if was_selected {
                                state.selected_device_id = None;
                            }
                            state.notice = Some(Notice {
                                kind: NoticeKind::Error,
                                text: format!("Local player token update failed: {error}"),
                            });
                        }
                        if matches!(
                            effect,
                            Effect::LoadPage { .. }
                                | Effect::RefreshPlayback
                                | Effect::RefreshQueue
                        ) {
                            worker.send(effect)?;
                            state.notice = Some(Notice {
                                kind: NoticeKind::Info,
                                text: "Spotify session refreshed.".into(),
                            });
                        } else {
                            state.notice = Some(Notice {
                                kind: NoticeKind::Info,
                                text: "Spotify session refreshed; retry the command.".into(),
                            });
                        }
                    }
                    Err(refresh) => {
                        state.notice = Some(Notice {
                            kind: NoticeKind::Error,
                            text: format!("Spotify authorization expired: {refresh}"),
                        });
                    }
                }
            }
            Err(error) => {
                state.notice = Some(Notice { kind: NoticeKind::Error, text: actionable(&error) });
                if matches!(effect, Effect::LoadPage { .. }) {
                    state.page.loading_more = false;
                    state.page.state = if state.page.sections.is_empty() {
                        LoadState::Failed(error.to_string())
                    } else {
                        LoadState::Stale(error.to_string())
                    };
                }
                if error.kind == ErrorKind::Unavailable {
                    state.overlay = Some(mellowdeck_cli::Overlay::DevicePicker);
                }
                worker.send(Effect::RefreshPlayback)?;
                if matches!(effect, Effect::Enqueue(_)) {
                    worker.send(Effect::RefreshQueue)?;
                }
            }
        },
    }
    Ok(())
}

fn handle_local_player_events(
    local_player: &BackgroundLocalPlayer,
    worker: &ServiceHandle,
    state: &mut AppState,
) -> Result<()> {
    while let Some(event) = local_player.try_next_event()? {
        match event {
            LocalPlayerEvent::Ready { device_id } => {
                let device_id = device_id.to_string();
                state.local_device_id = Some(device_id.clone());
                select_automatic_device(state);
                send_local_command(
                    local_player,
                    LocalPlayerCommand::Volume {
                        value_milli: u16::from(state.playback.volume) * 10,
                    },
                    state,
                );
                select_automatic_device(state);
                state.notice = Some(Notice {
                    kind: NoticeKind::Success,
                    text: "Local playback engine is ready on this device.".into(),
                });
                worker.send(Effect::RefreshPlayback)?;
            }
            LocalPlayerEvent::Unavailable => {
                let was_selected = local_player_selected(state);
                state.local_device_id = None;
                if was_selected {
                    state.selected_device_id = None;
                }
                state.notice = Some(Notice {
                    kind: NoticeKind::Info,
                    text: "Local playback engine is unavailable; choose a Spotify Connect device with d."
                        .into(),
                });
            }
            LocalPlayerEvent::StateChanged { playing, position_ms, duration_ms, track_uri } => {
                if local_player_selected(state) {
                    state.playback.playing = playing;
                    state.playback.progress_ms = position_ms.min(duration_ms);
                    state.playback.duration_ms = duration_ms;
                    state.playback.observed_at = Some(Instant::now());
                    if let Some(track_uri) = track_uri {
                        state.playback.track_uri = Some(track_uri.to_string());
                    }
                }
            }
            LocalPlayerEvent::AuthenticationError(message)
            | LocalPlayerEvent::PlaybackError(message)
            | LocalPlayerEvent::AccountError(message) => {
                state.notice = Some(Notice { kind: NoticeKind::Error, text: message });
                worker.send(Effect::RefreshPlayback)?;
            }
        }
    }
    Ok(())
}

fn local_player_selected(state: &AppState) -> bool {
    state.local_device_id.is_some()
        && state.local_device_id.as_deref() == state.selected_device_id.as_deref()
}

/// Re-evaluate after either playback or local-player readiness arrives so their arrival order
/// cannot change the chosen device. Active remote playback wins over local idle playback.
fn select_automatic_device(state: &mut AppState) {
    if state.device_selected_by_user {
        return;
    }
    if state.playback.playing
        && let Some(device_id) = state.playback.device_id.clone()
        && state.local_device_id.as_deref() != Some(device_id.as_str())
    {
        state.selected_device_id = Some(device_id);
    } else if let Some(device_id) = state.local_device_id.clone() {
        state.selected_device_id = Some(device_id);
    }
}

#[cfg(test)]
mod device_selection_tests {
    use super::*;

    fn local_ready(state: &mut AppState, id: &str) {
        state.local_device_id = Some(id.into());
        select_automatic_device(state);
    }
    fn playback(state: &mut AppState, id: &str, playing: bool) {
        state.playback.device_id = Some(id.into());
        state.playback.playing = playing;
        select_automatic_device(state);
    }

    #[test]
    fn selects_local_when_ready_arrives_first() {
        let mut state = AppState::default();
        local_ready(&mut state, "local");
        playback(&mut state, "remote", false);
        assert_eq!(state.selected_device_id.as_deref(), Some("local"));
    }
    #[test]
    fn selects_local_when_playback_arrives_first() {
        let mut state = AppState::default();
        playback(&mut state, "remote", false);
        local_ready(&mut state, "local");
        assert_eq!(state.selected_device_id.as_deref(), Some("local"));
    }
    #[test]
    fn preserves_a_playing_remote_device() {
        let mut state = AppState::default();
        playback(&mut state, "remote", true);
        local_ready(&mut state, "local");
        assert_eq!(state.selected_device_id.as_deref(), Some("remote"));
    }
    #[test]
    fn selects_local_when_remote_playback_is_paused() {
        let mut state = AppState::default();
        playback(&mut state, "remote", false);
        local_ready(&mut state, "local");
        assert_eq!(state.selected_device_id.as_deref(), Some("local"));
    }
    #[test]
    fn preserves_a_manually_selected_device() {
        let mut state = AppState::default();
        state.selected_device_id = Some("manual".into());
        state.device_selected_by_user = true;
        playback(&mut state, "remote", true);
        local_ready(&mut state, "local");
        assert_eq!(state.selected_device_id.as_deref(), Some("manual"));
    }
}

fn restore_cached_track(state: &mut AppState, track: Option<PersistedTrack>) {
    let Some(track) = track else { return };
    state.playback.track_uri = Some(track.uri);
    state.playback.title = track.title;
    state.playback.artist = track.artist;
    state.playback.artists = track.artists;
    state.playback.album = track.album;
    state.playback.album_uri = track.album_uri;
    state.playback.artwork_url = track.artwork_url;
    state.playback.duration_ms = track.duration_ms;
    state.playback.playing = false;
    state.playback.cached_track = true;
}

fn session_snapshot(state: &AppState) -> CliSessionState {
    let playback = &state.playback;
    let last_track =
        playback.track_uri.as_ref().filter(|_| !playback.title.is_empty()).map(|uri| {
            PersistedTrack {
                uri: uri.clone(),
                title: playback.title.clone(),
                artist: playback.artist.clone(),
                artists: playback.artists.clone(),
                album: playback.album.clone(),
                album_uri: playback.album_uri.clone(),
                artwork_url: playback.artwork_url.clone(),
                duration_ms: playback.duration_ms,
            }
        });
    CliSessionState { schema_version: 1, volume: playback.volume.min(100), last_track }
}
fn send_local_command(
    local_player: &BackgroundLocalPlayer,
    command: LocalPlayerCommand,
    state: &mut AppState,
) {
    if let Err(error) = local_player.send(command) {
        state.notice = Some(Notice {
            kind: NoticeKind::Error,
            text: format!("Local playback command failed: {error}"),
        });
    }
}

fn request_next_page_if_needed(worker: &ServiceHandle, state: &mut AppState) -> Result<()> {
    let len = state.page.flattened().len();
    if !state.page.loading_more
        && state.page.cursor.selected.saturating_add(5) >= len
        && let Some(offset) = state.page.next_offset
    {
        state.page.loading_more = true;
        worker.send(Effect::LoadPage {
            route: state.page.route.clone(),
            generation: state.generation,
            offset,
            query: state.search_query.clone(),
        })?;
    }
    Ok(())
}

fn command_success(effect: &Effect) -> String {
    match effect {
        Effect::Enqueue(_) => "Added to queue.".into(),
        Effect::Transfer(_) => "Playback device selected.".into(),
        Effect::PlayTrack { .. } | Effect::PlayContext { .. } => "Playback started.".into(),
        _ => "Spotify updated.".into(),
    }
}

fn update_settings_rows(page: &mut mellowdeck_cli::PageState, state: &AppState) {
    for item in page.sections.iter_mut().flat_map(|section| &mut section.items) {
        item.title = match item.id.as_str() {
            "artwork" => format!("Artwork: {:?}", state.artwork),
            "mouse" => format!("Mouse input: {}", if state.mouse { "enabled" } else { "disabled" }),
            "wide-queue" => format!(
                "Wide-screen queue: {}",
                if state.wide_queue { "enabled" } else { "disabled" }
            ),
            _ => item.title.clone(),
        };
    }
}

fn save_cli_settings(session: &Session, state: &AppState) -> Result<()> {
    let mut saved = session.settings.load()?;
    saved.cli = Some(CliSettings {
        artwork: state.artwork,
        mouse: state.mouse,
        wide_queue: state.wide_queue,
    });
    session.settings.save(&saved)
}

fn actionable(error: &AppError) -> String {
    match error.kind {
        ErrorKind::Authorization | ErrorKind::Restricted => {
            "Spotify does not allow this action for this account, app, or item.".into()
        }
        ErrorKind::RateLimited => error.retry_after.map_or_else(
            || "Spotify is rate limiting requests; retry shortly.".into(),
            |delay| {
                format!("Spotify is rate limiting requests; retry in {} seconds.", delay.as_secs())
            },
        ),
        ErrorKind::Unavailable => {
            "No usable playback device is available. Open Spotify on a device, then press d.".into()
        }
        _ => format!("Spotify request failed: {error}"),
    }
}

#[allow(dead_code)]
fn app_paths() -> AppPaths {
    let root = env::var_os("LOCALAPPDATA")
        .map_or_else(|| PathBuf::from("."), PathBuf::from)
        .join("Mellowdeck");
    AppPaths::under(root)
}

#[cfg(target_os = "windows")]
fn credential_store() -> Result<Box<dyn CredentialStore>> {
    Ok(Box::new(WindowsCredentialStore::open()?))
}
#[cfg(not(target_os = "windows"))]
fn credential_store() -> Result<Box<dyn CredentialStore>> {
    Ok(Box::new(MemoryCredentialStore::default()))
}

fn io_error(error: io::Error) -> AppError {
    let message = error.to_string();
    drop(error);
    AppError::new(ErrorKind::Storage, message)
}

struct TerminalGuard {
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
}

impl TerminalGuard {
    fn new() -> Result<Self> {
        enable_raw_mode().map_err(io_error)?;
        let mut stdout = io::stdout();
        if let Err(error) = execute!(stdout, EnterAlternateScreen, EnableMouseCapture) {
            let _ = disable_raw_mode();
            return Err(io_error(error));
        }
        Terminal::new(CrosstermBackend::new(stdout))
            .map(|terminal| Self { terminal })
            .map_err(io_error)
    }
    fn draw(&mut self, draw: impl FnOnce(&mut ratatui::Frame<'_>)) -> Result<()> {
        self.terminal.draw(draw).map(|_| ()).map_err(io_error)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(self.terminal.backend_mut(), DisableMouseCapture, LeaveAlternateScreen);
        let _ = self.terminal.show_cursor();
    }
}
