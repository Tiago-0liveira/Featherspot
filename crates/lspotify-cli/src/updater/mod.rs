pub mod download;
pub mod github;
pub mod install;

use std::{
    path::{Path, PathBuf},
    sync::mpsc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    #[error("Network error: {0}")]
    Network(String),
    #[error("GitHub API error: status {0}, {1}")]
    GithubApi(u16, String),
    #[error("GitHub API rate limit exceeded. Please try again later or set GH_TOKEN.")]
    RateLimited,
    #[error("No release found")]
    ReleaseNotFound,
    #[error("Invalid version tag: {0}")]
    InvalidVersion(String),
    #[error("Unsupported platform: {os}-{arch}")]
    UnsupportedPlatform { os: String, arch: String },
    #[error("Missing asset '{asset}' for release '{tag}'. Available assets: {available:?}")]
    MissingAsset { asset: String, tag: String, available: Vec<String> },
    #[error("Missing checksums asset in release '{0}'")]
    MissingChecksums(String),
    #[error("Checksum for '{0}' not found in checksums file")]
    ChecksumNotFound(String),
    #[error("Checksum mismatch for '{asset}': expected {expected}, actual {actual}")]
    ChecksumMismatch { asset: String, expected: String, actual: String },
    #[error("Installation error: {0}")]
    Install(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Serialization error: {0}")]
    Serialization(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UpdateCheckResult {
    UpToDate {
        current_version: String,
    },
    UpdateAvailable {
        current_version: String,
        latest_version: String,
        release: github::ReleaseInfo,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UpdateSummary {
    AlreadyUpToDate { current_version: String },
    Updated { previous_version: String, new_version: String },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UpdateState {
    pub last_update_check: String,
    pub latest_version: String,
}

impl UpdateState {
    pub const STATE_FILENAME: &'static str = "update-state.json";

    pub fn new(version: impl Into<String>) -> Self {
        Self {
            last_update_check: system_time_to_iso8601(SystemTime::now()),
            latest_version: version.into(),
        }
    }

    pub fn load_from_dir(dir: &Path) -> Option<Self> {
        let path = dir.join(Self::STATE_FILENAME);
        let contents = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&contents).ok()
    }

    /// Saves the update state to a directory as `update-state.json`.
    ///
    /// # Errors
    /// Returns `std::io::Error` if directory creation or file writing fails.
    pub fn save_to_dir(&self, dir: &Path) -> std::io::Result<()> {
        let _ = std::fs::create_dir_all(dir);
        let path = dir.join(Self::STATE_FILENAME);
        let temp = dir.join(format!("{}.tmp", Self::STATE_FILENAME));
        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(&temp, json)?;
        if path.exists() {
            let _ = std::fs::remove_file(&path);
        }
        std::fs::rename(temp, path)
    }

    pub fn is_fresh(&self, max_age: Duration) -> bool {
        if let Some(parsed) = parse_iso8601(&self.last_update_check)
            && let Ok(elapsed) = SystemTime::now().duration_since(parsed)
        {
            return elapsed < max_age;
        }
        false
    }
}

pub fn system_time_to_iso8601(time: SystemTime) -> String {
    let dur = time.duration_since(UNIX_EPOCH).unwrap_or_default();
    let total_secs = dur.as_secs();
    let sec = total_secs % 60;
    let total_mins = total_secs / 60;
    let min = total_mins % 60;
    let total_hours = total_mins / 60;
    let hour = total_hours % 24;
    let mut days = i64::try_from(total_hours / 24).unwrap_or(0);

    let mut year = 1970;
    loop {
        let leap = is_leap_year(year);
        let days_in_year = if leap { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }

    let leap = is_leap_year(year);
    let month_days = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut month = 1;
    for &d in &month_days {
        if days < d {
            break;
        }
        days -= d;
        month += 1;
    }
    let day = days + 1;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{min:02}:{sec:02}Z")
}

fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

pub fn parse_iso8601(s: &str) -> Option<SystemTime> {
    let trimmed = s.trim();
    if trimmed.len() != 20 || !trimmed.ends_with('Z') || trimmed.as_bytes()[10] != b'T' {
        return None;
    }

    let year: i64 = trimmed[0..4].parse().ok()?;
    let month: usize = trimmed[5..7].parse().ok()?;
    let day: i64 = trimmed[8..10].parse().ok()?;
    let hour: u64 = trimmed[11..13].parse().ok()?;
    let min: u64 = trimmed[14..16].parse().ok()?;
    let sec: u64 = trimmed[17..19].parse().ok()?;

    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour >= 24
        || min >= 60
        || sec >= 60
    {
        return None;
    }

    let mut total_days: i64 = 0;
    for y in 1970..year {
        total_days += if is_leap_year(y) { 366 } else { 365 };
    }

    let leap = is_leap_year(year);
    let month_days = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    for &d in &month_days[..month - 1] {
        total_days += d;
    }
    total_days += day - 1;

    let total_secs = u64::try_from(total_days).ok()? * 86400 + hour * 3600 + min * 60 + sec;
    UNIX_EPOCH.checked_add(Duration::from_secs(total_secs))
}

/// Queries GitHub for the latest release and compares versions.
///
/// # Errors
/// Returns `UpdateError` if fetching release info or version parsing fails.
pub fn check(repo: &str, current_version: &str) -> Result<UpdateCheckResult, UpdateError> {
    let current_semver = github::parse_version(current_version)?;
    let release = github::fetch_latest_release(repo)?;
    let latest_semver = release.parse_version()?;

    if latest_semver > current_semver {
        Ok(UpdateCheckResult::UpdateAvailable {
            current_version: current_version.to_string(),
            latest_version: latest_semver.to_string(),
            release,
        })
    } else {
        Ok(UpdateCheckResult::UpToDate { current_version: current_version.to_string() })
    }
}

/// Downloads and installs an update from GitHub if available.
///
/// # Errors
/// Returns `UpdateError` if check, download, verification, or installation fails.
pub fn update(repo: &str, current_version: &str) -> Result<UpdateSummary, UpdateError> {
    let check_result = check(repo, current_version)?;
    match check_result {
        UpdateCheckResult::UpToDate { current_version } => {
            Ok(UpdateSummary::AlreadyUpToDate { current_version })
        }
        UpdateCheckResult::UpdateAvailable {
            current_version: prev_ver,
            latest_version: new_ver,
            release,
        } => {
            let binary_asset = download::find_asset_for_platform(
                &release,
                std::env::consts::OS,
                std::env::consts::ARCH,
            )?;
            let checksums_asset = download::find_checksums_asset(&release)?;

            let temp_dir = tempfile::Builder::new()
                .prefix("lspotify-update-")
                .tempdir()
                .map_err(UpdateError::Io)?;

            let downloaded_binary_path = temp_dir.path().join(&binary_asset.name);
            let downloaded_checksums_path = temp_dir.path().join(&checksums_asset.name);

            download::download_url_to_path(
                &checksums_asset.browser_download_url,
                &downloaded_checksums_path,
            )?;
            download::download_url_to_path(
                &binary_asset.browser_download_url,
                &downloaded_binary_path,
            )?;

            // Verify checksum
            let checksums_content =
                std::fs::read_to_string(&downloaded_checksums_path).map_err(UpdateError::Io)?;
            let expected_hash = download::parse_checksum(&checksums_content, &binary_asset.name)?;

            let mut bin_file =
                std::fs::File::open(&downloaded_binary_path).map_err(UpdateError::Io)?;
            let actual_hash = download::compute_sha256(&mut bin_file).map_err(UpdateError::Io)?;

            download::verify_checksum(&actual_hash, &expected_hash, &binary_asset.name)?;

            // Current executable
            let current_exe = std::env::current_exe().map_err(UpdateError::Io)?;
            install::safe_replace_executable(&current_exe, &downloaded_binary_path)?;

            #[cfg(target_os = "windows")]
            {
                // In helper mode, keep the temp dir alive so the helper can read replacement
                let _ = temp_dir.keep();
            }

            Ok(UpdateSummary::Updated { previous_version: prev_ver, new_version: new_ver })
        }
    }
}

pub const CHECK_INTERVAL: Duration = Duration::from_hours(24);

pub fn spawn_background_check(
    state_dir: PathBuf,
    current_version: String,
    repo: Option<String>,
    sender: mpsc::Sender<String>,
) {
    let repo = repo
        .or_else(|| std::env::var("LSPOTIFY_REPO").ok())
        .unwrap_or_else(|| github::DEFAULT_REPO.to_string());

    std::thread::Builder::new()
        .name("lspotify-updater".into())
        .spawn(move || {
            if let Some(state) = UpdateState::load_from_dir(&state_dir)
                && state.is_fresh(CHECK_INTERVAL)
            {
                if let (Ok(cached_ver), Ok(curr_ver)) = (
                    github::parse_version(&state.latest_version),
                    github::parse_version(&current_version),
                ) && cached_ver > curr_ver
                {
                    let _ = sender.send(format!(
                        "lspotify v{} available — run `lspotify update`",
                        state.latest_version
                    ));
                }
                return;
            }

            match check(&repo, &current_version) {
                Ok(UpdateCheckResult::UpdateAvailable { latest_version, .. }) => {
                    let new_state = UpdateState::new(&latest_version);
                    let _ = new_state.save_to_dir(&state_dir);
                    let _ = sender.send(format!(
                        "lspotify v{latest_version} available — run `lspotify update`"
                    ));
                }
                Ok(UpdateCheckResult::UpToDate { .. }) => {
                    let new_state = UpdateState::new(&current_version);
                    let _ = new_state.save_to_dir(&state_dir);
                }
                Err(err) => {
                    tracing::debug!(%err, "Background update check skipped or failed");
                }
            }
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso8601_roundtrip() {
        let now = SystemTime::now();
        let formatted = system_time_to_iso8601(now);
        let parsed = parse_iso8601(&formatted).expect("parses correctly");
        let diff =
            now.duration_since(parsed).unwrap_or_else(|_| parsed.duration_since(now).unwrap());
        // Truncated to seconds, difference should be under 1s
        assert!(diff < Duration::from_secs(1));
    }

    #[test]
    fn update_state_persistence_and_freshness() {
        let temp = tempfile::tempdir().unwrap();
        let state = UpdateState::new("0.5.0");

        assert!(state.is_fresh(Duration::from_secs(60)));
        assert!(!state.is_fresh(Duration::from_secs(0)));

        state.save_to_dir(temp.path()).unwrap();
        let loaded = UpdateState::load_from_dir(temp.path()).expect("loads state");

        assert_eq!(loaded.latest_version, "0.5.0");
        assert_eq!(loaded.last_update_check, state.last_update_check);
    }

    #[test]
    fn cached_update_check_prevents_repeated_calls() {
        let temp = tempfile::tempdir().unwrap();
        let cached_state = UpdateState::new("0.5.0");
        cached_state.save_to_dir(temp.path()).unwrap();

        let (tx, rx) = mpsc::channel();
        // Spawning with fresh cache and older current_version (0.4.2)
        spawn_background_check(
            temp.path().to_path_buf(),
            "0.4.2".to_string(),
            Some("invalid/nonexistent-repo".to_string()),
            tx,
        );

        // Notification should arrive from cache without hitting GitHub API (which would fail for invalid repo)
        let notification =
            rx.recv_timeout(Duration::from_secs(2)).expect("should receive cached notification");
        assert!(notification.contains("v0.5.0 available"));
    }

    #[test]
    fn download_failure_returns_error() {
        let temp = tempfile::tempdir().unwrap();
        let dest = temp.path().join("downloaded.bin");
        let result = download::download_url_to_path("http://127.0.0.1:9/nonexistent", &dest);
        assert!(result.is_err());
        assert!(!dest.exists());
    }

    #[test]
    fn checksum_mismatch_prevents_installation() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("lspotify.exe");
        std::fs::write(&target, b"original").unwrap();

        let wrong_hash = "0000000000000000000000000000000000000000000000000000000000000000";
        let actual_hash = "1111111111111111111111111111111111111111111111111111111111111111";

        let check_res = download::verify_checksum(actual_hash, wrong_hash, "lspotify.exe");
        assert!(matches!(check_res, Err(UpdateError::ChecksumMismatch { .. })));

        // Target remains untouched
        assert_eq!(std::fs::read(&target).unwrap(), b"original");
    }
}
