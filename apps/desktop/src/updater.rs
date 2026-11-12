use quick_xml::events::Event;
use quick_xml::Reader;
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

const RELEASES_FEED: &str = "https://github.com/Sensei002/bridge/releases.atom";
const RELEASE_PAGE_PREFIX: &str = "https://github.com/Sensei002/bridge/releases/tag/";
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

fn latest_release_tag(channel: &str, client: &reqwest::blocking::Client) -> Result<String, String> {
    let feed = client.get(RELEASES_FEED).send()
        .map_err(|error| format!("Could not reach the Bridge releases feed: {error}"))?
        .error_for_status().map_err(|error| format!("Bridge releases feed request failed: {error}"))?
        .text().map_err(|error| error.to_string())?;
    let mut reader = Reader::from_str(&feed);
    reader.config_mut().trim_text(true);
    loop {
        match reader.read_event() {
            Ok(Event::Start(element)) | Ok(Event::Empty(element)) if element.local_name().as_ref() == b"link" => {
                for attribute in element.attributes().flatten() {
                    if attribute.key.local_name().as_ref() == b"href" {
                        let href = attribute.decode_and_unescape_value(reader.decoder())
                            .map_err(|error| error.to_string())?;
                        if let Some(tag) = href.strip_prefix(RELEASE_PAGE_PREFIX) {
                            let expected = if channel == "development" { "dev-" } else { "stable-" };
                            if tag.starts_with(expected)
                                && tag[expected.len()..].bytes().all(|byte| byte.is_ascii_digit())
                                && tag.len() > expected.len()
                            {
                                return Ok(tag.to_owned());
                            }
                        }
                    }
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => return Err(format!("Could not parse Bridge releases feed: {error}")),
        }
    }
    Err(format!("No {channel} release is available yet."))
}

fn check_and_download(channel: &str) -> Result<Option<(String, PathBuf)>, String> {
    let client = reqwest::blocking::Client::builder()
        .user_agent("Bridge-Windows-Updater")
        .timeout(Duration::from_secs(90))
        .build()
        .map_err(|error| error.to_string())?;

    let tag = latest_release_tag(channel, &client)?;
    let manifest_url = format!("{RELEASE_PREFIX}{tag}/bridge-update.json");
    let manifest: serde_json::Value = client.get(&manifest_url).send()
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
    let download_url = windows["download_url"].as_str().ok_or("Windows installer download URL is missing")?;
    if !download_url.starts_with(&format!("{RELEASE_PREFIX}{tag}/")) {
        return Err("Windows installer URL is not an asset from the selected official Bridge release".into());
    }
    let expected_hash = windows["sha256"].as_str().ok_or("Windows installer checksum is missing")?.to_ascii_lowercase();
    if expected_hash.len() != 64 || !expected_hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("Windows installer checksum is invalid".into());
    }
    let asset = windows["asset"].as_str().ok_or("Windows installer asset name is missing")?;
    if asset.contains('/') || asset.contains('\\') || !asset.to_ascii_lowercase().ends_with(".exe") {
        return Err("Windows installer asset filename is invalid".into());
    }

    let bytes = client.get(download_url).send()
        .map_err(|error| error.to_string())?
        .error_for_status().map_err(|error| error.to_string())?
        .bytes().map_err(|error| error.to_string())?;
    let actual_hash = format!("{:x}", Sha256::digest(&bytes));
    if actual_hash != expected_hash {
        return Err("Windows installer SHA-256 verification failed; installation was blocked".into());
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
