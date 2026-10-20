//! Real serial library execution, independent of any CLI renderer or signal adapter.
#![cfg(any(unix, windows))]
use eml_core::{
    Counts, DeliveryPhase, ErrorCode, ErrorPhase, Event, EventSink, ExitClass, FailureEffect,
    InvalidRequest, ItemResult, OutputLocation, PlanAction, RunControl, RunMode, RunRequest,
    RunStart, dry_run,
};
use std::{
    fs, io,
    path::{Path, PathBuf},
};
const INPUT: &[u8] = include_bytes!("../../../tests/contract/mime/inputs/01-gmail-attachment.eml");
const GOLDEN: &[u8] = include_bytes!("../../../tests/contract/mime/golden/01-gmail-attachment.eml");
fn root() -> tempfile::TempDir {
    let parent = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/run-tests");
    fs::create_dir_all(&parent).unwrap();
    tempfile::tempdir_in(parent).unwrap()
}
fn request(root: &Path, paths: &[&str], output: Option<&str>) -> RunRequest {
    RunRequest {
        paths: paths.iter().map(PathBuf::from).collect(),
        working_directory: root.to_owned(),
        output: output.map_or(OutputLocation::BesideSource, |path| {
            OutputLocation::Directory(path.into())
        }),
        mode: RunMode::DryRun,
    }
}
#[derive(Debug)]
struct ObservedItem {
    index: usize,
    source_native: PathBuf,
    destination_native: Option<PathBuf>,
    destination: Option<PathBuf>,
    action: Option<PlanAction>,
    error: Option<(ErrorCode, ErrorPhase)>,
    removals: Option<usize>,
    existing: Option<(Option<bool>, bool)>,
}
#[derive(Default)]
struct Sink {
    start: Option<RunStart>,
    items: Vec<ObservedItem>,
    summary: Option<(Counts, bool, u8)>,
    fail: Option<DeliveryPhase>,
    stop_after: Option<(DeliveryPhase, RunControl)>,
    after_start: Option<Box<dyn FnOnce()>>,
}
impl EventSink for Sink {
    fn emit(&mut self, event: Event<'_>) -> io::Result<()> {
        let phase = match event {
            Event::Start(start) => {
                assert!(self.start.replace(*start).is_none());
                assert!(start.dry_run);
                if let Some(action) = self.after_start.take() {
                    action();
                }
                DeliveryPhase::Start
            }
            Event::Item(item) => {
                self.observe_item(item);
                DeliveryPhase::Item
            }
            Event::Summary(outcome) => {
                assert!(self.start.is_some());
                assert!(
                    self.summary
                        .replace((
                            outcome.counts(),
                            outcome.interrupted(),
                            outcome.exit_class().code()
                        ))
                        .is_none()
                );
                let counts = outcome.counts();
                assert_eq!(
                    counts.created()
                        + counts.up_to_date()
                        + counts.planned()
                        + counts.failed()
                        + counts.not_processed(),
                    outcome.total()
                );
                DeliveryPhase::Summary
            }
        };
        if let Some((selected, control)) = &self.stop_after
            && *selected == phase
        {
            control.request_stop();
        }
        if self.fail == Some(phase) {
            return Err(io::Error::from(io::ErrorKind::BrokenPipe));
        }
        Ok(())
    }
}
impl Sink {
    fn observe_item(&mut self, item: &eml_core::ItemEvent<'_>) {
        assert!(self.start.is_some());
        assert!(self.summary.is_none());
        assert!(self.items.last().is_none_or(|last| last.index < item.index));
        let (action, error, metadata, existing) = match &item.result {
            ItemResult::Planned(plan) => (
                Some(plan.action()),
                None,
                Some(plan.metadata()),
                plan.existing(),
            ),
            ItemResult::Failed(failure) => {
                assert!(failure.error().code().allows_phase(failure.error().phase()));
                match failure.effect() {
                    FailureEffect::NotPublished {
                        candidate_identity,
                        address,
                        file_sync,
                        namespace_sync,
                        cleanup,
                    } => {
                        assert!(candidate_identity.is_none());
                        assert!(matches!(
                            address,
                            eml_core::AddressState::NotChecked
                                | eml_core::AddressState::Changed
                                | eml_core::AddressState::Unknown
                        ));
                        assert!(file_sync.is_none());
                        assert_eq!(namespace_sync.required_edges, 0);
                        assert!(namespace_sync.receipt.is_none());
                        assert_eq!(cleanup.state, eml_fs::CleanupState::NotNeeded);
                    }
                    _ => panic!("Dry run cannot publish"),
                }
                (
                    None,
                    Some((failure.error().code(), failure.error().phase())),
                    failure.metadata(),
                    failure.existing(),
                )
            }
            _ => panic!("Dry run cannot create or resynchronize a copy"),
        };
        if let Some(observed) = existing {
            assert!(observed.file_sync.is_none());
            assert!(observed.namespace_sync.receipt.is_none());
            assert_eq!(observed.namespace_sync.required_edges, 0);
        }
        self.items.push(ObservedItem {
            index: item.index,
            source_native: item.source.native.to_owned(),
            destination_native: item.destination.map(|path| path.native.to_owned()),
            destination: item.destination.map(|path| path.display.to_owned()),
            action,
            error,
            removals: metadata.map(|data| data.removed.len()),
            existing: existing.map(|data| (data.bytes_equal, data.stable)),
        });
    }
}
#[test]
fn library_plans_actual_mime_and_mirrored_layout_without_creating_entries() {
    let root = root();
    fs::create_dir_all(root.path().join("Inbox/nested")).unwrap();
    fs::create_dir(root.path().join("out")).unwrap();
    fs::write(root.path().join("Inbox/nested/a.eml"), INPUT).unwrap();
    let mut sink = Sink::default();
    let outcome = dry_run(
        &request(root.path(), &["Inbox"], Some("out")),
        &RunControl::default(),
        &mut sink,
    )
    .unwrap();
    assert_eq!(outcome.exit_class(), ExitClass::Success);
    assert_eq!(outcome.counts().planned(), 1);
    assert_eq!(sink.items[0].action, Some(PlanAction::Create));
    assert_eq!(sink.items[0].removals, Some(1));
    assert_eq!(
        sink.items[0].destination.as_deref(),
        Some(Path::new("out/Inbox/nested/a.no-attachments.eml"))
    );
    assert_eq!(fs::read_dir(root.path().join("out")).unwrap().count(), 0);
    assert_eq!(
        fs::read(root.path().join("Inbox/nested/a.eml")).unwrap(),
        INPUT
    );
    assert_eq!(sink.summary.unwrap().2, 0);
    root.close().unwrap();
}
#[test]
fn matching_readonly_existing_copy_is_a_verify_plan_without_sync() {
    let root = root();
    let source = root.path().join("a.eml");
    let existing = root.path().join("a.no-attachments.eml");
    fs::write(&source, INPUT).unwrap();
    fs::write(&existing, GOLDEN).unwrap();
    let permissions = fs::metadata(&existing).unwrap().permissions();
    let mut readonly = permissions.clone();
    readonly.set_readonly(true);
    fs::set_permissions(&existing, readonly).unwrap();
    let mut sink = Sink::default();
    let outcome = dry_run(
        &request(root.path(), &["a.eml"], None),
        &RunControl::default(),
        &mut sink,
    )
    .unwrap();
    assert_eq!(outcome.exit_class(), ExitClass::Success);
    assert_eq!(outcome.counts().planned(), 1);
    assert_eq!(outcome.counts().up_to_date(), 0);
    assert_eq!(sink.items[0].action, Some(PlanAction::VerifyExisting));
    assert_eq!(sink.items[0].existing, Some((Some(true), true)));
    assert_eq!(fs::read(&source).unwrap(), INPUT);
    assert_eq!(fs::read(&existing).unwrap(), GOLDEN);
    fs::set_permissions(&existing, permissions).unwrap();
    root.close().unwrap();
}
#[test]
fn differing_existing_bytes_keep_the_comparison_and_fail_without_overwrite() {
    let root = root();
    fs::write(root.path().join("a.eml"), INPUT).unwrap();
    fs::write(
        root.path().join("a.no-attachments.eml"),
        b"different owned copy",
    )
    .unwrap();
    let mut sink = Sink::default();
    let outcome = dry_run(
        &request(root.path(), &["a.eml"], None),
        &RunControl::default(),
        &mut sink,
    )
    .unwrap();
    assert_eq!(outcome.exit_class(), ExitClass::Failure);
    assert_eq!(outcome.counts().failed(), 1);
    assert_eq!(
        sink.items[0].error,
        Some((ErrorCode::DestinationExists, ErrorPhase::Existing))
    );
    assert_eq!(sink.items[0].existing, Some((Some(false), true)));
    assert_eq!(sink.items[0].removals, Some(1));
    assert_eq!(
        fs::read(root.path().join("a.no-attachments.eml")).unwrap(),
        b"different owned copy"
    );
    root.close().unwrap();
}
#[test]
fn failed_explicit_root_emits_only_its_gapped_known_position() {
    let root = root();
    fs::write(root.path().join("a.eml"), INPUT).unwrap();
    fs::write(root.path().join("b.eml"), INPUT).unwrap();
    let mut sink = Sink::default();
    let outcome = dry_run(
        &request(root.path(), &["a.eml", "missing.eml", "b.eml"], None),
        &RunControl::default(),
        &mut sink,
    )
    .unwrap();
    assert_eq!(outcome.total(), 3);
    assert!(!outcome.collection_complete());
    assert_eq!(
        outcome.run_error().unwrap().code(),
        ErrorCode::InputAdmissionFailed
    );
    assert_eq!(outcome.counts().failed(), 1);
    assert_eq!(outcome.counts().not_processed(), 2);
    assert_eq!(sink.items.len(), 1);
    assert_eq!(sink.items[0].index, 1);
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 2);
    root.close().unwrap();
}
#[test]
fn stop_from_start_or_after_a_terminal_item_preserves_discovery_and_counts() {
    for phase in [DeliveryPhase::Start, DeliveryPhase::Item] {
        let root = root();
        fs::write(root.path().join("a.eml"), INPUT).unwrap();
        fs::write(root.path().join("b.eml"), INPUT).unwrap();
        let control = RunControl::default();
        let mut sink = Sink {
            stop_after: Some((phase, control.clone())),
            ..Sink::default()
        };
        let outcome = dry_run(
            &request(root.path(), &["a.eml", "b.eml"], None),
            &control,
            &mut sink,
        )
        .unwrap();
        let done = usize::from(phase == DeliveryPhase::Item);
        assert_eq!(outcome.exit_class(), ExitClass::Interrupted);
        assert!(outcome.collection_complete());
        assert_eq!(outcome.total(), 2);
        assert_eq!(outcome.counts().planned(), done);
        assert_eq!(outcome.counts().not_processed(), 2 - done);
        assert_eq!(sink.items.len(), done);
        assert_eq!(sink.summary.unwrap().2, 130);
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 2);
        root.close().unwrap();
    }
}
#[test]
fn sink_failure_stops_delivery_and_work_without_requesting_user_stop() {
    for phase in [
        DeliveryPhase::Start,
        DeliveryPhase::Item,
        DeliveryPhase::Summary,
    ] {
        let root = root();
        fs::write(root.path().join("a.eml"), INPUT).unwrap();
        fs::write(root.path().join("b.eml"), INPUT).unwrap();
        let control = RunControl::default();
        let mut sink = Sink {
            fail: Some(phase),
            ..Sink::default()
        };
        let outcome = dry_run(
            &request(root.path(), &["a.eml", "b.eml"], None),
            &control,
            &mut sink,
        )
        .unwrap();
        let done = match phase {
            DeliveryPhase::Start => 0,
            DeliveryPhase::Item => 1,
            DeliveryPhase::Summary => 2,
        };
        assert_eq!(outcome.exit_class(), ExitClass::Failure);
        assert!(!control.is_stopped());
        assert!(!outcome.interrupted());
        assert_eq!(outcome.delivery_failure().unwrap().phase(), phase);
        assert_eq!(
            outcome.delivery_failure().unwrap().error().kind(),
            io::ErrorKind::BrokenPipe
        );
        assert_eq!(outcome.counts().planned(), done);
        assert_eq!(outcome.counts().not_processed(), 2 - done);
        assert_eq!(sink.items.len(), done);
        assert_eq!(sink.summary.is_some(), phase == DeliveryPhase::Summary);
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 2);
        root.close().unwrap();
    }
}
#[test]
fn mode_and_output_admission_errors_precede_events_and_processing() {
    let root = root();
    let mut req = request(&root.path().join("nonexistent"), &["a.eml"], None);
    req.mode = RunMode::Create;
    let mut sink = Sink::default();
    assert!(matches!(
        dry_run(&req, &RunControl::default(), &mut sink),
        Err(InvalidRequest::DryRunModeRequired)
    ));
    req = request(root.path(), &["a.eml"], Some("nonexistent"));
    assert!(matches!(
        dry_run(&req, &RunControl::default(), &mut sink),
        Err(InvalidRequest::OutputDirectory(_))
    ));
    assert!(sink.start.is_none());
    assert!(sink.items.is_empty());
    assert!(sink.summary.is_none());
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    root.close().unwrap();
}
#[test]
fn no_inputs_and_output_overlap_are_run_errors_without_fabricated_items() {
    let root = root();
    let mut sink = Sink::default();
    let outcome = dry_run(
        &request(root.path(), &[], None),
        &RunControl::default(),
        &mut sink,
    )
    .unwrap();
    assert_eq!(outcome.total(), 0);
    assert_eq!(outcome.run_error().unwrap().code(), ErrorCode::NoInputs);
    assert_eq!(outcome.exit_class(), ExitClass::Failure);
    assert!(sink.items.is_empty());
    fs::create_dir_all(root.path().join("Inbox/out")).unwrap();
    fs::write(root.path().join("Inbox/a.eml"), INPUT).unwrap();
    let mut sink = Sink::default();
    let outcome = dry_run(
        &request(root.path(), &["Inbox"], Some("Inbox/out")),
        &RunControl::default(),
        &mut sink,
    )
    .unwrap();
    assert_eq!(
        outcome.run_error().unwrap().code(),
        ErrorCode::OutputOverlap
    );
    assert_eq!(outcome.total(), 1);
    assert_eq!(outcome.counts().not_processed(), 1);
    assert!(sink.items.is_empty());
    assert_eq!(
        fs::read_dir(root.path().join("Inbox/out")).unwrap().count(),
        0
    );
    root.close().unwrap();
}
#[test]
fn collection_limit_emits_no_items_and_never_processes_a_truncated_subset() {
    let root = root();
    fs::create_dir(root.path().join("Inbox")).unwrap();
    for index in 0..4097 {
        fs::write(
            root.path().join(format!("Inbox/{index:04}.eml")),
            b"owned non-MIME sentinel",
        )
        .unwrap();
    }
    let mut sink = Sink::default();
    let outcome = dry_run(
        &request(root.path(), &["Inbox"], None),
        &RunControl::default(),
        &mut sink,
    )
    .unwrap();
    assert_eq!(
        outcome.run_error().unwrap().code(),
        ErrorCode::InputLimitExceeded
    );
    assert!(!outcome.collection_complete());
    assert_eq!(outcome.total(), 4096);
    assert_eq!(outcome.counts().not_processed(), 4096);
    assert!(sink.items.is_empty());
    assert_eq!(
        fs::read_dir(root.path().join("Inbox")).unwrap().count(),
        4097
    );
    root.close().unwrap();
}
#[test]
fn current_existing_identity_is_checked_again_after_start() {
    let root = root();
    let input = root.path().join("a.eml");
    let other = root.path().join("b.eml");
    let alias = root.path().join("a.no-attachments.eml");
    fs::write(&input, INPUT).unwrap();
    fs::write(&other, INPUT).unwrap();
    let source_for_action = other.clone();
    let alias_for_action = alias.clone();
    let mut sink = Sink {
        after_start: Some(Box::new(move || {
            fs::hard_link(source_for_action, alias_for_action).unwrap();
        })),
        ..Sink::default()
    };
    let outcome = dry_run(
        &request(root.path(), &["a.eml", "b.eml"], None),
        &RunControl::default(),
        &mut sink,
    )
    .unwrap();
    // The first source is unchanged. The new output aliases another selected
    // source, so only the all-source guard can produce this first-item refusal.
    assert_eq!(outcome.total(), 2);
    assert_eq!(
        sink.items[0].error,
        Some((ErrorCode::SourceDestinationAlias, ErrorPhase::Existing))
    );
    assert_eq!(sink.items[0].existing, Some((None, false)));
    assert_eq!(fs::read(&input).unwrap(), INPUT);
    assert_eq!(fs::read(&alias).unwrap(), INPUT);
    root.close().unwrap();
}
#[test]
fn all_known_collisions_fail_before_preparation_without_output_entries() {
    let root = root();
    fs::create_dir(root.path().join("out")).unwrap();
    fs::create_dir(root.path().join("left")).unwrap();
    fs::create_dir(root.path().join("right")).unwrap();
    for side in ["left", "right"] {
        fs::write(root.path().join(side).join("a.eml"), INPUT).unwrap();
    }
    let mut sink = Sink::default();
    let outcome = dry_run(
        &request(root.path(), &["left/a.eml", "right/a.eml"], Some("out")),
        &RunControl::default(),
        &mut sink,
    )
    .unwrap();
    assert_eq!(outcome.counts().failed(), 2);
    assert_eq!(sink.items.len(), 2);
    for item in sink.items {
        assert_eq!(
            item.error,
            Some((ErrorCode::DestinationCollision, ErrorPhase::Admission))
        );
        assert_eq!(item.removals, None);
    }
    assert_eq!(fs::read_dir(root.path().join("out")).unwrap().count(), 0);
    root.close().unwrap();
}
fn multipart(removals: usize) -> Vec<u8> {
    let mut value = b"MIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=owned\r\n\r\n--owned\r\nContent-Type: text/plain\r\n\r\nbody\r\n".to_vec();
    for _ in 0..removals {
        value.extend_from_slice(b"--owned\r\nContent-Type: application/octet-stream\r\nContent-Disposition: attachment\r\n\r\nx\r\n");
    }
    value.extend_from_slice(b"--owned--\r\n");
    value
}
#[test]
fn exact_run_removal_budget_includes_reported_failures_and_refuses_the_next_entry() {
    let root = root();
    let large = multipart(5000);
    for name in ["a", "b", "c", "d"] {
        fs::write(root.path().join(format!("{name}.eml")), &large).unwrap();
    }
    fs::write(root.path().join("e.eml"), multipart(1)).unwrap();
    fs::write(
        root.path().join("a.no-attachments.eml"),
        b"different owned copy",
    )
    .unwrap();
    let mut sink = Sink::default();
    let outcome = dry_run(
        &request(
            root.path(),
            &["a.eml", "b.eml", "c.eml", "d.eml", "e.eml"],
            None,
        ),
        &RunControl::default(),
        &mut sink,
    )
    .unwrap();
    assert_eq!(outcome.counts().planned(), 3);
    assert_eq!(outcome.counts().failed(), 2);
    assert_eq!(outcome.counts().not_processed(), 0);
    assert_eq!(sink.items.len(), 5);
    assert_eq!(
        sink.items[..4]
            .iter()
            .map(|item| item.removals.unwrap())
            .sum::<usize>(),
        20_000
    );
    assert_eq!(
        sink.items[0].error,
        Some((ErrorCode::DestinationExists, ErrorPhase::Existing))
    );
    assert_eq!(
        sink.items[4].error,
        Some((ErrorCode::LimitExceeded, ErrorPhase::Prepare))
    );
    assert_eq!(sink.items[4].removals, None);
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 6);
    assert_eq!(fs::read(root.path().join("a.eml")).unwrap(), large);
    root.close().unwrap();
}

