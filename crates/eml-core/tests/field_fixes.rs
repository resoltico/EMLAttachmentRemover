//! Field regressions at the real core boundary; no frontend admission or replay.
use eml_core::{
    ErrorCode, Event, EventSink, InvalidRequest, OutputLocation, RunControl, RunMode, RunRequest,
    RunStart, SettledItem, vocabulary::Outcome,
};
use std::{fs, io};
#[derive(Default)]
struct Capture {
    start: Option<RunStart>,
    items: Vec<SettledItem>,
}
impl EventSink for Capture {
    fn emit(&mut self, event: Event<'_>) -> io::Result<()> {
        match event {
            Event::Start(s) => self.start = Some(s.clone()),
            Event::Item(i) => self.items.push(i.into()),
            Event::Summary(_) => {}
        }
        Ok(())
    }
}
#[test]
fn y4_check_admits_unavailable_folder_but_create_stays_prestream() {
    let root = tempfile::tempdir().unwrap();
    let folder = fs::canonicalize(root.path()).unwrap();
    let input = folder.join("mail.eml");
    fs::write(&input, b"Subject: Mail\r\n\r\nBody\r\n").unwrap();
    for target in [folder.join("missing/copies"), folder.join("file")] {
        if target.ends_with("file") {
            fs::write(&target, b"owned").unwrap();
        }
        let mut request = RunRequest::from_absolute(
            vec![input.clone()],
            OutputLocation::Directory(target.clone()),
            RunMode::DryRun,
        )
        .unwrap();
        let mut check = Capture::default();
        let result = eml_core::run(&request, &RunControl::default(), &mut check).unwrap();
        assert!(check.start.unwrap().output_unavailable);
        assert_eq!(result.exit_class().code(), 1);
        assert_eq!(result.run_error().unwrap().code(), ErrorCode::BindFailed);
        assert_eq!(check.items.len(), 1);
        assert_eq!(check.items[0].outcome(), Outcome::NothingToRemove);
        assert!(
            check.items[0]
                .destination
                .as_ref()
                .unwrap()
                .starts_with(&target)
        );
        request.mode = RunMode::Create;
        let mut create = Capture::default();
        assert!(matches!(
            eml_core::run(&request, &RunControl::default(), &mut create),
            Err(InvalidRequest::OutputDirectory { .. })
        ));
        assert!(create.start.is_none() && create.items.is_empty());
        assert!(!folder.join("mail.no-attachments.eml").exists());
    }
}
#[test]
fn y4_unavailable_predictions_still_enforce_the_report_path_budget() {
    let root = tempfile::tempdir().unwrap();
    let folder = fs::canonicalize(root.path()).unwrap();
    for i in 0..300 {
        fs::write(
            folder.join(format!("{i}.eml")),
            b"Subject: Mail\r\n\r\nBody",
        )
        .unwrap();
    }
    let request = RunRequest::from_absolute(
        vec![folder.clone()],
        OutputLocation::Directory(folder.join(("x".repeat(200) + "/").repeat(150))),
        RunMode::DryRun,
    )
    .unwrap();
    let mut sink = Capture::default();
    let result = eml_core::run(&request, &RunControl::default(), &mut sink).unwrap();
    assert_eq!(
        result.run_error().unwrap().code(),
        ErrorCode::InputLimitExceeded
    );
    assert!(!result.run_error().unwrap().is_selection_limit());
    assert!(sink.items.is_empty());
}
#[test]
fn y2_whitespace_after_end_is_silent_without_claiming_byte_identical_copy() {
    let root = tempfile::tempdir().unwrap();
    let folder = fs::canonicalize(root.path()).unwrap();
    let input = folder.join("mail.eml");
    let original = b"Content-Type: multipart/mixed; boundary=B\r\n\r\npreamble\r\n--B\r\n\r\nbody\r\n--B--\r\n \t\r\n";
    fs::write(&input, original).unwrap();
    let request =
        RunRequest::from_absolute(vec![input], OutputLocation::BesideSource, RunMode::Create)
            .unwrap();
    let mut sink = Capture::default();
    eml_core::run(&request, &RunControl::default(), &mut sink).unwrap();
    assert!(sink.items[0].removed.is_empty());
    assert_eq!(sink.items[0].outcome(), Outcome::CopySaved);
    assert_eq!(
        fs::read(folder.join("mail.no-attachments.eml")).unwrap(),
        &original[..original.len() - 4]
    );
}
