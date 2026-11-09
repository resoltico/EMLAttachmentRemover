//! Exported wire bytes are checked at the wire boundary, never used to build rows.
use super::*;
use eml_protocol::records::{self, EventRecord, ItemStatus};
use slint::Model;
fn exported(c: &DesktopController, root: &Path, name: &str) -> Vec<u8> {
    let path = root.join(name);
    c.start_export(path.clone());
    pump(c, || !c.is_busy());
    fs::read(path).unwrap()
}
fn contract_item(name: &str) -> records::ItemRecord {
    let text = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("../../tests/contract/protocol/{name}.jsonl")),
    )
    .unwrap()
    .replace("@VERSION@", eml_core::product::VERSION);
    match records::decode_record(text.lines().nth(1).unwrap().as_bytes()).unwrap() {
        EventRecord::Item(i) => *i,
        _ => panic!("contract item expected"),
    }
}
#[test]
fn w4_report_is_the_protocol_one_contract_from_real_core() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "one.eml", ATTACHMENT);
    write(root.path(), "plain.eml", PLAIN);
    write(root.path(), "bad.eml", b"");
    let c = controller(root.path(), eml_core::run);
    c.stage(vec![root.path().to_owned()]).unwrap();
    pump(&c, || !c.is_busy());
    c.start_selection();
    pump(&c, || !c.is_busy());
    for existing in [false, true] {
        if existing {
            c.stage(vec![root.path().to_owned()]).unwrap();
            pump(&c, || !c.is_busy());
            c.start_selection();
            pump(&c, || !c.is_busy());
        }
        let bytes = exported(
            &c,
            root.path(),
            if existing {
                "existing.jsonl"
            } else {
                "created.jsonl"
            },
        );
        assert_eq!(bytes.last(), Some(&b'\n'));
        let records: Vec<_> = bytes
            .split(|b| *b == b'\n')
            .filter(|l| !l.is_empty())
            .map(|l| records::decode_record(l).expect("protocol-1 record"))
            .collect();
        assert_eq!(records.len(), 5);
        let EventRecord::Start(start) = &records[0] else {
            panic!("start record must be first")
        };
        assert_eq!(start.protocol, 1);
        assert_eq!(start.program, "emlattachmentremover");
        assert_eq!(start.version, eml_core::product::VERSION);
        assert_eq!(start.total, 3);
        assert!(!start.dry_run);
        for record in &records[1..4] {
            let EventRecord::Item(item) = record else {
                panic!("terminal item required")
            };
            let source = item.source.native_absolute_path().unwrap();
            let name = source.file_name().unwrap();
            let expected = contract_item(if name == "bad.eml" {
                "prepublication-failure"
            } else if existing {
                if name == "one.eml" {
                    "up-to-date-attachment"
                } else {
                    "up-to-date"
                }
            } else if name == "one.eml" {
                "created-attachment"
            } else {
                "created"
            });
            assert_eq!(item.status, expected.status);
            assert_eq!(item.source_bytes.value(), expected.source_bytes.value());
            assert_eq!(item.copy_bytes.value(), expected.copy_bytes.value());
            assert_eq!(item.removed.len(), expected.removed.len());
            assert_eq!(item.stripped_fields, expected.stripped_fields);
            for (actual, want) in item.removed.iter().zip(expected.removed) {
                assert_eq!(actual.section, want.section);
                assert_eq!(actual.content_type, want.content_type);
                assert_eq!(actual.filename.value(), want.filename.value());
                assert_eq!(actual.reason, want.reason);
                assert_eq!(actual.bytes, want.bytes);
            }
            if item.status == ItemStatus::Failed {
                assert_eq!(item.error.value().unwrap().code, "MALFORMED");
                assert_eq!(
                    item.error.value().unwrap().phase,
                    eml_core::ErrorPhase::Prepare
                );
            } else {
                let (address, file, namespace) = if existing {
                    let observed = item.existing_observation.value().unwrap();
                    assert!(observed.identity.value().is_some());
                    assert_eq!(observed.bytes_equal.value(), Some(&true));
                    assert!(observed.stable);
                    (
                        observed.address,
                        &observed.file_sync,
                        &observed.namespace_sync,
                    )
                } else {
                    let published = &item.publication;
                    assert_eq!(published.state, records::Publication::Published);
                    assert!(published.candidate_identity.value().is_some());
                    assert!(matches!(
                        published.cleanup.state,
                        records::Cleanup::Complete | records::Cleanup::NotNeeded
                    ));
                    assert!(published.cleanup.native_code.value().is_none());
                    assert!(published.cleanup.residual_name.value().is_none());
                    (
                        published.address,
                        &published.file_sync,
                        &published.namespace_sync,
                    )
                };
                assert_eq!(address, records::Address::Verified);
                for (method, state, code) in [
                    (file.method, file.state, file.native_code.value()),
                    (
                        namespace.method,
                        namespace.state,
                        namespace.native_code.value(),
                    ),
                ] {
                    assert_eq!(state, records::Synchronization::Succeeded);
                    match method {
                        records::SynchronizationMethod::NotRequested => {
                            panic!("synchronization was required")
                        }
                        records::SynchronizationMethod::FsyncFullfsyncUnsupported => {
                            assert!(matches!(code, Some(22 | 45 | 102)))
                        }
                        _ => assert!(code.is_none()),
                    }
                }
                assert!(namespace.required_edges > 0 && namespace.required_edges <= 513);
                assert_eq!(namespace.completed_edges, namespace.required_edges);
                assert!(namespace.failed_edge_index.value().is_none());
                assert!(
                    item.destination
                        .value()
                        .unwrap()
                        .native_absolute_path()
                        .unwrap()
                        .is_file()
                );
            }
        }
        let EventRecord::Summary(summary) = records.last().unwrap() else {
            panic!("summary record required")
        };
        assert_eq!(summary.counts.created, if existing { 0 } else { 2 });
        assert_eq!(summary.counts.up_to_date, if existing { 2 } else { 0 });
        assert_eq!(summary.counts.failed, 1);
        assert_eq!(summary.counts.not_processed, 0);
        assert_eq!(summary.exit_code, 1);
        assert!(summary.collection_complete);
    }
}
#[test]
fn w4_retained_report_bound_rejects_an_entire_oversized_write() {
    use std::io::Write;
    let mut buffer = eml_protocol::BoundedEventBuffer::new(eml_protocol::MAX_RUN_BYTES - 1);
    buffer.write_all(b"earlier complete event\n").unwrap();
    let oversized = vec![0u8; eml_protocol::MAX_RUN_BYTES];
    assert!(buffer.write_all(&oversized).is_err());
    assert_eq!(buffer.bytes(), b"earlier complete event\n");
}
#[test]
fn s16_export_pins_exact_bytes_and_rejects_stale_panels() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let c = done(root.path(), &["one.eml"]);
    assert!(c.window().get_view().can_export);
    let old = c.capture_export().unwrap();
    assert!(c.capture_export().is_none());
    let expected = exported_after_cancel(&c, &old, root.path());
    let old = c.capture_export().unwrap();
    c.start_over();
    let target = root.path().join("pinned.jsonl");
    c.start_captured_export(old, target.clone());
    pump(&c, || !c.is_busy());
    assert_eq!(fs::read(target).unwrap(), expected);
    assert_eq!(c.window().get_view().state, eml_gui::BatchState::Empty);
    c.stage(vec![root.path().join("one.eml")]).unwrap();
    pump(&c, || !c.is_busy());
    let stale = c.capture_export().unwrap();
    let current = c.capture_export().is_none();
    assert!(current);
    c.request_quit();
    c.cancel_quit();
    c.start_captured_export(stale, root.path().join("stale.jsonl"));
    assert!(!root.path().join("stale.jsonl").exists());
}
fn exported_after_cancel(
    c: &DesktopController,
    source: &eml_gui::ReportExport,
    root: &Path,
) -> Vec<u8> {
    c.cancel_export(source);
    exported(c, root, "first.jsonl")
}
#[test]
fn report_export_is_atomic_and_preserves_approved_replacement() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let c = done(root.path(), &["one.eml"]);
    let target = root.path().join("report.jsonl");
    fs::write(&target, b"old report").unwrap();
    c.start_export(target.clone());
    pump(&c, || !c.is_busy());
    let bytes = fs::read(&target).unwrap();
    assert!(!bytes.is_empty());
    assert!(records::decode_record(bytes.split(|b| *b == b'\n').next().unwrap()).is_ok());
    assert!(!fs::read_dir(root.path()).unwrap().any(|e| {
        e.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".emlattachmentremover-report-")
    }));
    let target = root.path().join("cannot-replace-directory");
    fs::create_dir(&target).unwrap();
    write(&target, "retained", b"unchanged");
    c.start_export(target.clone());
    pump(&c, || !c.is_busy());
    assert_eq!(fs::read(target.join("retained")).unwrap(), b"unchanged");
    assert!(!fs::read_dir(root.path()).unwrap().any(|e| {
        e.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".emlattachmentremover-report-")
    }));
    assert_eq!(c.window().get_rows().row_count(), 1);
}
