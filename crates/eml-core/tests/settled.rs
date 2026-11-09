//! Real core events stay typed and usable after the worker and borrowed input end.
use eml_core::{
    Event, EventSink, OutputLocation, RunControl, RunMode, RunOutcome, RunRequest, RunStart,
    SettledItem,
    vocabulary::{self, Outcome},
};
use std::{fs, io};
#[derive(Default)]
struct Capture {
    start: Option<RunStart>,
    items: Vec<SettledItem>,
    summary: Option<RunOutcome>,
}
impl EventSink for Capture {
    fn emit(&mut self, event: Event<'_>) -> io::Result<()> {
        match event {
            Event::Start(s) => self.start = Some(s.clone()),
            Event::Item(i) => self.items.push(i.into()),
            Event::Summary(s) => self.summary = Some(s.clone()),
        }
        Ok(())
    }
}
#[test]
fn threaded_core_retains_native_facts_and_independent_receipt_navigation() {
    let root = tempfile::tempdir().unwrap();
    // Core reports extended native paths on Windows; use the OS-native fixture root.
    let folder = fs::canonicalize(root.path()).unwrap();
    fs::write(
        folder.join("one.eml"),
        include_bytes!("../../../tests/contract/mime/inputs/01-gmail-attachment.eml"),
    )
    .unwrap();
    fs::write(folder.join("plain.eml"), b"Subject: Plain\r\n\r\nBody\r\n").unwrap();
    fs::write(folder.join("bad.eml"), b"").unwrap();
    let paths = ["one.eml", "plain.eml", "bad.eml"].map(|n| folder.join(n));
    for mode in [RunMode::DryRun, RunMode::Create, RunMode::Create] {
        let request =
            RunRequest::from_absolute(paths.to_vec(), OutputLocation::BesideSource, mode).unwrap();
        let capture = std::thread::spawn(move || {
            let mut c = Capture::default();
            let outcome = eml_core::run(&request, &RunControl::default(), &mut c).unwrap();
            assert_eq!(c.summary.as_ref().unwrap().counts(), outcome.counts());
            c
        })
        .join()
        .unwrap();
        let source = capture.items.iter().find(|i| i.source == paths[0]).unwrap();
        assert_eq!(source.source, paths[0]);
        assert_eq!(source.removed[0].filename.as_deref(), Some("report.pdf"));
        assert_eq!(source.copy_size_reduction(), Some(515));
        if mode == RunMode::DryRun {
            assert_eq!(source.outcome(), Outcome::Ready);
            assert!(source.reveal_path().is_none());
        } else {
            assert_eq!(
                source.reveal_path(),
                Some(folder.join("one.no-attachments.eml").as_path())
            );
        }
        let failure = capture
            .items
            .iter()
            .find(|i| i.source == paths[2])
            .unwrap()
            .failure
            .as_ref()
            .unwrap();
        assert_eq!(failure.code(), eml_core::ErrorCode::Malformed);
        assert_eq!(failure.phase(), eml_core::ErrorPhase::Prepare);
        assert!(capture.summary.as_ref().unwrap().results_cover_inventory());
        assert_eq!(
            capture
                .summary
                .as_ref()
                .unwrap()
                .settled_totals(&capture.items)
                .emails,
            3
        );
        assert!(vocabulary::can_offer_copy(
            capture.items.iter().map(SettledItem::outcome),
            None
        ));
        assert_eq!(
            capture.start.unwrap().unreported_item_count(
                capture.items.len(),
                capture.summary.as_ref(),
                true
            ),
            None
        );
    }
    assert_eq!(
        fs::read(folder.join("one.no-attachments.eml")).unwrap(),
        include_bytes!("../../../tests/contract/mime/golden/01-gmail-attachment.eml")
    );
}
#[test]
fn incomplete_counts_and_invalid_admission_keep_typed_causes() {
    let start = RunStart {
        run_error: None,
        output_unavailable: false,
        total: 3,
        dry_run: false,
        collection_complete: true,
    };
    assert_eq!(start.unreported_item_count(1, None, false), None);
    assert_eq!(start.unreported_item_count(1, None, true), Some(2));
    assert_eq!(start.unreported_item_count(4, None, true), None);
    let e = eml_core::InvalidRequest::OutputDirectory {
        path: std::path::PathBuf::from("/save"),
        error: io::Error::from_raw_os_error(13),
    }
    .into_failure();
    assert_eq!(e.code(), eml_core::ErrorCode::BindFailed);
    assert_eq!(
        e.native_diagnostic(),
        Some(eml_core::NativeDiagnostic::OsCode(13))
    );
    let e = eml_core::OperationFailure::delivery(io::Error::from_raw_os_error(32));
    assert_eq!(e.code(), eml_core::ErrorCode::OutputFailed);
    assert_eq!(e.phase(), eml_core::ErrorPhase::Delivery);
}
