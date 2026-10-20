//! Resource boundaries independent of the production parser's counting helpers.
use eml_core::{MAX_MESSAGE_BYTES, PreparationError, Refusal, RunControl, prepare_message};
fn accepted(bytes: &[u8]) {
    let prepared = prepare_message(bytes, &RunControl::default()).unwrap();
    assert!(prepared.as_bytes() == bytes);
    assert!(prepared.removed().is_empty());
}
fn limited(bytes: &[u8]) {
    assert!(matches!(
        prepare_message(bytes, &RunControl::default()),
        Err(PreparationError::Refused {
            kind: Refusal::LimitExceeded,
            ..
        })
    ));
}
fn multipart(parts: &[Vec<u8>]) -> Vec<u8> {
    let mut wire = b"Content-Type: multipart/mixed; boundary=B\r\n\r\n".to_vec();
    for part in parts {
        wire.extend_from_slice(b"--B\r\n");
        wire.extend(part);
        wire.extend_from_slice(b"\r\n");
    }
    wire.extend_from_slice(b"--B--\r\n");
    wire
}
#[test]
fn raw_message_at_and_above_limit() {
    assert_eq!(MAX_MESSAGE_BYTES, 256 * 1024 * 1024);
    let mut wire = vec![b'x'; MAX_MESSAGE_BYTES];
    accepted(&wire);
    wire.push(b'x');
    limited(&wire);
}
#[test]
fn entity_depth_at_and_above_limit() {
    let wrap = |count| {
        let mut child = b"Content-Type: text/plain\r\n\r\nbody".to_vec();
        for n in 0..count {
            let mut wire =
                format!("Content-Type: multipart/mixed; boundary=Z{n}\r\n\r\n--Z{n}\r\n")
                    .into_bytes();
            wire.extend(child);
            wire.extend(format!("\r\n--Z{n}--").into_bytes());
            child = wire;
        }
        child
    };
    accepted(&wrap(63));
    limited(&wrap(64));
}
#[test]
fn entity_count_at_and_above_limit() {
    let mut parts = vec![b"\r\nx".to_vec(); 9999];
    accepted(&multipart(&parts));
    parts.push(b"\r\nx".to_vec());
    limited(&multipart(&parts));
}
#[test]
fn header_fields_per_entity_at_and_above_limit() {
    let mut wire = b"X: y\r\n".repeat(4096);
    accepted(&wire);
    wire.extend_from_slice(b"X: y\r\n");
    limited(&wire);
}
#[test]
fn header_fields_total_at_and_above_limit() {
    let mut parts = vec![b"X: y\r\n".repeat(4096); 15];
    parts.push(b"X: y\r\n".repeat(4095));
    for part in &mut parts {
        part.extend_from_slice(b"\r\nx");
    }
    accepted(&multipart(&parts));
    parts[15].splice(0..0, b"X: y\r\n".iter().copied());
    limited(&multipart(&parts));
}
#[test]
fn header_block_byte_limit_excludes_separator() {
    let mut wire = b"X:".to_vec();
    wire.extend(vec![b'x'; 1024 * 1024 - 4]);
    wire.extend_from_slice(b"\r\n\r\nbody");
    accepted(&wire);
    wire.insert(2, b'x');
    limited(&wire);
}
#[test]
fn aggregate_header_bytes_at_and_above_limit() {
    let root = b"Content-Type: multipart/mixed; boundary=B\r\n".len();
    let mut parts = Vec::new();
    for n in 0..8 {
        let bytes = 1024 * 1024 - if n == 7 { root } else { 0 };
        let mut part = b"X:".to_vec();
        part.extend(vec![b'x'; bytes - 4]);
        part.extend_from_slice(b"\r\n\r\nx");
        parts.push(part);
    }
    accepted(&multipart(&parts));
    parts[7].insert(2, b'x');
    limited(&multipart(&parts));
}
#[test]
fn parameter_piece_limits_include_empty_pieces() {
    let field =
        |count| format!("Content-Type: text/plain{}\r\n\r\nbody", ";".repeat(count)).into_bytes();
    accepted(&field(256));
    limited(&field(257));
    let mut parts = vec![field(256); 127];
    parts.push(field(255));
    accepted(&multipart(&parts));
    parts[127] = field(256);
    limited(&multipart(&parts));
}
#[test]
fn comment_nesting_at_and_above_limit() {
    let field = |count| {
        format!(
            "Content-Type: {}{}text/plain\r\n\r\nbody",
            "(".repeat(count),
            ")".repeat(count)
        )
    };
    accepted(field(32).as_bytes());
    limited(field(33).as_bytes());
}
#[test]
fn continuation_segments_are_bounded_without_large_index_allocation() {
    let field = |last| {
        let mut disposition = String::from("Content-Disposition: attachment");
        for n in 0..=last {
            disposition.push_str(&format!("; filename*{n}=x"));
        }
        multipart(&[
            b"\r\nbody".to_vec(),
            format!("Content-Type: application/pdf\r\n{disposition}\r\n\r\nx").into_bytes(),
        ])
    };
    assert!(prepare_message(&field(127), &RunControl::default()).is_ok());
    limited(&field(128));
    limited(&multipart(&[b"\r\nbody".to_vec(),b"Content-Type: application/pdf\r\nContent-Disposition: attachment; filename*999999999999999999999999=x\r\n\r\nx".to_vec()]));
}
#[test]
fn media_type_tokens_are_bounded_before_metadata() {
    let wire = |count| {
        multipart(&[
            b"\r\nbody".to_vec(),
            format!(
                "Content-Type: {}/{}\r\nContent-Disposition: attachment\r\n\r\nx",
                "x".repeat(count),
                "y".repeat(count)
            )
            .into_bytes(),
        ])
    };
    let good = prepare_message(&wire(127), &RunControl::default()).unwrap();
    assert_eq!(good.removed()[0].content_type.len(), 255);
    limited(&wire(128));
}
#[test]
fn related_start_identifier_limit_excludes_outer_cfws() {
    let wire = |count| {
        let id = "x".repeat(count);
        format!("Content-Type: multipart/related; boundary=B; start=\"(outside) <{id}> (outside)\"\r\n\r\n--B\r\nContent-ID: <{id}>\r\n\r\nbody\r\n--B--\r\n").into_bytes()
    };
    accepted(&wire(996));
    limited(&wire(997));
}
