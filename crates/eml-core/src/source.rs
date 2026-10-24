//! Bounded native reading and the one pure MIME preparation capability.
use crate::{
    ErrorCode, ErrorPhase, MAX_MESSAGE_BYTES, PreparationError, PreparedMessage, Refusal,
    RunControl,
};
use eml_fs::{BoundFile, BoundFileError, FileSignature};
use std::io;

const READ_CHUNK_BYTES: usize = 64 * 1024;

/// Native source preparation failed before producing a verified held-source value.
#[derive(Debug)]
pub enum SourcePreparationError {
    /// Cooperative cancellation while obtaining or validating source bytes.
    InterruptedRead,
    /// Native binding/type facts or inability to inspect them.
    Binding(BoundFileError),
    /// Actual positional data-read failure, distinct from metadata inspection.
    Read(io::Error),
    /// Existing pure preparation refusal/interruption, or a stable raw-size refusal.
    Preparation(PreparationError),
}
impl SourcePreparationError {
    /// Classify through the one core semantic vocabulary, never error prose.
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::InterruptedRead => ErrorCode::Interrupted,
            Self::Binding(BoundFileError::NotRegular) => ErrorCode::SourceNotRegular,
            Self::Binding(BoundFileError::Changed) => ErrorCode::SourceChanged,
            Self::Binding(BoundFileError::ObservationFailed(_)) | Self::Read(_) => {
                ErrorCode::ReadFailed
            }
            Self::Preparation(error) => error.code(),
        }
    }
    /// Phase of this preparation operation. Later publication-time source
    /// revalidation returns native facts separately and chooses its own context.
    pub const fn phase(&self) -> ErrorPhase {
        match self {
            Self::Preparation(error) => error.phase(),
            Self::InterruptedRead | Self::Binding(_) | Self::Read(_) => ErrorPhase::Read,
        }
    }
}
impl std::fmt::Display for SourcePreparationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InterruptedRead => f.write_str("Source reading interrupted"),
            Self::Binding(error) => std::fmt::Display::fmt(error, f),
            Self::Read(error) => write!(f, "Source data read failed: {error}"),
            Self::Preparation(error) => std::fmt::Display::fmt(error, f),
        }
    }
}
impl std::error::Error for SourcePreparationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InterruptedRead => None,
            Self::Binding(error) => Some(error),
            Self::Read(error) => Some(error),
            Self::Preparation(error) => Some(error),
        }
    }
}
impl From<BoundFileError> for SourcePreparationError {
    fn from(error: BoundFileError) -> Self {
        Self::Binding(error)
    }
}
impl From<PreparationError> for SourcePreparationError {
    fn from(error: PreparationError) -> Self {
        Self::Preparation(error)
    }
}

/// Independently verified MIME output tied to the original read-only held file.
/// Construction is private; source bytes are not retained after preparation.
/// This is not a filesystem snapshot, publication receipt or run outcome.
pub struct PreparedSource {
    source: BoundFile,
    message: PreparedMessage,
}
impl PreparedSource {
    pub(super) fn identity(&self) -> eml_fs::Identity {
        self.source.initial_signature().identity
    }
    /// Immutable output bytes and metadata from the existing pure preparer.
    pub fn message(&self) -> &PreparedMessage {
        &self.message
    }
    /// Initial length, qualified against actual EOF/read count during preparation.
    pub fn source_length(&self) -> u64 {
        self.source.initial_signature().size
    }
    /// Reobserve the held source and its binding without assigning a workflow
    /// phase. A later publisher must still call this at its required boundaries.
    /// Success is a current observation, not permanent or atomic stability.
    pub fn revalidate(&self) -> Result<(), BoundFileError> {
        self.source.check_binding()
    }
    /// Nonshipping qualification readback from the same retained source handle.
    /// This observation supplies no later operation authority.
    #[cfg(all(windows, any(test, feature = "native-fixtures")))]
    pub fn fixture_source_observation(&self) -> std::io::Result<String> {
        self.source.fixture_source_observation()
    }
}

fn read_checkpoint(control: &RunControl) -> Result<(), SourcePreparationError> {
    if control.is_stopped() {
        Err(SourcePreparationError::InterruptedRead)
    } else {
        Ok(())
    }
}
fn validate_binding(
    source: &BoundFile,
    control: &RunControl,
) -> Result<(), SourcePreparationError> {
    loop {
        read_checkpoint(control)?;
        match source.check_binding() {
            Err(BoundFileError::ObservationFailed(error))
                if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error.into()),
            Ok(()) => return read_checkpoint(control),
        }
    }
}
enum ObtainedBytes {
    Complete(Vec<u8>),
    OverLimit,
}

