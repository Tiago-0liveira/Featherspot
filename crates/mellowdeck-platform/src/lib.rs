#![forbid(unsafe_code)]

use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};

#[cfg(target_os = "windows")]
use std::{
    io::{BufRead as _, BufReader, Write as _},
    process::{Child, Command, Stdio},
};

use mellowdeck_core::{
    AppError, CredentialStore, ErrorKind, MediaButton, MediaMetadata, MediaSession, Result,
};
use mellowdeck_core::{LocalPlayerCommand, LocalPlayerEvent};

#[cfg(target_os = "windows")]
mod webview;

#[cfg(target_os = "windows")]
mod windows_credentials;

#[cfg(target_os = "windows")]
pub use webview::{
    PlaybackWebView, create_playback_webview, is_allowed_player_navigation,
    playback_webview_builder,
};

#[cfg(target_os = "windows")]
pub use windows_credentials::WindowsCredentialStore;

/// Opens a URL in the user's default browser without blocking the application window.
///
/// # Errors
///
/// Returns an error when the operating system cannot launch the URL handler.
pub fn open_system_browser(url: &str) -> Result<()> {
    open::that_detached(url).map_err(|error| {
        AppError::new(
            ErrorKind::Unavailable,
            format!("default browser could not be opened: {error}"),
        )
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppPaths {
    pub data: PathBuf,
    pub cache: PathBuf,
    pub logs: PathBuf,
    pub settings: PathBuf,
}

/// Handle for the optional local Web Playback SDK host.
///
/// A terminal has no native child window to attach `WebView2` to. Consequently this portable
/// implementation reports an unavailable local player immediately and leaves Spotify Connect as
/// the authoritative fallback. Desktop surfaces create the actual `WebView` child themselves.
#[derive(Debug)]
pub struct BackgroundLocalPlayer {
    events: Mutex<mpsc::Receiver<LocalPlayerEvent>>,
    commands: mpsc::SyncSender<LocalPlayerCommand>,
    ready: Mutex<bool>,
    pid: Mutex<Option<u32>>,
    device_id: Mutex<Option<String>>,
    shutdown_reason: Arc<Mutex<Option<String>>>,
    #[cfg(target_os = "windows")]
    child: Mutex<Option<Child>>,
    #[cfg(target_os = "windows")]
    threads: Mutex<Vec<std::thread::JoinHandle<()>>>,
}

impl BackgroundLocalPlayer {
    /// Starts the local-player lifecycle and subscribes callers to typed availability events.
    pub fn start() -> Self {
        let (event_sender, events) = mpsc::sync_channel(64);
        let (commands, command_receiver) = mpsc::sync_channel(64);
        let shutdown_reason = Arc::new(Mutex::new(None));
        #[cfg(target_os = "windows")]
        {
            let executable =
                std::env::var_os("MELLOWDECK_PLAYER_HOST").map(PathBuf::from).or_else(|| {
                    std::env::current_exe().ok().and_then(|path| {
                        let directory = path.parent()?;
                        let candidate1 = directory.join("mellowdeck-player-host.exe");
                        if candidate1.is_file() {
                            return Some(candidate1);
                        }
                        let candidate2 =
                            directory.join("helpers").join("mellowdeck-player-host.exe");
                        if candidate2.is_file() {
                            return Some(candidate2);
                        }
                        None
                    })
                });
            if let Some(executable) = executable.filter(|path| path.is_file())
                && let Ok(mut child) =
                    Command::new(executable).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn()
            {
                let pid = child.id();
                tracing::info!(pid, "spawned player-host process");
                let mut threads = Vec::new();
                if let Some(mut stdin) = child.stdin.take() {
                    let handle = std::thread::spawn(move || {
                        for command in command_receiver {
                            let shutdown = matches!(command, LocalPlayerCommand::Shutdown);
                            let line = local_command_json(command);
                            if writeln!(stdin, "{line}").is_err() || stdin.flush().is_err() {
                                break;
                            }
                            if shutdown {
                                break;
                            }
                        }
                    });
                    threads.push(handle);
                }
                if let Some(stdout) = child.stdout.take() {
                    let sender = event_sender.clone();
                    let reason_clone = Arc::clone(&shutdown_reason);
                    let handle = std::thread::spawn(move || {
                        forward_host_events(stdout, &sender, &reason_clone);
                    });
                    threads.push(handle);
                }
                return Self {
                    events: Mutex::new(events),
                    commands,
                    ready: Mutex::new(false),
                    pid: Mutex::new(Some(pid)),
                    device_id: Mutex::new(None),
                    shutdown_reason,
                    child: Mutex::new(Some(child)),
                    threads: Mutex::new(threads),
                };
            }
        }
        #[cfg(not(target_os = "windows"))]
        {
            std::thread::spawn(move || for _ in command_receiver {});
        }
        let _ = event_sender.send(LocalPlayerEvent::Unavailable);
        Self {
            events: Mutex::new(events),
            commands,
            ready: Mutex::new(false),
            pid: Mutex::new(None),
            device_id: Mutex::new(None),
            shutdown_reason,
            #[cfg(target_os = "windows")]
            child: Mutex::new(None),
            #[cfg(target_os = "windows")]
            threads: Mutex::new(Vec::new()),
        }
    }

    /// Queues a typed command. Commands are harmless while the local host is unavailable.
    ///
    /// # Errors
    ///
    /// Returns an error after the host command channel has closed.
    pub fn send(&self, command: LocalPlayerCommand) -> Result<()> {
        self.commands.send(command).map_err(|_| {
            AppError::new(ErrorKind::Unavailable, "local player host has already shut down")
        })
    }

    /// Retrieves the next SDK event without blocking terminal rendering.
    ///
    /// # Errors
    ///
    /// Returns an error if the event subscription has been poisoned.
    pub fn try_next_event(&self) -> Result<Option<LocalPlayerEvent>> {
        let maybe_event = {
            let events = self.events.lock().map_err(|_| poisoned())?;
            events.try_recv().ok()
        };

        if let Some(event) = maybe_event {
            if let LocalPlayerEvent::Ready { ref device_id } = event {
                *self.ready.lock().map_err(|_| poisoned())? = true;
                *self.device_id.lock().map_err(|_| poisoned())? = Some(device_id.to_string());
                tracing::info!(%device_id, "local player host ready with device ID");
            } else if matches!(
                event,
                LocalPlayerEvent::Unavailable
                    | LocalPlayerEvent::AuthenticationError(_)
                    | LocalPlayerEvent::AccountError(_)
            ) {
                *self.ready.lock().map_err(|_| poisoned())? = false;
                *self.device_id.lock().map_err(|_| poisoned())? = None;
            }
            return Ok(Some(event));
        }
        Ok(None)
    }

    /// Returns whether the SDK has registered a local device.
    ///
    /// # Errors
    ///
    /// Returns an error if lifecycle state has been poisoned.
    pub fn is_ready(&self) -> Result<bool> {
        self.ready.lock().map_err(|_| poisoned()).map(|ready| *ready)
    }

    /// Returns the process ID of the spawned player-host process, if active.
    ///
    /// # Errors
    ///
    /// Returns an error if lifecycle state has been poisoned.
    pub fn pid(&self) -> Result<Option<u32>> {
        self.pid.lock().map_err(|_| poisoned()).map(|pid| *pid)
    }

    /// Returns the active Spotify Web Playback SDK device ID, if ready.
    ///
    /// # Errors
    ///
    /// Returns an error if lifecycle state has been poisoned.
    pub fn device_id(&self) -> Result<Option<String>> {
        self.device_id.lock().map_err(|_| poisoned()).map(|id| id.clone())
    }

    /// Returns the helper shutdown reason, if helper has terminated or shut down.
    ///
    /// # Errors
    ///
    /// Returns an error if lifecycle state has been poisoned.
    pub fn shutdown_reason(&self) -> Result<Option<String>> {
        self.shutdown_reason.lock().map_err(|_| poisoned()).map(|reason| reason.clone())
    }

    /// Explicitly shuts down the helper with an associated reason.
    ///
    /// # Errors
    ///
    /// Returns an error if lifecycle state has been poisoned.
    pub fn shutdown_with_reason(&self, reason: &str) -> Result<()> {
        let mut shutdown_guard = self.shutdown_reason.lock().map_err(|_| poisoned())?;
        if shutdown_guard.is_none() {
            tracing::info!(reason, "shutting down player-host helper");
            *shutdown_guard = Some(reason.to_string());
        }
        let _ = self.commands.send(LocalPlayerCommand::Shutdown);
        *self.ready.lock().map_err(|_| poisoned())? = false;
        *self.device_id.lock().map_err(|_| poisoned())? = None;
        #[cfg(target_os = "windows")]
        {
            let child = self.child.lock().map_err(|_| poisoned())?.take();
            if let Some(mut child) = child {
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
                let mut exited = false;
                while std::time::Instant::now() < deadline {
                    if let Ok(Some(_)) = child.try_wait() {
                        exited = true;
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                if !exited {
                    let _ = child.kill();
                }
                let _ = child.wait();
            }
            let handles = std::mem::take(&mut *self.threads.lock().map_err(|_| poisoned())?);
            for handle in handles {
                let _ = handle.join();
            }
        }
        Ok(())
    }

    /// Deterministically asks the host to end. It is safe to call more than once.
    ///
    /// # Errors
    ///
    /// Returns an error if lifecycle state has been poisoned.
    pub fn shutdown(&self) -> Result<()> {
        self.shutdown_with_reason("normal shutdown")
    }
}

#[cfg(target_os = "windows")]
fn forward_host_events(
    stdout: impl std::io::Read,
    sender: &mpsc::SyncSender<LocalPlayerEvent>,
    shutdown_reason: &Mutex<Option<String>>,
) {
    for line in BufReader::new(stdout).lines().map_while(std::result::Result::ok) {
        if let Ok(envelope) = mellowdeck_playback::parse_event(&line) {
            let event = match envelope.payload {
                mellowdeck_playback::BridgeEvent::Ready { device_id } => {
                    LocalPlayerEvent::Ready { device_id }
                }
                mellowdeck_playback::BridgeEvent::Unavailable { .. } => {
                    LocalPlayerEvent::Unavailable
                }
                mellowdeck_playback::BridgeEvent::StateChanged {
                    playing,
                    position_ms,
                    duration_ms,
                    track_uri,
                    title,
                    artist,
                    album,
                    artwork_url,
                } => LocalPlayerEvent::StateChanged {
                    playing,
                    position_ms,
                    duration_ms,
                    track_uri,
                    title,
                    artist,
                    album,
                    artwork_url,
                },
                mellowdeck_playback::BridgeEvent::AuthenticationError { message } => {
                    LocalPlayerEvent::AuthenticationError(message)
                }
                mellowdeck_playback::BridgeEvent::PlaybackError { message } => {
                    LocalPlayerEvent::PlaybackError(message)
                }
                mellowdeck_playback::BridgeEvent::AccountError { message } => {
                    LocalPlayerEvent::AccountError(message)
                }
            };
            let _ = sender.send(event);
        }
    }
    if let Ok(mut reason) = shutdown_reason.lock()
        && reason.is_none()
    {
        let unexpected = "player-host process exited unexpectedly".to_string();
        tracing::warn!(reason = %unexpected, "local player host terminated");
        *reason = Some(unexpected);
    }
    let _ = sender.send(LocalPlayerEvent::Unavailable);
}

impl Drop for BackgroundLocalPlayer {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

#[cfg(target_os = "windows")]
fn local_command_json(command: LocalPlayerCommand) -> String {
    use serde_json::json;
    match command {
        LocalPlayerCommand::TokenUpdate { access_token } => {
            json!({"type":"token_update","access_token":access_token})
        }
        LocalPlayerCommand::Connect => json!({"type":"connect"}),
        LocalPlayerCommand::Disconnect => json!({"type":"disconnect"}),
        LocalPlayerCommand::Play => json!({"type":"play"}),
        LocalPlayerCommand::Pause => json!({"type":"pause"}),
        LocalPlayerCommand::Seek { position_ms } => {
            json!({"type":"seek","position_ms":position_ms})
        }
        LocalPlayerCommand::Volume { value_milli } => {
            json!({"type":"volume","value_milli":value_milli})
        }
        LocalPlayerCommand::Shutdown => json!({"type":"shutdown"}),
    }
    .to_string()
}

impl AppPaths {
    /// Discovers the per-user application locations. Composition roots should use
    /// this instead of reading platform environment variables themselves.
    ///
    /// # Errors
    /// Returns an error if the home or data directory cannot be determined.
    pub fn discover() -> Result<Self> {
        #[cfg(target_os = "windows")]
        let root =
            std::env::var_os("LOCALAPPDATA").map(PathBuf::from).map(|path| path.join("Mellowdeck"));
        #[cfg(target_os = "macos")]
        let root = std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|home| home.join("Library").join("Application Support").join("Mellowdeck"));
        #[cfg(all(unix, not(target_os = "macos")))]
        let root = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share"))
            })
            .map(|path| path.join("mellowdeck"));
        root.map_or_else(
            || Err(AppError::new(ErrorKind::Storage, "could not discover a user data directory")),
            |root| Ok(Self::under(root)),
        )
    }

    pub fn under(root: impl AsRef<Path>) -> Self {
        let root = root.as_ref();
        Self {
            data: root.join("data"),
            cache: root.join("cache"),
            logs: root.join("logs"),
            settings: root.join("settings.json"),
        }
    }
}

#[derive(Debug, Default)]
pub struct MemoryCredentialStore {
    refresh_token: Mutex<Option<String>>,
}

impl CredentialStore for MemoryCredentialStore {
    fn load_refresh_token(&self) -> Result<Option<String>> {
        self.refresh_token.lock().map_err(|_| poisoned()).map(|token| token.clone())
    }

    fn store_refresh_token(&self, refresh_token: &str) -> Result<()> {
        if refresh_token.is_empty() {
            return Err(AppError::new(ErrorKind::InvalidInput, "refresh token cannot be empty"));
        }
        *self.refresh_token.lock().map_err(|_| poisoned())? = Some(refresh_token.to_owned());
        Ok(())
    }

    fn clear_refresh_token(&self) -> Result<()> {
        *self.refresh_token.lock().map_err(|_| poisoned())? = None;
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct NoopMediaSession {
    metadata: Mutex<Option<MediaMetadata>>,
    state: Mutex<(bool, Duration)>,
    buttons: Mutex<VecDeque<MediaButton>>,
}

impl NoopMediaSession {
    /// Adds a simulated media button event for adapter and application tests.
    ///
    /// # Errors
    ///
    /// Returns an error if another panic has poisoned the internal event queue.
    pub fn push_test_button(&self, button: MediaButton) -> Result<()> {
        self.buttons.lock().map_err(|_| poisoned())?.push_back(button);
        Ok(())
    }
}

impl MediaSession for NoopMediaSession {
    fn publish(&self, metadata: Option<MediaMetadata>) -> Result<()> {
        *self.metadata.lock().map_err(|_| poisoned())? = metadata;
        Ok(())
    }

    fn set_playback(&self, playing: bool, position: Duration) -> Result<()> {
        *self.state.lock().map_err(|_| poisoned())? = (playing, position);
        Ok(())
    }

    fn take_button_event(&self) -> Result<Option<MediaButton>> {
        Ok(self.buttons.lock().map_err(|_| poisoned())?.pop_front())
    }

    fn clear(&self) -> Result<()> {
        *self.metadata.lock().map_err(|_| poisoned())? = None;
        *self.state.lock().map_err(|_| poisoned())? = (false, Duration::ZERO);
        self.buttons.lock().map_err(|_| poisoned())?.clear();
        Ok(())
    }
}

fn poisoned() -> AppError {
    AppError::new(ErrorKind::Unexpected, "platform adapter lock was poisoned")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_credentials_obey_single_account_lifecycle() {
        let store = MemoryCredentialStore::default();
        assert_eq!(store.load_refresh_token().unwrap(), None);
        store.store_refresh_token("secret").unwrap();
        assert_eq!(store.load_refresh_token().unwrap().as_deref(), Some("secret"));
        store.clear_refresh_token().unwrap();
        assert_eq!(store.load_refresh_token().unwrap(), None);
    }

    #[test]
    fn platform_paths_are_kept_under_the_selected_root() {
        let paths = AppPaths::under("portable");
        assert_eq!(paths.cache, Path::new("portable").join("cache"));
        assert_eq!(paths.settings, Path::new("portable").join("settings.json"));
    }

    #[test]
    fn background_host_reports_connect_fallback_without_blocking() {
        let player = BackgroundLocalPlayer::start();
        assert_eq!(player.try_next_event().unwrap(), Some(LocalPlayerEvent::Unavailable));
        assert!(!player.is_ready().unwrap());
        player.shutdown().unwrap();
    }

    #[test]
    fn background_host_shutdown_is_idempotent() {
        let player = BackgroundLocalPlayer::start();
        player.shutdown().unwrap();
        player.shutdown().unwrap();
    }

    #[test]
    fn background_host_tracks_device_id_and_shutdown_reason() {
        let player = BackgroundLocalPlayer::start();
        assert_eq!(player.device_id().unwrap(), None);
        assert_eq!(player.shutdown_reason().unwrap(), None);
        player.shutdown_with_reason("test shutdown").unwrap();
        assert_eq!(player.shutdown_reason().unwrap().as_deref(), Some("test shutdown"));
        assert_eq!(player.device_id().unwrap(), None);
    }

    #[test]
    fn repeated_connect_commands_do_not_create_duplicate_connections() {
        use std::io::Write as _;

        let bridge_code = include_str!("../../../assets/web-player/player-bridge.js");
        let script = format!(
            r"
let listeners = {{}};
let connectCalls = 0;
let disconnectCalls = 0;
let posted = [];

class MockPlayer {{
  constructor(opts) {{ this.opts = opts; }}
  addListener(evt, cb) {{ listeners[evt] = cb; }}
  async connect() {{ connectCalls++; return true; }}
  disconnect() {{ disconnectCalls++; }}
}}

global.window = {{
  ipc: {{ postMessage: (msg) => posted.push(JSON.parse(msg)) }},
  Spotify: {{ Player: MockPlayer }},
  addEventListener: (evt, cb) => {{ if (evt === 'message') global.onMsg = cb; }}
}};

{bridge_code}

async function run() {{
  // 1. Initial connect
  global.onMsg({{ source: global.window, data: {{ version: 1, id: 1, payload: {{ type: 'connect' }} }} }});
  await new Promise(r => setTimeout(r, 10));
  if (connectCalls !== 1) throw new Error('Expected 1 connect call, got ' + connectCalls);

  // 2. Duplicate connect while connecting/connected must be ignored
  global.onMsg({{ source: global.window, data: {{ version: 1, id: 2, payload: {{ type: 'connect' }} }} }});
  await new Promise(r => setTimeout(r, 10));
  if (connectCalls !== 1) throw new Error('Duplicate connect was not ignored while connecting');

  // 3. Mark ready
  listeners['ready']({{ device_id: 'test-device-id' }});

  // 4. Duplicate connect while connected must be ignored
  global.onMsg({{ source: global.window, data: {{ version: 1, id: 3, payload: {{ type: 'connect' }} }} }});
  await new Promise(r => setTimeout(r, 10));
  if (connectCalls !== 1) throw new Error('Duplicate connect was not ignored while connected');

  // 5. Disconnect resets flags
  global.onMsg({{ source: global.window, data: {{ version: 1, id: 4, payload: {{ type: 'disconnect' }} }} }});
  await new Promise(r => setTimeout(r, 10));
  if (disconnectCalls !== 1) throw new Error('Expected 1 disconnect call');

  // 6. Connect after disconnect succeeds
  global.onMsg({{ source: global.window, data: {{ version: 1, id: 5, payload: {{ type: 'connect' }} }} }});
  await new Promise(r => setTimeout(r, 10));
  if (connectCalls !== 2) throw new Error('Expected 2nd connect call after disconnect, got ' + connectCalls);

  // 7. not_ready resets flags
  listeners['not_ready']();

  // 8. Connect after not_ready succeeds
  global.onMsg({{ source: global.window, data: {{ version: 1, id: 6, payload: {{ type: 'connect' }} }} }});
  await new Promise(r => setTimeout(r, 10));
  if (connectCalls !== 3) throw new Error('Expected 3rd connect call after not_ready, got ' + connectCalls);

  // 9. Shutdown resets flags and disconnects
  global.onMsg({{ source: global.window, data: {{ version: 1, id: 7, payload: {{ type: 'shutdown' }} }} }});
  await new Promise(r => setTimeout(r, 10));
  if (disconnectCalls !== 2) throw new Error('Expected 2 disconnect calls on shutdown, got ' + disconnectCalls);
}}

run().catch(err => {{
  console.error(err);
  process.exit(1);
}});
"
        );
        if let Ok(mut child) = std::process::Command::new("node")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
        {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(script.as_bytes());
            }
            let output = child.wait_with_output().expect("node execution");
            assert!(
                output.status.success(),
                "node script failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}
