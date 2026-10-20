//! Bounded native archive inspection/readback on one immutable snapshot.
//! Structural/material evidence is separate from executable qualification.
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, OpenOptions},
    io::{self, Cursor, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

/// Shipping archives are bounded independently of advertised expanded sizes.
pub const MAX_ARCHIVE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_DIRECTORY_BYTES: u32 = 8 * 1024 * 1024;
const MAX_ENTRIES: u16 = 4096;
const MAX_NAME_BYTES: usize = 1024;
const MAX_PATH_DEPTH: usize = 8;
const MAX_CLAIM_TEXT_BYTES: usize = 16 * 1024 * 1024;
const MAX_MEMBER_BYTES: u64 = 256 * 1024 * 1024;
const MAX_EXPANDED_BYTES: u64 = 1024 * 1024 * 1024;

/// One immutable bounded copy used by inspection and extraction alike.
pub struct ArchiveSnapshot {
    bytes: Vec<u8>,
}

/// Byte identity of the immutable inspected input, not review/behavior quality.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ByteIdentity {
    /// Exact snapshot byte length.
    pub byte_length: usize,
    /// SHA-256 of these same immutable bytes.
    pub sha256: String,
}
impl ByteIdentity {
    /// Identify bytes without implying source provenance or review quality.
    pub fn of(bytes: &[u8]) -> Self {
        Self {
            byte_length: bytes.len(),
            sha256: Sha256::digest(bytes)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
        }
    }
}
mod enclosure;
mod layout;
mod tar_format;
mod zip_format;

pub use enclosure::{ArtifactDirectory, create_artifact_directory};
pub use layout::validate_member_name;
use layout::*;
pub use tar_format::encode_cli_tar_gz;
pub use zip_format::{ZipMember, encode_cli_zip, inspect_zip_directory};

/// Extracted CLI bytes inside a retained private enclosure. Dropping this value
/// closes held descriptors but does not remove useful files or failure evidence.
/// Retain it through bounded pathname-operation observation. If settlement is
/// unknown, abort later work and preserve files; handles can then close without
/// sweeping/removing unverified resources. The full-chain policy excludes
/// interference by the trusted UID/root.
pub struct ExtractedCli {
    root: PathBuf,
    _enclosure: ArtifactDirectory,
}
impl ExtractedCli {
    /// Package directory beneath the private enclosure.
    pub fn root(&self) -> &Path {
        &self.root
    }
}

impl ArchiveSnapshot {
    /// Borrow the immutable bytes used by inspection; native extraction receives
    /// an owned copy of these bytes, never a second read of the caller's path.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    /// Identify the same snapshot used by all parsing and extraction passes.
    pub fn identity(&self) -> ByteIdentity {
        ByteIdentity::of(&self.bytes)
    }
    /// Read a snapshot without trusting an advertised file length for allocation.
    pub fn read(path: &Path) -> io::Result<Self> {
        if !fs::symlink_metadata(path)?.is_file() {
            return Err(invalid(
                "Archive input must be an ordinary non-symlink file",
            ));
        }
        let mut open = OpenOptions::new();
        open.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            open.custom_flags(
                (rustix::fs::OFlags::NONBLOCK | rustix::fs::OFlags::NOFOLLOW).bits() as i32,
            );
        }
        let file = open.open(path)?;
        if !file.metadata()?.is_file() {
            return Err(invalid("Archive input must be an ordinary file"));
        }
        let mut bytes = Vec::new();
        file.take(MAX_ARCHIVE_BYTES + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_ARCHIVE_BYTES {
            return Err(invalid("Archive snapshot exceeds its bound"));
        }
        Ok(Self { bytes })
    }
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests;
