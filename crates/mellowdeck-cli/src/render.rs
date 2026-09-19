use std::time::Instant;

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Padding, Paragraph, Wrap},
};

use crate::{
    HitMap, HitTarget,
    artwork::{ArtworkManager, ArtworkPlacement},
    state::{
        AppState, EntityKind, FocusRegion, LayoutMode, LibraryTab, LoadState, MIN_HEIGHT,
        MIN_WIDTH, NoticeKind, Overlay, Route, SearchFilter,
    },
};

const INK: Color = Color::Rgb(18, 18, 25);
const INK_SOFT: Color = Color::Rgb(31, 31, 42);
const LAVENDER: Color = Color::Rgb(190, 174, 255);
const SAGE: Color = Color::Rgb(155, 207, 166);
const PEACH: Color = Color::Rgb(255, 187, 153);
const MUTED: Color = Color::Rgb(143, 143, 160);

pub fn render(frame: &mut Frame<'_>, state: &mut AppState, hits: &mut HitMap) {
    render_inner(frame, state, hits, None);
}

pub fn render_with_artwork(
    frame: &mut Frame<'_>,
    state: &mut AppState,
    hits: &mut HitMap,
    artwork: &mut ArtworkManager,
) {
    render_inner(frame, state, hits, Some(artwork));
}

fn render_inner(
    frame: &mut Frame<'_>,
    state: &mut AppState,
    hits: &mut HitMap,
    mut artwork: Option<&mut ArtworkManager>,
) {
    hits.clear();
    let area = frame.area();
    state.update_layout(area.width, area.height);
    let artwork_visible = state.overlay.is_none();
    if !artwork_visible && let Some(manager) = artwork.as_deref_mut() {
        manager.clear_placement();
    }
    frame.render_widget(Block::default().style(Style::default().bg(INK).fg(Color::White)), area);
    if state.layout == LayoutMode::Resize {
        render_resize(frame, area);
        return;
    }
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(10), Constraint::Length(4), Constraint::Length(2)])
        .split(area);
    let body = vertical[0];
    match state.layout {
        LayoutMode::Compact => {
            let header = Rect { x: body.x, y: body.y, width: body.width, height: 2 };
            hits.add(Rect { x: header.x, y: header.y, width: 8, height: 2 }, HitTarget::MenuButton);
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(
                        " ☰ Menu ",
                        Style::default().fg(LAVENDER).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(state.page.route.label(), Style::default().fg(PEACH)),
                ]))
                .block(Block::default().borders(Borders::BOTTOM)),
                header,
            );
            let content = Rect { y: body.y + 2, height: body.height.saturating_sub(2), ..body };
            render_content(
                frame,
                content,
                state,
                hits,
                if artwork_visible { artwork.as_deref_mut() } else { None },
            );
        }
        LayoutMode::Standard => {
            let columns = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Length(18), Constraint::Min(40)])
                .split(body);
            render_sidebar(frame, columns[0], state, hits);
            render_content(
                frame,
                columns[1],
                state,
                hits,
                if artwork_visible { artwork.as_deref_mut() } else { None },
            );
        }
        LayoutMode::Wide => {
            let queue_width = if state.queue_visible() { 34 } else { 0 };
            let columns = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Length(18),
                    Constraint::Min(50),
                    Constraint::Length(queue_width),
                ])
                .split(body);
            render_sidebar(frame, columns[0], state, hits);
            render_content(
                frame,
                columns[1],
                state,
                hits,
                if artwork_visible { artwork.as_deref_mut() } else { None },
            );
            if state.queue_visible() {
                render_queue(frame, columns[2], state, hits);
            }
        }
        LayoutMode::Resize => {}
    }
    render_player(frame, vertical[1], state, hits, if artwork_visible { artwork } else { None });
    render_footer(frame, vertical[2], state, hits);
    render_overlay(frame, state, hits);
}

