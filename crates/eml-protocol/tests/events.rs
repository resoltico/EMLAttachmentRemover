//! Hand-authored protocol output and actual owned-library integration.
use eml_core::{ExitClass, OutputLocation, RunControl, RunMode, RunRequest, run};
use eml_protocol::JsonSink;
use std::{fs, path::Path};
fn root() -> tempfile::TempDir {
    let parent = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/event-tests");
    fs::create_dir_all(&parent).unwrap();
    tempfile::tempdir_in(parent).unwrap()
}
fn request(root: &Path, mode: RunMode) -> RunRequest {
    RunRequest {
        paths: vec!["a.eml".into()],
        working_directory: fs::canonicalize(root).unwrap(),
        output: OutputLocation::BesideSource,
        mode,
    }
}
fn values(bytes: &[u8]) -> Vec<serde_json::Value> {
    assert_eq!(bytes.last(), Some(&b'\n'));
    bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice(line).unwrap())
        .collect()
}
#[test]
fn actual_dry_run_matches_hand_authored_native_path_and_fixed_receipt_golden() {
    let root = root();
    let input = b"Content-Type: text/plain\r\n\r\nbody\r\n";
    assert_eq!(input.len(), 34);
    fs::write(root.path().join("a.eml"), input).unwrap();
    let req = request(root.path(), RunMode::DryRun);
    let mut sink = JsonSink::new(Vec::new());
    let outcome = run(&req, &RunControl::default(), &mut sink).unwrap();
    assert_eq!(outcome.exit_class(), ExitClass::Success);
    let bytes = sink.into_inner();
    #[cfg(unix)]
    let actual = String::from_utf8(bytes).unwrap();
    #[cfg(windows)]
    let actual = {
        use base64::{Engine, engine::general_purpose::STANDARD};
        use std::os::windows::ffi::{OsStrExt, OsStringExt};
        let prefix: Vec<_> = req.working_directory.as_os_str().encode_wide().collect();
        let mut events = values(&bytes);
        for field in ["source", "destination"] {
            let raw = STANDARD
                .decode(events[1][field]["native"]["base64"].as_str().unwrap())
                .unwrap();
            assert_eq!(raw.len() % 2, 0);
            let wide: Vec<_> = raw
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                .collect();
            assert!(wide.starts_with(&prefix));
            assert_eq!(wide[prefix.len()], 92);
            let mut normalized: Vec<_> = r"\\?\C:\ROOT".encode_utf16().collect();
            normalized.extend_from_slice(&wide[prefix.len()..]);
            let native = std::ffi::OsString::from_wide(&normalized);
            let encoded: Vec<_> = native.encode_wide().flat_map(u16::to_le_bytes).collect();
            events[1][field]["native"]["base64"] = STANDARD.encode(encoded).into();
        }
        // Structural equality preserves the hand-authored protocol values while
        // normalization changes only the random temporary root.
        let expected =
            include_str!("../../../tests/contract/protocol/planned-create-windows.jsonl")
                .replace("@VERSION@", env!("CARGO_PKG_VERSION"));
        assert_eq!(events, values(expected.as_bytes()));
        String::new()
    };
    #[cfg(unix)]
    assert_eq!(
        actual,
        include_str!("../../../tests/contract/protocol/planned-create-unix.jsonl")
            .replace("@VERSION@", env!("CARGO_PKG_VERSION"))
    );
    #[cfg(windows)]
    assert!(actual.is_empty());
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    assert_eq!(fs::read(root.path().join("a.eml")).unwrap(), input);
    root.close().unwrap();
}
#[test]
fn actual_created_and_up_to_date_wire_fields_come_from_qualified_core_results() {
    let root = root();
    let input = include_bytes!("../../../tests/contract/mime/inputs/01-gmail-attachment.eml");
    let golden = include_bytes!("../../../tests/contract/mime/golden/01-gmail-attachment.eml");
    fs::write(root.path().join("a.eml"), input).unwrap();
    let req = request(root.path(), RunMode::Create);
    for status in ["created", "up_to_date"] {
        let mut sink = JsonSink::new(Vec::new());
        let outcome = run(&req, &RunControl::default(), &mut sink).unwrap();
        assert_eq!(outcome.exit_class(), ExitClass::Success);
        let events = values(&sink.into_inner());
        assert_eq!(events.len(), 3);
        let item = &events[1];
        assert_eq!(item["status"], status);
        assert!(item["error"].is_null());
        assert!(item["plan"].is_null());
        assert_eq!(item["removed"][0]["reason"], "attachment");
        assert_eq!(item["removed"][0]["filename"], "report.pdf");
        let receipt = if status == "created" {
            assert_eq!(item["publication"]["state"], "published");
            assert!(item["existing_observation"].is_null());
            &item["publication"]
        } else {
            assert_eq!(item["publication"]["state"], "not_published");
            assert_eq!(item["publication"]["file_sync"]["state"], "not_attempted");
            assert_eq!(item["existing_observation"]["bytes_equal"], true);
            &item["existing_observation"]
        };
        assert_eq!(receipt["address"], "verified");
        assert_eq!(receipt["file_sync"]["state"], "succeeded");
        assert_eq!(receipt["namespace_sync"]["state"], "succeeded");
        assert_eq!(receipt["namespace_sync"]["required_edges"], 1);
        assert_eq!(receipt["namespace_sync"]["completed_edges"], 1);
        assert_eq!(events[2]["exit_code"], 0);
        #[cfg(unix)]
        {
            let mut normalized = events.clone();
            let observation = if status == "created" {
                &mut normalized[1]["publication"]
            } else {
                &mut normalized[1]["existing_observation"]
            };
            let identity_key = if status == "created" {
                "candidate_identity"
            } else {
                "identity"
            };
            assert_eq!(observation[identity_key]["kind"], "posix");
            assert!(
                !observation[identity_key]["value"]
                    .as_str()
                    .unwrap()
                    .is_empty()
            );
            observation[identity_key]["value"] = "1:2".into();
            for key in ["file_sync", "namespace_sync"] {
                assert_eq!(observation[key]["state"], "succeeded");
                assert!(matches!(
                    observation[key]["method"].as_str().unwrap(),
                    "fsync" | "fsync_fullfsync" | "fsync_fullfsync_unsupported"
                ));
                if !observation[key]["native_code"].is_null() {
                    assert_eq!(observation[key]["method"], "fsync_fullfsync_unsupported");
                }
                // Only native identity and actual OS receipt observations vary.
                // Counts, fields, paths, metadata and status remain exact goldens.
                observation[key]["method"] = "fsync".into();
                observation[key]["native_code"] = serde_json::Value::Null;
            }
            if status == "created" {
                assert!(matches!(
                    observation["cleanup"]["state"].as_str().unwrap(),
                    "complete" | "not_needed"
                ));
                assert!(observation["cleanup"]["native_code"].is_null());
                observation["cleanup"]["state"] = "complete".into();
            }
            let expected = if status == "created" {
                include_str!("../../../tests/contract/protocol/created-attachment.jsonl")
            } else {
                include_str!("../../../tests/contract/protocol/up-to-date-attachment.jsonl")
            }
            .replace("@VERSION@", env!("CARGO_PKG_VERSION"));
            assert_eq!(normalized, values(expected.as_bytes()));
        }
        assert_eq!(
            fs::read(root.path().join("a.no-attachments.eml")).unwrap(),
            golden
        );
        assert_eq!(fs::read(root.path().join("a.eml")).unwrap(), input);
    }
    root.close().unwrap();
}

#[test]
fn actual_json_lines_keep_raw_unicode_separators_inside_strings() {
    let root = root();
    let name = "mail\u{85}\u{2028}.eml";
    let input = b"Content-Type: text/plain\r\n\r\nbody\r\n";
    fs::write(root.path().join(name), input).unwrap();
    let mut req = request(root.path(), RunMode::DryRun);
    req.paths = vec![name.into()];
    let mut sink = JsonSink::new(Vec::new());
    let outcome = run(&req, &RunControl::default(), &mut sink).unwrap();
    assert_eq!(outcome.exit_class(), ExitClass::Success);
    let bytes = sink.into_inner();
    assert!(bytes.windows(2).any(|part| part == [0xc2, 0x85]));
    assert!(bytes.windows(3).any(|part| part == [0xe2, 0x80, 0xa8]));
    assert_eq!(bytes.iter().filter(|byte| **byte == b'\n').count(), 3);
    let events = values(&bytes);
    assert_eq!(events.len(), 3);
    assert_eq!(events[1]["source"]["display"], name);
    assert_eq!(fs::read(root.path().join(name)).unwrap(), input);
    root.close().unwrap();
}
