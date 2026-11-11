//! Safe, bounded file-transfer primitives shared by Bridge clients.
//!
//! The transport layer is responsible for carrying these messages inside an
//! authenticated Noise session. This crate validates offers, encodes bounded
//! chunks, and writes incoming files to collision-safe destinations.
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

pub const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
pub const MAX_FILENAME_BYTES: usize = 240;
pub const CHUNK_DATA_BYTES: usize = 48 * 1024;
const CHUNK_MAGIC: &[u8; 4] = b"BRF1";
const CHUNK_HEADER_BYTES: usize = 4 + 16 + 8;

#[derive(Debug, Error)]
pub enum TransferError {
    #[error("invalid transfer offer: {0}")]
    InvalidOffer(String),
    #[error("invalid transfer chunk: {0}")]
    InvalidChunk(String),
    #[error("transfer was cancelled")]
    Cancelled,
    #[error("file I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("file checksum did not match the sender's offer")]
    ChecksumMismatch,
    #[error("transfer is incomplete")]
    Incomplete,
    #[error("source file changed while it was being sent")]
    SourceChanged,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileOffer {
    pub transfer_id: Uuid,
    pub file_name: String,
    pub file_size: u64,
    /// Lowercase SHA-256 hex digest of the complete file.
    pub sha256: String,
}

impl FileOffer {
    pub fn validate(&self) -> Result<(), TransferError> {
        if self.file_size > MAX_FILE_BYTES {
            return Err(TransferError::InvalidOffer("file exceeds the 2 GiB transfer limit".into()));
        }
        validate_file_name(&self.file_name)?;
        if self.sha256.len() != 64 || !self.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(TransferError::InvalidOffer("SHA-256 digest must be 64 hexadecimal characters".into()));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TransferControl {
    FileOffer { offer: FileOffer },
    FileAccept { transfer_id: Uuid },
    FileReject { transfer_id: Uuid, reason: String },
    FileComplete { transfer_id: Uuid },
    FileCancel { transfer_id: Uuid },
    /// Relay a one-time challenge and the live desktop/Android Noise session ID.
    DisplayChallengeRequest { request_id: Uuid, session_id: Uuid, nonce: String },
    /// Ed25519 proof returned only over the authenticated Noise session that received the request.
    DisplayChallengeResponse {
        request_id: Uuid,
        session_id: Uuid,
        nonce: String,
        device_fingerprint: String,
        signature: String,
    },
}

pub fn encode_control(message: &TransferControl) -> Result<Vec<u8>, TransferError> {
    match message {
        TransferControl::FileOffer { offer } => offer.validate()?,
        TransferControl::FileReject { reason, .. } if reason.len() > 256 =>
            return Err(TransferError::InvalidOffer("rejection reason is too long".into())),
        TransferControl::DisplayChallengeRequest { nonce, .. } => validate_challenge_nonce(nonce)?,
        TransferControl::DisplayChallengeResponse { nonce, device_fingerprint, signature, .. } => {
            validate_challenge_nonce(nonce)?;
            validate_hex_field(device_fingerprint, 64, "device fingerprint")?;
            validate_hex_field(signature, 128, "attestation signature")?;
        },
        _ => {}
    }
    serde_json::to_vec(message).map_err(|error| TransferError::InvalidOffer(error.to_string()))
}

pub fn decode_control(bytes: &[u8]) -> Result<TransferControl, TransferError> {
    if bytes.is_empty() || bytes.len() > 4096 {
        return Err(TransferError::InvalidOffer("control message size is invalid".into()));
    }
    let message: TransferControl = serde_json::from_slice(bytes)
        .map_err(|error| TransferError::InvalidOffer(error.to_string()))?;
    if let TransferControl::FileOffer { offer } = &message {
        offer.validate()?;
    }
    if let TransferControl::FileReject { reason, .. } = &message {
        if reason.len() > 256 {
            return Err(TransferError::InvalidOffer("rejection reason is too long".into()));
        }
    }
    match &message {
        TransferControl::DisplayChallengeRequest { nonce, .. } => validate_challenge_nonce(nonce)?,
        TransferControl::DisplayChallengeResponse { nonce, device_fingerprint, signature, .. } => {
            validate_challenge_nonce(nonce)?;
            validate_hex_field(device_fingerprint, 64, "device fingerprint")?;
            validate_hex_field(signature, 128, "attestation signature")?;
        }
        _ => {}
    }
    Ok(message)
}

fn validate_hex_field(value: &str, expected_len: usize, label: &str) -> Result<(), TransferError> {
    if value.len() != expected_len || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(TransferError::InvalidOffer(format!("{label} must be {expected_len} hexadecimal characters")));
    }
    Ok(())
}

fn validate_challenge_nonce(nonce: &str) -> Result<(), TransferError> {
    if nonce.len() != 64 || !nonce.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(TransferError::InvalidOffer(
            "display challenge must be exactly 64 hexadecimal characters".into(),
        ));
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileChunk {
    pub transfer_id: Uuid,
    pub offset: u64,
    pub data: Vec<u8>,
}

pub fn encode_chunk(chunk: &FileChunk) -> Result<Vec<u8>, TransferError> {
    if chunk.data.is_empty() || chunk.data.len() > CHUNK_DATA_BYTES {
        return Err(TransferError::InvalidChunk("chunk payload must be between 1 byte and 48 KiB".into()));
    }
    let mut encoded = Vec::with_capacity(CHUNK_HEADER_BYTES + chunk.data.len());
    encoded.extend_from_slice(CHUNK_MAGIC);
    encoded.extend_from_slice(chunk.transfer_id.as_bytes());
    encoded.extend_from_slice(&chunk.offset.to_be_bytes());
    encoded.extend_from_slice(&chunk.data);
    Ok(encoded)
}

/// A transfer packet carried as one encrypted Noise application message.
/// The leading discriminator prevents JSON control messages from being confused
/// with binary file chunks while preserving the existing chunk wire format.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum DisplayInputAction {
    Down = 1,
    Move = 2,
    Up = 3,
    Cancel = 4,
}

/// One normalized pointer sample from the Android display surface.
/// Coordinates use the inclusive 0..=65535 range so the Windows side can
/// map them to the active virtual-monitor dimensions without trusting pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DisplayInputEvent {
    pub sequence: u64,
    pub timestamp_ns: u64,
    pub pointer_id: u8,
    pub action: DisplayInputAction,
    pub x: u16,
    pub y: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TransferPacket {
    Control(TransferControl),
    Chunk(FileChunk),
    DisplayFrame(DisplayFrameFragment),
    EncodedVideoFrame(EncodedVideoFrameFragment),
    DisplayInput(DisplayInputEvent),
}

pub const CONTROL_PACKET_TAG: u8 = 0x01;
pub const CHUNK_PACKET_TAG: u8 = 0x02;
pub const DISPLAY_INPUT_PACKET_TAG: u8 = 0x05;
pub const MAX_DISPLAY_INPUT_POINTERS: u8 = 10;
const DISPLAY_INPUT_PACKET_BYTES: usize = 1 + 8 + 8 + 1 + 1 + 2 + 2;
pub const MAX_PACKET_BYTES: usize = 65_519;

fn encode_display_input(event: DisplayInputEvent) -> Result<Vec<u8>, TransferError> {
    if event.sequence == 0 || event.timestamp_ns == 0 || event.pointer_id >= MAX_DISPLAY_INPUT_POINTERS {
        return Err(TransferError::InvalidChunk("display input sequence, timestamp, or pointer ID is invalid".into()));
    }
    let mut bytes = Vec::with_capacity(DISPLAY_INPUT_PACKET_BYTES);
    bytes.push(DISPLAY_INPUT_PACKET_TAG);
    bytes.extend_from_slice(&event.sequence.to_be_bytes());
    bytes.extend_from_slice(&event.timestamp_ns.to_be_bytes());
    bytes.push(event.pointer_id);
    bytes.push(event.action as u8);
    bytes.extend_from_slice(&event.x.to_be_bytes());
    bytes.extend_from_slice(&event.y.to_be_bytes());
    Ok(bytes)
}

fn decode_display_input(bytes: &[u8]) -> Result<DisplayInputEvent, TransferError> {
    if bytes.len() != DISPLAY_INPUT_PACKET_BYTES || bytes[0] != DISPLAY_INPUT_PACKET_TAG {
        return Err(TransferError::InvalidChunk("display input packet size or tag is invalid".into()));
    }
    let sequence = u64::from_be_bytes(bytes[1..9].try_into().expect("fixed display input header"));
    let timestamp_ns = u64::from_be_bytes(bytes[9..17].try_into().expect("fixed display input header"));
    let pointer_id = bytes[17];
    let action = match bytes[18] {
        1 => DisplayInputAction::Down,
        2 => DisplayInputAction::Move,
        3 => DisplayInputAction::Up,
        4 => DisplayInputAction::Cancel,
        _ => return Err(TransferError::InvalidChunk("unknown display input action".into())),
    };
    let event = DisplayInputEvent {
        sequence, timestamp_ns, pointer_id, action,
        x: u16::from_be_bytes(bytes[19..21].try_into().expect("fixed display input header")),
        y: u16::from_be_bytes(bytes[21..23].try_into().expect("fixed display input header")),
    };
    if event.sequence == 0 || event.timestamp_ns == 0 || event.pointer_id >= MAX_DISPLAY_INPUT_POINTERS {
        return Err(TransferError::InvalidChunk("display input sequence, timestamp, or pointer ID is invalid".into()));
    }
    Ok(event)
}

pub fn encode_packet(packet: &TransferPacket) -> Result<Vec<u8>, TransferError> {
    let encoded = match packet {
        TransferPacket::Control(message) => {
            let mut bytes = vec![CONTROL_PACKET_TAG];
            bytes.extend_from_slice(&encode_control(message)?);
            bytes
        }
        TransferPacket::Chunk(chunk) => {
            let mut bytes = vec![CHUNK_PACKET_TAG];
            bytes.extend_from_slice(&encode_chunk(chunk)?);
            bytes
        }
        TransferPacket::DisplayFrame(fragment) => encode_display_frame_fragment(fragment)?,
        TransferPacket::EncodedVideoFrame(fragment) => encode_encoded_video_fragment(fragment)?,
        TransferPacket::DisplayInput(event) => encode_display_input(*event)?,
    };
    if encoded.len() > MAX_PACKET_BYTES {
        return Err(TransferError::InvalidChunk("packet exceeds the encrypted transport limit".into()));
    }
    Ok(encoded)
}

pub fn decode_packet(bytes: &[u8]) -> Result<TransferPacket, TransferError> {
    if bytes.len() < 2 || bytes.len() > MAX_PACKET_BYTES {
        return Err(TransferError::InvalidChunk("packet size is invalid".into()));
    }
    match bytes[0] {
        CONTROL_PACKET_TAG => Ok(TransferPacket::Control(decode_control(&bytes[1..])?)),
        CHUNK_PACKET_TAG => Ok(TransferPacket::Chunk(decode_chunk(&bytes[1..])?)),
        DISPLAY_FRAME_PACKET_TAG => Ok(TransferPacket::DisplayFrame(decode_display_frame_fragment(bytes)?)),
        ENCODED_VIDEO_PACKET_TAG => Ok(TransferPacket::EncodedVideoFrame(decode_encoded_video_fragment(bytes)?)),
        DISPLAY_INPUT_PACKET_TAG => Ok(TransferPacket::DisplayInput(decode_display_input(bytes)?)),
        _ => Err(TransferError::InvalidChunk("unknown transfer packet type".into())),
    }
}

pub fn decode_chunk(bytes: &[u8]) -> Result<FileChunk, TransferError> {
    if bytes.len() <= CHUNK_HEADER_BYTES || bytes.len() > CHUNK_HEADER_BYTES + CHUNK_DATA_BYTES {
        return Err(TransferError::InvalidChunk("encoded chunk size is invalid".into()));
    }
    if &bytes[..4] != CHUNK_MAGIC {
        return Err(TransferError::InvalidChunk("unknown chunk format".into()));
    }
    let transfer_id = Uuid::from_slice(&bytes[4..20])
        .map_err(|error| TransferError::InvalidChunk(error.to_string()))?;
    let offset = u64::from_be_bytes(bytes[20..28].try_into().expect("fixed header length"));
    Ok(FileChunk { transfer_id, offset, data: bytes[CHUNK_HEADER_BYTES..].to_vec() })
}

pub fn hash_file(path: impl AsRef<Path>) -> Result<(u64, String), TransferError> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut size = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 { break; }
        size = size.checked_add(read as u64)
            .ok_or_else(|| TransferError::InvalidOffer("file size overflow".into()))?;
        if size > MAX_FILE_BYTES {
            return Err(TransferError::InvalidOffer("file exceeds the 2 GiB transfer limit".into()));
        }
        hasher.update(&buffer[..read]);
    }
    Ok((size, hex::encode(hasher.finalize())))
}