fn render_resize(frame: &mut Frame<'_>, area: Rect) {
    let text = format!(
        "Mellowdeck needs at least {MIN_WIDTH}×{MIN_HEIGHT}\nCurrent: {}×{}\n\nPress x or Ctrl-C to quit",
        area.width, area.height
    );
    frame.render_widget(
        Paragraph::new(text)
            .alignment(Alignment::Center)
            .style(Style::default().fg(PEACH))
            .wrap(Wrap { trim: true }),
        centered(60, 8, area),
    );
}

fn focus_block(title: impl Into<Line<'static>>, focused: bool) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(if focused { LAVENDER } else { MUTED }))
        .style(Style::default().bg(INK_SOFT))
}

fn render_sidebar(frame: &mut Frame<'_>, area: Rect, state: &AppState, hits: &mut HitMap) {
    hits.sidebar = Some(area);
    let items = AppState::NAVIGATION
        .iter()
        .enumerate()
        .map(|(index, route)| {
            let active = same_root(route, &state.page.route);
            let selected = state.sidebar.selected == index;
            let marker = if active {
                "●"
            } else if selected {
                "›"
            } else {
                " "
            };
            let style = if active {
                Style::default().fg(PEACH).add_modifier(Modifier::BOLD)
            } else if selected && state.focus == FocusRegion::Sidebar {
                Style::default().fg(LAVENDER)
            } else {
                Style::default().fg(Color::White)
            };
            hits.add(
                Rect {
                    x: area.x + 1,
                    y: area.y + 1 + u16::try_from(index).unwrap_or(u16::MAX),
                    width: area.width.saturating_sub(2),
                    height: 1,
                },
                HitTarget::Sidebar(index),
            );
            ListItem::new(Line::from(Span::styled(format!(" {marker} {}", route.label()), style)))
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        List::new(items).block(focus_block(" Navigation ", state.focus == FocusRegion::Sidebar)),
        area,
    );
}

fn same_root(left: &Route, right: &Route) -> bool {
    std::mem::discriminant(left) == std::mem::discriminant(right)
}

#[allow(clippy::too_many_lines)]
fn render_content(
    frame: &mut Frame<'_>,
    area: Rect,
    state: &mut AppState,
    hits: &mut HitMap,
    artwork: Option<&mut ArtworkManager>,
) {
    hits.content = Some(area);
    let has_detail = matches!(
        state.page.route,
        Route::Album { .. } | Route::Playlist { .. } | Route::Artist { .. }
    );
    let header_height = if has_detail {
        7
    } else if matches!(state.page.route, Route::Search | Route::Library) {
        4
    } else {
        3
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(header_height), Constraint::Min(2)])
        .split(area);
    render_content_header(frame, chunks[0], state, hits);
    if let Some(artwork) = artwork {
        let selected_art = state.page.selected_item().and_then(|item| item.artwork_url.as_deref());
        let (placement, url, placeholder) = if has_detail {
            (
                ArtworkPlacement::Header,
                state.page.artwork_url.as_deref().or(selected_art),
                match state.page.route {
                    Route::Artist { .. } => "ARTIST",
                    Route::Playlist { .. } => "PLAYLIST",
                    _ => "ALBUM",
                },
            )
        } else {
            (ArtworkPlacement::Preview, selected_art, "ART")
        };
        let art_area = Rect {
            x: chunks[0].x + 1,
            y: chunks[0].y + 1,
            width: if has_detail { 10 } else { 6 },
            height: chunks[0].height.saturating_sub(2),
        };
        artwork.render(frame, art_area, placement, url, placeholder);
    }
    let list_area = chunks[1];
    state.content_height = usize::from(list_area.height.saturating_sub(2));
    let filtered = state.page.flattened();
    let rows = filtered
        .iter()
        .map(|item| {
            let section = state
                .page
                .sections
                .iter()
                .find(|section| {
                    section.items.iter().any(|candidate| std::ptr::eq(candidate, *item))
                })
                .map_or("", |section| section.title.as_str());
            let marker = match item.kind {
                EntityKind::Track => "♪",
                EntityKind::Album => "▣",
                EntityKind::Artist => "●",
                EntityKind::Playlist => "≡",
                EntityKind::Device => "◉",
                EntityKind::Action => "›",
                EntityKind::Message => "·",
            };
            let unavailable = if item.available { "" } else { " unavailable" };
            let duration =
                item.duration_ms.map_or_else(String::new, |ms| format!("  {}", clock(ms)));
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!(" {marker} {section} · "),
                    Style::default().fg(if item.available { PEACH } else { MUTED }),
                ),
                Span::styled(
                    item.title.clone(),
                    Style::default().fg(if item.available { Color::White } else { MUTED }),
                ),
                Span::styled(
                    format!(" — {}{duration}{unavailable}", item.subtitle),
                    Style::default().fg(MUTED),
                ),
            ]))
        })
        .collect::<Vec<_>>();
    let visible_rows =
        rows.len().saturating_sub(state.page.cursor.offset).min(state.content_height);
    for visible in 0..visible_rows {
        let absolute = state.page.cursor.offset + visible;
        hits.add(
            Rect {
                x: list_area.x + 1,
                y: list_area.y + 1 + u16::try_from(visible).unwrap_or(u16::MAX),
                width: list_area.width.saturating_sub(2),
                height: 1,
            },
            HitTarget::ContentRow(absolute),
        );
    }
    let title = match &state.page.state {
        LoadState::Loading => format!(" {} · loading… ", state.page.title),
        LoadState::Partial(message) | LoadState::Stale(message) => {
            format!(" {} · {message} ", state.page.title)
        }
        _ => format!(" {} ", state.page.title),
    };
    let mut list_state = ListState::default()
        .with_selected((!rows.is_empty()).then_some(state.page.cursor.selected));
    *list_state.offset_mut() = state.page.cursor.offset;
    frame.render_stateful_widget(
        List::new(rows)
            .highlight_symbol("▸ ")
            .highlight_style(
                Style::default()
                    .bg(Color::Rgb(63, 55, 84))
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )
            .block(focus_block(title, state.focus == FocusRegion::Content)),
        list_area,
        &mut list_state,
    );
    if filtered.is_empty() {
        render_empty_state(frame, list_area, &state.page.state);
    }
}

