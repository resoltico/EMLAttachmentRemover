//! Native file/entry observations, independent of frontend encoding or policy.
use crate::Identity;

/// Observable file change signature, excluding ordinary read-access timestamps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileSignature {
    /// Physical object, never a path-derived identifier.
    pub identity: Identity,
    /// Observed byte length.
    pub size: u64,
    // Equality-only backend-native values: POSIX seconds/nanoseconds;
    // Windows100ns ticks/0. No access time or cross-platform time conversion.
    pub(crate) modified_native: [i64; 2],
    pub(crate) changed_native: [i64; 2],
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
