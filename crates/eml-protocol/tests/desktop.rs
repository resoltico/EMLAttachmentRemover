//! Independent private operation framing and receipt rejection controls.
use eml_protocol::desktop::*;
use std::io::Write;
fn envelope(header: &str, body: &[u8]) -> Vec<u8> {
    let mut b = MAGIC.to_vec();
    b.extend_from_slice(&(header.len() as u32).to_be_bytes());
    b.extend_from_slice(header.as_bytes());
    if !body.is_empty() {
        b.extend_from_slice(&(body.len() as u32).to_be_bytes());
        b.extend_from_slice(body);
    }
    b.extend_from_slice(&[0; 4]);
    b
}
#[test]
fn processing_frame_is_exact_and_every_truncation_and_trailing_byte_refuses() {
    let h = Header {
        operation_id: OperationId::new(41).unwrap(),
        operation: Operation::Processing,
    };
    let mut out = Vec::new();
    let mut w = BodyWriter::new(&mut out, &h).unwrap();
    w.write_all(b"{\"protocol\":1}").unwrap();
    w.finish().unwrap();
    assert_eq!(
        out,
        envelope(
            r#"{"operation_id":41,"operation":"processing"}"#,
            b"{\"protocol\":1}"
        )
    );
    let (actual, body) = read_operation(out.as_slice()).unwrap();
    assert_eq!(actual, h);
    assert_eq!(body, b"{\"protocol\":1}");
    for n in 0..out.len() {
        assert!(read_operation(&out[..n]).is_err(), "truncation {n}");
    }
    out.push(0);
    assert!(read_operation(out.as_slice()).is_err());
}
#[test]
fn header_strict_shapes_identity_operations_and_destination_encoding() {
    for header in [
        r#"{}"#,
        r#"[1,"processing"]"#,
        r#"{"operation_id":0,"operation":"processing"}"#,
        r#"{"operation_id":-1,"operation":"processing"}"#,
        r#"{"operation_id":18446744073709551616,"operation":"processing"}"#,
        r#"{"operation_id":1,"operation":"processing","operation_id":2}"#,
        r#"{"operation_id":1,"operation":"processing","extra":true}"#,
        r#"{"operation_id":1,"operation":"export"}"#,
        r#"{"operation_id":1,"operation":"unknown"}"#,
        r#"{"operation_id":1,"operation":"processing","destination":null}"#,
        r#"{"operation_id":1,"operation":"export","destination":["unix-bytes","L3RtcC9yLmpzb24="]}"#,
        r#"{"operation_id":1,"operation":"processing"} {}"#,
    ] {
        assert!(
            read_operation(envelope(header, b"{}").as_slice()).is_err(),
            "{header}"
        );
    }
    assert!(OperationId::new(u64::MAX).unwrap().next().is_none());
}
#[test]
fn inclusive_body_chunk_and_header_limits_have_negative_controls() {
    let h = Header {
        operation_id: OperationId::new(1).unwrap(),
        operation: Operation::Processing,
    };
    let mut out = Vec::new();
    let mut writer = BodyWriter::new(&mut out, &h).unwrap();
    for _ in 0..8 {
        writer.write_all(&vec![b' '; MAX_CHUNK_BYTES]).unwrap();
    }
    assert!(writer.write_all(b"x").is_err());
    writer.finish().unwrap();
    assert_eq!(
        read_operation(out.as_slice()).unwrap().1.len(),
        eml_protocol::MAX_REQUEST_BYTES
    );
    let mut bad = envelope(r#"{"operation_id":1,"operation":"processing"}"#, b"{}");
    let offset = MAGIC.len() + 4 + r#"{"operation_id":1,"operation":"processing"}"#.len();
    bad[offset..offset + 4].copy_from_slice(&((MAX_CHUNK_BYTES + 1) as u32).to_be_bytes());
    assert!(read_operation(bad.as_slice()).is_err());
    let mut bad = MAGIC.to_vec();
    bad.extend_from_slice(&((MAX_HEADER_BYTES + 1) as u32).to_be_bytes());
    assert!(read_operation(bad.as_slice()).is_err());
    let mut exact = r#"{"operation_id":1,"operation":"processing"}"#.to_owned();
    exact.extend(std::iter::repeat_n(' ', MAX_HEADER_BYTES - exact.len()));
    assert!(read_operation(envelope(&exact, b"").as_slice()).is_ok());
}
#[test]
fn receipt_rejects_wrong_id_unknown_duplicate_and_nonobject_nested_fields() {
    let text = r#"{"operation_id":8,"publication":"not_published","file_sync":null,"namespace_sync":null,"address_verified":null,"candidate_identity":null,"final_file_verified":null,"cleanup":{"state":"not_needed","residual_name":null,"native_code":null},"failures":[]}"#;
    let id = OperationId::new(8).unwrap();
    assert!(!decode_receipt(text.as_bytes(), id).unwrap().qualified());
    assert!(decode_receipt(text.as_bytes(), OperationId::new(9).unwrap()).is_err());
    for bad in [
        text.replace(
            "\"state\":\"not_needed\"",
            "\"state\":\"not_needed\",\"extra\":1",
        ),
        text.replace(
            "\"operation_id\":8",
            "\"operation_id\":8,\"operation_id\":8",
        ),
        text.replace(
            "{\"state\":\"not_needed\",\"residual_name\":null,\"native_code\":null}",
            "[\"not_needed\",null,null]",
        ),
        format!("{text} null"),
    ] {
        assert!(decode_receipt(bad.as_bytes(), id).is_err(), "{bad}");
    }
}

#[test]
fn accepted_receipt_survives_later_stdout_corruption_without_clean_completion() {
    let text = r#"{"operation_id":8,"publication":"published","file_sync":{"method":"fsync","state":"succeeded","native_code":null},"namespace_sync":{"method":"fsync","state":"succeeded","native_code":null},"address_verified":true,"candidate_identity":{"kind":"posix","value":"1:2"},"final_file_verified":true,"cleanup":{"state":"complete","residual_name":null,"native_code":null},"failures":[]}"#;
    let id = OperationId::new(8).unwrap();
    let bad = format!("{text} broken");
    let (receipt, complete) = observe_receipt(bad.as_bytes(), id).unwrap();
    assert_eq!(receipt.operation_id, id);
    assert!(receipt.qualified());
    assert!(!complete);
    assert!(decode_receipt(bad.as_bytes(), id).is_err());
}

#[test]
fn export_success_rejects_contradictory_codes_and_preserves_limited_sync() {
    let text = r#"{"operation_id":8,"publication":"published","file_sync":{"method":"fsync","state":"succeeded","native_code":null},"namespace_sync":{"method":"fsync","state":"succeeded","native_code":null},"address_verified":true,"candidate_identity":{"kind":"posix","value":"1:2"},"final_file_verified":true,"cleanup":{"state":"complete","residual_name":null,"native_code":null},"failures":[]}"#;
    let id = OperationId::new(8).unwrap();
    for edge in ["file_sync", "namespace_sync"] {
        for (method, code, accepted) in [
            ("fsync", Some(5), false),
            ("fsync", Some(0), false),
            ("fsync_fullfsync_unsupported", None, false),
            ("fsync_fullfsync_unsupported", Some(5), false),
            ("fsync_fullfsync_unsupported", Some(22), true),
            ("fsync_fullfsync_unsupported", Some(45), true),
            ("fsync_fullfsync_unsupported", Some(102), true),
        ] {
            let mut fields: serde_json::Value = serde_json::from_str(text).unwrap();
            fields[edge]["method"] = method.into();
            fields[edge]["native_code"] = serde_json::json!(code);
            let bytes = serde_json::to_vec(&fields).unwrap();
            let raw: ExportReceipt = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(raw.qualified(), accepted);
            assert_eq!(decode_receipt(&bytes, id).is_ok(), accepted);
        }
    }
    // Export retains its separate policy: an explicitly unsupported request is
    // usable with the actual limitation shown, not full durability.
    let unsupported = text.replace(
        "\"state\":\"succeeded\",\"native_code\":null",
        "\"state\":\"unsupported\",\"native_code\":95",
    );
    assert!(
        decode_receipt(unsupported.as_bytes(), id)
            .unwrap()
            .qualified()
    );
}

#[cfg(feature = "native-fixtures")]
#[test]
fn observed_header_boundary_uses_the_strict_codec_before_any_body_read() {
    use std::{
        cell::Cell,
        io::{self, Read},
    };
    let header = r#"{"operation_id":57,"operation":"processing"}"#;
    let bytes = envelope(header, b"authored-body");
    let boundary = MAGIC.len() + 4 + header.len();
    let reached = Cell::new(false);
    struct Checked<'a> {
        bytes: &'a [u8],
        offset: usize,
        boundary: usize,
        reached: &'a Cell<bool>,
    }
    impl Read for Checked<'_> {
        fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
            if self.offset >= self.boundary {
                assert!(
                    self.reached.get(),
                    "Body read preceded validated-header observation"
                );
            }
            let count = out.len().min(self.bytes.len() - self.offset);
            out[..count].copy_from_slice(&self.bytes[self.offset..self.offset + count]);
            self.offset += count;
            Ok(count)
        }
    }
    let reader = Checked {
        bytes: &bytes,
        offset: 0,
        boundary,
        reached: &reached,
    };
    let (_, body) = read_operation_observed(reader, |accepted| {
        assert_eq!(accepted.operation_id.get(), 57);
        assert_eq!(accepted.operation, Operation::Processing);
        assert!(!reached.replace(true));
    })
    .unwrap();
    assert_eq!(body, b"authored-body");
    let mut calls = 0;
    assert!(
        read_operation_observed(
            envelope(r#"{"operation_id":0,"operation":"processing"}"#, b"{}").as_slice(),
            |_| calls += 1
        )
        .is_err()
    );
    assert_eq!(calls, 0);
    assert!(read_operation_observed(&bytes[..boundary], |_| calls += 1).is_err());
    assert_eq!(
        calls, 1,
        "A valid observed header never excuses a truncated body"
    );
}
