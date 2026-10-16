//! Serial read-only execution through the final typed event/outcome boundary.
use crate::{
    AddressState, CollectedObservation, CollectionState, Counts, DeliveryFailure, DeliveryPhase,
    DestinationPlan, DestinationPlanState, ErrorCode, ErrorPhase, Event, EventSink,
    ExistingComparisonError, ExistingObservation, FailureEffect, InvalidRequest, ItemEvent,
    ItemFailure, ItemResult, MessageMetadata, NamespaceObservation, OperationFailure, PlanAction,
    PlannedCopy, PreparationError, PreparedSource, ReportPath, RunControl, RunMode, RunOutcome,
    RunRequest, RunStart, SourceOrigin, SourcePreparationError, collect_sources, compare_existing,
    destination::{source_identities, source_parent},
    plan_destinations, prepare_source,
};
use eml_fs::{
    BoundDirectory, BoundFile, BoundFileError, CleanupReceipt, CleanupState, EntryKind,
    EntryObservation, FileSignature, Identity,
};
use std::collections::HashSet;

/// Maximum removal entries fully admitted for reporting across one run.
pub const MAX_REMOVAL_ENTRIES: usize = 20_000;

fn failure<'a>(error: OperationFailure) -> Box<ItemFailure<'a>> {
    let address = match error.code() {
        ErrorCode::AddressChanged => AddressState::Changed,
        ErrorCode::AddressUnverified => AddressState::Unknown,
        _ => AddressState::NotChecked,
    };
    Box::new(ItemFailure {
        error,
        section: None,
        metadata: None,
        existing: None,
        effect: FailureEffect::NotPublished {
            candidate_identity: None,
            address,
            file_sync: None,
            namespace_sync: NamespaceObservation::unattempted(),
            cleanup: CleanupReceipt {
                state: CleanupState::NotNeeded,
                residual_name: None,
                native_code: None,
            },
        },
    })
}
fn interrupted(phase: ErrorPhase) -> OperationFailure {
    OperationFailure::new(
        ErrorCode::Interrupted,
        phase,
        "Started item interrupted before publication",
    )
}
fn checkpoint(control: &RunControl, phase: ErrorPhase) -> Result<(), OperationFailure> {
    if control.is_stopped() {
        Err(interrupted(phase))
    } else {
        Ok(())
    }
}
fn source_error(error: BoundFileError, phase: ErrorPhase) -> OperationFailure {
    match error {
        BoundFileError::NotRegular => OperationFailure::new(
            ErrorCode::SourceNotRegular,
            phase,
            "Opened source is not an ordinary regular file",
        ),
        BoundFileError::Changed => OperationFailure::new(
            ErrorCode::SourceChanged,
            phase,
            "Source differs from its admitted binding",
        ),
        BoundFileError::ObservationFailed(error) => OperationFailure::io(
            ErrorCode::ReadFailed,
            phase,
            "Source binding cannot be inspected",
            error,
        ),
    }
}
fn preparation_failure(error: SourcePreparationError) -> Box<ItemFailure<'static>> {
    let error = match error {
        SourcePreparationError::InterruptedRead => interrupted(ErrorPhase::Read),
        SourcePreparationError::Binding(error) => source_error(error, ErrorPhase::Read),
        SourcePreparationError::Read(error) => OperationFailure::io(
            ErrorCode::ReadFailed,
            ErrorPhase::Read,
            "Source data cannot be read",
            error,
        ),
        SourcePreparationError::Preparation(PreparationError::Interrupted) => {
            interrupted(ErrorPhase::Prepare)
        }
        SourcePreparationError::Preparation(error @ PreparationError::Refused { .. }) => {
            let code = error.code();
            let PreparationError::Refused {
                detail, section, ..
            } = error
            else {
                unreachable!()
            };
            let mut item = failure(OperationFailure::new(code, ErrorPhase::Prepare, detail));
            item.section = section;
            return item;
        }
    };
    failure(error)
}
fn bind_source(
    plan: &DestinationPlan<'_>,
    index: usize,
    control: &RunControl,
) -> Result<PreparedSource, Box<ItemFailure<'static>>> {
    checkpoint(control, ErrorPhase::Read).map_err(failure)?;
    let sources = plan.sources();
    let row = &sources.items()[index];
    let CollectedObservation::File {
        signature,
        parent: expected_parent,
    } = row.observation()
    else {
        unreachable!("caller handles collection failures");
    };
    let parent = source_parent(sources, index, control)
        .map_err(|error| failure(error.during_execution(ErrorPhase::Read)))?;
    #[cfg(unix)]
    let file = parent.open_file(
        row.native_path().file_name().expect("collected filename"),
        row.origin() == SourceOrigin::Explicit,
    );
    #[cfg(windows)]
    let file = BoundFile::open(row.native_path(), row.origin() == SourceOrigin::Explicit);
    let file = file.map_err(|error| failure(source_error(error, ErrorPhase::Read)))?;
    #[cfg(windows)]
    if file.parent_identity() != *expected_parent {
        return Err(failure(OperationFailure::new(
            ErrorCode::SourceChanged,
            ErrorPhase::Read,
            "Opened source parent differs from inventory",
        )));
    }
    // Keep the read-only parent live through initial file admission on both hosts.
    if parent.identity() != *expected_parent {
        return Err(failure(OperationFailure::new(
            ErrorCode::SourceChanged,
            ErrorPhase::Read,
            "Source parent differs from inventory",
        )));
    }
    prepare_source(file, *signature, control).map_err(preparation_failure)
}
fn metadata(source: &PreparedSource) -> MessageMetadata<'_> {
    MessageMetadata {
        source_bytes: source.source_length(),
        copy_bytes: source.message().as_bytes().len() as u64,
        removed: source.message().removed(),
        stripped_fields: source.message().stripped_fields(),
    }
}
fn existing_error(error: ExistingComparisonError) -> OperationFailure {
    match error {
        ExistingComparisonError::Interrupted => interrupted(ErrorPhase::Existing),
        ExistingComparisonError::SourceAlias => OperationFailure::new(
            ErrorCode::SourceDestinationAlias,
            ErrorPhase::Existing,
            "Existing output aliases the held source",
        ),
        ExistingComparisonError::Source(error) => source_error(error, ErrorPhase::Read),
        ExistingComparisonError::Existing(BoundFileError::ObservationFailed(error)) => {
            OperationFailure::io(
                ErrorCode::BindFailed,
                ErrorPhase::Read,
                "Existing copy binding cannot be inspected",
                error,
            )
        }
        ExistingComparisonError::Read(error) => OperationFailure::io(
            ErrorCode::ReadFailed,
            ErrorPhase::Read,
            "Existing copy data cannot be read",
            error,
        ),
        ExistingComparisonError::Existing(_) => OperationFailure::new(
            ErrorCode::DestinationExists,
            ErrorPhase::Existing,
            "Existing copy binding changed during comparison",
        ),
    }
}
fn observation(identity: Identity, bytes_equal: Option<bool>, stable: bool) -> ExistingObservation {
    ExistingObservation {
        identity: Some(identity),
        bytes_equal,
        stable,
        address: AddressState::NotChecked,
        file_sync: None,
        namespace_sync: NamespaceObservation::unattempted(),
    }
}
fn open_existing(
    parent: &BoundDirectory,
    info: &crate::DestinationInfo,
) -> Result<BoundFile, BoundFileError> {
    #[cfg(unix)]
    {
        parent.open_file(info.basename(), false)
    }
    #[cfg(windows)]
    {
        let file = BoundFile::open(info.native_path(), false)?;
        if file.parent_identity() != parent.identity() {
            return Err(BoundFileError::Changed);
        }
        Ok(file)
    }
}
fn prepared_plan<'a>(
    plan: &DestinationPlan<'_>,
    index: usize,
    source: &'a PreparedSource,
    ids: &HashSet<Identity>,
    control: &RunControl,
    admitted: &mut usize,
) -> Result<PlannedCopy<'a>, Box<ItemFailure<'a>>> {
    checkpoint(control, ErrorPhase::Prepare).map_err(failure)?;
    let entries = source.message().removed().len();
    let Some(total) = admitted
        .checked_add(entries)
        .filter(|total| *total <= MAX_REMOVAL_ENTRIES)
    else {
        return Err(failure(OperationFailure::new(
            ErrorCode::LimitExceeded,
            ErrorPhase::Prepare,
            "Run removal metadata exceeds its retained-entry budget",
        )));
    };
    *admitted = total;
    let data = metadata(source);
    let mut inspected = None;
    let inspect = (|| {
        checkpoint(control, ErrorPhase::Existing)?;
        let Some(parent) = plan.bind_parent(index, control)? else {
            source
                .revalidate()
                .map_err(|error| source_error(error, ErrorPhase::Read))?;
            checkpoint(control, ErrorPhase::Existing)?;
            return Ok((PlanAction::Create, None));
        };
        let info = plan.records()[index]
            .destination()
            .expect("planned destination");
        let entry = parent
            .observe_child_if_present(info.basename())
            .map_err(|error| {
                OperationFailure::io(
                    ErrorCode::BindFailed,
                    ErrorPhase::Read,
                    "Existing output cannot be inspected",
                    error,
                )
            })?;
        let Some(entry) = entry else {
            source
                .revalidate()
                .map_err(|error| source_error(error, ErrorPhase::Read))?;
            checkpoint(control, ErrorPhase::Existing)?;
            return Ok((PlanAction::Create, None));
        };
        inspected = Some(observation(entry.identity, None, false));
        if ids.contains(&entry.identity) {
            return Err(OperationFailure::new(
                ErrorCode::SourceDestinationAlias,
                ErrorPhase::Existing,
                "Existing output aliases a selected source",
            ));
        }
        if entry.kind != EntryKind::File {
            return Err(OperationFailure::new(
                ErrorCode::DestinationExists,
                ErrorPhase::Existing,
                "Existing output is not an ordinary regular file",
            ));
        }
        Ok((PlanAction::VerifyExisting, Some((parent, entry))))
    })();
    let (action, existing) = match inspect {
        Ok(value) => value,
        Err(error) => {
            let mut item = failure(error);
            item.metadata = Some(data);
            item.existing = inspected;
            return Err(item);
        }
    };
    let Some((parent, entry)) = existing else {
        return Ok(PlannedCopy {
            action,
            metadata: data,
            existing: None,
        });
    };
    let info = plan.records()[index]
        .destination()
        .expect("known destination");
    let bound = match open_existing(&parent, info) {
        Ok(file) if Some(file.initial_signature()) == entry.file => file,
        Ok(_) => {
            let mut item = failure(OperationFailure::new(
                ErrorCode::DestinationExists,
                ErrorPhase::Existing,
                "Existing output changed while opening",
            ));
            item.metadata = Some(data);
            item.existing = Some(observation(entry.identity, None, false));
            return Err(item);
        }
        Err(error) => {
            let mut item = failure(existing_error(ExistingComparisonError::Existing(error)));
            item.metadata = Some(data);
            item.existing = Some(observation(entry.identity, None, false));
            return Err(item);
        }
    };
    let compared = match compare_existing(source, bound, control) {
        Ok(value) => value,
        Err(error) => {
            let observed = observation(error.identity(), error.bytes_equal(), error.stable());
            // Preserve the original native cause through the typed role mapping.
            let mut item = failure(comparison_failure(error));
            item.metadata = Some(data);
            item.existing = Some(observed);
            return Err(item);
        }
    };
    let mut observed = observation(
        compared.signature().identity,
        Some(compared.bytes_equal()),
        true,
    );
    let final_check = (|| {
        checkpoint(control, ErrorPhase::Existing)?;
        let Some(current) = plan.bind_parent(index, control)? else {
            return Err(OperationFailure::new(
                ErrorCode::AddressChanged,
                ErrorPhase::Address,
                "Existing output parent disappeared",
            ));
        };
        if current.identity() != parent.identity() {
            return Err(OperationFailure::new(
                ErrorCode::AddressChanged,
                ErrorPhase::Address,
                "Existing output address changed",
            ));
        }
        let entry = current
            .observe_child_if_present(info.basename())
            .map_err(|error| {
                observed.stable = false;
                OperationFailure::io(
                    ErrorCode::AddressUnverified,
                    ErrorPhase::Address,
                    "Existing output address cannot be inspected",
                    error,
                )
            })?;
        check_existing_observation(compared.signature(), entry).inspect_err(|_| {
            observed.stable = false;
        })?;
        compared.revalidate().map_err(|error| {
            observed.stable = false;
            existing_error(ExistingComparisonError::Existing(error))
        })?;
        source
            .revalidate()
            .map_err(|error| source_error(error, ErrorPhase::Read))?;
        checkpoint(control, ErrorPhase::Existing)
    })();
    if let Err(error) = final_check {
        match error.code() {
            ErrorCode::AddressChanged => observed.address = AddressState::Changed,
            ErrorCode::AddressUnverified => observed.address = AddressState::Unknown,
            _ => {}
        }
        let mut item = failure(error);
        item.metadata = Some(data);
        item.existing = Some(observed);
        return Err(item);
    }
    if !compared.bytes_equal() {
        let mut item = failure(OperationFailure::new(
            ErrorCode::DestinationExists,
            ErrorPhase::Existing,
            "Existing output bytes differ from the verified copy",
        ));
        item.metadata = Some(data);
        item.existing = Some(observed);
        return Err(item);
    }
    Ok(PlannedCopy {
        action,
        metadata: data,
        existing: Some(observed),
    })
}
fn check_existing_observation(
    expected: FileSignature,
    entry: Option<EntryObservation>,
) -> Result<(), OperationFailure> {
    let Some(entry) = entry else {
        return Err(OperationFailure::new(
            ErrorCode::AddressChanged,
            ErrorPhase::Address,
            "Previously observed existing output is missing",
        ));
    };
    if entry.identity != expected.identity {
        return Err(OperationFailure::new(
            ErrorCode::AddressChanged,
            ErrorPhase::Address,
            "Existing output address designates a different file",
        ));
    }
    if entry.kind != EntryKind::File || entry.file != Some(expected) {
        return Err(OperationFailure::new(
            ErrorCode::DestinationExists,
            ErrorPhase::Existing,
            "Existing output changed after byte comparison",
        ));
    }
    Ok(())
}
fn comparison_failure(error: crate::ExistingComparisonFailure) -> OperationFailure {
    // Into-error preserves native causes without formatting source/header payload.
    existing_error(error.into_error())
}
fn emit_item(
    plan: &DestinationPlan<'_>,
    index: usize,
    result: ItemResult<'_>,
    outcome: &mut RunOutcome,
    sink: &mut impl EventSink,
) -> bool {
    match &result {
        ItemResult::Created(..) => outcome.counts.created += 1,
        ItemResult::UpToDate(..) => outcome.counts.up_to_date += 1,
        ItemResult::Planned(_) => outcome.counts.planned += 1,
        ItemResult::Failed(error) => {
            outcome.counts.failed += 1;
            outcome.interrupted |= error.error().code() == ErrorCode::Interrupted;
        }
    }
    outcome.counts.not_processed -= 1;
    let source_display = plan.sources().display_path(index).expect("known source");
    let destination_display = plan.display_path(index);
    let event = ItemEvent {
        index,
        source: ReportPath {
            native: plan.sources().items()[index].native_path(),
            display: &source_display,
        },
        destination: plan.records()[index]
            .destination()
            .zip(destination_display.as_deref())
            .map(|(info, display)| ReportPath {
                native: info.native_path(),
                display,
            }),
        result,
    };
    if let Err(error) = sink.emit(Event::Item(&event)) {
        outcome.delivery_failure = Some(DeliveryFailure {
            phase: DeliveryPhase::Item,
            error,
        });
        false
    } else {
        true
    }
}
/// Execute a real serial read-only run through the typed core event boundary.
/// Requires RunMode::DryRun before any filesystem I/O or event; no create request
/// is silently converted. This operation never stages, creates or synchronizes.
pub fn dry_run(
    request: &RunRequest,
    control: &RunControl,
    sink: &mut impl EventSink,
) -> Result<RunOutcome, InvalidRequest> {
    if request.mode != RunMode::DryRun {
        return Err(InvalidRequest::DryRunModeRequired);
    }
    let sources = collect_sources(request, control)?;
    let plan = plan_destinations(sources, control);
    let total = plan.sources().items().len();
    let mut outcome = RunOutcome {
        total,
        counts: Counts {
            not_processed: total,
            ..Counts::default()
        },
        collection_complete: plan.sources().collection_complete(),
        interrupted: control.is_stopped() || plan.state() == DestinationPlanState::Interrupted,
        run_error: plan.run_error().or(plan.sources().run_error()).cloned(),
        delivery_failure: None,
    };
    if total == 0 && outcome.run_error.is_none() && plan.state() == DestinationPlanState::Complete {
        outcome.run_error = Some(OperationFailure::new(
            ErrorCode::NoInputs,
            ErrorPhase::Collection,
            "Discovery found no work items",
        ));
    }
    let start = RunStart {
        dry_run: true,
        total,
        collection_complete: outcome.collection_complete,
    };
    if let Err(error) = sink.emit(Event::Start(&start)) {
        outcome.delivery_failure = Some(DeliveryFailure {
            phase: DeliveryPhase::Start,
            error,
        });
        outcome.interrupted |= control.is_stopped();
        return Ok(outcome);
    }
    let ids = source_identities(plan.sources());
    let mut admitted = 0;
    for index in 0..total {
        if control.is_stopped() {
            outcome.interrupted = true;
            break;
        }
        if outcome.interrupted {
            break;
        }
        let row = &plan.sources().items()[index];
        if plan.sources().state() == CollectionState::InputAdmissionFailed {
            if row.origin() == SourceOrigin::Explicit
                && let CollectedObservation::Failed(error) = row.observation()
                && !emit_item(
                    &plan,
                    index,
                    ItemResult::Failed(failure(error.clone())),
                    &mut outcome,
                    sink,
                )
            {
                break;
            }
            continue;
        }
        if plan.state() != DestinationPlanState::Complete {
            break;
        }
        if let Some(error) = plan.records()[index].failure() {
            if !emit_item(
                &plan,
                index,
                ItemResult::Failed(failure(error.clone())),
                &mut outcome,
                sink,
            ) {
                break;
            }
            continue;
        }
        let source = match bind_source(&plan, index, control) {
            Ok(source) => source,
            Err(error) => {
                if !emit_item(&plan, index, ItemResult::Failed(error), &mut outcome, sink) {
                    break;
                }
                continue;
            }
        };
        let result = match prepared_plan(&plan, index, &source, &ids, control, &mut admitted) {
            Ok(planned) => ItemResult::Planned(planned),
            Err(error) => ItemResult::Failed(error),
        };
        if !emit_item(&plan, index, result, &mut outcome, sink) {
            break;
        }
    }
    outcome.interrupted |= control.is_stopped();
    if outcome.delivery_failure.is_none()
        && let Err(error) = sink.emit(Event::Summary(&outcome))
    {
        outcome.delivery_failure = Some(DeliveryFailure {
            phase: DeliveryPhase::Summary,
            error,
        });
        outcome.interrupted |= control.is_stopped();
    }
    // A successfully delivered terminal summary fixes the semantic outcome.
    // A later stop cannot retroactively disagree with that completed stream.
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::Path};
    #[test]
    fn final_existing_observation_distinguishes_address_and_same_file_changes() {
        let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/run-tests");
        fs::create_dir_all(&base).unwrap();
        let root = tempfile::tempdir_in(base).unwrap();
        fs::write(root.path().join("copy"), b"owned").unwrap();
        fs::write(root.path().join("other"), b"other").unwrap();
        let context = crate::selection::WorkingContext::capture(root.path()).unwrap();
        let directory = context
            .directory(Path::new("."))
            .unwrap_or_else(|_| panic!("Owned directory must bind"));
        let original = directory
            .observe_child(std::ffi::OsStr::new("copy"))
            .unwrap();
        let expected = original.file.unwrap();
        check_existing_observation(expected, Some(original)).unwrap();
        let missing = directory
            .observe_child_if_present(std::ffi::OsStr::new("missing"))
            .unwrap();
        let absent = check_existing_observation(expected, missing).unwrap_err();
        assert_eq!(
            (absent.code(), absent.phase()),
            (ErrorCode::AddressChanged, ErrorPhase::Address)
        );
        let other = directory
            .observe_child(std::ffi::OsStr::new("other"))
            .unwrap();
        let different = check_existing_observation(expected, Some(other)).unwrap_err();
        assert_eq!(
            (different.code(), different.phase()),
            (ErrorCode::AddressChanged, ErrorPhase::Address)
        );
        fs::write(root.path().join("copy"), b"changed owned contents").unwrap();
        let changed = directory
            .observe_child(std::ffi::OsStr::new("copy"))
            .unwrap();
        assert_eq!(changed.identity, expected.identity);
        let same_file = check_existing_observation(expected, Some(changed)).unwrap_err();
        assert_eq!(
            (same_file.code(), same_file.phase()),
            (ErrorCode::DestinationExists, ErrorPhase::Existing)
        );
        // A typed same-ID type inconsistency is a classifier control, not an OS
        // file-to-directory conversion or interference experiment.
        let mut inconsistent = original;
        inconsistent.kind = EntryKind::Special;
        inconsistent.file = None;
        let kind = check_existing_observation(expected, Some(inconsistent)).unwrap_err();
        assert_eq!(
            (kind.code(), kind.phase()),
            (ErrorCode::DestinationExists, ErrorPhase::Existing)
        );
        drop(directory);
        drop(context);
        root.close().unwrap();
    }
}
