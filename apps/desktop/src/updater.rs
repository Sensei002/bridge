use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;

#[derive(Debug)]
pub enum UpdateEvent {
    Status(String),
    Available { version: String, installer: PathBuf },
}

const API: &str = "https://api.github.com/repos/Sensei002/bridge/releases?per_page=30";
const RELEASE_PREFIX: &str = "https://github.com/Sensei002/bridge/releases/download/";

pub fn check_for_update(channel: String, sender: Sender<UpdateEvent>) {
    let _ = sender.send(UpdateEvent::Status(format!("Checking the {channel} channel…")));
    thread::Builder::new()
        .name("bridge-update-check".into())
        .spawn(move || {
            match check_and_download(&channel) {
                Ok(Some((version, installer))) => {
                    let _ = sender.send(UpdateEvent::Available { version, installer });
                }
                Ok(None) => {
                    let _ = sender.send(UpdateEvent::Status("Bridge is up to date.".into()));
                }
                Err(error) => {
                    let _ = sender.send(UpdateEvent::Status(format!("Update check failed: {error}")));
                }
            }
        })
        .ok();
}

fn check_and_download(channel: &str) -> Result<Option<(String, PathBuf)>, String> {
    let development = channel == "development";
    let client = reqwest::blocking::Client::builder()
        .user_agent("Bridge-Windows-Updater")
        .timeout(Duration::from_secs(90))
        .build()
        .map_err(|error| error.to_string())?;

    let releases: serde_json::Value = client.get(API).send()
        .map_err(|error| error.to_string())?
        .error_for_status().map_err(|error| error.to_string())?
        .json().map_err(|error| error.to_string())?;
    let release = releases.as_array()
        .and_then(|items| items.iter().find(|item| {
            !item["draft"].as_bool().unwrap_or(true)
                && item["prerelease"].as_bool().unwrap_or(false) == development
        }))
        .ok_or_else(|| format!("No {channel} release is available yet."))?;

    let assets = release["assets"].as_array().ok_or("Release assets are missing")?;
    let manifest_url = assets.iter()
        .find(|asset| asset["name"].as_str() == Some("bridge-update.json"))
        .and_then(|asset| asset["browser_download_url"].as_str())
        .ok_or("Release update manifest is missing")?;
    if !manifest_url.starts_with(RELEASE_PREFIX) {
        return Err("Manifest URL is not an official Bridge release asset".into());
    }
    let manifest: serde_json::Value = client.get(manifest_url).send()
        .map_err(|error| error.to_string())?
        .error_for_status().map_err(|error| error.to_string())?
        .json().map_err(|error| error.to_string())?;
    if manifest["channel"].as_str() != Some(channel) {
        return Err("Release channel does not match the selected channel".into());
    }

    let windows = &manifest["windows"];
    let version = windows["version"].as_str().ok_or("Windows version is missing")?.to_owned();
    if compare_versions(&version, option_env!("BRIDGE_BUILD_VERSION").unwrap_or(env!("CARGO_PKG_VERSION")))? != std::cmp::Ordering::Greater {
        return Ok(None);
    }
    let download_url = windows["download_url"].as_str().ok_or("MSI download URL is missing")?;
    if !download_url.starts_with(RELEASE_PREFIX) {
        return Err("MSI URL is not an official Bridge release asset".into());
    }
    let expected_hash = windows["sha256"].as_str().ok_or("MSI checksum is missing")?.to_ascii_lowercase();
    if expected_hash.len() != 64 || !expected_hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("MSI checksum is invalid".into());
    }
    let asset = windows["asset"].as_str().ok_or("MSI asset name is missing")?;
    if asset.contains('/') || asset.contains('\\') || !asset.to_ascii_lowercase().ends_with(".msi") {
        return Err("MSI asset filename is invalid".into());
    }

    let bytes = client.get(download_url).send()
        .map_err(|error| error.to_string())?
        .error_for_status().map_err(|error| error.to_string())?
        .bytes().map_err(|error| error.to_string())?;
    let actual_hash = format!("{:x}", Sha256::digest(&bytes));
    if actual_hash != expected_hash {
        return Err("MSI SHA-256 verification failed; installation was blocked".into());
    }

    let directory = std::env::temp_dir().join("Bridge").join("Updates");
    std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let installer = directory.join(asset);
    std::fs::write(&installer, &bytes).map_err(|error| error.to_string())?;
    Ok(Some((version, installer)))
}

fn compare_versions(left: &str, right: &str) -> Result<std::cmp::Ordering, String> {
    fn parts(value: &str) -> Result<Vec<u64>, String> {
        value.split('.').map(|part| part.parse::<u64>().map_err(|_| format!("Invalid version: {value}"))).collect()
    }
    let mut a = parts(left)?;
    let mut b = parts(right)?;
    let length = a.len().max(b.len());
    a.resize(length, 0);
    b.resize(length, 0);
    Ok(a.cmp(&b))
}