fn render_content_header(frame: &mut Frame<'_>, area: Rect, state: &AppState, hits: &mut HitMap) {
    let padding = if matches!(
        state.page.route,
        Route::Album { .. } | Route::Playlist { .. } | Route::Artist { .. }
    ) {
        11
    } else {
        7
    };
    let mut lines = vec![Line::from(vec![
        Span::styled(
            format!(" {} ", state.page.route.label()),
            Style::default().fg(PEACH).add_modifier(Modifier::BOLD),
        ),
        Span::styled(state.page.subtitle.clone(), Style::default().fg(MUTED)),
    ])];
    match state.page.route {
        Route::Search => {
            let field = format!(
                " Search: {}{}",
                state.search_query,
                if state.text_entry { "▏" } else { "" }
            );
            lines.push(Line::from(Span::styled(
                field,
                Style::default().fg(if state.text_entry { LAVENDER } else { Color::White }),
            )));
            lines.push(Line::from(
                SearchFilter::ALL
                    .into_iter()
                    .map(|filter| {
                        Span::styled(
                            format!(" {} ", filter.label()),
                            if filter == state.page.search_filter {
                                Style::default().bg(LAVENDER).fg(INK)
                            } else {
                                Style::default().fg(MUTED)
                            },
                        )
                    })
                    .collect::<Vec<_>>(),
            ));
            hits.add(
                Rect { x: area.x, y: area.y + 1, width: area.width, height: 1 },
                HitTarget::SearchField,
            );
        }
        Route::Library => {
            lines.push(Line::from(vec![
                Span::styled(
                    " Albums ",
                    if state.page.library_tab == LibraryTab::Albums {
                        Style::default().bg(LAVENDER).fg(INK)
                    } else {
                        Style::default().fg(MUTED)
                    },
                ),
                Span::styled(
                    " Playlists ",
                    if state.page.library_tab == LibraryTab::Playlists {
                        Style::default().bg(LAVENDER).fg(INK)
                    } else {
                        Style::default().fg(MUTED)
                    },
                ),
                Span::styled(
                    format!("  Filter: {}", state.page.filter),
                    Style::default().fg(MUTED),
                ),
            ]));
        }
        Route::Album { .. } | Route::Playlist { .. } | Route::Artist { .. } => {
            let icon = match state.page.route {
                Route::Artist { .. } => "  ◉  ",
                Route::Playlist { .. } => "  ≡  ",
                _ => "  ▣  ",
            };
            lines.push(Line::from(vec![
                Span::styled(icon, Style::default().fg(LAVENDER).add_modifier(Modifier::BOLD)),
                Span::styled(&state.page.title, Style::default().add_modifier(Modifier::BOLD)),
            ]));
            lines.push(Line::from(Span::styled(&state.page.subtitle, Style::default().fg(MUTED))));
            lines.push(Line::from(Span::styled(
                " Enter plays/opens · a queues · i inspects · right-click actions ",
                Style::default().fg(SAGE),
            )));
        }
        _ => {}
    }
    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: true }).block(
            Block::default()
                .borders(Borders::BOTTOM)
                .padding(Padding::left(padding))
                .style(Style::default().bg(INK)),
        ),
        area,
    );
}

