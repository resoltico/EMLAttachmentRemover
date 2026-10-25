//! Real owned files exercise the reusable read-only business boundary.
use eml_core::{
    CollectedObservation, CollectionState, ErrorCode, ErrorPhase, InvalidRequest, OutputLocation,
    RequestPathError, RunControl, RunMode, RunRequest, SourceOrigin, collect_sources,
};
use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
};
fn scratch() -> tempfile::TempDir {
    let parent = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/collection-tests");
    fs::create_dir_all(&parent).unwrap();
    tempfile::tempdir_in(parent).unwrap()
}
fn request(cwd: &Path, paths: &[&str]) -> RunRequest {
    RunRequest {
        paths: paths.iter().map(PathBuf::from).collect(),
        working_directory: cwd.to_owned(),
        output: OutputLocation::BesideSource,
        mode: RunMode::Create,
    }
}
fn displays(result: &eml_core::CollectedSources<'_>) -> Vec<PathBuf> {
    (0..result.items().len())
        .map(|index| result.display_path(index).unwrap().into_owned())
        .collect()
}

#[test]
fn output_preflight_is_read_only_and_does_not_authorize_later_collection() {
    let root = scratch();
    let output = root.path().join("copies");
    let mut req = request(root.path(), &["missing-source.eml"]);
    req.output = OutputLocation::Directory(output.clone());
    assert!(matches!(
        eml_core::check_output_location(&req),
        Err(InvalidRequest::OutputDirectory(_))
    ));
    assert!(
        !output.exists(),
        "Preflight created the missing destination"
    );
    fs::write(&output, b"ordinary file is not a directory").unwrap();
    assert!(matches!(
        eml_core::check_output_location(&req),
        Err(InvalidRequest::OutputDirectory(_))
    ));
    assert_eq!(
        fs::read(&output).unwrap(),
        b"ordinary file is not a directory"
    );
    fs::remove_file(&output).unwrap();
    fs::create_dir(&output).unwrap();
    eml_core::check_output_location(&req).unwrap();
    assert_eq!(fs::read_dir(&output).unwrap().count(), 0);
    fs::remove_dir(&output).unwrap();
    assert!(matches!(
        collect_sources(&req, &RunControl::default()),
        Err(InvalidRequest::OutputDirectory(_))
    ));
    root.close().unwrap();
}

#[test]
fn ordered_dfs_skips_only_walked_names_and_explicit_occurrences_own_entries() {
    let root = scratch();
    fs::create_dir_all(root.path().join("selected/nested")).unwrap();
    fs::create_dir_all(root.path().join("selected/.hidden")).unwrap();
    fs::create_dir_all(root.path().join("selected/ignored.no-attachments.eml")).unwrap();
    for (path, body) in [
        ("selected/a.EML", "a"),
        ("selected/b.eml", "b"),
        ("selected/nested/c.eml", "c"),
        ("selected/nested/d.txt", "not walked"),
        ("selected/.hidden/e.eml", "explicit hidden"),
        (
            "selected/ignored.no-attachments.eml/inside.eml",
            "not walked",
        ),
        ("selected/plain.txt", "explicit any suffix"),
        ("selected/skip.no-attachments.eml", "not walked"),
    ] {
        fs::write(root.path().join(path), body).unwrap();
    }
    let req = request(
        root.path(),
        &[
            "selected",
            "selected/nested/c.eml",
            "selected/plain.txt",
            "selected/.hidden/e.eml",
        ],
    );
    let found = collect_sources(&req, &RunControl::default()).unwrap();
    assert_eq!(found.state(), CollectionState::Collected);
    assert!(found.collection_complete());
    assert_eq!(
        displays(&found),
        [
            "selected/a.EML",
            "selected/b.eml",
            "selected/nested/c.eml",
            "selected/plain.txt",
            "selected/.hidden/e.eml"
        ]
        .map(PathBuf::from)
    );
    assert_eq!(
        found
            .items()
            .iter()
            .map(|item| item.origin())
            .collect::<Vec<_>>(),
        [
            SourceOrigin::Walked,
            SourceOrigin::Walked,
            SourceOrigin::Explicit,
            SourceOrigin::Explicit,
            SourceOrigin::Explicit
        ]
    );
    assert_eq!(found.request_index(2), Some(1));
    assert_eq!(found.items()[2].relative_path(), Path::new(""));
    assert!(
        found
            .items()
            .iter()
            .all(|item| matches!(item.observation(), CollectedObservation::File { .. }))
    );
    assert!(found.run_error().is_none());
    // Contents deliberately are not MIME: collection must not prepare or rewrite them.
    assert_eq!(
        fs::read(root.path().join("selected/plain.txt")).unwrap(),
        b"explicit any suffix"
    );
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    drop(found);
    root.close().unwrap();
}

