//! Independent synthetic golden and MIME policy contracts.
use eml_message::{
    PreparationError, Refusal, RemovalReason as Reason, RemovedPart, RunControl, prepare_message,
};
macro_rules! fixture {
    ($folder:literal, $name:literal) => {
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/contract/mime/",
            $folder,
            "/",
            $name,
            ".eml"
        ))
        .as_slice()
    };
}
fn removed(
    section: &str,
    media: &str,
    filename: Option<&str>,
    bytes: u64,
    reason: Reason,
) -> RemovedPart {
    RemovedPart {
        section: section.into(),
        content_type: media.into(),
        filename: filename.map(str::to_owned),
        bytes,
        reason,
    }
}
fn check(input: &[u8], expected: &[u8], removals: Vec<RemovedPart>) {
    let result = prepare_message(input, &RunControl::default()).unwrap();
    assert_eq!(result.as_bytes(), expected);
    assert_eq!(result.removed(), removals);
    assert!(result.stripped_fields().is_empty());
    let again = prepare_message(result.as_bytes(), &RunControl::default()).unwrap();
    assert_eq!(again.as_bytes(), expected);
    assert!(again.removed().is_empty());
    assert!(again.stripped_fields().is_empty());
}
#[test]
fn gmail_attachment() {
    check(
        fixture!("inputs", "01-gmail-attachment"),
        fixture!("golden", "01-gmail-attachment"),
        vec![removed(
            "2",
            "application/pdf",
            Some("report.pdf"),
            507,
            Reason::Attachment,
        )],
    );
}
#[test]
fn outlook_related_image() {
    check(
        fixture!("inputs", "02-outlook-related-image"),
        fixture!("golden", "02-outlook-related-image"),
        vec![removed(
            "2",
            "image/png",
            Some("image001.png"),
            351,
            Reason::RelatedResource,
        )],
    );
}
#[test]
fn apple_mail_both_html_bodies() {
    check(
        fixture!("inputs", "03-apple-mail-inline-image"),
        fixture!("golden", "03-apple-mail-inline-image"),
        vec![removed(
            "2.2",
            "image/jpeg",
            Some("IMG_0001.jpeg"),
            332,
            Reason::NonBodyPart,
        )],
    );
}
#[test]
fn calendar_invite() {
    check(
        fixture!("inputs", "04-calendar-invite"),
        fixture!("golden", "04-calendar-invite"),
        vec![
            removed("1.3", "text/calendar", None, 141, Reason::NonBodyPart),
            removed(
                "2",
                "application/ics",
                Some("invite.ics"),
                187,
                Reason::Attachment,
            ),
        ],
    );
}
#[test]
fn forwarded_inline_message() {
    check(
        fixture!("inputs", "05-forwarded-inline-message"),
        fixture!("golden", "05-forwarded-inline-message"),
        vec![removed(
            "2.2",
            "application/pdf",
            Some("report.pdf"),
            507,
            Reason::Attachment,
        )],
    );
}
#[test]
fn smime_signed_refuses_at_root() {
    assert!(matches!(
        prepare_message(
            fixture!("inputs", "06-smime-signed"),
            &RunControl::default()
        ),
        Err(PreparationError::Refused {
            kind: Refusal::Protected,
            section: None,
            ..
        })
    ));
}
#[test]
fn sloppy_qp_body_is_not_regenerated() {
    check(
        fixture!("inputs", "07-sloppy-quoted-printable"),
        fixture!("golden", "07-sloppy-quoted-printable"),
        vec![removed(
            "2",
            "application/pdf",
            Some("report.pdf"),
            507,
            Reason::Attachment,
        )],
    );
}
#[test]
fn undeclared_eightbit_body_is_not_regenerated() {
    check(
        fixture!("inputs", "08-eightbit-without-cte"),
        fixture!("golden", "08-eightbit-without-cte"),
        vec![removed(
            "2",
            "application/pdf",
            Some("report.pdf"),
            507,
            Reason::Attachment,
        )],
    );
}
#[test]
fn attachment_only_has_no_body() {
    assert!(matches!(
        prepare_message(
            fixture!("inputs", "09-attachment-only"),
            &RunControl::default()
        ),
        Err(PreparationError::Refused {
            kind: Refusal::NoBody,
            ..
        })
    ));
}
#[test]
fn named_text_without_attachment_disposition_is_body() {
    check(
        fixture!("inputs", "10-text-file-without-disposition"),
        fixture!("golden", "10-text-file-without-disposition"),
        vec![],
    );
}
#[test]
fn nested_all_attachment_container_collapses() {
    check(
        fixture!("inputs", "11-all-attachment-nested-container"),
        fixture!("golden", "11-all-attachment-nested-container"),
        vec![
            removed(
                "2.1",
                "application/pdf",
                Some("report.pdf"),
                507,
                Reason::Attachment,
            ),
            removed("2.2", "image/png", Some("a.png"), 319, Reason::Attachment),
        ],
    );
}
#[test]
fn winmail_attachment() {
    check(
        fixture!("inputs", "12-winmail-dat"),
        fixture!("golden", "12-winmail-dat"),
        vec![removed(
            "2",
            "application/ms-tnef",
            Some("winmail.dat"),
            157,
            Reason::Attachment,
        )],
    );
}
#[test]
fn transport_variants_keep_exact_original_terminators() {
    let input = fixture!("inputs", "01-gmail-attachment");
    let golden = fixture!("golden", "01-gmail-attachment");
    for ending in [b"\n".as_slice(), b"\r".as_slice(), b"\r\n".as_slice()] {
        let convert = |bytes: &[u8]| {
            let mut out = Vec::new();
            let mut i = 0;
            while i < bytes.len() {
                if bytes.get(i..i + 2) == Some(b"\r\n") {
                    out.extend_from_slice(ending);
                    i += 2;
                } else {
                    out.push(bytes[i]);
                    i += 1;
                }
            }
            out
        };
        check(
            &convert(input),
            &convert(golden),
            vec![removed(
                "2",
                "application/pdf",
                Some("report.pdf"),
                if ending.len() == 2 { 507 } else { 503 },
                Reason::Attachment,
            )],
        );
    }
}
fn refusal(input: &[u8], expected: Refusal) {
    assert!(
        matches!(prepare_message(input,&RunControl::default()),Err(PreparationError::Refused{kind,..}) if kind==expected),
        "{expected:?}: {:?}",
        prepare_message(input, &RunControl::default())
    );
}
#[test]
fn empty_and_root_text_are_structural_bodies() {
    for input in [
        b"".as_slice(),
        b"\r\n",
        b"Subject: fields only\r\n",
        b"Content-Type: text/html\r\n\r\n",
        b"Content-Type: text/plain\r\nContent-Disposition: attachment\r\n\r\nbody",
        b"Meeting at 10:00\rfinal",
        b"From owner\rSubject: test\r\rbody",
        b"body\0bytes\xff",
    ] {
        check(input, input, vec![]);
    }
}
#[test]
fn all_protected_types_refuse_and_attachment_opacity_wins() {
    for media in [
        "multipart/signed",
        "multipart/encrypted",
        "application/pkcs7-mime",
        "application/x-pkcs7-mime",
        "application/pkcs7-signature",
        "application/x-pkcs7-signature",
        "application/pgp-encrypted",
        "application/pgp-signature",
    ] {
        refusal(
            format!("Content-Type: {media}\r\n\r\nnot parsed").as_bytes(),
            Refusal::Protected,
        );
        let input = format!(
            "Content-Type: multipart/mixed; boundary=B\r\n\r\n--B\r\n\r\nbody\r\n--B\r\nContent-Type: {media}\r\nContent-Disposition: attachment\r\n\r\ninvalid interior\r\n--B--\r\n"
        );
        let result = prepare_message(input.as_bytes(), &RunControl::default()).unwrap();
        assert_eq!(result.removed().len(), 1);
        assert_eq!(result.removed()[0].reason, Reason::Attachment);
    }
}
#[test]
fn malformed_control_headers_refuse_without_headerless_fallback() {
    for input in [
        b" orphan".as_slice(),
        b"Subject: starts\r\nnot a field\r\n\r\nbody",
        b"Subject: starts\r\nContent-Type : text/plain\r\n\r\nbody",
        b"Content-Type: text/plain\r\nContent-Type: text/plain\r\n\r\nbody",
        b"Content-Disposition: attachment (unterminated\r\n\r\nbody",
        b"Content-Type: text/plain junk\r\n\r\nbody",
        b"Content-Disposition: attachment (comment)junk\r\n\r\nbody",
    ] {
        refusal(input, Refusal::Malformed);
    }
}
#[test]
fn opaque_attachment_interior_is_not_parsed() {
    let input=b"Content-Type: multipart/mixed; boundary=B\r\n\r\n--B\r\n\r\nbody\r\n--B\r\nContent-Type: multipart/mixed\r\nContent-Disposition: attachment; filename=\"a.pdf\"junk\r\n\r\nmalformed interior\r\n--B--\r\n";
    let result = prepare_message(input, &RunControl::default()).unwrap();
    assert_eq!(result.removed().len(), 1);
    assert_eq!(result.removed()[0].filename, None);
}
#[test]
fn related_nonfirst_root_and_irrelevant_ids() {
    let input=b"Content-Type: multipart/related; boundary=B; start=\"(lead) <root@x> (tail)\"; type=\"wrong/advisory\"\r\n\r\n--B\r\nContent-Type: multipart/signed\r\nContent-ID: <resource>\r\n\r\nopaque protected resource\r\n--B\r\nContent-Type: text/html\r\nContent-ID: <root@x>\r\n\r\n<p>body</p>\r\n--B--\r\n";
    let result = prepare_message(input, &RunControl::default()).unwrap();
    assert_eq!(result.removed()[0].section, "1");
    assert_eq!(result.removed()[0].reason, Reason::RelatedResource);
    let no_start=b"Content-Type: multipart/related; boundary=B\r\n\r\n--B\r\nContent-ID: broken irrelevant\r\n\r\nbody\r\n--B\r\nContent-Type: image/png\r\nContent-ID: broken duplicate\r\nContent-ID: irrelevant\r\n\r\nresource\r\n--B--\r\n";
    assert!(prepare_message(no_start, &RunControl::default()).is_ok());
}
#[test]
fn changed_root_and_container_stale_fields_are_complete_deletions() {
    let input=b"From owner\r\nSubject: test\r\nContent-Type: multipart/mixed; boundary=B\r\nContent-Length: 999\r\n continuation\r\nLines: 50\r\nContent-MD5: stale\r\nDKIM-Signature: stale\r\nDomainKey-Signature: stale\r\nARC-Seal: stale\r\nARC-Message-Signature: stale\r\nARC-Authentication-Results: stale\r\nX-MS-Has-Attach: yes\r\n\r\npreamble\r\n--B\r\nContent-Length: keep unchanged leaf\r\n\r\nbody\r\n--B\r\nContent-Type: image/png\r\n\r\nimage\r\n--B--\r\nepilogue";
    let expected=b"From owner\r\nSubject: test\r\nContent-Type: multipart/mixed; boundary=B\r\n\r\npreamble\r\n--B\r\nContent-Length: keep unchanged leaf\r\n\r\nbody\r\n--B--\r\nepilogue";
    let result = prepare_message(input, &RunControl::default()).unwrap();
    assert_eq!(result.as_bytes(), expected);
    assert_eq!(
        result.stripped_fields(),
        [
            "arc-authentication-results",
            "arc-message-signature",
            "arc-seal",
            "content-length",
            "content-md5",
            "dkim-signature",
            "domainkey-signature",
            "lines",
            "x-ms-has-attach"
        ]
    );
}
#[test]
fn cooperative_preparation_control_is_per_run() {
    let cancelled = RunControl::default();
    cancelled.request_stop();
    assert_eq!(
        prepare_message(b"body", &cancelled).unwrap_err(),
        PreparationError::Interrupted
    );
    assert!(prepare_message(b"body", &RunControl::default()).is_ok());
}
