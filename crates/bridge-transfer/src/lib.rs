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
}

pub fn encode_control(message: &TransferControl) -> Result<Vec<u8>, TransferError> {
    match message {
        TransferControl::FileOffer { offer } => offer.validate()?,
        TransferControl::FileReject { reason, .. } if reason.len() > 256 =>
            return Err(TransferError::InvalidOffer("rejection reason is too long".into())),
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
    Ok(message)
}

#[derive(Debug, PartialEq, Eq)]
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

fn validate_file_name(name: &str) -> Result<(), TransferError> {
    if name.is_empty() || name.len() > MAX_FILENAME_BYTES || name == "." || name == ".." {
        return Err(TransferError::InvalidOffer("file name is empty, too long, or reserved".into()));
    }
    if name.chars().any(|ch| ch.is_control() || ch == '/' || ch == '\\\\' || ch == ':' ) {
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
    fn control_round_trip_and_offer_validation() {
        let offer = offer(b"hello", "hello.txt");
        let encoded = encode_control(&TransferControl::FileOffer { offer: offer.clone() }).unwrap();
        assert_eq!(decode_control(&encoded).unwrap(), TransferControl::FileOffer { offer });
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
}