fn render_empty_state(frame: &mut Frame<'_>, area: Rect, state: &LoadState) {
    let text = match state {
        LoadState::Loading => "Loading…",
        LoadState::Empty(message)
        | LoadState::Failed(message)
        | LoadState::Restricted(message)
        | LoadState::Stale(message)
        | LoadState::Partial(message) => message,
        LoadState::Ready => "Nothing here yet",
    };
    frame.render_widget(
        Paragraph::new(text)
            .alignment(Alignment::Center)
            .style(Style::default().fg(match state {
                LoadState::Failed(_) | LoadState::Restricted(_) => PEACH,
                _ => MUTED,
            }))
            .wrap(Wrap { trim: true }),
        area.inner(ratatui::layout::Margin { horizontal: 2, vertical: 2 }),
    );
}

fn render_queue(frame: &mut Frame<'_>, area: Rect, state: &AppState, hits: &mut HitMap) {
    hits.queue = Some(area);
    let mut rows = Vec::new();
    rows.push(ListItem::new(Line::from(Span::styled(
        " Now playing",
        Style::default().fg(SAGE).add_modifier(Modifier::BOLD),
    ))));
    if let Some(now) = &state.queue_now {
        rows.push(ListItem::new(format!(" ♪ {} — {}", now.title, now.subtitle)));
    } else {
        rows.push(ListItem::new(Span::styled(" Nothing playing", Style::default().fg(MUTED))));
    }
    rows.push(ListItem::new(Line::from(Span::styled(
        " Up next",
        Style::default().fg(PEACH).add_modifier(Modifier::BOLD),
    ))));
    for (index, item) in state.queue_upcoming.iter().enumerate() {
        rows.push(ListItem::new(format!(" {}. {} — {}", index + 1, item.title, item.subtitle)));
        hits.add(
            Rect {
                x: area.x + 1,
                y: area.y + 4 + u16::try_from(index).unwrap_or(u16::MAX),
                width: area.width.saturating_sub(2),
                height: 1,
            },
            HitTarget::QueueRow(index),
        );
    }
    frame.render_widget(
        List::new(rows).block(focus_block(" Queue ", state.focus == FocusRegion::Queue)),
        area,
    );
}

