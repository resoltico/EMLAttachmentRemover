//! Presentation of retained core facts, including receipts hard to induce via I/O.
use super::*;
use crate::{
    present::{Row, RowStatus},
    state::{Kind, Slot},
};
use eml_core::{
    AddressState, CleanupReceipt, CleanupState, ErrorCode, NamespaceObservation, PlanAction,
    SyncMethod, SyncReceipt, SyncState,
    vocabulary::{ExistingFacts, Outcome, OutcomeFacts, Publication, ResultKind},
};
use slint::{ComponentHandle, Model};
fn observation(outcome: Outcome, root: &std::path::Path, name: &str) -> SettledItem {
    let synced = SyncReceipt {
        method: SyncMethod::Fsync,
        state: SyncState::Succeeded,
        native_code: None,
    };
    let mut facts = OutcomeFacts {
        result: ResultKind::Planned,
        plan: Some(PlanAction::Create),
        publication: Publication::NotPublished,
        address: AddressState::NotChecked,
        candidate_identity_known: false,
        final_readback: None,
        file_sync: None,
        namespace_sync: NamespaceObservation {
            receipt: None,
            required_edges: 0,
            completed_edges: 0,
            failed_edge_index: None,
        },
        cleanup: CleanupReceipt {
            state: CleanupState::NotNeeded,
            residual_name: None,
            native_code: None,
        },
        existing: None,
        error: None,
        related_resources: false,
    };
    let mut removed = vec![eml_core::RemovedPart {
        section: "2".into(),
        content_type: "application/pdf".into(),
        filename: Some("report.pdf".into()),
        bytes: 500,
        reason: eml_core::RemovalReason::Attachment,
    }];
    match outcome {
        Outcome::Ready | Outcome::NothingToRemove => {}
        Outcome::CopySaved | Outcome::CopiedAsIs | Outcome::NeedsChecking => {
            facts.result = if outcome == Outcome::NeedsChecking {
                ResultKind::Failed
            } else {
                ResultKind::Created
            };
            facts.plan = None;
            facts.publication = Publication::Published;
            facts.address = AddressState::Verified;
            facts.candidate_identity_known = true;
            facts.final_readback = Some(true);
            facts.file_sync = Some(synced);
            facts.namespace_sync = NamespaceObservation {
                receipt: Some(synced),
                required_edges: 1,
                completed_edges: 1,
                failed_edge_index: None,
            };
            facts.cleanup.state = CleanupState::Complete;
            if outcome == Outcome::NeedsChecking {
                facts.error = Some(ErrorCode::FileSyncFailed);
                facts.file_sync = None;
            }
        }
        Outcome::AlreadyExists | Outcome::WillVerify => {
            facts.result = if outcome == Outcome::AlreadyExists {
                ResultKind::Existing
            } else {
                ResultKind::Planned
            };
            facts.plan = if outcome == Outcome::WillVerify {
                Some(PlanAction::VerifyExisting)
            } else {
                None
            };
            facts.existing = Some(ExistingFacts {
                identity_known: true,
                bytes_equal: Some(true),
                stable: true,
                address: AddressState::Verified,
                file_sync: if outcome == Outcome::AlreadyExists {
                    Some(synced)
                } else {
                    None
                },
                namespace_sync: if outcome == Outcome::AlreadyExists {
                    NamespaceObservation {
                        receipt: Some(synced),
                        required_edges: 1,
                        completed_edges: 1,
                        failed_edge_index: None,
                    }
                } else {
                    NamespaceObservation {
                        receipt: None,
                        required_edges: 0,
                        completed_edges: 0,
                        failed_edge_index: None,
                    }
                },
            });
        }
        Outcome::Unknown => {
            facts.result = ResultKind::Failed;
            facts.plan = None;
            facts.publication = Publication::Unknown;
        }
        Outcome::NotProcessed => {
            facts.result = ResultKind::Failed;
            facts.plan = None;
            facts.error = Some(ErrorCode::Interrupted);
        }
        Outcome::Failed(code) => {
            facts.result = ResultKind::Failed;
            facts.plan = None;
            facts.error = Some(code);
            removed.clear();
        }
    }
    if matches!(outcome, Outcome::NothingToRemove | Outcome::CopiedAsIs) {
        removed.clear();
    }
    SettledItem {
        index: 0,
        source: root.join(name),
        destination: Some(root.join(name).with_extension("no-attachments.eml")),
        facts,
        removed,
        source_bytes: Some(1000),
        copy_bytes: Some(500),
        failure: None,
        section: None,
    }
}
fn view(items: Vec<SettledItem>, kind: Kind) -> Rc<crate::DesktopController> {
    let root = tempfile::tempdir().unwrap();
    let c = crate::DesktopController::new(
        eml_core::run,
        root.path().to_owned(),
        crate::preferences::Preferences::load(root.path().join("preferences.json")),
    )
    .unwrap();
    let paths = items.iter().map(|i| i.source.clone()).collect();
    let total = items.len();
    let report = ReportHandle(Rc::new(RefCell::new(RunReport {
        start: Some(RunStart {
            dry_run: kind == Kind::Check,
            total,
            collection_complete: true,
        }),
        items,
        summary: None,
        settled: true,
    })));
    let completion = WorkerCompletion {
        state: WorkerState::Completed,
        failure: None,
        report: report.clone(),
        jsonl: Arc::from([]),
    };
    let slot = Some(Slot {
        generation: 1,
        report: Some(report),
        completion: Some(completion),
        launch_error: false,
        stopped: false,
    });
    {
        let mut r = c.runtime.borrow_mut();
        r.batch.inputs = paths;
        if kind == Kind::Check {
            r.batch.check = slot;
        } else {
            r.batch.create = slot;
        }
    }
    c.window().show().unwrap();
    c.refresh();
    c
}
#[test]
fn u1_s5_every_core_outcome_and_error_label_reaches_the_view() {
    i_slint_backend_testing::init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let mut outcomes = vec![
        Outcome::Ready,
        Outcome::NothingToRemove,
        Outcome::CopySaved,
        Outcome::CopiedAsIs,
        Outcome::AlreadyExists,
        Outcome::WillVerify,
        Outcome::NeedsChecking,
        Outcome::Unknown,
        Outcome::NotProcessed,
    ];
    outcomes.extend(ErrorCode::ALL.iter().copied().map(|c| {
        if c == ErrorCode::Interrupted {
            Outcome::NotProcessed
        } else {
            Outcome::Failed(c)
        }
    }));
    for outcome in outcomes {
        let item = observation(outcome, root.path(), "mail.eml");
        assert_eq!(item.outcome(), outcome);
        let c = view(
            vec![item],
            if matches!(
                outcome,
                Outcome::Ready | Outcome::NothingToRemove | Outcome::WillVerify
            ) {
                Kind::Check
            } else {
                Kind::Create
            },
        );
        let row = c.window().get_rows().row_data(0).unwrap();
        assert_eq!(row.status, outcome.label());
        c.window().invoke_select_email(row.key);
        assert!(
            c.window()
                .get_detail()
                .sentence
                .starts_with(outcome.explanation())
        );
        assert!(c.clipboard_text().unwrap().contains(outcome.label()));
        if matches!(outcome, Outcome::NeedsChecking | Outcome::Unknown) {
            assert_eq!(c.window().get_view().title, "Results need attention");
        }
    }
}
#[test]
fn u11_rows_follow_core_indices_not_filename_or_arrival_order() {
    i_slint_backend_testing::init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let names = ["mail10.eml", "mail2.eml", "mail1.eml"];
    let mut items: Vec<_> = names
        .iter()
        .enumerate()
        .map(|(index, name)| {
            let mut i = observation(Outcome::Ready, root.path(), name);
            i.index = index;
            i
        })
        .collect();
    items.swap(0, 2);
    let c = view(items, Kind::Check);
    let actual: Vec<_> = c
        .window()
        .get_rows()
        .iter()
        .map(|r| r.name.to_string())
        .collect();
    assert_eq!(actual, names);
    let selected = c.window().get_rows().row_data(1).unwrap().key;
    c.window().invoke_select_email(selected.clone());
    let mut r = c.runtime.borrow_mut();
    let report = r.batch.check.as_ref().unwrap().report.as_ref().unwrap();
    report.0.borrow_mut().items.swap(0, 1);
    r.batch.touch();
    drop(r);
    c.refresh();
    assert_eq!(
        c.window()
            .get_rows()
            .iter()
            .find(|r| r.selected)
            .unwrap()
            .key,
        selected
    );
}
#[test]
fn s5_s10_receipts_govern_reveal_and_cleanup_details_keep_native_facts() {
    i_slint_backend_testing::init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    for (outcome, expected) in [
        (Outcome::CopySaved, true),
        (Outcome::AlreadyExists, true),
        (Outcome::NeedsChecking, false),
        (Outcome::Unknown, false),
        (Outcome::Failed(ErrorCode::ReadFailed), false),
    ] {
        let c = view(
            vec![observation(outcome, root.path(), "mail.eml")],
            Kind::Create,
        );
        c.window()
            .invoke_select_email(c.window().get_rows().row_data(0).unwrap().key);
        assert_eq!(c.window().get_detail().can_show_copy, expected);
        assert_eq!(c.selected_copy().is_some(), expected);
    }
    let mut item = observation(Outcome::NeedsChecking, root.path(), "mail.eml");
    item.facts.cleanup = CleanupReceipt {
        state: CleanupState::Residual,
        residual_name: Some(".emlattachmentremover-\n\u{202e}.tmp".into()),
        native_code: Some(13),
    };
    let c = view(vec![item], Kind::Create);
    c.window()
        .invoke_select_email(c.window().get_rows().row_data(0).unwrap().key);
    let text = c.clipboard_text().unwrap();
    assert!(text.contains("A temporary file was left behind. Its name is in the details."));
    assert!(text.contains("Cleanup: Residual\nNative code: 13\nResidual resource: .emlattachmentremover-\\n\\u{202e}.tmp"));
    assert!(!c.window().get_detail().can_show_copy);
}
#[test]
fn core_consequences_are_shared_by_details_and_copy_details() {
    i_slint_backend_testing::init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    for (outcome, expected) in [
        (
            Outcome::NeedsChecking,
            "The copy was written, but saving it to disk couldn't be confirmed. Check the copy before running again.",
        ),
        (
            Outcome::Unknown,
            "A copy may have been written. Check the destination before running again.",
        ),
    ] {
        let item = observation(outcome, root.path(), "mail.eml");
        let status = RowStatus::Settled(item.outcome());
        let row = Row {
            key: item.source.clone(),
            copy: None,
            item,
            kind: Kind::Create,
            status,
        };
        assert!(row.details().contains(expected));
    }
    let mut item = observation(Outcome::NeedsChecking, root.path(), "mail.eml");
    item.facts.cleanup = CleanupReceipt {
        state: CleanupState::Complete,
        residual_name: None,
        native_code: Some(5),
    };
    let row = Row {
        key: item.source.clone(),
        copy: None,
        item,
        kind: Kind::Create,
        status: RowStatus::Settled(Outcome::NeedsChecking),
    };
    assert!(row.details().contains(
        "A temporary file was removed, but the system reported an error. See the details."
    ));
}
#[test]
fn unobserved_replacement_does_not_reuse_the_preview_notice() {
    i_slint_backend_testing::init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let mut item = observation(Outcome::Ready, root.path(), "mail.eml");
    item.facts.related_resources = true;
    let row = Row {
        key: item.source.clone(),
        item,
        kind: Kind::Check,
        status: RowStatus::Settled(Outcome::Unknown),
        copy: None,
    };
    let text = row.details();
    assert!(text.contains(Outcome::Unknown.explanation()));
    assert!(!text.contains(eml_core::vocabulary::PREPARED_RELATED_RESOURCE_NOTICE));
}
#[test]
fn row_size_and_missing_wording_stay_independent_of_protocol() {
    let root = tempfile::tempdir().unwrap();
    let item = observation(Outcome::Ready, root.path(), "mail.eml");
    let row = Row {
        key: item.source.clone(),
        item,
        kind: Kind::Check,
        status: RowStatus::Missing,
        copy: None,
    };
    assert_eq!(row.label(), "No longer found");
    assert_eq!(row.size_text(), "1.0 kB → 500 B");
}

