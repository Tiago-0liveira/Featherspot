use std::{path::Path, time::Duration};

use sha2::{Digest, Sha256};

use super::{
    UpdateError,
    github::{ReleaseAsset, ReleaseInfo},
};

/// Maps an OS and architecture to the expected binary asset name.
///
/// # Errors
/// Returns `UpdateError::UnsupportedPlatform` if the OS or architecture combination is unsupported.
pub fn target_asset_name(os: &str, arch: &str) -> Result<String, UpdateError> {
    match (os, arch) {
        ("windows", "x86_64") => Ok("featherspot-windows-x86_64.exe".to_string()),
        ("linux", "x86_64") => Ok("featherspot-linux-x86_64".to_string()),
        ("macos", "x86_64") => Ok("featherspot-macos-x86_64".to_string()),
        ("macos", "aarch64" | "arm64") => Ok("featherspot-macos-aarch64".to_string()),
        _ => Err(UpdateError::UnsupportedPlatform { os: os.to_string(), arch: arch.to_string() }),
    }
}

/// Returns the expected asset name for the current running platform.
///
/// # Errors
/// Returns `UpdateError::UnsupportedPlatform` if current platform is unsupported.
pub fn current_platform_asset_name() -> Result<String, UpdateError> {
    target_asset_name(std::env::consts::OS, std::env::consts::ARCH)
}

/// Locates the release asset matching the target operating system and architecture.
///
/// # Errors
/// Returns `UpdateError::MissingAsset` if no compatible asset is found in `release`.
pub fn find_asset_for_platform<'a>(
    release: &'a ReleaseInfo,
    os: &str,
    arch: &str,
) -> Result<&'a ReleaseAsset, UpdateError> {
    let expected = target_asset_name(os, arch)?;
    if let Some(asset) = release.assets.iter().find(|a| a.name == expected) {
        return Ok(asset);
    }
    let available = release.assets.iter().map(|a| a.name.clone()).collect();
    Err(UpdateError::MissingAsset { asset: expected, tag: release.tag_name.clone(), available })
}

/// Locates the checksums file in a release.
///
/// # Errors
/// Returns `UpdateError::MissingChecksums` if no checksum file is found in `release`.
pub fn find_checksums_asset(release: &ReleaseInfo) -> Result<&ReleaseAsset, UpdateError> {
    release
        .assets
        .iter()
        .find(|a| {
            a.name.eq_ignore_ascii_case("checksums.txt")
                || a.name.eq_ignore_ascii_case("sha256sums.txt")
        })
        .ok_or_else(|| UpdateError::MissingChecksums(release.tag_name.clone()))
}

/// Parses a SHA-256 checksum for `target_filename` from checksums file content.
///
/// # Errors
/// Returns `UpdateError::ChecksumNotFound` if no checksum entry for `target_filename` is found.
pub fn parse_checksum(content: &str, target_filename: &str) -> Result<String, UpdateError> {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // Standard GNU format: <hash>  <filename> or <hash> *<filename>
        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.len() >= 2 {
            let hash = parts[0];
            let filename = parts[1].strip_prefix('*').unwrap_or(parts[1]);
            if filename.eq_ignore_ascii_case(target_filename) && is_hex_sha256(hash) {
                return Ok(hash.to_ascii_lowercase());
            }
        }

        // BSD format: SHA256 (<filename>) = <hash>
        if let Some(rest) =
            trimmed.strip_prefix("SHA256 (").or_else(|| trimmed.strip_prefix("SHA256("))
            && let Some((fname, hash_part)) = rest.split_once(')')
            && fname.trim().eq_ignore_ascii_case(target_filename)
            && let Some(hash) = hash_part.trim().strip_prefix('=').map(str::trim)
            && is_hex_sha256(hash)
        {
            return Ok(hash.to_ascii_lowercase());
        }
    }

    Err(UpdateError::ChecksumNotFound(target_filename.to_string()))
}

fn is_hex_sha256(s: &str) -> bool {
    s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit())
}

/// Computes the SHA-256 hash for data from a reader.
///
/// # Errors
/// Returns an `std::io::Error` if reading from `reader` fails.
pub fn compute_sha256(reader: &mut impl std::io::Read) -> std::io::Result<String> {
    let mut hasher = Sha256::new();
    std::io::copy(reader, &mut hasher)?;
    let result = hasher.finalize();
    Ok(format!("{result:x}"))
}

/// Verifies whether `actual` checksum matches `expected` checksum.
///
/// # Errors
/// Returns `UpdateError::ChecksumMismatch` if the hashes do not match.
pub fn verify_checksum(actual: &str, expected: &str, asset_name: &str) -> Result<(), UpdateError> {
    if actual.trim().eq_ignore_ascii_case(expected.trim()) {
        Ok(())
    } else {
        Err(UpdateError::ChecksumMismatch {
            asset: asset_name.to_string(),
            expected: expected.trim().to_ascii_lowercase(),
            actual: actual.trim().to_ascii_lowercase(),
        })
    }
}

