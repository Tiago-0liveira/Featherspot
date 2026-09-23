use std::time::Duration;

use semver::Version;
use serde::{Deserialize, Serialize};

use super::UpdateError;

pub const DEFAULT_REPO: &str = "Tiago-0liveira/Featherspot";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReleaseAsset {
    pub name: String,
    pub browser_download_url: String,
    #[serde(default)]
    pub size: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReleaseInfo {
    pub tag_name: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub prerelease: bool,
    #[serde(default)]
    pub assets: Vec<ReleaseAsset>,
}

impl ReleaseInfo {
    /// Parses the semantic version from the release tag.
    ///
    /// # Errors
    /// Returns `UpdateError::InvalidVersion` if the tag name is not valid semantic versioning.
    pub fn parse_version(&self) -> Result<Version, UpdateError> {
        parse_version(&self.tag_name)
    }
}

/// Parses a version string, stripping any leading 'v' or 'V'.
///
/// # Errors
/// Returns `UpdateError::InvalidVersion` if `tag` is not valid semantic versioning.
pub fn parse_version(tag: &str) -> Result<Version, UpdateError> {
    let trimmed = tag.trim();
    let cleaned = trimmed.strip_prefix(['v', 'V']).unwrap_or(trimmed);
    Version::parse(cleaned).map_err(|err| UpdateError::InvalidVersion(format!("{tag}: {err}")))
}

/// Parses a GitHub release response JSON string.
///
/// # Errors
/// Returns `UpdateError::Serialization` if JSON deserialization fails.
pub fn parse_release_response(json_str: &str) -> Result<ReleaseInfo, UpdateError> {
    serde_json::from_str(json_str)
        .map_err(|err| UpdateError::Serialization(format!("Invalid GitHub release JSON: {err}")))
}

/// Fetches the latest release from the specified GitHub repository.
///
/// # Errors
/// Returns an error if the request fails, the release is not found, or rate limit is exceeded.
pub fn fetch_latest_release(repo: &str) -> Result<ReleaseInfo, UpdateError> {
    let url = format!("https://api.github.com/repos/{repo}/releases/latest");
    fetch_release_from_url(&url)
}

/// Fetches release information from a specific URL.
///
/// # Errors
/// Returns an error if network connection fails, HTTP response is not successful, or rate limit is hit.
pub fn fetch_release_from_url(url: &str) -> Result<ReleaseInfo, UpdateError> {
    let client = reqwest::blocking::Client::builder()
        .user_agent(format!("featherspot-updater/{}", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|err| UpdateError::Network(err.to_string()))?;

    let mut request = client.get(url).header("Accept", "application/vnd.github+json");

    if let Ok(token) = std::env::var("GH_TOKEN").or_else(|_| std::env::var("GITHUB_TOKEN"))
        && !token.trim().is_empty()
    {
        request = request.header("Authorization", format!("Bearer {}", token.trim()));
    }

    let response = request.send().map_err(|err| UpdateError::Network(err.to_string()))?;
    let status = response.status();

    if status.as_u16() == 403 || status.as_u16() == 429 {
        return Err(UpdateError::RateLimited);
    }
    if status.as_u16() == 404 {
        return Err(UpdateError::ReleaseNotFound);
    }
    if !status.is_success() {
        let body = response.text().unwrap_or_default();
        return Err(UpdateError::GithubApi(status.as_u16(), body));
    }

    let body = response.text().map_err(|err| UpdateError::Network(err.to_string()))?;
    parse_release_response(&body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_comparisons_and_semantics() {
        let v0_4_9 = parse_version("0.4.9").expect("valid version");
        let v0_5_0 = parse_version("0.5.0").expect("valid version");
        let v0_9_9 = parse_version("0.9.9").expect("valid version");
        let v1_0_0 = parse_version("1.0.0").expect("valid version");
        let v1_0_0_dup = parse_version("v1.0.0").expect("valid version");

        assert!(v0_4_9 < v0_5_0);
        assert!(v0_9_9 < v1_0_0);
        assert_eq!(v1_0_0, v1_0_0_dup);
    }

    #[test]
    fn prerelease_handling() {
        let rc = parse_version("v0.5.0-rc.1").expect("valid rc");
        let release = parse_version("0.5.0").expect("valid release");
        let beta1 = parse_version("0.5.0-beta.1").expect("valid beta 1");
        let beta2 = parse_version("0.5.0-beta.2").expect("valid beta 2");

        assert!(rc < release);
        assert!(beta1 < beta2);
        assert!(beta2 < rc);
    }

    #[test]
    fn invalid_version_fails() {
        assert!(parse_version("invalid").is_err());
        assert!(parse_version("v.1.2").is_err());
    }

    #[test]
    fn parse_valid_release_json() {
        let json = r#"{
            "tag_name": "v0.5.0",
            "name": "Release 0.5.0",
            "draft": false,
            "prerelease": false,
            "assets": [
                {
                    "name": "featherspot-windows-x86_64.exe",
                    "browser_download_url": "https://example.com/featherspot.exe",
                    "size": 1024
                },
                {
                    "name": "checksums.txt",
                    "browser_download_url": "https://example.com/checksums.txt",
                    "size": 64
                }
            ]
        }"#;

        let release = parse_release_response(json).expect("parses release");
        assert_eq!(release.tag_name, "v0.5.0");
        assert_eq!(release.parse_version().unwrap(), Version::new(0, 5, 0));
        assert_eq!(release.assets.len(), 2);
        assert_eq!(release.assets[0].name, "featherspot-windows-x86_64.exe");
    }

    #[test]
    fn parse_invalid_release_json_fails() {
        let json = r#"{"invalid": true}"#;
        assert!(parse_release_response(json).is_err());
    }
}
