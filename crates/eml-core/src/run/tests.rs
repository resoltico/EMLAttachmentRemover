use super::*;
use std::{fs, path::Path};
#[test]
fn final_existing_observation_distinguishes_address_and_same_file_changes() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/run-tests");
    fs::create_dir_all(&base).unwrap();
    let root = tempfile::tempdir_in(base).unwrap();
    fs::write(root.path().join("copy"), b"owned").unwrap();
    fs::write(root.path().join("other"), b"other").unwrap();
    let context = crate::selection::WorkingContext::capture(root.path()).unwrap();
    let directory = context
        .directory(Path::new("."))
        .unwrap_or_else(|_| panic!("Owned directory must bind"));
    let original = directory
        .observe_child(std::ffi::OsStr::new("copy"))
        .unwrap();
    let expected = original.file.unwrap();
    check_existing_observation(expected, Some(original)).unwrap();
    let missing = directory
        .observe_child_if_present(std::ffi::OsStr::new("missing"))
        .unwrap();
    let absent = check_existing_observation(expected, missing).unwrap_err();
    assert_eq!(
        (absent.code(), absent.phase()),
        (ErrorCode::AddressChanged, ErrorPhase::Address)
    );
    let other = directory
        .observe_child(std::ffi::OsStr::new("other"))
        .unwrap();
    let different = check_existing_observation(expected, Some(other)).unwrap_err();
    assert_eq!(
        (different.code(), different.phase()),
        (ErrorCode::AddressChanged, ErrorPhase::Address)
    );
    fs::write(root.path().join("copy"), b"changed owned contents").unwrap();
    let changed = directory
        .observe_child(std::ffi::OsStr::new("copy"))
        .unwrap();
    assert_eq!(changed.identity, expected.identity);
    let same_file = check_existing_observation(expected, Some(changed)).unwrap_err();
    assert_eq!(
        (same_file.code(), same_file.phase()),
        (ErrorCode::DestinationExists, ErrorPhase::Existing)
    );
    // A typed same-ID type inconsistency is a classifier control, not an OS
    // file-to-directory conversion or interference experiment.
    let mut inconsistent = original;
    inconsistent.kind = EntryKind::Special;
    inconsistent.file = None;
    let kind = check_existing_observation(expected, Some(inconsistent)).unwrap_err();
    assert_eq!(
        (kind.code(), kind.phase()),
        (ErrorCode::DestinationExists, ErrorPhase::Existing)
    );
    drop(directory);
    drop(context);
    root.close().unwrap();
}
#[test]
fn stage_checkpoint_origin_is_distinct_from_a_native_interrupted_error() {
    for checkpoint_stopped in [false, true] {
        let sync = crate::fs::SyncReceipt {
            method: crate::fs::SyncMethod::Fsync,
            state: crate::fs::SyncState::Succeeded,
            native_code: None,
        };
        let original = crate::fs::StageFailure {
            operation: crate::fs::StageOperation::Write,
            error: std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "Scripted operation interruption",
            ),
            file_sync: Some(sync),
            cleanup: CleanupReceipt {
                state: CleanupState::Residual,
                residual_name: Some(".owned-stage".into()),
                native_code: Some(5),
            },
        };
        let (error, receipt, cleanup) = stage_failure(original, checkpoint_stopped);
        assert_eq!(
            error.code(),
            if checkpoint_stopped {
                ErrorCode::Interrupted
            } else {
                ErrorCode::WriteFailed
            }
        );
        assert_eq!(error.phase(), ErrorPhase::Write);
        assert_eq!(receipt, Some(sync));
        assert_eq!(cleanup.state, CleanupState::Residual);
        assert_eq!(cleanup.native_code, Some(5));
        assert_eq!(
            error.native_error().unwrap().kind(),
            std::io::ErrorKind::Interrupted
        );
    }
}