#[test]
fn explicit_root_failure_materializes_known_files_without_expanding_folders() {
    let root = scratch();
    fs::write(root.path().join("first.eml"), b"first").unwrap();
    fs::write(root.path().join("last.eml"), b"last").unwrap();
    fs::create_dir(root.path().join("folder")).unwrap();
    fs::write(root.path().join("folder/unexpanded.eml"), b"unexpanded").unwrap();
    let req = request(root.path(), &["first.eml", "missing", "folder", "last.eml"]);
    let found = collect_sources(&req, &RunControl::default()).unwrap();
    assert_eq!(found.state(), CollectionState::InputAdmissionFailed);
    assert!(!found.collection_complete());
    assert_eq!(
        displays(&found),
        ["first.eml", "missing", "last.eml"].map(PathBuf::from)
    );
    assert_eq!(found.request_index(2), Some(3));
    let CollectedObservation::Failed(error) = found.items()[1].observation() else {
        panic!("missing root must be retained as failure");
    };
    assert_eq!(error.code(), ErrorCode::ReadFailed);
    assert_eq!(error.phase(), ErrorPhase::Admission);
    assert_eq!(error.native_error().unwrap().raw_os_error(), Some(2));
    assert!(error.source().is_some());
    assert_eq!(found.items()[1].origin(), SourceOrigin::Explicit);
    assert_eq!(
        found.run_error().unwrap().code(),
        ErrorCode::InputAdmissionFailed
    );
    assert_eq!(found.examined_entries(), 0);
    drop(found);
    root.close().unwrap();
}

#[test]
fn genuine_prerequisite_errors_are_prestream_and_keep_their_causes() {
    let root = scratch();
    fs::write(root.path().join("file"), b"owned").unwrap();
    let mut req = request(root.path(), &["file"]);
    req.output = OutputLocation::Directory(PathBuf::from("file"));
    let error = collect_sources(&req, &RunControl::default()).err().unwrap();
    assert!(
        matches!(&error, InvalidRequest::OutputDirectory(cause) if cause.kind() == std::io::ErrorKind::NotADirectory)
    );
    assert!(error.source().is_some());
    req.output = OutputLocation::Directory(PathBuf::from("missing-output"));
    let error = collect_sources(&req, &RunControl::default()).err().unwrap();
    assert!(
        matches!(&error, InvalidRequest::OutputDirectory(cause) if cause.raw_os_error() == Some(2))
    );
    req.working_directory = root.path().join("file");
    assert!(matches!(
        collect_sources(&req, &RunControl::default()),
        Err(InvalidRequest::WorkingDirectory(_))
    ));
    req.paths = vec![PathBuf::from("file"); 4097];
    assert!(matches!(
        collect_sources(&req, &RunControl::default()),
        Err(InvalidRequest::Paths(RequestPathError::TooManyInputs))
    ));
    assert!(!root.path().join("missing-output").exists());
    root.close().unwrap();
}

#[test]
fn native_invalid_input_after_raw_validation_is_a_collected_root_failure() {
    let root = scratch();
    let req = request(root.path(), &[""]);
    assert!(req.validate_path_limits().is_ok());
    let found = collect_sources(&req, &RunControl::default()).unwrap();
    assert_eq!(found.state(), CollectionState::InputAdmissionFailed);
    assert_eq!(found.items().len(), 1);
    let CollectedObservation::Failed(error) = found.items()[0].observation() else {
        panic!("failed root")
    };
    assert_eq!(error.code(), ErrorCode::ReadFailed);
    assert_eq!(error.phase(), ErrorPhase::Admission);
    assert_eq!(
        error.native_error().unwrap().kind(),
        std::io::ErrorKind::InvalidInput
    );
    assert_eq!(found.request_index(0), Some(0));
    assert!(!found.collection_complete());
    drop(found);
    root.close().unwrap();
}

