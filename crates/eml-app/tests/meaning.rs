//! Core facts and independently accepted transport facts must mean the same thing.
use eml_core::meaning::{OutcomeFacts, OutcomeMeaning, classify};
use eml_core::{Event, EventSink, OutputLocation, RunControl, RunMode, RunRequest};
use std::io;
struct Sink {
    report: eml_app::RunReport,
    meanings: Vec<OutcomeMeaning>,
}
impl EventSink for Sink {
    fn emit(&mut self, event: Event<'_>) -> io::Result<()> {
        let mut encoded = eml_protocol::JsonSink::new(Vec::new());
        if let Event::Item(item) = &event {
            self.meanings
                .push(classify(&OutcomeFacts::from(&item.result)));
        }
        encoded.emit(event)?;
        let bytes = encoded.into_inner();
        self.report
            .accept_frame(&bytes[..bytes.len() - 1])
            .map_err(|e| io::Error::other(format!("{e:?}")))
    }
}
#[test]
fn actual_core_results_and_validated_wire_projections_agree() {
    let parent =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/meaning-tests");
    std::fs::create_dir_all(&parent).unwrap();
    let root = tempfile::tempdir_in(parent).unwrap();
    let root_path = std::fs::canonicalize(root.path()).unwrap();
    std::fs::write(
        root_path.join("message.eml"),
        b"Content-Type: text/plain\r\n\r\nbody\r\n",
    )
    .unwrap();
    for mode in [
        RunMode::DryRun,
        RunMode::Create,
        RunMode::DryRun,
        RunMode::Create,
    ] {
        let request = RunRequest {
            paths: vec!["message.eml".into()],
            working_directory: root_path.clone(),
            output: OutputLocation::BesideSource,
            mode,
        };
        let mut sink = Sink {
            report: eml_app::RunReport::new(eml_core::product::VERSION),
            meanings: Vec::new(),
        };
        let outcome = eml_core::run(&request, &RunControl::default(), &mut sink).unwrap();
        assert_eq!(outcome.exit_class(), eml_core::ExitClass::Success);
        sink.report.finish(Some(0), false).unwrap();
        let wire: Vec<_> = sink
            .report
            .items()
            .iter()
            .map(|i| classify(&eml_protocol::meaning::outcome_facts(i).unwrap()))
            .collect();
        assert_eq!(wire, sink.meanings);
    }
    root.close().unwrap();
}

#[test]
fn report_refusal_is_accepted_end_to_end_and_has_no_destination_effect() {
    let parent =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/meaning-tests");
    std::fs::create_dir_all(&parent).unwrap();
    let root = tempfile::tempdir_in(parent).unwrap();
    let root_path = std::fs::canonicalize(root.path()).unwrap();
    let source = include_bytes!("../../../tests/contract/mime/dispositions/report_valid_dsn.eml");
    std::fs::write(root_path.join("report.eml"), source).unwrap();
    for mode in [RunMode::DryRun, RunMode::Create] {
        let request = RunRequest {
            paths: vec!["report.eml".into()],
            working_directory: root_path.clone(),
            output: OutputLocation::BesideSource,
            mode,
        };
        let mut sink = Sink {
            report: eml_app::RunReport::new(eml_core::product::VERSION),
            meanings: Vec::new(),
        };
        let outcome = eml_core::run(&request, &RunControl::default(), &mut sink).unwrap();
        assert_eq!(outcome.exit_class(), eml_core::ExitClass::Failure);
        sink.report.finish(Some(1), false).unwrap();
        let item = &sink.report.items()[0];
        let error = item.error.value().unwrap();
        assert_eq!(error.code, "UNSUPPORTED_MIME");
        assert_eq!(error.phase, eml_protocol::records::ErrorPhase::Prepare);
        assert_eq!(
            item.publication.state,
            eml_protocol::records::Publication::NotPublished
        );
        assert!(item.removed.is_empty());
        assert!(item.copy_bytes.value().is_none());
        assert_eq!(std::fs::read(root_path.join("report.eml")).unwrap(), source);
        assert_eq!(std::fs::read_dir(&root_path).unwrap().count(), 1);
    }
    root.close().unwrap();
}
