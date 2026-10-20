//! Private readiness/first-INT facts cannot be synthesized from stop intent.
use eml_protocol::worker_control::*;
#[test]
fn exact_control_sequence_and_normal_ready_only_eof_are_valid() {
    for bytes in [b"R".as_slice(), b"RF"] {
        for split in 0..=bytes.len() {
            let mut stream = WorkerControlStream::default();
            stream.append(&bytes[..split]).unwrap();
            stream.append(&bytes[split..]).unwrap();
            assert!(stream.ready());
            assert_eq!(stream.first_interrupt(), bytes == b"RF");
            stream.finish().unwrap();
            assert!(stream.append(b"F").is_err());
        }
    }
}
#[test]
fn empty_out_of_order_duplicate_extra_and_invalid_control_fail_closed() {
    for bytes in [b"".as_slice(), b"F", b"RR", b"RFF", b"RFX", b"X", b"RF\n"] {
        let mut stream = WorkerControlStream::default();
        assert!(
            stream.append(bytes).and_then(|()| stream.finish()).is_err(),
            "{bytes:?}"
        );
        assert!(stream.append(b"R").is_err());
    }
}
#[test]
fn later_corruption_does_not_erase_preceding_real_acknowledgments() {
    let mut stream = WorkerControlStream::default();
    stream.append(b"RF").unwrap();
    assert!(stream.append(b"X").is_err());
    assert!(stream.ready());
    assert!(stream.first_interrupt());
    assert!(stream.finish().is_err());
}
