//! Nonshipping synchronization still exercises the real engine and filesystem.
#![cfg(all(unix, feature = "native-fixtures"))]
use eml_core::{
    AddressState, ErrorCode, Event, EventSink, FailureEffect, ItemResult, OperationObservation,
    OutputLocation, RunControl, RunMode, RunRequest, run_observed,
};
use eml_fs::StoragePhase;
use std::{cell::Cell, fs, io, rc::Rc};

fn once_at(phase: StoragePhase, action: Box<dyn FnOnce()>) -> Box<dyn FnMut(StoragePhase)> {
    let mut action = Some(action);
    Box::new(move |observed| {
        if observed == phase
            && let Some(action) = action.take()
        {
            action();
        }
    })
}

const INPUT: &[u8] = include_bytes!("../../../tests/contract/mime/inputs/01-gmail-attachment.eml");
const GOLDEN: &[u8] = include_bytes!("../../../tests/contract/mime/golden/01-gmail-attachment.eml");

struct Sink {
    published: bool,
    items: usize,
    summaries: usize,
}
impl EventSink for Sink {
    fn emit(&mut self, event: Event<'_>) -> io::Result<()> {
        match event {
            Event::Start(start) => assert_eq!(start.total, 1),
            Event::Item(item) => {
                self.items += 1;
                let ItemResult::Failed(failure) = &item.result else {
                    panic!("Controlled real operation unexpectedly succeeded");
                };
                if self.published {
                    assert_eq!(failure.error().code(), ErrorCode::AddressChanged);
                    let FailureEffect::Published(copy) = failure.effect() else {
                        panic!("Actual publication receipt was lost");
                    };
                    assert_eq!(copy.address(), AddressState::Changed);
                } else {
                    assert_eq!(failure.error().code(), ErrorCode::Interrupted);
                    assert!(matches!(
                        failure.effect(),
                        FailureEffect::NotPublished { .. }
                    ));
                }
            }
            Event::Summary(_) => self.summaries += 1,
        }
        Ok(())
    }
}

#[test]
fn synchronized_full_run_preserves_real_stop_and_published_error_receipts() {
    for published in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("one.eml");
        fs::write(&source, INPUT).unwrap();
        let output = root.path().join("output");
        let moved = root.path().join("moved-output");
        fs::create_dir(&output).unwrap();
        let request = RunRequest {
            paths: vec![source.clone()],
            working_directory: root.path().to_owned(),
            output: OutputLocation::Directory(output.clone()),
            mode: RunMode::Create,
        };
        let control = RunControl::default();
        let reached = Rc::new(Cell::new(false));
        let observed = Rc::clone(&reached);
        let action: Box<dyn FnOnce()> = if published {
            let (output, moved) = (output.clone(), moved.clone());
            Box::new(move || {
                assert_eq!(
                    fs::read(output.join("one.7wxqoa2jifs7cdaqt2anqogulu.no-attachments.eml"))
                        .unwrap(),
                    GOLDEN
                );
                fs::rename(&output, moved).unwrap();
                fs::create_dir(output).unwrap();
                observed.set(true);
            })
        } else {
            let control = control.clone();
            Box::new(move || {
                observed.set(true);
                control.request_stop();
            })
        };
        let observation = OperationObservation {
            item_index: 0,
            callback: once_at(
                if published {
                    StoragePhase::AfterPublication
                } else {
                    StoragePhase::Publication
                },
                action,
            ),
        };
        let mut sink = Sink {
            published,
            items: 0,
            summaries: 0,
        };
        let outcome = run_observed(&request, &control, &mut sink, observation).unwrap();
        assert!(reached.get());
        assert_eq!((sink.items, sink.summaries), (1, 1));
        assert_eq!(outcome.counts().failed(), 1);
        assert_eq!(fs::read(&source).unwrap(), INPUT);
        assert!(
            !output
                .join("one.7wxqoa2jifs7cdaqt2anqogulu.no-attachments.eml")
                .exists()
        );
        if published {
            assert_eq!(
                fs::read(moved.join("one.7wxqoa2jifs7cdaqt2anqogulu.no-attachments.eml")).unwrap(),
                GOLDEN
            );
        }
        root.close().unwrap();
    }
}