pub fn offer_for_file(path: impl AsRef<Path>) -> Result<FileOffer, TransferError> {
    let path = path.as_ref();
    let file_name = path.file_name().and_then(|name| name.to_str())
        .ok_or_else(|| TransferError::InvalidOffer("file name is not valid UTF-8".into()))?
        .to_owned();
    validate_file_name(&file_name)?;
    let (file_size, sha256) = hash_file(path)?;
    let offer = FileOffer { transfer_id: Uuid::new_v4(), file_name, file_size, sha256 };
    offer.validate()?;
    Ok(offer)
}

/// Streams an offered source file in bounded chunks and verifies that it did
/// not change between offer creation and the end of transmission.
pub struct OutgoingFile {
    file: File,
    offer: FileOffer,
    offset: u64,
    hasher: Sha256,
    finished: bool,
}

impl OutgoingFile {
    pub fn open(path: impl AsRef<Path>, offer: FileOffer) -> Result<Self, TransferError> {
        offer.validate()?;
        let file = File::open(path)?;
        Ok(Self { file, offer, offset: 0, hasher: Sha256::new(), finished: false })
    }

    pub fn next_chunk(&mut self) -> Result<Option<FileChunk>, TransferError> {
        if self.finished { return Ok(None); }
        if self.offset == self.offer.file_size {
            let mut extra = [0_u8; 1];
            if self.file.read(&mut extra)? != 0 {
                return Err(TransferError::SourceChanged);
            }
            let digest = hex::encode(self.hasher.clone().finalize());
            if !digest.eq_ignore_ascii_case(&self.offer.sha256) {
                return Err(TransferError::SourceChanged);
            }
            self.finished = true;
            return Ok(None);
        }

        let remaining = self.offer.file_size - self.offset;
        let length = remaining.min(CHUNK_DATA_BYTES as u64) as usize;
        let mut data = vec![0_u8; length];
        self.file.read_exact(&mut data).map_err(|error| {
            if error.kind() == io::ErrorKind::UnexpectedEof {
                TransferError::SourceChanged
            } else {
                TransferError::Io(error)
            }
        })?;
        self.hasher.update(&data);
        let chunk = FileChunk { transfer_id: self.offer.transfer_id, offset: self.offset, data };
        self.offset += length as u64;
        Ok(Some(chunk))
    }

