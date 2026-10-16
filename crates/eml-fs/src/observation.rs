//! Native file/entry observations, independent of frontend encoding or policy.
use crate::Identity;
use std::io;

/// Native facts from opening or revalidating an ordinary bound file.
/// This type also applies to existing destinations; workflow phases belong to core.
#[derive(Debug)]
pub enum BoundFileError {
    /// Initial type observation did not establish an ordinary regular disk file.
    NotRegular,
    /// A native comparison proved a change from the captured ordinary binding.
    Changed,
    /// Inspection failed or returned unusable metadata; no change is inferred.
    ObservationFailed(io::Error),
}
impl BoundFileError {
    /// Exact native error when the failed observation supplied one.
    pub fn native_code(&self) -> Option<i64> {
        match self {
            Self::ObservationFailed(error) => error.raw_os_error().map(i64::from),
            Self::NotRegular | Self::Changed => None,
        }
    }
    /// An already ordinary held file cannot acquire a different observed type
    /// without changing its captured binding. Query failures remain unverified.
    pub(crate) fn against_regular_baseline(self) -> Self {
        match self {
            Self::NotRegular => Self::Changed,
            other => other,
        }
    }
    pub(crate) fn into_io_error(self) -> io::Error {
        match self {
            Self::ObservationFailed(error) => error,
            Self::NotRegular => io::Error::new(io::ErrorKind::InvalidInput, self),
            Self::Changed => io::Error::new(io::ErrorKind::InvalidData, self),
        }
    }
}
impl From<io::Error> for BoundFileError {
    fn from(error: io::Error) -> Self {
        Self::ObservationFailed(error)
    }
}
#[cfg(unix)]
impl From<rustix::io::Errno> for BoundFileError {
    fn from(error: rustix::io::Errno) -> Self {
        Self::ObservationFailed(error.into())
    }
}
impl std::fmt::Display for BoundFileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotRegular => f.write_str("Opened object is not an ordinary regular disk file"),
            Self::Changed => f.write_str("Bound file identity, entry, parent or signature changed"),
            Self::ObservationFailed(error) => write!(f, "File observation failed: {error}"),
        }
    }
}
impl std::error::Error for BoundFileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ObservationFailed(error) => Some(error),
            Self::NotRegular | Self::Changed => None,
        }
    }
}

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
/// Native entry type observed without opening object data.
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
/// Metadata observation, revalidated through a later held handle before processing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EntryObservation {
    /// Physical native object identity.
    pub identity: Identity,
    /// Native kind, not derived from filename extension.
    pub kind: EntryKind,
    /// Change signature for ordinary files only.
    pub file: Option<FileSignature>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;
    #[test]
    fn failed_observations_retain_native_cause_through_inventory_adapter() {
        let error = BoundFileError::from(io::Error::from_raw_os_error(5));
        assert_eq!(error.native_code(), Some(5));
        assert_eq!(
            error
                .source()
                .unwrap()
                .downcast_ref::<io::Error>()
                .unwrap()
                .raw_os_error(),
            Some(5)
        );
        let io = error.into_io_error();
        assert_eq!(io.raw_os_error(), Some(5));
    }
    #[test]
    fn only_observed_type_changes_upgrade_against_an_ordinary_baseline() {
        assert!(matches!(
            BoundFileError::NotRegular.against_regular_baseline(),
            BoundFileError::Changed
        ));
        let failed = BoundFileError::from(io::Error::new(
            io::ErrorKind::InvalidData,
            "Unusable observation",
        ));
        assert!(matches!(
            failed.against_regular_baseline(),
            BoundFileError::ObservationFailed(_)
        ));
        assert!(matches!(
            BoundFileError::Changed.against_regular_baseline(),
            BoundFileError::Changed
        ));
    }
}
