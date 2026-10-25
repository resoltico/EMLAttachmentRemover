//! Private deterministic measurements of actual preparation work.
use crate::{PreparationError, RunControl};
use std::{cell::RefCell, collections::BTreeMap};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Stage {
    #[default]
    SourceParse,
    Planning,
    Authorization,
    CandidateCopy,
    ExactComparison,
    CandidateParse,
    StructureComparison,
    FixedPoint,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Work {
    Physical,
    Classification,
    Padding,
    Copy,
    Comparison,
    Node,
}
#[derive(Clone, Debug, Default)]
pub(crate) struct Measurement {
    pub(crate) bytes: BTreeMap<(Stage, Work), usize>,
    pub(crate) max_unit: usize,
    stage: Stage,
    events: usize,
    stop: Option<(Stage, Work, usize)>,
}
thread_local! {static MEASURED: RefCell<Measurement> = RefCell::new(Measurement::default());}
pub(crate) fn stage(stage: Stage) {
    MEASURED.with_borrow_mut(|m| m.stage = stage);
}
pub(crate) fn record(
    kind: Work,
    bytes: usize,
    control: &RunControl,
) -> Result<(), PreparationError> {
    MEASURED.with_borrow_mut(|m| {
        *m.bytes.entry((m.stage, kind)).or_default() += bytes;
        m.max_unit = m.max_unit.max(if kind == Work::Node { 0 } else { bytes });
        if m.stop
            .is_some_and(|(stage, work, _)| stage == m.stage && work == kind)
        {
            m.events += 1;
            if m.stop.is_some_and(|(_, _, event)| m.events >= event) {
                control.request_stop();
            }
        }
    });
    control.checkpoint()
}
fn reset(stop: Option<(Stage, Work, usize)>) {
    MEASURED.with_borrow_mut(|m| {
        *m = Measurement {
            stop,
            ..Measurement::default()
        }
    });
}
fn measured() -> Measurement {
    MEASURED.with_borrow(Clone::clone)
}
fn message(wrappers: usize, payload: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for n in 0..wrappers {
        bytes.extend_from_slice(
            format!("Content-Type: multipart/mixed; boundary=B{n}\r\n\r\n--B{n}\r\n").as_bytes(),
        );
    }
    bytes.extend_from_slice(b"Content-Type: text/plain\r\n\r\n");
    bytes.extend_from_slice(payload);
    for n in (0..wrappers).rev() {
        bytes.extend_from_slice(format!("\r\n--B{n}--\r\n").as_bytes());
    }
    bytes
}
#[test]
fn payload_work_is_linear_at_shallow_middle_and_admitted_deep_shapes() {
    for payload in [
        vec![b'x'; 32 * 1024 * 1024],
        b"--not-a-delimiter\r\n".repeat(4096),
    ] {
        for depth in [1, 16, 63] {
            let bytes = message(depth, &payload);
            reset(None);
            let prepared = crate::prepare_message(&bytes, &RunControl::default()).unwrap();
            assert!(
                prepared.as_bytes() == bytes.as_slice(),
                "Prepared bytes differ from source: prepared length {}; source length {}",
                prepared.as_bytes().len(),
                bytes.len()
            );
            let m = measured();
            assert!(m.max_unit <= crate::PREPARATION_CHUNK_BYTES, "{m:?}");
            for stage in [Stage::SourceParse, Stage::CandidateParse] {
                let physical = m.bytes.get(&(stage, Work::Physical)).copied().unwrap_or(0);
                let classification = m
                    .bytes
                    .get(&(stage, Work::Classification))
                    .copied()
                    .unwrap_or(0);
                let padding = m.bytes.get(&(stage, Work::Padding)).copied().unwrap_or(0);
                assert!(physical <= bytes.len(), "{m:?}");
                assert!(classification <= 2 * bytes.len(), "{m:?}");
                assert!(padding <= 3 * bytes.len(), "{m:?}");
            }
            assert_eq!(
                m.bytes[&(Stage::CandidateCopy, Work::Copy)],
                prepared.as_bytes().len()
            );
            assert_eq!(
                m.bytes[&(Stage::ExactComparison, Work::Comparison)],
                prepared.as_bytes().len()
            );
            assert!(m.bytes[&(Stage::StructureComparison, Work::Comparison)] <= bytes.len());
            assert!(!m.bytes.contains_key(&(Stage::Authorization, Work::Copy)));
            assert!(!m.bytes.contains_key(&(Stage::FixedPoint, Work::Copy)));
        }
    }
    reset(None);
}
#[test]
fn every_designated_phase_observes_deterministic_stop_without_prepared_value() {
    let bytes = message(16, &vec![b'x'; 256 * 1024]);
    for (stage, kind) in [
        (Stage::SourceParse, Work::Physical),
        (Stage::Planning, Work::Node),
        (Stage::Authorization, Work::Node),
        (Stage::CandidateCopy, Work::Copy),
        (Stage::ExactComparison, Work::Comparison),
        (Stage::CandidateParse, Work::Physical),
        (Stage::StructureComparison, Work::Comparison),
        (Stage::FixedPoint, Work::Node),
    ] {
        reset(Some((stage, kind, 1)));
        assert!(
            matches!(
                crate::prepare_message(&bytes, &RunControl::default()),
                Err(PreparationError::Interrupted)
            ),
            "{stage:?}: {:?}",
            measured()
        );
    }
    reset(None);
}
#[test]
fn preadmission_classification_and_envelope_lookahead_are_checkpointed() {
    for bytes in [
        vec![b'x'; 256 * 1024],
        [vec![b'x'; 256 * 1024], b": value".to_vec()].concat(),
        [
            b"From sender\r\n".to_vec(),
            vec![b'x'; 256 * 1024],
            b": value\r\n\r\nbody".to_vec(),
        ]
        .concat(),
    ] {
        reset(Some((Stage::SourceParse, Work::Classification, 2)));
        assert!(matches!(
            crate::prepare_message(&bytes, &RunControl::default()),
            Err(PreparationError::Interrupted)
        ));
        assert!(measured().max_unit <= crate::PREPARATION_CHUNK_BYTES);
    }
    reset(None);
}
