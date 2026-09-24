use std::collections::BTreeMap;
use std::fmt;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::state::Route;

/// Identifiers for all keyboard-driven actions in lspotify-cli.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub enum ShortcutAction {
    Quit,
    Home,
    Search,
    Library,
    Settings,
    Queue,
    Devices,
    FocusNext,
    FocusPrevious,
    MoveUp,
    MoveDown,
    PageUp,
    PageDown,
    CursorHome,
    CursorEnd,
    PreviousTab,
    NextTab,
    Activate,
    Cancel,
    StartSearch,
    StartFilter,
    TogglePlayback,
    PreviousTrack,
    NextTrack,
    SeekBackward,
    SeekForward,
    VolumeDown,
    VolumeUp,
    Shuffle,
    Repeat,
    AddToQueue,
    ToggleSaved,
    OpenActions,
    DetailPlay,
    DetailShuffle,
    DetailOpenSpotify,
    ToggleHelp,
    Inspect,
}

impl ShortcutAction {
    pub const ALL: [Self; 38] = [
        Self::Quit,
        Self::Home,
        Self::Search,
        Self::Library,
        Self::Settings,
        Self::Queue,
        Self::Devices,
        Self::FocusNext,
        Self::FocusPrevious,
        Self::MoveUp,
        Self::MoveDown,
        Self::PageUp,
        Self::PageDown,
        Self::CursorHome,
        Self::CursorEnd,
        Self::PreviousTab,
        Self::NextTab,
        Self::Activate,
        Self::Cancel,
        Self::StartSearch,
        Self::StartFilter,
        Self::TogglePlayback,
        Self::PreviousTrack,
        Self::NextTrack,
        Self::SeekBackward,
        Self::SeekForward,
        Self::VolumeDown,
        Self::VolumeUp,
        Self::Shuffle,
        Self::Repeat,
        Self::AddToQueue,
        Self::ToggleSaved,
        Self::OpenActions,
        Self::DetailPlay,
        Self::DetailShuffle,
        Self::DetailOpenSpotify,
        Self::ToggleHelp,
        Self::Inspect,
    ];

    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::Quit => "quit",
            Self::Home => "home",
            Self::Search => "search",
            Self::Library => "library",
            Self::Settings => "settings",
            Self::Queue => "queue",
            Self::Devices => "devices",
            Self::FocusNext => "focus_next",
            Self::FocusPrevious => "focus_previous",
            Self::MoveUp => "move_up",
            Self::MoveDown => "move_down",
            Self::PageUp => "page_up",
            Self::PageDown => "page_down",
            Self::CursorHome => "cursor_home",
            Self::CursorEnd => "cursor_end",
            Self::PreviousTab => "previous_tab",
            Self::NextTab => "next_tab",
            Self::Activate => "activate",
            Self::Cancel => "cancel",
            Self::StartSearch => "start_search",
            Self::StartFilter => "start_filter",
            Self::TogglePlayback => "toggle_playback",
            Self::PreviousTrack => "previous_track",
            Self::NextTrack => "next_track",
            Self::SeekBackward => "seek_backward",
            Self::SeekForward => "seek_forward",
            Self::VolumeDown => "volume_down",
            Self::VolumeUp => "volume_up",
            Self::Shuffle => "shuffle",
            Self::Repeat => "repeat",
            Self::AddToQueue => "add_to_queue",
            Self::ToggleSaved => "toggle_saved",
            Self::OpenActions => "open_actions",
            Self::DetailPlay => "detail_play",
            Self::DetailShuffle => "detail_shuffle",
            Self::DetailOpenSpotify => "detail_open_spotify",
            Self::ToggleHelp => "toggle_help",
            Self::Inspect => "inspect",
        }
    }

    #[must_use]
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|action| action.id() == id)
    }

    #[must_use]
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Quit => "Quit",
            Self::Home => "Home",
            Self::Search => "Search",
            Self::Library => "Library",
            Self::Settings => "Settings",
            Self::Queue => "Queue",
            Self::Devices => "Devices",
            Self::FocusNext => "Focus Next",
            Self::FocusPrevious => "Focus Previous",
            Self::MoveUp => "Move Up",
            Self::MoveDown => "Move Down",
            Self::PageUp => "Page Up",
            Self::PageDown => "Page Down",
            Self::CursorHome => "Go to Start",
            Self::CursorEnd => "Go to End",
            Self::PreviousTab => "Previous Tab / Filter",
            Self::NextTab => "Next Tab / Filter",
            Self::Activate => "Activate / Open",
            Self::Cancel => "Back / Cancel",
            Self::StartSearch => "Start Search",
            Self::StartFilter => "Filter Library",
            Self::TogglePlayback => "Play / Pause",
            Self::PreviousTrack => "Previous Track",
            Self::NextTrack => "Next Track",
            Self::SeekBackward => "Seek Backward",
            Self::SeekForward => "Seek Forward",
            Self::VolumeDown => "Volume Down",
            Self::VolumeUp => "Volume Up",
            Self::Shuffle => "Toggle Shuffle",
            Self::Repeat => "Cycle Repeat",
            Self::AddToQueue => "Add to Queue",
            Self::ToggleSaved => "Like / Unlike",
            Self::OpenActions => "Actions Menu",
            Self::DetailPlay => "Play Collection",
            Self::DetailShuffle => "Shuffle Collection",
            Self::DetailOpenSpotify => "Open in Spotify",
            Self::ToggleHelp => "Help",
            Self::Inspect => "Inspector",
        }
    }

    #[must_use]
    pub const fn scope(self) -> Scope {
        match self {
            Self::Quit => Scope::Global,
            Self::StartFilter => Scope::Library,
            Self::DetailPlay | Self::DetailShuffle | Self::DetailOpenSpotify => Scope::Detail,
            _ => Scope::General,
        }
    }

    #[must_use]
    pub const fn is_global(self) -> bool {
        matches!(self, Self::Quit)
    }

    #[allow(clippy::unused_self)]
    #[must_use]
    pub const fn is_configurable(self) -> bool {
        true
    }

    #[must_use]
    pub fn default_bindings(self) -> Vec<KeyBinding> {
        match self {
            Self::Quit => vec![
                KeyBinding::char('x'),
                KeyBinding::char_with_shift('Q'),
                KeyBinding::char_with_ctrl('c'),
                KeyBinding::char_with_ctrl('q'),
            ],
            Self::Home => vec![KeyBinding::char('1')],
            Self::Search => vec![KeyBinding::char('2')],
            Self::Library => vec![KeyBinding::char('3')],
            Self::Settings => vec![KeyBinding::char('4')],
            Self::Queue => vec![KeyBinding::char('5'), KeyBinding::char('q')],
            Self::Devices => vec![KeyBinding::char('d')],
            Self::FocusNext => vec![KeyBinding::new(KeyCode::Tab, KeyModifiers::NONE)],
            Self::FocusPrevious => vec![KeyBinding::new(KeyCode::Tab, KeyModifiers::SHIFT)],
            Self::MoveUp => {
                vec![KeyBinding::new(KeyCode::Up, KeyModifiers::NONE), KeyBinding::char('k')]
            }
            Self::MoveDown => {
                vec![KeyBinding::new(KeyCode::Down, KeyModifiers::NONE), KeyBinding::char('j')]
            }
            Self::PageUp => vec![KeyBinding::new(KeyCode::PageUp, KeyModifiers::NONE)],
            Self::PageDown => vec![KeyBinding::new(KeyCode::PageDown, KeyModifiers::NONE)],
            Self::CursorHome => vec![KeyBinding::new(KeyCode::Home, KeyModifiers::NONE)],
            Self::CursorEnd => vec![KeyBinding::new(KeyCode::End, KeyModifiers::NONE)],
            Self::PreviousTab => vec![KeyBinding::new(KeyCode::Left, KeyModifiers::NONE)],
            Self::NextTab => vec![KeyBinding::new(KeyCode::Right, KeyModifiers::NONE)],
            Self::Activate => vec![KeyBinding::new(KeyCode::Enter, KeyModifiers::NONE)],
            Self::Cancel => vec![
                KeyBinding::new(KeyCode::Esc, KeyModifiers::NONE),
                KeyBinding::new(KeyCode::Backspace, KeyModifiers::NONE),
            ],
            Self::StartSearch => vec![KeyBinding::char('/')],
            Self::StartFilter => vec![KeyBinding::char('f')],
            Self::TogglePlayback => vec![KeyBinding::char(' ')],
            Self::PreviousTrack => vec![KeyBinding::char('[')],
            Self::NextTrack => vec![KeyBinding::char(']')],
            Self::SeekBackward => vec![KeyBinding::new(KeyCode::Left, KeyModifiers::SHIFT)],
            Self::SeekForward => vec![KeyBinding::new(KeyCode::Right, KeyModifiers::SHIFT)],
            Self::VolumeDown => vec![KeyBinding::char('-')],
            Self::VolumeUp => vec![KeyBinding::char('+')],
            Self::Shuffle => vec![KeyBinding::char('s')],
            Self::Repeat => vec![KeyBinding::char('r')],
            Self::AddToQueue => vec![KeyBinding::char('a')],
            Self::ToggleSaved => vec![KeyBinding::char('l')],
            Self::OpenActions => vec![KeyBinding::char('m')],
            Self::DetailPlay => vec![KeyBinding::char('p')],
            Self::DetailShuffle => vec![KeyBinding::char_with_shift('S')],
            Self::DetailOpenSpotify => vec![KeyBinding::char('o')],
            Self::ToggleHelp => vec![KeyBinding::char('?')],
            Self::Inspect => vec![KeyBinding::char('i')],
        }
    }
}

