//! Real serial copy/resynchronization through the reusable business operation.
#![cfg(any(unix, windows))]
use eml_core::{
    AddressState, Counts, ErrorCode, ErrorPhase, Event, EventSink, ExitClass, FailureEffect,
    ItemResult, OutputLocation, RunControl, RunMode, RunRequest, run,
};
use std::{
    fs, io,
    path::{Path, PathBuf},
};
const INPUT: &[u8] = include_bytes!("../../../tests/contract/mime/inputs/01-gmail-attachment.eml");
const GOLDEN: &[u8] = include_bytes!("../../../tests/contract/mime/golden/01-gmail-attachment.eml");
fn root() -> tempfile::TempDir {
    let parent = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/execution-tests");
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
        mode: RunMode::Create,
    }
}
#[derive(Debug)]
struct Item {
    status: &'static str,
    code: Option<(ErrorCode, ErrorPhase)>,
    namespace_edges: u16,
    equality: Option<bool>,
    #[cfg(all(windows, feature = "native-fixtures"))]
    native_code: Option<i64>,
    #[cfg(all(windows, feature = "native-fixtures"))]
    existing: Option<eml_core::ExistingObservation>,
}
#[derive(Default)]
struct Sink {
    items: Vec<Item>,
    summary: Option<Counts>,
    fail_item: bool,
    stop_after: Option<RunControl>,
    after_first: Option<Box<dyn FnOnce()>>,
    after_start: Option<Box<dyn FnOnce()>>,
}
fn namespace(receipt: eml_core::NamespaceObservation) {
    assert!(receipt.required_edges > 0 && receipt.required_edges <= 513);
    assert_eq!(receipt.completed_edges, receipt.required_edges);
    assert!(receipt.failed_edge_index.is_none());
    assert_eq!(receipt.receipt.unwrap().state, eml_fs::SyncState::Succeeded);
}
impl EventSink for Sink {
    fn emit(&mut self, event: Event<'_>) -> io::Result<()> {
        match event {
            Event::Start(start) => {
                assert!(!start.dry_run);
                if let Some(action) = self.after_start.take() {
                    action();
                }
            }
            Event::Item(event) => {
                let item = match &event.result {
                    ItemResult::Created(copy, metadata) => {
                        assert_eq!(copy.publication().address(), AddressState::Verified);
                        assert_eq!(
                            copy.publication().file_sync().unwrap().state,
                            eml_fs::SyncState::Succeeded
                        );
                        namespace(copy.publication().namespace_sync());
                        assert!(matches!(
                            copy.publication().cleanup().state,
                            eml_fs::CleanupState::Complete | eml_fs::CleanupState::NotNeeded
                        ));
                        assert_eq!(copy.publication().cleanup().native_code, None);
                        assert_eq!(metadata.source_bytes, INPUT.len() as u64);
                        assert_eq!(metadata.copy_bytes, GOLDEN.len() as u64);
                        Item {
                            status: "created",
                            #[cfg(all(windows, feature = "native-fixtures"))]
                            native_code: None,
                            #[cfg(all(windows, feature = "native-fixtures"))]
                            existing: None,
                            code: None,
                            namespace_edges: copy.publication().namespace_sync().required_edges,
                            equality: None,
                        }
                    }
                    ItemResult::UpToDate(copy, _) => {
                        let copy = copy.observation();
                        assert_eq!(copy.address, AddressState::Verified);
                        assert!(copy.stable);
                        assert_eq!(copy.bytes_equal, Some(true));
                        assert_eq!(copy.file_sync.unwrap().state, eml_fs::SyncState::Succeeded);
                        namespace(copy.namespace_sync);
                        Item {
                            status: "up_to_date",
                            #[cfg(all(windows, feature = "native-fixtures"))]
                            native_code: None,
                            #[cfg(all(windows, feature = "native-fixtures"))]
                            existing: Some(copy.clone()),
                            code: None,
                            namespace_edges: copy.namespace_sync.required_edges,
                            equality: copy.bytes_equal,
                        }
                    }
                    ItemResult::Failed(failure) => {
                        assert!(failure.error().code().allows_phase(failure.error().phase()));
                        assert!(
                            matches!(failure.effect(), FailureEffect::NotPublished { .. }),
                            "Unexpected publication failure: {failure:?}"
                        );
                        Item {
                            status: "failed",
                            #[cfg(all(windows, feature = "native-fixtures"))]
                            native_code: failure
                                .error()
                                .native_error()
                                .and_then(|error| error.raw_os_error())
                                .map(i64::from),
                            #[cfg(all(windows, feature = "native-fixtures"))]
                            existing: failure.existing().cloned(),
                            code: Some((failure.error().code(), failure.error().phase())),
                            namespace_edges: failure
                                .existing()
                                .map_or(0, |observed| observed.namespace_sync.required_edges),
                            equality: failure.existing().and_then(|observed| observed.bytes_equal),
                        }
                    }
                    ItemResult::Planned(_) => {
                        panic!("Create execution cannot silently become a plan")
                    }
                };
                self.items.push(item);
                if self.items.len() == 1
                    && let Some(action) = self.after_first.take()
                {
                    action();
                }
                if let Some(control) = &self.stop_after {
                    control.request_stop();
                }
                if self.fail_item {
                    return Err(io::ErrorKind::BrokenPipe.into());
                }
            }
            Event::Summary(outcome) => {
                assert!(self.summary.replace(outcome.counts()).is_none());
                let c = outcome.counts();
                assert_eq!(
                    c.created() + c.up_to_date() + c.planned() + c.failed() + c.not_processed(),
                    outcome.total()
                );
            }
        }
        Ok(())
    }
}
#[test]
fn real_run_creates_independent_golden_then_resynchronizes_the_existing_copy() {
    let root = root();
    let source = root.path().join("a.eml");
    let copy = root
        .path()
        .join("a.ulaixrqur2pkcd3knlgxwbxf6i.no-attachments.eml");
    fs::write(&source, INPUT).unwrap();
    let req = request(root.path(), &["a.eml"], None);
    let mut first = Sink::default();
    let outcome = run(&req, &RunControl::default(), &mut first).unwrap();
    assert_eq!(
        outcome.exit_class(),
        ExitClass::Success,
        "{:?}",
        first.items
    );
    assert_eq!(outcome.counts().created(), 1);
    assert_eq!(first.items[0].status, "created");
    assert_eq!(first.items[0].namespace_edges, 1);
    assert_eq!(fs::read(&copy).unwrap(), GOLDEN);
    assert_eq!(fs::read(&source).unwrap(), INPUT);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&copy).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    let mut second = Sink::default();
    let outcome = run(&req, &RunControl::default(), &mut second).unwrap();
    assert_eq!(
        outcome.exit_class(),
        ExitClass::Success,
        "{:?}",
        second.items
    );
    assert_eq!(outcome.counts().up_to_date(), 1);
    assert_eq!(second.items[0].status, "up_to_date");
    assert_eq!(second.items[0].namespace_edges, 1);
    assert_eq!(fs::read(&copy).unwrap(), GOLDEN);
    assert_eq!(fs::read(&source).unwrap(), INPUT);
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 2);
    root.close().unwrap();
}
#[test]
fn new_multilevel_mirrors_have_all_required_namespace_edges_and_no_staging_residue() {
    let root = root();
    fs::create_dir_all(root.path().join("Inbox/deep")).unwrap();
    fs::create_dir(root.path().join("out")).unwrap();
    let source = root.path().join("Inbox/deep/a.eml");
    let copy = root
        .path()
        .join("out/Inbox.eml-dir/deep.eml-dir/a.me3ivmmrueemyhdrt3amlnsvre.no-attachments.eml");
    fs::write(&source, INPUT).unwrap();
    let req = request(root.path(), &["Inbox"], Some("out"));
    let mut sink = Sink::default();
    let outcome = run(&req, &RunControl::default(), &mut sink).unwrap();
    assert_eq!(outcome.exit_class(), ExitClass::Success, "{:?}", sink.items);
    assert_eq!(sink.items[0].namespace_edges, 5);
    assert_eq!(fs::read(&copy).unwrap(), GOLDEN);
    assert_eq!(fs::read(&source).unwrap(), INPUT);
    assert_eq!(fs::read_dir(copy.parent().unwrap()).unwrap().count(), 1);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for path in ["out/Inbox.eml-dir", "out/Inbox.eml-dir/deep.eml-dir"] {
            assert_eq!(
                fs::metadata(root.path().join(path))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
        }
    }
    let mut second = Sink::default();
    let outcome = run(&req, &RunControl::default(), &mut second).unwrap();
    assert_eq!(
        outcome.exit_class(),
        ExitClass::Success,
        "{:?}",
        second.items
    );
    assert_eq!(second.items[0].namespace_edges, 1);
    root.close().unwrap();
}
#[test]
fn readable_different_readonly_copy_is_a_conflict_before_any_sync_access() {
    let root = root();
    let source = root.path().join("a.eml");
    let copy = root
        .path()
        .join("a.ulaixrqur2pkcd3knlgxwbxf6i.no-attachments.eml");
    fs::write(&source, INPUT).unwrap();
    fs::write(&copy, b"different owned data").unwrap();
    let permissions = fs::metadata(&copy).unwrap().permissions();
    let mut readonly = permissions.clone();
    readonly.set_readonly(true);
    fs::set_permissions(&copy, readonly).unwrap();
    #[cfg(all(windows, feature = "native-fixtures"))]
    let before = (
        eml_fs::existing_sync_evidence::file_observation(&source).unwrap(),
        eml_fs::existing_sync_evidence::file_observation(&copy).unwrap(),
    );
    let mut sink = Sink::default();
    let outcome = run(
        &request(root.path(), &["a.eml"], None),
        &RunControl::default(),
        &mut sink,
    )
    .unwrap();
    #[cfg(all(windows, feature = "native-fixtures"))]
    retain_existing_evidence(
        "core-readonly-unequal",
        root.path(),
        &source,
        &copy,
        &before.0,
        &before.1,
        &sink.items[0],
    );
    fs::set_permissions(&copy, permissions).unwrap();
    assert_eq!(outcome.exit_class(), ExitClass::Failure);
    assert_eq!(
        sink.items[0].code,
        Some((ErrorCode::DestinationExists, ErrorPhase::Existing))
    );
    assert_eq!(sink.items[0].equality, Some(false));
    assert_eq!(sink.items[0].namespace_edges, 0);
    assert_eq!(fs::read(&copy).unwrap(), b"different owned data");
    assert_eq!(fs::read(&source).unwrap(), INPUT);
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 2);
    root.close().unwrap();
}
#[test]
fn same_run_output_identity_is_a_collision_even_when_its_bytes_match() {
    let root = root();
    for name in ["a.eml", "b.eml"] {
        fs::write(root.path().join(name), INPUT).unwrap();
    }
    let a = root
        .path()
        .join("a.ulaixrqur2pkcd3knlgxwbxf6i.no-attachments.eml");
    let b = root
        .path()
        .join("b.rsdsaffec2pcixjwzvef2ljmuu.no-attachments.eml");
    let (from, to) = (a.clone(), b.clone());
    let mut sink = Sink {
        after_first: Some(Box::new(move || fs::hard_link(from, to).unwrap())),
        ..Sink::default()
    };
    let outcome = run(
        &request(root.path(), &["a.eml", "b.eml"], None),
        &RunControl::default(),
        &mut sink,
    )
    .unwrap();
    assert_eq!(outcome.counts().created(), 1, "{:?}", sink.items);
    assert_eq!(outcome.counts().failed(), 1);
    assert_eq!(outcome.counts().up_to_date(), 0);
    assert_eq!(
        sink.items[1].code,
        Some((ErrorCode::DestinationCollision, ErrorPhase::Existing))
    );
    assert_eq!(fs::read(a).unwrap(), GOLDEN);
    assert_eq!(fs::read(b).unwrap(), GOLDEN);
    root.close().unwrap();
}
#[test]
fn delivery_failure_retains_the_created_result_and_stops_the_next_source() {
    let root = root();
    for name in ["a.eml", "b.eml"] {
        fs::write(root.path().join(name), INPUT).unwrap();
    }
    let mut sink = Sink {
        fail_item: true,
        ..Sink::default()
    };
    let control = RunControl::default();
    let outcome = run(
        &request(root.path(), &["a.eml", "b.eml"], None),
        &control,
        &mut sink,
    )
    .unwrap();
    assert_eq!(outcome.exit_class(), ExitClass::Failure);
    assert_eq!(outcome.counts().created(), 1, "{:?}", sink.items);
    assert_eq!(outcome.counts().not_processed(), 1);
    assert!(!control.is_stopped());
    assert!(sink.summary.is_none());
    assert_eq!(
        fs::read(
            root.path()
                .join("a.ulaixrqur2pkcd3knlgxwbxf6i.no-attachments.eml")
        )
        .unwrap(),
        GOLDEN
    );
    assert!(
        !root
            .path()
            .join("b.rsdsaffec2pcixjwzvef2ljmuu.no-attachments.eml")
            .exists()
    );
    root.close().unwrap();
}
#[test]
fn stop_after_a_published_terminal_item_keeps_its_complete_copy_and_counts() {
    let root = root();
    for name in ["a.eml", "b.eml"] {
        fs::write(root.path().join(name), INPUT).unwrap();
    }
    let control = RunControl::default();
    let mut sink = Sink {
        stop_after: Some(control.clone()),
        ..Sink::default()
    };
    let outcome = run(
        &request(root.path(), &["a.eml", "b.eml"], None),
        &control,
        &mut sink,
    )
    .unwrap();
    assert_eq!(outcome.exit_class(), ExitClass::Interrupted);
    assert_eq!(outcome.counts().created(), 1, "{:?}", sink.items);
    assert_eq!(outcome.counts().not_processed(), 1);
    assert_eq!(
        fs::read(
            root.path()
                .join("a.ulaixrqur2pkcd3knlgxwbxf6i.no-attachments.eml")
        )
        .unwrap(),
        GOLDEN
    );
    assert!(
        !root
            .path()
            .join("b.rsdsaffec2pcixjwzvef2ljmuu.no-attachments.eml")
            .exists()
    );
    root.close().unwrap();
}
#[test]
fn missing_explicit_root_prevents_every_other_copy_before_processing() {
    let root = root();
    fs::write(root.path().join("a.eml"), INPUT).unwrap();
    let mut sink = Sink::default();
    let outcome = run(
        &request(root.path(), &["a.eml", "missing.eml"], None),
        &RunControl::default(),
        &mut sink,
    )
    .unwrap();
    assert_eq!(
        outcome.run_error().unwrap().code(),
        ErrorCode::InputAdmissionFailed
    );
    assert_eq!(outcome.counts().failed(), 1);
    assert_eq!(outcome.counts().not_processed(), 1);
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    assert_eq!(fs::read(root.path().join("a.eml")).unwrap(), INPUT);
    root.close().unwrap();
}