fn render_player(
    frame: &mut Frame<'_>,
    area: Rect,
    state: &AppState,
    hits: &mut HitMap,
    artwork: Option<&mut ArtworkManager>,
) {
    let progress = state.playback.progress_at(Instant::now());
    let status = if state.playback.playing { "❚❚" } else { "▶" };
    let title =
        if state.playback.title.is_empty() { "Nothing playing" } else { &state.playback.title };
    let album = if state.playback.album.is_empty() {
        String::new()
    } else {
        format!(" · {}", state.playback.album)
    };
    let inner = Block::default().borders(Borders::ALL).inner(area);
    frame.render_widget(focus_block(" Player ", state.focus == FocusRegion::Player), area);
    let cells = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Min(12),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(5),
            Constraint::Min(10),
            Constraint::Length(5),
            Constraint::Length(8),
        ])
        .split(Rect { height: 1, ..inner });
    let metadata = Line::from(vec![
        Span::styled(title, Style::default().add_modifier(Modifier::BOLD)),
        Span::styled(format!(" · {}{album}", state.playback.artist), Style::default().fg(MUTED)),
    ]);
    frame.render_widget(Paragraph::new(metadata).wrap(Wrap { trim: true }), cells[2]);
    // Directional transport buttons are deliberately high-contrast terminal "circles".
    frame.render_widget(
        Paragraph::new(Span::styled(
            " ◀ ",
            Style::default().fg(INK).bg(PEACH).add_modifier(Modifier::BOLD),
        )),
        cells[3],
    );
    frame.render_widget(
        Paragraph::new(Span::styled(
            format!("{status:^3}"),
            Style::default().fg(INK).bg(SAGE).add_modifier(Modifier::BOLD),
        )),
        cells[4],
    );
    frame.render_widget(
        Paragraph::new(Span::styled(
            " ▶ ",
            Style::default().fg(INK).bg(PEACH).add_modifier(Modifier::BOLD),
        )),
        cells[5],
    );
    frame.render_widget(
        Paragraph::new(clock(progress)).style(Style::default().fg(LAVENDER)),
        cells[6],
    );
    let bar_width = usize::from(cells[7].width);
    let filled = if state.playback.duration_ms == 0 {
        0
    } else {
        usize::try_from(progress.saturating_mul(bar_width as u64) / state.playback.duration_ms)
            .unwrap_or_default()
            .min(bar_width)
    };
    frame.render_widget(
        Paragraph::new(format!("{}{}", "━".repeat(filled), "─".repeat(bar_width - filled)))
            .style(Style::default().fg(LAVENDER)),
        cells[7],
    );
    frame.render_widget(
        Paragraph::new(clock(state.playback.duration_ms)).style(Style::default().fg(LAVENDER)),
        cells[8],
    );
    frame.render_widget(
        Paragraph::new(format!("Vol {}%", state.playback.volume))
            .style(Style::default().fg(LAVENDER)),
        cells[9],
    );
    if inner.height > 1 {
        frame.render_widget(
            Paragraph::new(format!(
                "Device: {}   Shuffle: {}   Repeat: {}",
                state.playback.device_name.as_deref().unwrap_or("choose with d"),
                if state.playback.shuffle { "on" } else { "off" },
                ["off", "context", "track"][usize::from(state.playback.repeat)]
            ))
            .style(Style::default().fg(MUTED))
            .wrap(Wrap { trim: true }),
            Rect { y: inner.y + 1, height: inner.height.saturating_sub(1), ..inner },
        );
    }
    if let Some(artwork) = artwork {
        artwork.render(
            frame,
            cells[0],
            ArtworkPlacement::Player,
            state.playback.artwork_url.as_deref(),
            "♪",
        );
    } else {
        frame.render_widget(
            Paragraph::new("♪").centered().style(Style::default().fg(MUTED)),
            cells[0],
        );
    }
    hits.add(cells[3], HitTarget::PlayerPrevious);
    hits.add(cells[4], HitTarget::PlayerToggle);
    hits.add(cells[5], HitTarget::PlayerNext);
    hits.add(cells[7], HitTarget::PlayerProgress);
    hits.add(cells[9], HitTarget::PlayerVolume);
}
fn render_footer(frame: &mut Frame<'_>, area: Rect, state: &AppState, hits: &mut HitMap) {
    let shortcuts = match state.focus {
        FocusRegion::Sidebar => "↑↓ route · Enter open · Tab focus · ? help · x quit",
        FocusRegion::Queue => "↑↓ queue · Enter play now · a append · Tab focus · q full queue",
        FocusRegion::Player => {
            "Space play/pause · [ ] skip · Shift+←→ seek · -/+ volume · s shuffle · r repeat"
        }
        FocusRegion::Content => {
            "↑↓ select · Enter open/play · ←→ tabs · / search · f filter · a queue · i inspect · ? help"
        }
    };
    let notice = state.notice.as_ref().map_or(shortcuts, |notice| notice.text.as_str());
    let color = state.notice.as_ref().map_or(MUTED, |notice| match notice.kind {
        NoticeKind::Success => SAGE,
        NoticeKind::Error => PEACH,
        NoticeKind::Pending => LAVENDER,
        NoticeKind::Info => MUTED,
    });
    frame.render_widget(
        Paragraph::new(notice)
            .style(Style::default().fg(color))
            .block(Block::default().borders(Borders::TOP)),
        area,
    );
    hits.add(area, HitTarget::Help);
}

