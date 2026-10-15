//! Display metadata does not grant MIME roles or choose operational paths.
use eml_core::{RemovalReason, RunControl, prepare_message};
fn metadata(parameters: &str) -> Option<String> {
    let wire = format!(
        "Content-Type: multipart/mixed; boundary=B\r\n\r\n--B\r\n\r\nbody\r\n--B\r\nContent-Type: application/pdf\r\nContent-Disposition: attachment; {parameters}\r\n\r\nx\r\n--B--\r\n"
    );
    let result = prepare_message(wire.as_bytes(), &RunControl::default()).unwrap();
    assert_eq!(result.removed().len(), 1);
    assert_eq!(result.removed()[0].reason, RemovalReason::Attachment);
    result.removed()[0].filename.clone()
}
#[test]
fn declared_display_charsets_and_encoded_words() {
    for (parameter, expected) in [
        ("filename*=UTF-8''%E2%82%AC.pdf", "€.pdf"),
        ("filename*=iso-8859-1''caf%E9.pdf", "café.pdf"),
        ("filename*=unknown''caf%C3%A9.pdf", "café.pdf"),
        ("filename=\"=?UTF-8?B?4oKsLnBkZg==?=\"", "€.pdf"),
        ("filename=\"=?ISO-8859-1?Q?caf=E9.pdf?=\"", "café.pdf"),
        ("filename=\"=?UTF-8?Q?a_b.pdf?=\"", "a b.pdf"),
        ("filename=\"=?UTF-8?B?broken!?=\"", "=?UTF-8?B?broken!?="),
    ] {
        assert_eq!(
            metadata(parameter).as_deref(),
            Some(expected),
            "{parameter}"
        );
    }
}
#[test]
fn display_forms_follow_first_occurrence_without_duplicate_fallback() {
    for (parameter, expected) in [
        ("filename=first; filename=second", Some("first")),
        ("filename=first; filename*=UTF-8''second", Some("first")),
        ("filename*=UTF-8''first; filename=second", Some("first")),
        (
            "filename*1=tail; filename=later; filename*0=head",
            Some("headtail"),
        ),
        (
            "filename*0=first; filename*0=other; filename*1=tail",
            Some("firsttail"),
        ),
        ("filename*=UTF-8''bad%XX; filename=valid", None),
        ("filename*1=gap; filename=valid", None),
        ("filename=\"a.pdf\"junk; filename=valid", None),
    ] {
        assert_eq!(metadata(parameter).as_deref(), expected, "{parameter}");
    }
}
#[test]
fn display_controls_and_scalar_limit() {
    assert_eq!(
        metadata("filename=\"a\0b\u{0085}c\u{007f}.pdf\""),
        Some("abc.pdf".into())
    );
    let symbols = "😀".repeat(256);
    let name = metadata(&format!("filename=\"{symbols}\"")).unwrap();
    assert_eq!(name, "😀".repeat(255));
    assert_eq!(
        metadata("filename=\"quote\\\"back\\\\slash.pdf\""),
        Some("quote\"back\\slash.pdf".into())
    );
}
#[test]
fn display_comments_and_malformed_encoded_prefixes_are_bounded() {
    assert_eq!(metadata("filename=a.pdf (tail)"), Some("a.pdf".into()));
    assert_eq!(metadata("filename=a(comment)b.pdf"), Some("a b.pdf".into()));
    let malformed = "=?a?B?x".repeat(16_000);
    let result = metadata(&format!("filename=\"{malformed}\"")).unwrap();
    assert_eq!(result, malformed.chars().take(255).collect::<String>());
}
#[test]
fn adjacent_encoded_words_suppress_only_interword_lws() {
    for (value, expected) in [
        ("=?UTF-8?B?YQ==?= =?UTF-8?B?Yg==?=", "ab"),
        ("=?UTF-8?B?YQ==?=  \t =?UTF-8?Q?b?=", "ab"),
        ("=?UTF-8?Q?a?=\r\n \t=?UTF-8?Q?b?=", "ab"),
        ("=?UTF-8?Q?a?= ordinary", "a ordinary"),
        ("ordinary =?UTF-8?Q?b?=", "ordinary b"),
        ("=?UTF-8?Q?a?= =?UTF-8?B?broken!?=", "a =?UTF-8?B?broken!?="),
    ] {
        assert_eq!(
            metadata(&format!("filename=\"{value}\"")).as_deref(),
            Some(expected),
            "{value}"
        );
    }
}
#[test]
fn invalid_encoded_word_syntax_remains_literal() {
    let overlong = format!("=?UTF-8?Q?{}?=", "a".repeat(64));
    assert!(overlong.len() > 75);
    for value in [
        "=?UTF-8?Q?this is text?=",
        "=?UTF-8?Q?a\tb?=",
        "=?UTF-8?Q?a?b?=",
        "=?UTF-8?Q?é?=",
        "=?UTF-8?Q??=",
        "=?UTF-8?Q?=XX?=",
        &overlong,
    ] {
        // Display sanitation removes HT even in an otherwise preserved invalid word.
        let expected = value.replace('\t', "");
        assert_eq!(
            metadata(&format!("filename=\"{value}\"")).as_deref(),
            Some(expected.as_str())
        );
    }
}
#[test]
fn encoded_word_exact_75_byte_boundary_and_empty_b_word() {
    let valid = format!("=?UTF-8?Q?{}?=", "a".repeat(63));
    assert_eq!(valid.len(), 75);
    assert_eq!(
        metadata(&format!("filename=\"{valid}\"")),
        Some("a".repeat(63))
    );
    assert_eq!(
        metadata("filename=\"=?UTF-8?B??=\""),
        Some("=?UTF-8?B??=".into())
    );
}