#[test]
fn identical_readonly_copy_retains_equality_and_never_clears_its_attribute() {
    let root = root();
    let source = root.path().join("a.eml");
    let copy = root
        .path()
        .join("a.ulaixrqur2pkcd3knlgxwbxf6i.no-attachments.eml");
    fs::write(&source, INPUT).unwrap();
    fs::write(&copy, GOLDEN).unwrap();
    let permissions = fs::metadata(&copy).unwrap().permissions();
    let mut readonly = permissions.clone();
    readonly.set_readonly(true);
    fs::set_permissions(&copy, readonly).unwrap();
    #[cfg(all(windows, feature = "native-fixtures"))]
    let before = (
        eml_fs::existing_sync_evidence::file_observation(&source).unwrap(),
        eml_fs::existing_sync_evidence::file_observation(&copy).unwrap(),
    );
    let mut sink = Sink::default();
    let outcome = run(
        &request(root.path(), &["a.eml"], None),
        &RunControl::default(),
        &mut sink,
    )
    .unwrap();
    let stayed_readonly = fs::metadata(&copy).unwrap().permissions().readonly();
    #[cfg(all(windows, feature = "native-fixtures"))]
    retain_existing_evidence(
        "core-readonly-equal",
        root.path(),
        &source,
        &copy,
        &before.0,
        &before.1,
        &sink.items[0],
    );
    fs::set_permissions(&copy, permissions).unwrap();
    #[cfg(unix)]
    {
        assert_eq!(outcome.exit_class(), ExitClass::Success, "{:?}", sink.items);
        assert_eq!(outcome.counts().up_to_date(), 1);
    }
    #[cfg(windows)]
    {
        assert_eq!(outcome.exit_class(), ExitClass::Failure);
        assert_eq!(
            sink.items[0].code,
            Some((ErrorCode::ExistingSyncFailed, ErrorPhase::Existing))
        );
    }
    assert_eq!(sink.items[0].equality, Some(true));
    assert!(stayed_readonly);
    assert_eq!(fs::read(&copy).unwrap(), GOLDEN);
    assert_eq!(fs::read(&source).unwrap(), INPUT);
    root.close().unwrap();
}
#[test]
fn multiple_items_keep_frozen_missing_layout_provenance_without_retaining_batch_handles() {
    let root = root();
    fs::create_dir_all(root.path().join("Inbox/deep")).unwrap();
    fs::create_dir(root.path().join("out")).unwrap();
    for name in ["a.eml", "b.eml"] {
        fs::write(root.path().join("Inbox/deep").join(name), INPUT).unwrap();
    }
    let mut sink = Sink::default();
    let outcome = run(
        &request(root.path(), &["Inbox"], Some("out")),
        &RunControl::default(),
        &mut sink,
    )
    .unwrap();
    assert_eq!(outcome.exit_class(), ExitClass::Success, "{:?}", sink.items);
    assert_eq!(outcome.counts().created(), 2);
    assert_eq!(sink.items[0].namespace_edges, 5);
    assert_eq!(
        sink.items[1].namespace_edges,
        if cfg!(windows) { 5 } else { 1 }
    );
    for name in [
        "a.me3ivmmrueemyhdrt3amlnsvre.no-attachments.eml",
        "b.qmtwumtsfrv2ay6uorjwh3nmpa.no-attachments.eml",
    ] {
        assert_eq!(
            fs::read(
                root.path()
                    .join("out/Inbox.eml-dir/deep.eml-dir")
                    .join(name)
            )
            .unwrap(),
            GOLDEN
        );
    }
    assert_eq!(
        fs::read_dir(root.path().join("out/Inbox.eml-dir/deep.eml-dir"))
            .unwrap()
            .count(),
        2
    );
    root.close().unwrap();
}