    pub fn total_bytes(&self) -> u64 { self.offer.file_size }
}

fn validate_file_name(name: &str) -> Result<(), TransferError> {
    if name.is_empty() || name.len() > MAX_FILENAME_BYTES || name == "." || name == ".." {
        return Err(TransferError::InvalidOffer("file name is empty, too long, or reserved".into()));
    }
    if name.chars().any(|ch| ch.is_control() || ch == '/' || ch == '\\' || ch == ':' ) {
        return Err(TransferError::InvalidOffer("file name contains a path separator or control character".into()));
    }
    if name.trim() != name || name.ends_with('.') || name.ends_with(' ') {
        return Err(TransferError::InvalidOffer("file name has unsafe leading or trailing characters".into()));
    }
    Ok(())
}

/// A pending incoming file. It remains a hidden temporary file until the
/// declared size and SHA-256 digest have both been verified.
pub struct IncomingFile {
    offer: FileOffer,
    directory: PathBuf,
    temporary_path: PathBuf,
    file: Option<File>,
    hasher: Sha256,
    received: u64,
    committed: bool,
}

impl IncomingFile {
    pub fn create(directory: impl AsRef<Path>, offer: FileOffer) -> Result<Self, TransferError> {
        offer.validate()?;
        let directory = directory.as_ref().to_path_buf();
        fs::create_dir_all(&directory)?;
        let temporary_path = directory.join(format!(".bridge-{}.part", offer.transfer_id));
        let file = OpenOptions::new().write(true).create_new(true).open(&temporary_path)?;
        Ok(Self {
            offer, directory, temporary_path, file: Some(file),
            hasher: Sha256::new(), received: 0, committed: false,
        })
    }

    pub fn received_bytes(&self) -> u64 { self.received }
    pub fn total_bytes(&self) -> u64 { self.offer.file_size }

    pub fn write_chunk(&mut self, chunk: FileChunk) -> Result<(), TransferError> {
        if chunk.transfer_id != self.offer.transfer_id {
            return Err(TransferError::InvalidChunk("transfer ID mismatch".into()));
        }
        if chunk.offset != self.received {
            return Err(TransferError::InvalidChunk("chunk offset is not the next expected byte".into()));
        }
        if chunk.data.is_empty() || chunk.data.len() > CHUNK_DATA_BYTES {
            return Err(TransferError::InvalidChunk("chunk payload size is invalid".into()));
        }
        let next = self.received.checked_add(chunk.data.len() as u64)
            .ok_or_else(|| TransferError::InvalidChunk("received byte count overflow".into()))?;
        if next > self.offer.file_size {
            return Err(TransferError::InvalidChunk("chunk exceeds the offered file size".into()));
        }
        self.file.as_mut().ok_or(TransferError::Incomplete)?.write_all(&chunk.data)?;
        self.hasher.update(&chunk.data);
        self.received = next;
        Ok(())
    }

