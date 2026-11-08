//! Local-network discovery and authenticated Bridge transport primitives.
//!
//! Discovery metadata is untrusted. A resolved service is only a candidate;
//! callers must complete the Noise handshake and compare the short
//! authentication string on both screens before marking a peer trusted.
use std::{
    io::{self, Read, Write},
    net::{IpAddr, SocketAddr, TcpStream},
    time::Duration,
};

use bridge_protocol::{Platform, PROTOCOL_VERSION};
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use snow::{HandshakeState, TransportState};
use thiserror::Error;

pub const SERVICE_TYPE: &str = "_bridge._tcp.local.";
pub const DEFAULT_PORT: u16 = 45821;
pub const MAX_FRAME_BYTES: usize = 65_535;
const NOISE_PATTERN: &str = "Noise_XX_25519_ChaChaPoly_SHA256";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const IO_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Error)]
pub enum NetworkError {
    #[error("mDNS error: {0}")]
    Mdns(String),
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("invalid frame length: {0}")]
    InvalidFrameLength(usize),
    #[error("Noise error: {0}")]
    Noise(#[from] snow::Error),
    #[error("peer identity did not match the approved fingerprint")]
    IdentityMismatch,
    #[error("handshake did not complete")]
    HandshakeIncomplete,
    #[error("Noise static private key must contain exactly 32 bytes")]
    InvalidPrivateKey,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryRecord {
    pub device_id: String,
    pub display_name: String,
    pub platform: Platform,
    pub protocol_version: u16,
    pub public_key_fingerprint: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscoveredPeer {
    pub record: DiscoveryRecord,
    pub address: SocketAddr,
    pub instance_name: String,
}

pub struct LocalDiscovery {
    daemon: ServiceDaemon,
    instance_fullname: String,
}

impl LocalDiscovery {
    /// Publish a Bridge TCP endpoint and browse for other Bridge peers.
    /// The public-key fingerprint is informational until authenticated.
    pub fn start(
        instance_name: &str,
        host_name: &str,
        port: u16,
        record: &DiscoveryRecord,
    ) -> Result<(Self, mdns_sd::Receiver<ServiceEvent>), NetworkError> {
        if record.protocol_version != PROTOCOL_VERSION {
            return Err(NetworkError::Mdns("unsupported local protocol version".into()));
        }
        let daemon = ServiceDaemon::new().map_err(|e| NetworkError::Mdns(e.to_string()))?;
        let version = record.protocol_version.to_string();
        let properties = [
            ("id", record.device_id.as_str()),
            ("name", record.display_name.as_str()),
            ("platform", platform_name(&record.platform)),
            ("version", version.as_str()),
            ("fingerprint", record.public_key_fingerprint.as_str()),
        ];
        let service = ServiceInfo::new(
            SERVICE_TYPE, instance_name, host_name, "", port, &properties[..],
        )
        .map_err(|e| NetworkError::Mdns(e.to_string()))?
        .enable_addr_auto();
        let instance_fullname = service.get_fullname().to_owned();
        daemon.register(service).map_err(|e| NetworkError::Mdns(e.to_string()))?;
        let events = daemon.browse(SERVICE_TYPE).map_err(|e| NetworkError::Mdns(e.to_string()))?;
        Ok((Self { daemon, instance_fullname }, events))
    }

    pub fn shutdown(self) -> Result<(), NetworkError> {
        let _ = self.daemon.unregister(&self.instance_fullname);
        self.daemon.shutdown().map(|_| ()).map_err(|e| NetworkError::Mdns(e.to_string()))
    }
}

/// Convert a resolved DNS-SD event into an untrusted candidate peer.
pub fn peer_from_event(event: &ServiceEvent) -> Option<DiscoveredPeer> {
    let ServiceEvent::ServiceResolved(service) = event else { return None };
    // Resolved events are scoped to the service type passed to browse().
    if !service.get_fullname().ends_with(SERVICE_TYPE) || !service.is_valid() { return None; }
    let property = |key: &str| service.get_properties().get_property_val_str(key).map(str::to_owned);
    let record = DiscoveryRecord {
        device_id: property("id")?,
        display_name: property("name")?,
        platform: parse_platform(&property("platform")?)?,
        protocol_version: property("version")?.parse().ok()?,
        public_key_fingerprint: property("fingerprint")?,
    };
    if record.protocol_version != PROTOCOL_VERSION
        || record.device_id.is_empty()
        || record.display_name.trim().is_empty()
        || record.display_name.len() > 64
        || record.public_key_fingerprint.len() != 64
        || !record.public_key_fingerprint.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return None;
    }
    let address = service.get_addresses().iter().find_map(|ip| match ip {
        mdns_sd::ScopedIp::V4(v4) => Some(IpAddr::V4(*v4.addr())),
        mdns_sd::ScopedIp::V6(v6) => Some(IpAddr::V6(*v6.addr())),
        _ => None,
    })?;
    Some(DiscoveredPeer {
        record,
        address: SocketAddr::new(address, service.get_port()),
        instance_name: service.get_fullname().to_owned(),
    })
}

fn platform_name(platform: &Platform) -> &'static str {
    match platform {
        Platform::Windows => "windows",
        Platform::Android => "android",
        Platform::Ios => "ios",
        Platform::Linux => "linux",
        Platform::Macos => "macos",
        Platform::Unknown => "unknown",
    }
}

