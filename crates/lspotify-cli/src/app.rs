use std::{
    env,
    io::{self, Write as _},
    path::PathBuf,
    time::{Duration, Instant},
};

use crate::{
    AppState, Effect, HitMap, LoadState, Notice, NoticeKind, PlaybackState, Route,
    artwork::ArtworkManager,
    dispatch_key, dispatch_mouse,
    render::render_with_artwork,
    service::{ServiceHandle, ServiceResponse},
    session_state::{CliSessionState, CliSessionStore, PersistedTrack},
};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use lspotify_core::{
    AppError, CliSettings, CredentialStore, ErrorKind, LocalPlayerCommand, LocalPlayerEvent, Result,
};
#[cfg(not(target_os = "windows"))]
use lspotify_platform::MemoryCredentialStore;
#[cfg(target_os = "windows")]
use lspotify_platform::WindowsCredentialStore;
use lspotify_platform::{AppPaths, BackgroundLocalPlayer, open_system_browser};
use lspotify_spotify::{SpotifyAuthenticator, TokenSet, validate_client_id};
use lspotify_storage::JsonSettingsStore;
use ratatui::{Terminal, backend::CrosstermBackend};

#[allow(clippy::struct_field_names)]
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
        #[cfg(target_os = "windows")]
        let credentials: Box<dyn CredentialStore> = Box::new(WindowsCredentialStore::open()?);
        #[cfg(not(target_os = "windows"))]
        let credentials: Box<dyn CredentialStore> = Box::new(MemoryCredentialStore::default());
        Ok(Self {
            cli_session: CliSessionStore::new(&paths.data),
            settings,
            client_id: saved.client_id,
            credentials,
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

pub fn entry() {
    let first_arg = env::args().nth(1);
    match first_arg.as_deref() {
        Some("--version" | "-V" | "version") => {
            let version = option_env!("LSPOTIFY_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"));
            println!("lspotify {version}");
            return;
        }
        Some("update") => {
            println!("Checking for updates...");
            let repo = env::var("LSPOTIFY_REPO")
                .unwrap_or_else(|_| crate::updater::github::DEFAULT_REPO.to_string());
            let current_version = env!("CARGO_PKG_VERSION");
            match crate::updater::update(&repo, current_version) {
                Ok(crate::updater::UpdateSummary::AlreadyUpToDate { .. }) => {
                    println!("lspotify is up to date");
                }
                Ok(crate::updater::UpdateSummary::Updated { previous_version, new_version }) => {
                    println!("Updating {previous_version} → {new_version}");
                    println!("✓ lspotify updated successfully");
                }
                Err(err) => {
                    eprintln!("Update failed: {err}");
                    std::process::exit(1);
                }
            }
            return;
        }
        Some("--help" | "-h" | "help") => {
            println!("lspotify - Spotify terminal client\n");
            println!("Usage: lspotify [COMMAND]\n");
            println!("Commands:");
            println!("  update    Check for and install updates from GitHub Releases");
            println!("  version   Print version information");
            println!("  help      Print this help message\n");
            println!("Options:");
            println!("  -V, --version   Print version information");
            println!("  -h, --help      Print help information");
            return;
        }
        _ => {}
    }

    if let Err(error) = run() {
        eprintln!("lspotify: {error}");
    }
}

#[allow(clippy::too_many_lines)]
fn run() -> Result<()> {
    let mut session = Session::load()?;
    println!("lspotify — sign in to Spotify in your browser.");
    let account = session.authenticate()?;
    let mut terminal = TerminalGuard::new()?;
    let saved = session.settings.load()?;
    let cli = saved.cli.unwrap_or_default();
    let cached_session = session.cli_session.load();
    let mut state = AppState {
        artwork: cli.artwork,
        artwork_under_overlays: cli.artwork_under_overlays,
        mouse: cli.mouse,
        wide_queue: cli.wide_queue,
        side_player_max_height: cli.side_player_max_height,
        side_player_min_width: cli.side_player_min_width,
        stacked_queue_min_height: cli.stacked_queue_min_height,
        wide_breakpoint_width: cli.wide_breakpoint_width,
        playback: PlaybackState { volume: cached_session.volume, ..PlaybackState::default() },
        recent_searches: cached_session.recent_searches,
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
            Effect::LoadPage {
                route: Route::Home,
                generation: 0,
                offset: 0,
                query: String::new(),
                tab: None,
            },
            Effect::RefreshPlayback,
            Effect::RefreshQueue,
        ],
        &mut state,
    )?;
    let mut playback_polled = Instant::now();
    let mut fast_polls = FastPollTracker::default();

    let (update_tx, update_rx) = std::sync::mpsc::channel();
    if let Ok(paths) = AppPaths::discover() {
        crate::updater::spawn_background_check(
            paths.data,
            env!("CARGO_PKG_VERSION").to_string(),
            None,
            update_tx,
        );
    }

    while !state.quit {
        if let Ok(update_notice) = update_rx.try_recv() {
            state.notice = Some(Notice { kind: NoticeKind::Info, text: update_notice });
        }
        for item in state.queue_upcoming.iter().take(5) {
            if let Some(url) = &item.artwork_url {
                artwork.prefetch(url);
            }
        }
        artwork.set_mode(state.artwork);
        terminal.draw(|frame| render_with_artwork(frame, &mut state, &mut hits, &mut artwork))?;
        while let Some(response) = worker.try_recv() {
            handle_response(
                response,
                &worker,
                &local_player,
                &mut session,
                &mut state,
                &mut fast_polls,
            )?;
        }
        handle_local_player_events(&local_player, &worker, &mut state, &mut fast_polls)?;
        let interval =
            if state.playback.playing { Duration::from_secs(5) } else { Duration::from_secs(15) };
        if fast_polls.should_poll(Instant::now()) || playback_polled.elapsed() >= interval {
            worker.send(Effect::RefreshPlayback)?;
            playback_polled = Instant::now();
        }
        if event::poll(Duration::from_millis(50)).map_err(io_error)? {
            let prev_track_uri = state.playback.track_uri.clone();
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
            let is_track_change = effects.iter().any(|e| {
                matches!(
                    e,
                    Effect::Next
                        | Effect::Previous
                        | Effect::PlayTrack { .. }
                        | Effect::PlayContext { .. }
                )
            });
            if is_track_change {
                let expected = if state.playback.track_uri == prev_track_uri {
                    None
                } else {
                    state.playback.track_uri.clone()
                };
                fast_polls.schedule(prev_track_uri, expected);
            }
            let save_settings = effects.iter().any(|effect| matches!(effect, Effect::SaveSettings));
            send_effects(&worker, &local_player, effects, &mut state)?;
            if save_settings {
                save_cli_settings(&session, &state)?;
            }
            request_next_page_if_needed(&worker, &mut state)?;
        }
    }
    artwork.clear_placement();
    let settings = CliSettings {
        artwork: state.artwork,
        artwork_under_overlays: state.artwork_under_overlays,
        mouse: state.mouse,
        wide_queue: state.wide_queue,
        side_player_max_height: state.side_player_max_height,
        side_player_min_width: state.side_player_min_width,
        stacked_queue_min_height: state.stacked_queue_min_height,
        wide_breakpoint_width: state.wide_breakpoint_width,
    };
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

#[allow(clippy::too_many_lines, clippy::too_many_arguments)]
fn handle_response(
    response: ServiceResponse,
    worker: &ServiceHandle,
    local_player: &BackgroundLocalPlayer,
    session: &mut Session,
    state: &mut AppState,
    fast_polls: &mut FastPollTracker,
) -> Result<()> {
    match response {
        ServiceResponse::Page(page) => {
            if let Some(effect) = process_page_response(page, state) {
                let _ = worker.send(effect);
            }
        }
        ServiceResponse::Playback(playback) => {
            if !fast_polls.should_accept_playback(playback.track_uri.as_deref()) {
                return Ok(());
            }
            if playback.playing {
                if state.overlay == Some(crate::Overlay::DevicePicker) {
                    state.overlay = None;
                }
                state.clear_error_notice();
            }
            if let Some(ref device_name) = playback.device_name {
                state.playback.device_name = Some(device_name.clone());
            }
            if let Some(ref device_id) = playback.device_id {
                state.playback.device_id = Some(device_id.clone());
            }
            if playback.track_uri.is_some() {
                if state.playback.track_uri.is_some()
                    && state.playback.track_uri != playback.track_uri
                {
                    state.push_playback_history();
                }
                state.playback = PlaybackState {
                    track_uri: playback.track_uri,
                    context_uri: playback.context_uri,
                    title: playback.title,
                    artist: playback.subtitle,
                    artists: playback.artists,
                    album: playback.album,
                    album_uri: playback.album_uri,
                    artwork_url: playback.artwork_url,
                    device_id: playback.device_id.or_else(|| state.playback.device_id.clone()),
                    device_name: playback
                        .device_name
                        .or_else(|| state.playback.device_name.clone()),
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
            } else if playback.playing {
                state.playback.playing = true;
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
        ServiceResponse::PlaylistPicker { playlists, pending_uri } => {
            if let Some(crate::Overlay::PlaylistPicker { selected, .. }) = state.overlay {
                state.overlay = Some(crate::Overlay::PlaylistPicker {
                    selected: selected.min(playlists.len().saturating_sub(1)),
                    playlists,
                    pending_uri,
                });
            }
        }
        ServiceResponse::Command { effect, result } => match result {
            Ok(()) => {
                state.notice =
                    Some(Notice { kind: NoticeKind::Success, text: command_success(&effect) });
                if matches!(effect, Effect::PlayTrack { .. } | Effect::PlayContext { .. })
                    && state.overlay == Some(crate::Overlay::DevicePicker)
                {
                    state.overlay = None;
                }
                if matches!(effect, Effect::Volume(_))
                    && let Err(error) = session.cli_session.save(&session_snapshot(state))
                {
                    tracing::warn!(%error, "failed to persist CLI session state");
                }
                if !matches!(effect, Effect::Shuffle(_) | Effect::Repeat(_) | Effect::Volume(_)) {
                    worker.send(Effect::RefreshPlayback)?;
                }
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
                fast_polls.cancel();
                if let Effect::SetSaved { uri, saved } = &effect {
                    let reverted = !*saved;
                    for section in &mut state.page.sections {
                        for track in &mut section.items {
                            if track.uri.as_deref() == Some(uri.as_str()) {
                                track.saved = Some(reverted);
                            }
                        }
                    }
                    state.page_cache.remove("library:liked");
                    state.notice = Some(Notice {
                        kind: NoticeKind::Error,
                        text: "Could not update Spotify library".into(),
                    });
                } else {
                    state.notice =
                        Some(Notice { kind: NoticeKind::Error, text: actionable(&error) });
                }
                if matches!(effect, Effect::LoadPage { .. }) {
                    state.page.loading_more = false;
                    state.page.state = if state.page.sections.is_empty() {
                        LoadState::Failed(error.to_string())
                    } else {
                        LoadState::Stale(error.to_string())
                    };
                }
                if error.kind == ErrorKind::Unavailable && state.target_device_id().is_none() {
                    state.overlay = Some(crate::Overlay::DevicePicker);
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
    fast_polls: &mut FastPollTracker,
) -> Result<()> {
    while let Some(event) = local_player.try_next_event()? {
        match event {
            LocalPlayerEvent::Ready { device_id } => {
                let device_id = device_id.to_string();
                state.local_device_id = Some(device_id.clone());
                select_automatic_device(state);
                if state.playback.device_name.is_none() {
                    state.playback.device_name = Some("lspotify".into());
                }
                if state.playback.device_id.is_none() {
                    state.playback.device_id = Some(device_id);
                }
                if state.overlay == Some(crate::Overlay::DevicePicker) {
                    state.overlay = None;
                }
                state.clear_error_notice();
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
            LocalPlayerEvent::StateChanged {
                playing,
                position_ms,
                duration_ms,
                track_uri,
                title,
                artist,
                album,
                artwork_url,
            } => {
                if local_player_selected(state) {
                    if playing {
                        if state.overlay == Some(crate::Overlay::DevicePicker) {
                            state.overlay = None;
                        }
                        state.clear_error_notice();
                        if state.playback.device_name.is_none() {
                            state.playback.device_name = Some("lspotify".into());
                        }
                    }
                    if let Some(ref uri) = track_uri {
                        let uri_str = uri.to_string();
                        if state.playback.track_uri.as_deref() != Some(&uri_str) {
                            state.push_playback_history();
                            fast_polls.should_accept_playback(Some(&uri_str));
                        }
                        state.playback.track_uri = Some(uri_str);
                    }
                    state.playback.playing = playing;
                    state.playback.progress_ms = position_ms.min(duration_ms);
                    state.playback.duration_ms = duration_ms;
                    state.playback.observed_at = Some(Instant::now());
                    if let Some(title) = title {
                        state.playback.title = title;
                    }
                    if let Some(artist) = artist {
                        state.playback.artist.clone_from(&artist);
                        state.playback.artists = vec![(artist, None)];
                    }
                    if let Some(album) = album {
                        state.playback.album = album;
                    }
                    if let Some(artwork_url) = artwork_url {
                        state.playback.artwork_url = Some(artwork_url);
                    }
                    state.playback.cached_track = false;
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
    } else if let Some(device_id) = state.playback.device_id.clone() {
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
    #[allow(clippy::field_reassign_with_default)]
    fn preserves_a_manually_selected_device() {
        let mut state = AppState::default();
        state.selected_device_id = Some("manual".into());
        state.device_selected_by_user = true;
        playback(&mut state, "remote", true);
        local_ready(&mut state, "local");
        assert_eq!(state.selected_device_id.as_deref(), Some("manual"));
    }
    #[test]
    fn selects_paused_remote_when_no_local_device() {
        let mut state = AppState::default();
        playback(&mut state, "remote", false);
        assert_eq!(state.selected_device_id.as_deref(), Some("remote"));
    }
}

#[cfg(test)]
mod fast_poll_tests {
    use super::*;

    #[test]
    fn schedule_creates_three_deadlines() {
        let mut tracker = FastPollTracker::default();
        tracker.schedule(Some("spotify:track:old".into()), Some("spotify:track:new".into()));
        assert_eq!(tracker.deadlines.len(), 3);
        assert_eq!(tracker.stale_track_uris, vec!["spotify:track:old"]);
        assert_eq!(tracker.expected_track_uri.as_deref(), Some("spotify:track:new"));
    }

    #[test]
    fn rejects_stale_track_and_accepts_expected_track() {
        let mut tracker = FastPollTracker::default();
        tracker.schedule(Some("spotify:track:old".into()), Some("spotify:track:new".into()));

        assert!(!tracker.should_accept_playback(Some("spotify:track:old")));
        assert_eq!(tracker.deadlines.len(), 3);

        assert!(tracker.should_accept_playback(Some("spotify:track:new")));
        assert!(tracker.deadlines.is_empty());
        assert!(tracker.expected_track_uri.is_none());

        assert!(tracker.should_accept_playback(Some("spotify:track:old")));
    }

    #[test]
    fn accepts_different_track_when_expected_is_none() {
        let mut tracker = FastPollTracker::default();
        tracker.schedule(Some("spotify:track:old".into()), None);

        assert!(!tracker.should_accept_playback(Some("spotify:track:old")));

        assert!(tracker.should_accept_playback(Some("spotify:track:different")));
        assert!(tracker.deadlines.is_empty());
    }

    #[test]
    fn accepts_after_all_deadlines_expire() {
        let mut tracker = FastPollTracker::default();
        tracker.schedule(Some("spotify:track:old".into()), Some("spotify:track:new".into()));

        let future = Instant::now() + Duration::from_secs(10);
        assert!(tracker.should_poll(future));
        assert!(tracker.should_poll(future));
        assert!(tracker.should_poll(future));
        assert!(!tracker.should_poll(future));
        assert!(tracker.deadlines.is_empty());

        assert!(tracker.should_accept_playback_at(Some("spotify:track:old"), future));
    }

    #[test]
    fn rapid_skips_reject_all_stale_tracks() {
        let mut tracker = FastPollTracker::default();
        tracker.schedule(Some("spotify:track:1".into()), Some("spotify:track:2".into()));
        tracker.schedule(Some("spotify:track:2".into()), Some("spotify:track:3".into()));

        assert_eq!(tracker.stale_track_uris, vec!["spotify:track:1", "spotify:track:2"]);
        assert_eq!(tracker.expected_track_uri.as_deref(), Some("spotify:track:3"));

        // Both track 1 and track 2 in-flight responses must be rejected
        assert!(!tracker.should_accept_playback(Some("spotify:track:1")));
        assert!(!tracker.should_accept_playback(Some("spotify:track:2")));

        // Only track 3 is accepted
        assert!(tracker.should_accept_playback(Some("spotify:track:3")));
        assert!(!tracker.is_active(Instant::now()));
    }

    #[test]
    fn rejects_stale_response_while_last_poll_in_flight() {
        let mut tracker = FastPollTracker::default();
        tracker.schedule(Some("spotify:track:old".into()), Some("spotify:track:new".into()));

        let t1 = Instant::now() + Duration::from_millis(500);
        let t2 = Instant::now() + Duration::from_millis(1100);
        let t3 = Instant::now() + Duration::from_millis(2100);
        assert!(tracker.should_poll(t1));
        assert!(tracker.should_poll(t2));
        assert!(tracker.should_poll(t3));
        assert!(!tracker.should_poll(t3));

        // Deadlines are empty, but expiry is 3500ms so tracker is still active!
        assert!(tracker.deadlines.is_empty());
        assert!(tracker.is_active(t3));

        // In-flight response returning old track must be rejected!
        assert!(!tracker.should_accept_playback_at(Some("spotify:track:old"), t3));

        // Expected track is accepted and clears tracker
        assert!(tracker.should_accept_playback_at(Some("spotify:track:new"), t3));
        assert!(!tracker.is_active(t3));
    }

    #[test]
    fn cancel_on_command_error_allows_immediate_acceptance() {
        let mut tracker = FastPollTracker::default();
        tracker.schedule(Some("spotify:track:old".into()), Some("spotify:track:new".into()));
        assert!(!tracker.should_accept_playback(Some("spotify:track:old")));

        tracker.cancel();
        assert!(!tracker.is_active(Instant::now()));
        assert!(tracker.should_accept_playback(Some("spotify:track:old")));
    }
}

#[cfg(test)]
mod page_response_tests {
    use super::*;

    #[test]
    fn initial_home_response_enqueues_warm_liked_songs() {
        let mut state = AppState::default();
        let mut home_page = crate::PageState::loading(Route::Home, 0);
        home_page.state = LoadState::Ready;
        let effect = process_page_response(home_page, &mut state);
        assert_eq!(effect, Some(Effect::WarmLikedSongs));
        assert!(state.liked_songs_warmed);
    }

    #[test]
    fn warm_liked_songs_response_populates_cache_without_disrupting_home() {
        let mut state = AppState::default();
        let mut liked_page = crate::PageState::loading(Route::Library, 0);
        liked_page.state = LoadState::Ready;
        liked_page.library_tab = crate::LibraryTab::LikedSongs;
        liked_page.sections = vec![crate::state::Section {
            title: "Liked Songs".into(),
            items: vec![crate::BrowseItem::message("track-1", "Liked 1")],
        }];

        let effect = process_page_response(liked_page, &mut state);
        assert_eq!(effect, None);
        assert_eq!(state.page.route, Route::Home);
        assert!(state.page_cache.contains_key("library:liked"));
    }

    #[test]
    fn warm_prefetch_does_not_replace_or_delay_foreground_navigation() {
        let mut state = AppState::default();
        state.navigate(Route::Search);
        assert_eq!(state.page.route, Route::Search);

        let mut liked_page = crate::PageState::loading(Route::Library, 0);
        liked_page.state = LoadState::Ready;
        liked_page.library_tab = crate::LibraryTab::LikedSongs;

        let effect = process_page_response(liked_page, &mut state);
        assert_eq!(effect, None);
        assert_eq!(state.page.route, Route::Search);
        assert!(state.page_cache.contains_key("library:liked"));
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
    CliSessionState {
        schema_version: 1,
        volume: playback.volume.min(100),
        last_track,
        recent_searches: state.recent_searches.clone(),
    }
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
            tab: if state.page.route == Route::Library {
                Some(state.page.library_tab)
            } else {
                None
            },
        })?;
    }
    Ok(())
}

fn command_success(effect: &Effect) -> String {
    match effect {
        Effect::Enqueue(_) => "Added to queue.".into(),
        Effect::Transfer(_) => "Playback device selected.".into(),
        Effect::PlayTrack { .. } | Effect::PlayContext { .. } => "Playback started.".into(),
        Effect::SetSaved { saved: true, .. } => "♥ Added to Liked Songs".into(),
        Effect::SetSaved { saved: false, .. } => "♡ Removed from Liked Songs".into(),
        Effect::AddToPlaylist { .. } => "Added track to playlist.".into(),
        Effect::RemoveFromPlaylist { .. } => "Removed track from playlist.".into(),
        Effect::RenamePlaylist { new_name, .. } => format!("Renamed playlist to \"{new_name}\"."),
        _ => "Spotify updated.".into(),
    }
}

fn process_page_response(mut page: crate::PageState, state: &mut AppState) -> Option<Effect> {
    if page.route == Route::Library
        && page.cache_key() == "library:liked"
        && (state.page.route != Route::Library || page.generation != state.generation)
    {
        if page.state == LoadState::Ready || matches!(page.state, LoadState::Partial(_)) {
            state.page_cache.insert(page.cache_key(), (page, Instant::now()));
        }
        return None;
    }
    if page.generation != state.generation || page.route.key() != state.page.route.key() {
        return None;
    }
    if state.page.loading_more {
        for incoming in page.sections.drain(..) {
            if let Some(existing) =
                state.page.sections.iter_mut().find(|section| section.title == incoming.title)
            {
                existing.items.extend(incoming.items);
            } else {
                state.page.sections.push(incoming);
            }
        }
        state.page.next_offset = page.next_offset;
        state.page.loading_more = false;
        state.page.state = page.state;
        None
    } else {
        if page.route == Route::Settings {
            update_settings_rows(&mut page, state);
        }
        page.filter.clone_from(&state.page.filter);
        page.library_tab = state.page.library_tab;
        page.search_filter = state.page.search_filter;
        let accepted = state.accept_page(page);
        if accepted
            && state.page.route == Route::Home
            && !state.liked_songs_warmed
            && !state.page_cache.contains_key("library:liked")
        {
            state.liked_songs_warmed = true;
            Some(Effect::WarmLikedSongs)
        } else {
            None
        }
    }
}

fn update_settings_rows(page: &mut crate::PageState, state: &AppState) {
    for item in page.sections.iter_mut().flat_map(|section| &mut section.items) {
        item.title = match item.id.as_str() {
            "artwork" => format!("Artwork: {:?}", state.artwork),
            "artwork-under-overlays" => format!(
                "Artwork behind menus: {}",
                if state.artwork_under_overlays { "enabled" } else { "disabled" }
            ),
            "mouse" => format!("Mouse input: {}", if state.mouse { "enabled" } else { "disabled" }),
            "wide-queue" => format!(
                "Wide-screen queue: {}",
                if state.wide_queue { "enabled" } else { "disabled" }
            ),
            "side-player-height" => {
                if state.side_player_max_height == 0 {
                    "Side player max height: Disabled".into()
                } else {
                    format!("Side player max height: {} rows", state.side_player_max_height)
                }
            }
            "side-player-width" => {
                format!("Side player min width: {} cols", state.side_player_min_width)
            }
            "stacked-queue-height" => {
                if state.stacked_queue_min_height > 500 {
                    "Stacked queue min height: Disabled".into()
                } else {
                    format!("Stacked queue min height: {} rows", state.stacked_queue_min_height)
                }
            }
            "wide-breakpoint" => {
                format!("Wide layout min width: {} cols", state.wide_breakpoint_width)
            }
            _ => item.title.clone(),
        };
    }
}

fn save_cli_settings(session: &Session, state: &AppState) -> Result<()> {
    let mut saved = session.settings.load()?;
    saved.cli = Some(CliSettings {
        artwork: state.artwork,
        artwork_under_overlays: state.artwork_under_overlays,
        mouse: state.mouse,
        wide_queue: state.wide_queue,
        side_player_max_height: state.side_player_max_height,
        side_player_min_width: state.side_player_min_width,
        stacked_queue_min_height: state.stacked_queue_min_height,
        wide_breakpoint_width: state.wide_breakpoint_width,
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
        .join("lspotify");
    AppPaths::under(root)
}

fn io_error(error: io::Error) -> AppError {
    let message = error.to_string();
    drop(error);
    AppError::new(ErrorKind::Storage, message)
}

#[derive(Debug, Default)]
struct FastPollTracker {
    expected_track_uri: Option<String>,
    stale_track_uris: Vec<String>,
    deadlines: Vec<Instant>,
    expiry: Option<Instant>,
}

impl FastPollTracker {
    fn schedule(&mut self, previous_uri: Option<String>, expected_uri: Option<String>) {
        let now = Instant::now();
        if let Some(prev) = previous_uri
            && !self.stale_track_uris.contains(&prev)
        {
            self.stale_track_uris.push(prev);
        }
        self.expected_track_uri = expected_uri;
        self.deadlines = vec![
            now + Duration::from_millis(400),
            now + Duration::from_millis(1000),
            now + Duration::from_millis(2000),
        ];
        self.expiry = Some(now + Duration::from_millis(3500));
    }

    fn should_poll(&mut self, now: Instant) -> bool {
        if let Some(&first) = self.deadlines.first()
            && now >= first
        {
            self.deadlines.remove(0);
            return true;
        }
        false
    }

    fn is_active(&self, now: Instant) -> bool {
        if !self.deadlines.is_empty() {
            return true;
        }
        self.expiry.is_some_and(|exp| now < exp)
    }

    fn cancel(&mut self) {
        self.expected_track_uri = None;
        self.stale_track_uris.clear();
        self.deadlines.clear();
        self.expiry = None;
    }

    fn should_accept_playback(&mut self, incoming_uri: Option<&str>) -> bool {
        self.should_accept_playback_at(incoming_uri, Instant::now())
    }

    fn should_accept_playback_at(&mut self, incoming_uri: Option<&str>, now: Instant) -> bool {
        if !self.is_active(now) {
            self.cancel();
            return true;
        }
        if let Some(incoming) = incoming_uri
            && self.stale_track_uris.iter().any(|stale| stale == incoming)
        {
            return false;
        }
        if let Some(expected) = &self.expected_track_uri {
            if incoming_uri == Some(expected.as_str()) {
                self.cancel();
                true
            } else {
                false
            }
        } else if incoming_uri.is_some() {
            self.cancel();
            true
        } else {
            false
        }
    }
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