#[path = "support/directory_alias.rs"]
mod directory_alias;
#[test]
fn selected_aliases_keep_requested_paths_through_real_publication_and_resynchronization() {
    for output in [None, Some("out")] {
        let root = root();
        let target = root.path().join("target");
        fs::create_dir(&target).unwrap();
        fs::create_dir(root.path().join("out")).unwrap();
        fs::write(target.join("a.eml"), INPUT).unwrap();
        directory_alias::create(&target, &root.path().join("alias"));
        let req = request(root.path(), &["alias"], output);
        let mut first = Sink::default();
        let outcome = run(&req, &RunControl::default(), &mut first).unwrap();
        assert_eq!(
            outcome.exit_class(),
            ExitClass::Success,
            "{:?}",
            first.items
        );
        assert_eq!(outcome.counts().created(), 1);
        let copy = if output.is_some() {
            root.path()
                .join("out/alias.eml-dir/a.qk7agux3h6qbo3hcinfmncoeiq.no-attachments.eml")
        } else {
            target.join("a.ulaixrqur2pkcd3knlgxwbxf6i.no-attachments.eml")
        };
        assert_eq!(fs::read(&copy).unwrap(), GOLDEN);
        assert_eq!(fs::read(target.join("a.eml")).unwrap(), INPUT);
        let mut second = Sink::default();
        let outcome = run(&req, &RunControl::default(), &mut second).unwrap();
        assert_eq!(
            outcome.exit_class(),
            ExitClass::Success,
            "{:?}",
            second.items
        );
        assert_eq!(outcome.counts().up_to_date(), 1);
        assert_eq!(fs::read(copy).unwrap(), GOLDEN);
        root.close().unwrap();
    }
}

