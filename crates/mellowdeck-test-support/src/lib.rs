#![forbid(unsafe_code)]

use std::{sync::Mutex, time::Duration};

use mellowdeck_core::{
    AccountCapabilities, AppError, BoxFuture, CapabilityAvailability, CredentialStore, Device,
    DeviceId, ErrorKind, PlayRequest, PlaybackService, ProductTier, Queue, RepeatMode, Result,
    SessionService, SpotifyUri,
};

#[derive(Debug, Default)]
pub struct FakeCredentialStore {
    token: Mutex<Option<String>>,
    pub fail_reads: bool,
}

impl CredentialStore for FakeCredentialStore {
    fn load_refresh_token(&self) -> Result<Option<String>> {
        if self.fail_reads {
            return Err(AppError::new(ErrorKind::Storage, "injected credential read failure"));
        }
        self.token
            .lock()
            .map_err(|_| AppError::new(ErrorKind::Storage, "fake lock poisoned"))
            .map(|value| value.clone())
    }

    fn store_refresh_token(&self, refresh_token: &str) -> Result<()> {
        *self
            .token
            .lock()
            .map_err(|_| AppError::new(ErrorKind::Storage, "fake lock poisoned"))? =
            Some(refresh_token.to_owned());
        Ok(())
    }

    fn clear_refresh_token(&self) -> Result<()> {
        *self
            .token
            .lock()
            .map_err(|_| AppError::new(ErrorKind::Storage, "fake lock poisoned"))? = None;
        Ok(())
    }
}

#[derive(Debug)]
pub struct FakeSessionService {
    signed_in: Mutex<bool>,
    capabilities: AccountCapabilities,
}

impl Default for FakeSessionService {
    fn default() -> Self {
        Self {
            signed_in: Mutex::new(false),
            capabilities: AccountCapabilities {
                product: ProductTier::Premium,
                embedded_playback: CapabilityAvailability::Available,
                library_mutation: CapabilityAvailability::Available,
                playlist_mutation: CapabilityAvailability::Available,
                connect_control: CapabilityAvailability::Available,
                restrictions: Vec::new(),
            },
        }
    }
}

impl SessionService for FakeSessionService {
    fn authorize<'a>(&'a self, _client_id: &'a str) -> BoxFuture<'a, Result<AccountCapabilities>> {
        Box::pin(async move {
            *self.signed_in.lock().map_err(|_| fake_lock_error())? = true;
            Ok(self.capabilities.clone())
        })
    }

    fn refresh(&self) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            if *self.signed_in.lock().map_err(|_| fake_lock_error())? {
                Ok(())
            } else {
                Err(AppError::new(ErrorKind::Authentication, "fake session is signed out"))
            }
        })
    }

    fn logout(&self) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            *self.signed_in.lock().map_err(|_| fake_lock_error())? = false;
            Ok(())
        })
    }

    fn capabilities(&self) -> BoxFuture<'_, Result<AccountCapabilities>> {
        Box::pin(async move { Ok(self.capabilities.clone()) })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PlaybackCall {
    Play(PlayRequest, Option<DeviceId>),
    Pause,
    Resume,
    Previous,
    Next,
    Seek(Duration),
    Shuffle(bool),
    Repeat(RepeatMode),
    Volume(u8),
    Enqueue(SpotifyUri),
    Transfer(DeviceId, bool),
}

#[derive(Debug, Default)]
pub struct FakePlaybackService {
    calls: Mutex<Vec<PlaybackCall>>,
    devices: Mutex<Vec<Device>>,
    queue: Mutex<Queue>,
}

impl FakePlaybackService {
    /// Returns all recorded commands in submission order.
    ///
    /// # Errors
    ///
    /// Returns an error if another panic poisoned the fake adapter lock.
    pub fn calls(&self) -> Result<Vec<PlaybackCall>> {
        self.calls.lock().map_err(|_| fake_lock_error()).map(|calls| calls.clone())
    }

    /// Replaces the devices returned by [`PlaybackService::devices`].
    ///
    /// # Errors
    ///
    /// Returns an error if another panic poisoned the fake adapter lock.
    pub fn set_devices(&self, devices: Vec<Device>) -> Result<()> {
        *self.devices.lock().map_err(|_| fake_lock_error())? = devices;
        Ok(())
    }

    fn record(&self, call: PlaybackCall) -> Result<()> {
        self.calls.lock().map_err(|_| fake_lock_error())?.push(call);
        Ok(())
    }
}

impl PlaybackService for FakePlaybackService {
    fn play(&self, request: PlayRequest, target: Option<DeviceId>) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move { self.record(PlaybackCall::Play(request, target)) })
    }

    fn pause(&self) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move { self.record(PlaybackCall::Pause) })
    }

    fn resume(&self) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move { self.record(PlaybackCall::Resume) })
    }

    fn previous(&self) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move { self.record(PlaybackCall::Previous) })
    }

    fn next(&self) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move { self.record(PlaybackCall::Next) })
    }

    fn seek(&self, position: Duration) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move { self.record(PlaybackCall::Seek(position)) })
    }

    fn shuffle(&self, enabled: bool) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move { self.record(PlaybackCall::Shuffle(enabled)) })
    }

    fn repeat(&self, mode: RepeatMode) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move { self.record(PlaybackCall::Repeat(mode)) })
    }

    fn volume(&self, percent: u8) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            if percent > 100 {
                return Err(AppError::new(ErrorKind::InvalidInput, "volume exceeds 100 percent"));
            }
            self.record(PlaybackCall::Volume(percent))
        })
    }

    fn queue(&self) -> BoxFuture<'_, Result<Queue>> {
        Box::pin(async move {
            self.queue.lock().map_err(|_| fake_lock_error()).map(|queue| queue.clone())
        })
    }

    fn enqueue(&self, uri: SpotifyUri) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move { self.record(PlaybackCall::Enqueue(uri)) })
    }

    fn devices(&self) -> BoxFuture<'_, Result<Vec<Device>>> {
        Box::pin(async move {
            self.devices.lock().map_err(|_| fake_lock_error()).map(|devices| devices.clone())
        })
    }

    fn transfer(&self, device: DeviceId, start_playing: bool) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move { self.record(PlaybackCall::Transfer(device, start_playing)) })
    }
}

fn fake_lock_error() -> AppError {
    AppError::new(ErrorKind::Unexpected, "fake adapter lock was poisoned")
}
