//! Keeps the app up to date from GitHub releases.
//!
//! Checks the latest release, downloads its installer, verifies the SHA-256 checksum GitHub publishes
//! for it, then runs it silently. The installer closes this app, updates it and starts it again.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::version_numbers::compare_versions;

const LATEST_RELEASE_URL: &str = "https://api.github.com/repos/NNKTV28/JoistickMngr/releases/latest";
const INSTALLER_NAME_PREFIX: &str = "PerifeMngr-Setup-";
const SHA256_DIGEST_PREFIX: &str = "sha256:";
const AUTOMATIC_CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
const UPDATES_FOLDER_NAME: &str = "updates";
const USER_AGENT: &str = "PerifeMngr-updater (+https://nikicoding.com)";

#[derive(Clone, Debug, PartialEq)]
pub struct AvailableUpdate {
    pub version: String,
    installer_url: String,
    installer_sha256: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum UpdateState {
    #[default]
    NotChecked,
    Checking,
    UpToDate,
    Available(AvailableUpdate),
    Downloading(AvailableUpdate),
    Failed(String),
}

#[derive(Deserialize)]
struct ReleaseResponse {
    tag_name: String,
    assets: Vec<ReleaseAsset>,
}

#[derive(Deserialize)]
struct ReleaseAsset {
    name: String,
    browser_download_url: String,
    digest: Option<String>,
}

#[derive(Clone, Default)]
pub struct UpdateService {
    state: Arc<Mutex<UpdateState>>,
}

impl UpdateService {
    pub fn current_version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    pub fn state(&self) -> UpdateState {
        self.state.lock().unwrap().clone()
    }

    /// Checks now and then every few hours, for as long as the app runs.
    pub fn start_automatic_checks(&self) {
        let service = self.clone();
        std::thread::Builder::new()
            .name("update-check".into())
            .spawn(move || {
                loop {
                    service.check_now();
                    std::thread::sleep(AUTOMATIC_CHECK_INTERVAL);
                }
            })
            .expect("update check thread starts");
    }

    pub fn start_check(&self) {
        let service = self.clone();
        std::thread::spawn(move || service.check_now());
    }

    /// Downloads and verifies the installer, then hands over to it. The app is closed by the installer.
    pub fn start_install(&self, update: AvailableUpdate) {
        *self.state.lock().unwrap() = UpdateState::Downloading(update.clone());
        let state = Arc::clone(&self.state);
        std::thread::spawn(move || {
            if let Err(error) = download_and_run_installer(&update) {
                *state.lock().unwrap() = UpdateState::Failed(format!("Update failed: {error}"));
            }
        });
    }

    fn check_now(&self) {
        if matches!(self.state(), UpdateState::Downloading(_)) {
            return;
        }
        *self.state.lock().unwrap() = UpdateState::Checking;
        let new_state = match fetch_latest_update() {
            Ok(Some(update)) => UpdateState::Available(update),
            Ok(None) => UpdateState::UpToDate,
            Err(error) => UpdateState::Failed(format!("Could not check for updates: {error}")),
        };
        *self.state.lock().unwrap() = new_state;
    }
}

/// The newest release when it is newer than this build and ships a checksummed installer.
fn fetch_latest_update() -> anyhow::Result<Option<AvailableUpdate>> {
    let release: ReleaseResponse = ureq::get(LATEST_RELEASE_URL)
        .header("User-Agent", USER_AGENT)
        .header("Accept", "application/vnd.github+json")
        .call()?
        .body_mut()
        .read_json()?;
    if compare_versions(&release.tag_name, UpdateService::current_version()).is_le() {
        return Ok(None);
    }
    let installer = release
        .assets
        .iter()
        .find(|asset| asset.name.starts_with(INSTALLER_NAME_PREFIX) && asset.name.ends_with(".exe"))
        .ok_or_else(|| anyhow::anyhow!("the release has no installer"))?;
    let installer_sha256 = installer
        .digest
        .as_deref()
        .and_then(|digest| digest.strip_prefix(SHA256_DIGEST_PREFIX))
        .ok_or_else(|| anyhow::anyhow!("the release does not publish a checksum for its installer"))?;
    Ok(Some(AvailableUpdate {
        version: release.tag_name.trim_start_matches('v').to_owned(),
        installer_url: installer.browser_download_url.clone(),
        installer_sha256: installer_sha256.to_lowercase(),
    }))
}

fn download_and_run_installer(update: &AvailableUpdate) -> anyhow::Result<()> {
    let installer_bytes = ureq::get(&update.installer_url)
        .header("User-Agent", USER_AGENT)
        .call()?
        .body_mut()
        .with_config()
        .limit(200 * 1024 * 1024)
        .read_to_vec()?;
    anyhow::ensure!(
        sha256_hex(&installer_bytes) == update.installer_sha256,
        "the downloaded installer does not match the published checksum"
    );
    let installer_path = installer_download_path(&update.version)?;
    save_as_new_file(&installer_path, &installer_bytes)?;
    run_installer_silently(&installer_path)?;
    std::process::exit(0);
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The installer runs with administrator rights, so it is saved inside the app's own folder
/// (under Program Files), where only administrators can write. A file in %TEMP% could be swapped
/// by any program between the checksum check and the launch.
fn installer_download_path(version: &str) -> anyhow::Result<PathBuf> {
    let application_folder = std::env::current_exe()?
        .parent()
        .ok_or_else(|| anyhow::anyhow!("the application folder is unknown"))?
        .to_path_buf();
    let updates_folder = application_folder.join(UPDATES_FOLDER_NAME);
    std::fs::create_dir_all(&updates_folder)?;
    Ok(updates_folder.join(format!("{INSTALLER_NAME_PREFIX}{version}.exe")))
}

/// Replaces any leftover file and writes the new one, refusing to reuse a file another process created.
fn save_as_new_file(path: &Path, contents: &[u8]) -> anyhow::Result<()> {
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(contents)?;
    Ok(())
}

/// The app already runs elevated, so the installer starts without another administrator prompt.
fn run_installer_silently(installer_path: &Path) -> anyhow::Result<()> {
    Command::new(installer_path)
        .args(["/SILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/CLOSEAPPLICATIONS"])
        .spawn()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::sha256_hex;

    #[test]
    fn checksum_is_lowercase_hex() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
