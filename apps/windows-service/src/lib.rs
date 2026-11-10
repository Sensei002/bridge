//! Versioned local control protocol shared by the Bridge UI and Windows service.
//!
//! The pipe is local-only. The service validates every command and keeps privileged
//! driver access out of the normal desktop process.

pub const PROTOCOL_VERSION: u32 = 1;
pub const MAX_REQUEST_BYTES: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    Ping,
    Query,
    Connect,
    Disconnect,
}

impl Request {
    pub fn parse(bytes: &[u8]) -> Result<Self, &'static str> {
        if bytes.is_empty() || bytes.len() > MAX_REQUEST_BYTES {
            return Err("invalid request length");
        }
        let text = std::str::from_utf8(bytes).map_err(|_| "request must be UTF-8")?;
        match text.trim() {
            "BRIDGE_DISPLAY/1 PING" => Ok(Self::Ping),
            "BRIDGE_DISPLAY/1 QUERY" => Ok(Self::Query),
            "BRIDGE_DISPLAY/1 CONNECT" => Ok(Self::Connect),
            "BRIDGE_DISPLAY/1 DISCONNECT" => Ok(Self::Disconnect),
            _ => Err("unsupported display-service request"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplayStatus {
    pub connected: bool,
    pub supported_mode_count: u32,
}

impl DisplayStatus {
    pub fn encode(self) -> String {
        format!(
            "BRIDGE_DISPLAY/1 OK connected={} modes={}\n",
            u8::from(self.connected),
            self.supported_mode_count
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_only_versioned_commands() {
        assert_eq!(Request::parse(b"BRIDGE_DISPLAY/1 PING"), Ok(Request::Ping));
        assert_eq!(Request::parse(b"BRIDGE_DISPLAY/1 QUERY"), Ok(Request::Query));
        assert_eq!(Request::parse(b"BRIDGE_DISPLAY/1 CONNECT"), Ok(Request::Connect));
        assert_eq!(Request::parse(b"BRIDGE_DISPLAY/1 DISCONNECT"), Ok(Request::Disconnect));
        assert!(Request::parse(b"CONNECT").is_err());
        assert!(Request::parse(b"BRIDGE_DISPLAY/2 QUERY").is_err());
    }

    #[test]
    fn rejects_empty_oversized_and_non_utf8_requests() {
        assert!(Request::parse(b"").is_err());
        assert!(Request::parse(&vec![b'X'; MAX_REQUEST_BYTES + 1]).is_err());
        assert!(Request::parse(&[0xff, 0xfe]).is_err());
    }

    #[test]
    fn status_response_is_versioned_and_machine_readable() {
        assert_eq!(
            DisplayStatus { connected: true, supported_mode_count: 3 }.encode(),
            "BRIDGE_DISPLAY/1 OK connected=1 modes=3\n"
        );
    }
}