#[test]
fn completed_summary_is_the_stop_cutoff_and_failed_delivery_keeps_stop_precedence() {
    for fail in [false, true] {
        let root = root();
        fs::write(root.path().join("a.eml"), INPUT).unwrap();
        let control = RunControl::default();
        let mut sink = Sink {
            stop_after: Some((DeliveryPhase::Summary, control.clone())),
            fail: fail.then_some(DeliveryPhase::Summary),
            ..Sink::default()
        };
        let outcome =
            dry_run(&request(root.path(), &["a.eml"], None), &control, &mut sink).unwrap();
        assert!(control.is_stopped());
        assert_eq!(sink.summary.unwrap().2, 0);
        assert_eq!(outcome.counts().planned(), 1);
        assert_eq!(outcome.interrupted(), fail);
        assert_eq!(outcome.delivery_failure().is_some(), fail);
        assert_eq!(outcome.exit_class().code(), if fail { 130 } else { 0 });
        root.close().unwrap();
    }
}
#[test]
fn changed_owned_source_after_start_is_not_prepared_as_a_different_input() {
    let root = root();
    let source = root.path().join("a.eml");
    fs::write(&source, INPUT).unwrap();
    let edited = source.clone();
    let mut sink = Sink {
        after_start: Some(Box::new(move || {
            fs::write(edited, b"changed owned file").unwrap()
        })),
        ..Sink::default()
    };
    let outcome = dry_run(
        &request(root.path(), &["a.eml"], None),
        &RunControl::default(),
        &mut sink,
    )
    .unwrap();
    assert_eq!(outcome.counts().failed(), 1);
    assert_eq!(
        sink.items[0].error,
        Some((ErrorCode::SourceChanged, ErrorPhase::Read))
    );
    assert_eq!(sink.items[0].removals, None);
    assert_eq!(fs::read(&source).unwrap(), b"changed owned file");
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    root.close().unwrap();
}
#[test]
fn changed_owned_output_root_after_start_preserves_the_frozen_binding() {
    let root = root();
    fs::write(root.path().join("a.eml"), INPUT).unwrap();
    let output = root.path().join("out");
    let moved = root.path().join("moved");
    fs::create_dir(&output).unwrap();
    let (old, replacement) = (output.clone(), moved.clone());
    let mut sink = Sink {
        after_start: Some(Box::new(move || {
            fs::rename(&old, replacement).unwrap();
            fs::create_dir(old).unwrap();
        })),
        ..Sink::default()
    };
    let outcome = dry_run(
        &request(root.path(), &["a.eml"], Some("out")),
        &RunControl::default(),
        &mut sink,
    )
    .unwrap();
    assert_eq!(outcome.counts().failed(), 1);
    assert_eq!(
        sink.items[0].error,
        Some((ErrorCode::AddressChanged, ErrorPhase::Address))
    );
    assert_eq!(sink.items[0].removals, Some(1));
    assert_eq!(fs::read_dir(&output).unwrap().count(), 0);
    assert_eq!(fs::read_dir(&moved).unwrap().count(), 0);
    assert_eq!(fs::read(root.path().join("a.eml")).unwrap(), INPUT);
    root.close().unwrap();
}
#[test]
fn actual_mime_refusals_are_started_failures_and_do_not_hide_later_plans() {
    let root = root();
    let protected = include_bytes!("../../../tests/contract/mime/inputs/06-smime-signed.eml");
    fs::write(root.path().join("protected.eml"), protected).unwrap();
    fs::write(
        root.path().join("no-body.eml"),
        b"Content-Type: application/octet-stream\r\n\r\nopaque\r\n",
    )
    .unwrap();
    fs::write(root.path().join("valid.eml"), INPUT).unwrap();
    let mut sink = Sink::default();
    let outcome = dry_run(
        &request(
            root.path(),
            &["protected.eml", "no-body.eml", "valid.eml"],
            None,
        ),
        &RunControl::default(),
        &mut sink,
    )
    .unwrap();
    assert_eq!(outcome.counts().failed(), 2);
    assert_eq!(outcome.counts().planned(), 1);
    assert_eq!(outcome.counts().not_processed(), 0);
    assert_eq!(
        sink.items[0].error,
        Some((ErrorCode::Protected, ErrorPhase::Prepare))
    );
    assert_eq!(
        sink.items[1].error,
        Some((ErrorCode::NoBody, ErrorPhase::Prepare))
    );
    assert_eq!(sink.items[2].action, Some(PlanAction::Create));
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 3);
    root.close().unwrap();
}

