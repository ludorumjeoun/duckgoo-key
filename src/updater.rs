//! Secure, user-approved in-place updates for the signed macOS application.
//!
//! The mutable manifest is deliberately small and served with no-cache headers.
//! Release ZIP files are immutable, hash-checked before use, and their expanded
//! application bundle is checked again by macOS before replacing the running app.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use directories::ProjectDirs;
use reqwest::redirect::Policy;
use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use thiserror::Error;
use url::Url;

pub const UPDATE_MANIFEST_URL: &str = "https://updates.key.duckgoo.net/latest.json";
const UPDATE_HOST: &str = "updates.key.duckgoo.net";
const MAX_MANIFEST_BYTES: usize = 1024 * 1024;
const MAX_ARCHIVE_BYTES: usize = 512 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AvailableUpdate {
    pub version: String,
    platform: &'static str,
    archive_url: Url,
    archive_sha256: String,
}

#[cfg(test)]
impl AvailableUpdate {
    pub(crate) fn for_test(version: impl Into<String>) -> Self {
        Self {
            version: version.into(),
            platform: "macos-aarch64",
            archive_url: Url::parse("https://updates.key.duckgoo.net/test.app.zip")
                .expect("the test update URL must be valid"),
            archive_sha256: "0".repeat(64),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CheckResult {
    UpToDate,
    Available(AvailableUpdate),
}

#[derive(Debug, Error)]
pub enum UpdateError {
    #[error("the update service returned an unexpected response: {0}")]
    Http(String),
    #[error("the update manifest is too large")]
    ManifestTooLarge,
    #[error("the update manifest is invalid: {0}")]
    InvalidManifest(String),
    #[error("the downloaded update is too large")]
    ArchiveTooLarge,
    #[error("the downloaded update failed its SHA-256 check")]
    ChecksumMismatch,
    #[error("the update cannot be installed from this development location")]
    NotInstalledApplication,
    #[error("the current app signing identity could not be verified")]
    CurrentSigningIdentityUnavailable,
    #[error("the app's installation folder is not writable: {0}")]
    InstallationFolderUnavailable(String),
    #[error("automatic installation is supported only on macOS")]
    UnsupportedPlatform,
    #[error("local update storage is unavailable")]
    UpdateDirectoryUnavailable,
    #[error("could not prepare the update: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Deserialize)]
struct ReleaseManifest {
    version: String,
    platforms: std::collections::HashMap<String, PlatformRelease>,
}

#[derive(Debug, Deserialize)]
struct PlatformRelease {
    url: String,
    sha256: String,
    /// v0.1.0 manifests predate the in-place updater and intentionally have no
    /// application archive. Keep parsing them compatible so an already newer
    /// app can correctly report itself as up to date.
    #[serde(default)]
    app_url: Option<String>,
    #[serde(default)]
    app_sha256: Option<String>,
}

pub async fn check() -> Result<CheckResult, UpdateError> {
    let manifest_url = Url::parse(UPDATE_MANIFEST_URL)
        .map_err(|error| UpdateError::InvalidManifest(error.to_string()))?;
    let client = update_client()?;
    let response = client
        .get(manifest_url)
        .header(reqwest::header::CACHE_CONTROL, "no-cache")
        .send()
        .await
        .map_err(|error| UpdateError::Http(error.to_string()))?
        .error_for_status()
        .map_err(|error| UpdateError::Http(error.to_string()))?;

    if response
        .content_length()
        .is_some_and(|length| length > MAX_MANIFEST_BYTES as u64)
    {
        return Err(UpdateError::ManifestTooLarge);
    }
    let body = response
        .bytes()
        .await
        .map_err(|error| UpdateError::Http(error.to_string()))?;
    if body.len() > MAX_MANIFEST_BYTES {
        return Err(UpdateError::ManifestTooLarge);
    }

    check_manifest(&body, env!("CARGO_PKG_VERSION"), current_platform()?)
}

fn check_manifest(
    body: &[u8],
    current_version_raw: &str,
    platform: &'static str,
) -> Result<CheckResult, UpdateError> {
    let manifest: ReleaseManifest = serde_json::from_slice(body)
        .map_err(|error| UpdateError::InvalidManifest(error.to_string()))?;
    let remote_version = Version::parse(&manifest.version)
        .map_err(|error| UpdateError::InvalidManifest(format!("invalid version: {error}")))?;
    let current_version =
        Version::parse(current_version_raw).expect("Cargo package version must be valid SemVer");

    if remote_version.cmp_precedence(&current_version).is_le() {
        return Ok(CheckResult::UpToDate);
    }

    let release = manifest.platforms.get(platform).ok_or_else(|| {
        UpdateError::InvalidManifest(format!("missing platform release for {platform}"))
    })?;
    let (app_url, app_sha256) = validate_release(platform, &manifest.version, release)?;

    Ok(CheckResult::Available(AvailableUpdate {
        version: manifest.version,
        platform,
        archive_url: Url::parse(app_url)
            .map_err(|error| UpdateError::InvalidManifest(error.to_string()))?,
        archive_sha256: app_sha256.to_owned(),
    }))
}

pub async fn download_and_prepare_install(update: AvailableUpdate) -> Result<(), UpdateError> {
    #[cfg(not(target_os = "macos"))]
    {
        let _ = update;
        return Err(UpdateError::UnsupportedPlatform);
    }

    #[cfg(target_os = "macos")]
    {
        let app_bundle = current_app_bundle()?;
        let team_identifier = current_team_identifier(&app_bundle)?;
        ensure_parent_is_writable(&app_bundle)?;
        let archive = download_archive(&update).await?;
        spawn_install_helper(&archive, &app_bundle, &team_identifier)
    }
}

fn update_client() -> Result<reqwest::Client, UpdateError> {
    reqwest::Client::builder()
        .https_only(true)
        .redirect(Policy::limited(3))
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(90))
        .build()
        .map_err(|error| UpdateError::Http(error.to_string()))
}

fn current_platform() -> Result<&'static str, UpdateError> {
    match std::env::consts::ARCH {
        "aarch64" => Ok("macos-aarch64"),
        "x86_64" => Ok("macos-x86_64"),
        architecture => Err(UpdateError::InvalidManifest(format!(
            "updates are unavailable for {architecture}"
        ))),
    }
}

fn validate_release<'a>(
    platform: &str,
    version: &str,
    release: &'a PlatformRelease,
) -> Result<(&'a str, &'a str), UpdateError> {
    validate_artifact_url(platform, version, &release.url, ".dmg")?;
    validate_sha256(&release.sha256)?;
    let app_url = release
        .app_url
        .as_deref()
        .ok_or_else(|| UpdateError::InvalidManifest(format!("missing app_url for {platform}")))?;
    let app_sha256 = release.app_sha256.as_deref().ok_or_else(|| {
        UpdateError::InvalidManifest(format!("missing app_sha256 for {platform}"))
    })?;
    validate_artifact_url(platform, version, app_url, ".app.zip")?;
    validate_sha256(app_sha256)?;
    Ok((app_url, app_sha256))
}

fn validate_artifact_url(
    platform: &str,
    version: &str,
    value: &str,
    suffix: &str,
) -> Result<(), UpdateError> {
    let url = Url::parse(value).map_err(|error| UpdateError::InvalidManifest(error.to_string()))?;
    if url.scheme() != "https"
        || url.host_str() != Some(UPDATE_HOST)
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(UpdateError::InvalidManifest(
            "artifact URL must use the DuckGooKey HTTPS update host".to_owned(),
        ));
    }
    let expected_path = format!("/releases/v{version}/DuckGooKey-{version}-{platform}{suffix}");
    if url.path() != expected_path {
        return Err(UpdateError::InvalidManifest(format!(
            "artifact URL does not match the versioned release path: {}",
            url.path()
        )));
    }
    Ok(())
}

