#![forbid(unsafe_code)]

use std::{
    env,
    io::{self, Write as _},
    path::PathBuf,
    time::{Duration, Instant},
};

use crossterm::{
    event::{self, Event, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use mellowdeck_cli::{CliState, Intent, PaneFocus, View};
use mellowdeck_core::{
    AppError, CredentialStore, ErrorKind, LocalPlayerCommand, LocalPlayerEvent, Result,
};
#[cfg(not(target_os = "windows"))]
use mellowdeck_platform::MemoryCredentialStore;
#[cfg(target_os = "windows")]
use mellowdeck_platform::WindowsCredentialStore;
use mellowdeck_platform::{AppPaths, BackgroundLocalPlayer, open_system_browser};
use mellowdeck_spotify::{
    SpotifyAuthenticator, SpotifyDisplayItem, SpotifyWebApi, TokenSet, validate_client_id,
};
use mellowdeck_storage::JsonSettingsStore;
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
};

struct Session {
    settings: JsonSettingsStore,
    client_id: Option<String>,
    credentials: Box<dyn CredentialStore>,
    auth: SpotifyAuthenticator,
    api: SpotifyWebApi,
    token: Option<TokenSet>,
}

impl Session {
    fn load() -> Result<Self> {
        let paths = app_paths();
        let settings = JsonSettingsStore::new(paths.settings);
        let saved = settings.load()?;
        Ok(Self {
            settings,
            client_id: saved.client_id,
            credentials: credential_store()?,
            auth: SpotifyAuthenticator::new()?,
            api: SpotifyWebApi::new()?,
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
        let session = match restored {
            Some(Ok(session)) => session,
            Some(Err(_)) | None => self.auth.authorize(&client_id, open_system_browser)?,
        };
        if let Some(refresh) = session.tokens.refresh_token() {
            self.credentials.store_refresh_token(refresh)?;
        }
        let name = session.display_name;
        self.token = Some(session.tokens);
        Ok(name)
    }

    fn token(&self) -> Result<&str> {
        self.token.as_ref().map(TokenSet::access_token).ok_or_else(|| {
            AppError::new(ErrorKind::Authentication, "sign in is required before using Spotify")
        })
    }
}

#[derive(Default)]
struct Content {
    title: String,
    items: Vec<SpotifyDisplayItem>,
    subtitle: String,
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
    let mut state = CliState::default();
    let mut content = load_home(&session)?;
    let local_player = BackgroundLocalPlayer::start();
    let _ = local_player
        .send(LocalPlayerCommand::TokenUpdate { access_token: session.token()?.to_owned() });
    let _ = local_player.send(LocalPlayerCommand::Connect);
    state.notice = Some(format!(
        "Signed in as {account}. Local WebView playback is unavailable in the CLI; choose a Connect device with d."
    ));

    while !state.quit {
        terminal.draw(|frame| render(frame, &state, &content))?;
        state.advance_vinyl(state.authoritative_playing);
        if state.refresh_due(Instant::now()) {
            refresh(&session, &mut state, &mut content);
        }
        while let Some(event) = local_player.try_next_event()? {
            match event {
                LocalPlayerEvent::Ready { device_id } => {
                    state.local_device_id = Some(device_id.to_string());
                    state.notice = Some(
                        "Mellowdeck local player ready; selected playback will use it.".into(),
                    );
                }
                LocalPlayerEvent::Unavailable => state.notice = Some(
                    "Local WebView2 player unavailable; choose a Spotify Connect device with [d]."
                        .into(),
                ),
                LocalPlayerEvent::StateChanged { playing, position_ms, duration_ms, .. } => {
                    state.observe_playback(playing, position_ms, duration_ms, Instant::now());
                }
                LocalPlayerEvent::AuthenticationError(message)
                | LocalPlayerEvent::AccountError(message)
                | LocalPlayerEvent::PlaybackError(message) => state.notice = Some(message),
            }
        }
        if event::poll(Duration::from_millis(100)).map_err(io_error)?
            && let Event::Key(key) = event::read().map_err(io_error)?
            && key.kind == KeyEventKind::Press
        {
            let intent = state.key(key, content.items.len());
            handle_intent(intent, &mut session, &mut state, &mut content, &local_player);
        }
    }
    local_player.shutdown()?;
    Ok(())
}

fn refresh(session: &Session, state: &mut CliState, content: &mut Content) {
    if let Ok(playback) = session.api.playback(session.token().unwrap_or_default()) {
        state.observe_playback(
            playback.playing,
            playback.progress_ms,
            playback.duration_ms,
            Instant::now(),
        );
        if playback.volume_percent.is_some() {
            state.volume = playback.volume_percent.unwrap_or(state.volume);
        }
    }
    let result = match state.view {
        View::Home => load_home(session),
        View::Library => load_library(session),
        View::Queue => load_queue(session),
        View::Devices => load_devices(session),
        _ => Ok(Content::default()),
    };
    if let Ok(new_content) = result {
        *content = new_content;
    }
}

fn handle_intent(
    intent: Intent,
    session: &mut Session,
    state: &mut CliState,
    content: &mut Content,
    local_player: &BackgroundLocalPlayer,
) {
    let result: Result<()> = (|| match intent {
        Intent::TogglePlayback => {
            if state.local_device_id.is_some() {
                local_player.send(if state.playing {
                    LocalPlayerCommand::Play
                } else {
                    LocalPlayerCommand::Pause
                })
            } else if state.playing {
                session.api.resume(session.token()?)
            } else {
                session.api.pause(session.token()?)
            }
        }
        Intent::Previous => session.api.previous(session.token()?),
        Intent::Next => session.api.next(session.token()?),
        Intent::Seek(offset) if state.local_device_id.is_some() => {
            let position = i64::try_from(state.interpolated_progress_ms(Instant::now()))
                .unwrap_or(i64::MAX)
                .saturating_add(offset)
                .max(0)
                .cast_unsigned();
            local_player.send(LocalPlayerCommand::Seek { position_ms: position })
        }
        Intent::Seek(offset) => {
            let snapshot = session.api.playback(session.token()?)?;
            let progress = i64::try_from(snapshot.progress_ms).unwrap_or(i64::MAX);
            let position = progress.saturating_add(offset).max(0).cast_unsigned();
            session.api.seek(session.token()?, position)
        }
        Intent::Volume(value) if state.local_device_id.is_some() => {
            local_player.send(LocalPlayerCommand::Volume { value_milli: u16::from(value) * 10 })
        }
        Intent::Volume(value) => session.api.volume(session.token()?, value),
        Intent::ToggleShuffle => session.api.shuffle(session.token()?, state.shuffle),
        Intent::Open => open_selected(session, state, content),
        Intent::Refresh => {
            refresh(session, state, content);
            Ok(())
        }
        Intent::Repeat(mode) => {
            session.api.repeat(session.token()?, ["off", "context", "track"][usize::from(mode)])
        }
        Intent::AddToQueue => content
            .items
            .get(state.selected)
            .and_then(|item| item.uri.as_deref())
            .map_or(Ok(()), |uri| session.api.enqueue(session.token()?, uri)),
        Intent::None | Intent::Quit => Ok(()),
    })();
    if let Err(error) = result {
        state.notice = Some(actionable(&error));
    }
}

fn open_selected(session: &Session, state: &mut CliState, content: &mut Content) -> Result<()> {
    if state.view == View::Search && state.searching {
        let results = session.api.search(session.token()?, &state.query)?;
        content.title = format!("Search: {}", state.query);
        content.subtitle = "Tracks, albums, artists and playlists".into();
        content.items = results
            .into_iter()
            .map(|item| SpotifyDisplayItem {
                title: format!("{} · {}", item.kind, item.title),
                subtitle: item.subtitle,
                uri: item.uri,
                artwork_url: item.artwork_url,
            })
            .collect();
        state.searching = false;
        state.selected = 0;
        return Ok(());
    }
    let Some(item) = content.items.get(state.selected) else {
        return Ok(());
    };
    let Some(uri) = item.uri.as_deref() else {
        return Ok(());
    };
    if state.view == View::Devices {
        session.api.transfer(session.token()?, uri)?;
        state.notice = Some(format!("Transferred playback to {}.", item.title));
    } else if state.view == View::Search
        && (item.title.starts_with("album ·")
            || item.title.starts_with("artist ·")
            || item.title.starts_with("playlist ·"))
    {
        let item = item.clone();
        state.navigate(View::Detail);
        content.title.clone_from(&item.title);
        content.subtitle =
            format!("{} — press Enter to play this context, [a] to queue", item.subtitle);
        content.items = vec![item];
    } else {
        if let Some(device_id) = state.local_device_id.as_deref() {
            session.api.play_uri_on_device(session.token()?, uri, Some(device_id))?;
        } else {
            session.api.play_uri(session.token()?, uri)?;
        }
        state.playing = true;
    }
    Ok(())
}

fn load_home(session: &Session) -> Result<Content> {
    let home = session.api.load_home(session.token()?)?;
    let mut items = home.recently_played.items;
    items.extend(home.top_tracks.items);
    items.extend(home.saved_albums.items);
    items.extend(home.playlists.items);
    Ok(Content {
        title: "Home".into(),
        subtitle: "Recently played, top tracks, albums and playlists".into(),
        items,
    })
}

fn load_library(session: &Session) -> Result<Content> {
    let library = session.api.load_library(session.token()?)?;
    let mut items = library.albums.items;
    items.extend(library.playlists.items);
    Ok(Content {
        title: "Library".into(),
        subtitle: "Saved albums and your playlists".into(),
        items,
    })
}

fn load_queue(session: &Session) -> Result<Content> {
    let queue = session.api.queue(session.token()?)?;
    let mut items = queue.current.into_iter().collect::<Vec<_>>();
    items.extend(queue.upcoming);
    Ok(Content { title: "Queue".into(), subtitle: "Now playing and upcoming".into(), items })
}

fn load_devices(session: &Session) -> Result<Content> {
    let items = session
        .api
        .devices(session.token()?)?
        .into_iter()
        .map(|device| SpotifyDisplayItem {
            title: format!("{}{}", device.name, if device.active { " (active)" } else { "" }),
            subtitle: if device.restricted {
                format!("{} · restricted", device.kind)
            } else {
                device.kind
            },
            uri: Some(device.id),
            artwork_url: None,
        })
        .collect();
    Ok(Content {
        title: "Devices".into(),
        subtitle: "Enter transfers playback to a usable Spotify Connect device".into(),
        items,
    })
}

fn render(frame: &mut ratatui::Frame<'_>, state: &CliState, content: &Content) {
    let areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(4), Constraint::Length(3)])
        .split(frame.area());
    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(36), Constraint::Percentage(64)])
        .split(areas[0]);

    render_playback_pane(frame, panes[0], state, content);
    render_navigation_pane(frame, panes[1], state, content);
    let compact_help = "[↑/↓] select  [Tab] pane  [Enter] open/play  [/] search  [Space] play/pause  [[/]] skip  [←/→] seek  [-/+] volume  [d] devices  [?] help  [x] quit";
    frame.render_widget(
        Paragraph::new(compact_help)
            .style(Style::default().fg(Color::DarkGray))
            .wrap(Wrap { trim: true })
            .block(Block::default().borders(Borders::TOP).title(" Shortcuts ")),
        areas[1],
    );
    if state.view == View::Help {
        render_help(frame);
    }
}

