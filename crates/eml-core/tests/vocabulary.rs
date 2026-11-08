//! Core human-outcome acceptance, independent of interface rendering.
use eml_core::{
    AddressState, CleanupReceipt, CleanupState, NamespaceObservation, PlanAction, SyncMethod,
    SyncReceipt, SyncState, vocabulary::*,
};
fn facts() -> OutcomeFacts {
    let synced = SyncReceipt {
        method: SyncMethod::Fsync,
        state: SyncState::Succeeded,
        native_code: None,
    };
    OutcomeFacts {
        result: ResultKind::Created,
        plan: None,
        publication: Publication::Published,
        address: AddressState::Verified,
        candidate_identity_known: true,
        final_readback: Some(true),
        file_sync: Some(synced),
        namespace_sync: NamespaceObservation {
            receipt: Some(synced),
            required_edges: 1,
            completed_edges: 1,
            failed_edge_index: None,
        },
        cleanup: CleanupReceipt {
            state: CleanupState::Complete,
            residual_name: None,
            native_code: None,
        },
        existing: None,
        error: None,
        related_resources: true,
    }
}
#[test]
fn u2_outcomes_preserve_qualified_rules_and_typed_failures() {
    let mut f = facts();
    assert_eq!(Outcome::from(&f, 1), Outcome::CopySaved);
    assert_eq!(Outcome::from(&f, 0), Outcome::CopiedAsIs);
    f.result = ResultKind::Planned;
    f.publication = Publication::NotPublished;
    f.file_sync = None;
    f.namespace_sync = NamespaceObservation {
        receipt: None,
        required_edges: 0,
        completed_edges: 0,
        failed_edge_index: None,
    };
    f.plan = Some(PlanAction::Create);
    assert_eq!(Outcome::from(&f, 1), Outcome::Ready);
    assert_eq!(Outcome::from(&f, 0), Outcome::NothingToRemove);
    let good = facts();
    f.existing = Some(ExistingFacts {
        identity_known: true,
        bytes_equal: Some(true),
        stable: true,
        address: AddressState::Verified,
        file_sync: None,
        namespace_sync: f.namespace_sync,
    });
    f.plan = Some(PlanAction::VerifyExisting);
    assert_eq!(Outcome::from(&f, 1), Outcome::WillVerify);
    f.result = ResultKind::Existing;
    f.plan = None;
    f.existing.as_mut().unwrap().file_sync = good.file_sync;
    f.existing.as_mut().unwrap().namespace_sync = good.namespace_sync;
    assert_eq!(Outcome::from(&f, 1), Outcome::AlreadyExists);
    f.result = ResultKind::Failed;
    f.error = Some(eml_core::ErrorCode::ExistingSyncFailed);
    assert_eq!(Outcome::from(&f, 1), Outcome::NeedsChecking);
    f.existing = None;
    for code in eml_core::ErrorCode::ALL {
        f.error = Some(*code);
        let outcome = Outcome::from(&f, 1);
        assert_eq!(
            outcome,
            if *code == eml_core::ErrorCode::Interrupted {
                Outcome::NotProcessed
            } else {
                Outcome::Failed(*code)
            }
        );
        assert!(!outcome.label().is_empty());
        assert!(!outcome.explanation().is_empty());
        assert!(!RunFailure(*code).explanation().is_empty());
    }
    f.publication = Publication::Unknown;
    f.error = Some(eml_core::ErrorCode::WriteFailed);
    assert_eq!(Outcome::from(&f, 1), Outcome::Unknown);
    f.publication = Publication::Published;
    assert_eq!(Outcome::from(&f, 1), Outcome::NeedsChecking);
    f.result = ResultKind::Created;
    assert_eq!(Outcome::from(&f, 1), Outcome::Unknown);
}
#[test]
fn u2_totals_sizes_and_unnamed_parts() {
    for (n, expected) in [
        (999, "999 B"),
        (1000, "1.0 kB"),
        (1_000_000, "1.0 MB"),
        (1_000_000_000, "1.0 GB"),
    ] {
        assert_eq!(format_bytes(n), expected);
    }
    assert_eq!(
        part_name(None, eml_core::RemovalReason::RelatedResource, "image/png"),
        "Inline image"
    );
    assert_eq!(
        part_name(
            Some("photo.png"),
            eml_core::RemovalReason::RelatedResource,
            "image/png"
        ),
        "photo.png"
    );
    let totals = BatchTotals::from(
        [
            (Outcome::Ready, 2, Some((1000, 400))),
            (Outcome::CopySaved, 3, Some((900, 200))),
            (Outcome::Failed(eml_core::ErrorCode::Malformed), 0, None),
            (Outcome::Unknown, 0, None),
            (Outcome::AlreadyExists, 4, Some((100, 0))),
            (Outcome::NotProcessed, 0, None),
        ]
        .map(|(outcome, parts, bytes)| (Some(outcome), parts, bytes)),
        0,
    );
    assert_eq!(
        (
            totals.emails,
            totals.attachments,
            totals.bytes_removed,
            totals.problems
        ),
        (6, 5, 1300, 2)
    );
    assert_eq!(totals.count(Outcome::Ready), 1);
}

#[test]
fn unresolved_emails_have_no_invented_terminal_outcome() {
    let totals = BatchTotals::from([(None, 0, None)], 0);
    assert_eq!(totals.emails, 1);
    assert!(totals.counts.is_empty());
    assert_eq!(totals.problems, 0);
    let mut f = facts();
    f.result = ResultKind::Failed;
    f.publication = Publication::NotPublished;
    f.error = None;
    assert_eq!(Outcome::from(&f, 0), Outcome::Unknown);
}

