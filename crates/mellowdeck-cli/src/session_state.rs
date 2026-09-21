//! CLI-owned playback cache. This deliberately does not extend core settings.
use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    path::{Path, PathBuf},
};
const SCHEMA_VERSION: u32 = 1;
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CliSessionState {
    pub schema_version: u32,
    pub volume: u8,
    pub last_track: Option<PersistedTrack>,
    #[serde(default)]
    pub recent_searches: Vec<String>,
}
impl Default for CliSessionState {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            volume: 50,
            last_track: None,
            recent_searches: Vec::new(),
        }
    }
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PersistedTrack {
    pub uri: String,
    pub title: String,
    pub artist: String,
    pub artists: Vec<(String, Option<String>)>,
    pub album: String,
    pub album_uri: Option<String>,
    pub artwork_url: Option<String>,
    pub duration_ms: u64,
}
#[derive(Clone, Debug)]
pub struct CliSessionStore {
    path: PathBuf,
}
impl CliSessionStore {
    pub fn new(data_dir: impl AsRef<Path>) -> Self {
        Self { path: data_dir.as_ref().join("cli-session.json") }
    }
    pub fn load(&self) -> CliSessionState {
        let Ok(contents) = fs::read_to_string(&self.path) else {
            return CliSessionState::default();
        };
        let Ok(mut state) = serde_json::from_str::<CliSessionState>(&contents) else {
            return CliSessionState::default();
        };
        if state.schema_version > SCHEMA_VERSION {
            return CliSessionState::default();
        }
        state.schema_version = SCHEMA_VERSION;
        state.volume = state.volume.min(100);
        state
    }
    /// Saves the CLI session state to disk atomically.
    ///
    /// # Errors
    /// Returns an error if the directory cannot be created, serialization fails, or writing fails.
    pub fn save(&self, state: &CliSessionState) -> io::Result<()> {
        let Some(parent) = self.path.parent() else { return Ok(()) };
        fs::create_dir_all(parent)?;
        let temporary = self.path.with_extension("json.tmp");
        let encoded = serde_json::to_vec_pretty(&CliSessionState {
            schema_version: SCHEMA_VERSION,
            volume: state.volume.min(100),
            last_track: state.last_track.clone(),
            recent_searches: state.recent_searches.clone(),
        })
        .map_err(io::Error::other)?;
        fs::write(&temporary, encoded)?;
        if self.path.exists() {
            let _ = fs::remove_file(&self.path);
        }
        fs::rename(temporary, &self.path)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_and_future_files_fall_back() {
        let root =
            std::env::temp_dir().join(format!("mellowdeck-session-fallback-{}", std::process::id()));
        let store = CliSessionStore::new(&root);
        assert_eq!(store.load().volume, 50);
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("cli-session.json"),
            r#"{"schema_version":99,"volume":7,"last_track":null}"#,
        )
        .unwrap();
        assert_eq!(store.load().volume, 50);
        let _ = fs::remove_file(root.join("cli-session.json"));
        let _ = fs::remove_dir(root);
    }
    #[test]
    fn round_trip_clamps_volume() {
        let root =
            std::env::temp_dir().join(format!("mellowdeck-session-clamp-{}", std::process::id()));
        let store = CliSessionStore::new(&root);
        store.save(&CliSessionState {
            schema_version: 1,
            volume: 255,
            last_track: None,
            recent_searches: Vec::new(),
        })
        .unwrap();
        assert_eq!(store.load().volume, 100);
        let _ = fs::remove_file(root.join("cli-session.json"));
        let _ = fs::remove_dir(root);
    }
    #[test]
    fn multiple_saves_to_same_path_succeed_and_update_content() {
        let root =
            std::env::temp_dir().join(format!("mellowdeck-session-multi-{}", std::process::id()));
        let store = CliSessionStore::new(&root);
        let first = CliSessionState {
            schema_version: 1,
            volume: 40,
            last_track: None,
            recent_searches: Vec::new(),
        };
        store.save(&first).unwrap();
        assert_eq!(store.load().volume, 40);

        let second = CliSessionState {
            schema_version: 1,
            volume: 80,
            last_track: Some(PersistedTrack {
                uri: "spotify:track:123".into(),
                title: "Track 1".into(),
                artist: "Artist 1".into(),
                artists: vec![("Artist 1".into(), None)],
                album: "Album 1".into(),
                album_uri: None,
                artwork_url: None,
                duration_ms: 200_000,
            }),
            recent_searches: vec!["daft punk".into()],
        };
        store.save(&second).unwrap();
        assert_eq!(store.load(), second);

        let third = CliSessionState {
            schema_version: 1,
            volume: 60,
            last_track: None,
            recent_searches: Vec::new(),
        };
        store.save(&third).unwrap();
        assert_eq!(store.load(), third);

        let _ = fs::remove_file(root.join("cli-session.json"));
        let _ = fs::remove_dir(root);
    }
}