#[test]
fn refused_selected_item_cannot_pass_unused_observation_to_next_message() {
    #[derive(Default)]
    struct Counts {
        created: usize,
        failed: usize,
    }
    impl EventSink for Counts {
        fn emit(&mut self, event: Event<'_>) -> io::Result<()> {
            if let Event::Item(item) = event {
                match &item.result {
                    ItemResult::Created(..) => self.created += 1,
                    ItemResult::Failed(_) => self.failed += 1,
                    _ => panic!("Unexpected controlled run outcome"),
                }
            }
            Ok(())
        }
    }
    let root = tempfile::tempdir().unwrap();
    let protected = root.path().join("a.eml");
    let valid = root.path().join("b.eml");
    let protected_bytes = include_bytes!("../../../tests/contract/mime/inputs/06-smime-signed.eml");
    fs::write(&protected, protected_bytes).unwrap();
    fs::write(&valid, INPUT).unwrap();
    let request = RunRequest {
        paths: vec![protected.clone(), valid.clone()],
        working_directory: root.path().to_owned(),
        output: OutputLocation::BesideSource,
        mode: RunMode::Create,
    };
    let reached = Rc::new(Cell::new(false));
    let observed = Rc::clone(&reached);
    let mut sink = Counts::default();
    run_observed(
        &request,
        &RunControl::default(),
        &mut sink,
        OperationObservation {
            item_index: 0,
            callback: Box::new(move |_| observed.set(true)),
        },
    )
    .unwrap();
    assert!(
        !reached.get(),
        "Refused first item's hook reached the second item"
    );
    assert_eq!((sink.created, sink.failed), (1, 1));
    assert_eq!(
        fs::read(
            root.path()
                .join("b.rsdsaffec2pcixjwzvef2ljmuu.no-attachments.eml")
        )
        .unwrap(),
        GOLDEN
    );
    assert!(
        !root
            .path()
            .join("a.ulaixrqur2pkcd3knlgxwbxf6i.no-attachments.eml")
            .exists()
    );
    assert_eq!(fs::read(protected).unwrap(), protected_bytes);
    assert_eq!(fs::read(valid).unwrap(), INPUT);
}

#[test]
fn stopping_second_publication_retains_first_copy_and_actual_interrupted_summary() {
    #[derive(Default)]
    struct Summary {
        created: usize,
        failed: usize,
        not_processed: usize,
        exit: u8,
    }
    impl EventSink for Summary {
        fn emit(&mut self, event: Event<'_>) -> io::Result<()> {
            if let Event::Summary(outcome) = event {
                self.created = outcome.counts().created();
                self.failed = outcome.counts().failed();
                self.not_processed = outcome.counts().not_processed();
                self.exit = outcome.exit_class().code();
            }
            Ok(())
        }
    }
    let root = tempfile::tempdir().unwrap();
    for name in ["one.eml", "two.eml"] {
        fs::write(root.path().join(name), INPUT).unwrap();
    }
    let control = RunControl::default();
    let stop = control.clone();
    let mut summary = Summary::default();
    run_observed(
        &RunRequest {
            paths: vec![root.path().join("one.eml"), root.path().join("two.eml")],
            working_directory: root.path().to_owned(),
            output: OutputLocation::BesideSource,
            mode: RunMode::Create,
        },
        &control,
        &mut summary,
        OperationObservation {
            item_index: 1,
            callback: once_at(
                StoragePhase::Publication,
                Box::new(move || stop.request_stop()),
            ),
        },
    )
    .unwrap();
    assert_eq!(
        (
            summary.created,
            summary.failed,
            summary.not_processed,
            summary.exit
        ),
        (1, 1, 0, 130)
    );
    assert_eq!(
        fs::read(
            root.path()
                .join("one.7wxqoa2jifs7cdaqt2anqogulu.no-attachments.eml")
        )
        .unwrap(),
        GOLDEN
    );
    assert!(
        !root
            .path()
            .join("two.ctpjzw7urreuwuroqgjcbuwt3m.no-attachments.eml")
            .exists()
    );
    assert_eq!(fs::read(root.path().join("one.eml")).unwrap(), INPUT);
    assert_eq!(fs::read(root.path().join("two.eml")).unwrap(), INPUT);
}