#[test]
fn native_filename_units_survive_actual_serial_preparation_and_destination_reporting() {
    use std::ffi::OsString;
    #[cfg(target_os = "linux")]
    let (name, expected) = {
        use std::os::unix::ffi::OsStringExt;
        (
            OsString::from_vec(b"a\xff.eml".to_vec()),
            OsString::from_vec(b"a\xff.no-attachments.eml".to_vec()),
        )
    };
    #[cfg(target_os = "macos")]
    let (name, expected) = (
        OsString::from("e\u{301}.eml"),
        OsString::from("e\u{301}.no-attachments.eml"),
    );
    #[cfg(windows)]
    let (name, expected) = {
        use std::os::windows::ffi::OsStringExt;
        let mut input = vec![0x61, 0xd800];
        input.extend(".eml".encode_utf16());
        let mut output = vec![0x61, 0xd800];
        output.extend(".no-attachments.eml".encode_utf16());
        (OsString::from_wide(&input), OsString::from_wide(&output))
    };
    let root = root();
    fs::write(root.path().join(&name), INPUT).unwrap();
    let req = RunRequest {
        paths: vec![PathBuf::from(&name)],
        working_directory: root.path().to_owned(),
        output: OutputLocation::BesideSource,
        mode: RunMode::DryRun,
    };
    let mut sink = Sink::default();
    let outcome = dry_run(&req, &RunControl::default(), &mut sink).unwrap();
    assert_eq!(outcome.exit_class(), ExitClass::Success);
    assert_eq!(
        sink.items[0].source_native.file_name(),
        Some(name.as_os_str())
    );
    assert_eq!(
        sink.items[0]
            .destination_native
            .as_ref()
            .unwrap()
            .file_name(),
        Some(expected.as_os_str())
    );
    assert_eq!(
        sink.items[0].destination.as_ref().unwrap().file_name(),
        Some(expected.as_os_str())
    );
    assert_eq!(fs::read(root.path().join(&name)).unwrap(), INPUT);
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    root.close().unwrap();
}