#[test]
fn explicit_output_checks_hidden_and_generated_branches_before_any_write() {
    for branch in [".hidden", "generated.no-attachments.eml"] {
        let root = scratch();
        fs::create_dir_all(root.path().join("selected").join(branch).join("output")).unwrap();
        fs::write(root.path().join("selected/a.eml"), b"owned source").unwrap();
        let mut req = request(root.path(), &["selected"]);
        req.output =
            OutputLocation::Directory(PathBuf::from("selected").join(branch).join("output"));
        req.mode = RunMode::DryRun;
        let found = collect_sources(&req, &RunControl::default()).unwrap();
        assert_eq!(found.state(), CollectionState::OutputOverlap, "{branch}");
        assert_eq!(found.run_error().unwrap().code(), ErrorCode::OutputOverlap);
        assert!(!found.collection_complete());
        assert_eq!(
            fs::read(root.path().join("selected/a.eml")).unwrap(),
            b"owned source"
        );
        assert_eq!(
            fs::read_dir(root.path().join("selected").join(branch).join("output"))
                .unwrap()
                .count(),
            0
        );
        drop(found);
        root.close().unwrap();
    }
}

#[test]
fn native_name_sort_uses_bytes_on_unix_and_utf16_units_on_windows() {
    let root = scratch();
    fs::create_dir(root.path().join("order")).unwrap();
    for name in ["\u{10000}.eml", "\u{e000}.eml"] {
        fs::write(root.path().join("order").join(name), b"owned").unwrap();
    }
    let req = request(root.path(), &["order"]);
    let found = collect_sources(&req, &RunControl::default()).unwrap();
    assert_eq!(found.state(), CollectionState::Collected);
    let names: Vec<_> = found
        .items()
        .iter()
        .map(|item| item.native_path().file_name().unwrap().to_owned())
        .collect();
    #[cfg(unix)]
    let expected = ["\u{e000}.eml", "\u{10000}.eml"];
    #[cfg(windows)]
    let expected = ["\u{10000}.eml", "\u{e000}.eml"];
    assert_eq!(names, expected.map(std::ffi::OsString::from));
    drop(found);
    root.close().unwrap();
}

#[test]
fn source_charge_counts_complete_display_or_expanded_reported_native_value() {
    let root = scratch();
    fs::write(root.path().join("a.eml"), b"owned").unwrap();
    for spelling in [String::from("a.eml"), format!("{}a.eml", "./".repeat(100))] {
        let req = RunRequest {
            paths: vec![PathBuf::from(&spelling)],
            working_directory: root.path().to_owned(),
            output: OutputLocation::BesideSource,
            mode: RunMode::DryRun,
        };
        let found = collect_sources(&req, &RunControl::default()).unwrap();
        assert_eq!(found.state(), CollectionState::Collected);
        assert_eq!(found.items().len(), 1);
        assert_eq!(
            found.display_path(0).unwrap().as_os_str(),
            req.paths[0].as_os_str()
        );
        #[cfg(unix)]
        {
            assert_eq!(
                found.items()[0].native_path().as_os_str(),
                req.paths[0].as_os_str()
            );
            assert_eq!(found.source_path_bytes(), spelling.len());
        }
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            let expected_native = eml_core::fs::PathContext::capture(root.path())
                .unwrap()
                .resolve(Path::new("a.eml"))
                .unwrap();
            assert_eq!(
                found.items()[0].native_path().as_os_str(),
                expected_native.as_os_str()
            );
            let original_bytes = spelling.encode_utf16().count() * 2;
            let reported_bytes = expected_native.as_os_str().encode_wide().count() * 2;
            assert_eq!(
                found.source_path_bytes(),
                original_bytes.max(reported_bytes)
            );
        }
        drop(found);
    }
    root.close().unwrap();
}