fn render_playback_pane(
    frame: &mut ratatui::Frame<'_>,
    area: ratatui::layout::Rect,
    state: &CliState,
    content: &Content,
) {
    let title =
        if state.focus == PaneFocus::Playback { " Playback • focused " } else { " Playback " };
    let border = if state.focus == PaneFocus::Playback { Color::Cyan } else { Color::DarkGray };
    let current = content.items.get(state.selected);
    let track = current.map_or("Nothing selected", |item| item.title.as_str());
    let artist =
        current.map_or("Choose a track from the right pane", |item| item.subtitle.as_str());
    let vinyl = vinyl_lines(state.vinyl_frame);
    let status = if state.playing { "● Playing" } else { "○ Paused" };
    let mut body = vinyl
        .into_iter()
        .map(|line| Line::from(Span::styled(line, Style::default().fg(Color::Magenta))))
        .collect::<Vec<_>>();
    body.extend([
        Line::from(""),
        Line::from(Span::styled(track, Style::default().add_modifier(Modifier::BOLD))),
        Line::from(Span::styled(artist, Style::default().fg(Color::Gray))),
        Line::from(""),
        Line::from(format!(
            "{status}  ·  {} / {}  ·  {}% volume",
            clock(state.interpolated_progress_ms(Instant::now())),
            clock(state.duration_ms),
            state.volume
        )),
        Line::from(format!(
            "Target: {}",
            if state.notice.as_deref().is_some_and(|n| n.contains("Local")) {
                "Spotify Connect"
            } else {
                "Spotify Connect / local ready when available"
            }
        )),
        Line::from(format!(
            "[Space] {}  [[] previous  []] next",
            if state.playing { "pause" } else { "play" }
        )),
        Line::from("[←/→] seek  [-/+] volume"),
        Line::from(format!(
            "[s] shuffle {}  [r] repeat {}",
            if state.shuffle { "on" } else { "off" },
            ["off", "context", "track"][usize::from(state.repeat)]
        )),
    ]);
    frame.render_widget(
        Paragraph::new(body)
            .alignment(Alignment::Center)
            // The record is deliberately fixed-width; trimming its indentation deforms it.
            .wrap(Wrap { trim: false })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(border))
                    .title(title),
            ),
        area,
    );
}

