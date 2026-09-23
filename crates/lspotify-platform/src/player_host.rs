use std::{
    env, fs,
    io::{BufRead as _, BufReader, Write},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{Mutex, mpsc},
};

use lspotify_core::{AppError, ErrorKind, LocalPlayerCommand, LocalPlayerEvent, Result};
use lspotify_playback::{
    BRIDGE_PROTOCOL_VERSION, BridgeEvent, Envelope, HostRecord, LocalCommand, parse_host_record,
};

const HOST_BASENAME: &str = "lspotify-player-host";

/// Resolves the private helper from a known application layout. It intentionally never consults
/// PATH: the helper receives short-lived Spotify access tokens.
#[derive(Clone, Debug)]
pub struct PlayerHostLocator {
    executable: PathBuf,
}

impl PlayerHostLocator {
    /// Uses the running application executable as the package-layout anchor.
    pub fn for_current_executable() -> Result<Self> {
        Ok(Self { executable: env::current_exe().map_err(io_error)? })
    }

    /// Creates a locator anchored to an executable path. Useful to package builders and tests.
    pub fn from_executable(executable: impl Into<PathBuf>) -> Self {
        Self { executable: executable.into() }
    }

    /// Finds and validates the private helper. The development override is available only in
    /// debug builds so production packages cannot be redirected by environment variables.
    pub fn locate(&self) -> Result<PathBuf> {
        #[cfg(debug_assertions)]
        if let Some(path) = env::var_os("LSPOTIFY_PLAYER_HOST") {
            return validate_host(PathBuf::from(path));
        }
        let directory = self.executable.parent().ok_or_else(|| {
            AppError::new(ErrorKind::Unavailable, "application executable has no parent directory")
        })?;
        let name = host_filename();
        let candidates = if cfg!(target_os = "macos") {
            vec![directory.join("../Helpers").join(name)]
        } else if cfg!(target_os = "windows") {
            vec![directory.join("helpers").join(name), directory.join(name)]
        } else {
            vec![
                directory.join("../lib/lspotify").join(name),
                directory.join("../libexec").join(name),
                directory.join(name),
            ]
        };
        candidates.into_iter().find_map(|candidate| validate_host(candidate).ok()).ok_or_else(
            || {
                AppError::new(
                    ErrorKind::Unavailable,
                    "lspotify's private player helper is missing; reinstall the application",
                )
            },
        )
    }
}

/// Managed stdin/stdout client for the isolated private helper.
#[derive(Debug)]
pub struct PlayerHostClient {
    commands: mpsc::Sender<LocalPlayerCommand>,
    events: Mutex<mpsc::Receiver<LocalPlayerEvent>>,
    child: Mutex<Option<Child>>,
}

impl PlayerHostClient {
    /// Launches the packaged helper, verifies its nonce/protocol handshake, and starts forwarding
    /// typed events. No token is sent until this function returns successfully.
    pub fn launch(locator: &PlayerHostLocator) -> Result<Self> {
        let executable = locator.locate()?;
        let nonce = random_nonce();
        let mut command = Command::new(&executable);
        command.args([
            "--nonce",
            &nonce,
            "--protocol-version",
            &BRIDGE_PROTOCOL_VERSION.to_string(),
        ]);
        command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt as _;
            command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        let mut child = command.spawn().map_err(io_error)?;
        let stdout = child.stdout.take().ok_or_else(|| {
            AppError::new(ErrorKind::Unavailable, "player helper has no stdout pipe")
        })?;
        let mut lines = BufReader::new(stdout).lines();
        let hello = lines.next().transpose().map_err(io_error)?.ok_or_else(|| {
            AppError::new(
                ErrorKind::Unavailable,
                "player helper exited before its compatibility handshake",
            )
        })?;
        if let Err(error) = validate_hello(&hello, &nonce) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        let stdin = child.stdin.take().ok_or_else(|| {
            AppError::new(ErrorKind::Unavailable, "player helper has no stdin pipe")
        })?;
        let (commands, command_receiver) = mpsc::channel::<LocalPlayerCommand>();
        let (event_sender, events) = mpsc::channel();
        let write_nonce = nonce.clone();
        std::thread::spawn(move || write_commands(stdin, command_receiver, write_nonce));
        std::thread::spawn(move || {
            for line in lines.map_while(std::result::Result::ok) {
                match parse_host_record(&line) {
                    Ok(HostRecord::Event { nonce: record_nonce, event })
                        if record_nonce == nonce =>
                    {
                        let _ = event_sender.send(map_event(event.payload));
                    }
                    Ok(HostRecord::Hello(_)) | Ok(HostRecord::Event { .. }) | Err(_) => {}
                }
            }
            let _ = event_sender.send(LocalPlayerEvent::Unavailable);
        });
        Ok(Self { commands, events: Mutex::new(events), child: Mutex::new(Some(child)) })
    }