#[test]
fn actual_work_and_depth_boundaries_keep_bounded_known_prefixes() {
    let root = scratch();
    fs::create_dir(root.path().join("many")).unwrap();
    for index in 0..4096 {
        fs::write(
            root.path().join("many").join(format!("{index:04}.eml")),
            b"",
        )
        .unwrap();
    }
    let req = request(root.path(), &["many"]);
    let found = collect_sources(&req, &RunControl::default()).unwrap();
    assert_eq!(found.state(), CollectionState::Collected);
    assert_eq!(found.items().len(), 4096);
    drop(found);
    fs::write(root.path().join("many/4096.eml"), b"").unwrap();
    let found = collect_sources(&req, &RunControl::default()).unwrap();
    assert_eq!(found.state(), CollectionState::InputLimitExceeded);
    assert_eq!(found.items().len(), 4096);
    assert_eq!(
        displays(&found),
        (0..4096)
            .map(|index| PathBuf::from("many").join(format!("{index:04}.eml")))
            .collect::<Vec<_>>()
    );
    assert!(!found.collection_complete());
    assert_eq!(
        found.run_error().unwrap().code(),
        ErrorCode::InputLimitExceeded
    );
    drop(found);
    let mut leaf = root.path().join("depth");
    fs::create_dir(&leaf).unwrap();
    for _ in 1..64 {
        leaf.push("d");
        fs::create_dir(&leaf).unwrap();
    }
    fs::write(leaf.join("a.eml"), b"owned depth").unwrap();
    let req = request(root.path(), &["depth"]);
    let found = collect_sources(&req, &RunControl::default()).unwrap();
    assert_eq!(found.state(), CollectionState::Collected);
    assert_eq!(found.items().len(), 1);
    drop(found);
    fs::create_dir(leaf.join("overflow")).unwrap();
    let found = collect_sources(&req, &RunControl::default()).unwrap();
    assert_eq!(found.state(), CollectionState::InputLimitExceeded);
    assert_eq!(found.items().len(), 1);
    drop(found);
    let mut overlap = req.clone();
    overlap.output = OutputLocation::Directory(leaf.join("overflow"));
    let found = collect_sources(&overlap, &RunControl::default()).unwrap();
    assert_eq!(found.state(), CollectionState::OutputOverlap);
    assert_eq!(found.run_error().unwrap().code(), ErrorCode::OutputOverlap);
    assert_eq!(found.items().len(), 1);
    drop(found);
    root.close().unwrap();
}

#[test]
fn initially_stopped_collection_retains_no_metadata_only_root_as_work() {
    let root = scratch();
    fs::write(root.path().join("source.eml"), b"owned").unwrap();
    let req = request(root.path(), &["source.eml"]);
    let control = RunControl::default();
    control.request_stop();
    let found = collect_sources(&req, &control).unwrap();
    assert_eq!(found.state(), CollectionState::Interrupted);
    assert!(found.items().is_empty());
    assert!(!found.collection_complete());
    assert!(found.run_error().is_none());
    drop(found);
    root.close().unwrap();
}

#[path = "support/directory_alias.rs"]
mod directory_alias;

#[test]
fn selected_directory_aliases_coalesce_only_proven_leaf_entries_and_walked_aliases_are_excluded() {
    let root = scratch();
    fs::create_dir(root.path().join("target")).unwrap();
    fs::create_dir(root.path().join("outside")).unwrap();
    fs::write(root.path().join("target/a.eml"), b"owned alias body").unwrap();
    fs::write(root.path().join("outside/not-walked.eml"), b"outside").unwrap();
    directory_alias::create(&root.path().join("target"), &root.path().join("alpha"));
    directory_alias::create(&root.path().join("target"), &root.path().join("beta"));
    directory_alias::create(
        &root.path().join("outside"),
        &root.path().join("target/walked-link"),
    );
    let req = request(root.path(), &["alpha", "beta"]);
    let found = collect_sources(&req, &RunControl::default()).unwrap();
    assert_eq!(
        found.state(),
        CollectionState::Collected,
        "Selected alias observations: {:?}",
        found
            .items()
            .iter()
            .map(|item| (item.native_path(), item.observation()))
            .collect::<Vec<_>>()
    );
    assert_eq!(displays(&found), ["alpha/a.eml"].map(PathBuf::from));
    let ids: Vec<_> = found
        .items()
        .iter()
        .map(|item| match item.observation() {
            CollectedObservation::File { signature, .. } => signature.identity,
            _ => panic!("ordinary alias target file"),
        })
        .collect();
    assert_eq!(ids.len(), 1);
    for (item, expected) in found.items().iter().zip(["alpha"]) {
        assert_eq!(
            item.native_path().parent().unwrap().file_name(),
            Some(std::ffi::OsStr::new(expected))
        );
        #[cfg(windows)]
        assert_eq!(fs::read(item.native_path()).unwrap(), b"owned alias body");
        #[cfg(unix)]
        assert_eq!(
            fs::read(root.path().join(item.native_path())).unwrap(),
            b"owned alias body"
        );
    }
    drop(found);
    // Remove only the owned static aliases before recursive fixture teardown.
    #[cfg(unix)]
    for path in ["alpha", "beta", "target/walked-link"] {
        fs::remove_file(root.path().join(path)).unwrap();
    }
    #[cfg(windows)]
    for path in ["alpha", "beta", "target/walked-link"] {
        fs::remove_dir(root.path().join(path)).unwrap();
    }
    root.close().unwrap();
}

