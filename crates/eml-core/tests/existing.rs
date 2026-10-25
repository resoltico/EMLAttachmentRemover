//! Actual owned-file comparison; no synchronization or publication qualification.
#![cfg(any(unix, windows))]
use eml_core::fs::BoundFile;
use eml_core::{
    ExistingComparisonError, PreparedSource, RunControl, compare_existing, prepare_source,
};
use std::{fs, path::Path};
const INPUT: &[u8] = include_bytes!("../../../tests/contract/mime/inputs/01-gmail-attachment.eml");
const GOLDEN: &[u8] = include_bytes!("../../../tests/contract/mime/golden/01-gmail-attachment.eml");
fn root() -> tempfile::TempDir {
    let parent = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/existing-tests");
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
        let context = eml_core::fs::PathContext::capture(path.parent().unwrap()).unwrap();
        BoundFile::open(&context.resolve(path).unwrap(), false).unwrap()
    }
}
fn prepared(root: &Path) -> PreparedSource {
    let file = bind(&root.join("source.eml"));
    let signature = file.initial_signature();
    prepare_source(file, signature, &RunControl::default()).unwrap()
}
#[test]
fn matching_golden_retains_the_actual_file_and_preserves_both_contents() {
    let root = root();
    let input = root.path().join("source.eml");
    let output = root.path().join("copy.eml");
    fs::write(&input, INPUT).unwrap();
    fs::write(&output, GOLDEN).unwrap();
    let source = prepared(root.path());
    let file = bind(&output);
    let signature = file.initial_signature();
    let compared = compare_existing(&source, file, &RunControl::default()).unwrap();
    assert!(compared.bytes_equal());
    assert_eq!(compared.signature(), signature);
    compared.revalidate().unwrap();
    source.revalidate().unwrap();
    #[cfg(windows)]
    assert_eq!(
        fs::OpenOptions::new()
            .write(true)
            .open(&output)
            .unwrap_err()
            .raw_os_error(),
        Some(32)
    );
    assert_eq!(fs::read(&input).unwrap(), INPUT);
    assert_eq!(fs::read(&output).unwrap(), GOLDEN);
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 2);
    drop(compared);
    drop(source);
    root.close().unwrap();
}
#[test]
fn different_equal_length_short_empty_and_long_copies_are_never_equal() {
    let root = root();
    fs::write(root.path().join("source.eml"), INPUT).unwrap();
    let source = prepared(root.path());
    let mut changed = GOLDEN.to_vec();
    changed[0] ^= 1;
    let mut longer = GOLDEN.to_vec();
    longer.push(b'x');
    let cases = [
        changed,
        GOLDEN[..GOLDEN.len() - 1].to_vec(),
        Vec::new(),
        longer,
    ];
    for (index, bytes) in cases.into_iter().enumerate() {
        let output = root.path().join(format!("copy{index}.eml"));
        fs::write(&output, &bytes).unwrap();
        let compared = compare_existing(&source, bind(&output), &RunControl::default()).unwrap();
        assert!(!compared.bytes_equal());
        compared.revalidate().unwrap();
        assert_eq!(fs::read(&output).unwrap(), bytes);
    }
    assert_eq!(fs::read(root.path().join("source.eml")).unwrap(), INPUT);
    drop(source);
    root.close().unwrap();
}
#[test]
fn hardlink_to_current_source_is_refused_before_byte_comparison() {
    let root = root();
    let input = root.path().join("source.eml");
    let alias = root.path().join("alias.eml");
    fs::write(&input, INPUT).unwrap();
    fs::hard_link(&input, &alias).unwrap();
    let source = prepared(root.path());
    let existing = bind(&alias);
    let identity = existing.initial_signature().identity;
    let Err(failure) = compare_existing(&source, existing, &RunControl::default()) else {
        panic!("Selected-source identity must never qualify as an existing copy");
    };
    assert!(matches!(
        failure.error(),
        ExistingComparisonError::SourceAlias
    ));
    assert_eq!(failure.identity(), identity);
    assert_eq!(failure.bytes_equal(), None);
    assert!(!failure.stable());
    assert_eq!(fs::read(&input).unwrap(), INPUT);
    assert_eq!(fs::read(&alias).unwrap(), INPUT);
    drop(source);
    root.close().unwrap();
}
#[test]
fn initial_stop_retains_identity_without_inventing_a_comparison() {
    let root = root();
    fs::write(root.path().join("source.eml"), INPUT).unwrap();
    let output = root.path().join("copy.eml");
    fs::write(&output, GOLDEN).unwrap();
    let source = prepared(root.path());
    let existing = bind(&output);
    let identity = existing.initial_signature().identity;
    let control = RunControl::default();
    control.request_stop();
    let Err(failure) = compare_existing(&source, existing, &control) else {
        panic!("Stop must prevent comparison");
    };
    assert!(matches!(
        failure.error(),
        ExistingComparisonError::Interrupted
    ));
    assert_eq!(failure.identity(), identity);
    assert_eq!(failure.bytes_equal(), None);
    assert!(!failure.stable());
    assert_eq!(fs::read(&output).unwrap(), GOLDEN);
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 2);
    drop(source);
    root.close().unwrap();
}
#[cfg(unix)]
#[test]
fn actual_changed_source_and_existing_file_keep_distinct_causes() {
    for change_source in [false, true] {
        let root = root();
        let input = root.path().join("source.eml");
        let output = root.path().join("copy.eml");
        fs::write(&input, INPUT).unwrap();
        fs::write(&output, GOLDEN).unwrap();
        let source = prepared(root.path());
        let existing = bind(&output);
        let identity = existing.initial_signature().identity;
        fs::write(
            if change_source { &input } else { &output },
            b"changed owned file",
        )
        .unwrap();
        let Err(failure) = compare_existing(&source, existing, &RunControl::default()) else {
            panic!("Changed binding must not produce a stable comparison");
        };
        assert!(matches!(
            (change_source, failure.error()),
            (
                true,
                ExistingComparisonError::Source(eml_core::fs::BoundFileError::Changed)
            ) | (
                false,
                ExistingComparisonError::Existing(eml_core::fs::BoundFileError::Changed)
            )
        ));
        assert_eq!(failure.identity(), identity);
        assert_eq!(failure.bytes_equal(), None);
        assert!(!failure.stable());
        drop(source);
        root.close().unwrap();
    }
}
