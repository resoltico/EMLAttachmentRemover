//! Native file/entry observations, independent of frontend encoding or policy.
use crate::Identity;

/// Observable file change signature, excluding ordinary read-access timestamps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileSignature {
    /// Physical object, never a path-derived identifier.
    pub identity: Identity,
    /// Observed byte length.
    pub size: u64,
    pub(crate) modified_seconds: i64,
    pub(crate) modified_nanoseconds: i64,
    pub(crate) changed_seconds: i64,
    pub(crate) changed_nanoseconds: i64,
}
/// Native entry type observed without opening a special file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryKind {
    /// Ordinary regular file.
    File,
    /// Ordinary directory.
    Directory,
    /// A final symbolic link, when final-link following was disabled.
    Symlink,
    /// A Windows reparse entry, including junctions.
    Reparse,
    /// FIFO/socket/device or other unsupported kind.
    Special,
}
/// Snapshot metadata, revalidated through a later held handle before processing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EntryObservation {
    /// Physical native object identity.
    pub identity: Identity,
    /// Native kind, not derived from filename extension.
    pub kind: EntryKind,
    /// Change signature for ordinary files only.
    pub file: Option<FileSignature>,
}