    /// Flush, verify the full digest, and atomically publish without overwriting
    /// an existing file. Returns the actual destination path.
    pub fn finish(mut self) -> Result<PathBuf, TransferError> {
        if self.received != self.offer.file_size { return Err(TransferError::Incomplete); }
        let actual = hex::encode(self.hasher.clone().finalize());
        if !actual.eq_ignore_ascii_case(&self.offer.sha256) {
            return Err(TransferError::ChecksumMismatch);
        }
        let mut file = self.file.take().ok_or(TransferError::Incomplete)?;
        file.flush()?;
        file.sync_all()?;
        drop(file);

        for index in 0_u32..10_000 {
            let name = if index == 0 {
                self.offer.file_name.clone()
            } else {
                let path = Path::new(&self.offer.file_name);
                let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("file");
                match path.extension().and_then(|s| s.to_str()) {
                    Some(extension) => format!("{stem} ({index}).{extension}"),
                    None => format!("{stem} ({index})"),
                }
            };
            let destination = self.directory.join(name);
            match fs::hard_link(&self.temporary_path, &destination) {
                Ok(()) => {
                    fs::remove_file(&self.temporary_path)?;
                    self.committed = true;
                    return Ok(destination);
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(TransferError::Io(error)),
            }
        }
        Err(TransferError::Io(io::Error::new(io::ErrorKind::AlreadyExists, "too many filename collisions")))
    }
}

impl Drop for IncomingFile {
    fn drop(&mut self) {
        if !self.committed { let _ = fs::remove_file(&self.temporary_path); }
    }
}


/// Maximum uncompressed BGRA8 display frame accepted by the transport.
pub const MAX_DISPLAY_FRAME_BYTES: usize = 64 * 1024 * 1024;
/// Each display fragment fits inside one authenticated Noise application message.
pub const DISPLAY_FRAME_FRAGMENT_BYTES: usize = 48 * 1024;
pub const DISPLAY_FRAME_PACKET_TAG: u8 = 0x04;
const DISPLAY_FRAME_FRAGMENT_HEADER_BYTES: usize = 1 + 8 + 4 + 4 + 4 + 8 + 4 + 4;

/// H.264 access units use a separate packet discriminator from raw BGRA frames.
pub const ENCODED_VIDEO_PACKET_TAG: u8 = 0x06;
/// Bound a single encoded access unit to 16 MiB, including large keyframes.
pub const MAX_ENCODED_VIDEO_FRAME_BYTES: usize = 16 * 1024 * 1024;
const ENCODED_VIDEO_FRAGMENT_HEADER_BYTES: usize = 1 + 8 + 4 + 4 + 8 + 4 + 4 + 1;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisplayFrameFragment {
    pub sequence: u64,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub timestamp_ns: u64,
    pub total_bytes: u32,
    pub offset: u32,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisplayFrame {
    pub sequence: u64,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub timestamp_ns: u64,
    pub pixels: Vec<u8>,
}


/// A tightly packed NV12 frame ready for a Media Foundation encoder input sample.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Nv12Frame {
    pub sequence: u64,
    pub width: u32,
    pub height: u32,
    pub timestamp_ns: u64,
    /// Y plane followed by interleaved U/V chroma plane.
    pub pixels: Vec<u8>,
}

/// Convert a tightly packed BGRA8 display frame to 8-bit BT.709 limited-range NV12.
/// Chroma is averaged over each 2x2 pixel block; NV12 requires even dimensions.
pub fn bgra_to_nv12(frame: &DisplayFrame) -> Result<Nv12Frame, TransferError> {
    let total_bytes = u32::try_from(frame.pixels.len())
        .map_err(|_| TransferError::InvalidChunk("BGRA frame is too large".into()))?;
    validate_display_frame_metadata(
        frame.sequence, frame.width, frame.height, frame.stride, total_bytes,
    )?;
    if !frame.width.is_multiple_of(2) || !frame.height.is_multiple_of(2) {
        return Err(TransferError::InvalidChunk("NV12 conversion requires even frame dimensions".into()));
    }
    let width = frame.width as usize;
    let height = frame.height as usize;
    let y_bytes = width.checked_mul(height)
        .ok_or_else(|| TransferError::InvalidChunk("NV12 luma size overflow".into()))?;
    let total_nv12 = y_bytes.checked_add(y_bytes / 2)
        .ok_or_else(|| TransferError::InvalidChunk("NV12 frame size overflow".into()))?;
    if total_nv12 > MAX_DISPLAY_FRAME_BYTES {
        return Err(TransferError::InvalidChunk("NV12 frame exceeds 64 MiB".into()));
    }
    let mut pixels = vec![0_u8; total_nv12];

    // BT.709 RGB-to-Y'CbCr coefficients with video-range offsets.
    let yuv = |pixel: usize| -> (u8, i32, i32) {
        let b = frame.pixels[pixel] as i32;
        let g = frame.pixels[pixel + 1] as i32;
        let r = frame.pixels[pixel + 2] as i32;
        let y = 16 + ((47 * r + 157 * g + 16 * b + 128) >> 8);
        let u = 128 + ((-26 * r - 87 * g + 112 * b + 128) >> 8);
        let v = 128 + ((112 * r - 102 * g - 10 * b + 128) >> 8);
        (y.clamp(16, 235) as u8, u, v)
    };

    for row in 0..height {
        for col in 0..width {
            let source = row * frame.stride as usize + col * 4;
            pixels[row * width + col] = yuv(source).0;
        }
    }
    let uv_start = y_bytes;
    for row in (0..height).step_by(2) {
        for col in (0..width).step_by(2) {
            let mut u_sum = 0_i32;
            let mut v_sum = 0_i32;
            for dy in 0..2 {
                for dx in 0..2 {
                    let source = (row + dy) * frame.stride as usize + (col + dx) * 4;
                    let (_, u, v) = yuv(source);
                    u_sum += u;
                    v_sum += v;
                }
            }
            let destination = uv_start + (row / 2) * width + col;
            pixels[destination] = ((u_sum + 2) / 4).clamp(16, 240) as u8;
            pixels[destination + 1] = ((v_sum + 2) / 4).clamp(16, 240) as u8;
        }
    }
    Ok(Nv12Frame {
        sequence: frame.sequence, width: frame.width, height: frame.height,
        timestamp_ns: frame.timestamp_ns, pixels,
    })
}

/// One bounded fragment of an Annex-B H.264 access unit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncodedVideoFrameFragment {
    pub sequence: u64,
    pub width: u32,
    pub height: u32,
    pub timestamp_ns: u64,
    pub total_bytes: u32,
    pub offset: u32,
    pub keyframe: bool,
    pub data: Vec<u8>,
}

/// Complete encoded H.264 access unit, ready for a decoder input buffer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncodedVideoFrame {
    pub sequence: u64,
    pub width: u32,
    pub height: u32,
    pub timestamp_ns: u64,
    pub keyframe: bool,
    /// Annex-B byte stream. The encoder is responsible for including SPS/PPS
    /// on keyframes or the receiver must obtain codec configuration separately.
    pub bitstream: Vec<u8>,
}

fn validate_encoded_video_metadata(
    sequence: u64, width: u32, height: u32, timestamp_ns: u64, total_bytes: u32,
) -> Result<(), TransferError> {
    if sequence == 0 || timestamp_ns == 0 || width == 0 || height == 0
        || width > 4096 || height > 4096
        || total_bytes == 0 || total_bytes as usize > MAX_ENCODED_VIDEO_FRAME_BYTES
    {
        return Err(TransferError::InvalidChunk("encoded video metadata is invalid or exceeds the 16 MiB access-unit limit".into()));
    }
    Ok(())
}

pub fn encode_encoded_video_fragment(
    fragment: &EncodedVideoFrameFragment,
) -> Result<Vec<u8>, TransferError> {
    validate_encoded_video_metadata(
        fragment.sequence, fragment.width, fragment.height,
        fragment.timestamp_ns, fragment.total_bytes,
    )?;
    let end = (fragment.offset as u64).checked_add(fragment.data.len() as u64)
        .ok_or_else(|| TransferError::InvalidChunk("encoded video fragment range overflow".into()))?;
    if fragment.data.is_empty() || fragment.data.len() > DISPLAY_FRAME_FRAGMENT_BYTES
        || end > fragment.total_bytes as u64
    {
        return Err(TransferError::InvalidChunk("encoded video fragment payload range is invalid".into()));
    }
    let mut bytes = Vec::with_capacity(ENCODED_VIDEO_FRAGMENT_HEADER_BYTES + fragment.data.len());
    bytes.push(ENCODED_VIDEO_PACKET_TAG);
    bytes.extend_from_slice(&fragment.sequence.to_be_bytes());
    bytes.extend_from_slice(&fragment.width.to_be_bytes());
    bytes.extend_from_slice(&fragment.height.to_be_bytes());
    bytes.extend_from_slice(&fragment.timestamp_ns.to_be_bytes());
    bytes.extend_from_slice(&fragment.total_bytes.to_be_bytes());
    bytes.extend_from_slice(&fragment.offset.to_be_bytes());
    bytes.push(u8::from(fragment.keyframe));
    bytes.extend_from_slice(&fragment.data);
    if bytes.len() > MAX_PACKET_BYTES {
        return Err(TransferError::InvalidChunk("encoded video fragment exceeds the encrypted message limit".into()));
    }
    Ok(bytes)
}