#[cfg(windows)]
#[test]
fn walked_literal_parent_is_reported_as_usable_extended_native_path() {
    use std::os::windows::ffi::OsStrExt;
    let root = scratch();
    fs::create_dir(root.path().join("selected")).unwrap();
    let context = eml_core::fs::PathContext::capture(root.path()).unwrap();
    let literal_parent = context
        .resolve(Path::new("selected"))
        .unwrap()
        .join("folder. ");
    fs::create_dir(&literal_parent).unwrap();
    fs::write(literal_parent.join("a.eml"), b"literal parent body").unwrap();
    fs::create_dir(root.path().join("selected/folder")).unwrap();
    fs::write(
        root.path().join("selected/folder/a.eml"),
        b"different trimmed control",
    )
    .unwrap();
    let req = request(root.path(), &["selected"]);
    let found = collect_sources(&req, &RunControl::default()).unwrap();
    assert_eq!(found.state(), CollectionState::Collected);
    assert_eq!(found.items().len(), 2);
    let index = found
        .items()
        .iter()
        .position(|item| {
            item.relative_path().as_os_str() == std::ffi::OsStr::new(r"folder. \a.eml")
        })
        .unwrap();
    let item = &found.items()[index];
    let native: Vec<_> = item.native_path().as_os_str().encode_wide().collect();
    assert!(native.starts_with(&[92, 92, 63, 92]));
    let expected_tail: Vec<_> = r"\folder. \a.eml".encode_utf16().collect();
    assert!(native.ends_with(&expected_tail));
    assert_eq!(
        fs::read(item.native_path()).unwrap(),
        b"literal parent body"
    );
    assert_eq!(
        found.display_path(index).unwrap().as_os_str(),
        std::ffi::OsStr::new(r"selected\folder. \a.eml")
    );
    drop(found);
    root.close().unwrap();
}

#[test]
fn actual_directory_entry_bound_accepts_exact_total_and_refuses_next_name() {
    let root = scratch();
    let folder = root.path().join("entries");
    fs::create_dir(&folder).unwrap();
    for index in 0..99_998 {
        fs::write(folder.join(format!("entry-{index:06}.txt")), b"").unwrap();
    }
    #[cfg(unix)]
    let operand = folder.clone();
    #[cfg(windows)]
    let operand = eml_core::fs::PathContext::capture(root.path())
        .unwrap()
        .resolve(&folder)
        .unwrap();
    let directory = eml_core::fs::BoundDirectory::open(&operand).unwrap();
    // This raw native stream is an oracle for provider dot-entry behavior,
    // independent of core's examination counter and skip policy.
    let raw_count = directory
        .read_entries()
        .unwrap()
        .try_fold(0, |count, entry| entry.map(|_| count + 1))
        .unwrap();
    assert!((99_998..=100_000).contains(&raw_count));
    drop(directory);
    for index in 0..100_000 - raw_count {
        fs::write(folder.join(format!("fill-{index}.txt")), b"").unwrap();
    }
    let req = request(root.path(), &["entries"]);
    let found = collect_sources(&req, &RunControl::default()).unwrap();
    assert_eq!(found.state(), CollectionState::Collected);
    assert!(found.collection_complete());
    assert_eq!(found.examined_entries(), 100_000);
    assert!(found.items().is_empty());
    drop(found);
    fs::write(folder.join("one-over.txt"), b"").unwrap();
    let found = collect_sources(&req, &RunControl::default()).unwrap();
    assert_eq!(found.state(), CollectionState::InputLimitExceeded);
    assert_eq!(found.examined_entries(), 100_000);
    assert!(found.items().is_empty());
    assert!(!found.collection_complete());
    drop(found);
    root.close().unwrap();
}
