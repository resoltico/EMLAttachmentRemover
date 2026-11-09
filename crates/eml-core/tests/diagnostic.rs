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
fn wire_names_are_unique_exact_and_do_not_invent_unknown_codes() {
    let names: BTreeSet<_> = ErrorCode::ALL.iter().map(|code| code.as_str()).collect();
    assert_eq!(names.len(), ErrorCode::ALL.len());
    assert!(names.contains("MALFORMED"));
    assert!(!names.contains("malformed"));
    assert!(!names.contains("FUTURE_CODE"));
    assert!(
        eml_core::vocabulary::Outcome::Failed(ErrorCode::Malformed)
            .explanation()
            .contains("export")
    );
}

#[test]
fn numeric_native_diagnostics_have_stable_domains() {
    assert_eq!(
        eml_core::NativeDiagnostic::OsCode(13).to_string(),
        "OS error 13"
    );
    assert_eq!(
        eml_core::NativeDiagnostic::OsCode(-1).to_string(),
        "OS error -1"
    );
    #[cfg(windows)]
    assert_eq!(
        eml_core::NativeDiagnostic::NtStatus {
            status: 0xC0000022u32 as i32,
            win32: 5
        }
        .to_string(),
        "NTSTATUS 0xC0000022; Win32 5"
    );
}