fn vinyl_lines(frame: u8) -> [&'static str; 9] {
    let groove = match frame {
        0 => "│",
        1 => "╲",
        2 => "─",
        _ => "╱",
    };
    match groove {
        "│" => [
            "       .-=====-.",
            "     .'  .---.  '.",
            "    /   /  |  \\   \\",
            "   ;   | --O-- |   ;",
            "   |   |  |  | |   |",
            "   ;    \\  |  /    ;",
            "    \\    '---'    /",
            "     '.         .'",
            "       '-=====-'",
        ],
        "╲" => [
            "       .-=====-.",
            "     .'  .---.  '.",
            "    /   /  ╲  \\   \\",
            "   ;   | --O-- |   ;",
            "   |   |  ╲  | |   |",
            "   ;    \\  ╲  /    ;",
            "    \\    '---'    /",
            "     '.         .'",
            "       '-=====-'",
        ],
        "─" => [
            "       .-=====-.",
            "     .'  .---.  '.",
            "    /   /  ─  \\   \\",
            "   ;   | --O-- |   ;",
            "   |   |  ─  | |   |",
            "   ;    \\  ─  /    ;",
            "    \\    '---'    /",
            "     '.         .'",
            "       '-=====-'",
        ],
        _ => [
            "       .-=====-.",
            "     .'  .---.  '.",
            "    /   /  ╱  \\   \\",
            "   ;   | --O-- |   ;",
            "   |   |  ╱  | |   |",
            "   ;    \\  ╱  /    ;",
            "    \\    '---'    /",
            "     '.         .'",
            "       '-=====-'",
        ],
    }
}

