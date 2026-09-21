use std::{
    fs,
    io::Write as _,
    path::{Path, PathBuf},
};

use mellowdeck_core::{AppError, ErrorKind, Result, SETTINGS_SCHEMA_VERSION, SettingsState};

#[derive(Clone, Debug)]
pub struct JsonSettingsStore {
    path: PathBuf,
}

impl JsonSettingsStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Loads settings or returns defaults when no settings file exists.
    ///
    /// # Errors
    ///
    /// Returns an error when the file cannot be read or contains invalid JSON.
    pub fn load(&self) -> Result<SettingsState> {
        if !self.path.exists() {
            return Ok(SettingsState::default());
        }
        let bytes = fs::read(&self.path).map_err(io_error)?;
        let mut settings: SettingsState = serde_json::from_slice(&bytes).map_err(|error| {
            AppError::new(ErrorKind::Storage, format!("invalid settings: {error}"))
        })?;
        if settings.schema_version > SETTINGS_SCHEMA_VERSION {
            return Err(AppError::new(
                ErrorKind::Storage,
                "settings were created by a newer Mellowdeck version",
            ));
        }
        if settings.schema_version == 0 {
            settings.schema_version = SETTINGS_SCHEMA_VERSION;
        }
        Ok(settings)
    }

    /// Atomically replaces the settings file while retaining rollback data until commit.
    ///
    /// # Errors
    ///
    /// Returns an error when serialization or a filesystem operation fails.
    pub fn save(&self, settings: &SettingsState) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(io_error)?;
        }
        let temporary = sibling_with_extension(&self.path, "tmp");
        let backup = sibling_with_extension(&self.path, "bak");
        let bytes = serde_json::to_vec_pretty(settings)
            .map_err(|error| AppError::new(ErrorKind::Storage, error.to_string()))?;
        let mut file = fs::File::create(&temporary).map_err(io_error)?;
        file.write_all(&bytes).map_err(io_error)?;
        file.sync_all().map_err(io_error)?;

        let had_existing = self.path.exists();
        if had_existing && backup.exists() {
            fs::remove_file(&backup).map_err(io_error)?;
        }
        if had_existing {
            fs::rename(&self.path, &backup).map_err(io_error)?;
        }
        if let Err(error) = fs::rename(&temporary, &self.path) {
            if had_existing
                && let Err(rollback_error) = fs::rename(&backup, &self.path)
            {
                tracing::error!(%rollback_error, "failed to rollback settings from backup");
            }
            return Err(io_error(error));
        }
        if had_existing {
            fs::remove_file(backup).map_err(io_error)?;
        }
        Ok(())
    }
}

fn sibling_with_extension(path: &Path, extension: &str) -> PathBuf {
    let mut sibling = path.as_os_str().to_owned();
    sibling.push(format!(".{extension}"));
    PathBuf::from(sibling)
}

fn io_error(error: std::io::Error) -> AppError {
    let message = error.to_string();
    drop(error);
    AppError::new(ErrorKind::Storage, message)
}

#[cfg(test)]
mod tests {
    use mellowdeck_core::ThemePreference;

    use super::*;

    #[test]
    fn missing_settings_use_defaults_and_updates_round_trip() {
        let directory = tempfile::tempdir().unwrap();
        let store = JsonSettingsStore::new(directory.path().join("settings.json"));
        assert_eq!(store.load().unwrap().cache_limit_mb, 500);
        let mut settings =
            SettingsState { theme: ThemePreference::InkDark, ..SettingsState::default() };
        store.save(&settings).unwrap();
        assert_eq!(store.load().unwrap().theme, ThemePreference::InkDark);
        settings.cache_limit_mb = 700;
        store.save(&settings).unwrap();
        assert_eq!(store.load().unwrap().cache_limit_mb, 700);
    }

    #[test]
    fn future_settings_are_rejected_without_overwriting_them() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        fs::write(&path, r#"{"schema_version":999}"#).unwrap();
        let store = JsonSettingsStore::new(&path);
        let error = store.load().unwrap_err();
        assert_eq!(error.kind, ErrorKind::Storage);
        assert!(fs::read_to_string(path).unwrap().contains("999"));
    }
}
