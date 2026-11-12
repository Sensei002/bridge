//! Fail-closed verification of the exact Bridge desktop executable.
//!
//! The elevated installer calculates this executable's SHA-256 digest and installs
//! the pin beside the LocalSystem service in a directory writable only by SYSTEM
//! and Administrators. No private signing certificate or public CA is required.

use std::{fs, io, path::Path};

use sha2::{Digest, Sha256};

/// Verify the process image against the exact desktop binary hash provisioned by setup.
pub fn verify_client_publisher(image_path: &Path) -> io::Result<()> {
    let service_path = std::env::current_exe()?;
    let service_directory = service_path.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::NotFound, "Bridge Display Service directory is unavailable")
    })?;
    let pin_path = service_directory.join("bridge-desktop-publisher.sha256");
    let pin_contents = fs::read_to_string(&pin_path).map_err(|error| {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("Bridge desktop executable pin is unavailable: {error}"),
        )
    })?;
    let expected = normalize_sha256(&pin_contents)?;
    let image = fs::read(image_path).map_err(|error| {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("Could not read the Bridge desktop process image: {error}"),
        )
    })?;
    let actual = format!("{:X}", Sha256::digest(&image));
    if !constant_time_eq(&actual, &expected) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "desktop executable SHA-256 does not match the installed Bridge build",
        ));
    }
    Ok(())
}

fn normalize_sha256(value: &str) -> io::Result<String> {
    let normalized: String = value.chars()
        .filter(|character| *character != ':' && !character.is_ascii_whitespace())
        .collect();
    if normalized.len() != 64 || !normalized.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "installed Bridge desktop executable pin must contain exactly 64 hexadecimal characters",
        ));
    }
    Ok(normalized.to_ascii_uppercase())
}

fn constant_time_eq(left: &str, right: &str) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.bytes().zip(right.bytes()).fold(0_u8, |difference, (a, b)| difference | (a ^ b)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_well_formed_sha256_pins() {
        assert_eq!(normalize_sha256(&"ab".repeat(32)).unwrap(), "AB".repeat(32));
        assert_eq!(normalize_sha256(&("AB:".to_owned() + &"CD:".repeat(30) + "CD")).unwrap(), "AB".to_owned() + "CD".repeat(31));
        assert!(normalize_sha256("").is_err());
        assert!(normalize_sha256(&"a".repeat(40)).is_err());
        assert!(normalize_sha256(&"g".repeat(64)).is_err());
    }

    #[test]
    fn compares_pins_without_early_byte_exit() {
        let expected = "A1".repeat(32);
        assert!(constant_time_eq(&expected, &expected));
        assert!(!constant_time_eq(&expected, &("A1".repeat(31) + "A2")));
        assert!(!constant_time_eq(&expected, "short"));
    }
}