// The private limit parameter permits small independent boundary/cancellation
// tests. Production always supplies the one MAX_MESSAGE_BYTES policy authority.
fn obtain_bytes(
    mut read: impl FnMut(&mut [u8], u64) -> io::Result<usize>,
    initial_size: u64,
    control: &RunControl,
    limit: usize,
) -> Result<ObtainedBytes, SourcePreparationError> {
    let cap = limit
        .checked_add(1)
        .expect("Raw limit must fit its EOF probe");
    let mut bytes = Vec::with_capacity(initial_size.min(cap as u64) as usize);
    let mut buffer = [0; READ_CHUNK_BYTES];
    loop {
        read_checkpoint(control)?;
        if bytes.len() == cap {
            return Ok(ObtainedBytes::OverLimit);
        }
        let offset = bytes.len();
        let length = READ_CHUNK_BYTES.min(cap - offset);
        let amount = match read(&mut buffer[..length], offset as u64) {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(SourcePreparationError::Read(error)),
            Ok(amount) => amount,
        };
        read_checkpoint(control)?;
        if amount == 0 {
            if offset as u64 != initial_size {
                return Err(BoundFileError::Changed.into());
            }
            return Ok(ObtainedBytes::Complete(bytes));
        }
        if (offset + amount) as u64 > initial_size {
            // A positive data-length difference already disqualifies this source.
            // Do not grow the allocation or mistake it for a stable size refusal.
            return Err(BoundFileError::Changed.into());
        }
        bytes.extend_from_slice(&buffer[..amount]);
    }
}

