use std::{
    path::PathBuf,
    sync::{Arc, Mutex, mpsc},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use librespot_connect::{ConnectConfig, Spirc};
use librespot_core::{
    Session, SessionConfig,
    authentication::Credentials,
    cache::Cache,
    config::DeviceType,
};
use librespot_oauth::OAuthClientBuilder;
use librespot_playback::{
    audio_backend,
    config::{AudioFormat, PlayerConfig},
    mixer::{self, MixerConfig},
    player::Player,
};
use lspotify_core::{
    AppError, DeviceId, ErrorKind, LocalPlayerCommand, LocalPlayerEvent, Result,
};
use rand::Rng as _;

#[derive(Debug)]
pub struct LibrespotLocalPlayer {
    commands: mpsc::SyncSender<LocalPlayerCommand>,
    events: Mutex<mpsc::Receiver<LocalPlayerEvent>>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl LibrespotLocalPlayer {
    /// Starts the lightweight coordinator thread. Spotify networking and audio are initialized
    /// only after a `Connect` command is received.
    ///
    /// # Errors
    ///
    /// Returns an error when the worker thread cannot be created.
    pub fn start(credentials_dir: PathBuf) -> Result<Self> {
        let (commands, command_receiver) = mpsc::sync_channel(64);
        let (event_sender, events) = mpsc::sync_channel(64);
        let thread = thread::Builder::new()
            .name("lspotify-librespot".into())
            .spawn(move || run(command_receiver, event_sender, credentials_dir))
            .map_err(|error| AppError::new(ErrorKind::Unavailable, error.to_string()))?;
        Ok(Self { commands, events: Mutex::new(events), thread: Mutex::new(Some(thread)) })
    }

    pub fn send(&self, command: LocalPlayerCommand) -> Result<()> {
        self.commands.send(command).map_err(|_| {
            AppError::new(ErrorKind::Unavailable, "librespot playback worker has stopped")
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
        if let Some(thread) = self.thread.lock().map_err(|_| poisoned())?.take() {
            let _ = thread.join();
        }
        Ok(())
    }
}

impl Drop for LibrespotLocalPlayer {
    fn drop(&mut self) {
        let _ = self.commands.send(LocalPlayerCommand::Shutdown);
        if let Ok(thread) = self.thread.get_mut()
            && let Some(thread) = thread.take()
        {
            let _ = thread.join();
        }
    }
}

struct ActivePlayer {
    session: Session,
    spirc: Spirc,
    _player: Arc<Player>,
    spirc_task: tokio::task::JoinHandle<()>,
    device_id: DeviceId,
    ready_at: Instant,
    ready_emitted: bool,
}

fn run(
    commands: mpsc::Receiver<LocalPlayerCommand>,
    events: mpsc::SyncSender<LocalPlayerEvent>,
    credentials_dir: PathBuf,
) {
    let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(error) => {
            let _ = events.try_send(LocalPlayerEvent::Unavailable);
            tracing::error!(%error, "could not create librespot runtime");
            return;
        }
    };
    runtime.block_on(run_async(commands, events, credentials_dir));
}

async fn run_async(
    commands: mpsc::Receiver<LocalPlayerCommand>,
    events: mpsc::SyncSender<LocalPlayerEvent>,
    credentials_dir: PathBuf,
) {
    let mut active: Option<ActivePlayer> = None;

    loop {
        match commands.try_recv() {
            Ok(LocalPlayerCommand::TokenUpdate { .. }) => {}
            Ok(LocalPlayerCommand::Connect) => {
                if let Some(player) = active.as_ref() {
                    if let Err(error) = player.spirc.activate() {
                        playback_error(&events, error.to_string());
                    }
                } else {
                    match connect(credentials_dir.clone()).await {
                        Ok(player) => {
                            tracing::info!(
                                device_id = %player.device_id,
                                "librespot session authenticated; waiting for Spotify Connect registration"
                            );
                            active = Some(player);
                        }
                        Err(error) => {
                            let _ = events.try_send(LocalPlayerEvent::Unavailable);
                            playback_error(&events, error);
                        }
                    }
                }
            }
            Ok(LocalPlayerCommand::Disconnect) => {
                if let Some(player) = active.as_ref()
                    && let Err(error) = player.spirc.disconnect(true)
                {
                    playback_error(&events, error.to_string());
                }
            }
            Ok(LocalPlayerCommand::Play) => {
                if let Some(player) = active.as_ref()
                    && let Err(error) = player.spirc.play()
                {
                    playback_error(&events, error.to_string());
                }
            }
            Ok(LocalPlayerCommand::Pause) => {
                if let Some(player) = active.as_ref()
                    && let Err(error) = player.spirc.pause()
                {
                    playback_error(&events, error.to_string());
                }
            }
            Ok(LocalPlayerCommand::Seek { position_ms }) => {
                if let Some(player) = active.as_ref() {
                    let position = u32::try_from(position_ms).unwrap_or(u32::MAX);
                    if let Err(error) = player.spirc.set_position_ms(position) {
                        playback_error(&events, error.to_string());
                    }
                }
            }
            Ok(LocalPlayerCommand::Volume { value_milli }) => {
                if let Some(player) = active.as_ref() {
                    let value = u32::from(value_milli.min(1000));
                    let volume = ((value * u32::from(u16::MAX)) / 1000) as u16;
                    if let Err(error) = player.spirc.set_volume(volume) {
                        playback_error(&events, error.to_string());
                    }
                }
            }
            Ok(LocalPlayerCommand::Shutdown) => {
                shutdown_active(active.take());
                break;
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                shutdown_active(active.take());
                break;
            }
        }

        if active.as_ref().is_some_and(|player| player.spirc_task.is_finished()) {
            let was_ready = active.as_ref().is_some_and(|player| player.ready_emitted);
            shutdown_active(active.take());
            let _ = events.try_send(LocalPlayerEvent::Unavailable);
            playback_error(
                &events,
                if was_ready {
                    "Spotify Connect session stopped unexpectedly after becoming ready"
                } else {
                    "Spotify Connect session stopped during startup before the device became ready"
                },
            );
        } else if let Some(player) = active.as_mut()
            && !player.ready_emitted
            && Instant::now() >= player.ready_at
        {
            player.ready_emitted = true;
            tracing::info!(
                device_id = %player.device_id,
                "librespot startup grace period completed; reporting local device ready"
            );
            let _ = events.try_send(LocalPlayerEvent::Ready { device_id: player.device_id.clone() });
        }

        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn connect(credentials_dir: PathBuf) -> std::result::Result<ActivePlayer, String> {
    const REGISTRATION_GRACE_PERIOD: Duration = Duration::from_secs(3);
    const OAUTH_REDIRECT_URI: &str = "http://127.0.0.1:8898/login";

    let mut session_config = SessionConfig::default();
    session_config.device_id = random_device_id();
    let device_id = DeviceId::parse(session_config.device_id.clone())
        .map_err(|error| format!("generated device id is invalid: {error}"))?;

    let cache = Cache::new(Some(credentials_dir.clone()), None, None, None)
        .map_err(|error| format!("could not open Librespot credential cache: {error}"))?;

    let credentials = if let Some(credentials) = cache.credentials() {
        tracing::info!(
            path = %credentials_dir.display(),
            "using cached Librespot credentials"
        );
        credentials
    } else {
        tracing::info!(
            path = %credentials_dir.display(),
            "no cached Librespot credentials; starting one-time Spotify authorization"
        );
        OAuthClientBuilder::new(
            &session_config.client_id,
            OAUTH_REDIRECT_URI,
            vec!["streaming"],
        )
        .open_in_browser()
        .build()
        .map_err(|error| format!("could not prepare Librespot OAuth: {error}"))?
        .get_access_token()
        .map(|token| Credentials::with_access_token(token.access_token))
        .map_err(|error| format!("Librespot OAuth failed: {error}"))?
    };

    let session = Session::new(session_config, Some(cache));

    tracing::info!(device_id = %device_id, "initializing librespot audio and Spotify session");

    let mixer_builder =
        mixer::find(None).ok_or_else(|| "librespot soft-volume mixer is unavailable".to_string())?;
    let mixer = mixer_builder(MixerConfig::default())
        .map_err(|error| format!("could not initialize librespot mixer: {error}"))?;

    let sink_builder = audio_backend::find(None)
        .ok_or_else(|| "librespot audio backend is unavailable".to_string())?;
    let player = Player::new(
        PlayerConfig::default(),
        session.clone(),
        mixer.get_soft_volume(),
        move || sink_builder(None, AudioFormat::default()),
    );

    let connect_config = ConnectConfig {
        name: "lspotify".into(),
        device_type: DeviceType::Computer,
        ..ConnectConfig::default()
    };
    let (spirc, spirc_task) =
        Spirc::new(connect_config, session.clone(), credentials, Arc::clone(&player), mixer)
            .await
            .map_err(|error| format!("Spotify/Librespot authentication or SPIRC setup failed: {error}"))?;

    spirc
        .activate()
        .map_err(|error| format!("could not activate the Librespot Connect device: {error}"))?;

    tracing::info!(
        device_id = %device_id,
        "librespot SPIRC initialized and activation queued"
    );

    let spirc_task = tokio::spawn(spirc_task);

    Ok(ActivePlayer {
        session,
        spirc,
        _player: player,
        spirc_task,
        device_id,
        ready_at: Instant::now() + REGISTRATION_GRACE_PERIOD,
        ready_emitted: false,
    })
}

fn shutdown_active(active: Option<ActivePlayer>) {
    if let Some(player) = active {
        let _ = player.spirc.shutdown();
        player.session.shutdown();
        player.spirc_task.abort();
    }
}

fn playback_error(events: &mpsc::SyncSender<LocalPlayerEvent>, message: impl Into<String>) {
    let _ = events.try_send(LocalPlayerEvent::PlaybackError(format!(
        "Librespot: {}",
        message.into()
    )));
}

fn random_device_id() -> String {
    let bytes: [u8; 20] = rand::rng().random();
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn poisoned() -> AppError {
    AppError::new(ErrorKind::Unexpected, "librespot player lock was poisoned")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_device_ids_fit_spotify_identifier_rules() {
        let id = random_device_id();
        assert_eq!(id.len(), 40);
        assert!(DeviceId::parse(id).is_ok());
    }
}
