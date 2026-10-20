use super::*;
fn request(root: &Path, output: OutputLocation) -> RunRequest {
    RunRequest {
        paths: vec![PathBuf::from("source")],
        working_directory: root.to_owned(),
        output,
        mode: crate::RunMode::Create,
    }
}
fn scratch() -> tempfile::TempDir {
    let parent = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/collection-tests");
    std::fs::create_dir_all(&parent).unwrap();
    tempfile::tempdir_in(parent).unwrap()
}
#[test]
fn report_charge_accepts_exact_bound_rejects_one_over_and_uses_larger_fact() {
    assert_eq!(MAX_COLLECTED_PATH_BYTES, 8_388_608);
    assert_eq!(path_total(8_388_598, 10, 9), Some(8_388_608));
    assert_eq!(path_total(8_388_598, 9, 10), Some(8_388_608));
    assert_eq!(path_total(8_388_598, 11, 9), None);
    assert_eq!(path_total(8_388_598, 9, 11), None);
    assert_eq!(path_total(8_388_408, 5, 200), Some(8_388_608));
    assert_eq!(path_total(usize::MAX, 1, 1), None);
}
#[test]
fn forced_hash_collision_does_not_merge_distinct_paths_or_hardlink_spellings() {
    let root = scratch();
    std::fs::write(root.path().join("a.eml"), b"owned a").unwrap();
    std::fs::write(root.path().join("b.eml"), b"owned b").unwrap();
    std::fs::hard_link(root.path().join("a.eml"), root.path().join("hard.eml")).unwrap();
    let mut req = request(root.path(), OutputLocation::BesideSource);
    req.paths = ["a.eml", "./a.eml", "b.eml", "a.eml", "hard.eml"]
        .map(PathBuf::from)
        .to_vec();
    let found = collect_with_hash(&req, &RunControl::default(), |_| 0).unwrap();
    assert_eq!(found.state(), CollectionState::Collected);
    #[cfg(unix)]
    let expected = ["a.eml", "./a.eml", "b.eml", "hard.eml"]
        .map(PathBuf::from)
        .to_vec();
    #[cfg(windows)]
    let expected = ["a.eml", "b.eml", "hard.eml"].map(PathBuf::from).to_vec();
    assert_eq!(found.items().len(), expected.len());
    assert_eq!(
        (0..expected.len())
            .map(|index| found.display_path(index).unwrap().into_owned())
            .collect::<Vec<_>>(),
        expected
    );
    let ids: Vec<_> = found
        .items()
        .iter()
        .map(|item| match item.observation() {
            CollectedObservation::File { signature, .. } => signature.identity,
            _ => panic!("ordinary file"),
        })
        .collect();
    #[cfg(unix)]
    {
        assert_eq!(ids[0], ids[1]);
        assert_eq!(ids[0], ids[3]);
        assert_ne!(ids[0], ids[2]);
    }
    #[cfg(windows)]
    {
        assert_eq!(ids[0], ids[2]);
        assert_ne!(ids[0], ids[1]);
    }
    drop(found);
    root.close().unwrap();
}
#[test]
fn stop_during_real_discovery_retains_only_committed_work_and_no_output() {
    let root = scratch();
    std::fs::create_dir(root.path().join("source")).unwrap();
    std::fs::create_dir(root.path().join("output")).unwrap();
    for name in ["a.eml", "b.eml", "c.eml"] {
        std::fs::write(root.path().join("source").join(name), b"owned").unwrap();
    }
    let req = request(
        root.path(),
        OutputLocation::Directory(PathBuf::from("output")),
    );
    let control = RunControl::default();
    // Private hash injection chooses a deterministic semantic checkpoint;
    // all selection/enumeration/metadata operations remain actual filesystem I/O.
    let found = collect_with_hash(&req, &control, |path| {
        if Path::new(path).file_name() == Some(OsStr::new("b.eml")) {
            control.request_stop();
        }
        0
    })
    .unwrap();
    assert_eq!(found.state(), CollectionState::Interrupted);
    assert_eq!(found.items().len(), 1);
    assert_eq!(
        found.display_path(0).unwrap().as_ref(),
        Path::new("source/a.eml")
    );
    assert!(!found.collection_complete());
    assert!(found.run_error().is_none());
    assert_eq!(
        std::fs::read_dir(root.path().join("output"))
            .unwrap()
            .count(),
        0
    );
    drop(found);
    root.close().unwrap();
}
#[test]
fn actual_uninspectable_entry_blocks_explicit_output_without_fabricating_root_failure() {
    for explicit_output in [false, true] {
        let root = scratch();
        std::fs::create_dir_all(root.path().join("source/z-removed")).unwrap();
        std::fs::create_dir(root.path().join("output")).unwrap();
        std::fs::write(root.path().join("source/a.eml"), b"owned").unwrap();
        let req = request(
            root.path(),
            if explicit_output {
                OutputLocation::Directory(PathBuf::from("output"))
            } else {
                OutputLocation::BesideSource
            },
        );
        let removed = std::sync::atomic::AtomicBool::new(false);
        let found = collect_with_hash(&req, &RunControl::default(), |path| {
            if Path::new(path).file_name() == Some(OsStr::new("a.eml"))
                && !removed.swap(true, std::sync::atomic::Ordering::Relaxed)
            {
                std::fs::remove_dir(root.path().join("source/z-removed")).unwrap();
            }
            0
        })
        .unwrap();
        assert!(removed.load(std::sync::atomic::Ordering::Relaxed));
        assert!(!found.collection_complete());
        if explicit_output {
            assert_eq!(found.state(), CollectionState::Unverified);
            assert_eq!(found.items().len(), 1);
            let error = found.run_error().unwrap();
            assert_eq!(error.code(), ErrorCode::BindFailed);
            assert_eq!(error.phase(), ErrorPhase::Admission);
            assert_eq!(
                error.native_error().unwrap().kind(),
                io::ErrorKind::NotFound
            );
        } else {
            assert_eq!(found.state(), CollectionState::Collected);
            assert_eq!(found.items().len(), 2);
            assert_eq!(found.items()[1].origin(), SourceOrigin::Walked);
            let CollectedObservation::Failed(error) = found.items()[1].observation() else {
                panic!("discovery failure")
            };
            assert_eq!(error.code(), ErrorCode::ReadFailed);
            assert_eq!(error.phase(), ErrorPhase::Collection);
            assert!(found.run_error().is_none());
        }
        assert_eq!(
            std::fs::read_dir(root.path().join("output"))
                .unwrap()
                .count(),
            0
        );
        drop(found);
        root.close().unwrap();
    }
}
#[test]
fn repeated_unadmitted_inputs_do_not_gain_an_operational_dedupe_key() {
    let root = scratch();
    let mut req = request(root.path(), OutputLocation::BesideSource);
    req.paths = vec![PathBuf::new(), PathBuf::new()];
    let found = collect_with_hash(&req, &RunControl::default(), |_| 0).unwrap();
    assert_eq!(found.state(), CollectionState::InputAdmissionFailed);
    assert_eq!(found.items().len(), 2);
    for (index, item) in found.items().iter().enumerate() {
        assert_eq!(found.request_index(index), Some(index));
        assert!(matches!(
            item.observation(),
            CollectedObservation::Failed(_)
        ));
        assert!(item.native_path().as_os_str().is_empty());
    }
    drop(found);
    root.close().unwrap();
}
#[test]
fn late_explicit_root_failure_is_not_hidden_by_an_earlier_walked_file() {
    let root = scratch();
    std::fs::create_dir_all(root.path().join("source/z.eml")).unwrap();
    std::fs::write(root.path().join("source/a.eml"), b"owned").unwrap();
    let mut req = request(root.path(), OutputLocation::BesideSource);
    req.paths.push(PathBuf::from("source/z.eml"));
    let changed = std::sync::atomic::AtomicBool::new(false);
    let found = collect_with_hash(&req, &RunControl::default(), |path| {
        if Path::new(path).file_name() == Some(OsStr::new("a.eml"))
            && !changed.swap(true, std::sync::atomic::Ordering::Relaxed)
        {
            std::fs::remove_dir(root.path().join("source/z.eml")).unwrap();
            std::fs::write(root.path().join("source/z.eml"), b"owned changed type").unwrap();
        }
        0
    })
    .unwrap();
    assert!(changed.load(std::sync::atomic::Ordering::Relaxed));
    assert_eq!(found.state(), CollectionState::InputAdmissionFailed);
    assert_eq!(found.items().len(), 3);
    assert_eq!(found.items()[1].origin(), SourceOrigin::Walked);
    assert!(matches!(
        found.items()[1].observation(),
        CollectedObservation::File { .. }
    ));
    assert_eq!(found.items()[2].origin(), SourceOrigin::Explicit);
    assert_eq!(found.request_index(2), Some(1));
    assert!(matches!(
        found.items()[2].observation(),
        CollectedObservation::Failed(_)
    ));
    assert!(!found.collection_complete());
    drop(found);
    root.close().unwrap();
}
#[test]
fn scripted_late_stop_preserves_observed_failure_and_completed_discovery() {
    let root = scratch();
    let req = request(root.path(), OutputLocation::BesideSource);
    let control = RunControl::default();
    let mut collector = Collector {
        result: CollectedSources::empty(&req),
        control: &control,
        hash: |_: &OsStr| 0,
        seen: HashMap::new(),
        unknown: false,
    };
    collector.result.complete = true;
    // State mapping only: this does not claim an OS permission failure or
    // an actual late signal. The original scripted native cause stays intact.
    collector.unverified(
        "Captured context could not be verified",
        io::Error::from_raw_os_error(5),
    );
    assert!(collector.result.complete);
    control.request_stop();
    assert!(!collector.checkpoint());
    assert_eq!(collector.result.state(), CollectionState::Interrupted);
    assert!(collector.result.collection_complete());
    let failure = collector.result.run_error().unwrap();
    assert_eq!(failure.code(), ErrorCode::BindFailed);
    assert_eq!(failure.native_error().unwrap().raw_os_error(), Some(5));
    drop(collector);
    root.close().unwrap();
}
#[cfg(unix)]
#[test]
fn held_relative_and_replaced_absolute_cwd_names_are_not_deduplicated() {
    for hardlink in [false, true] {
        let root = scratch();
        let working = root.path().join("working");
        let moved = root.path().join("moved");
        std::fs::create_dir(&working).unwrap();
        std::fs::write(working.join("a.eml"), b"A").unwrap();
        let mut req = request(&working, OutputLocation::BesideSource);
        req.paths = vec![PathBuf::from("a.eml"), working.join("a.eml")];
        let changed = std::sync::atomic::AtomicBool::new(false);
        let found = collect_with_hash(&req, &RunControl::default(), |_| {
            if !changed.swap(true, std::sync::atomic::Ordering::Relaxed) {
                std::fs::rename(&working, &moved).unwrap();
                std::fs::create_dir(&working).unwrap();
                if hardlink {
                    std::fs::hard_link(moved.join("a.eml"), working.join("a.eml")).unwrap();
                } else {
                    std::fs::write(working.join("a.eml"), b"replacement").unwrap();
                }
            }
            0
        })
        .unwrap();
        assert_eq!(found.state(), CollectionState::Collected);
        assert_eq!(found.items().len(), 2);
        let facts: Vec<_> = found
            .items()
            .iter()
            .map(|item| match item.observation() {
                CollectedObservation::File { signature, parent } => (*signature, *parent),
                _ => panic!("ordinary distinct-context source"),
            })
            .collect();
        assert_eq!(facts[0].0.size, 1);
        if hardlink {
            assert_eq!(facts[0].0, facts[1].0);
        } else {
            assert_eq!(facts[1].0.size, 11);
            assert_ne!(facts[0].0.identity, facts[1].0.identity);
        }
        assert_ne!(facts[0].1, facts[1].1);
        assert_eq!(
            found.items()[0].native_path().as_os_str(),
            OsStr::new("a.eml")
        );
        assert_eq!(
            found.items()[1].native_path().as_os_str(),
            working.join("a.eml").as_os_str()
        );
        assert_eq!(std::fs::read(moved.join("a.eml")).unwrap(), b"A");
        let expected: &[u8] = if hardlink { b"A" } else { b"replacement" };
        assert_eq!(std::fs::read(working.join("a.eml")).unwrap(), expected);
        drop(found);
        root.close().unwrap();
    }
}
#[cfg(unix)]
#[test]
fn cross_anchor_directory_routes_walk_independently_then_compare_leaf_bindings() {
    let root = scratch();
    std::fs::create_dir(root.path().join("source")).unwrap();
    std::fs::write(root.path().join("source/a.eml"), b"owned").unwrap();
    let mut req = request(root.path(), OutputLocation::BesideSource);
    let first = collect_with_hash(&req, &RunControl::default(), |_| 0).unwrap();
    let examined_once = first.examined_entries();
    drop(first);
    req.paths.push(root.path().join("source"));
    let found = collect_with_hash(&req, &RunControl::default(), |_| 0).unwrap();
    assert_eq!(found.state(), CollectionState::Collected);
    assert_eq!(found.roots.len(), 2);
    assert_eq!(found.examined_entries(), 2 * examined_once);
    assert_eq!(found.items().len(), 1);
    assert_eq!(found.request_index(0), Some(0));
    assert_eq!(found.items()[0].origin(), SourceOrigin::Walked);
    drop(found);
    root.close().unwrap();
}