/// Scope in which a shortcut is active.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Scope {
    Global,
    General,
    Library,
    Detail,
}

impl Scope {
    #[must_use]
    pub fn conflicts_with(self, other: Self) -> bool {
        matches!(
            (self, other),
            (Self::Global, _)
                | (_, Self::Global)
                | (Self::General, Self::General | Self::Library | Self::Detail)
                | (Self::Library | Self::Detail, Self::General)
                | (Self::Library, Self::Library)
                | (Self::Detail, Self::Detail)
        )
    }
}

impl fmt::Display for Scope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Global => write!(f, "Global"),
            Self::General => write!(f, "General"),
            Self::Library => write!(f, "Library"),
            Self::Detail => write!(f, "Detail"),
        }
    }
}

/// Representation of a key binding that matches crossterm `KeyEvent`s.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct KeyBinding {
    pub code: KeyCode,
    pub modifiers: KeyModifiers,
}

impl KeyBinding {
    #[must_use]
    pub const fn new(code: KeyCode, modifiers: KeyModifiers) -> Self {
        Self { code, modifiers }
    }

    #[must_use]
    pub const fn char(ch: char) -> Self {
        Self { code: KeyCode::Char(ch), modifiers: KeyModifiers::NONE }
    }

    #[must_use]
    pub const fn char_with_ctrl(ch: char) -> Self {
        Self { code: KeyCode::Char(ch), modifiers: KeyModifiers::CONTROL }
    }

