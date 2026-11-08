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

use bridge_protocol::{DeviceDescriptor, Platform, PROTOCOL_VERSION};
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
const CONNECT_RETRIES: usize = 8;
const CONNECT_RETRY_DELAY: Duration = Duration::from_millis(250);
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
    #[error("device descriptor serialization error: {0}")]
    DescriptorSerialization(#[from] serde_json::Error),
    #[error("remote device descriptor is invalid: {0}")]
    InvalidDescriptor(String),
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

/// Result of a completed Noise XX handshake. The fingerprint is the
/// authenticated remote static key, not the untrusted mDNS advertisement.
pub struct NoiseConnection {
    pub stream: TcpStream,
    pub transport: TransportState,
    pub sas: String,
    pub remote_fingerprint: String,
}

impl NoiseConnection {
    /// Exchange device metadata only after the authenticated Noise handshake.
    /// Both peers send first, then receive, avoiding role-dependent deadlocks.
    pub fn exchange_descriptors(
        &mut self,
        local: &DeviceDescriptor,
    ) -> Result<DeviceDescriptor, NetworkError> {
        local.validate().map_err(|error| NetworkError::InvalidDescriptor(error.to_string()))?;
        let encoded = serde_json::to_vec(local)?;
        let mut ciphertext = vec![0_u8; MAX_FRAME_BYTES];
        let length = self.transport.write_message(&encoded, &mut ciphertext)?;
        write_frame(&mut self.stream, &ciphertext[..length])?;

        let received = read_frame(&mut self.stream)?;
        let mut plaintext = vec![0_u8; MAX_FRAME_BYTES];
        let length = self.transport.read_message(&received, &mut plaintext)?;
        let remote: DeviceDescriptor = serde_json::from_slice(&plaintext[..length])?;
        remote.validate().map_err(|error| NetworkError::InvalidDescriptor(error.to_string()))?;
        if remote.id == local.id {
            return Err(NetworkError::InvalidDescriptor("peer presented this device's own ID".into()));
        }
        Ok(remote)
    }
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

/// Choose a usable IPv4 endpoint before IPv6. Link-local IPv6 scope IDs
/// are not represented by a plain SocketAddr, so they are a fallback only.
fn prefer_ipv4_address(addresses: impl IntoIterator<Item = IpAddr>) -> Option<IpAddr> {
    let mut ipv6_fallback = None;
    for address in addresses {
        match address {
            IpAddr::V4(_) => return Some(address),
            IpAddr::V6(_) if ipv6_fallback.is_none() => ipv6_fallback = Some(address),
            IpAddr::V6(_) => {}
        }
    }
    ipv6_fallback
}

#[cfg(test)]
mod address_selection_tests {
    use super::*;

    #[test]
    fn prefers_ipv4_even_when_ipv6_is_discovered_first() {
        let ipv6 = "fe80::54fc:41ff:fe1d:a855".parse().unwrap();
        let ipv4 = "192.168.1.12".parse().unwrap();
        assert_eq!(prefer_ipv4_address([ipv6, ipv4]), Some(ipv4));
    }

    #[test]
    fn falls_back_to_ipv6_when_no_ipv4_exists() {
        let ipv6 = "fe80::54fc:41ff:fe1d:a855".parse().unwrap();
        assert_eq!(prefer_ipv4_address([ipv6]), Some(ipv6));
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
    // Prefer IPv4 when both families are advertised. Link-local IPv6
    // addresses require an interface scope ID; converting them to a plain
    // SocketAddr loses that scope and can make Windows connect attempts fail.
    let address = prefer_ipv4_address(service.get_addresses().iter().filter_map(|ip| match ip {
        mdns_sd::ScopedIp::V4(v4) => Some(IpAddr::V4(*v4.addr())),
        mdns_sd::ScopedIp::V6(v6) => Some(IpAddr::V6(*v6.addr())),
        _ => None,
    }))?;
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
) -> Result<NoiseConnection, NetworkError> {
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
) -> Result<NoiseConnection, NetworkError> {
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
) -> Result<NoiseConnection, NetworkError> {
    if !state.is_handshake_finished() { return Err(NetworkError::HandshakeIncomplete); }
    let remote = state.get_remote_static().ok_or(NetworkError::HandshakeIncomplete)?;
    let actual = fingerprint(remote);
    if let Some(expected) = expected_remote_fingerprint {
        if !constant_time_eq(expected.as_bytes(), actual.as_bytes()) {
            return Err(NetworkError::IdentityMismatch);
        }
    }
    let sas = short_authentication_string(private_key, remote)?;
    Ok(NoiseConnection {
        stream,
        transport: state.into_transport_mode()?,
        sas,
        remote_fingerprint: actual,
    })
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

fn connect_with_retry(address: &SocketAddr) -> Result<TcpStream, NetworkError> {
    for attempt in 0..CONNECT_RETRIES {
        match TcpStream::connect_timeout(address, CONNECT_TIMEOUT) {
            Ok(stream) => return Ok(stream),
            Err(error)
                if error.kind() == io::ErrorKind::ConnectionRefused
                    && attempt + 1 < CONNECT_RETRIES =>
            {
                // Android may briefly reopen its one-shot native acceptor after
                // handling a peer. Retry only an explicit refusal; other errors
                // should be surfaced immediately instead of hiding real faults.
                std::thread::sleep(CONNECT_RETRY_DELAY);
            }
            Err(error) => return Err(NetworkError::Io(error)),
        }
    }
    unreachable!("the final connection attempt always returns")
}

pub fn connect_to_peer(peer: &DiscoveredPeer) -> Result<TcpStream, NetworkError> {
    connect_with_retry(&peer.address)
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
    fn connect_retries_when_android_listener_is_reopening() {
        let reservation = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = reservation.local_addr().unwrap();
        drop(reservation);

        let server = thread::spawn(move || {
            thread::sleep(Duration::from_millis(400));
            let listener = TcpListener::bind(address).unwrap();
            let (stream, _) = listener.accept().unwrap();
            drop(stream);
        });

        let stream = connect_with_retry(&address).unwrap();
        drop(stream);
        server.join().unwrap();
    }

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
    fn noise_handshake_rejects_wrong_pinned_fingerprint() {
        let params = NOISE_PATTERN.parse().unwrap();
        let a = Builder::new(params).generate_keypair().unwrap();
        let params = NOISE_PATTERN.parse().unwrap();
        let b = Builder::new(params).generate_keypair().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let b_private = b.private.clone();
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            accept_noise_xx(stream, &b_private, None)
        });
        let stream = TcpStream::connect(address).unwrap();
        assert!(matches!(
            connect_noise_xx(stream, &a.private, Some(&fingerprint(b"not the peer key"))),
            Err(NetworkError::IdentityMismatch)
        ));
        let _ = server.join().unwrap();
    }

    #[test]
    fn encrypted_descriptor_exchange_authenticates_metadata() {
        use bridge_protocol::{DeviceId, Platform, PROTOCOL_VERSION};
        let params = NOISE_PATTERN.parse().unwrap();
        let a = Builder::new(params).generate_keypair().unwrap();
        let params = NOISE_PATTERN.parse().unwrap();
        let b = Builder::new(params).generate_keypair().unwrap();
        let a_descriptor = DeviceDescriptor {
            id: DeviceId::new(), display_name: "Windows PC".into(),
            platform: Platform::Windows, protocol_version: PROTOCOL_VERSION,
        };
        let b_descriptor = DeviceDescriptor {
            id: DeviceId::new(), display_name: "Android phone".into(),
            platform: Platform::Android, protocol_version: PROTOCOL_VERSION,
        };
        let a_for_server = a_descriptor.clone();
        let b_for_server = b_descriptor.clone();
        let b_private = b.private.clone();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut session = accept_noise_xx(stream, &b_private, None).unwrap();
            session.exchange_descriptors(&b_for_server).unwrap()
        });
        let stream = TcpStream::connect(address).unwrap();
        let mut client = connect_noise_xx(stream, &a.private, None).unwrap();
        let remote = client.exchange_descriptors(&a_for_server).unwrap();
        assert_eq!(remote, b_descriptor);
        assert_eq!(server.join().unwrap(), a_descriptor);
    }

    #[test]
    fn local_key_fingerprint_matches_handshake_identity() {
        let params = NOISE_PATTERN.parse().unwrap();
        let pair = Builder::new(params).generate_keypair().unwrap();
        assert_eq!(local_key_fingerprint(&pair.private).unwrap(), fingerprint(&pair.public));
        assert!(matches!(local_key_fingerprint(&[0; 31]), Err(NetworkError::InvalidPrivateKey)));
    }

    #[test]
    fn noise_handshake_verifies_fingerprint_and_encrypts() {
        let params = NOISE_PATTERN.parse().unwrap();
        let a = Builder::new(params).generate_keypair().unwrap();
        let params = NOISE_PATTERN.parse().unwrap();
        let b = Builder::new(params).generate_keypair().unwrap();
        let a_public = a.public.clone();
        let b_public = b.public.clone();
        let expected_a_fingerprint = fingerprint(&a_public);
        let expected_for_server = expected_a_fingerprint.clone();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let b_private = b.private.clone();
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            accept_noise_xx(stream, &b_private, Some(&expected_for_server))
        });
        let stream = TcpStream::connect(address).unwrap();
        let mut client = connect_noise_xx(stream, &a.private, Some(&fingerprint(&b_public))).unwrap();
        let mut server = server.join().unwrap().unwrap();
        assert_eq!(client.sas, server.sas);
        assert_eq!(client.remote_fingerprint, fingerprint(&b_public));
        assert_eq!(server.remote_fingerprint, expected_a_fingerprint);
        let mut encrypted = vec![0_u8; 128];
        let length = client.transport.write_message(b"private hello", &mut encrypted).unwrap();
        write_frame(&mut client.stream, &encrypted[..length]).unwrap();
        let received = read_frame(&mut server.stream).unwrap();
        let mut plaintext = vec![0_u8; 128];
        let length = server.transport.read_message(&received, &mut plaintext).unwrap();
        assert_eq!(&plaintext[..length], b"private hello");
        assert!(server.transport.read_message(&received, &mut plaintext).is_err(), "replayed transport messages must be rejected");
    }
}