#[test]
fn report_delivery_failure_keeps_the_typed_published_item() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("mail.eml");
    std::fs::write(
        &input,
        include_bytes!("../../../../tests/contract/mime/inputs/01-gmail-attachment.eml"),
    )
    .unwrap();
    let request = RunRequest::from_absolute(
        vec![input],
        eml_core::OutputLocation::BesideSource,
        eml_core::RunMode::Create,
    )
    .unwrap();
    let (sender, receiver) = mpsc::channel();
    let mut sink = RunSink {
        sender,
        json: JsonSink::new(BoundedEventBuffer::new(256)),
    };
    let result = eml_core::run(&request, &RunControl::default(), &mut sink).unwrap();
    assert!(result.delivery_failure().is_some());
    let observed: Vec<_> = receiver.try_iter().collect();
    let item = observed
        .iter()
        .find_map(|e| {
            if let Observation::Item(item) = e {
                Some(item)
            } else {
                None
            }
        })
        .expect("published typed observation retained");
    assert_eq!(item.outcome(), Outcome::CopySaved);
    assert!(item.reveal_path().is_some());
    assert_eq!(
        std::fs::read(root.path().join("mail.no-attachments.eml")).unwrap(),
        include_bytes!("../../../../tests/contract/mime/golden/01-gmail-attachment.eml")
    );
    let bytes = sink.json.into_inner().into_bytes();
    assert!(bytes.starts_with(b"{\"type\":\"start\""));
    assert_eq!(bytes.iter().filter(|b| **b == b'\n').count(), 1);
}

#[test]
fn x2_real_delivery_failure_uses_core_explanation_and_details() {
    i_slint_backend_testing::init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("mail.eml");
    std::fs::write(&input, b"Subject: Plain\r\n\r\nBody\r\n").unwrap();
    let c = crate::DesktopController::new(
        |r, control, sink| {
            sink.json = JsonSink::new(BoundedEventBuffer::new(0));
            eml_core::run(r, control, sink)
        },
        root.path().to_owned(),
        crate::preferences::Preferences::load(root.path().join("settings")),
    )
    .unwrap();
    c.stage(vec![input]).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while c.is_busy() {
        c.tick();
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert_eq!(
        c.runtime
            .borrow()
            .batch
            .check
            .as_ref()
            .unwrap()
            .completion
            .as_ref()
            .unwrap()
            .state,
        WorkerState::Incomplete
    );
    assert_eq!(
        c.window().get_view().banner,
        eml_core::vocabulary::RunFailure(ErrorCode::OutputFailed).explanation()
    );
    assert!(c.window().get_view().diagnostics.contains(
        "Code: OUTPUT_FAILED\nPhase: delivery\nDetail: Result delivery did not complete."
    ));
}