    #[must_use]
    pub const fn char_with_shift(ch: char) -> Self {
        Self { code: KeyCode::Char(ch), modifiers: KeyModifiers::SHIFT }
    }

    /// Creates a `KeyBinding` from a crossterm `KeyEvent`.
    #[must_use]
    pub fn from_event(event: &KeyEvent) -> Self {
        let mut modifiers =
            event.modifiers & (KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SHIFT);
        let code = match event.code {
            KeyCode::BackTab => {
                modifiers |= KeyModifiers::SHIFT;
                KeyCode::Tab
            }
            KeyCode::Char(ch) => {
                if modifiers.contains(KeyModifiers::CONTROL) {
                    KeyCode::Char(ch.to_ascii_lowercase())
                } else if ch.is_ascii_uppercase() {
                    modifiers |= KeyModifiers::SHIFT;
                    KeyCode::Char(ch)
                } else {
                    KeyCode::Char(ch)
                }
            }
            other => other,
        };
        Self { code, modifiers }
    }

    /// Parses a canonical string representation such as `"x"`, `"Shift+Q"`, `"Ctrl+C"`, `"Space"`.
    ///
    /// # Errors
    ///
    /// Returns [`ParseKeyError`] if the input is empty or contains an unrecognized key name.
    pub fn parse(input: &str) -> Result<Self, ParseKeyError> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err(ParseKeyError::Empty);
        }

        // Special case: "+" or symbols ending with "+"
        if trimmed == "+" {
            return Ok(Self::char('+'));
        }

        let mut modifiers = KeyModifiers::NONE;
        let mut remainder = trimmed;

        loop {
            if let Some(rest) =
                remainder.strip_prefix("Ctrl+").or_else(|| remainder.strip_prefix("ctrl+"))
            {
                modifiers |= KeyModifiers::CONTROL;
                remainder = rest;
            } else if let Some(rest) =
                remainder.strip_prefix("Alt+").or_else(|| remainder.strip_prefix("alt+"))
            {
                modifiers |= KeyModifiers::ALT;
                remainder = rest;
            } else if let Some(rest) =
                remainder.strip_prefix("Shift+").or_else(|| remainder.strip_prefix("shift+"))
            {
                modifiers |= KeyModifiers::SHIFT;
                remainder = rest;
            } else {
                break;
            }
        }

        if remainder.is_empty() {
            return Err(ParseKeyError::MissingKey);
        }

        let code = match remainder {
            "Space" | "space" => KeyCode::Char(' '),
            "Enter" | "enter" => KeyCode::Enter,
            "Esc" | "esc" | "Escape" | "escape" => KeyCode::Esc,
            "Backspace" | "backspace" => KeyCode::Backspace,
            "Tab" | "tab" => KeyCode::Tab,
            "BackTab" | "backtab" => {
                modifiers |= KeyModifiers::SHIFT;
                KeyCode::Tab
            }
            "Up" | "up" => KeyCode::Up,
            "Down" | "down" => KeyCode::Down,
            "Left" | "left" => KeyCode::Left,
            "Right" | "right" => KeyCode::Right,
            "PageUp" | "pageup" | "Page_Up" | "page_up" => KeyCode::PageUp,
            "PageDown" | "pagedown" | "Page_Down" | "page_down" => KeyCode::PageDown,
            "Home" | "home" => KeyCode::Home,
            "End" | "end" => KeyCode::End,
            "Delete" | "delete" | "Del" | "del" => KeyCode::Delete,
            "Insert" | "insert" | "Ins" | "ins" => KeyCode::Insert,
            other => {
                let mut chars = other.chars();
                if let (Some(ch), None) = (chars.next(), chars.next()) {
                    if modifiers.contains(KeyModifiers::CONTROL) {
                        KeyCode::Char(ch.to_ascii_lowercase())
                    } else {
                        if ch.is_ascii_uppercase() {
                            modifiers |= KeyModifiers::SHIFT;
                        }
                        KeyCode::Char(ch)
                    }
                } else {
                    return Err(ParseKeyError::UnknownKey(other.to_string()));
                }
            }
        };

        Ok(Self { code, modifiers })
    }

    /// Serializes to a stable canonical string representation.
    #[must_use]
    pub fn to_canonical(self) -> String {
        let mut prefix = String::new();
        if self.modifiers.contains(KeyModifiers::CONTROL) {
            prefix.push_str("Ctrl+");
        }
        if self.modifiers.contains(KeyModifiers::ALT) {
            prefix.push_str("Alt+");
        }
        let has_shift = self.modifiers.contains(KeyModifiers::SHIFT);

        if let Some(name) = format_named_key(self.code) {
            if has_shift || self.code == KeyCode::BackTab {
                prefix.push_str("Shift+");
            }
            return format!("{prefix}{name}");
        }

        if let KeyCode::Char(ch) = self.code {
            if ch.is_ascii_alphabetic() {
                if has_shift {
                    prefix.push_str("Shift+");
                    format!("{prefix}{}", ch.to_ascii_uppercase())
                } else if self.modifiers.contains(KeyModifiers::CONTROL) {
                    format!("{prefix}{}", ch.to_ascii_uppercase())
                } else {
                    format!("{prefix}{ch}")
                }
            } else {
                if has_shift {
                    prefix.push_str("Shift+");
                }
                format!("{prefix}{ch}")
            }
        } else {
            format!("{prefix}{:?}", self.code)
        }
    }

    /// Formats for human-readable display in Help / hints (e.g. arrows as symbols).
    #[must_use]
    pub fn display_symbol(self) -> String {
        let mut prefix = String::new();
        if self.modifiers.contains(KeyModifiers::CONTROL) {
            prefix.push_str("Ctrl+");
        }
        if self.modifiers.contains(KeyModifiers::ALT) {
            prefix.push_str("Alt+");
        }
        let has_shift = self.modifiers.contains(KeyModifiers::SHIFT);

        match self.code {
            KeyCode::Up => {
                if has_shift {
                    prefix.push_str("Shift+");
                }
                format!("{prefix}↑")
            }
            KeyCode::Down => {
                if has_shift {
                    prefix.push_str("Shift+");
                }
                format!("{prefix}↓")
            }
            KeyCode::Left => {
                if has_shift {
                    prefix.push_str("Shift+");
                }
                format!("{prefix}←")
            }
            KeyCode::Right => {
                if has_shift {
                    prefix.push_str("Shift+");
                }
                format!("{prefix}→")
            }
            KeyCode::PageUp => {
                if has_shift {
                    prefix.push_str("Shift+");
                }
                format!("{prefix}PgUp")
            }
            KeyCode::PageDown => {
                if has_shift {
                    prefix.push_str("Shift+");
                }
                format!("{prefix}PgDn")
            }
            _ => self.to_canonical(),
        }
    }

    /// Checks if a crossterm `KeyEvent` matches this key binding.
    #[must_use]
    pub fn matches(&self, event: &KeyEvent) -> bool {
        let event_ctrl = event.modifiers.contains(KeyModifiers::CONTROL);
        let event_alt = event.modifiers.contains(KeyModifiers::ALT);
        let event_shift = event.modifiers.contains(KeyModifiers::SHIFT);

        let self_ctrl = self.modifiers.contains(KeyModifiers::CONTROL);
        let self_alt = self.modifiers.contains(KeyModifiers::ALT);
        let self_shift = self.modifiers.contains(KeyModifiers::SHIFT);

        if event_ctrl != self_ctrl || event_alt != self_alt {
            return false;
        }

        match (self.code, event.code) {
            (KeyCode::Char(' '), KeyCode::Char(' ')) => !event_shift,
            (KeyCode::Tab, KeyCode::Tab) => event_shift == self_shift,
            (KeyCode::Tab, KeyCode::BackTab) => self_shift,
            (KeyCode::BackTab, KeyCode::BackTab) => true,
            (KeyCode::BackTab, KeyCode::Tab) => event_shift,
            (KeyCode::Char(sc), KeyCode::Char(ec)) => {
                if sc.is_ascii_alphabetic() {
                    if self_shift {
                        (event_shift || ec.is_ascii_uppercase()) && sc.eq_ignore_ascii_case(&ec)
                    } else {
                        !event_shift && ec.is_ascii_lowercase() && sc == ec
                    }
                } else {
                    sc == ec
                }
            }
            (a, b) => a == b && event_shift == self_shift,
        }
    }
}