#[cfg(unix)]
#[test]
fn relative_io_keeps_the_captured_directory_after_its_address_moves() {
    let root = root();
    let cwd = root.path().join("working");
    let moved = root.path().join("moved");
    fs::create_dir_all(cwd.join("out")).unwrap();
    fs::write(cwd.join("a.eml"), INPUT).unwrap();
    let (from, to) = (cwd.clone(), moved.clone());
    let mut sink = Sink {
        after_start: Some(Box::new(move || {
            fs::rename(&from, &to).unwrap();
            fs::create_dir_all(from.join("out")).unwrap();
            fs::write(from.join("a.eml"), b"owned replacement directory entry").unwrap();
        })),
        ..Sink::default()
    };
    let outcome = dry_run(
        &request(&cwd, &["a.eml"], Some("out")),
        &RunControl::default(),
        &mut sink,
    )
    .unwrap();
    assert_eq!(outcome.exit_class(), ExitClass::Success);
    assert_eq!(sink.items[0].action, Some(PlanAction::Create));
    assert_eq!(sink.items[0].source_native, Path::new("a.eml"));
    assert_eq!(fs::read(moved.join("a.eml")).unwrap(), INPUT);
    assert_eq!(
        fs::read(cwd.join("a.eml")).unwrap(),
        b"owned replacement directory entry"
    );
    assert_eq!(fs::read_dir(moved.join("out")).unwrap().count(), 0);
    assert_eq!(fs::read_dir(cwd.join("out")).unwrap().count(), 0);
    root.close().unwrap();
}

