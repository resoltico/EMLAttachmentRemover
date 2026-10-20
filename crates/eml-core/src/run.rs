//! Serial native execution through the typed event/outcome boundary.
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
struct RunContext {
    source_ids: HashSet<Identity>,
    output_candidates: HashSet<Identity>,
    pending_layout: crate::binding::PendingLayout,
    admitted_removals: usize,
    #[cfg(any(test, feature = "native-fixtures"))]
    before_edge: Option<Box<dyn FnOnce()>>,
    #[cfg(any(test, feature = "native-fixtures"))]
    after_edge: Option<Box<dyn FnOnce()>>,
    #[cfg(all(test, windows))]
    before_sync_reopen: Option<Box<dyn FnOnce()>>,
}
impl RunContext {
    fn new(sources: &crate::CollectedSources<'_>) -> Self {
        Self {
            source_ids: source_identities(sources),
            output_candidates: HashSet::new(),
            pending_layout: crate::binding::PendingLayout::default(),
            admitted_removals: 0,
            #[cfg(any(test, feature = "native-fixtures"))]
            before_edge: None,
            #[cfg(any(test, feature = "native-fixtures"))]
            after_edge: None,
            #[cfg(all(test, windows))]
            before_sync_reopen: None,
        }
    }
}

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
fn admit_metadata<'a>(
    source: &'a PreparedSource,
    control: &RunControl,
    admitted: &mut usize,
) -> Result<MessageMetadata<'a>, Box<ItemFailure<'a>>> {
    checkpoint(control, ErrorPhase::Prepare).map_err(failure)?;
    let Some(total) = admitted
        .checked_add(source.message().removed().len())
        .filter(|total| *total <= MAX_REMOVAL_ENTRIES)
    else {
        return Err(failure(OperationFailure::new(
            ErrorCode::LimitExceeded,
            ErrorPhase::Prepare,
            "Run removal metadata exceeds its retained-entry budget",
        )));
    };
    *admitted = total;
    Ok(metadata(source))
}
fn execute_prepared<'a>(
    plan: &DestinationPlan<'_>,
    index: usize,
    source: &'a PreparedSource,
    context: &mut RunContext,
    control: &RunControl,
) -> ItemResult<'a> {
    let data = match admit_metadata(source, control, &mut context.admitted_removals) {
        Ok(data) => data,
        Err(error) => return ItemResult::Failed(error),
    };
    let inspection = match inspect_existing(
        plan,
        index,
        source,
        &context.source_ids,
        &context.output_candidates,
        control,
        data,
    ) {
        Ok(value) => value,
        Err(error) => return ItemResult::Failed(error),
    };
    if plan.sources().request.mode == RunMode::DryRun {
        return ItemResult::Planned(PlannedCopy {
            action: match &inspection {
                Inspection::Missing(_) => PlanAction::Create,
                Inspection::Matching(_) => PlanAction::VerifyExisting,
            },
            metadata: data,
            existing: inspection.existing(),
        });
    }
    match inspection {
        Inspection::Missing(frame) => {
            publish_item(plan, index, source, frame, context, control, data)
        }
        Inspection::Matching(matched) => {
            resynchronize_item(plan, index, source, *matched, context, control, data)
        }
    }
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
    run(request, control, sink)
}
/// Execute the complete serial native workflow for Create or DryRun.
/// All business decisions, terminal counts and checked event delivery live here;
/// adapters only capture requests, request stop and render these observations.
pub fn run(
    request: &RunRequest,
    control: &RunControl,
    sink: &mut impl EventSink,
) -> Result<RunOutcome, InvalidRequest> {
    run_inner(
        request,
        control,
        sink,
        #[cfg(any(test, feature = "native-fixtures"))]
        None,
    )
}

/// Nonshipping synchronization around an actual selected item's publication.
/// Hooks observe real operations; they supply no replacement events or receipts.
#[cfg(any(test, feature = "native-fixtures"))]
pub struct PublicationObservation {
    /// Zero-based collected item whose publication is observed.
    pub item_index: usize,
    /// After real candidate preparation, before the final publication guards.
    pub before: Option<Box<dyn FnOnce()>>,
    /// After the native publication attempt, before independent qualification.
    pub after: Option<Box<dyn FnOnce()>>,
}

/// Execute the same real engine with nonshipping publication synchronization.
#[cfg(feature = "native-fixtures")]
pub fn run_observed(
    request: &RunRequest,
    control: &RunControl,
    sink: &mut impl EventSink,
    observation: PublicationObservation,
) -> Result<RunOutcome, InvalidRequest> {
    run_inner(request, control, sink, Some(observation))
}

fn run_inner(
    request: &RunRequest,
    control: &RunControl,
    sink: &mut impl EventSink,
    #[cfg(any(test, feature = "native-fixtures"))] mut observation: Option<PublicationObservation>,
) -> Result<RunOutcome, InvalidRequest> {
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
    if !emit_start(request.mode == RunMode::DryRun, &mut outcome, control, sink) {
        return Ok(outcome);
    }
    let mut context = RunContext::new(plan.sources());
    for index in 0..total {
        #[cfg(any(test, feature = "native-fixtures"))]
        {
            // A refused/preparation-failed item must not carry its unused
            // publication observation into a later, publishable item.
            context.before_edge = None;
            context.after_edge = None;
        }
        #[cfg(any(test, feature = "native-fixtures"))]
        if observation
            .as_ref()
            .is_some_and(|hook| hook.item_index == index)
        {
            let hook = observation.take().expect("matched publication observation");
            context.before_edge = hook.before;
            context.after_edge = hook.after;
        }
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
        let result = execute_prepared(&plan, index, &source, &mut context, control);
        if !emit_item(&plan, index, result, &mut outcome, sink) {
            break;
        }
    }
    emit_summary(&mut outcome, control, sink);
    Ok(outcome)
}

mod existing;
use existing::*;
mod publication;
use publication::*;

#[cfg(test)]
mod tests;

fn emit_start(
    dry_run: bool,
    outcome: &mut RunOutcome,
    control: &RunControl,
    sink: &mut impl EventSink,
) -> bool {
    let start = RunStart {
        dry_run,
        total: outcome.total,
        collection_complete: outcome.collection_complete,
    };
    if let Err(error) = sink.emit(Event::Start(&start)) {
        outcome.delivery_failure = Some(DeliveryFailure {
            phase: DeliveryPhase::Start,
            error,
        });
        outcome.interrupted |= control.is_stopped();
        return false;
    }
    true
}

fn emit_summary(outcome: &mut RunOutcome, control: &RunControl, sink: &mut impl EventSink) {
    outcome.interrupted |= control.is_stopped();
    if outcome.delivery_failure.is_none()
        && let Err(error) = sink.emit(Event::Summary(outcome))
    {
        outcome.delivery_failure = Some(DeliveryFailure {
            phase: DeliveryPhase::Summary,
            error,
        });
        outcome.interrupted |= control.is_stopped();
    }
    // A successfully delivered terminal summary fixes the semantic outcome.
    // A later stop cannot retroactively disagree with that completed stream.
}
