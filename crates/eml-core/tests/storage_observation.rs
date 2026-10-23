//! Finite fixture boundaries invoke the actual engine and retain its native receipts.
#![cfg(feature = "native-fixtures")]
use eml_core::{
    Event, EventSink, OperationObservation, OutputLocation, RunControl, RunMode, RunRequest,
    run_observed,
};
use eml_fs::StoragePhase;
use std::{cell::RefCell, fs, io, rc::Rc};
const INPUT: &[u8] = include_bytes!("../../../tests/contract/mime/inputs/01-gmail-attachment.eml");
const GOLDEN: &[u8] = include_bytes!("../../../tests/contract/mime/golden/01-gmail-attachment.eml");
struct Sink;
impl EventSink for Sink {
    fn emit(&mut self, _: Event<'_>) -> io::Result<()> {
        Ok(())
    }
}
#[test]
fn actual_create_existing_and_check_have_distinct_finite_boundaries() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("message.eml");
    fs::write(&source, INPUT).unwrap();
    let mut request = RunRequest {
        paths: vec![source.clone()],
        working_directory: root.path().to_owned(),
        output: OutputLocation::BesideSource,
        mode: RunMode::Create,
    };
    for expected in [
        vec![
            StoragePhase::Preflight,
            StoragePhase::StageWrite,
            StoragePhase::FileSync,
            StoragePhase::Publication,
            StoragePhase::AfterPublication,
            StoragePhase::NamespaceSync,
        ],
        vec![
            StoragePhase::Preflight,
            StoragePhase::Readback,
            StoragePhase::FileSync,
            StoragePhase::NamespaceSync,
        ],
        vec![],
    ] {
        if expected.is_empty() {
            request.mode = RunMode::DryRun;
        }
        let phases = Rc::new(RefCell::new(Vec::new()));
        let captured = Rc::clone(&phases);
        let result = run_observed(
            &request,
            &RunControl::default(),
            &mut Sink,
            OperationObservation {
                item_index: 0,
                callback: Box::new(move |phase| captured.borrow_mut().push(phase)),
            },
        )
        .unwrap();
        assert_eq!(result.counts().failed(), 0);
        assert_eq!(*phases.borrow(), expected);
        assert_eq!(fs::read(&source).unwrap(), INPUT);
        assert_eq!(
            fs::read(
                root.path()
                    .join("message.eudtfo5quf3d2hlohgknt7qpaa.no-attachments.eml")
            )
            .unwrap(),
            GOLDEN
        );
    }
}

#[cfg(unix)]
#[test]
fn changed_requested_parent_before_actual_edge_refuses_without_renaming_or_publication() {
    use eml_core::{ErrorCode, FailureEffect, ItemResult};
    use std::os::unix::fs::PermissionsExt;
    struct Refusal;
    impl EventSink for Refusal {
        fn emit(&mut self, event: Event<'_>) -> io::Result<()> {
            if let Event::Item(item) = event {
                let ItemResult::Failed(failure) = &item.result else {
                    panic!("Changed parent unexpectedly qualified")
                };
                assert_eq!(failure.error().code(), ErrorCode::AddressChanged);
                let FailureEffect::NotPublished {
                    candidate_identity,
                    cleanup,
                    ..
                } = failure.effect()
                else {
                    panic!("Unattempted edge became publication")
                };
                assert!(candidate_identity.is_some());
                assert_eq!(cleanup.state, eml_fs::CleanupState::Complete);
            }
            Ok(())
        }
    }
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("message.eml");
    let output = root.path().join("copies");
    let moved = root.path().join("retained-parent");
    fs::write(&source, INPUT).unwrap();
    fs::create_dir(&output).unwrap();
    let (from, to) = (output.clone(), moved.clone());
    let mut invoked = false;
    let callback = Box::new(move |phase| {
        if phase == StoragePhase::Publication {
            assert!(!invoked);
            invoked = true;
            fs::rename(&from, &to).unwrap();
            fs::create_dir(&from).unwrap();
            fs::set_permissions(&from, fs::Permissions::from_mode(0o777)).unwrap();
        }
    });
    let result = run_observed(
        &RunRequest {
            paths: vec![source.clone()],
            working_directory: root.path().to_owned(),
            output: OutputLocation::Directory(output.clone()),
            mode: RunMode::Create,
        },
        &RunControl::default(),
        &mut Refusal,
        OperationObservation {
            item_index: 0,
            callback,
        },
    )
    .unwrap();
    assert_eq!(result.counts().failed(), 1);
    assert_eq!(fs::read_dir(&output).unwrap().count(), 0);
    assert_eq!(fs::read_dir(&moved).unwrap().count(), 0);
    assert_eq!(
        fs::metadata(&output).unwrap().permissions().mode() & 0o777,
        0o777
    );
    assert_eq!(fs::read(&source).unwrap(), INPUT);
}

#[test]
fn stop_at_stage_write_or_file_sync_preserves_real_unattempted_storage_receipts() {
    use eml_core::{ErrorCode, FailureEffect, ItemResult};
    struct Interrupted;
    impl EventSink for Interrupted {
        fn emit(&mut self, event: Event<'_>) -> io::Result<()> {
            if let Event::Item(item) = event {
                let ItemResult::Failed(failure) = &item.result else {
                    panic!("Stopped stage qualified")
                };
                assert_eq!(failure.error().code(), ErrorCode::Interrupted);
                let FailureEffect::NotPublished {
                    file_sync, cleanup, ..
                } = failure.effect()
                else {
                    panic!("Stopped stage published")
                };
                assert!(file_sync.is_none());
                assert_eq!(cleanup.state, eml_fs::CleanupState::Complete);
            }
            Ok(())
        }
    }
    for selected in [StoragePhase::StageWrite, StoragePhase::FileSync] {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("message.eml");
        fs::write(&source, INPUT).unwrap();
        let output = root.path().join("copies");
        fs::create_dir(&output).unwrap();
        let control = RunControl::default();
        let stop = control.clone();
        let result = run_observed(
            &RunRequest {
                paths: vec![source.clone()],
                working_directory: root.path().to_owned(),
                output: OutputLocation::Directory(output.clone()),
                mode: RunMode::Create,
            },
            &control,
            &mut Interrupted,
            OperationObservation {
                item_index: 0,
                callback: Box::new(move |phase| {
                    if phase == selected {
                        stop.request_stop()
                    }
                }),
            },
        )
        .unwrap();
        assert_eq!(result.exit_class().code(), 130);
        assert_eq!(fs::read_dir(output).unwrap().count(), 0);
        assert_eq!(fs::read(source).unwrap(), INPUT);
    }
}