const fn format_named_key(code: KeyCode) -> Option<&'static str> {
    match code {
        KeyCode::Char(' ') => Some("Space"),
        KeyCode::Enter => Some("Enter"),
        KeyCode::Esc => Some("Esc"),
        KeyCode::Backspace => Some("Backspace"),
        KeyCode::Tab | KeyCode::BackTab => Some("Tab"),
        KeyCode::Up => Some("Up"),
        KeyCode::Down => Some("Down"),
        KeyCode::Left => Some("Left"),
        KeyCode::Right => Some("Right"),
        KeyCode::PageUp => Some("PageUp"),
        KeyCode::PageDown => Some("PageDown"),
        KeyCode::Home => Some("Home"),
        KeyCode::End => Some("End"),
        KeyCode::Delete => Some("Delete"),
        KeyCode::Insert => Some("Insert"),
        _ => None,
    }
}

impl fmt::Display for KeyBinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_canonical())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ParseKeyError {
    Empty,
    MissingKey,
    UnknownKey(String),
}

impl fmt::Display for ParseKeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "empty key string"),
            Self::MissingKey => write!(f, "missing key after modifiers"),
            Self::UnknownKey(key) => write!(f, "unknown key: {key}"),
        }
    }
}

impl std::error::Error for ParseKeyError {}