/// Downloads a remote URL directly to a local file destination.
///
/// # Errors
/// Returns `UpdateError::Network` or `UpdateError::Io` if download or saving fails.
pub fn download_url_to_path(url: &str, destination: &Path) -> Result<(), UpdateError> {
    let client = reqwest::blocking::Client::builder()
        .user_agent(format!("featherspot-updater/{}", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|err| UpdateError::Network(err.to_string()))?;

    let mut response =
        client.get(url).send().map_err(|err| UpdateError::Network(err.to_string()))?;

    if !response.status().is_success() {
        return Err(UpdateError::Network(format!(
            "Failed to download {url}: HTTP {}",
            response.status()
        )));
    }

    let mut file = std::fs::File::create(destination).map_err(UpdateError::Io)?;
    response.copy_to(&mut file).map_err(|err| UpdateError::Network(err.to_string()))?;
    file.sync_all().map_err(UpdateError::Io)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_selection_mapping() {
        assert_eq!(
            target_asset_name("windows", "x86_64").unwrap(),
            "featherspot-windows-x86_64.exe"
        );
        assert_eq!(target_asset_name("linux", "x86_64").unwrap(), "featherspot-linux-x86_64");
        assert_eq!(target_asset_name("macos", "x86_64").unwrap(), "featherspot-macos-x86_64");
        assert_eq!(target_asset_name("macos", "aarch64").unwrap(), "featherspot-macos-aarch64");
        assert_eq!(target_asset_name("macos", "arm64").unwrap(), "featherspot-macos-aarch64");

        assert!(target_asset_name("unknown_os", "x86_64").is_err());
        assert!(target_asset_name("linux", "armv7").is_err());
    }

    #[test]
    fn find_platform_and_checksum_assets() {
        let release = ReleaseInfo {
            tag_name: "v0.5.0".to_string(),
            name: None,
            draft: false,
            prerelease: false,
            assets: vec![
                ReleaseAsset {
                    name: "featherspot-windows-x86_64.exe".to_string(),
                    browser_download_url: "https://example.com/win".to_string(),
                    size: 100,
                },
                ReleaseAsset {
                    name: "featherspot-linux-x86_64".to_string(),
                    browser_download_url: "https://example.com/linux".to_string(),
                    size: 100,
                },
                ReleaseAsset {
                    name: "checksums.txt".to_string(),
                    browser_download_url: "https://example.com/checksums".to_string(),
                    size: 100,
                },
            ],
        };

        let win_asset = find_asset_for_platform(&release, "windows", "x86_64").unwrap();
        assert_eq!(win_asset.name, "featherspot-windows-x86_64.exe");

        let linux_asset = find_asset_for_platform(&release, "linux", "x86_64").unwrap();
        assert_eq!(linux_asset.name, "featherspot-linux-x86_64");

        let mac_err = find_asset_for_platform(&release, "macos", "x86_64").unwrap_err();
        assert!(matches!(mac_err, UpdateError::MissingAsset { .. }));

        let checksums = find_checksums_asset(&release).unwrap();
        assert_eq!(checksums.name, "checksums.txt");
    }

    #[test]
    fn parse_checksum_formats() {
        let file_content = r"
# SHA256 checksums
ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  featherspot-linux-x86_64
cb832e18842da70078d179c7993b6577f440274904a5f7e8a60731a4395b28e9 *featherspot-windows-x86_64.exe
SHA256 (featherspot-macos-x86_64) = 2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824
";

        let linux_hash = parse_checksum(file_content, "featherspot-linux-x86_64").unwrap();
        assert_eq!(linux_hash, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");

        let win_hash = parse_checksum(file_content, "featherspot-windows-x86_64.exe").unwrap();
        assert_eq!(win_hash, "cb832e18842da70078d179c7993b6577f440274904a5f7e8a60731a4395b28e9");

        let mac_hash = parse_checksum(file_content, "featherspot-macos-x86_64").unwrap();
        assert_eq!(mac_hash, "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824");

        assert!(parse_checksum(file_content, "missing-file.bin").is_err());
    }

    #[test]
    fn compute_and_verify_sha256() {
        let data = b"hello featherspot";
        let mut reader = &data[..];
        let hash = compute_sha256(&mut reader).unwrap();
        // SHA-256 of "hello featherspot"
        let expected = "7d7074016d837247774dbe0899f0ebc1990cb6b3ebe03c04f78cff8815343392";
        assert_eq!(hash, expected);

        assert!(verify_checksum(&hash, expected, "test.bin").is_ok());
        assert!(verify_checksum(&hash, &expected.to_uppercase(), "test.bin").is_ok());

        let mismatch = verify_checksum(
            &hash,
            "0000000000000000000000000000000000000000000000000000000000000000",
            "test.bin",
        );
        assert!(matches!(mismatch, Err(UpdateError::ChecksumMismatch { .. })));
    }
}
