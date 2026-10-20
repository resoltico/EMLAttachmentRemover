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
        let sync = eml_fs::SyncReceipt {
            method: eml_fs::SyncMethod::Fsync,
            state: eml_fs::SyncState::Succeeded,
            native_code: None,
        };
        let original = eml_fs::StageFailure {
            operation: eml_fs::StageOperation::Write,
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
#[cfg(unix)]
#[test]
fn modeled_failed_creation_residue_requires_namespace_work_before_a_later_real_copy() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/run-tests");
    fs::create_dir_all(&base).unwrap();
    let root = tempfile::tempdir_in(base).unwrap();
    fs::create_dir(root.path().join("Inbox")).unwrap();
    fs::create_dir(root.path().join("out")).unwrap();
    let input = include_bytes!("../../../../tests/contract/mime/inputs/01-gmail-attachment.eml");
    let golden = include_bytes!("../../../../tests/contract/mime/golden/01-gmail-attachment.eml");
    fs::write(root.path().join("Inbox/a.eml"), input).unwrap();
    let request = RunRequest {
        paths: vec!["Inbox".into()],
        working_directory: root.path().to_owned(),
        output: crate::OutputLocation::Directory("out".into()),
        mode: RunMode::Create,
    };
    let control = RunControl::default();
    let plan = plan_destinations(collect_sources(&request, &control).unwrap(), &control);
    // Actual owned layout models the residue; the failure receipt is a
    // classifier input, not a claim of an injected native mkdir failure.
    fs::create_dir(root.path().join("out/Inbox")).unwrap();
    let parent = plan.rebind_root(0, &control).unwrap();
    let mut context = RunContext::new(plan.sources());
    context.pending_layout.failed_creation(
        parent.identity(),
        std::ffi::OsStr::new("Inbox"),
        &CleanupReceipt {
            state: CleanupState::Residual,
            residual_name: None,
            native_code: None,
        },
    );
    let source = bind_source(&plan, 0, &control)
        .unwrap_or_else(|error| panic!("Owned source failed: {error:?}"));
    let result = execute_prepared(&plan, 0, &source, &mut context, &control);
    let ItemResult::Created(copy, _) = &result else {
        panic!("Later real copy did not qualify: {result:?}");
    };
    assert_eq!(copy.publication().namespace_sync().required_edges, 3);
    assert_eq!(copy.publication().namespace_sync().completed_edges, 3);
    assert_eq!(
        fs::read(root.path().join("out/Inbox/a.no-attachments.eml")).unwrap(),
        golden
    );
    assert_eq!(fs::read(root.path().join("Inbox/a.eml")).unwrap(), input);
    drop(result);
    drop(source);
    drop(parent);
    drop(plan);
    root.close().unwrap();
}
#[cfg(unix)]
#[test]
fn real_held_publication_witness_survives_modeled_ambiguity_and_changed_requested_address() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/run-tests");
    fs::create_dir_all(&base).unwrap();
    let root = tempfile::tempdir_in(base).unwrap();
    let input = include_bytes!("../../../../tests/contract/mime/inputs/01-gmail-attachment.eml");
    let golden = include_bytes!("../../../../tests/contract/mime/golden/01-gmail-attachment.eml");
    fs::write(root.path().join("a.eml"), input).unwrap();
    fs::create_dir(root.path().join("out")).unwrap();
    let request = RunRequest {
        paths: vec!["a.eml".into()],
        working_directory: root.path().to_owned(),
        output: crate::OutputLocation::Directory("out".into()),
        mode: RunMode::Create,
    };
    let control = RunControl::default();
    let plan = plan_destinations(collect_sources(&request, &control).unwrap(), &control);
    let source = bind_source(&plan, 0, &control)
        .unwrap_or_else(|error| panic!("Owned source failed: {error:?}"));
    let frame = plan.read_parent(0, &control).unwrap();
    let mut pending = crate::binding::PendingLayout::default();
    let mut binding =
        crate::binding::DestinationBinding::acquire(&plan, 0, &frame, &control, &mut pending)
            .unwrap_or_else(|_| panic!("Owned destination failed"));
    let info = plan.records()[0].destination().unwrap();
    let ready = binding
        .directory()
        .stage_bytes(source.message().as_bytes(), || Ok(()))
        .unwrap();
    let identity = ready.identity();
    let actual = ready.publish(info.basename());
    assert_eq!(actual.state, eml_fs::PublicationState::Published);
    assert!(actual.error.is_none());
    binding.final_sync().unwrap();
    fs::rename(root.path().join("out"), root.path().join("moved")).unwrap();
    fs::create_dir(root.path().join("out")).unwrap();
    // Only the ambiguous native return is modeled. Publication and address
    // movement above are actual owned filesystem operations.
    let effect = reconcile_held_effect(
        eml_fs::PublicationState::Unknown,
        &binding,
        identity,
        info.basename(),
    );
    assert_eq!(effect, eml_fs::PublicationState::Published);
    assert_eq!(actual.candidate.cleanup().state, CleanupState::Complete);
    let mut confirmed = true;
    let error = verify_published_address(&plan, 0, &binding, identity, &source, &mut confirmed)
        .unwrap_err();
    assert_eq!(error.code(), ErrorCode::AddressChanged);
    assert!(confirmed);
    assert_eq!(
        fs::read(root.path().join("moved/a.no-attachments.eml")).unwrap(),
        golden
    );
    assert_eq!(fs::read_dir(root.path().join("out")).unwrap().count(), 0);
    drop(binding);
    drop(frame);
    drop(source);
    drop(plan);
    root.close().unwrap();
}
#[test]
fn controlled_stop_before_and_after_the_real_edge_preserves_actual_effect() {
    for after in [false, true] {
        let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/run-tests");
        fs::create_dir_all(&base).unwrap();
        let root = tempfile::tempdir_in(base).unwrap();
        let input =
            include_bytes!("../../../../tests/contract/mime/inputs/01-gmail-attachment.eml");
        let golden =
            include_bytes!("../../../../tests/contract/mime/golden/01-gmail-attachment.eml");
        fs::write(root.path().join("a.eml"), input).unwrap();
        let request = RunRequest {
            paths: vec!["a.eml".into()],
            working_directory: root.path().to_owned(),
            output: crate::OutputLocation::BesideSource,
            mode: RunMode::Create,
        };
        let control = RunControl::default();
        let plan = plan_destinations(collect_sources(&request, &control).unwrap(), &control);
        let source = bind_source(&plan, 0, &control)
            .unwrap_or_else(|error| panic!("Source failed: {error:?}"));
        let mut context = RunContext::new(plan.sources());
        let signal = control.clone();
        let hook = Box::new(move || signal.request_stop());
        if after {
            context.after_edge = Some(hook);
        } else {
            context.before_edge = Some(hook);
        }
        let result = execute_prepared(&plan, 0, &source, &mut context, &control);
        assert!(control.is_stopped());
        if after {
            assert!(matches!(result, ItemResult::Created(..)), "{result:?}");
            assert_eq!(
                fs::read(root.path().join("a.no-attachments.eml")).unwrap(),
                golden
            );
        } else {
            let ItemResult::Failed(failure) = &result else {
                panic!("Pre-edge stop published: {result:?}");
            };
            assert_eq!(
                (failure.error().code(), failure.error().phase()),
                (ErrorCode::Interrupted, ErrorPhase::Publish)
            );
            let FailureEffect::NotPublished {
                file_sync, cleanup, ..
            } = failure.effect()
            else {
                panic!("Wrong stop effect");
            };
            assert_eq!(file_sync.unwrap().state, eml_fs::SyncState::Succeeded);
            assert_eq!(cleanup.state, CleanupState::Complete);
            assert!(!root.path().join("a.no-attachments.eml").exists());
        }
        assert_eq!(fs::read(root.path().join("a.eml")).unwrap(), input);
        assert_eq!(
            fs::read_dir(root.path()).unwrap().count(),
            if after { 2 } else { 1 }
        );
        drop(result);
        drop(source);
        drop(plan);
        root.close().unwrap();
    }
}
#[cfg(unix)]
#[test]
fn controlled_source_change_before_and_after_edge_keeps_publication_truth() {
    for after in [false, true] {
        let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/run-tests");
        fs::create_dir_all(&base).unwrap();
        let root = tempfile::tempdir_in(base).unwrap();
        let input =
            include_bytes!("../../../../tests/contract/mime/inputs/01-gmail-attachment.eml");
        let golden =
            include_bytes!("../../../../tests/contract/mime/golden/01-gmail-attachment.eml");
        let source_path = root.path().join("a.eml");
        fs::write(&source_path, input).unwrap();
        let request = RunRequest {
            paths: vec!["a.eml".into()],
            working_directory: root.path().to_owned(),
            output: crate::OutputLocation::BesideSource,
            mode: RunMode::Create,
        };
        let control = RunControl::default();
        let plan = plan_destinations(collect_sources(&request, &control).unwrap(), &control);
        let source = bind_source(&plan, 0, &control)
            .unwrap_or_else(|error| panic!("Source failed: {error:?}"));
        let mut context = RunContext::new(plan.sources());
        let changed = source_path.clone();
        let hook = Box::new(move || {
            fs::write(changed, b"controlled changed owned source").unwrap();
        });
        if after {
            context.after_edge = Some(hook);
        } else {
            context.before_edge = Some(hook);
        }
        let result = execute_prepared(&plan, 0, &source, &mut context, &control);
        let ItemResult::Failed(failure) = &result else {
            panic!("Changed source qualified: {result:?}");
        };
        assert_eq!(
            (failure.error().code(), failure.error().phase()),
            (ErrorCode::SourceChanged, ErrorPhase::Publish)
        );
        if after {
            assert!(matches!(failure.effect(), FailureEffect::Published(_)));
            assert_eq!(
                fs::read(root.path().join("a.no-attachments.eml")).unwrap(),
                golden
            );
        } else {
            assert!(matches!(
                failure.effect(),
                FailureEffect::NotPublished { .. }
            ));
            assert!(!root.path().join("a.no-attachments.eml").exists());
        }
        assert_eq!(
            fs::read(&source_path).unwrap(),
            b"controlled changed owned source"
        );
        drop(result);
        drop(source);
        drop(plan);
        root.close().unwrap();
    }
}
#[cfg(unix)]
#[test]
fn controlled_requested_output_move_after_edge_keeps_complete_published_copy() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/run-tests");
    fs::create_dir_all(&base).unwrap();
    let root = tempfile::tempdir_in(base).unwrap();
    let input = include_bytes!("../../../../tests/contract/mime/inputs/01-gmail-attachment.eml");
    let golden = include_bytes!("../../../../tests/contract/mime/golden/01-gmail-attachment.eml");
    fs::write(root.path().join("a.eml"), input).unwrap();
    let output = root.path().join("out");
    let moved = root.path().join("moved");
    fs::create_dir(&output).unwrap();
    let request = RunRequest {
        paths: vec!["a.eml".into()],
        working_directory: root.path().to_owned(),
        output: crate::OutputLocation::Directory("out".into()),
        mode: RunMode::Create,
    };
    let control = RunControl::default();
    let plan = plan_destinations(collect_sources(&request, &control).unwrap(), &control);
    let source =
        bind_source(&plan, 0, &control).unwrap_or_else(|error| panic!("Source failed: {error:?}"));
    let mut context = RunContext::new(plan.sources());
    let (from, to) = (output.clone(), moved.clone());
    context.after_edge = Some(Box::new(move || {
        fs::rename(&from, &to).unwrap();
        fs::create_dir(from).unwrap();
    }));
    let result = execute_prepared(&plan, 0, &source, &mut context, &control);
    let ItemResult::Failed(failure) = &result else {
        panic!("Moved address qualified: {result:?}");
    };
    assert_eq!(failure.error().code(), ErrorCode::AddressChanged);
    let FailureEffect::Published(copy) = failure.effect() else {
        panic!("Published effect lost");
    };
    assert_eq!(copy.address(), AddressState::Changed);
    assert_eq!(
        copy.file_sync().unwrap().state,
        eml_fs::SyncState::Succeeded
    );
    assert_eq!(
        fs::read(moved.join("a.no-attachments.eml")).unwrap(),
        golden
    );
    assert_eq!(fs::read_dir(&output).unwrap().count(), 0);
    assert_eq!(fs::read(root.path().join("a.eml")).unwrap(), input);
    drop(result);
    drop(source);
    drop(plan);
    root.close().unwrap();
}
#[cfg(windows)]
#[test]
fn controlled_resync_gap_distinguishes_file_replacement_and_same_id_change() {
    for replace in [false, true] {
        let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/run-tests");
        fs::create_dir_all(&base).unwrap();
        let root = tempfile::tempdir_in(base).unwrap();
        let input =
            include_bytes!("../../../../tests/contract/mime/inputs/01-gmail-attachment.eml");
        let golden =
            include_bytes!("../../../../tests/contract/mime/golden/01-gmail-attachment.eml");
        fs::write(root.path().join("a.eml"), input).unwrap();
        let copy = root.path().join("a.no-attachments.eml");
        fs::write(&copy, golden).unwrap();
        let request = RunRequest {
            paths: vec!["a.eml".into()],
            working_directory: root.path().to_owned(),
            output: crate::OutputLocation::BesideSource,
            mode: RunMode::Create,
        };
        let control = RunControl::default();
        let plan = plan_destinations(collect_sources(&request, &control).unwrap(), &control);
        let source = bind_source(&plan, 0, &control)
            .unwrap_or_else(|error| panic!("Source failed: {error:?}"));
        let mut context = RunContext::new(plan.sources());
        let old = match plan.records()[0].destination().unwrap().observation() {
            crate::DestinationObservation::Present(entry) => entry.identity,
            _ => panic!("Owned copy missing"),
        };
        let changed = copy.clone();
        let working = root.path().to_owned();
        context.before_sync_reopen = Some(Box::new(move || {
            if replace {
                fs::remove_file(&changed).unwrap();
                fs::write(&changed, golden).unwrap();
            } else {
                fs::write(&changed, b"controlled changed copy").unwrap();
            }
            let paths = eml_fs::PathContext::capture(&working).unwrap();
            let now = eml_fs::observe_selected(&paths.resolve(&changed).unwrap()).unwrap();
            assert_eq!(now.identity != old, replace);
        }));
        let result = execute_prepared(&plan, 0, &source, &mut context, &control);
        let ItemResult::Failed(failure) = &result else {
            panic!("Gap change qualified: {result:?}");
        };
        assert_eq!(
            failure.error().code(),
            if replace {
                ErrorCode::AddressChanged
            } else {
                ErrorCode::DestinationExists
            }
        );
        let observed = failure.existing().unwrap();
        assert!(!observed.stable);
        assert_eq!(observed.bytes_equal, Some(true));
        assert!(observed.file_sync.is_none());
        assert_eq!(
            observed.address,
            if replace {
                AddressState::Changed
            } else {
                AddressState::NotChecked
            }
        );
        assert_eq!(fs::read(root.path().join("a.eml")).unwrap(), input);
        drop(result);
        drop(source);
        drop(plan);
        root.close().unwrap();
    }
}
