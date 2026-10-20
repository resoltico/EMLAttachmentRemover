//! Bounded native address observations before any candidate content is written.
use super::{handle, path};
use crate::{EntryKind, Identity};
use std::{
    ffi::OsStr,
    fs::File,
    io,
    os::windows::{ffi::OsStrExt, io::AsRawHandle},
};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_NAME_NORMALIZED, GetFinalPathNameByHandleW, VOLUME_NAME_DOS,
};

/// Physical creation-address knowledge; Core assigns business error codes.
#[derive(Debug)]
pub enum ContainmentFailure {
    /// An observed parent, type or path differs from the admitted relation.
    Changed,
    /// Native observation could not establish the relation.
    Unverified(io::Error),
}
impl std::fmt::Display for ContainmentFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Changed => f.write_str("Created object address differs from the bound parent"),
            Self::Unverified(_) => f.write_str("Created object address could not be verified"),
        }
    }
}
impl std::error::Error for ContainmentFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Unverified(error) => Some(error),
            Self::Changed => None,
        }
    }
}
pub(super) fn changed() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, ContainmentFailure::Changed)
}
fn unverified(error: io::Error) -> io::Error {
    io::Error::other(ContainmentFailure::Unverified(error))
}
pub(super) fn final_path(file: &File) -> io::Result<Vec<u16>> {
    let mut buffer = vec![0u16; path::MAX_EXTENDED_CONTENT + 1];
    // SAFETY: live borrowed native handle and initialized bounded UTF16 buffer.
    // Identical normalized/DOS flags are used for parent and child observations.
    let written = unsafe {
        GetFinalPathNameByHandleW(
            file.as_raw_handle(),
            buffer.as_mut_ptr(),
            buffer.len() as u32,
            FILE_NAME_NORMALIZED | VOLUME_NAME_DOS,
        )
    } as usize;
    if written == 0 {
        return Err(io::Error::last_os_error());
    }
    if written >= buffer.len() {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Native final path exceeds the supported bound",
        ));
    }
    buffer.truncate(written);
    if buffer.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Native final path contains NUL",
        ));
    }
    Ok(buffer)
}
pub(super) fn append(parent: &[u16], name: &OsStr) -> io::Result<Vec<u16>> {
    let mut path = parent.to_vec();
    if path.last() != Some(&u16::from(b'\\')) {
        path.push(u16::from(b'\\'));
    }
    path.extend(name.encode_wide());
    if path.len() > super::path::MAX_EXTENDED_CONTENT {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Creation path exceeds the supported native bound",
        ));
    }
    Ok(path)
}
fn ordinary_parent(parent: &File, expected: Identity) -> io::Result<()> {
    let observed = handle::entry_observation(parent).map_err(unverified)?;
    if observed.kind != EntryKind::Directory || observed.identity != expected {
        return Err(changed());
    }
    Ok(())
}
pub(super) fn capture_parent(parent: &File, expected: Identity) -> io::Result<Vec<u16>> {
    ordinary_parent(parent, expected)?;
    let path = final_path(parent).map_err(unverified)?;
    ordinary_parent(parent, expected)?;
    Ok(path)
}
pub(super) fn verify(
    parent: &File,
    expected: Identity,
    captured: &[u16],
    child: &File,
    name: &OsStr,
    kind: EntryKind,
) -> io::Result<()> {
    ordinary_parent(parent, expected)?;
    let live = final_path(parent).map_err(unverified)?;
    let actual = final_path(child).map_err(unverified)?;
    let observed = handle::entry_observation(child).map_err(unverified)?;
    let same_volume = match (expected, observed.identity) {
        (Identity::Windows { volume: a, .. }, Identity::Windows { volume: b, .. }) => a == b,
        _ => false,
    };
    if live != captured || actual != append(&live, name)? || observed.kind != kind || !same_volume {
        return Err(changed());
    }
    ordinary_parent(parent, expected)
}