pub fn decode_encoded_video_fragment(
    bytes: &[u8],
) -> Result<EncodedVideoFrameFragment, TransferError> {
    if bytes.len() <= ENCODED_VIDEO_FRAGMENT_HEADER_BYTES || bytes.len() > MAX_PACKET_BYTES
        || bytes[0] != ENCODED_VIDEO_PACKET_TAG
    {
        return Err(TransferError::InvalidChunk("encoded video fragment packet size or tag is invalid".into()));
    }
    let u32_at = |offset: usize| -> Result<u32, TransferError> {
        let value: [u8; 4] = bytes.get(offset..offset + 4)
            .ok_or_else(|| TransferError::InvalidChunk("truncated encoded video header".into()))?
            .try_into().map_err(|_| TransferError::InvalidChunk("invalid encoded video field".into()))?;
        Ok(u32::from_be_bytes(value))
    };
    let u64_at = |offset: usize| -> Result<u64, TransferError> {
        let value: [u8; 8] = bytes.get(offset..offset + 8)
            .ok_or_else(|| TransferError::InvalidChunk("truncated encoded video header".into()))?
            .try_into().map_err(|_| TransferError::InvalidChunk("invalid encoded video field".into()))?;
        Ok(u64::from_be_bytes(value))
    };
    let keyframe = match bytes[33] {
        0 => false,
        1 => true,
        _ => return Err(TransferError::InvalidChunk("encoded video keyframe flag is invalid".into())),
    };
    let fragment = EncodedVideoFrameFragment {
        sequence: u64_at(1)?,
        width: u32_at(9)?,
        height: u32_at(13)?,
        timestamp_ns: u64_at(17)?,
        total_bytes: u32_at(25)?,
        offset: u32_at(29)?,
        keyframe,
        data: bytes[ENCODED_VIDEO_FRAGMENT_HEADER_BYTES..].to_vec(),
    };
    validate_encoded_video_metadata(
        fragment.sequence, fragment.width, fragment.height,
        fragment.timestamp_ns, fragment.total_bytes,
    )?;
    let end = (fragment.offset as u64).checked_add(fragment.data.len() as u64)
        .ok_or_else(|| TransferError::InvalidChunk("encoded video fragment range overflow".into()))?;
    if fragment.data.is_empty() || fragment.data.len() > DISPLAY_FRAME_FRAGMENT_BYTES
        || end > fragment.total_bytes as u64
    {
        return Err(TransferError::InvalidChunk("encoded video fragment payload range is invalid".into()));
    }
    Ok(fragment)
}

pub fn fragment_encoded_video_frame(
    frame: &EncodedVideoFrame,
) -> Result<Vec<Vec<u8>>, TransferError> {
    let total_bytes = u32::try_from(frame.bitstream.len())
        .map_err(|_| TransferError::InvalidChunk("encoded video access unit is too large".into()))?;
    validate_encoded_video_metadata(
        frame.sequence, frame.width, frame.height, frame.timestamp_ns, total_bytes,
    )?;
    let mut packets = Vec::with_capacity(frame.bitstream.len().div_ceil(DISPLAY_FRAME_FRAGMENT_BYTES));
    for (index, data) in frame.bitstream.chunks(DISPLAY_FRAME_FRAGMENT_BYTES).enumerate() {
        let offset = index.checked_mul(DISPLAY_FRAME_FRAGMENT_BYTES)
            .ok_or_else(|| TransferError::InvalidChunk("encoded video fragment offset overflow".into()))?;
        packets.push(encode_encoded_video_fragment(&EncodedVideoFrameFragment {
            sequence: frame.sequence,
            width: frame.width,
            height: frame.height,
            timestamp_ns: frame.timestamp_ns,
            total_bytes,
            offset: u32::try_from(offset)
                .map_err(|_| TransferError::InvalidChunk("encoded video fragment offset overflow".into()))?,
            keyframe: frame.keyframe,
            data: data.to_vec(),
        })?);
    }
    Ok(packets)
}

/// Reassembles one bounded H.264 access unit and rejects stale or reordered fragments.
#[derive(Default)]
pub struct EncodedVideoFrameAssembler {
    active: Option<EncodedVideoFrameFragment>,
    bitstream: Vec<u8>,
    last_completed_sequence: u64,
}

impl EncodedVideoFrameAssembler {
    pub fn reset(&mut self) {
        self.active = None;
        self.bitstream.clear();
    }

    pub fn push(&mut self, packet: &[u8]) -> Result<Option<EncodedVideoFrame>, TransferError> {
        let fragment = decode_encoded_video_fragment(packet)?;
        if fragment.offset == 0 {
            if self.active.is_some() || fragment.sequence <= self.last_completed_sequence {
                self.reset();
                return Err(TransferError::InvalidChunk("encoded video access unit is overlapping or stale".into()));
            }
            self.bitstream = Vec::with_capacity(fragment.total_bytes as usize);
            self.active = Some(fragment.clone());
        }
        let Some(first) = self.active.as_ref() else {
            return Err(TransferError::InvalidChunk("encoded video access unit must begin with offset zero".into()));
        };
        if fragment.sequence != first.sequence || fragment.width != first.width
            || fragment.height != first.height || fragment.timestamp_ns != first.timestamp_ns
            || fragment.total_bytes != first.total_bytes || fragment.keyframe != first.keyframe
            || fragment.offset as usize != self.bitstream.len()
        {
            self.reset();
            return Err(TransferError::InvalidChunk("encoded video fragments are out of order or have inconsistent metadata".into()));
        }
        self.bitstream.extend_from_slice(&fragment.data);
        if self.bitstream.len() == fragment.total_bytes as usize {
            let first = self.active.take().expect("active encoded frame exists");
            let bitstream = std::mem::take(&mut self.bitstream);
            self.last_completed_sequence = first.sequence;
            return Ok(Some(EncodedVideoFrame {
                sequence: first.sequence,
                width: first.width,
                height: first.height,
                timestamp_ns: first.timestamp_ns,
                keyframe: first.keyframe,
                bitstream,
            }));
        }
        Ok(None)
    }
}

fn validate_display_frame_metadata(
    sequence: u64, width: u32, height: u32, stride: u32, total_bytes: u32,
) -> Result<(), TransferError> {
    let expected_stride = width.checked_mul(4)
        .ok_or_else(|| TransferError::InvalidChunk("display frame stride overflow".into()))?;
    let expected_bytes = (stride as u64).checked_mul(height as u64)
        .ok_or_else(|| TransferError::InvalidChunk("display frame size overflow".into()))?;
    if sequence == 0 || width == 0 || height == 0 || width > 4096 || height > 4096
        || stride != expected_stride || expected_bytes == 0
        || expected_bytes > MAX_DISPLAY_FRAME_BYTES as u64
        || total_bytes as u64 != expected_bytes
    {
        return Err(TransferError::InvalidChunk("display frame metadata is invalid or exceeds 64 MiB".into()));
    }
    Ok(())
}

pub fn encode_display_frame_fragment(fragment: &DisplayFrameFragment) -> Result<Vec<u8>, TransferError> {
    validate_display_frame_metadata(fragment.sequence, fragment.width, fragment.height, fragment.stride, fragment.total_bytes)?;
    let end = (fragment.offset as u64).checked_add(fragment.data.len() as u64)
        .ok_or_else(|| TransferError::InvalidChunk("display fragment range overflow".into()))?;
    if fragment.data.is_empty() || fragment.data.len() > DISPLAY_FRAME_FRAGMENT_BYTES
        || end > fragment.total_bytes as u64
    {
        return Err(TransferError::InvalidChunk("display fragment payload range is invalid".into()));
    }
    let mut bytes = Vec::with_capacity(DISPLAY_FRAME_FRAGMENT_HEADER_BYTES + fragment.data.len());
    bytes.push(DISPLAY_FRAME_PACKET_TAG);
    bytes.extend_from_slice(&fragment.sequence.to_be_bytes());
    bytes.extend_from_slice(&fragment.width.to_be_bytes());
    bytes.extend_from_slice(&fragment.height.to_be_bytes());
    bytes.extend_from_slice(&fragment.stride.to_be_bytes());
    bytes.extend_from_slice(&fragment.timestamp_ns.to_be_bytes());
    bytes.extend_from_slice(&fragment.total_bytes.to_be_bytes());
    bytes.extend_from_slice(&fragment.offset.to_be_bytes());
    bytes.extend_from_slice(&fragment.data);
    if bytes.len() > MAX_PACKET_BYTES {
        return Err(TransferError::InvalidChunk("display fragment exceeds the encrypted message limit".into()));
    }
    Ok(bytes)
}