fn render_navigation_pane(
    frame: &mut ratatui::Frame<'_>,
    area: ratatui::layout::Rect,
    state: &CliState,
    content: &Content,
) {
    let title =
        if state.focus == PaneFocus::Navigation { " Browse • focused " } else { " Browse " };
    let border = if state.focus == PaneFocus::Navigation { Color::Cyan } else { Color::DarkGray };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(9), Constraint::Min(3), Constraint::Length(2)])
        .split(area);
    let nav = [View::Home, View::Search, View::Library, View::Queue, View::Devices, View::Settings];
    let menu = nav.into_iter().map(|view| {
        let active = view == state.view;
        let marker = if active { "›" } else { " " };
        ListItem::new(Span::styled(
            format!(" {marker} {}", view_name(view)),
            if active {
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            },
        ))
    });
    frame.render_widget(
        List::new(menu).block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border)),
        ),
        chunks[0],
    );
    let body = if state.view == View::Help {
        vec![ListItem::new("Help is open — press Esc to return.")]
    } else if state.view == View::Settings {
        vec![ListItem::new(
            "Local WebView player: unavailable in this terminal host. Use Devices (d) to select Spotify Connect.",
        )]
    } else {
        content
            .items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let marker = if index == state.selected { "› " } else { "  " };
                ListItem::new(Line::from(vec![
                    Span::styled(marker, Style::default().fg(Color::Cyan)),
                    Span::raw(&item.title),
                    Span::raw(format!(" — {}", item.subtitle)),
                ]))
            })
            .collect::<Vec<_>>()
    };
    let heading = if content.title.is_empty() {
        view_name(state.view).to_owned()
    } else {
        format!("{} — {}", content.title, content.subtitle)
    };
    frame.render_widget(
        List::new(body).block(Block::default().title(heading).borders(Borders::ALL)),
        chunks[1],
    );
    frame.render_widget(
        Paragraph::new(
            state.notice.as_deref().unwrap_or("Ready. Select an item, or press ? for help."),
        )
        .wrap(Wrap { trim: true })
        .style(Style::default().fg(Color::Yellow)),
        chunks[2],
    );
}