/// Read one ordinary held source and independently prepare its body-only bytes.
///
/// Inventory equality, bounded reads through EOF (or one byte over the raw limit),
/// native signature/binding checks and cooperative checkpoints precede a result.
/// Source reads are read-only. No destinations, staging, synchronization, run
/// metadata budget, event delivery or global signal state are involved.
/// The original file remains held by a successful value until that value drops.
pub fn prepare_source(
    source: BoundFile,
    inventory: FileSignature,
    control: &RunControl,
) -> Result<PreparedSource, SourcePreparationError> {
    read_checkpoint(control)?;
    let initial = source.initial_signature();
    if initial != inventory {
        return Err(BoundFileError::Changed.into());
    }
    validate_binding(&source, control)?;
    let obtained = obtain_bytes(
        |buffer, offset| source.read_at(buffer, offset),
        initial.size,
        control,
        MAX_MESSAGE_BYTES,
    )?;
    // A capped prefix is not a stable oversize refusal until these checks pass.
    validate_binding(&source, control)?;
    let bytes = match obtained {
        ObtainedBytes::OverLimit => {
            return Err(crate::error(
                Refusal::LimitExceeded,
                "Raw source exceeds the admitted byte limit",
            )
            .into());
        }
        ObtainedBytes::Complete(bytes) => bytes,
    };
    let message = crate::prepare_message(&bytes, control)?;
    drop(bytes);
    validate_binding(&source, control)?;
    Ok(PreparedSource { source, message })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;
    fn changed(result: Result<ObtainedBytes, SourcePreparationError>) {
        assert!(matches!(
            result,
            Err(SourcePreparationError::Binding(BoundFileError::Changed))
        ));
    }
    #[test]
    fn short_reads_preserve_all_bytes_with_bounded_buffers_and_offsets() {
        let expected = vec![b'x'; READ_CHUNK_BYTES * 2 + 17];
        let mut next = 0;
        let result = obtain_bytes(
            |buffer, offset| {
                assert_eq!(offset as usize, next);
                assert!(buffer.len() <= READ_CHUNK_BYTES);
                let amount = buffer.len().min(10007).min(expected.len() - next);
                buffer[..amount].copy_from_slice(&expected[next..next + amount]);
                next += amount;
                Ok(amount)
            },
            expected.len() as u64,
            &RunControl::default(),
            expected.len() + 10,
        )
        .unwrap();
        let ObtainedBytes::Complete(actual) = result else {
            panic!("Expected actual EOF");
        };
        assert_eq!(actual, expected);
    }
    #[test]
    fn exact_limit_requires_a_one_byte_eof_probe_after_a_short_read() {
        let mut calls = Vec::new();
        let result = obtain_bytes(
            |buffer, offset| {
                calls.push((buffer.len(), offset));
                let count = if offset == 0 { 7 } else { 0 };
                buffer[..count].fill(b'x');
                Ok(count)
            },
            7,
            &RunControl::default(),
            7,
        )
        .unwrap();
        assert!(matches!(result, ObtainedBytes::Complete(bytes) if bytes == b"xxxxxxx"));
        assert_eq!(calls, [(8, 0), (1, 7)]);
    }
    #[test]
    fn an_oversized_prefix_stops_at_limit_plus_one_without_inventing_eof() {
        let mut calls = 0;
        let result = obtain_bytes(
            |buffer, offset| {
                calls += 1;
                assert_eq!(offset, 0);
                assert_eq!(buffer.len(), 8);
                buffer.fill(b'x');
                Ok(8)
            },
            1000,
            &RunControl::default(),
            7,
        )
        .unwrap();
        assert!(matches!(result, ObtainedBytes::OverLimit));
        assert_eq!(calls, 1);
    }
    #[test]
    fn observed_growth_or_early_eof_is_changed_not_a_stable_size_refusal() {
        changed(obtain_bytes(
            |buffer, _| {
                buffer.fill(b'x');
                Ok(8)
            },
            7,
            &RunControl::default(),
            7,
        ));
        changed(obtain_bytes(|_, _| Ok(0), 1000, &RunControl::default(), 7));
    }
    #[test]
    fn interrupted_reads_retry_only_after_a_cooperative_checkpoint() {
        let control = RunControl::default();
        let mut calls = 0;
        let result = obtain_bytes(
            |_, _| {
                calls += 1;
                control.request_stop();
                Err(io::Error::from(io::ErrorKind::Interrupted))
            },
            1,
            &control,
            1,
        );
        assert!(matches!(
            result,
            Err(SourcePreparationError::InterruptedRead)
        ));
        assert_eq!(calls, 1);
        let mut calls = 0;
        let result = obtain_bytes(
            |buffer, offset| {
                calls += 1;
                if calls == 1 {
                    return Err(io::Error::from(io::ErrorKind::Interrupted));
                }
                if offset == 0 {
                    buffer[0] = b'x';
                    Ok(1)
                } else {
                    Ok(0)
                }
            },
            1,
            &RunControl::default(),
            1,
        )
        .unwrap();
        assert!(matches!(result, ObtainedBytes::Complete(bytes) if bytes == b"x"));
        assert_eq!(calls, 3);
    }
    #[test]
    fn stop_at_eof_or_size_boundary_precedes_acceptance_or_refusal() {
        for (initial, amount) in [(0, 0), (1000, 8)] {
            let control = RunControl::default();
            let result = obtain_bytes(
                |buffer, _| {
                    buffer[..amount].fill(b'x');
                    control.request_stop();
                    Ok(amount)
                },
                initial,
                &control,
                7,
            );
            assert!(matches!(
                result,
                Err(SourcePreparationError::InterruptedRead)
            ));
        }
    }
    #[test]
    fn data_io_metadata_and_preparation_causes_keep_their_operation_context() {
        let error = match obtain_bytes(
            |_, _| Err(io::Error::from_raw_os_error(5)),
            1,
            &RunControl::default(),
            1,
        ) {
            Err(error) => error,
            Ok(_) => panic!("A real read error cannot produce bytes"),
        };
        assert!(matches!(&error, SourcePreparationError::Read(_)));
        assert_eq!(
            error
                .source()
                .unwrap()
                .downcast_ref::<io::Error>()
                .unwrap()
                .raw_os_error(),
            Some(5)
        );
        assert_eq!(error.code(), ErrorCode::ReadFailed);
        assert_eq!(error.phase(), ErrorPhase::Read);
        let metadata = SourcePreparationError::Binding(BoundFileError::ObservationFailed(
            io::Error::new(io::ErrorKind::InvalidData, "Unusable metadata"),
        ));
        assert_eq!(metadata.code(), ErrorCode::ReadFailed);
        assert_eq!(metadata.phase(), ErrorPhase::Read);
        assert_eq!(
            SourcePreparationError::InterruptedRead.phase(),
            ErrorPhase::Read
        );
        let preparation = SourcePreparationError::Preparation(PreparationError::Interrupted);
        assert_eq!(preparation.code(), ErrorCode::Interrupted);
        assert_eq!(preparation.phase(), ErrorPhase::Prepare);
    }
}
