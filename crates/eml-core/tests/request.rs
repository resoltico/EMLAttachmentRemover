//! Pure admission tests: no fixture creation, filesystem binding or event sink.
#![cfg(any(unix, windows))]
use eml_core::{
    MAX_INPUT_PATH_BYTES, MAX_INPUT_PATHS, MAX_PATH_COMPONENTS, MAX_PATH_UNITS, OutputLocation,
    PathViolation, RequestPath, RequestPathError, RunMode, RunRequest,
};
use std::path::PathBuf;

fn root() -> PathBuf {
    #[cfg(unix)]
    {
        PathBuf::from("/")
    }
    #[cfg(windows)]
    {
        PathBuf::from(r"C:\")
    }
}

fn request(paths: Vec<PathBuf>) -> RunRequest {
    RunRequest {
        paths,
        working_directory: root(),
        output: OutputLocation::BesideSource,
        mode: RunMode::Create,
    }
}

fn rejected_path(path: RequestPath, violation: PathViolation) -> RequestPathError {
    RequestPathError::InvalidPath { path, violation }
}

#[test]
fn argument_limit_applies_before_deduplication() {
    let mut input = request(vec![PathBuf::from("same.eml"); MAX_INPUT_PATHS]);
    assert_eq!(input.validate_path_limits(), Ok(()));
    input.paths.push(PathBuf::from("same.eml"));
    assert_eq!(
        input.validate_path_limits(),
        Err(RequestPathError::TooManyInputs)
    );
}

#[test]
fn exact_native_unit_limit_is_admitted_and_one_over_is_rejected() {
    let mut input = request(vec![PathBuf::from("a".repeat(MAX_PATH_UNITS))]);
    assert_eq!(input.validate_path_limits(), Ok(()));
    input.paths[0].as_mut_os_string().push("a");
    assert_eq!(
        input.validate_path_limits(),
        Err(rejected_path(
            RequestPath::Input(0),
            PathViolation::TooManyUnits,
        ))
    );
}

#[test]
fn aggregate_limit_counts_only_input_native_bytes() {
    let bytes_per_unit = if cfg!(windows) { 2 } else { 1 };
    let count = MAX_INPUT_PATH_BYTES / (MAX_PATH_UNITS * bytes_per_unit);
    let mut input = request(vec![PathBuf::from("a".repeat(MAX_PATH_UNITS)); count]);
    input.output = OutputLocation::Directory(PathBuf::from("b".repeat(MAX_PATH_UNITS)));
    let mut working = root().into_os_string();
    let root_units = if cfg!(windows) { 3 } else { 1 };
    working.push("c".repeat(MAX_PATH_UNITS - root_units));
    input.working_directory = working.into();
    assert_eq!(input.validate_path_limits(), Ok(()));
    input.paths.push(PathBuf::from("z"));
    assert_eq!(
        input.validate_path_limits(),
        Err(RequestPathError::InputPathsTooLarge)
    );
}

#[test]
fn raw_dot_and_parent_components_count_without_normalization() {
    for component in ["a", ".", ".."] {
        let mut parts = vec![component; MAX_PATH_COMPONENTS];
        let input = request(vec![PathBuf::from(parts.join("/"))]);
        assert_eq!(input.validate_path_limits(), Ok(()));
        parts.push(component);
        let input = request(vec![PathBuf::from(parts.join("/"))]);
        assert_eq!(
            input.validate_path_limits(),
            Err(rejected_path(
                RequestPath::Input(0),
                PathViolation::TooManyComponents,
            ))
        );
    }
    let input = request(vec![PathBuf::from(format!(
        "///{}///",
        vec!["a"; 256].join("///")
    ))]);
    // On Windows a leading // introduces an UNC prefix; no extra components
    // can result from that prefix or repeated separators on either platform.
    assert_eq!(input.validate_path_limits(), Ok(()));
}

#[test]
fn cwd_must_be_absolute_but_paths_are_not_resolved_or_rewritten() {
    let before = std::env::current_dir().unwrap();
    let mut input = request(vec![PathBuf::from("a/link/../letter.eml"), PathBuf::new()]);
    input.output = OutputLocation::Directory(PathBuf::from("absent/output"));
    input.mode = RunMode::DryRun;
    let original = input.clone();
    assert_eq!(input.validate_path_limits(), Ok(()));
    assert_eq!(input, original);
    assert_eq!(std::env::current_dir().unwrap(), before);
    input.working_directory = PathBuf::from("relative");
    assert_eq!(
        input.validate_path_limits(),
        Err(RequestPathError::WorkingDirectoryNotAbsolute)
    );
    assert_eq!(request(vec![]).validate_path_limits(), Ok(()));
}

#[test]
fn output_and_captured_cwd_receive_their_own_path_checks() {
    let mut input = request(vec![]);
    input.output = OutputLocation::Directory(PathBuf::from("a\0b"));
    assert_eq!(
        input.validate_path_limits(),
        Err(rejected_path(
            RequestPath::OutputDirectory,
            PathViolation::EmbeddedNul,
        ))
    );
    input.output = OutputLocation::Directory(PathBuf::from("a".repeat(MAX_PATH_UNITS + 1)));
    assert_eq!(
        input.validate_path_limits(),
        Err(rejected_path(
            RequestPath::OutputDirectory,
            PathViolation::TooManyUnits,
        ))
    );
    input.output = OutputLocation::Directory(PathBuf::from(vec!["x"; 257].join("/")));
    assert_eq!(
        input.validate_path_limits(),
        Err(rejected_path(
            RequestPath::OutputDirectory,
            PathViolation::TooManyComponents,
        ))
    );
    input.output = OutputLocation::BesideSource;
    let mut working = root().into_os_string();
    working.push("a\0b");
    input.working_directory = working.into();
    assert_eq!(
        input.validate_path_limits(),
        Err(rejected_path(
            RequestPath::WorkingDirectory,
            PathViolation::EmbeddedNul,
        ))
    );
    let mut working = root().into_os_string();
    working.push("a".repeat(MAX_PATH_UNITS));
    input.working_directory = working.into();
    assert_eq!(
        input.validate_path_limits(),
        Err(rejected_path(
            RequestPath::WorkingDirectory,
            PathViolation::TooManyUnits,
        ))
    );
    let mut working = root().into_os_string();
    working.push(vec!["x"; 257].join("/"));
    input.working_directory = working.into();
    assert_eq!(
        input.validate_path_limits(),
        Err(rejected_path(
            RequestPath::WorkingDirectory,
            PathViolation::TooManyComponents,
        ))
    );
}

#[test]
fn nul_is_rejected_at_its_explicit_input_index() {
    let input = request(vec![PathBuf::from("first"), PathBuf::from("a\0b")]);
    assert_eq!(
        input.validate_path_limits(),
        Err(rejected_path(
            RequestPath::Input(1),
            PathViolation::EmbeddedNul,
        ))
    );
}

#[test]
fn unicode_limit_uses_native_units_instead_of_scalar_count() {
    let units_per_crab = if cfg!(windows) { 2 } else { 4 };
    let mut input = request(vec![PathBuf::from(
        "🦀".repeat(MAX_PATH_UNITS / units_per_crab),
    )]);
    assert_eq!(input.validate_path_limits(), Ok(()));
    input.paths[0].as_mut_os_string().push("x");
    assert_eq!(
        input.validate_path_limits(),
        Err(rejected_path(
            RequestPath::Input(0),
            PathViolation::TooManyUnits,
        ))
    );
}

#[cfg(unix)]
#[test]
fn unix_nonunicode_and_backslashes_are_preserved() {
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    let bytes = b"mail/\xff\\letter.eml".to_vec();
    let input = request(vec![std::ffi::OsString::from_vec(bytes.clone()).into()]);
    assert_eq!(input.validate_path_limits(), Ok(()));
    assert_eq!(input.paths[0].as_os_str().as_bytes(), bytes);
    // Backslash is an ordinary byte on Unix, so this is one component.
    let input = request(vec![PathBuf::from(vec!["a"; 257].join("\\"))]);
    assert_eq!(input.validate_path_limits(), Ok(()));
}

#[cfg(windows)]
#[test]
fn windows_unpaired_units_prefixes_and_both_separators_are_native() {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    let units = [109, 97, 105, 108, 92, 0xd800, 0xdc00, 0xd801];
    let input = request(vec![std::ffi::OsString::from_wide(&units).into()]);
    assert_eq!(input.validate_path_limits(), Ok(()));
    assert_eq!(
        input.paths[0].as_os_str().encode_wide().collect::<Vec<_>>(),
        units
    );
    for prefix in [
        r"C:\",
        r"C:",
        r"\\server\share\",
        r"\\?\C:\",
        r"\\?\UNC\server\share\",
    ] {
        for separator in ["/", "\\"] {
            let path = format!("{prefix}{}", vec!["a"; 256].join(separator));
            let mut input = request(vec![PathBuf::from(path)]);
            assert_eq!(input.validate_path_limits(), Ok(()));
            input.paths[0]
                .as_mut_os_string()
                .push(format!("{separator}a"));
            assert_eq!(
                input.validate_path_limits(),
                Err(rejected_path(
                    RequestPath::Input(0),
                    PathViolation::TooManyComponents,
                ))
            );
        }
    }
}
