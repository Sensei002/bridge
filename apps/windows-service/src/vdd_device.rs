//! Creates the root-enumerated VDD device only after the service has
//! validated a live, attested display lease. Installer setup merely stages
//! the signed package and this activation payload.
use std::{
    fs,
    io,
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::{Duration, Instant},
};

fn find_file(root: &Path, name: &str) -> io::Result<Option<PathBuf>> {
    if !root.exists() { return Ok(None); }
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() && path.file_name().is_some_and(|value| value.eq_ignore_ascii_case(name)) {
            return Ok(Some(path));
        }
        if path.is_dir() {
            if let Some(found) = find_file(&path, name)? { return Ok(Some(found)); }
        }
    }
    Ok(None)
}

/// Ensure the driver device exists. This must only be called from the
/// authenticated CONNECT request path, never from service startup or installer.
pub fn ensure_installed() -> io::Result<()> {
    if crate::vdd_topology::query().present {
        return wait_for_control_pipe();
    }

    let program_files = std::env::var_os("ProgramFiles")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Program Files"));
    let payload = program_files.join("Bridge").join("DisplayServiceSetup").join("VDD");
    let inf = find_file(&payload, "MttVDD.inf")?
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "staged MttVDD.inf was not found"))?;
    let nefcon = find_file(&payload, "nefconw.exe")?
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "staged nefconw.exe was not found"))?;

    let output = Command::new(&nefcon)
        .args(["install", inf.to_string_lossy().as_ref(), r"Root\MttVDD"])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!(
                "NefCon could not create ROOT\\MttVDD ({}): {}{}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr),
            ),
        ));
    }

    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        if crate::vdd_topology::query().present {
            return wait_for_control_pipe();
        }
        thread::sleep(Duration::from_millis(250));
    }
    Err(io::Error::new(
        io::ErrorKind::TimedOut,
        "Windows did not enumerate ROOT\\MttVDD after authenticated activation",
    ))
}

fn wait_for_control_pipe() -> io::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        if crate::vdd_control::set_display_count(0).is_ok() {
            // Establish a known disconnected state before CONNECT requests one
            // display. This also proves that the vendor pipe is actually ready.
            return Ok(());
        }
        thread::sleep(Duration::from_millis(250));
    }
    Err(io::Error::new(
        io::ErrorKind::TimedOut,
        "upstream VDD control pipe did not become available",
    ))
}