pub fn decode_display_frame_fragment(bytes: &[u8]) -> Result<DisplayFrameFragment, TransferError> {
    if bytes.len() <= DISPLAY_FRAME_FRAGMENT_HEADER_BYTES || bytes.len() > MAX_PACKET_BYTES
        || bytes[0] != DISPLAY_FRAME_PACKET_TAG
    {
        return Err(TransferError::InvalidChunk("display fragment packet size or tag is invalid".into()));
    }
    let u32_at = |offset: usize| -> Result<u32, TransferError> {
        let value: [u8; 4] = bytes.get(offset..offset + 4)
            .ok_or_else(|| TransferError::InvalidChunk("truncated display fragment header".into()))?
            .try_into().map_err(|_| TransferError::InvalidChunk("invalid display fragment field".into()))?;
        Ok(u32::from_be_bytes(value))
    };
    let u64_at = |offset: usize| -> Result<u64, TransferError> {
        let value: [u8; 8] = bytes.get(offset..offset + 8)
            .ok_or_else(|| TransferError::InvalidChunk("truncated display fragment header".into()))?
            .try_into().map_err(|_| TransferError::InvalidChunk("invalid display fragment field".into()))?;
        Ok(u64::from_be_bytes(value))
    };
    let fragment = DisplayFrameFragment {
        sequence: u64_at(1)?,
        width: u32_at(9)?,
        height: u32_at(13)?,
        stride: u32_at(17)?,
        timestamp_ns: u64_at(21)?,
        total_bytes: u32_at(29)?,
        offset: u32_at(33)?,
        data: bytes[DISPLAY_FRAME_FRAGMENT_HEADER_BYTES..].to_vec(),
    };
    validate_display_frame_metadata(fragment.sequence, fragment.width, fragment.height, fragment.stride, fragment.total_bytes)?;
    let end = (fragment.offset as u64).checked_add(fragment.data.len() as u64)
        .ok_or_else(|| TransferError::InvalidChunk("display fragment range overflow".into()))?;
    if fragment.data.is_empty() || fragment.data.len() > DISPLAY_FRAME_FRAGMENT_BYTES
        || end > fragment.total_bytes as u64
    {
        return Err(TransferError::InvalidChunk("display fragment payload range is invalid".into()));
    }
    Ok(fragment)
}

/// Splits a validated BGRA8 frame into bounded packets for the Noise transport.
pub fn fragment_display_frame(frame: &DisplayFrame) -> Result<Vec<Vec<u8>>, TransferError> {
    let total_bytes = u32::try_from(frame.pixels.len())
        .map_err(|_| TransferError::InvalidChunk("display frame is too large".into()))?;
    validate_display_frame_metadata(frame.sequence, frame.width, frame.height, frame.stride, total_bytes)?;
    let mut packets = Vec::with_capacity(frame.pixels.len().div_ceil(DISPLAY_FRAME_FRAGMENT_BYTES));
    for (index, data) in frame.pixels.chunks(DISPLAY_FRAME_FRAGMENT_BYTES).enumerate() {
        let offset = index.checked_mul(DISPLAY_FRAME_FRAGMENT_BYTES)
            .ok_or_else(|| TransferError::InvalidChunk("display fragment offset overflow".into()))?;
        packets.push(encode_display_frame_fragment(&DisplayFrameFragment {
            sequence: frame.sequence, width: frame.width, height: frame.height,
            stride: frame.stride, timestamp_ns: frame.timestamp_ns, total_bytes,
            offset: u32::try_from(offset).map_err(|_| TransferError::InvalidChunk("display fragment offset overflow".into()))?,
            data: data.to_vec(),
        })?);
    }
    Ok(packets)
}

/// Reassembles one frame at a time and never allocates beyond the 64 MiB frame cap.
#[derive(Default)]
pub struct DisplayFrameAssembler {
    active: Option<DisplayFrameFragment>,
    pixels: Vec<u8>,
}

impl DisplayFrameAssembler {
    pub fn reset(&mut self) {
        self.active = None;
        self.pixels.clear();
    }

