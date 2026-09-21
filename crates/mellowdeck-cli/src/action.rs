use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

use crate::state::{
    AppState, EntityKind, FocusRegion, LibraryTab, Notice, NoticeKind, Overlay, Route, SearchFilter,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Effect {
    LoadPage { route: Route, generation: u64, offset: u32, query: String },
    RefreshPlayback,
    RefreshQueue,
    TogglePlayback,
    Previous,
    Next,
    Seek(i64),
    Volume(u8),
    Shuffle(bool),
    Repeat(u8),
    PlayTrack { uri: String, device_id: Option<String> },
    PlayContext { uri: String, position: usize, device_id: Option<String> },
    Enqueue(String),
    Transfer(String),
    OpenExternal(String),
    SaveSettings,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Action {
    Navigate(Route),
    Back,
    FocusNext(bool),
    Move(isize),
    Page(isize),
    Home,
    End,
    LocalLeft,
    LocalRight,
    Activate,
    OpenActions,
    Inspect,
    ToggleHelp,
    StartSearch,
    StartFilter,
    Insert(char),
    Backspace,
    SubmitText,
    Cancel,
    TogglePlayback,
    Previous,
    Next,
    Seek(i64),
    Volume(i8),
    Shuffle,
    Repeat,
    Enqueue,
    Refresh,
    PlayDetailContext,
    PlayDetailContextWithShuffle,
    OpenDetailSpotify,
    Quit,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HitTarget {
    Sidebar(usize),
    TopNav(Route),
    PlayerArtist { title: String, uri: String },
    PlayerAlbum { title: String, uri: String },
    PlayerDevice,
    MenuButton,
    ContentRow(usize),
    QueueRow(usize),
    ActionRow(usize),
    PlayerToggle,
    PlayerPrevious,
    PlayerNext,
    PlayerProgress,
    PlayerVolume,
    PlayerShuffle,
    PlayerRepeat,
    SearchField,
    Help,
    DetailPlay,
    DetailShuffle,
    DetailOpenSpotify,
}

#[derive(Clone, Debug)]
struct HitRegion {
    rect: Rect,
    target: HitTarget,
}

#[derive(Clone, Debug, Default)]
pub struct HitMap {
    regions: Vec<HitRegion>,
    pub content: Option<Rect>,
    pub queue: Option<Rect>,
    pub sidebar: Option<Rect>,
    last_click: Option<(HitTarget, Instant)>,
}

impl HitMap {
    pub fn clear(&mut self) {
        self.regions.clear();
        self.content = None;
        self.queue = None;
        self.sidebar = None;
    }
    pub fn add(&mut self, rect: Rect, target: HitTarget) {
        self.regions.push(HitRegion { rect, target });
    }
    pub fn target_at(&self, column: u16, row: u16) -> Option<HitTarget> {
        self.regions
            .iter()
            .rev()
            .find(|hit| hit.rect.contains((column, row).into()))
            .map(|hit| hit.target.clone())
    }
    pub(crate) fn rect_for(&self, target: &HitTarget) -> Option<Rect> {
        self.regions.iter().rev().find(|hit| &hit.target == target).map(|hit| hit.rect)
    }
}

pub fn dispatch_key(state: &mut AppState, key: KeyEvent) -> Vec<Effect> {
    // Exit precedes text entry and overlays. Lower-case q remains the Queue shortcut.
    let action = if matches!(key.code, KeyCode::Char('x' | 'Q'))
        || (key.code == KeyCode::Char('q') && key.modifiers.contains(KeyModifiers::SHIFT))
        || (matches!(key.code, KeyCode::Char('c' | 'q'))
            && key.modifiers.contains(KeyModifiers::CONTROL))
    {
        Action::Quit
    } else if state.text_entry || matches!(state.overlay, Some(Overlay::Help { editing: true, .. }))
    {
        match key.code {
            KeyCode::Esc => Action::Cancel,
            KeyCode::Backspace => Action::Backspace,
            KeyCode::Enter => Action::SubmitText,
            KeyCode::Down if state.overlay.is_none() => {
                state.text_entry = false;
                state.focus = FocusRegion::Content;
                if state.page.route == Route::Search && state.search_query.is_empty() {
                    state.populate_recent_searches();
                }
                return Vec::new();
            }
            KeyCode::Tab if state.overlay.is_none() => {
                state.text_entry = false;
                if state.page.route == Route::Search && state.search_query.is_empty() {
                    state.populate_recent_searches();
                }
                Action::FocusNext(key.modifiers.contains(KeyModifiers::SHIFT))
            }
            KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                Action::Insert(ch)
            }
            _ => return Vec::new(),
        }
    } else {
        match key.code {
            KeyCode::Up
                if state.page.route == Route::Search
                    && state.focus == FocusRegion::Content
                    && state.page.cursor.selected == 0 =>
            {
                state.text_entry = true;
                if state.search_query.is_empty() {
                    state.populate_recent_searches();
                }
                return Vec::new();
            }
            KeyCode::Tab => Action::FocusNext(key.modifiers.contains(KeyModifiers::SHIFT)),
            KeyCode::Up | KeyCode::Char('k') => Action::Move(-1),
            KeyCode::Down | KeyCode::Char('j') => Action::Move(1),
            KeyCode::PageUp => Action::Page(-1),
            KeyCode::PageDown => Action::Page(1),
            KeyCode::Home => Action::Home,
            KeyCode::End => Action::End,
            KeyCode::Left if key.modifiers.contains(KeyModifiers::SHIFT) => Action::Seek(-5_000),
            KeyCode::Right if key.modifiers.contains(KeyModifiers::SHIFT) => Action::Seek(5_000),
            KeyCode::Left => Action::LocalLeft,
            KeyCode::Right => Action::LocalRight,
            KeyCode::Enter => Action::Activate,
            KeyCode::Esc | KeyCode::Backspace => Action::Cancel,
            KeyCode::Char('1') => Action::Navigate(Route::Home),
            KeyCode::Char('2') => Action::Navigate(Route::Search),
            KeyCode::Char('3') => Action::Navigate(Route::Library),
            KeyCode::Char('4') => Action::Navigate(Route::Settings),
            KeyCode::Char('/') => Action::StartSearch,
            KeyCode::Char('f') if state.page.route == Route::Library => Action::StartFilter,
            KeyCode::Char(' ') => Action::TogglePlayback,
            KeyCode::Char('[') => Action::Previous,
            KeyCode::Char(']') => Action::Next,
            KeyCode::Char('-') => Action::Volume(-5),
            KeyCode::Char('+' | '=') => Action::Volume(5),
            KeyCode::Char('s') => Action::Shuffle,
            KeyCode::Char('r') => Action::Repeat,
            KeyCode::Char('q') => Action::Navigate(Route::Queue),
            KeyCode::Char('d') => Action::Navigate(Route::Devices),
            KeyCode::Char('a') => Action::Enqueue,
            KeyCode::Char('?') => Action::ToggleHelp,
            KeyCode::Char('i') => Action::Inspect,
            KeyCode::Char('m') if state.layout == crate::state::LayoutMode::Compact => {
                Action::OpenActions
            }
            KeyCode::Char('p' | 'P') if has_detail_actions(state) => Action::PlayDetailContext,
            KeyCode::Char('S') if has_detail_actions(state) => {
                Action::PlayDetailContextWithShuffle
            }
            KeyCode::Char('o' | 'O') if has_detail_actions(state) => Action::OpenDetailSpotify,
            _ => return Vec::new(),
        }
    };
    reduce(state, action)
}

pub fn dispatch_mouse(
    state: &mut AppState,
    hits: &mut HitMap,
    event: MouseEvent,
    now: Instant,
) -> Vec<Effect> {
    if !state.mouse {
        return Vec::new();
    }
    let target = hits.target_at(event.column, event.row);
    match event.kind {
        MouseEventKind::ScrollUp => {
            state.focus = hovered_focus(hits, event.column, event.row).unwrap_or(state.focus);
            reduce(state, Action::Move(-3))
        }
        MouseEventKind::ScrollDown => {
            state.focus = hovered_focus(hits, event.column, event.row).unwrap_or(state.focus);
            reduce(state, Action::Move(3))
        }
        MouseEventKind::Down(MouseButton::Right) => {
            select_target(state, target.as_ref());
            reduce(state, Action::OpenActions)
        }
        MouseEventKind::Down(MouseButton::Left) => {
            let double = target.as_ref().is_some_and(|target| {
                hits.last_click.as_ref().is_some_and(|(last, at)| {
                    last == target
                        && now.saturating_duration_since(*at) <= Duration::from_millis(450)
                })
            });
            if let Some(target) = target.clone() {
                hits.last_click = Some((target, now));
            }
            let direct = match target.clone() {
                Some(HitTarget::MenuButton) => {
                    state.overlay = Some(Overlay::Menu);
                    return Vec::new();
                }
                Some(HitTarget::PlayerToggle) => Some(Action::TogglePlayback),
                Some(HitTarget::TopNav(route)) => Some(Action::Navigate(route)),
                Some(HitTarget::PlayerArtist { title, uri }) => {
                    Some(Action::Navigate(Route::Artist { title, uri }))
                }
                Some(HitTarget::PlayerAlbum { title, uri }) => {
                    Some(Action::Navigate(Route::Album { title, uri }))
                }
                Some(HitTarget::PlayerDevice) => Some(Action::Navigate(Route::Devices)),
                Some(HitTarget::PlayerPrevious) => Some(Action::Previous),
                Some(HitTarget::PlayerNext) => Some(Action::Next),
                Some(HitTarget::PlayerProgress) => {
                    let desired = hits.rect_for(&HitTarget::PlayerProgress).and_then(|area| {
                        seek_position_at(area, event.column, state.playback.duration_ms)
                    });
                    desired.map(|desired| {
                        let current = state.playback.progress_at(now);
                        Action::Seek(
                            i64::try_from(desired)
                                .unwrap_or(i64::MAX)
                                .saturating_sub(i64::try_from(current).unwrap_or(i64::MAX)),
                        )
                    })
                }
                Some(HitTarget::PlayerVolume) => {
                    let desired = hits.rect_for(&HitTarget::PlayerVolume).map_or(
                        state.playback.volume,
                        |area| {
                            let column = event.column.saturating_sub(area.x).min(area.width);
                            u8::try_from(u32::from(column) * 100 / u32::from(area.width.max(1)))
                                .unwrap_or(100)
                        },
                    );
                    Some(Action::Volume(
                        i8::try_from(desired)
                            .unwrap_or(100)
                            .saturating_sub(i8::try_from(state.playback.volume).unwrap_or(100)),
                    ))
                }
                Some(HitTarget::PlayerShuffle) => Some(Action::Shuffle),
                Some(HitTarget::PlayerRepeat) => Some(Action::Repeat),
                Some(HitTarget::SearchField) => Some(Action::StartSearch),
                Some(HitTarget::Help) => Some(Action::ToggleHelp),
                Some(HitTarget::DetailPlay) => Some(Action::PlayDetailContext),
                Some(HitTarget::DetailShuffle) => Some(Action::PlayDetailContextWithShuffle),
                Some(HitTarget::DetailOpenSpotify) => Some(Action::OpenDetailSpotify),
                _ => None,
            };
            if let Some(action) = direct {
                return reduce(state, action);
            }
            select_target(state, target.as_ref());
            if matches!(target, Some(HitTarget::Sidebar(_))) {
                return reduce(state, Action::Activate);
            }
            if double { reduce(state, Action::Activate) } else { Vec::new() }
        }
        _ => Vec::new(),
    }
}

fn hovered_focus(hits: &HitMap, column: u16, row: u16) -> Option<FocusRegion> {
    if hits.queue.is_some_and(|area| area.contains((column, row).into())) {
        Some(FocusRegion::Queue)
    } else if hits.sidebar.is_some_and(|area| area.contains((column, row).into())) {
        Some(FocusRegion::Sidebar)
    } else if hits.content.is_some_and(|area| area.contains((column, row).into())) {
        Some(FocusRegion::Content)
    } else {
        None
    }
}

/// Maps a terminal pointer column to a playback position using the actual progress hit rectangle.
/// The last visible cell maps to the end of the track and invalid durations cannot be sought.
fn seek_position_at(track: Rect, column: u16, duration_ms: u64) -> Option<u64> {
    if duration_ms == 0 || track.width == 0 {
        return None;
    }
    let offset = column.saturating_sub(track.x).min(track.width.saturating_sub(1));
    let denominator = u64::from(track.width.saturating_sub(1).max(1));
    Some(duration_ms.saturating_mul(u64::from(offset)) / denominator)
}
fn select_target(state: &mut AppState, target: Option<&HitTarget>) {
    match target {
        Some(HitTarget::Sidebar(index)) => {
            state.focus = FocusRegion::Sidebar;
            state.sidebar.selected = *index;
        }
        Some(HitTarget::ContentRow(index)) => {
            state.focus = FocusRegion::Content;
            state.page.cursor.selected = *index;
        }
        Some(HitTarget::QueueRow(index)) => {
            state.focus = FocusRegion::Queue;
            state.queue.selected = *index;
        }
        Some(HitTarget::ActionRow(index)) => {
            if let Some(Overlay::Actions { selected }) = &mut state.overlay {
                *selected = *index;
            }
        }
        _ => {}
    }
}

#[allow(clippy::too_many_lines)]
pub fn reduce(state: &mut AppState, action: Action) -> Vec<Effect> {
    match action {
        Action::Navigate(route) => load_route(state, route),
        Action::Back => {
            state.back();
            Vec::new()
        }
        Action::FocusNext(reverse) => {
            state.cycle_focus(reverse);
            Vec::new()
        }
        Action::Move(delta) => {
            match &mut state.overlay {
                Some(Overlay::Actions { selected }) => {
                    *selected = selected.saturating_add_signed(delta).min(5);
                }
                Some(Overlay::Menu) => state.sidebar.move_by(delta, AppState::NAVIGATION.len(), 6),
                _ => move_selection(state, delta),
            }
            Vec::new()
        }
        Action::Page(direction) => {
            let step = isize::try_from(state.content_height.max(1)).unwrap_or(isize::MAX);
            move_selection(state, step.saturating_mul(direction));
            Vec::new()
        }
        Action::Home => {
            move_to_edge(state, false);
            Vec::new()
        }
        Action::End => {
            move_to_edge(state, true);
            Vec::new()
        }
        Action::LocalLeft => {
            cycle_local(state, false);
            Vec::new()
        }
        Action::LocalRight => {
            cycle_local(state, true);
            Vec::new()
        }
        Action::Activate => {
            if let Some(Overlay::Actions { selected }) = state.overlay.clone() {
                activate_action_menu(state, selected)
            } else {
                activate(state)
            }
        }
        Action::OpenActions => {
            state.overlay = Some(
                if state.layout == crate::state::LayoutMode::Compact
                    && state.focus != FocusRegion::Content
                {
                    Overlay::Menu
                } else {
                    Overlay::Actions { selected: 0 }
                },
            );
            Vec::new()
        }
        Action::Inspect => {
            if state.selected_item().is_some() {
                state.overlay = Some(Overlay::Inspector);
            }
            Vec::new()
        }
        Action::ToggleHelp => {
            state.overlay = if matches!(state.overlay, Some(Overlay::Help { .. })) {
                None
            } else {
                Some(Overlay::Help { query: String::new(), editing: false })
            };
            Vec::new()
        }
        Action::StartSearch => {
            if let Some(Overlay::Help { editing, .. }) = &mut state.overlay {
                *editing = true;
                return Vec::new();
            }
            if state.page.route != Route::Search {
                let effects = load_route(state, Route::Search);
                state.text_entry = true;
                return effects;
            }
            state.text_entry = true;
            Vec::new()
        }
        Action::StartFilter => {
            state.text_entry = true;
            Vec::new()
        }
        Action::Insert(ch) => {
            insert_text(state, ch);
            Vec::new()
        }
        Action::Backspace => {
            backspace(state);
            Vec::new()
        }
        Action::SubmitText => submit_text(state),
        Action::Cancel => {
            if state.overlay.is_some() {
                state.overlay = None;
            } else if state.text_entry {
                state.text_entry = false;
                if state.page.route == Route::Search && state.search_query.is_empty() {
                    state.populate_recent_searches();
                }
            } else if state.page.route == Route::Search && !state.search_query.is_empty() {
                state.search_query.clear();
                state.populate_recent_searches();
            } else {
                state.back();
            }
            Vec::new()
        }
        Action::TogglePlayback if state.playback.cached_track && !state.playback.playing => {
            state.playback.track_uri.clone().map_or_else(Vec::new, |uri| {
                vec![Effect::PlayTrack { uri, device_id: state.selected_device_id.clone() }]
            })
        }
        Action::TogglePlayback => vec![Effect::TogglePlayback],
        Action::Previous => {
            let current_pos = state.playback.progress_at(Instant::now());
            if current_pos > 3_000 {
                state.playback.progress_ms = 0;
                state.playback.observed_at = Some(Instant::now());
            } else if let Some(previous_playback) = state.playback_history.pop() {
                if let Some(current_item) = state.playback_as_browse_item() {
                    state.queue_upcoming.insert(0, current_item);
                }
                state.playback = previous_playback;
                state.playback.progress_ms = 0;
                state.playback.playing = true;
                state.playback.observed_at = Some(Instant::now());
                state.playback.cached_track = false;
                state.queue_now = state.playback_as_browse_item();
            } else {
                state.playback.progress_ms = 0;
                state.playback.observed_at = Some(Instant::now());
            }
            vec![Effect::Previous]
        }
        Action::Next => {
            if !state.queue_upcoming.is_empty() {
                state.push_playback_history();
                let next_track = state.queue_upcoming.remove(0);
                state.queue.selected =
                    state.queue.selected.min(state.queue_upcoming.len().saturating_sub(1));
                state.apply_track_to_playback(&next_track);
                state.queue_now = Some(next_track);
            }
            vec![Effect::Next]
        }
        Action::Seek(offset) => vec![Effect::Seek(offset)],
        Action::Volume(delta) => {
            state.playback.volume = state.playback.volume.saturating_add_signed(delta).min(100);
            vec![Effect::Volume(state.playback.volume)]
        }
        Action::Shuffle => {
            state.playback.shuffle = !state.playback.shuffle;
            vec![Effect::Shuffle(state.playback.shuffle)]
        }
        Action::Repeat => {
            state.playback.repeat = (state.playback.repeat + 1) % 3;
            vec![Effect::Repeat(state.playback.repeat)]
        }
        Action::Enqueue => enqueue_selected(state),
        Action::Refresh => vec![Effect::LoadPage {
            route: state.page.route.clone(),
            generation: state.generation,
            offset: 0,
            query: state.search_query.clone(),
        }],
        Action::PlayDetailContext => detail_context_uri(state).map_or_else(Vec::new, |uri| {
            vec![Effect::PlayTrack { uri, device_id: state.selected_device_id.clone() }]
        }),
        Action::PlayDetailContextWithShuffle => {
            detail_context_uri(state).map_or_else(Vec::new, |uri| {
                state.playback.shuffle = true;
                vec![
                    Effect::PlayTrack { uri, device_id: state.selected_device_id.clone() },
                    Effect::Shuffle(true),
                ]
            })
        }
        Action::OpenDetailSpotify => detail_external_url(state).map_or_else(Vec::new, |url| {
            vec![Effect::OpenExternal(url)]
        }),
        Action::Quit => {
            state.quit = true;
            Vec::new()
        }
    }
}

fn has_detail_actions(state: &AppState) -> bool {
    matches!(
        state.page.route,
        Route::Album { .. } | Route::Playlist { .. } | Route::Artist { .. }
    ) && state.overlay.is_none()
        && !state.text_entry
}

fn detail_context_uri(state: &AppState) -> Option<String> {
    state.page.uri.clone().or_else(|| match &state.page.route {
        Route::Album { uri, .. } | Route::Playlist { uri, .. } | Route::Artist { uri, .. } => {
            Some(uri.clone())
        }
        _ => None,
    })
}

fn detail_external_url(state: &AppState) -> Option<String> {
    state.page.external_url.clone()
}

fn load_route(state: &mut AppState, route: Route) -> Vec<Effect> {
    let is_search = route == Route::Search;
    let is_search_empty = is_search && state.search_query.trim().is_empty();
    let is_fresh = state
        .page_cache
        .get(&route.key())
        .is_some_and(|(_, cached_at)| cached_at.elapsed() < Duration::from_secs(60));

    let generation = state.navigate(route.clone());
    if is_search {
        state.text_entry = true;
    }

    if is_search_empty {
        state.populate_recent_searches();
        return Vec::new();
    }

    if is_fresh && route != Route::Queue && route != Route::Devices {
        return Vec::new();
    }

    vec![Effect::LoadPage { route, generation, offset: 0, query: state.search_query.clone() }]
}

fn move_selection(state: &mut AppState, delta: isize) {
    match state.focus {
        FocusRegion::Sidebar => state.sidebar.move_by(delta, AppState::NAVIGATION.len(), 6),
        FocusRegion::Content => {
            let len = state.page.flattened().len();
            state.page.cursor.move_by(delta, len, state.content_height);
        }
        FocusRegion::Queue => {
            state.queue.move_by(delta, state.queue_upcoming.len(), state.queue_height);
        }
        FocusRegion::Player => {}
    }
}

fn move_to_edge(state: &mut AppState, end: bool) {
    let content_len = state.page.flattened().len();
    let (cursor, len, height) = match state.focus {
        FocusRegion::Sidebar => (&mut state.sidebar, AppState::NAVIGATION.len(), 6),
        FocusRegion::Queue => (&mut state.queue, state.queue_upcoming.len(), state.queue_height),
        FocusRegion::Content => (&mut state.page.cursor, content_len, state.content_height),
        FocusRegion::Player => return,
    };
    cursor.move_to(if end { len.saturating_sub(1) } else { 0 }, len, height);
}

fn cycle_local(state: &mut AppState, right: bool) {
    match state.page.route {
        Route::Library => {
            state.page.library_tab = match (state.page.library_tab, right) {
                (LibraryTab::Albums, true) | (LibraryTab::Playlists, false) => {
                    LibraryTab::Playlists
                }
                _ => LibraryTab::Albums,
            }
        }
        Route::Search => {
            let current = SearchFilter::ALL
                .iter()
                .position(|filter| *filter == state.page.search_filter)
                .unwrap_or(0);
            let index = if right {
                (current + 1) % SearchFilter::ALL.len()
            } else {
                current.checked_sub(1).unwrap_or(SearchFilter::ALL.len() - 1)
            };
            state.page.search_filter = SearchFilter::ALL[index];
        }
        _ => {}
    }
    state.page.cursor = crate::state::ListCursor::default();
}

fn activate(state: &mut AppState) -> Vec<Effect> {
    if matches!(state.overlay, Some(Overlay::Menu)) {
        return AppState::NAVIGATION
            .get(state.sidebar.selected)
            .cloned()
            .map_or_else(Vec::new, |route| load_route(state, route));
    }
    if state.focus == FocusRegion::Sidebar {
        return AppState::NAVIGATION
            .get(state.sidebar.selected)
            .cloned()
            .map_or_else(Vec::new, |route| load_route(state, route));
    }
    let Some(item) = state.selected_item().cloned() else {
        return Vec::new();
    };
    match item.kind {
        EntityKind::Track => {
            if !item.available {
                state.notice = Some(Notice {
                    kind: NoticeKind::Error,
                    text: "This track is unavailable".into(),
                });
                return Vec::new();
            }
            state.push_playback_history();
            state.apply_track_to_playback(&item);
            state.queue_now = Some(item.clone());
            if let Some(context) = item.context {
                vec![Effect::PlayContext {
                    uri: context.uri,
                    position: context.position,
                    device_id: state.selected_device_id.clone(),
                }]
            } else {
                item.uri.map_or_else(Vec::new, |uri| {
                    vec![Effect::PlayTrack { uri, device_id: state.selected_device_id.clone() }]
                })
            }
        }
        EntityKind::Album => item.uri.map_or_else(Vec::new, |uri| {
            load_route(state, Route::Album { uri, title: item.title })
        }),
        EntityKind::Playlist => item.uri.map_or_else(Vec::new, |uri| {
            load_route(state, Route::Playlist { uri, title: item.title })
        }),
        EntityKind::Artist => item.uri.map_or_else(Vec::new, |uri| {
            load_route(state, Route::Artist { uri, title: item.title })
        }),
        EntityKind::Device => {
            if item.restricted {
                state.notice = Some(Notice { kind: NoticeKind::Error, text: "Spotify marks this device as restricted; commands cannot be transferred to it.".into() });
                Vec::new()
            } else {
                item.uri.map_or_else(Vec::new, |id| {
                    state.selected_device_id = Some(id.clone());
                    state.device_selected_by_user = true;
                    vec![Effect::Transfer(id)]
                })
            }
        }
        EntityKind::Action if item.id.starts_with("recent-search:") => {
            let query = item.title.clone();
            state.search_query.clone_from(&query);
            state.recent_searches.retain(|q| !q.eq_ignore_ascii_case(&query));
            state.recent_searches.insert(0, query.clone());
            state.generation = state.generation.saturating_add(1);
            state.page.generation = state.generation;
            state.page.state = crate::state::LoadState::Loading;
            vec![
                Effect::LoadPage {
                    route: Route::Search,
                    generation: state.generation,
                    offset: 0,
                    query,
                },
                Effect::SaveSettings,
            ]
        }
        EntityKind::Action if item.id == "play-context" => item.uri.map_or_else(Vec::new, |uri| {
            vec![Effect::PlayTrack { uri, device_id: state.selected_device_id.clone() }]
        }),
        EntityKind::Action if item.id == "open-external" => {
            item.external_url.map_or_else(Vec::new, |url| vec![Effect::OpenExternal(url)])
        }
        EntityKind::Action => activate_setting(state, &item.id),
        EntityKind::Message => Vec::new(),
    }
}

fn activate_action_menu(state: &mut AppState, selected: usize) -> Vec<Effect> {
    let item = state.selected_item().cloned();
    state.overlay = None;
    let Some(item) = item else {
        return Vec::new();
    };
    match selected {
        0 => activate(state),
        1 => enqueue_selected(state),
        2 => item
            .album
            .map_or_else(Vec::new, |(title, uri)| load_route(state, Route::Album { uri, title })),
        3 => item
            .artists
            .first()
            .and_then(|(title, uri)| uri.as_ref().map(|uri| (title.clone(), uri.clone())))
            .map_or_else(Vec::new, |(title, uri)| load_route(state, Route::Artist { uri, title })),
        4 => item.external_url.map_or_else(Vec::new, |url| vec![Effect::OpenExternal(url)]),
        _ => {
            state.overlay = Some(Overlay::Inspector);
            Vec::new()
        }
    }
}

fn activate_setting(state: &mut AppState, id: &str) -> Vec<Effect> {
    match id {
        "artwork" => {
            state.artwork = match state.artwork {
                mellowdeck_core::CliArtworkPreference::Auto => {
                    mellowdeck_core::CliArtworkPreference::Blocks
                }
                mellowdeck_core::CliArtworkPreference::Blocks => {
                    mellowdeck_core::CliArtworkPreference::Off
                }
                mellowdeck_core::CliArtworkPreference::Off => {
                    mellowdeck_core::CliArtworkPreference::Auto
                }
            }
        }
        "mouse" => state.mouse = !state.mouse,
        "wide-queue" => state.wide_queue = !state.wide_queue,
        _ => return Vec::new(),
    }
    if let Some(item) = state
        .page
        .sections
        .iter_mut()
        .flat_map(|section| &mut section.items)
        .find(|item| item.id == id)
    {
        item.title = match id {
            "artwork" => format!("Artwork: {:?}", state.artwork),
            "mouse" => format!("Mouse input: {}", if state.mouse { "enabled" } else { "disabled" }),
            "wide-queue" => format!(
                "Wide-screen queue: {}",
                if state.wide_queue { "enabled" } else { "disabled" }
            ),
            _ => item.title.clone(),
        };
    }
    vec![Effect::SaveSettings]
}

fn enqueue_selected(state: &mut AppState) -> Vec<Effect> {
    let Some(item) = state.selected_item() else {
        return Vec::new();
    };
    if item.kind != EntityKind::Track || !item.available {
        return Vec::new();
    }
    let uri = item.uri.clone();
    if let Some(uri) = uri {
        state.notice = Some(Notice {
            kind: NoticeKind::Pending,
            text: format!("Adding {} to queue…", item.title),
        });
        vec![Effect::Enqueue(uri)]
    } else {
        Vec::new()
    }
}

fn insert_text(state: &mut AppState, ch: char) {
    if let Some(Overlay::Help { query, .. }) = &mut state.overlay {
        query.push(ch);
    } else if state.page.route == Route::Search {
        state.search_query.push(ch);
    } else {
        state.page.filter.push(ch);
    }
}

fn backspace(state: &mut AppState) {
    if let Some(Overlay::Help { query, .. }) = &mut state.overlay {
        query.pop();
    } else if state.page.route == Route::Search {
        state.search_query.pop();
    } else {
        state.page.filter.pop();
    }
}

fn submit_text(state: &mut AppState) -> Vec<Effect> {
    if matches!(state.overlay, Some(Overlay::Help { .. })) {
        state.text_entry = false;
        return Vec::new();
    }
    if state.page.route == Route::Search {
        let trimmed = state.search_query.trim().to_string();
        if trimmed.is_empty() {
            state.populate_recent_searches();
            return Vec::new();
        }
        state.text_entry = false;
        state.recent_searches.retain(|q| !q.eq_ignore_ascii_case(&trimmed));
        state.recent_searches.insert(0, trimmed);
        if state.recent_searches.len() > 10 {
            state.recent_searches.truncate(10);
        }
        state.generation = state.generation.saturating_add(1);
        state.page.generation = state.generation;
        state.page.state = crate::state::LoadState::Loading;
        vec![
            Effect::LoadPage {
                route: Route::Search,
                generation: state.generation,
                offset: 0,
                query: state.search_query.clone(),
            },
            Effect::SaveSettings,
        ]
    } else {
        state.text_entry = false;
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shuffle_and_repeat_mouse_targets_dispatch_only_inside_their_rectangles() {
        let now = Instant::now();
        let mut state = AppState::default();
        let mut hits = HitMap::default();
        hits.add(Rect { x: 4, y: 2, width: 12, height: 1 }, HitTarget::PlayerShuffle);
        hits.add(Rect { x: 17, y: 2, width: 12, height: 1 }, HitTarget::PlayerRepeat);
        let click = |column| MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row: 2,
            modifiers: KeyModifiers::NONE,
        };
        assert_eq!(
            dispatch_mouse(&mut state, &mut hits, click(5), now),
            vec![Effect::Shuffle(true)]
        );
        assert_eq!(
            dispatch_mouse(&mut state, &mut hits, click(18), now + Duration::from_secs(1)),
            vec![Effect::Repeat(1)]
        );
        assert!(
            dispatch_mouse(&mut state, &mut hits, click(16), now + Duration::from_secs(2))
                .is_empty()
        );
    }

    use crate::{BrowseItem, ContextPosition, LoadState, PageState, Section};
    use crossterm::event::{KeyEvent, MouseEvent};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn focus_isolated_and_cycles_visible_regions() {
        let mut state = AppState::default();
        state.update_layout(160, 45);
        dispatch_key(&mut state, key(KeyCode::Tab));
        assert_eq!(state.focus, FocusRegion::Queue);
        dispatch_key(&mut state, key(KeyCode::Tab));
        assert_eq!(state.focus, FocusRegion::Player);
    }

    #[test]
    fn text_entry_consumes_playback_shortcuts() {
        let mut state = AppState::default();
        dispatch_key(&mut state, key(KeyCode::Char('/')));
        let effects = dispatch_key(&mut state, key(KeyCode::Char(' ')));
        assert!(effects.is_empty());
        assert_eq!(state.search_query, " ");
    }

    #[test]
    fn quit_shortcuts_are_global_without_claiming_lowercase_queue() {
        for (key_event, text_entry, overlay) in [
            (key(KeyCode::Char('x')), false, None),
            (key(KeyCode::Char('Q')), false, Some(Overlay::Inspector)),
            (KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL), true, None),
            (
                KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL),
                false,
                Some(Overlay::Help { query: String::new(), editing: true }),
            ),
        ] {
            let mut state = AppState { overlay, text_entry, ..AppState::default() };
            dispatch_key(&mut state, key_event);
            assert!(state.quit);
        }

        let mut state = AppState::default();
        dispatch_key(&mut state, key(KeyCode::Char('q')));
        assert_eq!(state.page.route, Route::Queue);
        assert!(!state.quit);
    }

    #[test]
    fn history_restores_selection_scroll_and_filter() {
        let mut state = AppState::default();
        state.page.cursor.selected = 8;
        state.page.cursor.offset = 4;
        state.page.filter = "mix".into();
        reduce(&mut state, Action::Navigate(Route::Library));
        reduce(&mut state, Action::Cancel);
        assert_eq!(state.page.cursor.selected, 8);
        assert_eq!(state.page.cursor.offset, 4);
        assert_eq!(state.page.filter, "mix");
    }

    #[test]
    fn progress_seek_uses_the_visible_track_and_clamps() {
        let track = Rect { x: 20, y: 3, width: 5, height: 1 };
        assert_eq!(seek_position_at(track, 20, 200), Some(0));
        assert_eq!(seek_position_at(track, 21, 200), Some(50));
        assert_eq!(seek_position_at(track, 22, 200), Some(100));
        assert_eq!(seek_position_at(track, 23, 200), Some(150));
        assert_eq!(seek_position_at(track, 24, 200), Some(200));
        assert_eq!(seek_position_at(track, 2, 200), Some(0));
        assert_eq!(seek_position_at(track, 40, 200), Some(200));
        assert_eq!(seek_position_at(track, 20, 0), None);
    }
    #[test]
    fn shift_arrows_seek_while_plain_arrows_are_local() {
        let mut state = AppState::default();
        let seek = dispatch_key(&mut state, KeyEvent::new(KeyCode::Right, KeyModifiers::SHIFT));
        assert_eq!(seek, vec![Effect::Seek(5_000)]);
        assert!(dispatch_key(&mut state, key(KeyCode::Right)).is_empty());
    }

    #[test]
    fn escape_closes_overlay_before_restoring_history() {
        let mut state = AppState::default();
        state.history.push(PageState::loading(Route::Library, 0));
        state.overlay = Some(Overlay::Inspector);
        dispatch_key(&mut state, key(KeyCode::Esc));
        assert!(state.overlay.is_none());
        assert_eq!(state.page.route, Route::Home);
    }

    #[test]
    fn compact_menu_activation_uses_sidebar_selection() {
        let mut state = AppState::default();
        state.update_layout(80, 24);
        state.overlay = Some(Overlay::Menu);
        state.sidebar.selected = 2;
        let effects = dispatch_key(&mut state, key(KeyCode::Enter));
        assert!(matches!(state.page.route, Route::Library));
        assert!(matches!(effects.as_slice(), [Effect::LoadPage { route: Route::Library, .. }]));
    }

    #[test]
    fn sidebar_mouse_click_navigates_immediately() {
        let mut state = AppState::default();
        state.update_layout(120, 35);
        let mut hits = HitMap::default();
        hits.add(Rect { x: 0, y: 0, width: 18, height: 1 }, HitTarget::Sidebar(2));
        let effects = dispatch_mouse(
            &mut state,
            &mut hits,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 2,
                row: 0,
                modifiers: KeyModifiers::NONE,
            },
            Instant::now(),
        );
        assert_eq!(state.page.route, Route::Library);
        assert!(matches!(effects.as_slice(), [Effect::LoadPage { route: Route::Library, .. }]));
    }

    #[test]
    fn browse_play_enqueue_queue_and_back_preserves_playlist_position() {
        let mut library = page_with(
            Route::Library,
            vec![item(EntityKind::Playlist, "Mix", "spotify:playlist:mix", None)],
        );
        library.library_tab = LibraryTab::Playlists;
        let mut state = AppState { page: library, ..AppState::default() };
        let open = reduce(&mut state, Action::Activate);
        assert!(matches!(
            open.as_slice(),
            [Effect::LoadPage { route: Route::Playlist { .. }, .. }]
        ));

        state.page = page_with(
            Route::Playlist { uri: "spotify:playlist:mix".into(), title: "Mix".into() },
            vec![
                item(EntityKind::Track, "First", "spotify:track:one", Some(0)),
                item(EntityKind::Track, "Middle", "spotify:track:same", Some(1)),
                item(EntityKind::Track, "Duplicate", "spotify:track:same", Some(2)),
            ],
        );
        state.page.cursor.selected = 1;
        state.page.cursor.offset = 1;
        let play = reduce(&mut state, Action::Activate);
        assert!(matches!(play.as_slice(), [Effect::PlayContext { position: 1, .. }]));
        let snapshot = state.page.clone();

        reduce(
            &mut state,
            Action::Navigate(Route::Artist {
                uri: "spotify:artist:a".into(),
                title: "Artist".into(),
            }),
        );
        state.page = page_with(
            Route::Artist { uri: "spotify:artist:a".into(), title: "Artist".into() },
            vec![item(EntityKind::Album, "Album", "spotify:album:a", None)],
        );
        reduce(&mut state, Action::Activate);
        state.page = page_with(
            Route::Album { uri: "spotify:album:a".into(), title: "Album".into() },
            vec![item(EntityKind::Track, "Song", "spotify:track:song", Some(0))],
        );
        assert!(
            matches!(reduce(&mut state, Action::Enqueue).as_slice(), [Effect::Enqueue(uri)] if uri == "spotify:track:song")
        );
        reduce(&mut state, Action::Navigate(Route::Queue));
        state.history.push(snapshot.clone());
        state.back();
        assert_eq!(state.page.route, snapshot.route);
        assert_eq!(state.page.cursor, snapshot.cursor);
    }

    fn page_with(route: Route, items: Vec<BrowseItem>) -> PageState {
        let mut page = PageState::loading(route, 0);
        page.state = LoadState::Ready;
        page.sections = vec![Section { title: "Items".into(), items }];
        page
    }

    fn item(kind: EntityKind, title: &str, uri: &str, position: Option<usize>) -> BrowseItem {
        BrowseItem {
            id: format!("{title}-{position:?}"),
            kind,
            title: title.into(),
            subtitle: "Artist".into(),
            metadata: String::new(),
            uri: Some(uri.into()),
            external_url: None,
            artwork_url: None,
            artists: vec![("Artist".into(), Some("spotify:artist:a".into()))],
            album: Some(("Album".into(), "spotify:album:a".into())),
            duration_ms: Some(180_000),
            available: true,
            context: position
                .map(|position| ContextPosition { uri: "spotify:playlist:mix".into(), position }),
            restricted: false,
        }
    }

    #[test]
    fn next_action_advances_queue_and_updates_playback() {
        let mut state = AppState::default();
        state.playback.track_uri = Some("spotify:track:current".into());
        state.playback.title = "Current".into();
        state.queue_upcoming = vec![
            item(EntityKind::Track, "Upcoming 1", "spotify:track:up1", None),
            item(EntityKind::Track, "Upcoming 2", "spotify:track:up2", None),
        ];

        let effects = reduce(&mut state, Action::Next);
        assert_eq!(effects, vec![Effect::Next]);
        assert_eq!(state.playback.track_uri.as_deref(), Some("spotify:track:up1"));
        assert_eq!(state.playback.title, "Upcoming 1");
        assert_eq!(state.queue_upcoming.len(), 1);
        assert_eq!(state.queue_upcoming[0].title, "Upcoming 2");
        assert_eq!(state.playback_history.len(), 1);
        assert_eq!(state.playback_history[0].title, "Current");
    }

    #[test]
    fn previous_action_rewinds_if_over_three_seconds() {
        let mut state = AppState::default();
        state.playback.track_uri = Some("spotify:track:current".into());
        state.playback.title = "Current".into();
        state.playback.duration_ms = 200_000;
        state.playback.progress_ms = 10_000;
        state.playback.playing = false;

        let effects = reduce(&mut state, Action::Previous);
        assert_eq!(effects, vec![Effect::Previous]);
        assert_eq!(state.playback.progress_ms, 0);
        assert_eq!(state.playback.title, "Current");
    }

    #[test]
    fn previous_action_restores_from_history_if_under_three_seconds() {
        let mut state = AppState::default();
        state.playback.track_uri = Some("spotify:track:song1".into());
        state.playback.title = "Song 1".into();
        state.push_playback_history();

        state.playback.track_uri = Some("spotify:track:song2".into());
        state.playback.title = "Song 2".into();
        state.playback.progress_ms = 1_000;
        state.playback.playing = false;

        let effects = reduce(&mut state, Action::Previous);
        assert_eq!(effects, vec![Effect::Previous]);
        assert_eq!(state.playback.track_uri.as_deref(), Some("spotify:track:song1"));
        assert_eq!(state.playback.title, "Song 1");
        assert_eq!(state.playback.progress_ms, 0);
        assert!(state.playback_history.is_empty());
        // Song 2 was prepended back to queue_upcoming so subsequent Next preserves it
        assert_eq!(state.queue_upcoming.len(), 1);
        assert_eq!(state.queue_upcoming[0].title, "Song 2");
        assert_eq!(state.queue_now.as_ref().map(|n| n.title.as_str()), Some("Song 1"));
    }

    #[test]
    fn next_action_when_queue_empty_does_not_push_to_history() {
        let mut state = AppState::default();
        state.playback.track_uri = Some("spotify:track:current".into());
        state.playback.title = "Current".into();
        state.queue_upcoming.clear();

        let effects = reduce(&mut state, Action::Next);
        assert_eq!(effects, vec![Effect::Next]);
        assert_eq!(state.playback.title, "Current");
        assert!(state.playback_history.is_empty());
    }

    #[test]
    fn activate_track_optimistically_updates_playback() {
        let mut state = AppState::default();
        state.playback.track_uri = Some("spotify:track:initial".into());
        state.playback.title = "Initial".into();
        state.page = page_with(
            Route::Home,
            vec![item(EntityKind::Track, "Activated Track", "spotify:track:activated", None)],
        );
        state.page.cursor.selected = 0;

        let effects = reduce(&mut state, Action::Activate);
        assert!(matches!(effects.as_slice(), [Effect::PlayTrack { uri, .. }] if uri == "spotify:track:activated"));
        assert_eq!(state.playback.track_uri.as_deref(), Some("spotify:track:activated"));
        assert_eq!(state.playback.title, "Activated Track");
        assert_eq!(state.playback_history.len(), 1);
        assert_eq!(state.playback_history[0].title, "Initial");
    }

    #[test]
    fn navigation_shortcuts_one_to_four_switch_tabs() {
        let mut state = AppState::default();
        let _ = dispatch_key(&mut state, key(KeyCode::Char('2')));
        assert_eq!(state.page.route, Route::Search);
        assert!(state.text_entry);

        // Exiting text entry allows numeric shortcut navigation
        let _ = dispatch_key(&mut state, key(KeyCode::Down));
        assert!(!state.text_entry);

        let _ = dispatch_key(&mut state, key(KeyCode::Char('3')));
        assert_eq!(state.page.route, Route::Library);

        let _ = dispatch_key(&mut state, key(KeyCode::Char('4')));
        assert_eq!(state.page.route, Route::Settings);

        let _ = dispatch_key(&mut state, key(KeyCode::Char('1')));
        assert_eq!(state.page.route, Route::Home);
    }

    #[test]
    fn navigating_to_search_auto_selects_input_and_enter_does_not_activate_track() {
        let mut state = AppState {
            page: page_with(
                Route::Home,
                vec![item(EntityKind::Track, "Initial Track", "spotify:track:initial", None)],
            ),
            ..AppState::default()
        };

        // Press '2' to go to Search
        let _ = dispatch_key(&mut state, key(KeyCode::Char('2')));
        assert_eq!(state.page.route, Route::Search);
        assert!(state.text_entry, "Search input should be auto-selected");

        // Pressing Enter immediately on empty search query does NOT play music
        let effects = dispatch_key(&mut state, key(KeyCode::Enter));
        assert!(effects.is_empty(), "Enter on empty search should not emit play or load effects");
        assert_eq!(state.playback.track_uri, None, "Track must not start playing");
        assert!(state.text_entry, "Should remain in text entry if empty query submitted");

        // Typing characters goes into search query, not global shortcuts
        let _ = dispatch_key(&mut state, key(KeyCode::Char('r')));
        let _ = dispatch_key(&mut state, key(KeyCode::Char('a')));
        let _ = dispatch_key(&mut state, key(KeyCode::Char('d')));
        assert_eq!(state.search_query, "rad");

        // Submitting with query emits LoadPage and SaveSettings
        let effects = dispatch_key(&mut state, key(KeyCode::Enter));
        assert!(!state.text_entry, "Text entry should close on search submission");
        assert!(effects.contains(&Effect::SaveSettings));
        assert!(effects.iter().any(|e| matches!(e, Effect::LoadPage { query, .. } if query == "rad")));
    }

    #[test]
    fn search_input_down_and_up_arrow_navigation() {
        let mut state = AppState {
            recent_searches: vec!["arcade fire".into(), "radiohead".into()],
            ..AppState::default()
        };
        let _ = dispatch_key(&mut state, key(KeyCode::Char('2')));
        assert_eq!(state.page.route, Route::Search);
        assert!(state.text_entry);

        // Pressing Down leaves text entry and focuses content list at index 0
        let _ = dispatch_key(&mut state, key(KeyCode::Down));
        assert!(!state.text_entry);
        assert_eq!(state.focus, FocusRegion::Content);
        assert_eq!(state.page.cursor.selected, 0);

        // Pressing Down again moves cursor to index 1
        let _ = dispatch_key(&mut state, key(KeyCode::Down));
        assert_eq!(state.page.cursor.selected, 1);

        // Pressing Up moves cursor to index 0
        let _ = dispatch_key(&mut state, key(KeyCode::Up));
        assert_eq!(state.page.cursor.selected, 0);

        // Pressing Up at index 0 re-enters search text entry
        let _ = dispatch_key(&mut state, key(KeyCode::Up));
        assert!(state.text_entry);
    }

    #[test]
    fn search_submission_caches_recent_searches() {
        let mut state = AppState::default();
        state.page.route = Route::Search;
        state.search_query = "radiohead".into();

        let effects = submit_text(&mut state);
        assert_eq!(state.recent_searches, vec!["radiohead"]);
        assert!(effects.contains(&Effect::SaveSettings));
        assert!(effects.iter().any(|e| matches!(e, Effect::LoadPage { query, .. } if query == "radiohead")));

        // Duplicate submission brings it to the top without duplicates
        state.search_query = "daft punk".into();
        let _ = submit_text(&mut state);
        assert_eq!(state.recent_searches, vec!["daft punk", "radiohead"]);

        state.search_query = "radiohead".into();
        let _ = submit_text(&mut state);
        assert_eq!(state.recent_searches, vec!["radiohead", "daft punk"]);
    }

    #[test]
    #[allow(clippy::field_reassign_with_default)]
    fn recent_search_item_activation_executes_search() {
        let mut state = AppState::default();
        state.recent_searches = vec!["arcade fire".into()];
        state.populate_recent_searches();
        state.page.cursor.selected = 0;

        let effects = reduce(&mut state, Action::Activate);
        assert_eq!(state.search_query, "arcade fire");
        assert!(effects.iter().any(|e| matches!(e, Effect::LoadPage { query, .. } if query == "arcade fire")));
    }

    #[test]
    fn page_cache_fast_path_avoids_redundant_load_effects() {
        let mut state = AppState::default();
        let home_page = PageState {
            route: Route::Home,
            title: "Home".into(),
            subtitle: "Cached".into(),
            artwork_url: None,
            uri: None,
            external_url: None,
            sections: Vec::new(),
            cursor: crate::state::ListCursor::default(),
            state: LoadState::Ready,
            filter: String::new(),
            library_tab: LibraryTab::Albums,
            search_filter: SearchFilter::All,
            next_offset: None,
            loading_more: false,
            generation: 1,
        };
        state.page_cache.insert(Route::Home.key(), (home_page, Instant::now()));

        // Navigating to Home with fresh cache returns empty effects (no network round-trip)
        let effects = load_route(&mut state, Route::Home);
        assert!(effects.is_empty());
        assert_eq!(state.page.title, "Home");
        assert_eq!(state.page.subtitle, "Cached");
    }

    #[test]
    fn detail_context_actions_dispatch_via_mouse_and_keyboard() {
        let mut state = AppState::default();
        state.page = PageState::loading(
            Route::Album {
                uri: "spotify:album:1".into(),
                title: "OK Computer".into(),
            },
            1,
        );
        state.page.uri = Some("spotify:album:1".into());
        state.page.external_url = Some("https://open.spotify.com/album/1".into());
        state.selected_device_id = Some("dev-1".into());

        // Keyboard shortcuts
        let play_fx = dispatch_key(&mut state, key(KeyCode::Char('p')));
        assert_eq!(
            play_fx,
            vec![Effect::PlayTrack {
                uri: "spotify:album:1".into(),
                device_id: Some("dev-1".into()),
            }]
        );

        let shuffle_fx = dispatch_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('S'), KeyModifiers::SHIFT),
        );
        assert!(state.playback.shuffle);
        assert_eq!(
            shuffle_fx,
            vec![
                Effect::PlayTrack {
                    uri: "spotify:album:1".into(),
                    device_id: Some("dev-1".into()),
                },
                Effect::Shuffle(true),
            ]
        );

        let spotify_fx = dispatch_key(&mut state, key(KeyCode::Char('o')));
        assert_eq!(
            spotify_fx,
            vec![Effect::OpenExternal("https://open.spotify.com/album/1".into())]
        );

        // Mouse click targets
        let mut hits = HitMap::default();
        hits.add(Rect { x: 10, y: 5, width: 10, height: 1 }, HitTarget::DetailPlay);
        hits.add(Rect { x: 22, y: 5, width: 10, height: 1 }, HitTarget::DetailShuffle);
        hits.add(Rect { x: 34, y: 5, width: 10, height: 1 }, HitTarget::DetailOpenSpotify);

        let click = |col| MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: col,
            row: 5,
            modifiers: KeyModifiers::NONE,
        };

        let mouse_play = dispatch_mouse(&mut state, &mut hits, click(12), Instant::now());
        assert_eq!(
            mouse_play,
            vec![Effect::PlayTrack {
                uri: "spotify:album:1".into(),
                device_id: Some("dev-1".into()),
            }]
        );

        let mouse_shuffle = dispatch_mouse(&mut state, &mut hits, click(24), Instant::now());
        assert_eq!(
            mouse_shuffle,
            vec![
                Effect::PlayTrack {
                    uri: "spotify:album:1".into(),
                    device_id: Some("dev-1".into()),
                },
                Effect::Shuffle(true),
            ]
        );

        let mouse_spotify = dispatch_mouse(&mut state, &mut hits, click(36), Instant::now());
        assert_eq!(
            mouse_spotify,
            vec![Effect::OpenExternal("https://open.spotify.com/album/1".into())]
        );
    }
}