#[cfg(unix)]
#[test]
fn existing_sync_does_not_require_namespace_creation_policy() {
    use std::os::unix::fs::PermissionsExt;
    let root = root();
    fs::write(root.path().join("a.eml"), INPUT).unwrap();
    let output = root
        .path()
        .join("a.ulaixrqur2pkcd3knlgxwbxf6i.no-attachments.eml");
    fs::write(&output, GOLDEN).unwrap();
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o777)).unwrap();
    let mut sink = Sink::default();
    let result = run(
        &request(root.path(), &["a.eml"], None),
        &RunControl::default(),
        &mut sink,
    )
    .unwrap();
    assert_eq!(result.exit_class(), ExitClass::Success);
    assert_eq!(sink.items[0].status, "up_to_date");
    assert_eq!(fs::read(&output).unwrap(), GOLDEN);
    assert_eq!(fs::read(root.path().join("a.eml")).unwrap(), INPUT);
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 2);
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
    root.close().unwrap();
}

#[cfg(unix)]
#[test]
fn only_the_immediate_mutation_parent_requires_namespace_admission() {
    use std::os::unix::fs::PermissionsExt;
    let root = root();
    fs::create_dir(root.path().join("Inbox")).unwrap();
    fs::write(root.path().join("Inbox/a.eml"), INPUT).unwrap();
    fs::create_dir_all(root.path().join("out/Inbox.eml-dir")).unwrap();
    fs::set_permissions(root.path().join("out"), fs::Permissions::from_mode(0o777)).unwrap();
    let mut sink = Sink::default();
    let result = run(
        &request(root.path(), &["Inbox"], Some("out")),
        &RunControl::default(),
        &mut sink,
    )
    .unwrap();
    assert_eq!(result.exit_class(), ExitClass::Success);
    assert_eq!(sink.items[0].status, "created");
    assert_eq!(
        fs::read(
            root.path()
                .join("out/Inbox.eml-dir/a.jqp3zjytiqvs4hdhcvxvgec6qa.no-attachments.eml")
        )
        .unwrap(),
        GOLDEN
    );
    fs::set_permissions(root.path().join("out"), fs::Permissions::from_mode(0o700)).unwrap();
    root.close().unwrap();
}