/// Conflict detected when assigning a key to an action.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Conflict {
    pub key: KeyBinding,
    pub conflicting_action: ShortcutAction,
}

impl fmt::Display for Conflict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "'{}' is already bound to {}",
            self.key.to_canonical(),
            self.conflicting_action.display_name()
        )
    }
}

impl std::error::Error for Conflict {}

/// Central shortcut registry that manages key bindings, defaults, overrides, and matching.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShortcutRegistry {
    bindings: BTreeMap<ShortcutAction, Vec<KeyBinding>>,
}

impl Default for ShortcutRegistry {
    fn default() -> Self {
        let mut bindings = BTreeMap::new();
        for action in ShortcutAction::ALL {
            bindings.insert(action, action.default_bindings());
        }
        Self { bindings }
    }
}

impl ShortcutRegistry {
    #[must_use]
    pub fn get_bindings(&self, action: ShortcutAction) -> &[KeyBinding] {
        self.bindings.get(&action).map_or(&[], Vec::as_slice)
    }

    #[must_use]
    pub fn primary_label(&self, action: ShortcutAction) -> String {
        self.bindings
            .get(&action)
            .and_then(|list| list.first().copied())
            .map_or_else(|| "(none)".to_string(), KeyBinding::display_symbol)
    }

    #[must_use]
    pub fn primary_canonical(&self, action: ShortcutAction) -> String {
        self.bindings
            .get(&action)
            .and_then(|list| list.first().copied())
            .map_or_else(|| "(none)".to_string(), KeyBinding::to_canonical)
    }

    #[must_use]
    pub fn list_label(&self, action: ShortcutAction, separator: &str) -> String {
        let bindings = self.get_bindings(action);
        if bindings.is_empty() {
            "(unbound)".to_string()
        } else {
            bindings
                .iter()
                .copied()
                .map(KeyBinding::to_canonical)
                .collect::<Vec<_>>()
                .join(separator)
        }
    }

    #[must_use]
    pub fn format_quit_hint(&self) -> String {
        let bindings = self.get_bindings(ShortcutAction::Quit);
        if bindings.is_empty() {
            "Ctrl+C".to_string()
        } else {
            bindings.iter().copied().map(KeyBinding::to_canonical).collect::<Vec<_>>().join(" or ")
        }
    }

