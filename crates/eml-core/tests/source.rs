//! Real synthetic held-file preparation; no CLI, publication or copied outputs.
#![cfg(any(unix, windows))]
use eml_core::fs::BoundFile;
#[cfg(unix)]
use eml_core::fs::BoundFileError;
use eml_core::{
    ErrorCode, ErrorPhase, MAX_MESSAGE_BYTES, RunControl, SourcePreparationError, prepare_source,
};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

const INPUT: &[u8] = include_bytes!("../../../tests/contract/mime/inputs/01-gmail-attachment.eml");
const GOLDEN: &[u8] = include_bytes!("../../../tests/contract/mime/golden/01-gmail-attachment.eml");
fn root() -> tempfile::TempDir {
    let parent = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/source-tests");
    fs::create_dir_all(&parent).unwrap();
    tempfile::tempdir_in(parent).unwrap()
}
fn bind(path: &Path) -> BoundFile {
    #[cfg(unix)]
    {
        eml_core::fs::BoundDirectory::open(path.parent().unwrap())
            .unwrap()
            .open_file(path.file_name().unwrap(), false)
            .unwrap()
    }
    #[cfg(windows)]
    {
        BoundFile::open(
            &eml_core::fs::PathContext::capture(&std::env::current_dir().unwrap())
                .unwrap()
                .resolve(path)
                .unwrap(),
            false,
        )
        .unwrap()
    }
}
fn source(root: &Path, bytes: &[u8]) -> (PathBuf, BoundFile) {
    let path = root.join("source.eml");
    fs::write(&path, bytes).unwrap();
    let file = bind(&path);
    (path, file)
}
fn failure<T>(result: Result<T, SourcePreparationError>) -> SourcePreparationError {
    match result {
        Err(error) => error,
        Ok(_) => panic!("Expected native preparation refusal"),
    }
}
#[test]
fn actual_source_prepares_independent_golden_and_keeps_the_original_file_held() {
    let root = root();
    let (path, file) = source(root.path(), INPUT);
    let signature = file.initial_signature();
    let prepared = prepare_source(file, signature, &RunControl::default()).unwrap();
    assert_eq!(prepared.message().as_bytes(), GOLDEN);
    assert_eq!(prepared.source_length(), INPUT.len() as u64);
    assert_eq!(prepared.message().removed().len(), 1);
    assert_eq!(
        prepared.message().removed()[0].filename.as_deref(),
        Some("report.pdf")
    );
    prepared.revalidate().unwrap();
    assert_eq!(fs::read(&path).unwrap(), INPUT);
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    #[cfg(windows)]
    {
        let denied = fs::OpenOptions::new().write(true).open(&path).unwrap_err();
        assert_eq!(denied.raw_os_error(), Some(32));
    }
    drop(prepared);
    root.close().unwrap();
}
#[test]
fn inventory_mismatch_refuses_before_preparing_a_different_source_version() {
    let root = root();
    let (path, file) = source(root.path(), INPUT);
    let signature = file.initial_signature();
    drop(file);
    fs::write(&path, b"changed after inventory").unwrap();
    let file = bind(&path);
    let error = failure(prepare_source(file, signature, &RunControl::default()));
    assert_eq!(error.code(), ErrorCode::SourceChanged);
    assert_eq!(error.phase(), ErrorPhase::Read);
    assert_eq!(fs::read(&path).unwrap(), b"changed after inventory");
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    root.close().unwrap();
}
#[test]
fn pre_requested_stop_is_read_interruption_and_creates_no_entries() {
    let root = root();
    let (path, file) = source(root.path(), INPUT);
    let signature = file.initial_signature();
    let control = RunControl::default();
    control.request_stop();
    let error = failure(prepare_source(file, signature, &control));
    assert!(matches!(error, SourcePreparationError::InterruptedRead));
    assert_eq!(error.code(), ErrorCode::Interrupted);
    assert_eq!(error.phase(), ErrorPhase::Read);
    assert_eq!(fs::read(&path).unwrap(), INPUT);
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    root.close().unwrap();
}
#[test]
fn real_protected_source_uses_existing_preparation_refusal_and_preserves_bytes() {
    let root = root();
    let input = include_bytes!("../../../tests/contract/mime/inputs/06-smime-signed.eml");
    let (path, file) = source(root.path(), input);
    let signature = file.initial_signature();
    let error = failure(prepare_source(file, signature, &RunControl::default()));
    assert_eq!(error.code(), ErrorCode::Protected);
    assert_eq!(error.phase(), ErrorPhase::Prepare);
    assert_eq!(fs::read(&path).unwrap(), input);
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    root.close().unwrap();
}
#[cfg(unix)]
#[test]
fn immutable_prepared_bytes_do_not_claim_permanent_source_stability() {
    let root = root();
    let (path, file) = source(root.path(), INPUT);
    let signature = file.initial_signature();
    let prepared = prepare_source(file, signature, &RunControl::default()).unwrap();
    fs::write(&path, b"observable later change").unwrap();
    assert!(matches!(
        prepared.revalidate(),
        Err(BoundFileError::Changed)
    ));
    assert_eq!(prepared.message().as_bytes(), GOLDEN);
    drop(prepared);
    root.close().unwrap();
}
#[test]
fn actual_eof_at_raw_limit_is_admitted_but_one_byte_more_is_refused() {
    for extra in [0, 1] {
        let root = root();
        let path = root.path().join("source.eml");
        let mut writer = fs::File::create_new(&path).unwrap();
        writer.set_len((MAX_MESSAGE_BYTES + extra) as u64).unwrap();
        let header = b"Content-Type: text/plain\r\n\r\n";
        writer.write_all(header).unwrap();
        drop(writer);
        let file = bind(&path);
        let signature = file.initial_signature();
        let result = prepare_source(file, signature, &RunControl::default());
        if extra == 0 {
            let prepared = result.unwrap();
            assert_eq!(prepared.source_length(), MAX_MESSAGE_BYTES as u64);
            assert_eq!(&prepared.message().as_bytes()[..header.len()], header);
            assert_eq!(prepared.message().as_bytes().len(), MAX_MESSAGE_BYTES);
            assert!(
                prepared.message().as_bytes()[header.len()..]
                    .iter()
                    .all(|byte| *byte == 0)
            );
            assert!(prepared.message().removed().is_empty());
            prepared.revalidate().unwrap();
            drop(prepared);
        } else {
            let error = failure(result);
            assert_eq!(error.code(), ErrorCode::LimitExceeded);
            assert_eq!(error.phase(), ErrorPhase::Prepare);
        }
        assert_eq!(
            fs::metadata(&path).unwrap().len(),
            (MAX_MESSAGE_BYTES + extra) as u64
        );
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
        root.close().unwrap();
    }
}