#[allow(clippy::too_many_lines)]
fn render_overlay(frame: &mut Frame<'_>, state: &AppState, hits: &mut HitMap) {
    let Some(overlay) = &state.overlay else {
        return;
    };
    let area = match overlay {
        Overlay::Inspector => centered(76, 16, frame.area()),
        Overlay::Help { .. } => centered(82, 20, frame.area()),
        _ => centered(46, 13, frame.area()),
    };
    frame.render_widget(Clear, area);
    match overlay {
        Overlay::Menu => {
            let rows = AppState::NAVIGATION.iter().enumerate().map(|(index, route)| {
                hits.add(
                    Rect {
                        x: area.x + 1,
                        y: area.y + 1 + u16::try_from(index).unwrap_or(u16::MAX),
                        width: area.width.saturating_sub(2),
                        height: 1,
                    },
                    HitTarget::Sidebar(index),
                );
                ListItem::new(format!(
                    " {} {}",
                    if index == state.sidebar.selected { "›" } else { " " },
                    route.label()
                ))
            });
            frame.render_widget(
                List::new(rows).block(focus_block(" Menu · Enter to open · Esc to close ", true)),
                area,
            );
        }
        Overlay::Help { query, .. } => {
            let sections = [
                (
                    "Navigation",
                    "Tab/Shift+Tab focus · ↑↓ or j/k select · PgUp/PgDn · Home/End · Enter activate · Esc back",
                ),
                (
                    "Playback",
                    "Space play/pause · [ ] previous/next · Shift+←/→ seek · -/+ volume · s shuffle · r repeat",
                ),
                (
                    "Browse",
                    "/ search · ←/→ filters/tabs · a add to queue · q queue · d devices · i inspector",
                ),
                (
                    "Mouse",
                    "Click selects · double-click activates · right-click actions · wheel scrolls hovered pane",
                ),
                ("Session", "? help · x or Ctrl-C quit"),
            ];
            let needle = query.to_lowercase();
            let lines = sections
                .into_iter()
                .filter(|(name, body)| {
                    needle.is_empty()
                        || name.to_lowercase().contains(&needle)
                        || body.to_lowercase().contains(&needle)
                })
                .flat_map(|(name, body)| {
                    [
                        Line::from(Span::styled(
                            name,
                            Style::default().fg(PEACH).add_modifier(Modifier::BOLD),
                        )),
                        Line::from(body),
                        Line::from(""),
                    ]
                })
                .collect::<Vec<_>>();
            frame.render_widget(
                Paragraph::new(lines)
                    .wrap(Wrap { trim: true })
                    .block(focus_block(format!(" Help · filter: {query} · Esc closes "), true)),
                area,
            );
        }
        Overlay::Inspector => {
            let text = state.selected_item().map_or_else(|| "No item selected".into(), |item| format!("{}\n{}\n\nType: {:?}\nURI: {}\nArtists: {}\nAlbum: {}\nDuration: {}\nAvailability: {}\n\n{}", item.title, item.subtitle, item.kind, item.uri.as_deref().unwrap_or("—"), item.artists.iter().map(|artist| artist.0.as_str()).collect::<Vec<_>>().join(", "), item.album.as_ref().map_or("—", |album| album.0.as_str()), item.duration_ms.map_or_else(|| "—".into(), clock), if item.available { "available" } else { "unavailable" }, item.metadata));
            frame.render_widget(
                Paragraph::new(text)
                    .wrap(Wrap { trim: true })
                    .block(focus_block(" Inspector · Esc closes ", true)),
                area,
            );
        }
        Overlay::Actions { selected } => {
            let actions = [
                "Play",
                "Add to queue",
                "Open album",
                "Choose artist",
                "Open in Spotify",
                "Inspect",
            ];
            let rows = actions.into_iter().enumerate().map(|(index, action)| {
                hits.add(
                    Rect {
                        x: area.x + 1,
                        y: area.y + 1 + u16::try_from(index).unwrap_or(u16::MAX),
                        width: area.width.saturating_sub(2),
                        height: 1,
                    },
                    HitTarget::ActionRow(index),
                );
                ListItem::new(format!(" {} {action}", if index == *selected { "›" } else { " " }))
            });
            frame.render_widget(
                List::new(rows).block(focus_block(" Actions · Esc closes ", true)),
                area,
            );
        }
        Overlay::DevicePicker => frame.render_widget(
            Paragraph::new(
                "No active device. Open Spotify on a device, then choose it from Devices.",
            )
            .wrap(Wrap { trim: true })
            .block(focus_block(" Choose a device ", true)),
            area,
        ),
    }
}

