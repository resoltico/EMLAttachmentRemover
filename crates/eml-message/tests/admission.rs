//! Message-root admission is distinct from MIME-part defaults and payload validation.
use eml_message::{PreparationError, Refusal, RunControl, prepare_message};

#[test]
fn non_messages_cannot_acquire_an_implicit_plain_text_root() {
    for wire in [
        b"".as_slice(),
        b"\r\n",
        b"\n\nbody",
        b"not an email at all\0\xff",
        b"plain text",
        b"not a header\r\nSubject: too late\r\n\r\nbody",
        b": empty name\r\n\r\nbody",
        b"Bad name: value\n\nbody",
        b"X\0Y: value\n\nbody",
        b" Subject: orphan\n\nbody",
        b"From owner\n\nbody",
        b"\xef\xbb\xbfSubject: BOM\n\nbody",
    ] {
        assert!(
            matches!(
                prepare_message(wire, &RunControl::default()),
                Err(PreparationError::Refused {
                    kind: Refusal::Malformed,
                    section: None,
                    ..
                })
            ),
            "{wire:?}"
        );
    }
}

#[test]
fn minimal_exports_are_exact_and_repeatable_without_full_rfc_validation() {
    for wire in [
        b"From: a@example.test\r\nDate: Fri, 6 Nov 2026 12:00:00 +0200\r\n\r\n".as_slice(),
        b"Subject:",
        b"Subject: only\n",
        b"Subject: folded\n continuation\n\ntext",
        b"X-Private: value\r\n\r\nbody",
        b"Content-Type: text/html\n\n<p>body</p>",
        b"From owner\rSubject: export\r\rbody",
        b"Subject: bytes\n\n\0\xff",
        "Subject: Sveiki\r\n\r\nLabdien, pasaule! 🌍".as_bytes(),
    ] {
        for _ in 0..2 {
            let prepared = prepare_message(wire, &RunControl::default()).unwrap();
            assert_eq!(prepared.as_bytes(), wire);
            assert!(prepared.removed().is_empty());
            assert!(prepared.stripped_fields().is_empty());
        }
    }
}

#[test]
fn embedded_roots_require_fields_but_removable_messages_stay_opaque() {
    let prefix = "Content-Type: multipart/mixed; boundary=B\r\n\r\n--B\r\n\r\nbody\r\n--B\r\nContent-Type: message/rfc822\r\n";
    let retained = format!("{prefix}\r\nnot an email\r\n--B--\r\n");
    assert!(
        matches!(prepare_message(retained.as_bytes(), &RunControl::default()),
        Err(PreparationError::Refused { kind: Refusal::Malformed, section: Some(section), .. }) if section == "2")
    );
    let attached =
        format!("{prefix}Content-Disposition: attachment\r\n\r\nnot an email\r\n--B--\r\n");
    let prepared = prepare_message(attached.as_bytes(), &RunControl::default()).unwrap();
    assert_eq!(
        prepared.as_bytes(),
        b"Content-Type: multipart/mixed; boundary=B\r\n\r\n--B\r\n\r\nbody\r\n--B--\r\n"
    );
    assert_eq!(prepared.removed().len(), 1);
    let valid = format!("{prefix}\r\nSubject: forwarded\r\n\r\nbody\r\n--B--\r\n");
    assert_eq!(
        prepare_message(valid.as_bytes(), &RunControl::default())
            .unwrap()
            .as_bytes(),
        valid.as_bytes()
    );
}