#[path = "support/directory_alias.rs"]
mod directory_alias;
#[test]
fn selected_directory_alias_keeps_reported_spelling_through_preparation_and_comparison() {
    for output in [None, Some("out")] {
        let root = root();
        let target = root.path().join("target");
        let alias = root.path().join("alias");
        fs::create_dir(&target).unwrap();
        fs::create_dir(root.path().join("out")).unwrap();
        fs::write(target.join("a.eml"), INPUT).unwrap();
        fs::write(target.join("a.no-attachments.eml"), GOLDEN).unwrap();
        directory_alias::create(&target, &alias);
        let mut sink = Sink::default();
        let outcome = dry_run(
            &request(root.path(), &["alias"], output),
            &RunControl::default(),
            &mut sink,
        )
        .unwrap();
        assert_eq!(outcome.exit_class(), ExitClass::Success);
        assert_eq!(outcome.total(), 1);
        assert!(sink.items[0].source_native.ends_with("alias/a.eml"));
        let expected = if output.is_some() {
            "out/alias/a.no-attachments.eml"
        } else {
            "alias/a.no-attachments.eml"
        };
        assert_eq!(
            sink.items[0].destination.as_deref(),
            Some(Path::new(expected))
        );
        assert!(
            sink.items[0]
                .destination_native
                .as_ref()
                .unwrap()
                .ends_with(expected)
        );
        assert_eq!(
            sink.items[0].action,
            Some(if output.is_some() {
                PlanAction::Create
            } else {
                PlanAction::VerifyExisting
            })
        );
        assert_eq!(sink.items[0].removals, Some(1));
        assert_eq!(fs::read(target.join("a.eml")).unwrap(), INPUT);
        assert_eq!(
            fs::read(target.join("a.no-attachments.eml")).unwrap(),
            GOLDEN
        );
        assert_eq!(fs::read_dir(root.path().join("out")).unwrap().count(), 0);
        root.close().unwrap();
    }
}