fn validate_sha256(value: &str) -> Result<(), UpdateError> {
    if value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        Ok(())
    } else {
        Err(UpdateError::InvalidManifest(
            "SHA-256 must be 64 lowercase hexadecimal characters".to_owned(),
        ))
    }
}

async fn download_archive(update: &AvailableUpdate) -> Result<PathBuf, UpdateError> {
    let directory = update_directory()?;
    let archive_name = format!("DuckGooKey-{}-{}.app.zip", update.version, update.platform);
    let archive = directory.join(archive_name);

    if archive.is_file() && checksum_matches(&archive, &update.archive_sha256)? {
        return Ok(archive);
    }

    let client = update_client()?;
    let response = client
        .get(update.archive_url.clone())
        .send()
        .await
        .map_err(|error| UpdateError::Http(error.to_string()))?
        .error_for_status()
        .map_err(|error| UpdateError::Http(error.to_string()))?;
    if response
        .content_length()
        .is_some_and(|length| length > MAX_ARCHIVE_BYTES as u64)
    {
        return Err(UpdateError::ArchiveTooLarge);
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|error| UpdateError::Http(error.to_string()))?;
    if bytes.len() > MAX_ARCHIVE_BYTES {
        return Err(UpdateError::ArchiveTooLarge);
    }
    if sha256_hex(&bytes) != update.archive_sha256 {
        return Err(UpdateError::ChecksumMismatch);
    }

    write_private_file(&archive, &bytes)?;
    Ok(archive)
}

