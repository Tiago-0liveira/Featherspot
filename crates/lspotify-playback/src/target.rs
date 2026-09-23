use lspotify_core::DeviceId;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SelectedTarget {
    LocalEmbedded { device_id: DeviceId },
    SpotifyConnect { device_id: Option<DeviceId> },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FallbackReason {
    StartupTimeout,
    SdkRejected,
    WebViewMissing,
    DrmUnsupported,
    PlayerFailure,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlaybackCoordinator {
    selected: SelectedTarget,
    local_ready: Option<DeviceId>,
    fallback_reason: Option<FallbackReason>,
}

impl Default for PlaybackCoordinator {
    fn default() -> Self {
        Self {
            selected: SelectedTarget::SpotifyConnect { device_id: None },
            local_ready: None,
            fallback_reason: None,
        }
    }
}

impl PlaybackCoordinator {
    pub fn selected(&self) -> &SelectedTarget {
        &self.selected
    }

    pub fn fallback_reason(&self) -> Option<FallbackReason> {
        self.fallback_reason
    }

    pub fn local_became_ready(&mut self, device_id: DeviceId) {
        self.local_ready = Some(device_id);
        self.fallback_reason = None;
    }

    pub fn play_on_this_device(&mut self) -> Option<DeviceId> {
        let device_id = self.local_ready.clone()?;
        self.selected = SelectedTarget::LocalEmbedded { device_id: device_id.clone() };
        Some(device_id)
    }

    pub fn select_connect(&mut self, device_id: Option<DeviceId>) {
        self.selected = SelectedTarget::SpotifyConnect { device_id };
    }

    pub fn local_failed(
        &mut self,
        reason: FallbackReason,
        active_connect_device: Option<DeviceId>,
    ) {
        self.local_ready = None;
        self.fallback_reason = Some(reason);
        self.selected = SelectedTarget::SpotifyConnect { device_id: active_connect_device };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_readiness_does_not_transfer_implicitly() {
        let mut coordinator = PlaybackCoordinator::default();
        coordinator.local_became_ready(DeviceId::parse("local123").unwrap());
        assert_eq!(coordinator.selected(), &SelectedTarget::SpotifyConnect { device_id: None });
        assert_eq!(coordinator.play_on_this_device().unwrap().as_str(), "local123");
        assert!(matches!(coordinator.selected(), SelectedTarget::LocalEmbedded { .. }));
    }

    #[test]
    fn failure_returns_to_connect() {
        let mut coordinator = PlaybackCoordinator::default();
        coordinator.local_became_ready(DeviceId::parse("local123").unwrap());
        coordinator.play_on_this_device();
        let fallback = DeviceId::parse("speaker456").unwrap();
        coordinator.local_failed(FallbackReason::PlayerFailure, Some(fallback.clone()));
        assert_eq!(
            coordinator.selected(),
            &SelectedTarget::SpotifyConnect { device_id: Some(fallback) }
        );
    }
}
