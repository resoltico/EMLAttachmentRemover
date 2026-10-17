//! Actual framing limits and irreversible transport failure controls.
use eml_app::{FrameError, FrameSplitter};
#[test]
fn every_split_boundary_delivers_only_complete_records_in_order() {
    let bytes = b"{\"event\":\"start\"}\n{\"event\":\"summary\"}\n";
    for boundary in 0..=bytes.len() {
        let mut splitter = FrameSplitter::default();
        let mut frames = Vec::new();
        for chunk in [&bytes[..boundary], &bytes[boundary..]] {
            splitter
                .append::<FrameError>(chunk, |record| {
                    frames.push(record.to_vec());
                    Ok(())
                })
                .unwrap();
        }
        assert!(!splitter.finish());
        assert_eq!(
            frames,
            [
                b"{\"event\":\"start\"}".to_vec(),
                b"{\"event\":\"summary\"}".to_vec()
            ]
        );
        assert_eq!(
            splitter.append::<FrameError>(b"more\n", |_| Ok(())),
            Err(FrameError::Failed)
        );
    }
}
#[test]
fn partial_tail_is_not_delivered_and_consumer_or_empty_record_failure_latches() {
    let mut splitter = FrameSplitter::default();
    let mut frames = Vec::new();
    splitter
        .append::<FrameError>(b"accepted\npartial", |record| {
            frames.push(record.to_vec());
            Ok(())
        })
        .unwrap();
    assert!(splitter.finish());
    assert_eq!(frames, [b"accepted".to_vec()]);
    let mut splitter = FrameSplitter::default();
    assert_eq!(
        splitter.append::<FrameError>(b"bad\nlater\n", |_| Err(FrameError::Failed)),
        Err(FrameError::Failed)
    );
    assert_eq!(
        splitter.append::<FrameError>(b"valid\n", |_| Ok(())),
        Err(FrameError::Failed)
    );
    let mut splitter = FrameSplitter::default();
    assert_eq!(
        splitter.append::<FrameError>(b"\n", |_| Ok(())),
        Err(FrameError::EmptyRecord)
    );
    assert_eq!(
        splitter.append::<FrameError>(b"valid\n", |_| Ok(())),
        Err(FrameError::Failed)
    );
}
#[test]
fn line_limit_counts_lf_and_run_limit_is_strict() {
    use eml_protocol::{MAX_LINE_BYTES, MAX_RUN_BYTES};
    let mut splitter = FrameSplitter::default();
    let block = vec![b'x'; 65536];
    for _ in 0..(MAX_LINE_BYTES / 65536 - 1) {
        splitter.append::<FrameError>(&block, |_| Ok(())).unwrap();
    }
    splitter
        .append::<FrameError>(&vec![b'x'; 65535], |_| Ok(()))
        .unwrap();
    let mut length = 0;
    splitter
        .append::<FrameError>(b"\n", |record| {
            length = record.len();
            Ok(())
        })
        .unwrap();
    assert_eq!(length, MAX_LINE_BYTES - 1);
    let mut splitter = FrameSplitter::default();
    for _ in 0..MAX_LINE_BYTES / 65536 - 1 {
        splitter.append::<FrameError>(&block, |_| Ok(())).unwrap();
    }
    assert_eq!(
        splitter.append::<FrameError>(&block, |_| Ok(())),
        Err(FrameError::LineLimit)
    );
    let mut chunk = vec![b'x'; 65535];
    chunk.push(b'\n');
    let mut splitter = FrameSplitter::default();
    for _ in 0..MAX_RUN_BYTES / chunk.len() - 1 {
        splitter.append::<FrameError>(&chunk, |_| Ok(())).unwrap();
    }
    assert_eq!(
        splitter.append::<FrameError>(&chunk, |_| Ok(())),
        Err(FrameError::RunLimit)
    );
}
