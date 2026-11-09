//! Independent bytes and removal counts for root and nested multipart endings.
use eml_message::{RemovalReason, RunControl, prepare_message};
#[test]
fn y2_root_and_nested_epilogues_leave_only_message_parts() {
    let source = b"Content-Type: multipart/mixed; boundary=O\r\n\r\npreamble\r\n--O\r\nContent-Type: multipart/mixed; boundary=I\r\n\r\n--I\r\n\r\nbody\r\n--I--\r\nhidden-inner\r\n--O--\r\nhidden-outer";
    let expected = b"Content-Type: multipart/mixed; boundary=O\r\n\r\npreamble\r\n--O\r\nContent-Type: multipart/mixed; boundary=I\r\n\r\n--I\r\n\r\nbody\r\n--I--\r\n--O--\r\n";
    let message = prepare_message(source, &RunControl::default()).unwrap();
    assert_eq!(message.as_bytes(), expected);
    assert_eq!(
        message
            .removed()
            .iter()
            .map(|r| (r.section.as_str(), r.bytes, r.reason))
            .collect::<Vec<_>>(),
        [
            ("1", 14, RemovalReason::AfterMessageEnd),
            ("", 12, RemovalReason::AfterMessageEnd)
        ]
    );
    let again = prepare_message(expected, &RunControl::default()).unwrap();
    assert_eq!(again.as_bytes(), expected);
    assert!(again.removed().is_empty());
}
#[test]
fn y2_field_epilogue_metadata_counts_exact_source_bytes() {
    let input = include_bytes!("../../../tests/contract/mime/inputs/16-content-after-end.eml");
    let expected = include_bytes!("../../../tests/contract/mime/golden/16-content-after-end.eml");
    let message = prepare_message(input, &RunControl::default()).unwrap();
    assert_eq!(message.as_bytes(), expected);
    let closing = b"--===============3061883257043333973==--\n";
    let start = input
        .windows(closing.len())
        .position(|b| b == closing)
        .unwrap()
        + closing.len();
    assert_eq!(
        message.removed().last().unwrap().reason,
        RemovalReason::AfterMessageEnd
    );
    assert_eq!(
        message.removed().last().unwrap().bytes,
        (input.len() - start) as u64
    );
}

#[test]
fn z1_mixed_line_endings_keep_the_enclosing_framing() {
    let input =
        include_bytes!("../../../tests/contract/mime/inputs/19-mixed-line-ending-epilogue.eml");
    let expected =
        include_bytes!("../../../tests/contract/mime/golden/19-mixed-line-ending-epilogue.eml");
    let message = prepare_message(input, &RunControl::default()).unwrap();
    assert_eq!(message.as_bytes(), expected);
    assert_eq!(message.removed().len(), 1);
    assert_eq!(message.removed()[0].reason, RemovalReason::AfterMessageEnd);
    assert_eq!(message.removed()[0].bytes, 2);
}