#[test]
fn field_whitespace_names_use_visible_kinds_without_trimming_real_names() {
    use eml_core::RemovalReason;
    assert_eq!(
        part_name(Some("   "), RemovalReason::Attachment, "application/pdf"),
        "Attachment"
    );
    assert_eq!(
        part_name(
            Some("\u{2003}\u{00a0}"),
            RemovalReason::RelatedResource,
            "image/png"
        ),
        "Inline image"
    );
    assert_eq!(
        part_name(
            Some(" report.pdf "),
            RemovalReason::Attachment,
            "application/pdf"
        ),
        " report.pdf "
    );
}

#[test]
fn field_incomplete_qualification_does_not_invent_a_new_write() {
    let mut existing = facts();
    existing.result = ResultKind::Failed;
    existing.publication = Publication::NotPublished;
    existing.error = Some(eml_core::ErrorCode::ExistingSyncFailed);
    existing.existing = Some(ExistingFacts {
        identity_known: true,
        bytes_equal: Some(true),
        stable: true,
        address: AddressState::Verified,
        file_sync: None,
        namespace_sync: NamespaceObservation {
            receipt: None,
            required_edges: 0,
            completed_edges: 0,
            failed_edge_index: None,
        },
    });
    let outcome = Outcome::from(&existing, 1);
    assert_eq!(outcome, Outcome::NeedsChecking);
    assert_eq!(
        outcome.explanation(),
        "A copy exists, but the final checks didn’t complete. Check the save folder, then try again. Existing copies are never replaced."
    );
}

#[test]
fn completed_cleanup_with_a_native_error_is_not_reported_as_unconfirmed() {
    let mut f = facts();
    f.result = ResultKind::Failed;
    f.publication = Publication::NotPublished;
    f.error = Some(eml_core::ErrorCode::CleanupFailed);
    f.cleanup.state = eml_core::CleanupState::Complete;
    f.cleanup.native_code = Some(5);
    let meaning = classify(&f);
    assert!(
        !meaning
            .consequences
            .iter()
            .any(|c| c.text().contains("could not be established"))
    );
    assert!(meaning.consequences.iter().any(|c| c.text()
        == "Cleanup completed, but reported a native error. Inspect the cleanup details."));
    assert_eq!(
        Outcome::from(&f, 0),
        Outcome::Failed(eml_core::ErrorCode::CleanupFailed)
    );
}

#[test]
fn remaining_work_and_explicitly_interrupted_items_are_counted_once() {
    let totals = BatchTotals::from([(Some(Outcome::NotProcessed), 0, None)], 4);
    assert_eq!(totals.emails, 5);
    assert_eq!(totals.count(Outcome::NotProcessed), 5);
    assert_eq!(totals.problems, 0);
}

#[test]
fn comparison_plans_require_stable_known_unattempted_observations() {
    let no_sync = eml_core::NamespaceObservation {
        receipt: None,
        required_edges: 0,
        completed_edges: 0,
        failed_edge_index: None,
    };
    let mut f = facts();
    f.result = ResultKind::Planned;
    f.publication = Publication::NotPublished;
    f.plan = Some(eml_core::PlanAction::VerifyExisting);
    f.file_sync = None;
    f.namespace_sync = no_sync;
    f.existing = Some(ExistingFacts {
        identity_known: true,
        bytes_equal: Some(true),
        stable: true,
        address: AddressState::NotChecked,
        file_sync: None,
        namespace_sync: no_sync,
    });
    assert_eq!(classify(&f).kind, MeaningKind::Comparison);
    for property in 0..4 {
        let mut bad = f.clone();
        let existing = bad.existing.as_mut().unwrap();
        match property {
            0 => existing.identity_known = false,
            1 => existing.stable = false,
            2 => existing.namespace_sync.required_edges = 1,
            _ => existing.file_sync = facts().file_sync,
        }
        assert_eq!(
            classify(&bad).kind,
            MeaningKind::Unestablished,
            "{property}"
        );
    }
}

#[test]
fn preview_offers_and_complete_results_are_domain_decisions_not_tones() {
    assert!(can_offer_copy([Outcome::Ready], None));
    assert!(can_offer_copy([Outcome::Unknown], Some("FUTURE_CODE")));
    assert!(can_offer_copy([Outcome::NotProcessed], None));
    assert!(!can_offer_copy(
        [Outcome::Failed(eml_core::ErrorCode::Malformed)],
        None
    ));
    assert!(!can_offer_copy([], Some("INPUT_ADMISSION_FAILED")));
    for error in ["NO_INPUTS", "INPUT_LIMIT_EXCEEDED", "OUTPUT_OVERLAP"] {
        assert!(!can_offer_copy([Outcome::Ready], Some(error)));
    }
    assert!(results_cover_inventory(true, false, 0, None));
    assert!(results_cover_inventory(true, false, 0, Some("NO_INPUTS")));
    for (complete, interrupted, remaining, error) in [
        (false, false, 0, None),
        (true, true, 0, None),
        (true, false, 1, None),
        (true, false, 0, Some("FUTURE_CODE")),
        (true, false, 0, Some("OUTPUT_OVERLAP")),
    ] {
        assert!(!results_cover_inventory(
            complete,
            interrupted,
            remaining,
            error
        ));
    }
}
