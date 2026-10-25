//! Bounded read-only comparison against the one verified prepared source.
use crate::fs::{BoundFile, BoundFileError, FileSignature, Identity};
use crate::{PreparedSource, RunControl};
use std::io;

const COMPARE_CHUNK_BYTES: usize = 64 * 1024;

/// Operation that prevented a read-only existing-copy comparison.
#[derive(Debug)]
pub enum ExistingComparisonError {
    /// Cooperative stop before any publication or synchronization.
    Interrupted,
    /// The existing entry is the held source itself, including a hard link.
    SourceAlias,
    /// The held source could not be revalidated; the native distinction remains.
    Source(BoundFileError),
    /// The existing entry could not be revalidated; no change is guessed on I/O error.
    Existing(BoundFileError),
    /// Reading the existing file failed at the data boundary.
    Read(io::Error),
}
impl std::fmt::Display for ExistingComparisonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Interrupted => f.write_str("Existing-copy comparison interrupted"),
            Self::SourceAlias => f.write_str("Existing entry aliases the held source"),
            Self::Source(error) => write!(f, "Source revalidation failed: {error}"),
            Self::Existing(error) => write!(f, "Existing-entry revalidation failed: {error}"),
            Self::Read(error) => write!(f, "Existing-copy data read failed: {error}"),
        }
    }
}
impl std::error::Error for ExistingComparisonError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Source(error) | Self::Existing(error) => Some(error),
            Self::Read(error) => Some(error),
            Self::Interrupted | Self::SourceAlias => None,
        }
    }
}
/// Failed comparison retains what was actually observed, independently of its cause.
#[derive(Debug)]
pub struct ExistingComparisonFailure {
    identity: Identity,
    bytes_equal: Option<bool>,
    stable: bool,
    error: ExistingComparisonError,
}
impl ExistingComparisonFailure {
    pub(super) fn into_error(self) -> ExistingComparisonError {
        self.error
    }
    /// Identity from the actual opened ordinary file, even if later observations failed.
    pub fn identity(&self) -> Identity {
        self.identity
    }
    /// Comparison result only if reached; a read error does not imply different bytes.
    pub fn bytes_equal(&self) -> Option<bool> {
        self.bytes_equal
    }
    /// Whether both existing-entry binding checks completed without change.
    pub fn stable(&self) -> bool {
        self.stable
    }
    /// Exact operation/cause; terminal workflow classification belongs to the caller.
    pub fn error(&self) -> &ExistingComparisonError {
        &self.error
    }
}
impl std::fmt::Display for ExistingComparisonFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.error.fmt(f)
    }
}
impl std::error::Error for ExistingComparisonFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}
/// Current stable byte comparison, retaining the same read-only ordinary file.
/// Equality is not an UpToDate result, write capability or synchronization receipt.
pub struct ComparedExisting {
    file: BoundFile,
    equal: bool,
}
impl ComparedExisting {
    pub(super) fn sync_file(&self) -> crate::fs::SyncReceipt {
        self.file.sync_file()
    }
    /// Original native signature qualified before and after the bounded comparison.
    pub fn signature(&self) -> FileSignature {
        self.file.initial_signature()
    }
    /// Whether actual length and bounded reads through EOF matched verified bytes.
    pub fn bytes_equal(&self) -> bool {
        self.equal
    }
    /// Reobserve this same held file and entry; no stable-forever promise is made.
    pub fn revalidate(&self) -> Result<(), BoundFileError> {
        self.file.check_binding()
    }
}
fn checkpoint(control: &RunControl) -> Result<(), ExistingComparisonError> {
    if control.is_stopped() {
        Err(ExistingComparisonError::Interrupted)
    } else {
        Ok(())
    }
}
fn validate(
    control: &RunControl,
    mut check: impl FnMut() -> Result<(), BoundFileError>,
    source: bool,
) -> Result<(), ExistingComparisonError> {
    loop {
        checkpoint(control)?;
        match check() {
            Err(BoundFileError::ObservationFailed(error))
                if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => {
                return Err(if source {
                    ExistingComparisonError::Source(error)
                } else {
                    ExistingComparisonError::Existing(error)
                });
            }
            Ok(()) => return checkpoint(control),
        }
    }
}
fn compare_bytes(
    expected: &[u8],
    length: u64,
    control: &RunControl,
    mut read: impl FnMut(&mut [u8], u64) -> io::Result<usize>,
) -> Result<bool, ExistingComparisonError> {
    checkpoint(control)?;
    if length != expected.len() as u64 {
        return Ok(false);
    }
    let mut buffer = [0; COMPARE_CHUNK_BYTES];
    let mut offset = 0;
    loop {
        checkpoint(control)?;
        // Exactly one additional byte distinguishes EOF from extra data.
        let capacity = buffer.len().min(expected.len() - offset + 1);
        let amount = match read(&mut buffer[..capacity], offset as u64) {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(ExistingComparisonError::Read(error)),
            Ok(amount) => amount,
        };
        checkpoint(control)?;
        if amount == 0 {
            return Ok(offset == expected.len());
        }
        if amount > expected.len() - offset || buffer[..amount] != expected[offset..offset + amount]
        {
            return Ok(false);
        }
        offset += amount;
    }
}
/// Compare an already opened no-follow existing destination with privately verified
/// prepared bytes. This reads no more than candidate length plus one byte and never
/// flushes, writes, creates, or replaces the held file with a pathname lookup.
/// Ordinary reads may update access times; no metadata setters are called.
///
/// The workflow must separately reject identities reserved by other sources/earlier
/// publications, establish the destination parent, and qualify its requested address.
/// Only the currently prepared source's identity is available to this operation.
pub fn compare_existing(
    source: &PreparedSource,
    existing: BoundFile,
    control: &RunControl,
) -> Result<ComparedExisting, ExistingComparisonFailure> {
    let initial = existing.initial_signature();
    let mut bytes_equal = None;
    let mut stable = false;
    let result = (|| {
        validate(control, || source.revalidate(), true)?;
        validate(control, || existing.check_binding(), false)?;
        if initial.identity == source.identity() {
            return Err(ExistingComparisonError::SourceAlias);
        }
        bytes_equal = Some(compare_bytes(
            source.message().as_bytes(),
            initial.size,
            control,
            |buffer, offset| existing.read_at(buffer, offset),
        )?);
        // A mismatch also needs a truthful current binding, rather than hiding a change.
        validate(control, || existing.check_binding(), false)?;
        stable = true;
        validate(control, || source.revalidate(), true)?;
        Ok(())
    })();
    match result {
        Ok(()) => Ok(ComparedExisting {
            file: existing,
            equal: bytes_equal.expect("successful comparison obtained a byte observation"),
        }),
        Err(error) => Err(ExistingComparisonFailure {
            identity: initial.identity,
            bytes_equal,
            stable,
            error,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn short_reads_compare_across_chunks_and_require_actual_eof() {
        let expected: Vec<_> = (0..COMPARE_CHUNK_BYTES * 2 + 17)
            .map(|n| (n % 251) as u8)
            .collect();
        let mut calls = 0;
        let mut consumed = 0;
        assert!(
            compare_bytes(
                &expected,
                expected.len() as u64,
                &RunControl::default(),
                |buffer, offset| {
                    assert_eq!(offset as usize, consumed);
                    assert!(buffer.len() <= 65_536);
                    let amount = 10007.min(buffer.len()).min(expected.len() - consumed);
                    buffer[..amount].copy_from_slice(&expected[consumed..consumed + amount]);
                    consumed += amount;
                    calls += 1;
                    Ok(amount)
                }
            )
            .unwrap()
        );
        assert_eq!(consumed, expected.len());
        assert!(calls > 14);
    }
    #[test]
    fn length_mismatch_reads_nothing_and_empty_equality_still_probes_eof() {
        assert!(
            !compare_bytes(b"abc", 4, &RunControl::default(), |_, _| panic!(
                "Different length needs no data reads"
            ))
            .unwrap()
        );
        let mut calls = 0;
        assert!(
            compare_bytes(b"", 0, &RunControl::default(), |buffer, offset| {
                assert_eq!((buffer.len(), offset), (1, 0));
                calls += 1;
                Ok(0)
            })
            .unwrap()
        );
        assert_eq!(calls, 1);
    }
    #[test]
    fn extra_byte_early_eof_and_same_length_difference_are_unequal() {
        for actual in [b"abcd".as_slice(), b"ab", b"axc"] {
            let mut read = 0;
            assert!(
                !compare_bytes(b"abc", 3, &RunControl::default(), |buffer, offset| {
                    assert_eq!(offset as usize, read);
                    let amount = buffer.len().min(actual.len() - read);
                    buffer[..amount].copy_from_slice(&actual[read..read + amount]);
                    read += amount;
                    Ok(amount)
                })
                .unwrap()
            );
            assert!(read <= 4);
        }
    }
    #[test]
    fn interrupted_reads_retry_but_a_stop_during_read_or_eof_wins() {
        let mut calls = 0;
        assert!(
            compare_bytes(b"a", 1, &RunControl::default(), |buffer, offset| {
                calls += 1;
                if calls == 1 {
                    return Err(io::ErrorKind::Interrupted.into());
                }
                if offset == 0 {
                    buffer[0] = b'a';
                    Ok(1)
                } else {
                    Ok(0)
                }
            })
            .unwrap()
        );
        assert_eq!(calls, 3);
        for at_eof in [false, true] {
            let control = RunControl::default();
            let result = compare_bytes(
                if at_eof { b"" } else { b"a" },
                u64::from(!at_eof),
                &control,
                |_, _| {
                    control.request_stop();
                    if at_eof {
                        Ok(0)
                    } else {
                        Err(io::ErrorKind::Interrupted.into())
                    }
                },
            );
            assert!(matches!(result, Err(ExistingComparisonError::Interrupted)));
        }
    }
    #[test]
    fn native_data_read_error_is_not_a_byte_difference_or_binding_error() {
        use std::error::Error;
        let result = compare_bytes(b"a", 1, &RunControl::default(), |_, _| {
            Err(io::Error::from_raw_os_error(5))
        });
        let Err(error) = result else {
            panic!("Read error must survive");
        };
        assert!(matches!(error, ExistingComparisonError::Read(_)));
        assert_eq!(
            error
                .source()
                .unwrap()
                .downcast_ref::<io::Error>()
                .unwrap()
                .raw_os_error(),
            Some(5)
        );
    }
}