fn centered(width_percent: u16, height: u16, area: Rect) -> Rect {
    let width = area
        .width
        .saturating_mul(width_percent)
        .checked_div(100)
        .unwrap_or(area.width)
        .max(20)
        .min(area.width);
    let height = height.min(area.height);
    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    }
}

fn clock(milliseconds: u64) -> String {
    let seconds = milliseconds / 1_000;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    fn draw(width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = AppState::default();
        state.page.state = LoadState::Empty("No albums saved".into());
        let mut hits = HitMap::default();
        terminal.draw(|frame| render(frame, &mut state, &mut hits)).unwrap();
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect::<String>()
    }

    #[test]
    fn supported_sizes_render_without_panicking() {
        for (width, height) in [(80, 24), (120, 35), (160, 45)] {
            let output = draw(width, height);
            assert!(
                output.contains("Mellowdeck")
                    || output.contains("Navigation")
                    || output.contains("Menu")
            );
        }
    }

    #[test]
    fn undersized_terminal_keeps_quit_instruction() {
        assert!(draw(79, 20).contains("Ctrl-C"));
    }

    #[test]
    fn wide_layout_exposes_queue_hit_region() {
        let backend = TestBackend::new(160, 45);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = AppState::default();
        let mut hits = HitMap::default();
        terminal.draw(|frame| render(frame, &mut state, &mut hits)).unwrap();
        assert!(hits.queue.is_some());
    }

    #[test]
    fn long_unicode_and_error_state_render() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = AppState::default();
        state.page.title = "夢の中で café com açúcar — очень длинное имя".into();
        state.page.state = LoadState::Failed("Network unavailable".into());
        let mut hits = HitMap::default();
        terminal.draw(|frame| render(frame, &mut state, &mut hits)).unwrap();
        let output = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect::<String>();
        assert!(output.contains("Network unavailable"));
    }
}
