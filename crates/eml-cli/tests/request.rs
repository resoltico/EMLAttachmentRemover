//! Hand-authored protocol shape and independent native-byte expectations.
#![cfg(any(unix, windows))]
use base64::{Engine, engine::general_purpose::STANDARD};
use eml_core::{
    MAX_INPUT_PATH_BYTES, MAX_INPUT_PATHS, MAX_PATH_COMPONENTS, MAX_PATH_UNITS, OutputLocation,
    PathViolation, RequestPath, RequestPathError, RunMode,
};
use remove_eml_attachments::{MAX_REQUEST_BYTES, RequestDecodeError, decode_request};
use std::path::PathBuf;

const EMPTY: &str = r#"{"protocol":1,"paths":[],"output_directory":null,"dry_run":false}"#;

fn root() -> PathBuf {
    PathBuf::from(if cfg!(windows) { r"C:\" } else { "/" })
}
fn host_encoding() -> &'static str {
    if cfg!(windows) {
        "windows-utf16le"
    } else {
        "unix-bytes"
    }
}
fn native(base64: &str) -> String {
    format!(
        r#"{{"encoding":"{}","base64":"{base64}"}}"#,
        host_encoding()
    )
}
fn document(path: &str) -> String {
    format!(r#"{{"protocol":1,"paths":[{path}],"output_directory":null,"dry_run":false}}"#)
}
fn invalid_document(bytes: &[u8]) {
    assert!(matches!(
        decode_request(bytes, root()),
        Err(RequestDecodeError::InvalidDocument { .. })
    ));
}
fn encode_ascii(text: &str) -> String {
    if cfg!(windows) {
        STANDARD.encode(text.bytes().flat_map(|byte| [byte, 0]).collect::<Vec<_>>())
    } else {
        STANDARD.encode(text.as_bytes())
    }
}

#[test]
fn hand_authored_request_preserves_native_paths_and_explicit_options() {
    #[cfg(unix)]
    let input = br#"{"protocol":1,"paths":[{"encoding":"unix-bytes","base64":"YS5lbWw="}],"output_directory":{"encoding":"unix-bytes","base64":"b3V0"},"dry_run":true}"#;
    #[cfg(windows)]
    let input = br#"{"protocol":1,"paths":[{"encoding":"windows-utf16le","base64":"YQAuAGUAbQBsAA=="}],"output_directory":{"encoding":"windows-utf16le","base64":"bwB1AHQA"},"dry_run":true}"#;
    let result = decode_request(input, root()).unwrap();
    assert_eq!(result.paths, [PathBuf::from("a.eml")]);
    assert_eq!(
        result.output,
        OutputLocation::Directory(PathBuf::from("out"))
    );
    assert_eq!(result.mode, RunMode::DryRun);
    assert_eq!(result.working_directory, root());
    let empty = decode_request(EMPTY.as_bytes(), root()).unwrap();
    assert!(empty.paths.is_empty());
    assert_eq!(empty.output, OutputLocation::BesideSource);
    assert_eq!(empty.mode, RunMode::Create);
}

#[test]
fn every_top_level_field_is_required_including_nullable_output() {
    for input in [
        r#"{"paths":[],"output_directory":null,"dry_run":false}"#,
        r#"{"protocol":1,"output_directory":null,"dry_run":false}"#,
        r#"{"protocol":1,"paths":[],"dry_run":false}"#,
        r#"{"protocol":1,"paths":[],"output_directory":null}"#,
    ] {
        invalid_document(input.as_bytes());
    }
    for field in [r#"{"encoding":"unix-bytes"}"#, r#"{"base64":""}"#, "{}"] {
        invalid_document(document(field).as_bytes());
    }
}

#[test]
fn duplicate_fields_are_rejected_at_both_object_levels() {
    for field in [
        r#""protocol":1"#,
        r#""paths":[]"#,
        r#""output_directory":null"#,
        r#""dry_run":false"#,
    ] {
        let input = format!("{},{}{}", &EMPTY[..EMPTY.len() - 1], field, "}");
        invalid_document(input.as_bytes());
    }
    for field in [r#""encoding":"unix-bytes""#, r#""base64":"""#] {
        let path = native("");
        invalid_document(
            document(&format!("{},{}{}", &path[..path.len() - 1], field, "}")).as_bytes(),
        );
    }
    // Distinct spellings after JSON unescaping are still the same field name.
    invalid_document(
        br#"{"protocol":1,"proto\u0063ol":1,"paths":[],"output_directory":null,"dry_run":false}"#,
    );
}

#[test]
fn unknown_fields_and_unknown_encodings_are_rejected() {
    invalid_document(
        EMPTY
            .replace("\"protocol\":1", "\"protocol\":1,\"other\":0")
            .as_bytes(),
    );
    invalid_document(document(r#"{"encoding":"unix-bytes","base64":"","other":0}"#).as_bytes());
    invalid_document(document(r#"{"encoding":"utf8","base64":""}"#).as_bytes());
    invalid_document(EMPTY.replace("\"dry_run\"", "\"Dry_Run\"").as_bytes());
}

#[test]
fn positional_arrays_cannot_replace_required_objects() {
    invalid_document(b"[1,[],null,false]");
    let path = format!(r#"["{}",""]"#, host_encoding());
    invalid_document(document(&path).as_bytes());
    invalid_document(
        EMPTY
            .replace(
                "\"output_directory\":null",
                &format!("\"output_directory\":{path}"),
            )
            .as_bytes(),
    );
}

#[test]
fn native_encoding_is_a_string_not_a_tagged_enum_object() {
    for encoding in [
        r#"{"unix-bytes":null}"#,
        r#"{"windows-utf16le":null}"#,
        r#"["unix-bytes"]"#,
        r#"["windows-utf16le"]"#,
    ] {
        let path = format!(r#"{{"encoding":{encoding},"base64":""}}"#);
        invalid_document(document(&path).as_bytes());
        invalid_document(
            EMPTY
                .replace(
                    "\"output_directory\":null",
                    &format!("\"output_directory\":{path}"),
                )
                .as_bytes(),
        );
    }
}

#[test]
fn field_types_and_integer_protocol_are_strict() {
    for value in ["null", "true", "[]", "{}", "\"1\"", "1.0", "1e0", "-1"] {
        invalid_document(
            EMPTY
                .replace("\"protocol\":1", &format!("\"protocol\":{value}"))
                .as_bytes(),
        );
    }
    for value in ["null", "true", "{}", "\"path\"", "1"] {
        invalid_document(
            EMPTY
                .replace("\"paths\":[]", &format!("\"paths\":{value}"))
                .as_bytes(),
        );
    }
    for value in ["null", "0", "\"false\"", "[]", "{}"] {
        invalid_document(
            EMPTY
                .replace("\"dry_run\":false", &format!("\"dry_run\":{value}"))
                .as_bytes(),
        );
    }
    for value in ["false", "0", "\"out\"", "[]"] {
        invalid_document(
            EMPTY
                .replace(
                    "\"output_directory\":null",
                    &format!("\"output_directory\":{value}"),
                )
                .as_bytes(),
        );
    }
    invalid_document(document("null").as_bytes());
    invalid_document(document(r#"{"encoding":1,"base64":""}"#).as_bytes());
    invalid_document(document(r#"{"encoding":"unix-bytes","base64":null}"#).as_bytes());
    for input in ["[]", "null", "1", "true", "\"request\""] {
        invalid_document(input.as_bytes());
    }
}

#[test]
fn protocol_mismatch_is_not_a_compatibility_decoder() {
    for number in [0, 2, 3, u64::MAX] {
        let input = EMPTY.replace("\"protocol\":1", &format!("\"protocol\":{number}"));
        assert_eq!(
            decode_request(input.as_bytes(), root()),
            Err(RequestDecodeError::UnsupportedProtocol)
        );
    }
}

#[test]
fn base64_requires_canonical_padding_alphabet_and_trailing_bits() {
    for encoded in [
        "Zg", "Zg=", "Zg===", "Zh==", "Zm9=", "-w==", "_w==", "!!!!", "Zg==\\n", " Zg==", "Zg== ",
    ] {
        assert_eq!(
            decode_request(document(&native(encoded)).as_bytes(), root()),
            Err(RequestDecodeError::InvalidBase64 {
                path: RequestPath::Input(0)
            }),
            "{encoded}"
        );
    }
    // Canonical host encoding of 'f', and the empty native spelling, are valid.
    let input = document(&native(&encode_ascii("f")));
    assert_eq!(
        decode_request(input.as_bytes(), root()).unwrap().paths,
        [PathBuf::from("f")]
    );
    assert_eq!(
        decode_request(document(&native("")).as_bytes(), root())
            .unwrap()
            .paths,
        [PathBuf::new()]
    );
}

#[test]
fn native_encoding_must_match_host_for_inputs_and_output() {
    let wrong = if cfg!(windows) {
        "unix-bytes"
    } else {
        "windows-utf16le"
    };
    let path = format!(r#"{{"encoding":"{wrong}","base64":""}}"#);
    assert_eq!(
        decode_request(document(&path).as_bytes(), root()),
        Err(RequestDecodeError::WrongEncoding {
            path: RequestPath::Input(0)
        })
    );
    let input = EMPTY.replace(
        "\"output_directory\":null",
        &format!("\"output_directory\":{path}"),
    );
    assert_eq!(
        decode_request(input.as_bytes(), root()),
        Err(RequestDecodeError::WrongEncoding {
            path: RequestPath::OutputDirectory
        })
    );
}

#[test]
fn exact_document_byte_bound_includes_whitespace_and_precedes_parse() {
    let mut input = EMPTY.as_bytes().to_vec();
    input.resize(MAX_REQUEST_BYTES, b' ');
    assert!(decode_request(&input, root()).is_ok());
    input.push(b' ');
    assert_eq!(
        decode_request(&input, root()),
        Err(RequestDecodeError::TooLarge)
    );
    // Invalid oversized bytes must fail on length, not leak parser information.
    input.fill(0xff);
    assert_eq!(
        decode_request(&input, root()),
        Err(RequestDecodeError::TooLarge)
    );
}

#[test]
fn malformed_utf8_trailing_data_and_excessive_nesting_fail() {
    for bytes in [&b""[..], b"{", b"\xff", b"\xef\xbb\xbf{}", b"{}\0"] {
        invalid_document(bytes);
    }
    invalid_document(format!("{EMPTY}{EMPTY}").as_bytes());
    invalid_document(format!("{EMPTY}x").as_bytes());
    invalid_document(document(&format!("{}{}", "[".repeat(256), "]".repeat(256))).as_bytes());
    assert!(decode_request(format!(" \n\t{EMPTY}\r\n ").as_bytes(), root()).is_ok());
}

#[test]
fn decoded_values_are_subject_to_core_limits_before_use() {
    let empty_path = native("");
    let paths = vec![empty_path; MAX_INPUT_PATHS].join(",");
    let input = EMPTY.replace("\"paths\":[]", &format!("\"paths\":[{paths}]"));
    assert_eq!(
        decode_request(input.as_bytes(), root())
            .unwrap()
            .paths
            .len(),
        MAX_INPUT_PATHS
    );
    let input = input.replace("]", &format!(",{}]", native("")));
    assert_eq!(
        decode_request(input.as_bytes(), root()),
        Err(RequestDecodeError::InvalidRequest(
            RequestPathError::TooManyInputs
        ))
    );
    let input = document(&native(&encode_ascii("a\0b")));
    assert_eq!(
        decode_request(input.as_bytes(), root()),
        Err(RequestDecodeError::InvalidRequest(
            RequestPathError::InvalidPath {
                path: RequestPath::Input(0),
                violation: PathViolation::EmbeddedNul
            }
        ))
    );
    let input = document(&native(&encode_ascii(
        &vec!["x"; MAX_PATH_COMPONENTS + 1].join("/"),
    )));
    assert_eq!(
        decode_request(input.as_bytes(), root()),
        Err(RequestDecodeError::InvalidRequest(
            RequestPathError::InvalidPath {
                path: RequestPath::Input(0),
                violation: PathViolation::TooManyComponents
            }
        ))
    );
    assert_eq!(
        decode_request(EMPTY.as_bytes(), PathBuf::from("relative")),
        Err(RequestDecodeError::InvalidRequest(
            RequestPathError::WorkingDirectoryNotAbsolute
        ))
    );
}

#[test]
fn native_unit_and_aggregate_bytes_bounds_survive_transport() {
    let path = native(&encode_ascii(&"a".repeat(MAX_PATH_UNITS)));
    assert!(decode_request(document(&path).as_bytes(), root()).is_ok());
    for extra in [1, 4] {
        let input = document(&native(&encode_ascii(&"a".repeat(MAX_PATH_UNITS + extra))));
        assert_eq!(
            decode_request(input.as_bytes(), root()),
            Err(RequestDecodeError::InvalidRequest(
                RequestPathError::InvalidPath {
                    path: RequestPath::Input(0),
                    violation: PathViolation::TooManyUnits
                }
            ))
        );
    }
    let bytes_per_unit = if cfg!(windows) { 2 } else { 1 };
    let count = MAX_INPUT_PATH_BYTES / (MAX_PATH_UNITS * bytes_per_unit);
    let paths = vec![path; count].join(",");
    let input = EMPTY.replace("\"paths\":[]", &format!("\"paths\":[{paths}]"));
    assert!(input.len() < MAX_REQUEST_BYTES);
    assert!(decode_request(input.as_bytes(), root()).is_ok());
    let input = input.replace("]", &format!(",{}]", native(&encode_ascii("z"))));
    assert_eq!(
        decode_request(input.as_bytes(), root()),
        Err(RequestDecodeError::InvalidRequest(
            RequestPathError::InputPathsTooLarge
        ))
    );
}

#[test]
fn diagnostics_are_bounded_and_never_echo_request_content() {
    let marker = "private-request-marker".repeat(10000);
    let input = EMPTY.replace("\"protocol\":1", &format!("\"protocol\":1,\"{marker}\":0"));
    let error = decode_request(input.as_bytes(), root()).unwrap_err();
    for text in [error.to_string(), format!("{error:?}")] {
        assert!(text.len() < 200);
        assert!(!text.contains("private-request-marker"));
    }
}

#[cfg(unix)]
#[test]
fn unix_nonunicode_control_bytes_and_symlink_parent_spelling_are_lossless() {
    use std::os::unix::ffi::OsStrExt;
    let input = br#"{"protocol":1,"paths":[{"encoding":"unix-bytes","base64":"/w=="}],"output_directory":null,"dry_run":false}"#;
    assert_eq!(
        decode_request(input, root()).unwrap().paths[0]
            .as_os_str()
            .as_bytes(),
        b"\xff"
    );
    let bytes = b"links/alias/../a\n\x1b\xe2\x80\xa8\xc2\x85.eml";
    let input = document(&native(&STANDARD.encode(bytes)));
    assert_eq!(
        decode_request(input.as_bytes(), root()).unwrap().paths[0]
            .as_os_str()
            .as_bytes(),
        bytes
    );
}

#[cfg(windows)]
#[test]
fn windows_unpaired_surrogates_are_lossless_and_odd_bytes_are_rejected() {
    use std::os::windows::ffi::OsStrExt;
    let input = br#"{"protocol":1,"paths":[{"encoding":"windows-utf16le","base64":"ANg="}],"output_directory":null,"dry_run":false}"#;
    assert_eq!(
        decode_request(input, root()).unwrap().paths[0]
            .as_os_str()
            .encode_wide()
            .collect::<Vec<_>>(),
        [0xd800]
    );
    assert_eq!(
        decode_request(document(&native("Zg==")).as_bytes(), root()),
        Err(RequestDecodeError::OddUtf16Bytes {
            path: RequestPath::Input(0)
        })
    );
    let input = EMPTY.replace(
        "\"output_directory\":null",
        &format!("\"output_directory\":{}", native("Zg==")),
    );
    assert_eq!(
        decode_request(input.as_bytes(), root()),
        Err(RequestDecodeError::OddUtf16Bytes {
            path: RequestPath::OutputDirectory
        })
    );
}
