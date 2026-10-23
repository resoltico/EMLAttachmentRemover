//! Independent handoff bytes and policy table, including opaque interiors.
use eml_core::{PreparationError, Refusal, RemovalReason, RunControl, prepare_message};
fn prepared(bytes: &[u8]) -> eml_core::PreparedMessage {
    prepare_message(bytes, &RunControl::default()).unwrap()
}
fn refused(bytes: &[u8], kind: Refusal, section: Option<&str>) {
    let error = prepare_message(bytes, &RunControl::default()).unwrap_err();
    match error {
        PreparationError::Refused {
            kind: actual,
            section: actual_section,
            detail,
        } => {
            assert_eq!(actual, kind);
            assert_eq!(actual_section.as_deref(), section);
            if kind == Refusal::UnsupportedMime {
                assert_eq!(
                    detail,
                    "multipart/report requires parts that the body-only policy would remove."
                );
                assert_eq!(error_code(kind), eml_core::ErrorCode::UnsupportedMime);
            }
        }
        _ => panic!("Unexpected interruption"),
    }
}
fn error_code(kind: Refusal) -> eml_core::ErrorCode {
    PreparationError::Refused {
        kind,
        section: None,
        detail: "fixture",
    }
    .code()
}
fn mixed(part: &str) -> Vec<u8> {
    format!("Content-Type: multipart/mixed; boundary=B\r\n\r\n--B\r\nContent-Type: text/plain\r\n\r\nbody\r\n--B\r\n{part}\r\n--B--\r\n").into_bytes()
}
#[test]
fn handoff_attachment_extension_and_inline_have_independent_exact_bytes() {
    let expected =
        include_bytes!("../../../tests/contract/mime/dispositions/expected-body-copy.eml");
    for bytes in [
        include_bytes!("../../../tests/contract/mime/dispositions/disposition_attachment.eml")
            .as_slice(),
        include_bytes!("../../../tests/contract/mime/dispositions/disposition_x-unknown.eml")
            .as_slice(),
    ] {
        let result = prepared(bytes);
        assert_eq!(result.as_bytes(), expected);
        assert_eq!(result.removed().len(), 1);
        let part = &result.removed()[0];
        assert_eq!(part.section, "2");
        assert_eq!(part.reason, RemovalReason::Attachment);
        assert_eq!(part.filename.as_deref(), Some("secret.txt"));
        assert_eq!(part.bytes, if bytes.len() == 229 { 107 } else { 106 });
        assert!(result.stripped_fields().is_empty());
    }
    let inline = include_bytes!("../../../tests/contract/mime/dispositions/disposition_inline.eml");
    assert_eq!(prepared(inline).as_bytes(), inline);
}
#[test]
fn extension_grammar_and_whole_container_opacity_do_not_interpret_interiors() {
    for token in [
        "x-unknown",
        "FoRm-DaTa",
        "(leading) X-UNKNOWN (tail)",
        "attachment",
    ] {
        for media in [
            "text/plain",
            "multipart/mixed",
            "multipart/report",
            "message/rfc822",
            "multipart/signed",
        ] {
            let wire = mixed(&format!(
                "Content-Type: {media}\r\nContent-Disposition: {token}\r\nContent-Transfer-Encoding: not-valid\r\n\r\n--invalid interior"
            ));
            let result = prepared(&wire);
            assert_eq!(result.removed().len(), 1);
            assert_eq!(result.removed()[0].reason, RemovalReason::Attachment);
            assert!(!result.as_bytes().windows(8).any(|b| b == b"interior"));
        }
    }
    for value in [
        "",
        " ",
        "bad token",
        "inline\r\nContent-Disposition: x-unknown",
    ] {
        refused(
            &mixed(&format!("Content-Disposition: {value}\r\n\r\ntext")),
            Refusal::Malformed,
            Some("2"),
        );
    }
    for value in ["inline", "inline; filename=part.bin"] {
        let wire = mixed(&format!("Content-Disposition: {value}\r\n\r\ntext"));
        assert_eq!(prepared(&wire).as_bytes(), wire);
    }
}
#[test]
fn physical_message_roots_keep_their_disposition() {
    for token in ["x-unknown", "attachment", "inline"] {
        let root = format!("Content-Disposition: {token}\r\n\r\nbody");
        assert_eq!(prepared(root.as_bytes()).as_bytes(), root.as_bytes());
        let forwarded = mixed(&format!("Content-Type: message/rfc822\r\n\r\n{root}"));
        assert_eq!(prepared(&forwarded).as_bytes(), forwarded);
    }
}
#[test]
fn retained_reports_refuse_in_each_body_context_and_opaque_reports_remove() {
    let report = include_bytes!("../../../tests/contract/mime/dispositions/report_valid_dsn.eml");
    refused(report, Refusal::UnsupportedMime, None);
    let report = std::str::from_utf8(report)
        .unwrap()
        .replace("boundary=B", "boundary=REPORT")
        .replace("--B", "--REPORT");
    let three_part = report.replace("--REPORT--", "--REPORT\r\nContent-Type: message/rfc822\r\n\r\nSubject: original\r\n\r\noriginal body\r\n--REPORT--");
    refused(three_part.as_bytes(), Refusal::UnsupportedMime, None);
    refused(&mixed(&three_part), Refusal::UnsupportedMime, Some("2"));
    refused(&mixed(&report), Refusal::UnsupportedMime, Some("2"));
    refused(
        &mixed(&format!("Content-Type: message/rfc822\r\n\r\n{report}")),
        Refusal::UnsupportedMime,
        Some("2"),
    );
    for start in ["", "; start=\"<chosen>\""] {
        let related = format!(
            "Content-Type: multipart/related; boundary=R{start}\r\n\r\n--R\r\nContent-ID: <chosen>\r\n{report}\r\n--R\r\nContent-Type: text/plain\r\n\r\nother\r\n--R--\r\n"
        );
        refused(related.as_bytes(), Refusal::UnsupportedMime, Some("1"));
    }
    for start in ["", "; start=\"<chosen>\""] {
        let related = format!(
            "Content-Type: multipart/related; boundary=R{start}\r\n\r\n--R\r\nContent-ID: <chosen>\r\n\r\nbody\r\n--R\r\nContent-Type: multipart/report\r\nContent-Disposition: x-unknown\r\n\r\nmalformed opaque\r\n--R--\r\n"
        );
        let result = prepared(related.as_bytes());
        assert_eq!(result.removed()[0].reason, RemovalReason::RelatedResource);
    }
    assert_eq!(
        eml_core::ErrorCode::UnsupportedMime.recovery_hint(),
        Some("This report cannot be converted to a body-only copy. Keep the original report.")
    );
}
