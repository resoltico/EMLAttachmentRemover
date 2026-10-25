//! Expectations come from the hand-reviewed protocol contract, not enum output.
use eml_core::{ErrorCode, ErrorPhase, PreparationError, Refusal};
use std::collections::{BTreeMap, BTreeSet};

#[test]
fn every_code_and_allowed_or_disallowed_phase_matches_the_contract_table() {
    let document = include_str!("../../../docs/protocol.md");
    let table = document
        .split_once("<!-- error-vocabulary:start -->")
        .unwrap()
        .1
        .split_once("<!-- error-vocabulary:end -->")
        .unwrap()
        .0;
    let mut expected = BTreeMap::new();
    for row in table.lines().filter(|row| row.starts_with('|')).skip(2) {
        let fields: Vec<_> = row.trim_matches('|').split('|').map(str::trim).collect();
        assert_eq!(fields.len(), 2);
        let phases: BTreeSet<_> = fields[1].split('/').collect();
        assert!(
            expected.insert(fields[0], phases).is_none(),
            "Duplicate documented code"
        );
    }
    assert_eq!(
        expected.len(),
        28,
        "Protocol 1 has exactly 28 semantic codes"
    );
    assert_eq!(ErrorCode::ALL.len(), expected.len());
    assert_eq!(ErrorPhase::ALL.len(), 12);
    let phase_names: BTreeSet<_> = ErrorPhase::ALL.iter().map(|phase| phase.as_str()).collect();
    assert_eq!(phase_names.len(), 12);
    for code in ErrorCode::ALL {
        let expected_phases = expected.remove(code.as_str()).expect("Undocumented code");
        assert!(
            expected_phases.is_subset(&phase_names),
            "Unknown documented phase"
        );
        assert_eq!(code.to_string(), code.as_str());
        assert_eq!(code.allowed_phases().len(), expected_phases.len());
        for phase in ErrorPhase::ALL {
            assert_eq!(
                code.allows_phase(*phase),
                expected_phases.contains(phase.as_str()),
                "{code}/{phase}"
            );
        }
    }
    assert!(expected.is_empty(), "Documented code missing from core");
}

#[test]
fn preparation_classification_is_exhaustive_and_preserves_domain_diagnostics() {
    for (kind, expected) in [
        (Refusal::Malformed, ErrorCode::Malformed),
        (Refusal::LimitExceeded, ErrorCode::LimitExceeded),
        (Refusal::Protected, ErrorCode::Protected),
        (Refusal::UnsupportedMime, ErrorCode::UnsupportedMime),
        (Refusal::NoBody, ErrorCode::NoBody),
        (Refusal::Internal, ErrorCode::Internal),
    ] {
        let error = PreparationError::Refused {
            kind,
            section: Some("2.1".into()),
            detail: "Independent source refusal",
        };
        let before = error.clone();
        assert_eq!(ErrorCode::from_preparation(&error), expected);
        assert!(ErrorCode::from_preparation(&error).allows_phase(ErrorPhase::Prepare));
        assert_eq!(error, before);
    }
    assert_eq!(
        ErrorCode::from_preparation(&PreparationError::Interrupted),
        ErrorCode::Interrupted
    );
}

#[test]
fn source_publish_and_interruption_phase_boundaries_are_distinct() {
    assert!(ErrorCode::SourceChanged.allows_phase(ErrorPhase::Publish));
    assert!(!ErrorCode::ReadFailed.allows_phase(ErrorPhase::Publish));
    assert!(!ErrorCode::SourceNotRegular.allows_phase(ErrorPhase::Publish));
    for phase in [
        ErrorPhase::Admission,
        ErrorPhase::Collection,
        ErrorPhase::Cleanup,
        ErrorPhase::Delivery,
    ] {
        assert!(!ErrorCode::Interrupted.allows_phase(phase));
    }
    assert_eq!(ErrorCode::Interrupted.allowed_phases().len(), 8);
    assert!(ErrorCode::Interrupted.allows_phase(ErrorPhase::Publish));
    assert!(!ErrorCode::OutputFailed.allows_phase(ErrorPhase::Prepare));
    assert!(ErrorCode::OutputFailed.allows_phase(ErrorPhase::Delivery));
}

#[test]
fn known_wire_guidance_does_not_invent_meanings_for_future_or_misspelled_codes() {
    use eml_core::ErrorCode;
    assert_eq!(
        ErrorCode::from_wire_name("MALFORMED"),
        Some(ErrorCode::Malformed)
    );
    assert!(
        eml_core::vocabulary::recovery_hint(ErrorCode::Malformed.as_str())
            .unwrap()
            .contains("Export")
    );
    assert!(eml_core::vocabulary::recovery_hint(ErrorCode::Internal.as_str()).is_none());
    assert!(ErrorCode::from_wire_name("malformed").is_none());
    assert!(ErrorCode::from_wire_name("FUTURE_CODE").is_none());
}