    pub fn push(&mut self, packet: &[u8]) -> Result<Option<DisplayFrame>, TransferError> {
        let fragment = decode_display_frame_fragment(packet)?;
        if fragment.offset == 0 {
            if self.active.is_some() {
                return Err(TransferError::InvalidChunk("a new display frame arrived before the previous frame completed".into()));
            }
            self.pixels = Vec::with_capacity(fragment.total_bytes as usize);
            self.active = Some(fragment.clone());
        }
        let Some(first) = self.active.as_ref() else {
            return Err(TransferError::InvalidChunk("display frame must begin with offset zero".into()));
        };
        if fragment.sequence != first.sequence || fragment.width != first.width
            || fragment.height != first.height || fragment.stride != first.stride
            || fragment.timestamp_ns != first.timestamp_ns || fragment.total_bytes != first.total_bytes
            || fragment.offset as usize != self.pixels.len()
        {
            self.reset();
            return Err(TransferError::InvalidChunk("display frame fragments are out of order or have inconsistent metadata".into()));
        }
        self.pixels.extend_from_slice(&fragment.data);
        if self.pixels.len() == fragment.total_bytes as usize {
            let first = self.active.take().expect("active frame exists");
            let pixels = std::mem::take(&mut self.pixels);
            return Ok(Some(DisplayFrame {
                sequence: first.sequence, width: first.width, height: first.height,
                stride: first.stride, timestamp_ns: first.timestamp_ns, pixels,
            }));
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{env, time::{SystemTime, UNIX_EPOCH}};

    fn temp_dir() -> PathBuf {
        let unique = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let path = env::temp_dir().join(format!("bridge-transfer-test-{unique}"));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn offer(data: &[u8], name: &str) -> FileOffer {
        FileOffer {
            transfer_id: Uuid::new_v4(), file_name: name.into(), file_size: data.len() as u64,
            sha256: hex::encode(Sha256::digest(data)),
        }
    }

    #[test]
    fn bgra_to_nv12_converts_black_and_white_to_video_range() {
        let mut pixels = Vec::new();
        for _ in 0..4 { pixels.extend_from_slice(&[0, 0, 0, 255]); }
        for _ in 0..4 { pixels.extend_from_slice(&[255, 255, 255, 255]); }
        let frame = DisplayFrame { sequence: 7, width: 4, height: 2, stride: 16, timestamp_ns: 99, pixels };
        let converted = bgra_to_nv12(&frame).unwrap();
        assert_eq!((converted.sequence, converted.width, converted.height, converted.timestamp_ns), (7, 4, 2, 99));
        assert_eq!(&converted.pixels[..4], &[16, 16, 16, 16]);
        assert_eq!(&converted.pixels[4..8], &[235, 235, 235, 235]);
        assert_eq!(converted.pixels.len(), 12);
        assert_eq!(&converted.pixels[8..], &[128, 128, 128, 128]);
    }

    #[test]
    fn bgra_to_nv12_averages_chroma_over_each_2x2_block() {
        let pixels = vec![
            0, 0, 255, 255, 0, 0, 255, 255,
            255, 0, 0, 255, 255, 0, 0, 255,
        ];
        let frame = DisplayFrame { sequence: 1, width: 2, height: 2, stride: 8, timestamp_ns: 1, pixels };
        let converted = bgra_to_nv12(&frame).unwrap();
        assert_eq!(converted.pixels.len(), 6);
        assert_eq!(&converted.pixels[4..], &[171, 179]);
    }

    #[test]
    fn bgra_to_nv12_rejects_odd_dimensions_and_invalid_source_layout() {
        let odd = DisplayFrame { sequence: 1, width: 3, height: 2, stride: 12, timestamp_ns: 1, pixels: vec![0; 24] };
        assert!(bgra_to_nv12(&odd).is_err());
        let malformed = DisplayFrame { sequence: 1, width: 2, height: 2, stride: 8, timestamp_ns: 1, pixels: vec![0; 15] };
        assert!(bgra_to_nv12(&malformed).is_err());
    }

    #[test]
    fn control_round_trip_and_offer_validation() {
        let offer = offer(b"hello", "hello.txt");
        let encoded = encode_control(&TransferControl::FileOffer { offer: offer.clone() }).unwrap();
        assert_eq!(decode_control(&encoded).unwrap(), TransferControl::FileOffer { offer });
    }

    #[test]
    fn display_challenge_messages_round_trip_and_reject_malformed_proofs() {
        let request_id = Uuid::new_v4();
        let session_id = Uuid::new_v4();
        let nonce = "a5".repeat(32);
        let response = TransferControl::DisplayChallengeResponse {
            request_id,
            session_id,
            nonce: nonce.clone(),
            device_fingerprint: "b6".repeat(32),
            signature: "c7".repeat(64),
        };
        for message in [
            TransferControl::DisplayChallengeRequest { request_id, session_id, nonce: nonce.clone() },
            response.clone(),
        ] {
            let encoded = encode_control(&message).unwrap();
            assert_eq!(decode_control(&encoded).unwrap(), message);
            let packet = TransferPacket::Control(message.clone());
            assert_eq!(decode_packet(&encode_packet(&packet).unwrap()).unwrap(), packet);
        }
        let malformed = TransferControl::DisplayChallengeRequest {
            request_id, session_id, nonce: "not-a-nonce".into(),
        };
        assert!(encode_control(&malformed).is_err());
        let malformed_proof = TransferControl::DisplayChallengeResponse {
            request_id,
            session_id,
            nonce: nonce.clone(),
            device_fingerprint: "b6".repeat(32),
            signature: "bad".into(),
        };
        assert!(encode_control(&malformed_proof).is_err());
        let malformed_json = format!(r#"{{"type":"display_challenge_response","request_id":"{request_id}","session_id":"{session_id}","nonce":"bad","device_fingerprint":"{}","signature":"{}"}}"#, "b6".repeat(32), "c7".repeat(64));
        assert!(decode_control(malformed_json.as_bytes()).is_err());
    }

    #[test]
    fn rejects_path_traversal_and_invalid_names() {
        for name in ["../secret", "..", "folder/file.txt", "folder\\\\file.txt", "bad\nname"] {
            assert!(offer(b"x", name).validate().is_err(), "accepted {name:?}");
        }
    }

    #[test]
    fn chunks_are_bounded_and_round_trip() {
        let chunk = FileChunk { transfer_id: Uuid::new_v4(), offset: 4096, data: vec![7; CHUNK_DATA_BYTES] };
        assert_eq!(decode_chunk(&encode_chunk(&chunk).unwrap()).unwrap(), chunk);
        assert!(encode_chunk(&FileChunk { data: vec![0; CHUNK_DATA_BYTES + 1], ..chunk }).is_err());
    }

    #[test]
    fn transfer_packets_keep_control_and_binary_chunks_unambiguous() {
        let offer = offer(b"packet", "packet.bin");
        let control = TransferPacket::Control(TransferControl::FileOffer { offer });
        assert_eq!(decode_packet(&encode_packet(&control).unwrap()).unwrap(), control);

        let chunk = TransferPacket::Chunk(FileChunk {
            transfer_id: Uuid::new_v4(), offset: 0, data: vec![0, 0xff, 0x42, 0x7f],
        });
        assert_eq!(decode_packet(&encode_packet(&chunk).unwrap()).unwrap(), chunk);
        assert!(decode_packet(&[0xff, 0x00]).is_err());
        assert!(decode_packet(&[CONTROL_PACKET_TAG]).is_err());
    }

    #[test]
    fn outgoing_file_streams_bounded_chunks_and_verifies_source() {
        let directory = temp_dir();
        let data = vec![0x5a; CHUNK_DATA_BYTES + 17];
        let path = directory.join("outgoing.bin");
        fs::write(&path, &data).unwrap();
        let offer = offer_for_file(&path).unwrap();
        let mut outgoing = OutgoingFile::open(&path, offer.clone()).unwrap();
        let first = outgoing.next_chunk().unwrap().unwrap();
        let second = outgoing.next_chunk().unwrap().unwrap();
        assert_eq!(first.offset, 0);
        assert_eq!(first.data.len(), CHUNK_DATA_BYTES);
        assert_eq!(second.offset, CHUNK_DATA_BYTES as u64);
        assert_eq!(second.data, vec![0x5a; 17]);
        assert!(outgoing.next_chunk().unwrap().is_none());
        assert!(outgoing.next_chunk().unwrap().is_none());

        let mut changed = OutgoingFile::open(&path, offer).unwrap();
        fs::write(&path, b"changed after offer").unwrap();
        assert!(matches!(changed.next_chunk(), Err(TransferError::SourceChanged)));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn outgoing_file_supports_empty_files() {
        let directory = temp_dir();
        let path = directory.join("empty.txt");
        fs::write(&path, []).unwrap();
        let offer = offer_for_file(&path).unwrap();
        let mut outgoing = OutgoingFile::open(&path, offer).unwrap();
        assert!(outgoing.next_chunk().unwrap().is_none());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn receives_verifies_and_never_overwrites_existing_file() {
        let directory = temp_dir();
        let data = b"Bridge encrypted file transfer";
        fs::write(directory.join("report.txt"), b"existing").unwrap();
        let offer = offer(data, "report.txt");
        let mut incoming = IncomingFile::create(&directory, offer.clone()).unwrap();
        for (index, part) in data.chunks(5).enumerate() {
            incoming.write_chunk(FileChunk { transfer_id: offer.transfer_id, offset: (index * 5) as u64, data: part.to_vec() }).unwrap();
        }
        let destination = incoming.finish().unwrap();
        assert_eq!(destination.file_name().unwrap(), "report (1).txt");
        assert_eq!(fs::read(destination).unwrap(), data);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn rejects_out_of_order_chunks_and_bad_digest() {
        let directory = temp_dir();
        let data = b"abcdef";
        let mut offer = offer(data, "data.bin");
        offer.sha256 = "0".repeat(64);
        let mut incoming = IncomingFile::create(&directory, offer.clone()).unwrap();
        assert!(incoming.write_chunk(FileChunk { transfer_id: offer.transfer_id, offset: 1, data: b"bc".to_vec() }).is_err());
        incoming.write_chunk(FileChunk { transfer_id: offer.transfer_id, offset: 0, data: data.to_vec() }).unwrap();
        assert!(matches!(incoming.finish(), Err(TransferError::ChecksumMismatch)));
        fs::remove_dir_all(directory).unwrap();
    }



    #[test]
    fn display_input_packets_round_trip_and_reject_invalid_samples() {
        let event = DisplayInputEvent {
            sequence: 42,
            timestamp_ns: 123_456,
            pointer_id: 2,
            action: DisplayInputAction::Move,
            x: 0,
            y: u16::MAX,
        };
        let packet = TransferPacket::DisplayInput(event);
        let encoded = encode_packet(&packet).unwrap();
        assert_eq!(encoded.len(), DISPLAY_INPUT_PACKET_BYTES);
        assert_eq!(decode_packet(&encoded).unwrap(), packet);

        for invalid in [
            DisplayInputEvent { sequence: 0, ..event },
            DisplayInputEvent { timestamp_ns: 0, ..event },
            DisplayInputEvent { pointer_id: MAX_DISPLAY_INPUT_POINTERS, ..event },
        ] {
            assert!(encode_packet(&TransferPacket::DisplayInput(invalid)).is_err());
        }

        let mut malformed_action = encoded.clone();
        malformed_action[18] = 0xff;
        assert!(decode_packet(&malformed_action).is_err());
        assert!(decode_packet(&encoded[..encoded.len() - 1]).is_err());
    }

    #[test]
    fn encoded_video_fragments_round_trip_through_packet_decoder() {
        let frame = EncodedVideoFrame {
            sequence: 11,
            width: 1080,
            height: 2340,
            timestamp_ns: 987_654,
            keyframe: true,
            bitstream: vec![0x65; DISPLAY_FRAME_FRAGMENT_BYTES + 31],
        };
        let packets = fragment_encoded_video_frame(&frame).unwrap();
        assert!(packets.len() > 1);
        assert!(packets.iter().all(|packet| packet.len() <= MAX_PACKET_BYTES));
        let first = decode_packet(&packets[0]).unwrap();
        assert!(matches!(first, TransferPacket::EncodedVideoFrame(_)));
        let mut assembler = EncodedVideoFrameAssembler::default();
        for packet in &packets[..packets.len() - 1] {
            assert!(assembler.push(packet).unwrap().is_none());
        }
        assert_eq!(assembler.push(packets.last().unwrap()).unwrap(), Some(frame));
        assert!(assembler.push(&packets[0]).is_err());
    }

    #[test]
    fn encoded_video_rejects_invalid_metadata_flags_and_oversized_access_units() {
        let mut invalid = EncodedVideoFrame {
            sequence: 1, width: 1920, height: 1080, timestamp_ns: 1,
            keyframe: false, bitstream: vec![0x41; 10],
        };
        invalid.sequence = 0;
        assert!(fragment_encoded_video_frame(&invalid).is_err());
        invalid.sequence = 1;
        invalid.bitstream = vec![0; MAX_ENCODED_VIDEO_FRAME_BYTES + 1];
        assert!(fragment_encoded_video_frame(&invalid).is_err());

        let fragment = EncodedVideoFrameFragment {
            sequence: 1, width: 2, height: 2, timestamp_ns: 1,
            total_bytes: 4, offset: 0, keyframe: true, data: vec![1, 2, 3, 4],
        };
        let mut encoded = encode_encoded_video_fragment(&fragment).unwrap();
        encoded[33] = 2;
        assert!(decode_encoded_video_fragment(&encoded).is_err());
    }

    #[test]
    fn encoded_video_assembler_rejects_out_of_order_fragments() {
        let frame = EncodedVideoFrame {
            sequence: 3, width: 16, height: 16, timestamp_ns: 9,
            keyframe: false, bitstream: vec![0x41; DISPLAY_FRAME_FRAGMENT_BYTES + 4],
        };
        let packets = fragment_encoded_video_frame(&frame).unwrap();
        let mut assembler = EncodedVideoFrameAssembler::default();
        assert!(assembler.push(&packets[1]).is_err());
        assert!(assembler.push(&packets[0]).unwrap().is_none());
        assert!(assembler.push(&packets[0]).is_err());
    }

    #[test]
    fn display_frame_fragments_use_the_shared_transfer_packet_decoder() {
        let fragment = DisplayFrameFragment {
            sequence: 2, width: 2, height: 2, stride: 8, timestamp_ns: 7,
            total_bytes: 16, offset: 0, data: vec![0x44; 16],
        };
        let packet = TransferPacket::DisplayFrame(fragment.clone());
        let encoded = encode_packet(&packet).unwrap();
        assert_eq!(decode_packet(&encoded).unwrap(), packet);
    }

    #[test]
    fn display_frames_fragment_and_reassemble_in_order() {
        let frame = DisplayFrame {
            sequence: 7, width: 128, height: 128, stride: 512, timestamp_ns: 99,
            pixels: vec![0x5a; 512 * 128],
        };
        let packets = fragment_display_frame(&frame).unwrap();
        assert!(packets.len() > 1);
        assert!(packets.iter().all(|packet| packet.len() <= MAX_PACKET_BYTES));
        let mut assembler = DisplayFrameAssembler::default();
        for packet in &packets[..packets.len() - 1] {
            assert!(assembler.push(packet).unwrap().is_none());
        }
        assert_eq!(assembler.push(packets.last().unwrap()).unwrap(), Some(frame));
    }

    #[test]
    fn display_frame_assembler_rejects_out_of_order_fragments() {
        let frame = DisplayFrame {
            sequence: 8, width: 128, height: 128, stride: 512, timestamp_ns: 100,
            pixels: vec![0x33; 512 * 128],
        };
        let packets = fragment_display_frame(&frame).unwrap();
        let mut assembler = DisplayFrameAssembler::default();
        assert!(assembler.push(&packets[1]).is_err());
        assert!(assembler.push(&packets[0]).unwrap().is_none());
        assert!(assembler.push(&packets[0]).is_err());
    }

    #[test]
    fn display_frame_transport_rejects_invalid_metadata_and_oversized_fragments() {
        let invalid = DisplayFrame {
            sequence: 1, width: 10, height: 10, stride: 40, timestamp_ns: 1,
            pixels: vec![0; 399],
        };
        assert!(fragment_display_frame(&invalid).is_err());
        let fragment = DisplayFrameFragment {
            sequence: 1, width: 2, height: 2, stride: 8, timestamp_ns: 1,
            total_bytes: 16, offset: 0, data: vec![0; DISPLAY_FRAME_FRAGMENT_BYTES + 1],
        };
        assert!(encode_display_frame_fragment(&fragment).is_err());
    }

}