    /// Checks if a key event matches the global Quit shortcut.
    #[must_use]
    pub fn is_quit(&self, event: &KeyEvent) -> bool {
        self.get_bindings(ShortcutAction::Quit).iter().any(|b| b.matches(event))
    }

    /// Finds any conflicting action that already uses the given key in a conflicting scope.
    #[must_use]
    pub fn find_conflict(
        &self,
        action: ShortcutAction,
        key: &KeyBinding,
    ) -> Option<ShortcutAction> {
        for (&other_action, bindings) in &self.bindings {
            if other_action == action {
                continue;
            }
            if !action.scope().conflicts_with(other_action.scope()) {
                continue;
            }
            if bindings.iter().any(|b| b == key) {
                return Some(other_action);
            }
        }
        None
    }

    /// Attempts to assign `key` to `action`.
    ///
    /// # Errors
    ///
    /// Returns [`Conflict`] if `key` is already bound to another action in a conflicting scope.
    pub fn try_assign(&mut self, action: ShortcutAction, key: KeyBinding) -> Result<(), Conflict> {
        if let Some(conflicting_action) = self.find_conflict(action, &key) {
            return Err(Conflict { key, conflicting_action });
        }
        self.bindings.insert(action, vec![key]);
        Ok(())
    }

    /// Forcefully assigns `key` to `action`, removing it from any conflicting action.
    pub fn force_assign(&mut self, action: ShortcutAction, key: KeyBinding) {
        for (&other_action, bindings) in &mut self.bindings {
            if other_action != action && action.scope().conflicts_with(other_action.scope()) {
                bindings.retain(|b| b != &key);
            }
        }
        self.bindings.insert(action, vec![key]);
    }

    /// Removes all bindings for the given action.
    pub fn remove_binding(&mut self, action: ShortcutAction) {
        self.bindings.insert(action, Vec::new());
    }

    /// Restores a single action to its default bindings.
    pub fn restore_default(&mut self, action: ShortcutAction) {
        self.bindings.insert(action, action.default_bindings());
    }

    /// Restores all actions to their default bindings.
    pub fn restore_all_defaults(&mut self) {
        *self = Self::default();
    }

    /// Converts customized bindings into serializable overrides.
    #[must_use]
    pub fn to_overrides(&self) -> BTreeMap<String, Vec<String>> {
        let mut overrides = BTreeMap::new();
        for &action in &ShortcutAction::ALL {
            let current = self.get_bindings(action);
            let default = action.default_bindings();
            if current != default.as_slice() {
                let list = current.iter().copied().map(KeyBinding::to_canonical).collect();
                overrides.insert(action.id().to_string(), list);
            }
        }
        overrides
    }

    /// Loads overrides from serialized keybinding preferences.
    pub fn load_overrides(&mut self, overrides: &BTreeMap<String, Vec<String>>) {
        for (id, raw_keys) in overrides {
            if let Some(action) = ShortcutAction::from_id(id) {
                let parsed: Vec<KeyBinding> =
                    raw_keys.iter().filter_map(|s| KeyBinding::parse(s).ok()).collect();
                self.bindings.insert(action, parsed);
            }
        }
    }

