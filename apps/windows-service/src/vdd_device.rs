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
        if path.is_file() && path.file_name().is_some_and(|v| v.to_string_lossy().eq_ignore_ascii_case(name)) {
            return Ok(Some(path));
        }
        if path.is_dir() {
            if let Some(found) = find_file(&path, name)? { return Ok(Some(found)); }
        }
    }
    Ok(None)
}

/// Remove any VDD devices that are in an error state (ConfigManagerErrorCode != 0).
/// Uses pnputil to remove the device by instance ID.
fn remove_broken_vdd_devices() -> io::Result<()> {
    let output = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy", "Bypass",
            "-Command",
            r#"
            $ErrorActionPreference = "Stop"
            $devices = Get-CimInstance -ClassName Win32_PnPEntity -Filter "PNPClass = 'Display'" -ErrorAction Stop |
                Where-Object { @($_.HardwareID) -contains 'Root\MttVDD' }
            foreach ($d in $devices) {
                if ($d.ConfigManagerErrorCode -ne 0) {
                    Write-Host "Removing broken VDD device $($d.PNPDeviceID) (error $($d.ConfigManagerErrorCode))"
                    pnputil /remove-device $d.PNPDeviceID
                    if ($LASTEXITCODE -ne 0) { Write-Warning "Failed to remove $($d.PNPDeviceID) (exit $LASTEXITCODE)" }
                }
            }
            "#,
        ])
        .output()?;
    if !output.status.success() {
        eprintln!(
            "Warning: broken VDD cleanup reported non-zero exit: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

/// Ensure the driver device exists. This must only be called from the
/// authenticated CONNECT request path, never from service startup or installer.
pub fn ensure_installed() -> io::Result<()> {
    if crate::vdd_topology::query().present {
        return wait_for_control_pipe();
    }

    // Clean up any broken devices from previous failed attempts before creating a fresh one.
    let _ = remove_broken_vdd_devices();

    let program_files = std::env::var_os("ProgramFiles")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Program Files"));
    let payload = program_files.join("Bridge").join("DisplayServiceSetup").join("VDD");
    let inf = find_file(&payload, "MttVDD.inf")?
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "staged MttVDD.inf was not found"))?;
    let nefcon = find_file(&payload, "nefconw.exe")?
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "staged nefconw.exe was not found"))?;

    let output = Command::new(&nefcon)
        .args([
            "install",
            inf.to_string_lossy().as_ref(),
            r"Root\MttVDD",
            "--no-duplicates",
        ])
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
        if crate::vdd_control::ping().is_ok() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(250));
    }
    Err(io::Error::new(
        io::ErrorKind::TimedOut,
        "upstream VDD control pipe did not become available",
    ))
}