fn parse_platform(value: &str) -> Option<Platform> {
    match value {
        "windows" => Some(Platform::Windows),
        "android" => Some(Platform::Android),
        "ios" => Some(Platform::Ios),
        "linux" => Some(Platform::Linux),
        "macos" => Some(Platform::Macos),
        "unknown" => Some(Platform::Unknown),
        _ => None,
    }
}

/// Write one bounded, big-endian length-prefixed frame.
pub fn write_frame(writer: &mut impl Write, payload: &[u8]) -> Result<(), NetworkError> {
    if payload.is_empty() || payload.len() > MAX_FRAME_BYTES {
        return Err(NetworkError::InvalidFrameLength(payload.len()));
    }
    writer.write_all(&(payload.len() as u32).to_be_bytes())?;
    writer.write_all(payload)?;
    writer.flush()?;
    Ok(())
}

/// Read one bounded, big-endian length-prefixed frame.
pub fn read_frame(reader: &mut impl Read) -> Result<Vec<u8>, NetworkError> {
    let mut length = [0_u8; 4];
    reader.read_exact(&mut length)?;
    let length = u32::from_be_bytes(length) as usize;
    if length == 0 || length > MAX_FRAME_BYTES {
        return Err(NetworkError::InvalidFrameLength(length));
    }
    let mut payload = vec![0_u8; length];
    reader.read_exact(&mut payload)?;
    Ok(payload)
}

pub fn initiator_handshake(private_key: &[u8]) -> Result<HandshakeState, NetworkError> {
    let params = NOISE_PATTERN.parse().map_err(NetworkError::Noise)?;
    Ok(snow::Builder::new(params).local_private_key(private_key).build_initiator()?)
}

pub fn responder_handshake(private_key: &[u8]) -> Result<HandshakeState, NetworkError> {
    let params = NOISE_PATTERN.parse().map_err(NetworkError::Noise)?;
    Ok(snow::Builder::new(params).local_private_key(private_key).build_responder()?)
}

fn write_noise(state: &mut HandshakeState) -> Result<Vec<u8>, NetworkError> {
    let mut output = vec![0_u8; MAX_FRAME_BYTES];
    let length = state.write_message(&[], &mut output)?;
    output.truncate(length);
    Ok(output)
}

fn read_noise(state: &mut HandshakeState, message: &[u8]) -> Result<(), NetworkError> {
    let mut output = vec![0_u8; MAX_FRAME_BYTES];
    state.read_message(message, &mut output)?;
    Ok(())
}

/// Initiator side of the three-message Noise XX handshake. The caller must
/// compare the returned SAS with the peer's displayed SAS and obtain explicit
/// approval before storing the peer as trusted.
pub fn connect_noise_xx(
    mut stream: TcpStream,
    private_key: &[u8],
    expected_remote_fingerprint: Option<&str>,
) -> Result<(TcpStream, TransportState, String), NetworkError> {
    configure_stream(&stream)?;
    let mut state = initiator_handshake(private_key)?;
    write_frame(&mut stream, &write_noise(&mut state)?)?;
    let second = read_frame(&mut stream)?;
    read_noise(&mut state, &second)?;
    write_frame(&mut stream, &write_noise(&mut state)?)?;
    finish_handshake(stream, state, private_key, expected_remote_fingerprint)
}

/// Responder side of the three-message Noise XX handshake.
pub fn accept_noise_xx(
    mut stream: TcpStream,
    private_key: &[u8],
    expected_remote_fingerprint: Option<&str>,
) -> Result<(TcpStream, TransportState, String), NetworkError> {
    configure_stream(&stream)?;
    let mut state = responder_handshake(private_key)?;
    let first = read_frame(&mut stream)?;
    read_noise(&mut state, &first)?;
    write_frame(&mut stream, &write_noise(&mut state)?)?;
    let third = read_frame(&mut stream)?;
    read_noise(&mut state, &third)?;
    finish_handshake(stream, state, private_key, expected_remote_fingerprint)
}

fn configure_stream(stream: &TcpStream) -> Result<(), NetworkError> {
    stream.set_nodelay(true)?;
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;
    Ok(())
}

fn finish_handshake(
    stream: TcpStream,
    state: HandshakeState,
    private_key: &[u8],
    expected_remote_fingerprint: Option<&str>,
) -> Result<(TcpStream, TransportState, String), NetworkError> {
    if !state.is_handshake_finished() { return Err(NetworkError::HandshakeIncomplete); }
    let remote = state.get_remote_static().ok_or(NetworkError::HandshakeIncomplete)?;
    let actual = fingerprint(remote);
    if let Some(expected) = expected_remote_fingerprint {
        if !constant_time_eq(expected.as_bytes(), actual.as_bytes()) {
            return Err(NetworkError::IdentityMismatch);
        }
    }
    let sas = short_authentication_string(private_key, remote)?;
    Ok((stream, state.into_transport_mode()?, sas))
}