fn render_help(frame: &mut ratatui::Frame<'_>) {
    let popup = centered_rect(78, 72, frame.area());
    frame.render_widget(Clear, popup);
    let help = "Navigation\n[↑/↓] select  [Tab] change pane  [Enter] open, play or transfer  [Esc/Backspace] back\n\nPlayback\n[Space] play/pause  [[] previous  []] next  [←/→] seek  [-/+] volume  [s] shuffle  [r] repeat\n\nLibrary\n[/] search  [a] add selected track to queue  [q] queue  [d] devices  [g] settings\n\nSession\n[x] or [Ctrl-C] quit\n\nPress [Esc] to close this help.";
    frame.render_widget(
        Paragraph::new(help).wrap(Wrap { trim: true }).block(
            Block::default()
                .title(" Keyboard reference ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        ),
        popup,
    );
}

fn centered_rect(
    percent_x: u16,
    percent_y: u16,
    area: ratatui::layout::Rect,
) -> ratatui::layout::Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}

fn clock(milliseconds: u64) -> String {
    let seconds = milliseconds / 1_000;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

fn view_name(view: View) -> &'static str {
    match view {
        View::Home => "Home",
        View::Search => "Search",
        View::Library => "Library",
        View::Detail => "Detail",
        View::Queue => "Queue",
        View::Devices => "Devices",
        View::Settings => "Settings",
        View::Help => "Help",
    }
}

fn actionable(error: &AppError) -> String {
    match error.kind {
        ErrorKind::Authentication => {
            "Spotify authorization expired; restart and sign in again.".into()
        }
        ErrorKind::Authorization => {
            "Spotify rejected this action. Premium and an eligible playback device may be required."
                .into()
        }
        ErrorKind::RateLimited => "Spotify is rate limiting requests; try again shortly.".into(),
        ErrorKind::Unavailable => {
            "Spotify or the selected device is unavailable; choose another device with d.".into()
        }
        _ => format!("Spotify request failed: {error}"),
    }
}

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
        execute!(stdout, EnterAlternateScreen).map_err(io_error)?;
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
        let _ = execute!(self.terminal.backend_mut(), LeaveAlternateScreen);
        let _ = self.terminal.show_cursor();
    }
}