    pub fn send(&self, command: LocalPlayerCommand) -> Result<()> {
        self.commands.send(command).map_err(|_| {
            AppError::new(ErrorKind::Unavailable, "local player host has already shut down")
        })
    }

    pub fn try_next_event(&self) -> Result<Option<LocalPlayerEvent>> {
        self.events
            .lock()
            .map_err(|_| poisoned())?
            .try_recv()
            .ok()
            .map_or(Ok(None), |event| Ok(Some(event)))
    }

    pub fn shutdown(&self) -> Result<()> {
        let _ = self.commands.send(LocalPlayerCommand::Shutdown);
        if let Some(mut child) = self.child.lock().map_err(|_| poisoned())?.take() {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
            while std::time::Instant::now() < deadline {
                if child.try_wait().map_err(io_error)?.is_some() {
                    return Ok(());
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            child.kill().map_err(io_error)?;
        }
        Ok(())
    }
}

fn host_filename() -> &'static str {
    if cfg!(target_os = "windows") { "lspotify-player-host.exe" } else { HOST_BASENAME }
}

fn validate_host(path: PathBuf) -> Result<PathBuf> {
    let metadata = fs::metadata(&path).map_err(|_| {
        AppError::new(
            ErrorKind::Unavailable,
            "lspotify's private player helper is missing; reinstall the application",
        )
    })?;
    if !metadata.is_file() {
        return Err(AppError::new(
            ErrorKind::Unavailable,
            "lspotify's player helper is not a regular file",
        ));
    }
    #[cfg(unix)]
    if std::os::unix::fs::PermissionsExt::mode(&metadata.permissions()) & 0o111 == 0 {
        return Err(AppError::new(
            ErrorKind::Unavailable,
            "lspotify's player helper is not executable",
        ));
    }
    Ok(path)
}

fn validate_hello(line: &str, nonce: &str) -> Result<()> {
    match parse_host_record(line)? {
        HostRecord::Hello(hello)
            if hello.version == BRIDGE_PROTOCOL_VERSION && hello.nonce == nonce =>
        {
            Ok(())
        }
        HostRecord::Hello(_) => Err(AppError::new(
            ErrorKind::Unavailable,
            "player helper version mismatch; reinstall lspotify",
        )),
        HostRecord::Event { .. } => Err(AppError::new(
            ErrorKind::Unavailable,
            "player helper sent an invalid compatibility handshake",
        )),
    }
}

fn write_commands(
    mut stdin: impl Write,
    receiver: mpsc::Receiver<LocalPlayerCommand>,
    nonce: String,
) {
    let mut id = 1_u64;
    for command in receiver {
        let shutdown = matches!(command, LocalPlayerCommand::Shutdown);
        let payload = command_value(command);
        let record = LocalCommand {
            nonce: nonce.clone(),
            command: Envelope { version: BRIDGE_PROTOCOL_VERSION, id, payload },
        };
        id = id.wrapping_add(1);
        if serde_json::to_writer(&mut stdin, &record).is_err()
            || writeln!(stdin).is_err()
            || stdin.flush().is_err()
        {
            break;
        }
        if shutdown {
            break;
        }
    }
}

fn command_value(command: LocalPlayerCommand) -> serde_json::Value {
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
}
fn map_event(event: BridgeEvent) -> LocalPlayerEvent {
    match event {
        BridgeEvent::Ready { device_id } => LocalPlayerEvent::Ready { device_id },
        BridgeEvent::Unavailable { .. } => LocalPlayerEvent::Unavailable,
        BridgeEvent::StateChanged { playing, position_ms, duration_ms, track_uri } => {
            LocalPlayerEvent::StateChanged { playing, position_ms, duration_ms, track_uri }
        }
        BridgeEvent::AuthenticationError { message } => {
            LocalPlayerEvent::AuthenticationError(message)
        }
        BridgeEvent::PlaybackError { message } => LocalPlayerEvent::PlaybackError(message),
        BridgeEvent::AccountError { message } => LocalPlayerEvent::AccountError(message),
    }
}
fn random_nonce() -> String {
    let bytes: [u8; 32] = rand::random();
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
fn io_error(error: std::io::Error) -> AppError {
    AppError::new(ErrorKind::Unavailable, error.to_string())
}
fn poisoned() -> AppError {
    AppError::new(ErrorKind::Unexpected, "player host lock was poisoned")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn package_layouts_are_private() {
        let locator = PlayerHostLocator::from_executable(
            "/Applications/lspotify.app/Contents/MacOS/lspotify",
        );
        assert!(locator.executable.ends_with("lspotify"));
    }
    #[test]
    fn mismatched_handshake_is_rejected() {
        assert!(validate_hello(r#"{"kind":"hello","version":9,"package_version":"x","nonce":"n","backend":"x","capabilities":[]}"#, "n").is_err());
    }
}