#[test]
fn raw_case_normalization_and_greek_stems_keep_independent_native_outputs() {
    let root = root();
    fs::create_dir(root.path().join("out")).unwrap();
    let cases = [
        (
            "upper",
            "A.eml",
            "A.gsfuhldmedi75npfikxt6rlujm.no-attachments.eml",
        ),
        (
            "lower",
            "a.eml",
            "a.ulaixrqur2pkcd3knlgxwbxf6i.no-attachments.eml",
        ),
        (
            "nfc",
            "é.eml",
            "é.mvvnv6ggb7vtnbb5eidfbxgq3m.no-attachments.eml",
        ),
        (
            "nfd",
            "e\u{301}.eml",
            "e\u{301}.w7nepcjblfyamzt2pfw3uib3xi.no-attachments.eml",
        ),
        (
            "sigma-upper",
            "Σ.eml",
            "Σ.k4defi4kyicoiqn267xetvtr64.no-attachments.eml",
        ),
        (
            "sigma-lower",
            "σ.eml",
            "σ.6uox7dgm4ga7yqem52yadgfoay.no-attachments.eml",
        ),
    ];
    let mut paths = Vec::new();
    for (parent, name, _) in cases {
        fs::create_dir(root.path().join(parent)).unwrap();
        let relative = PathBuf::from(parent).join(name);
        fs::write(root.path().join(&relative), INPUT).unwrap();
        paths.push(relative);
    }
    let request = RunRequest {
        paths,
        working_directory: root.path().to_owned(),
        output: OutputLocation::Directory("out".into()),
        mode: RunMode::Create,
    };
    for repeat in [false, true] {
        let mut sink = Sink::default();
        let outcome = run(&request, &RunControl::default(), &mut sink).unwrap();
        assert_eq!(outcome.exit_class(), ExitClass::Success, "{:?}", sink.items);
        assert_eq!(
            if repeat {
                outcome.counts().up_to_date()
            } else {
                outcome.counts().created()
            },
            6
        );
        for (parent, name, output) in cases {
            assert_eq!(
                fs::read(root.path().join("out").join(output)).unwrap(),
                GOLDEN
            );
            assert_eq!(
                fs::read(root.path().join(parent).join(name)).unwrap(),
                INPUT
            );
        }
        assert_eq!(fs::read_dir(root.path().join("out")).unwrap().count(), 6);
    }
}