    /// Resolves an incoming key event to an action based on current route and detail state.
    #[must_use]
    pub fn resolve(
        &self,
        event: &KeyEvent,
        route: &Route,
        in_detail: bool,
    ) -> Option<ShortcutAction> {
        // 1. Contextual actions: Detail page
        if in_detail {
            for action in [
                ShortcutAction::DetailPlay,
                ShortcutAction::DetailShuffle,
                ShortcutAction::DetailOpenSpotify,
            ] {
                if self.get_bindings(action).iter().any(|b| b.matches(event)) {
                    return Some(action);
                }
            }
        }

        // 2. Contextual actions: Library page
        if *route == Route::Library
            && self.get_bindings(ShortcutAction::StartFilter).iter().any(|b| b.matches(event))
        {
            return Some(ShortcutAction::StartFilter);
        }

        // 3. Normal navigation, playback, and general actions
        for &action in &ShortcutAction::ALL {
            if action.scope() == Scope::Detail
                || action.scope() == Scope::Library
                || action.is_global()
            {
                continue;
            }
            if self.get_bindings(action).iter().any(|b| b.matches(event)) {
                return Some(action);
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_canonical_strings() {
        assert_eq!(
            KeyBinding::parse("x").unwrap(),
            KeyBinding::new(KeyCode::Char('x'), KeyModifiers::NONE)
        );
        assert_eq!(
            KeyBinding::parse("Shift+Q").unwrap(),
            KeyBinding::new(KeyCode::Char('Q'), KeyModifiers::SHIFT)
        );
        assert_eq!(
            KeyBinding::parse("Ctrl+C").unwrap(),
            KeyBinding::new(KeyCode::Char('c'), KeyModifiers::CONTROL)
        );
        assert_eq!(
            KeyBinding::parse("Space").unwrap(),
            KeyBinding::new(KeyCode::Char(' '), KeyModifiers::NONE)
        );
        assert_eq!(
            KeyBinding::parse("Left").unwrap(),
            KeyBinding::new(KeyCode::Left, KeyModifiers::NONE)
        );
        assert_eq!(
            KeyBinding::parse("Shift+Right").unwrap(),
            KeyBinding::new(KeyCode::Right, KeyModifiers::SHIFT)
        );
        assert_eq!(
            KeyBinding::parse("+").unwrap(),
            KeyBinding::new(KeyCode::Char('+'), KeyModifiers::NONE)
        );
        assert_eq!(
            KeyBinding::parse("-").unwrap(),
            KeyBinding::new(KeyCode::Char('-'), KeyModifiers::NONE)
        );
    }

    #[test]
    fn to_canonical_round_trip() {
        let keys = ["x", "Shift+Q", "Ctrl+C", "Space", "Left", "Shift+Right", "+", "-"];
        for raw in keys {
            let parsed = KeyBinding::parse(raw).unwrap();
            let canonical = parsed.to_canonical();
            assert_eq!(canonical, raw);
        }
    }

    #[test]
    fn queue_q_does_not_match_quit_shift_q() {
        let registry = ShortcutRegistry::default();
        let event_q = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE);
        assert!(!registry.is_quit(&event_q));
        let resolved = registry.resolve(&event_q, &Route::Home, false);
        assert_eq!(resolved, Some(ShortcutAction::Queue));
    }

    #[test]
    fn quit_keys_match_global_quit() {
        let registry = ShortcutRegistry::default();
        let x = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE);
        let shift_q = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::SHIFT);
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        let ctrl_q = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL);
        assert!(registry.is_quit(&x));
        assert!(registry.is_quit(&shift_q));
        assert!(registry.is_quit(&ctrl_c));
        assert!(registry.is_quit(&ctrl_q));
    }

    #[test]
    fn volume_up_and_down_match_defaults() {
        let registry = ShortcutRegistry::default();
        let plus = KeyEvent::new(KeyCode::Char('+'), KeyModifiers::NONE);
        let minus = KeyEvent::new(KeyCode::Char('-'), KeyModifiers::NONE);
        assert_eq!(registry.resolve(&plus, &Route::Home, false), Some(ShortcutAction::VolumeUp));
        assert_eq!(registry.resolve(&minus, &Route::Home, false), Some(ShortcutAction::VolumeDown));
    }

    #[test]
    fn conflict_rejection_and_reassignment() {
        let mut registry = ShortcutRegistry::default();
        // Space is bound to TogglePlayback
        let space = KeyBinding::parse("Space").unwrap();
        // Attempting to bind Space to NextTrack should fail with conflict
        let err = registry.try_assign(ShortcutAction::NextTrack, space).unwrap_err();
        assert_eq!(err.conflicting_action, ShortcutAction::TogglePlayback);

        // Force assign reassigns Space
        registry.force_assign(ShortcutAction::NextTrack, space);
        assert_eq!(registry.get_bindings(ShortcutAction::NextTrack), &[space]);
        assert!(registry.get_bindings(ShortcutAction::TogglePlayback).is_empty());
    }

    #[test]
    fn default_bindings_reproduce_current_behavior() {
        let registry = ShortcutRegistry::default();
        let home = Route::Home;
        let album = Route::Album { uri: "spotify:album:1".into(), title: "Album".into() };
        let library = Route::Library;

        let key = |c| KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);
        let shift_key = |c| KeyEvent::new(KeyCode::Char(c), KeyModifiers::SHIFT);

        // Playback
        assert_eq!(registry.resolve(&key(' '), &home, false), Some(ShortcutAction::TogglePlayback));
        assert_eq!(registry.resolve(&key('['), &home, false), Some(ShortcutAction::PreviousTrack));
        assert_eq!(registry.resolve(&key(']'), &home, false), Some(ShortcutAction::NextTrack));
        assert_eq!(registry.resolve(&key('-'), &home, false), Some(ShortcutAction::VolumeDown));
        assert_eq!(registry.resolve(&key('+'), &home, false), Some(ShortcutAction::VolumeUp));
        assert_eq!(registry.resolve(&key('s'), &home, false), Some(ShortcutAction::Shuffle));
        assert_eq!(registry.resolve(&key('r'), &home, false), Some(ShortcutAction::Repeat));

        // Navigation
        assert_eq!(registry.resolve(&key('1'), &home, false), Some(ShortcutAction::Home));
        assert_eq!(registry.resolve(&key('2'), &home, false), Some(ShortcutAction::Search));
        assert_eq!(registry.resolve(&key('3'), &home, false), Some(ShortcutAction::Library));
        assert_eq!(registry.resolve(&key('4'), &home, false), Some(ShortcutAction::Settings));
        assert_eq!(registry.resolve(&key('5'), &home, false), Some(ShortcutAction::Queue));
        assert_eq!(registry.resolve(&key('q'), &home, false), Some(ShortcutAction::Queue));
        assert_eq!(registry.resolve(&key('d'), &home, false), Some(ShortcutAction::Devices));

        // General actions
        assert_eq!(registry.resolve(&key('/'), &home, false), Some(ShortcutAction::StartSearch));
        assert_eq!(registry.resolve(&key('a'), &home, false), Some(ShortcutAction::AddToQueue));
        assert_eq!(registry.resolve(&key('l'), &home, false), Some(ShortcutAction::ToggleSaved));
        assert_eq!(registry.resolve(&key('m'), &home, false), Some(ShortcutAction::OpenActions));
        assert_eq!(registry.resolve(&key('?'), &home, false), Some(ShortcutAction::ToggleHelp));
        assert_eq!(registry.resolve(&key('i'), &home, false), Some(ShortcutAction::Inspect));

        // Contextual: Library filter
        assert_eq!(registry.resolve(&key('f'), &library, false), Some(ShortcutAction::StartFilter));
        assert_eq!(registry.resolve(&key('f'), &home, false), None);

        // Contextual: Detail page actions
        assert_eq!(registry.resolve(&key('p'), &album, true), Some(ShortcutAction::DetailPlay));
        assert_eq!(
            registry.resolve(&shift_key('S'), &album, true),
            Some(ShortcutAction::DetailShuffle)
        );
        assert_eq!(
            registry.resolve(&key('o'), &album, true),
            Some(ShortcutAction::DetailOpenSpotify)
        );
        assert_eq!(registry.resolve(&key('p'), &home, false), None);
    }

    #[test]
    fn settings_round_trip_preserves_overrides() {
        let mut registry = ShortcutRegistry::default();
        registry.force_assign(ShortcutAction::TogglePlayback, KeyBinding::parse("p").unwrap());
        registry.force_assign(ShortcutAction::NextTrack, KeyBinding::parse("Shift+N").unwrap());
        let overrides = registry.to_overrides();
        assert_eq!(overrides.get("toggle_playback").unwrap(), &vec!["p".to_string()]);
        assert_eq!(overrides.get("next_track").unwrap(), &vec!["Shift+N".to_string()]);

        let mut new_registry = ShortcutRegistry::default();
        new_registry.load_overrides(&overrides);
        assert_eq!(new_registry.to_overrides(), overrides);
        assert_eq!(new_registry.primary_canonical(ShortcutAction::TogglePlayback), "p");
        assert_eq!(new_registry.primary_canonical(ShortcutAction::NextTrack), "Shift+N");
    }

    #[test]
    fn older_settings_files_load_default_shortcuts() {
        let empty_overrides = std::collections::BTreeMap::new();
        let mut registry = ShortcutRegistry::default();
        registry.load_overrides(&empty_overrides);
        assert_eq!(
            registry.get_bindings(ShortcutAction::TogglePlayback),
            ShortcutAction::TogglePlayback.default_bindings()
        );
        assert_eq!(
            registry.get_bindings(ShortcutAction::Queue),
            ShortcutAction::Queue.default_bindings()
        );
        assert_eq!(
            registry.get_bindings(ShortcutAction::Quit),
            ShortcutAction::Quit.default_bindings()
        );
    }

    #[test]
    fn scope_conflict_rules() {
        let mut registry = ShortcutRegistry::default();
        // Global Quit vs General Playback
        let quit_key = KeyBinding::parse("x").unwrap();
        let err = registry.try_assign(ShortcutAction::TogglePlayback, quit_key).unwrap_err();
        assert_eq!(err.conflicting_action, ShortcutAction::Quit);

        // General vs Detail: both conflict
        let p_key = KeyBinding::parse("p").unwrap();
        let err2 = registry.try_assign(ShortcutAction::TogglePlayback, p_key).unwrap_err();
        assert_eq!(err2.conflicting_action, ShortcutAction::DetailPlay);
    }
}
