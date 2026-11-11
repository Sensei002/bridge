use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_LOG_BYTES: u64 = 512 * 1024;
static LOG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn log_path() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .or_else(|| std::env::var_os("APPDATA"))
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("Bridge")
        .join("Logs")
        .join("desktop.log")
}

pub fn record(component: &str, message: &str) {
    let Ok(_guard) = LOG_LOCK.lock() else { return };
    let path = log_path();
    let Some(parent) = path.parent() else {
        return;
    };
    if fs::create_dir_all(parent).is_err() {
        return;
    }
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let safe_message = message.replace(['\r', '\n'], " ");
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(file, "{timestamp} [{component}] {safe_message}");
    }
    if fs::metadata(&path)
        .map(|metadata| metadata.len() > MAX_LOG_BYTES)
        .unwrap_or(false)
    {
        if let Ok(mut file) = OpenOptions::new().read(true).open(&path) {
            let mut bytes = Vec::new();
            if file.read_to_end(&mut bytes).is_ok() {
                let keep_from = bytes.len().saturating_sub((MAX_LOG_BYTES / 2) as usize);
                let _ = fs::write(&path, &bytes[keep_from..]);
            }
        }
    }
}

pub fn snapshot() -> String {
    let Ok(_guard) = LOG_LOCK.lock() else { return "Bridge desktop diagnostics\nLog is busy; try again.".into() };
    let path = log_path();
    let Ok(mut file) = OpenOptions::new().read(true).open(path) else {
        return "Bridge desktop diagnostics\nNo log entries have been recorded yet.".into();
    };
    let mut bytes = Vec::new();
    if file.read_to_end(&mut bytes).is_err() {
        return "Bridge desktop diagnostics\nUnable to read the diagnostic log.".into();
    }
    let start = bytes.len().saturating_sub(MAX_LOG_BYTES as usize);
    format!(
        "Bridge desktop diagnostics\n{}",
        String::from_utf8_lossy(&bytes[start..])
    )
}
