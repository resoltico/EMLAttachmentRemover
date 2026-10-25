//! Context precedence, opaque descendants and exact structural boundaries.
use eml_message::{PreparationError, Refusal, RemovalReason, RunControl, prepare_message};
fn refusal(wire: &[u8], kind: Refusal) {
    assert!(
        matches!(prepare_message(wire,&RunControl::default()),Err(PreparationError::Refused{kind:actual,..}) if actual==kind)
    );
}
fn mixed(parts: &[&[u8]]) -> Vec<u8> {
    let mut wire = b"Content-Type: multipart/mixed; boundary=B\r\n\r\n".to_vec();
    for part in parts {
        wire.extend_from_slice(b"--B\r\n");
        wire.extend_from_slice(part);
        wire.extend_from_slice(b"\r\n");
    }
    wire.extend_from_slice(b"--B--\r\n");
    wire
}
#[test]
fn physical_header_names_use_ftext_and_the_first_colon() {
    for byte in (0x21_u8..=0x7e).filter(|byte| *byte != b':') {
        let mut valid = vec![byte];
        valid.extend_from_slice(b": value:with:colons\r\n\r\nbody");
        assert_eq!(
            prepare_message(&valid, &RunControl::default())
                .unwrap()
                .as_bytes(),
            valid
        );
        let mut wire = vec![byte];
        wire.extend_from_slice(b": value:with:colons\r\nnot a field\r\n");
        refusal(&wire, Refusal::Malformed);
    }
    for byte in [0, b' ', b'\t', 0x7f, 0x80, 0xff] {
        let mut wire = vec![b'X', byte, b'Y'];
        wire.extend_from_slice(b": value\r\nnot a field\r\n");
        let prepared = prepare_message(&wire, &RunControl::default()).unwrap();
        assert_eq!(prepared.as_bytes(), wire);
    }
    refusal(b"X:Y: still a value\r\nnot a field\r\n", Refusal::Malformed);
    let wire = b":not a field\r\nContent-Type: multipart/signed\r\n";
    assert_eq!(
        prepare_message(wire, &RunControl::default())
            .unwrap()
            .as_bytes(),
        wire
    );
}
#[test]
fn unix_from_is_only_an_outer_envelope_with_a_following_field() {
    refusal(
        b"From owner\r\nSubject: valid field\r\nnot a field\r\n",
        Refusal::Malformed,
    );
    for wire in [
        b"From owner\r\nnot a field\r\nContent-Type: multipart/signed\r\n".as_slice(),
        b"From owner\r\n Subject: not a field\r\n",
    ] {
        assert_eq!(
            prepare_message(wire, &RunControl::default())
                .unwrap()
                .as_bytes(),
            wire
        );
    }
    let body = b"From owner\r\nContent-Type: multipart/signed\r\n\r\nordinary text";
    let part = mixed(&[body]);
    assert_eq!(
        prepare_message(&part, &RunControl::default())
            .unwrap()
            .as_bytes(),
        part
    );
    let mut forward = b"Content-Type: message/rfc822\r\n\r\n".to_vec();
    forward.extend_from_slice(body);
    let wire = mixed(&[&forward]);
    assert_eq!(
        prepare_message(&wire, &RunControl::default())
            .unwrap()
            .as_bytes(),
        wire
    );
}
#[test]
fn nested_boundary_prefixes_are_distinct_but_reuse_and_premature_outer_edges_refuse() {
    for (outer, inner) in [("B", "BX"), ("BX", "B")] {
        let wire = format!(
            "Content-Type: multipart/mixed; boundary={outer}\r\n\r\n--{outer}\r\nContent-Type: multipart/mixed; boundary={inner}\r\n\r\n--{inner}\r\n\r\nbody\r\n--{inner}--\r\n--{outer}--\r\n"
        );
        assert_eq!(
            prepare_message(wire.as_bytes(), &RunControl::default())
                .unwrap()
                .as_bytes(),
            wire.as_bytes()
        );
    }
    refusal(b"Content-Type: multipart/mixed; boundary=B\r\n\r\n--B\r\nContent-Type: multipart/mixed; boundary=B\r\n\r\n--B\r\n\r\nbody\r\n--B--\r\n--B--\r\n", Refusal::Malformed);
    refusal(b"Content-Type: multipart/mixed; boundary=B\r\n\r\n--B\r\nContent-Type: multipart/mixed; boundary=I\r\n\r\n--I\r\n\r\nbody\r\n--B\r\n\r\nouter sibling\r\n--I--\r\n--B--\r\n", Refusal::Malformed);
}
#[test]
fn duplicate_transfer_encoding_is_structural_only_when_descent_requires_it() {
    let encodings = "Content-Transfer-Encoding: 7bit\r\nContent-Transfer-Encoding: 7bit\r\n";
    for wire in [
        format!(
            "Content-Type: multipart/mixed; boundary=B\r\n{encodings}\r\n--B\r\n\r\nbody\r\n--B--\r\n"
        ),
        format!("Content-Type: message/rfc822\r\n{encodings}\r\nSubject: forward\r\n\r\nbody"),
    ] {
        refusal(wire.as_bytes(), Refusal::Malformed);
    }
    let text = format!("Content-Type: text/plain\r\n{encodings}\r\nbody");
    assert_eq!(
        prepare_message(text.as_bytes(), &RunControl::default())
            .unwrap()
            .as_bytes(),
        text.as_bytes()
    );
    let attachment = format!(
        "Content-Disposition: attachment\r\nContent-Type: multipart/mixed\r\n{encodings}\r\nunparsed interior"
    );
    let wire = mixed(&[b"\r\nbody", attachment.as_bytes()]);
    let prepared = prepare_message(&wire, &RunControl::default()).unwrap();
    assert_eq!(prepared.as_bytes(), mixed(&[b"\r\nbody"]));
    assert_eq!(prepared.removed().len(), 1);
    assert_eq!(prepared.removed()[0].reason, RemovalReason::Attachment);
}
#[test]
fn forwarded_protected_roots_never_bypass_parent_policy() {
    for media in [
        "multipart/signed",
        "multipart/encrypted",
        "application/pkcs7-mime",
    ] {
        let forward =
            format!("Content-Type: message/rfc822\r\n\r\nContent-Type: {media}\r\n\r\nopaque");
        let source = mixed(&[b"\r\nouter text", forward.as_bytes()]);
        assert!(
            matches!(prepare_message(&source,&RunControl::default()),Err(PreparationError::Refused{kind:Refusal::Protected,section:Some(section),..}) if section=="2" || section=="2.1")
        );
    }
}
#[test]
fn forwarded_related_honors_its_own_nonfirst_root() {
    let forward=b"Content-Type: message/global\r\n\r\nContent-Type: multipart/related; boundary=R; start=\"<root>\"\r\n\r\n--R\r\nContent-Type: image/png\r\nContent-ID: <resource>\r\n\r\nresource\r\n--R\r\nContent-Type: text/html\r\nContent-ID: <root>\r\n\r\n<p>body</p>\r\n--R--";
    let result = prepare_message(
        &mixed(&[b"\r\nouter text", forward]),
        &RunControl::default(),
    )
    .unwrap();
    assert_eq!(result.removed()[0].section, "2.1");
    assert_eq!(result.removed()[0].reason, RemovalReason::RelatedResource);
    assert!(result.as_bytes().windows(11).any(|s| s == b"<p>body</p>"));
}
#[test]
fn forwarded_nonbody_root_collapses_and_keeps_source_section() {
    let forward = b"Content-Type: message/rfc822\r\n\r\nContent-Type: image/png\r\n\r\nimage";
    let result = prepare_message(
        &mixed(&[b"\r\nouter text", forward]),
        &RunControl::default(),
    )
    .unwrap();
    assert_eq!(result.removed()[0].section, "2.1");
    assert_eq!(result.removed()[0].reason, RemovalReason::NonBodyPart);
    assert_eq!(result.as_bytes(), mixed(&[b"\r\nouter text"]));
}
#[test]
fn nonidentity_messages_are_opaque_and_digest_defaults_reset() {
    for encoding in ["base64", "quoted-printable", "unknown"] {
        let forward = format!(
            "Content-Type: message/rfc822\r\nContent-Transfer-Encoding: {encoding}\r\n\r\nContent-Type: multipart/signed\r\n\r\nopaque"
        );
        let result = prepare_message(
            &mixed(&[b"\r\ntext", forward.as_bytes()]),
            &RunControl::default(),
        )
        .unwrap();
        assert_eq!(result.removed()[0].reason, RemovalReason::NonBodyPart);
        refusal(forward.as_bytes(), Refusal::NoBody);
    }
    let digest=b"Content-Type: multipart/digest; boundary=D\r\n\r\n--D\r\n\r\nSubject: forwarded\r\n\r\ntext\r\n--D--\r\n";
    let result = prepare_message(digest, &RunControl::default()).unwrap();
    assert_eq!(result.as_bytes(), digest);
    assert!(result.removed().is_empty());
}
#[test]
fn generic_multipart_subtypes_recurse_in_source_order() {
    for subtype in ["parallel", "x-unknown"] {
        let wire = format!(
            "Content-Type: multipart/{subtype}; boundary=R\r\n\r\n--R\r\n\r\ntext\r\n--R\r\nContent-Type: message/delivery-status\r\n\r\nstatus\r\n--R\r\nContent-Type: message/rfc822\r\n\r\nSubject: forwarded\r\n\r\nforwarded body\r\n--R--\r\n"
        );
        let result = prepare_message(wire.as_bytes(), &RunControl::default()).unwrap();
        assert_eq!(result.removed().len(), 1);
        assert_eq!(result.removed()[0].content_type, "message/delivery-status");
        assert_eq!(result.removed()[0].section, "2");
    }
}
#[test]
fn related_invalid_selection_and_selected_attachment() {
    for children in [
        "--R\r\nContent-ID: <other>\r\n\r\nbody\r\n",
        "--R\r\nContent-ID: <root>\r\nContent-ID: <root>\r\n\r\nbody\r\n",
        "--R\r\nContent-ID: <root>\r\n\r\na\r\n--R\r\nContent-ID: <root>\r\n\r\nb\r\n",
    ] {
        refusal(format!("Content-Type: multipart/related; boundary=R; start=\"<root>\"\r\n\r\n{children}--R--\r\n").as_bytes(),Refusal::Malformed);
    }
    refusal(b"Content-Type: multipart/related; boundary=R\r\n\r\n--R\r\nContent-Disposition: attachment\r\n\r\nbody\r\n--R--\r\n",Refusal::NoBody);
}
#[test]
fn structural_parameter_forms_and_malformed_values() {
    for parameter in [
        "boundary*=UTF-8''B",
        "boundary*0*=UTF-8''; boundary*1*=B",
        "boundary*0=B; boundary*1=",
        "boundary=\"B\"",
    ] {
        let wire = format!(
            "Content-Type: multipart/mixed; {parameter}\r\n\r\n--B\r\n\r\nbody\r\n--B--\r\n"
        );
        let result = prepare_message(wire.as_bytes(), &RunControl::default()).unwrap();
        assert_eq!(result.as_bytes(), wire.as_bytes());
    }
    for parameter in [
        "boundary=\"B\"junk",
        "boundary=B; boundary=B",
        "boundary=B; boundary*=UTF-8''B",
        "boundary",
        "boundary*=UTF-8''%XX",
        "boundary*1=B",
        "boundary*0=B; boundary*0=B",
        "boundary*0*=UTF-8''%4; boundary*1*=2",
        "name=\"unclosed; boundary=B",
    ] {
        refusal(
            format!(
                "Content-Type: multipart/mixed; {parameter}\r\n\r\n--B\r\n\r\nbody\r\n--B--\r\n"
            )
            .as_bytes(),
            Refusal::Malformed,
        );
    }
}
#[test]
fn full_delimiter_lines_padding_preamble_epilogue_and_missing_close() {
    let source=b"Content-Type: multipart/mixed; boundary=B\r\n\r\npreamble\r\n--BX\r\n--B \t\r\n\r\nbody\r\n--B-- \t\r\nepilogue";
    let result = prepare_message(source, &RunControl::default()).unwrap();
    assert_eq!(result.as_bytes(), source);
    for body in [
        "--B--\r\n",
        "--B\r\n\r\nbody",
        "--B\r\n\r\nbody\r\n--B--\r\n--B\r\n",
    ] {
        refusal(
            format!("Content-Type: multipart/mixed; boundary=B\r\n\r\n{body}").as_bytes(),
            Refusal::Malformed,
        );
    }
}
#[test]
fn embedded_changed_roots_strip_only_required_ancestor_fields() {
    let forward=b"Content-Type: message/rfc822\r\nContent-Length: stale wrapper\r\nDKIM-Signature: keep ordinary wrapper\r\n\r\nSubject: forwarded\r\nContent-Type: multipart/mixed; boundary=I\r\nContent-MD5: stale\r\nDomainKey-Signature: stale\r\n\r\n--I\r\n\r\nbody\r\n--I\r\nContent-Type: image/png\r\n\r\nimage\r\n--I--";
    let expected=b"Content-Type: message/rfc822\r\nDKIM-Signature: keep ordinary wrapper\r\n\r\nSubject: forwarded\r\nContent-Type: multipart/mixed; boundary=I\r\n\r\n--I\r\n\r\nbody\r\n--I--";
    let result = prepare_message(
        &mixed(&[b"\r\nouter text", forward]),
        &RunControl::default(),
    )
    .unwrap();
    assert_eq!(result.as_bytes(), mixed(&[b"\r\nouter text", expected]));
    assert_eq!(
        result.stripped_fields(),
        ["content-length", "content-md5", "domainkey-signature"]
    );
}
#[test]
fn unquoted_parameter_comments_are_cfws_without_token_splicing() {
    for (parameter, boundary) in [
        ("B (tail)", "B"),
        ("(lead) B (tail)", "B"),
        ("B(comment)C", "B C"),
        ("B(two; nested(comment))", "B"),
        ("\"B (tail)\"", "B (tail)"),
        ("\"B\" (tail)", "B"),
        ("B  C", "B  C"),
        ("B (comment) C", "B  C"),
    ] {
        let wire = format!(
            "Content-Type: multipart/mixed; boundary={parameter}\r\n\r\n--{boundary}\r\n\r\nbody\r\n--{boundary}--\r\n"
        );
        let result = prepare_message(wire.as_bytes(), &RunControl::default()).unwrap();
        assert_eq!(result.as_bytes(), wire.as_bytes());
    }
    for (parameter, boundary) in [
        ("B(comment)C", "BC"),
        ("B(unclosed", "B(unclosed"),
        ("B)tail", "B)tail"),
        ("B\tC", "B\tC"),
    ] {
        refusal(format!("Content-Type: multipart/mixed; boundary={parameter}\r\n\r\n--{boundary}\r\n\r\nbody\r\n--{boundary}--\r\n").as_bytes(),Refusal::Malformed);
    }
    let encoded=b"Content-Type: multipart/mixed; boundary*=UTF-8''B%28tail%29\r\n\r\n--B(tail)\r\n\r\nbody\r\n--B(tail)--\r\n";
    assert!(prepare_message(encoded, &RunControl::default()).is_ok());
}
#[test]
fn nested_control_failures_retain_known_source_section() {
    let child = b"Content-Type: text/plain\r\nContent-Type: text/html\r\n\r\nambiguous";
    let result = prepare_message(&mixed(&[b"\r\nbody", child]), &RunControl::default());
    assert!(
        matches!(result,Err(PreparationError::Refused{kind:Refusal::Malformed,section:Some(section),..}) if section=="2")
    );
    let nested=b"Content-Type: multipart/mixed; boundary=I\r\nContent-Transfer-Encoding: base64\r\n\r\nnot an identity structure";
    let result = prepare_message(&mixed(&[b"\r\nbody", nested]), &RunControl::default());
    assert!(
        matches!(result,Err(PreparationError::Refused{kind:Refusal::Malformed,section:Some(section),..}) if section=="2")
    );
}
#[test]
fn cr_child_ending_and_crlf_frame_keep_separate_transport_ownership() {
    let child = b"Content-Type: text/plain\rContent-Disposition: attachment\r";
    let mut source = b"Content-Type: multipart/mixed; boundary=B\n\n--B\n\nbody\n--B\n".to_vec();
    source.extend(child);
    source.extend_from_slice(b"\r\n--B--\n");
    let result = prepare_message(&source, &RunControl::default()).unwrap();
    assert_eq!(result.removed()[0].bytes, child.len() as u64);
    assert_eq!(
        result.as_bytes(),
        b"Content-Type: multipart/mixed; boundary=B\n\n--B\n\nbody\n--B--\n"
    );
}
#[test]
fn cr_child_ending_and_lf_frame_merge_into_one_crlf_terminator() {
    let child = b"Content-Type: text/plain\rContent-Disposition: attachment\r";
    let mut source = b"Content-Type: multipart/mixed; boundary=B\n\n--B\n\nbody\n--B\n".to_vec();
    source.extend(child);
    source.extend_from_slice(b"\n--B--\n");
    let result = prepare_message(&source, &RunControl::default()).unwrap();
    assert_eq!(result.removed()[0].bytes, (child.len() - 1) as u64);
    assert_eq!(
        result.as_bytes(),
        b"Content-Type: multipart/mixed; boundary=B\n\n--B\n\nbody\n--B--\n"
    );
}

#[test]
fn ambiguous_ancestor_closing_boundary_never_becomes_an_inner_opening() {
    let wire=b"Content-Type: multipart/mixed; boundary=B\r\n\r\n--B\r\nContent-Type: multipart/mixed; boundary=\"B--\"\r\n\r\n--B--\r\n\r\nbody\r\n--B----\r\n--B--\r\n";
    refusal(wire, Refusal::Malformed);
}
