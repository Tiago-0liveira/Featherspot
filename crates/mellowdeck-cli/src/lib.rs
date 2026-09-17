#![forbid(unsafe_code)]

use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// The persistent screens in the keyboard player.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum View {
    #[default]
    Home,
    Search,
    Library,
    Detail,
    Queue,
    Devices,
    Settings,
    Help,
}

/// Which persistent pane receives navigation keys. Playback shortcuts always work.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PaneFocus {
    Playback,
    #[default]
    Navigation,
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CliState {
    pub view: View,
    pub focus: PaneFocus,
    pub back: Vec<View>,
    pub selected: usize,
    pub query: String,
    pub searching: bool,
    pub playing: bool,
    /// Last server/SDK state; animation never advances on an optimistic key press alone.
    pub authoritative_playing: bool,
    pub local_device_id: Option<String>,
    pub volume: u8,
    pub shuffle: bool,
    pub repeat: u8,
    pub progress_ms: u64,
    pub duration_ms: u64,
    pub notice: Option<String>,
    pub quit: bool,
    /// Frame selected for the terminal record. It only advances with authoritative playback.
    pub vinyl_frame: u8,
    last_refresh: Option<Instant>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Intent {
    None,
    Refresh,
    TogglePlayback,
    Previous,
    Next,
    Seek(i64),
    Volume(u8),
    ToggleShuffle,
    Repeat(u8),
    Open,
    AddToQueue,
    Quit,
}

impl CliState {
    pub fn toggle_focus(&mut self) {
        self.focus = match self.focus {
            PaneFocus::Playback => PaneFocus::Navigation,
            PaneFocus::Navigation => PaneFocus::Playback,
        };
    }

    pub fn advance_vinyl(&mut self, authoritative_playing: bool) {
        if authoritative_playing {
            self.vinyl_frame = (self.vinyl_frame + 1) % 4;
        }
    }

    /// Returns a smooth display position without treating a local prediction as server truth.
    pub fn interpolated_progress_ms(&self, now: Instant) -> u64 {
        if !self.playing {
            return self.progress_ms.min(self.duration_ms);
        }
        let elapsed = self.last_refresh.map_or(0, |seen| {
            u64::try_from(now.saturating_duration_since(seen).as_millis()).unwrap_or(u64::MAX)
        });
        self.progress_ms.saturating_add(elapsed).min(self.duration_ms)
    }

    pub fn observe_playback(
        &mut self,
        playing: bool,
        progress_ms: u64,
        duration_ms: u64,
        now: Instant,
    ) {
        self.playing = playing;
        self.authoritative_playing = playing;
        self.progress_ms = progress_ms.min(duration_ms);
        self.duration_ms = duration_ms;
        self.last_refresh = Some(now);
    }
    pub fn navigate(&mut self, view: View) {
        if self.view != view {
            self.back.push(self.view);
            self.view = view;
            self.selected = 0;
        }
    }

    pub fn go_back(&mut self) {
        if let Some(view) = self.back.pop() {
            self.view = view;
            self.selected = 0;
        }
    }

    /// Uses a short interval while music moves and a modest idle interval otherwise.
    pub fn refresh_due(&mut self, now: Instant) -> bool {
        let interval = if self.playing { Duration::from_secs(5) } else { Duration::from_secs(15) };
        if self.last_refresh.is_none_or(|last| now.saturating_duration_since(last) >= interval) {
            self.last_refresh = Some(now);
            true
        } else {
            false
        }
    }

    pub fn key(&mut self, key: KeyEvent, items: usize) -> Intent {
        if self.searching {
            match key.code {
                KeyCode::Esc => {
                    self.searching = false;
                    return Intent::None;
                }
                KeyCode::Backspace => {
                    self.query.pop();
                    return Intent::None;
                }
                KeyCode::Enter => return Intent::Open,
                KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.query.push(ch);
                    return Intent::None;
                }
                _ => {}
            }
        }
        let intent = match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => Intent::Quit,
            KeyCode::Char('x') => Intent::Quit,
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected = self.selected.saturating_sub(1);
                Intent::None
            }
            KeyCode::Tab => {
                self.toggle_focus();
                Intent::None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.selected = self.selected.saturating_add(1).min(items.saturating_sub(1));
                Intent::None
            }
            KeyCode::Enter => Intent::Open,
            KeyCode::Esc | KeyCode::Backspace => {
                self.go_back();
                Intent::None
            }
            KeyCode::Char('/') => {
                self.navigate(View::Search);
                self.searching = true;
                Intent::None
            }
            KeyCode::Char('h') => {
                self.navigate(View::Home);
                Intent::Refresh
            }
            KeyCode::Char('l') => {
                self.navigate(View::Library);
                Intent::Refresh
            }
            KeyCode::Char('g') => {
                self.navigate(View::Settings);
                Intent::None
            }
            KeyCode::Char(' ') => {
                self.playing = !self.playing;
                Intent::TogglePlayback
            }
            KeyCode::Char('[') => Intent::Previous,
            KeyCode::Char(']') => Intent::Next,
            KeyCode::Left => Intent::Seek(-5_000),
            KeyCode::Right => Intent::Seek(5_000),
            KeyCode::Char('-') => {
                self.volume = self.volume.saturating_sub(5);
                Intent::Volume(self.volume)
            }
            KeyCode::Char('+' | '=') => {
                self.volume = self.volume.saturating_add(5).min(100);
                Intent::Volume(self.volume)
            }
            KeyCode::Char('s') => {
                self.shuffle = !self.shuffle;
                Intent::ToggleShuffle
            }
            KeyCode::Char('r') => {
                self.repeat = (self.repeat + 1) % 3;
                Intent::Repeat(self.repeat)
            }
            KeyCode::Char('q') => {
                self.navigate(View::Queue);
                Intent::Refresh
            }
            KeyCode::Char('d') => {
                self.navigate(View::Devices);
                Intent::Refresh
            }
            KeyCode::Char('a') => Intent::AddToQueue,
            KeyCode::Char('?') => {
                self.navigate(View::Help);
                Intent::None
            }
            _ => Intent::None,
        };
        if matches!(intent, Intent::Quit) {
            self.quit = true;
        }
        intent
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn navigation_keeps_a_back_stack() {
        let mut state = CliState::default();
        state.navigate(View::Library);
        state.navigate(View::Detail);
        state.go_back();
        assert_eq!(state.view, View::Library);
    }

    #[test]
    fn shortcuts_change_local_state() {
        let mut state = CliState::default();
        assert_eq!(state.key(key(KeyCode::Char(' ')), 0), Intent::TogglePlayback);
        assert!(state.playing);
        state.key(key(KeyCode::Char('+')), 0);
        assert_eq!(state.volume, 5);
    }

    #[test]
    fn refresh_is_adaptive() {
        let mut state = CliState::default();
        let now = Instant::now();
        assert!(state.refresh_due(now));
        assert!(!state.refresh_due(now + Duration::from_secs(14)));
        assert!(state.refresh_due(now + Duration::from_secs(15)));
        state.playing = true;
        assert!(state.refresh_due(now + Duration::from_secs(20)));
    }

    #[test]
    fn search_edits_and_submits_without_leaving_the_view() {
        let mut state = CliState::default();
        state.key(key(KeyCode::Char('/')), 0);
        state.key(key(KeyCode::Char('a')), 0);
        state.key(key(KeyCode::Char('b')), 0);
        state.key(key(KeyCode::Backspace), 0);
        assert_eq!(state.query, "a");
        assert_eq!(state.key(key(KeyCode::Enter), 0), Intent::Open);
        assert_eq!(state.view, View::Search);
    }

    #[test]
    fn focus_and_vinyl_are_explicit_state() {
        let mut state = CliState::default();
        state.key(key(KeyCode::Tab), 0);
        assert_eq!(state.focus, PaneFocus::Playback);
        state.advance_vinyl(false);
        assert_eq!(state.vinyl_frame, 0);
        state.advance_vinyl(true);
        assert_eq!(state.vinyl_frame, 1);
    }

    #[test]
    fn progress_is_interpolated_only_while_authoritatively_playing() {
        let now = Instant::now();
        let mut state = CliState::default();
        state.observe_playback(true, 900, 1_000, now);
        assert_eq!(state.interpolated_progress_ms(now + Duration::from_secs(1)), 1_000);
        state.observe_playback(false, 900, 1_000, now);
        assert_eq!(state.interpolated_progress_ms(now + Duration::from_secs(1)), 900);
    }
}