fn update_directory() -> Result<PathBuf, UpdateError> {
    let directories = ProjectDirs::from("com", "DuckGoo", "DuckGooKey")
        .ok_or(UpdateError::UpdateDirectoryUnavailable)?;
    let directory = directories.data_local_dir().join("updates");
    fs::create_dir_all(&directory)?;
    set_private_directory_permissions(&directory)?;
    Ok(directory)
}

fn checksum_matches(path: &Path, expected: &str) -> Result<bool, UpdateError> {
    Ok(sha256_hex(&fs::read(path)?) == expected)
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn write_private_file(path: &Path, bytes: &[u8]) -> Result<(), UpdateError> {
    let temporary = path.with_extension(format!("part-{}", std::process::id()));
    let _ = fs::remove_file(&temporary);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    set_private_file_permissions(&temporary)?;
    fs::rename(temporary, path)?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn current_app_bundle() -> Result<PathBuf, UpdateError> {
    let executable = std::env::current_exe()?;
    executable
        .ancestors()
        .find(|path| {
            path.file_name()
                .is_some_and(|name| name == "DuckGooKey.app")
        })
        .map(Path::to_path_buf)
        .ok_or(UpdateError::NotInstalledApplication)
}

#[cfg(target_os = "macos")]
fn current_team_identifier(app_bundle: &Path) -> Result<String, UpdateError> {
    let output = std::process::Command::new("/usr/bin/codesign")
        .args(["-dv", "--verbose=4"])
        .arg(app_bundle)
        .output()
        .map_err(|_| UpdateError::CurrentSigningIdentityUnavailable)?;
    if !output.status.success() {
        return Err(UpdateError::CurrentSigningIdentityUnavailable);
    }
    let details = String::from_utf8_lossy(&output.stderr);
    let team_identifier = details
        .lines()
        .find_map(|line| line.strip_prefix("TeamIdentifier="))
        .filter(|value| {
            value.len() == 10
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
        })
        .map(ToOwned::to_owned);
    team_identifier.ok_or(UpdateError::CurrentSigningIdentityUnavailable)
}

#[cfg(target_os = "macos")]
fn ensure_parent_is_writable(app_bundle: &Path) -> Result<(), UpdateError> {
    let parent = app_bundle
        .parent()
        .ok_or(UpdateError::NotInstalledApplication)?;
    let probe = parent.join(format!(".duckgookey-update-probe-{}", std::process::id()));
    fs::create_dir(&probe).map_err(|error| {
        UpdateError::InstallationFolderUnavailable(format!("{} ({error})", parent.display()))
    })?;
    fs::remove_dir(&probe)?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn spawn_install_helper(
    archive: &Path,
    app_bundle: &Path,
    team_identifier: &str,
) -> Result<(), UpdateError> {
    let directory = update_directory()?;
    let helper = directory.join(format!("install-{}.sh", std::process::id()));
    write_private_file(&helper, INSTALL_HELPER.as_bytes())?;
    make_executable(&helper)?;

    std::process::Command::new("/bin/sh")
        .arg(helper)
        .arg(std::process::id().to_string())
        .arg(archive)
        .arg(app_bundle)
        .arg(team_identifier)
        .spawn()
        .map_err(UpdateError::Io)?;
    Ok(())
}

#[cfg(target_os = "macos")]
const INSTALL_HELPER: &str = r#"#!/bin/sh
set -eu

pid="$1"
archive="$2"
target="$3"
expected_team="$4"
parent="$(dirname "$target")"
stage="$parent/.duckgookey-update-$pid"
replacement="$stage/DuckGooKey.app"
backup="$parent/.DuckGooKey.previous.app"

notify_failure() {
  /usr/bin/osascript -e "display notification \"DuckGooKey update could not be installed.\" with title \"DuckGooKey\"" >/dev/null 2>&1 || true
}

cleanup() {
  /bin/rm -rf "$stage"
}
trap cleanup EXIT

attempt=0
while /bin/kill -0 "$pid" >/dev/null 2>&1; do
  attempt=$((attempt + 1))
  if [ "$attempt" -gt 60 ]; then
    notify_failure
    exit 1
  fi
  /bin/sleep 1
done

/bin/rm -rf "$stage"
/bin/mkdir -p "$stage"
if ! /usr/bin/ditto -x -k "$archive" "$stage"; then
  notify_failure
  exit 1
fi
if [ ! -d "$replacement" ]; then
  notify_failure
  exit 1
fi
if ! /usr/bin/codesign --verify --deep --strict "$replacement" >/dev/null 2>&1; then
  notify_failure
  exit 1
fi
actual_team="$(/usr/bin/codesign -dv --verbose=4 "$replacement" 2>&1 | /usr/bin/awk -F= '/^TeamIdentifier=/{print $2; exit}')"
if [ "$actual_team" != "$expected_team" ]; then
  notify_failure
  exit 1
fi
if ! /usr/sbin/spctl --assess --type execute "$replacement" >/dev/null 2>&1; then
  notify_failure
  exit 1
fi

/bin/rm -rf "$backup"
if ! /bin/mv "$target" "$backup"; then
  notify_failure
  exit 1
fi
if ! /bin/mv "$replacement" "$target"; then
  /bin/mv "$backup" "$target" || true
  notify_failure
  exit 1
fi
if ! /usr/bin/open -n "$target"; then
  notify_failure
  exit 1
fi
"#;

#[cfg(unix)]
fn set_private_directory_permissions(path: &Path) -> Result<(), UpdateError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_private_directory_permissions(_path: &Path) -> Result<(), UpdateError> {
    Ok(())
}

#[cfg(unix)]
fn set_private_file_permissions(path: &Path) -> Result<(), UpdateError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_private_file_permissions(_path: &Path) -> Result<(), UpdateError> {
    Ok(())
}

#[cfg(target_os = "macos")]
fn make_executable(path: &Path) -> Result<(), UpdateError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_validation_rejects_uppercase_and_wrong_lengths() {
        assert!(validate_sha256(&"a".repeat(64)).is_ok());
        assert!(validate_sha256(&"A".repeat(64)).is_err());
        assert!(validate_sha256("a0").is_err());
    }

    #[test]
    fn artifact_url_must_be_the_expected_immutable_release_path() {
        assert!(validate_artifact_url(
            "macos-aarch64",
            "1.2.3",
            "https://updates.key.duckgoo.net/releases/v1.2.3/DuckGooKey-1.2.3-macos-aarch64.app.zip",
            ".app.zip",
        )
        .is_ok());
        assert!(
            validate_artifact_url(
                "macos-aarch64",
                "1.2.3",
                "https://example.com/releases/v1.2.3/DuckGooKey-1.2.3-macos-aarch64.app.zip",
                ".app.zip",
            )
            .is_err()
        );
    }

    #[test]
    fn sha256_digest_is_stable() {
        assert_eq!(
            sha256_hex(b"DuckGooKey"),
            "e50982a5d4b087a0ccfa5beb51a06678ccec66fcb36526972e4fcf041677d3f7"
        );
    }

    #[test]
    fn legacy_manifest_without_app_archives_is_up_to_date_for_a_newer_app() {
        let manifest = br#"{
          "version": "0.1.0",
          "platforms": {
            "macos-aarch64": {
              "url": "https://updates.key.duckgoo.net/releases/v0.1.0/DuckGooKey-0.1.0-macos-aarch64.dmg",
              "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            }
          }
        }"#;

        assert_eq!(
            check_manifest(manifest, "0.1.1", "macos-aarch64").unwrap(),
            CheckResult::UpToDate
        );
    }

    #[test]
    fn newer_legacy_manifest_explains_that_it_cannot_update() {
        let manifest = br#"{
          "version": "0.1.2",
          "platforms": {
            "macos-aarch64": {
              "url": "https://updates.key.duckgoo.net/releases/v0.1.2/DuckGooKey-0.1.2-macos-aarch64.dmg",
              "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            }
          }
        }"#;

        assert!(matches!(
            check_manifest(manifest, "0.1.1", "macos-aarch64"),
            Err(UpdateError::InvalidManifest(message)) if message == "missing app_url for macos-aarch64"
        ));
    }
}