pub fn fingerprint(public_key: &[u8]) -> String {
    hex::encode(Sha256::digest(public_key))
}

/// Derive the public-key fingerprint for a persisted 32-byte Noise static key.
pub fn local_key_fingerprint(private_key: &[u8]) -> Result<String, NetworkError> {
    use x25519_dalek::{PublicKey, StaticSecret};
    let bytes: [u8; 32] = private_key.try_into().map_err(|_| NetworkError::InvalidPrivateKey)?;
    let secret = StaticSecret::from(bytes);
    let public = PublicKey::from(&secret);
    Ok(fingerprint(public.as_bytes()))
}

/// Six-digit SAS derived symmetrically from both static public keys.
pub fn short_authentication_string(
    local_private: &[u8],
    remote_public: &[u8],
) -> Result<String, NetworkError> {
    use x25519_dalek::{PublicKey, StaticSecret};

    let private_bytes: [u8; 32] = local_private
        .try_into()
        .map_err(|_| NetworkError::InvalidPrivateKey)?;
    let local_secret = StaticSecret::from(private_bytes);
    let local_public = PublicKey::from(&local_secret);
    let local_public = local_public.as_bytes();
    let (first, second) = if local_public.as_slice() <= remote_public {
        (local_public.as_slice(), remote_public)
    } else {
        (remote_public, local_public.as_slice())
    };
    let mut hasher = Sha256::new();
    hasher.update(b"Bridge SAS v1");
    hasher.update(first);
    hasher.update(second);
    let digest = hasher.finalize();
    let value = u32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]]) % 1_000_000;
    Ok(format!("{value:06}"))
}

pub fn connect_to_peer(peer: &DiscoveredPeer) -> Result<TcpStream, NetworkError> {
    Ok(TcpStream::connect_timeout(&peer.address, CONNECT_TIMEOUT)?)
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() { return false; }
    left.iter().zip(right).fold(0_u8, |diff, (a, b)| diff | (a ^ b)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use snow::Builder;
    use std::{io::Cursor, net::TcpListener, thread};

    #[test]
    fn frames_round_trip_and_enforce_bounds() {
        let mut data = Vec::new();
        write_frame(&mut data, b"bridge").unwrap();
        assert_eq!(read_frame(&mut Cursor::new(data)).unwrap(), b"bridge");
        assert!(matches!(write_frame(&mut Vec::new(), &[]), Err(NetworkError::InvalidFrameLength(0))));
        assert!(matches!(write_frame(&mut Vec::new(), &vec![0; MAX_FRAME_BYTES + 1]), Err(NetworkError::InvalidFrameLength(_))));
    }

    #[test]
    fn fingerprints_are_stable_and_hex_encoded() {
        let value = fingerprint(b"public key");
        assert_eq!(value.len(), 64);
        assert_eq!(value, fingerprint(b"public key"));
    }

    #[test]
    fn sas_is_symmetric() {
        let params = NOISE_PATTERN.parse().unwrap();
        let a = Builder::new(params).generate_keypair().unwrap();
        let params = NOISE_PATTERN.parse().unwrap();
        let b = Builder::new(params).generate_keypair().unwrap();
        assert_eq!(
            short_authentication_string(&a.private, &b.public).unwrap(),
            short_authentication_string(&b.private, &a.public).unwrap()
        );
    }

    #[test]
    fn noise_handshake_verifies_fingerprint_and_encrypts() {
        let params = NOISE_PATTERN.parse().unwrap();
        let a = Builder::new(params).generate_keypair().unwrap();
        let params = NOISE_PATTERN.parse().unwrap();
        let b = Builder::new(params).generate_keypair().unwrap();
        let a_public = a.public.clone();
        let b_public = b.public.clone();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let b_private = b.private.clone();
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            accept_noise_xx(stream, &b_private, Some(&fingerprint(&a_public)))
        });
        let stream = TcpStream::connect(address).unwrap();
        let (mut client_stream, mut initiator, sas_a) =
            connect_noise_xx(stream, &a.private, Some(&fingerprint(&b_public))).unwrap();
        let (mut server_stream, mut responder, sas_b) = server.join().unwrap().unwrap();
        assert_eq!(sas_a, sas_b);
        let mut encrypted = vec![0_u8; 128];
        let length = initiator.write_message(b"private hello", &mut encrypted).unwrap();
        write_frame(&mut client_stream, &encrypted[..length]).unwrap();
        let received = read_frame(&mut server_stream).unwrap();
        let mut plaintext = vec![0_u8; 128];
        let length = responder.read_message(&received, &mut plaintext).unwrap();
        assert_eq!(&plaintext[..length], b"private hello");
    }
}