#[test]
fn natively_coalesced_mirrors_retain_full_raw_route_tokens_on_repeat() {
    let root = root();
    fs::create_dir(root.path().join("out")).unwrap();
    for source in ["left/A/message.eml", "right/a/message.eml"] {
        fs::create_dir_all(root.path().join(source).parent().unwrap()).unwrap();
        fs::write(root.path().join(source), INPUT).unwrap();
    }
    let request = request(root.path(), &["right/a", "left/A"], Some("out"));
    for repeat in [false, true] {
        let mut sink = Sink::default();
        let outcome = run(&request, &RunControl::default(), &mut sink).unwrap();
        assert_eq!(outcome.exit_class(), ExitClass::Success, "{:?}", sink.items);
        assert_eq!(
            if repeat {
                outcome.counts().up_to_date()
            } else {
                outcome.counts().created()
            },
            2
        );
        for path in [
            "out/A.eml-dir/message.updzf5iesh4csbqanb4nwnbwvq.no-attachments.eml",
            "out/a.eml-dir/message.cm4zcng4zlgyhbvuybqmj3gnbq.no-attachments.eml",
        ] {
            assert_eq!(fs::read(root.path().join(path)).unwrap(), GOLDEN);
        }
    }
}

#[cfg(all(windows, feature = "native-fixtures"))]
fn retain_existing_evidence(
    case: &'static str,
    root: &Path,
    source: &Path,
    copy: &Path,
    before_source: &str,
    before_copy: &str,
    item: &Item,
) {
    use eml_fs::existing_sync_evidence::{self as evidence, Step};
    let after_source = evidence::file_observation(source).unwrap();
    let after_copy = evidence::file_observation(copy).unwrap();
    let (signature, parent) = evidence::file_binding(copy).unwrap();
    let mut step = Step::new("core_returned", "refused");
    step.signature = Some(signature);
    step.parent = Some(parent);
    step.bytes_equal = item.equality;
    step.native_code = item.native_code;
    if let Some(existing) = &item.existing {
        step.sync = existing.file_sync;
        step.namespace = Some(existing.namespace_sync.fixture_json());
    }
    evidence::write_case(
        case,
        root,
        root,
        Some(before_source),
        before_copy,
        &[step],
        item.code.expect("Observed core refusal").0.as_str(),
        item.native_code,
        Some(&after_source),
        &after_copy,
    )
    .unwrap();
}
