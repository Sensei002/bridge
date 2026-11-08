//! Windows Credential Manager persistence for the local Noise static identity.
use bridge_security::generate_static_keypair;
use keyring::Entry;

const SERVICE: &str = "app.bridge.desktop";
const ACCOUNT: &str = "noise-static-private-key.v1";

pub fn load_or_create() -> Result<Vec<u8>, String> {
    let entry = Entry::new(SERVICE, ACCOUNT).map_err(|error| error.to_string())?;
    match entry.get_password() {
        Ok(encoded) => {
            let key = decode_hex(&encoded)?;
            if key.len() != 32 {
                return Err("stored Noise private key has an invalid length".into());
            }
            Ok(key)
        }
        Err(keyring::Error::NoEntry) => {
            let keypair = generate_static_keypair().map_err(|error| error.to_string())?;
            entry.set_password(&encode_hex(&keypair.private))
                .map(|()| keypair.private)
                .map_err(|error| error.to_string())
        }
        Err(error) => Err(error.to_string()),
    }
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

fn decode_hex(value: &str) -> Result<Vec<u8>, String> {
    if value.len() % 2 != 0 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("stored Noise private key is not valid hexadecimal".into());
    }
    (0..value.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&value[index..index + 2], 16)
            .map_err(|error| error.to_string()))
        .collect()
}
