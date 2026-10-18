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
    #[cfg(test)]
    before_edge: Option<Box<dyn FnOnce()>>,
    #[cfg(test)]
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
            #[cfg(test)]
            before_edge: None,
            #[cfg(test)]
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
struct MatchingInspection {
    frame: crate::destination::ReadOnlyParent,
    comparison: crate::ComparedExisting,
}
enum Inspection {
    Missing(crate::destination::ReadOnlyParent),
    Matching(Box<MatchingInspection>),
}
impl Inspection {
    fn existing(&self) -> Option<ExistingObservation> {
        match self {
            Self::Missing(_) => None,
            Self::Matching(matched) => Some(observation(
                matched.comparison.signature().identity,
                Some(true),
                true,
            )),
        }
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
fn inspect_existing<'a>(
    plan: &DestinationPlan<'_>,
    index: usize,
    source: &'a PreparedSource,
    ids: &HashSet<Identity>,
    reserved_outputs: &HashSet<Identity>,
    control: &RunControl,
    data: MessageMetadata<'a>,
) -> Result<Inspection, Box<ItemFailure<'a>>> {
    let fail = |error, existing| {
        let mut item = failure(error);
        item.metadata = Some(data);
        item.existing = existing;
        item
    };
    checkpoint(control, ErrorPhase::Existing).map_err(|error| fail(error, None))?;
    let frame = plan
        .read_parent(index, control)
        .map_err(|error| fail(error, None))?;
    let info = plan.records()[index]
        .destination()
        .expect("known destination");
    let entry = if frame.complete {
        frame
            .directory
            .observe_child_if_present(info.basename())
            .map_err(|error| {
                fail(
                    OperationFailure::io(
                        ErrorCode::BindFailed,
                        ErrorPhase::Read,
                        "Existing output cannot be inspected",
                        error,
                    ),
                    None,
                )
            })?
    } else {
        None
    };
    let Some(entry) = entry else {
        source
            .revalidate()
            .map_err(|error| fail(source_error(error, ErrorPhase::Read), None))?;
        checkpoint(control, ErrorPhase::Existing).map_err(|error| fail(error, None))?;
        return Ok(Inspection::Missing(frame));
    };
    let initial = Some(observation(entry.identity, None, false));
    if ids.contains(&entry.identity) {
        return Err(fail(
            OperationFailure::new(
                ErrorCode::SourceDestinationAlias,
                ErrorPhase::Existing,
                "Existing output aliases a selected source",
            ),
            initial,
        ));
    }
    if reserved_outputs.contains(&entry.identity) {
        return Err(fail(
            OperationFailure::new(
                ErrorCode::DestinationCollision,
                ErrorPhase::Existing,
                "Existing output belongs to another item in this run",
            ),
            initial,
        ));
    }
    if entry.kind != EntryKind::File {
        return Err(fail(
            OperationFailure::new(
                ErrorCode::DestinationExists,
                ErrorPhase::Existing,
                "Existing output is not an ordinary regular file",
            ),
            initial,
        ));
    }
    let bound = open_existing(&frame.directory, info).map_err(|error| {
        fail(
            existing_error(ExistingComparisonError::Existing(error)),
            initial.clone(),
        )
    })?;
    if Some(bound.initial_signature()) != entry.file {
        return Err(fail(
            OperationFailure::new(
                ErrorCode::DestinationExists,
                ErrorPhase::Existing,
                "Existing output changed while opening",
            ),
            initial,
        ));
    }
    // Repeat aliases against the actual opened ordinary object, not only metadata.
    if ids.contains(&bound.initial_signature().identity)
        || reserved_outputs.contains(&bound.initial_signature().identity)
    {
        let code = if ids.contains(&bound.initial_signature().identity) {
            ErrorCode::SourceDestinationAlias
        } else {
            ErrorCode::DestinationCollision
        };
        return Err(fail(
            OperationFailure::new(
                code,
                ErrorPhase::Existing,
                "Opened existing output violates a run identity reservation",
            ),
            initial,
        ));
    }
    let compared = match compare_existing(source, bound, control) {
        Ok(value) => value,
        Err(error) => {
            let observed = observation(error.identity(), error.bytes_equal(), error.stable());
            return Err(fail(comparison_failure(error), Some(observed)));
        }
    };
    let mut observed = observation(
        compared.signature().identity,
        Some(compared.bytes_equal()),
        true,
    );
    let final_check = (|| {
        checkpoint(control, ErrorPhase::Existing)?;
        let current = plan.read_parent(index, control)?;
        if !current.complete || current.prefix != frame.prefix {
            return Err(OperationFailure::new(
                ErrorCode::AddressChanged,
                ErrorPhase::Address,
                "Existing output route changed after inspection",
            ));
        }
        let entry = current
            .directory
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
        check_existing_observation(compared.signature(), entry)
            .inspect_err(|_| observed.stable = false)?;
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
        return Err(fail(error, Some(observed)));
    }
    if !compared.bytes_equal() {
        return Err(fail(
            OperationFailure::new(
                ErrorCode::DestinationExists,
                ErrorPhase::Existing,
                "Existing output bytes differ from the verified copy",
            ),
            Some(observed),
        ));
    }
    Ok(Inspection::Matching(Box::new(MatchingInspection {
        frame,
        comparison: compared,
    })))
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
        dry_run: request.mode == RunMode::DryRun,
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
    let mut context = RunContext::new(plan.sources());
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
        let result = execute_prepared(&plan, index, &source, &mut context, control);
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

fn no_cleanup() -> CleanupReceipt {
    CleanupReceipt {
        state: CleanupState::NotNeeded,
        residual_name: None,
        native_code: None,
    }
}
fn new_copy_failure<'a>(
    error: OperationFailure,
    data: MessageMetadata<'a>,
    identity: Option<Identity>,
    file_sync: Option<eml_fs::SyncReceipt>,
    namespace_sync: NamespaceObservation,
    cleanup: CleanupReceipt,
) -> ItemResult<'a> {
    let mut item = failure(error);
    let address = match item.error.code() {
        ErrorCode::AddressChanged => AddressState::Changed,
        ErrorCode::AddressUnverified => AddressState::Unknown,
        _ => AddressState::NotChecked,
    };
    item.metadata = Some(data);
    item.effect = FailureEffect::NotPublished {
        candidate_identity: identity,
        address,
        file_sync,
        namespace_sync,
        cleanup,
    };
    ItemResult::Failed(item)
}
fn existing_failure<'a>(
    error: OperationFailure,
    data: MessageMetadata<'a>,
    observed: ExistingObservation,
) -> ItemResult<'a> {
    let mut item = failure(error);
    item.metadata = Some(data);
    item.existing = Some(observed);
    ItemResult::Failed(item)
}
fn sync_access_error(error: OperationFailure) -> OperationFailure {
    if matches!(
        error.code(),
        ErrorCode::BindFailed | ErrorCode::NamespaceSyncFailed
    ) {
        error.with_context(
            ErrorCode::ExistingSyncFailed,
            ErrorPhase::Existing,
            "Existing copy synchronization access or namespace operation failed",
        )
    } else {
        error
    }
}
fn final_address_error(error: OperationFailure) -> OperationFailure {
    match error.code() {
        ErrorCode::AddressChanged => error,
        ErrorCode::SourceChanged => error.with_context(
            ErrorCode::SourceChanged,
            ErrorPhase::Publish,
            "Source binding changed during final qualification",
        ),
        _ => error.with_context(
            ErrorCode::AddressUnverified,
            ErrorPhase::Address,
            "Requested copy address cannot be qualified",
        ),
    }
}
fn guard_existing_identity(
    identity: Identity,
    ids: &HashSet<Identity>,
    reserved: &HashSet<Identity>,
    phase: ErrorPhase,
) -> Result<(), OperationFailure> {
    let code = if ids.contains(&identity) {
        Some(ErrorCode::SourceDestinationAlias)
    } else if reserved.contains(&identity) {
        Some(ErrorCode::DestinationCollision)
    } else {
        None
    };
    if let Some(code) = code {
        Err(OperationFailure::new(
            code,
            phase,
            "Output violates a run identity reservation",
        ))
    } else {
        Ok(())
    }
}
fn source_before_publication(source: &PreparedSource) -> Result<(), OperationFailure> {
    source.revalidate().map_err(|error| match error {
        BoundFileError::Changed | BoundFileError::NotRegular => OperationFailure::new(
            ErrorCode::SourceChanged,
            ErrorPhase::Publish,
            "Source changed before or after the publication edge",
        ),
        BoundFileError::ObservationFailed(error) => OperationFailure::io(
            ErrorCode::ReadFailed,
            ErrorPhase::Read,
            "Source binding cannot be revalidated",
            error,
        ),
    })
}
fn qualify_existing_address(
    plan: &DestinationPlan<'_>,
    index: usize,
    binding: &crate::binding::DestinationBinding,
    compared: &crate::ComparedExisting,
    source: &PreparedSource,
    control: &RunControl,
    observed: &mut ExistingObservation,
) -> Result<(), OperationFailure> {
    checkpoint(control, ErrorPhase::Existing)?;
    binding.check_held(control, false)?;
    let frame = plan.read_parent(index, control)?;
    if !frame.complete || frame.prefix != binding.identities() {
        return Err(OperationFailure::new(
            ErrorCode::AddressChanged,
            ErrorPhase::Address,
            "Existing copy route differs from the retained destination",
        ));
    }
    let info = plan.records()[index]
        .destination()
        .expect("known destination");
    let entry = frame
        .directory
        .observe_child_if_present(info.basename())
        .map_err(|error| {
            observed.stable = false;
            OperationFailure::io(
                ErrorCode::AddressUnverified,
                ErrorPhase::Address,
                "Existing copy address cannot be inspected",
                error,
            )
        })?;
    check_existing_observation(compared.signature(), entry)
        .inspect_err(|_| observed.stable = false)?;
    compared.revalidate().map_err(|error| {
        observed.stable = false;
        existing_error(ExistingComparisonError::Existing(error))
    })?;
    source
        .revalidate()
        .map_err(|error| source_error(error, ErrorPhase::Read))?;
    checkpoint(control, ErrorPhase::Existing)
}
fn resynchronize_item<'a>(
    plan: &DestinationPlan<'_>,
    index: usize,
    source: &'a PreparedSource,
    matched: MatchingInspection,
    context: &mut RunContext,
    control: &RunControl,
    data: MessageMetadata<'a>,
) -> ItemResult<'a> {
    let mut observed = observation(matched.comparison.signature().identity, Some(true), true);
    let mut binding = match crate::binding::DestinationBinding::acquire(
        plan,
        index,
        &matched.frame,
        control,
        &mut context.pending_layout,
    ) {
        Ok(binding) => binding,
        Err(error) => {
            observed.namespace_sync = error.namespace;
            let mut result = existing_failure(sync_access_error(error.error), data, observed);
            if let ItemResult::Failed(item) = &mut result
                && let FailureEffect::NotPublished { cleanup, .. } = &mut item.effect
            {
                *cleanup = error.cleanup;
            }
            return result;
        }
    };
    observed.namespace_sync = binding.namespace();
    #[cfg(unix)]
    let compared = matched.comparison;
    #[cfg(windows)]
    let compared = {
        let original = matched.comparison.signature();
        if control.is_stopped() {
            return existing_failure(interrupted(ErrorPhase::Existing), data, observed);
        }
        if let Err(error) = matched.comparison.into_file().close() {
            return existing_failure(
                OperationFailure::io(
                    ErrorCode::ExistingSyncFailed,
                    ErrorPhase::Existing,
                    "Existing comparison holder could not be checked closed",
                    error,
                ),
                data,
                observed,
            );
        }
        #[cfg(test)]
        if let Some(hook) = context.before_sync_reopen.take() {
            hook();
        }
        if control.is_stopped() {
            return existing_failure(interrupted(ErrorPhase::Existing), data, observed);
        }
        let info = plan.records()[index]
            .destination()
            .expect("known destination");
        let fresh = match BoundFile::open_for_sync(info.native_path()) {
            Ok(file) => file,
            Err(error) => {
                return existing_failure(
                    match error {
                        BoundFileError::ObservationFailed(error) => OperationFailure::io(
                            ErrorCode::ExistingSyncFailed,
                            ErrorPhase::Existing,
                            "Existing copy synchronization access is unavailable",
                            error,
                        ),
                        _ => OperationFailure::new(
                            ErrorCode::DestinationExists,
                            ErrorPhase::Existing,
                            "Existing copy became unsupported during synchronization handoff",
                        ),
                    },
                    data,
                    observed,
                );
            }
        };
        if fresh.parent_identity() != matched.frame.directory.identity()
            || fresh.parent_identity() != binding.directory().identity()
        {
            observed.stable = false;
            observed.address = AddressState::Changed;
            return existing_failure(
                OperationFailure::new(
                    ErrorCode::AddressChanged,
                    ErrorPhase::Address,
                    "Existing parent changed during synchronization handoff",
                ),
                data,
                observed,
            );
        }
        let signature = fresh.initial_signature();
        if let Err(error) = check_existing_observation(
            original,
            Some(EntryObservation {
                identity: signature.identity,
                kind: EntryKind::File,
                file: Some(signature),
            }),
        ) {
            observed.stable = false;
            if error.code() == ErrorCode::AddressChanged {
                observed.address = AddressState::Changed;
            }
            return existing_failure(error, data, observed);
        }
        if let Err(error) = guard_existing_identity(
            fresh.initial_signature().identity,
            &context.source_ids,
            &context.output_candidates,
            ErrorPhase::Existing,
        ) {
            return existing_failure(error, data, observed);
        }
        match compare_existing(source, fresh, control) {
            Ok(compared) if compared.bytes_equal() => compared,
            Ok(_) => {
                observed.bytes_equal = Some(false);
                return existing_failure(
                    OperationFailure::new(
                        ErrorCode::DestinationExists,
                        ErrorPhase::Existing,
                        "Existing bytes changed during synchronization handoff",
                    ),
                    data,
                    observed,
                );
            }
            Err(error) => {
                observed.stable = false;
                return existing_failure(comparison_failure(error), data, observed);
            }
        }
    };
    if let Err(error) = guard_existing_identity(
        compared.signature().identity,
        &context.source_ids,
        &context.output_candidates,
        ErrorPhase::Existing,
    ) {
        return existing_failure(error, data, observed);
    }
    if let Err(error) = checkpoint(control, ErrorPhase::Existing) {
        return existing_failure(error, data, observed);
    }
    let receipt = compared.sync_file();
    observed.file_sync = Some(receipt);
    if receipt.state != eml_fs::SyncState::Succeeded {
        return existing_failure(
            OperationFailure::new(
                ErrorCode::ExistingSyncFailed,
                ErrorPhase::Existing,
                "Existing copy file synchronization failed or is unsupported",
            ),
            data,
            observed,
        );
    }
    if let Err(error) = checkpoint(control, ErrorPhase::Existing) {
        return existing_failure(error, data, observed);
    }
    if let Err(error) = binding.final_sync() {
        observed.namespace_sync = binding.namespace();
        return existing_failure(sync_access_error(error), data, observed);
    }
    observed.namespace_sync = binding.namespace();
    if let Err(error) = qualify_existing_address(
        plan,
        index,
        &binding,
        &compared,
        source,
        control,
        &mut observed,
    ) {
        match error.code() {
            ErrorCode::AddressChanged => observed.address = AddressState::Changed,
            ErrorCode::AddressUnverified => observed.address = AddressState::Unknown,
            _ => {}
        }
        return existing_failure(error, data, observed);
    }
    observed.address = AddressState::Verified;
    match crate::ExistingCopy::qualify(observed) {
        Ok(copy) => ItemResult::UpToDate(copy, data),
        Err(observed) => existing_failure(
            OperationFailure::new(
                ErrorCode::ExistingSyncFailed,
                ErrorPhase::Existing,
                "Existing copy did not meet every qualification requirement",
            ),
            data,
            observed,
        ),
    }
}
fn destination_still_absent(
    binding: &crate::binding::DestinationBinding,
    info: &crate::DestinationInfo,
    ids: &HashSet<Identity>,
    reserved: &HashSet<Identity>,
) -> Result<(), OperationFailure> {
    if let Some(entry) = binding
        .directory()
        .observe_child_if_present(info.basename())
        .map_err(|error| {
            OperationFailure::io(
                ErrorCode::BindFailed,
                ErrorPhase::Stage,
                "Final destination cannot be inspected",
                error,
            )
        })?
    {
        guard_existing_identity(entry.identity, ids, reserved, ErrorPhase::Publish)?;
        return Err(OperationFailure::new(
            ErrorCode::DestinationExists,
            ErrorPhase::Publish,
            "Another entry won the final destination before publication",
        ));
    }
    Ok(())
}
fn stage_failure(
    error: eml_fs::StageFailure,
    checkpoint_stopped: bool,
) -> (
    OperationFailure,
    Option<eml_fs::SyncReceipt>,
    CleanupReceipt,
) {
    let phase = match error.operation {
        eml_fs::StageOperation::Create => ErrorPhase::Stage,
        eml_fs::StageOperation::Write => ErrorPhase::Write,
        eml_fs::StageOperation::Sync => ErrorPhase::Sync,
    };
    let code = if checkpoint_stopped {
        ErrorCode::Interrupted
    } else {
        match error.operation {
            eml_fs::StageOperation::Create => ErrorCode::StageFailed,
            eml_fs::StageOperation::Write => ErrorCode::WriteFailed,
            eml_fs::StageOperation::Sync => ErrorCode::FileSyncFailed,
        }
    };
    (
        OperationFailure::io(
            code,
            phase,
            "Candidate storage did not complete its required operation",
            error.error,
        ),
        error.file_sync,
        error.cleanup,
    )
}
fn reconcile_held_effect(
    state: eml_fs::PublicationState,
    binding: &crate::binding::DestinationBinding,
    identity: Identity,
    name: &std::ffi::OsStr,
) -> eml_fs::PublicationState {
    if state == eml_fs::PublicationState::Unknown
        && binding
            .directory()
            .observe_child_if_present(name)
            .ok()
            .flatten()
            .is_some_and(|entry| entry.identity == identity)
    {
        eml_fs::PublicationState::Published
    } else {
        state
    }
}
fn record_publication_address(
    result: Result<(), OperationFailure>,
    state: &mut eml_fs::PublicationState,
    error: &mut Option<OperationFailure>,
) -> AddressState {
    match result {
        Ok(()) => {
            if *state == eml_fs::PublicationState::Unknown {
                *state = eml_fs::PublicationState::Published;
            }
            AddressState::Verified
        }
        Err(failure) => {
            let address = if failure.code() == ErrorCode::AddressChanged {
                AddressState::Changed
            } else {
                AddressState::Unknown
            };
            if error.is_none() {
                *error = Some(failure);
            }
            address
        }
    }
}
fn verify_published_address(
    plan: &DestinationPlan<'_>,
    index: usize,
    binding: &crate::binding::DestinationBinding,
    identity: Identity,
    source: &PreparedSource,
    confirmed: &mut bool,
) -> Result<(), OperationFailure> {
    let finish = RunControl::default();
    binding
        .check_held(&finish, true)
        .map_err(final_address_error)?;
    let frame = plan
        .read_parent(index, &finish)
        .map_err(final_address_error)?;
    if !frame.complete || frame.prefix != binding.identities() {
        return Err(OperationFailure::new(
            ErrorCode::AddressChanged,
            ErrorPhase::Address,
            "Published copy's requested directory address changed",
        ));
    }
    let info = plan.records()[index]
        .destination()
        .expect("known destination");
    let entry = frame
        .directory
        .observe_child_if_present(info.basename())
        .map_err(|error| {
            OperationFailure::io(
                ErrorCode::AddressUnverified,
                ErrorPhase::Address,
                "Published final name cannot be inspected",
                error,
            )
        })?;
    let Some(entry) = entry else {
        return Err(OperationFailure::new(
            if *confirmed {
                ErrorCode::AddressChanged
            } else {
                ErrorCode::AddressUnverified
            },
            ErrorPhase::Address,
            "Published final name is no longer present",
        ));
    };
    if entry.identity != identity {
        return Err(OperationFailure::new(
            if *confirmed {
                ErrorCode::AddressChanged
            } else {
                ErrorCode::AddressUnverified
            },
            ErrorPhase::Address,
            "Published final name now designates another object",
        ));
    }
    // Positive final candidate identity proves effect before later qualification.
    *confirmed = true;
    if entry.kind != EntryKind::File {
        return Err(OperationFailure::new(
            ErrorCode::AddressUnverified,
            ErrorPhase::Address,
            "Published object is no longer an ordinary file",
        ));
    }
    let file = open_existing(&frame.directory, info).map_err(|error| match error {
        BoundFileError::ObservationFailed(error) => OperationFailure::io(
            ErrorCode::AddressUnverified,
            ErrorPhase::Address,
            "Published file cannot be reopened without following links",
            error,
        ),
        _ => OperationFailure::new(
            ErrorCode::AddressUnverified,
            ErrorPhase::Address,
            "Published file could not be rebound as ordinary",
        ),
    })?;
    if file.initial_signature().identity != identity {
        return Err(OperationFailure::new(
            ErrorCode::AddressChanged,
            ErrorPhase::Address,
            "Published file changed while reopening",
        ));
    }
    let compared =
        compare_existing(source, file, &finish).map_err(|error| match error.into_error() {
            ExistingComparisonError::Source(
                BoundFileError::Changed | BoundFileError::NotRegular,
            ) => OperationFailure::new(
                ErrorCode::SourceChanged,
                ErrorPhase::Publish,
                "Source changed after publication",
            ),
            ExistingComparisonError::Source(BoundFileError::ObservationFailed(error))
            | ExistingComparisonError::Existing(BoundFileError::ObservationFailed(error))
            | ExistingComparisonError::Read(error) => OperationFailure::io(
                ErrorCode::AddressUnverified,
                ErrorPhase::Address,
                "Final source/copy qualification could not complete",
                error,
            ),
            _ => OperationFailure::new(
                ErrorCode::AddressUnverified,
                ErrorPhase::Address,
                "Final copy comparison could not be qualified",
            ),
        })?;
    if !compared.bytes_equal() {
        return Err(OperationFailure::new(
            ErrorCode::AddressUnverified,
            ErrorPhase::Address,
            "Published file no longer matches verified bytes",
        ));
    }
    let final_frame = plan
        .read_parent(index, &finish)
        .map_err(final_address_error)?;
    if !final_frame.complete || final_frame.prefix != frame.prefix {
        return Err(OperationFailure::new(
            ErrorCode::AddressChanged,
            ErrorPhase::Address,
            "Published address changed during final comparison",
        ));
    }
    let final_entry = final_frame
        .directory
        .observe_child_if_present(info.basename())
        .map_err(|error| {
            OperationFailure::io(
                ErrorCode::AddressUnverified,
                ErrorPhase::Address,
                "Published address could not be rechecked",
                error,
            )
        })?;
    check_existing_observation(compared.signature(), final_entry).map_err(final_address_error)?;
    compared.revalidate().map_err(|error| {
        final_address_error(existing_error(ExistingComparisonError::Existing(error)))
    })?;
    source_before_publication(source)?;
    Ok(())
}
fn publish_item<'a>(
    plan: &DestinationPlan<'_>,
    index: usize,
    source: &'a PreparedSource,
    frame: crate::destination::ReadOnlyParent,
    context: &mut RunContext,
    control: &RunControl,
    data: MessageMetadata<'a>,
) -> ItemResult<'a> {
    let mut binding = match crate::binding::DestinationBinding::acquire(
        plan,
        index,
        &frame,
        control,
        &mut context.pending_layout,
    ) {
        Ok(value) => value,
        Err(error) => {
            return new_copy_failure(
                error.error,
                data,
                None,
                None,
                error.namespace,
                error.cleanup,
            );
        }
    };
    let info = plan.records()[index]
        .destination()
        .expect("known destination");
    let before_stage = (|| {
        checkpoint(control, ErrorPhase::Stage)?;
        source_before_publication(source)?;
        destination_still_absent(
            &binding,
            info,
            &context.source_ids,
            &context.output_candidates,
        )
    })();
    if let Err(error) = before_stage {
        let namespace = binding.namespace();
        let cleanup = binding.cleanup_before_publication(no_cleanup(), &mut context.pending_layout);
        return new_copy_failure(error, data, None, None, namespace, cleanup);
    }
    let mut checkpoint_stopped = false;
    let ready = match binding
        .directory()
        .stage_bytes(source.message().as_bytes(), || {
            if control.is_stopped() {
                checkpoint_stopped = true;
                Err(std::io::Error::from(std::io::ErrorKind::Interrupted))
            } else {
                Ok(())
            }
        }) {
        Ok(value) => value,
        Err(error) => {
            let (error, sync, cleanup) = stage_failure(error, checkpoint_stopped);
            let namespace = binding.namespace();
            let cleanup = binding.cleanup_before_publication(cleanup, &mut context.pending_layout);
            return new_copy_failure(error, data, None, sync, namespace, cleanup);
        }
    };
    let identity = ready.identity();
    let file_sync = ready.file_sync();
    #[cfg(test)]
    if let Some(hook) = context.before_edge.take() {
        hook();
    }
    let before_edge = (|| {
        checkpoint(control, ErrorPhase::Publish)?;
        source_before_publication(source)?;
        binding.check_held(control, false)?;
        let current = plan.read_parent(index, control)?;
        if !current.complete || current.prefix != binding.identities() {
            return Err(OperationFailure::new(
                ErrorCode::AddressChanged,
                ErrorPhase::Address,
                "Destination route changed before publication",
            ));
        }
        guard_existing_identity(
            identity,
            &context.source_ids,
            &context.output_candidates,
            ErrorPhase::Publish,
        )?;
        destination_still_absent(
            &binding,
            info,
            &context.source_ids,
            &context.output_candidates,
        )?;
        checkpoint(control, ErrorPhase::Publish)
    })();
    if let Err(error) = before_edge {
        let namespace = binding.namespace();
        let cleanup =
            binding.cleanup_before_publication(ready.cleanup(), &mut context.pending_layout);
        return new_copy_failure(
            error,
            data,
            Some(identity),
            Some(file_sync),
            namespace,
            cleanup,
        );
    }
    let attempt = ready.publish(info.basename());
    #[cfg(test)]
    if let Some(hook) = context.after_edge.take() {
        hook();
    }
    let collision = attempt.is_name_collision();
    let unsupported = attempt.is_unsupported();
    #[cfg(unix)]
    let (mut state, candidate, native_error) = (attempt.state, attempt.candidate, attempt.error);
    #[cfg(windows)]
    let (mut state, candidate, native_error) = {
        let state = attempt.state();
        let (candidate, error) = attempt.into_parts();
        (state, candidate, error)
    };
    #[cfg(windows)]
    let native_state = state;
    state = reconcile_held_effect(state, &binding, identity, info.basename());
    let mut confirmed = state == eml_fs::PublicationState::Published;
    let mut error = native_error.map(|error| {
        OperationFailure::io(
            if collision {
                ErrorCode::DestinationExists
            } else if unsupported {
                ErrorCode::DestinationUnsupported
            } else {
                ErrorCode::WriteFailed
            },
            ErrorPhase::Publish,
            "Atomic no-replace publication returned a native failure",
            error,
        )
    });
    if state != eml_fs::PublicationState::NotPublished {
        context.output_candidates.insert(identity);
        if let Err(failure) = binding.final_sync()
            && error.is_none()
        {
            error = Some(failure);
        }
        if let Err(failure) = source_before_publication(source)
            && error.is_none()
        {
            error = Some(failure);
        }
    }
    // Final identity observations must happen while the candidate still pins
    // the object. Closing it first would allow a removed identity to be reused.
    // Once the edge succeeded or may have succeeded, no user stop rolls it back.
    let mut address = AddressState::Unknown;
    #[cfg(unix)]
    if state != eml_fs::PublicationState::NotPublished {
        address = record_publication_address(
            verify_published_address(plan, index, &binding, identity, source, &mut confirmed),
            &mut state,
            &mut error,
        );
    }
    // Windows intentionally denies data sharing on the candidate. A distinct
    // metadata file object holds its identity across release without retaining
    // that data-sharing restriction or allowing the numeric identity to recycle.
    #[cfg(windows)]
    let identity_pin = if state != eml_fs::PublicationState::NotPublished {
        match binding.directory().pin_child(info.basename()) {
            Ok(pin) => {
                let observed = pin.observation();
                if observed.identity == identity && observed.kind == EntryKind::File {
                    confirmed = true;
                } else if error.is_none() {
                    error = Some(OperationFailure::new(
                        ErrorCode::AddressChanged,
                        ErrorPhase::Address,
                        "Published final name changed before identity could be retained",
                    ));
                }
                Some(pin)
            }
            Err(native) => {
                if error.is_none() {
                    error = Some(OperationFailure::io(
                        ErrorCode::AddressUnverified,
                        ErrorPhase::Address,
                        "Published identity could not be retained across candidate release",
                        native,
                    ));
                }
                None
            }
        }
    } else {
        None
    };
    #[cfg(unix)]
    let cleanup = candidate.cleanup();
    #[cfg(windows)]
    let cleanup = {
        let (actual, cleanup) = candidate.cleanup();
        assert_eq!(actual, native_state);
        cleanup
    };
    if state == eml_fs::PublicationState::NotPublished {
        let namespace = binding.namespace();
        let cleanup = binding.cleanup_before_publication(cleanup, &mut context.pending_layout);
        return new_copy_failure(
            error.unwrap_or_else(|| {
                OperationFailure::new(
                    ErrorCode::WriteFailed,
                    ErrorPhase::Publish,
                    "Publication did not occur",
                )
            }),
            data,
            Some(identity),
            Some(file_sync),
            namespace,
            cleanup,
        );
    }
    if (!matches!(
        cleanup.state,
        CleanupState::NotNeeded | CleanupState::Complete
    ) || cleanup.native_code.is_some())
        && error.is_none()
    {
        error = Some(OperationFailure::new(
            ErrorCode::CleanupFailed,
            ErrorPhase::Cleanup,
            "Owned staging cleanup or candidate release did not complete",
        ));
    }
    #[cfg(windows)]
    if let Some(pin) = identity_pin {
        let observed = pin.observation();
        if observed.identity == identity && observed.kind == EntryKind::File {
            address = record_publication_address(
                verify_published_address(plan, index, &binding, identity, source, &mut confirmed),
                &mut state,
                &mut error,
            );
        }
        if let Err(native) = pin.close()
            && error.is_none()
        {
            error = Some(OperationFailure::io(
                ErrorCode::CleanupFailed,
                ErrorPhase::Cleanup,
                "Final identity metadata handle could not be released",
                native,
            ));
        }
    }
    if confirmed {
        state = eml_fs::PublicationState::Published;
    }
    let namespace = binding.namespace();
    if state == eml_fs::PublicationState::Published {
        let publication =
            crate::PublishedCopy::observed(identity, address, Some(file_sync), namespace, cleanup);
        if error.is_none() {
            match crate::CreatedCopy::qualify(publication) {
                Ok(copy) => return ItemResult::Created(copy, data),
                Err(publication) => {
                    let mut item = failure(OperationFailure::new(
                        ErrorCode::AddressUnverified,
                        ErrorPhase::Address,
                        "Published copy did not meet every qualification requirement",
                    ));
                    item.metadata = Some(data);
                    item.effect = FailureEffect::Published(*publication);
                    return ItemResult::Failed(item);
                }
            }
        }
        let mut item = failure(error.expect("known publication failure"));
        item.metadata = Some(data);
        item.effect = FailureEffect::Published(publication);
        ItemResult::Failed(item)
    } else {
        let mut item = failure(error.unwrap_or_else(|| {
            OperationFailure::new(
                ErrorCode::WriteFailed,
                ErrorPhase::Publish,
                "Publication could not be confirmed",
            )
        }));
        item.metadata = Some(data);
        item.effect = FailureEffect::Unknown(crate::UnknownPublication::observed(
            Some(identity),
            address,
            Some(file_sync),
            namespace,
            cleanup,
        ));
        ItemResult::Failed(item)
    }
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
    #[test]
    fn stage_checkpoint_origin_is_distinct_from_a_native_interrupted_error() {
        for checkpoint_stopped in [false, true] {
            let sync = eml_fs::SyncReceipt {
                method: eml_fs::SyncMethod::Fsync,
                state: eml_fs::SyncState::Succeeded,
                native_code: None,
            };
            let original = eml_fs::StageFailure {
                operation: eml_fs::StageOperation::Write,
                error: std::io::Error::new(
                    std::io::ErrorKind::Interrupted,
                    "Scripted operation interruption",
                ),
                file_sync: Some(sync),
                cleanup: CleanupReceipt {
                    state: CleanupState::Residual,
                    residual_name: Some(".owned-stage".into()),
                    native_code: Some(5),
                },
            };
            let (error, receipt, cleanup) = stage_failure(original, checkpoint_stopped);
            assert_eq!(
                error.code(),
                if checkpoint_stopped {
                    ErrorCode::Interrupted
                } else {
                    ErrorCode::WriteFailed
                }
            );
            assert_eq!(error.phase(), ErrorPhase::Write);
            assert_eq!(receipt, Some(sync));
            assert_eq!(cleanup.state, CleanupState::Residual);
            assert_eq!(cleanup.native_code, Some(5));
            assert_eq!(
                error.native_error().unwrap().kind(),
                std::io::ErrorKind::Interrupted
            );
        }
    }
    #[cfg(unix)]
    #[test]
    fn modeled_failed_creation_residue_requires_namespace_work_before_a_later_real_copy() {
        let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/run-tests");
        fs::create_dir_all(&base).unwrap();
        let root = tempfile::tempdir_in(base).unwrap();
        fs::create_dir(root.path().join("Inbox")).unwrap();
        fs::create_dir(root.path().join("out")).unwrap();
        let input = include_bytes!("../../../tests/contract/mime/inputs/01-gmail-attachment.eml");
        let golden = include_bytes!("../../../tests/contract/mime/golden/01-gmail-attachment.eml");
        fs::write(root.path().join("Inbox/a.eml"), input).unwrap();
        let request = RunRequest {
            paths: vec!["Inbox".into()],
            working_directory: root.path().to_owned(),
            output: crate::OutputLocation::Directory("out".into()),
            mode: RunMode::Create,
        };
        let control = RunControl::default();
        let plan = plan_destinations(collect_sources(&request, &control).unwrap(), &control);
        // Actual owned layout models the residue; the failure receipt is a
        // classifier input, not a claim of an injected native mkdir failure.
        fs::create_dir(root.path().join("out/Inbox")).unwrap();
        let parent = plan.rebind_root(0, &control).unwrap();
        let mut context = RunContext::new(plan.sources());
        context.pending_layout.failed_creation(
            parent.identity(),
            std::ffi::OsStr::new("Inbox"),
            &CleanupReceipt {
                state: CleanupState::Residual,
                residual_name: None,
                native_code: None,
            },
        );
        let source = bind_source(&plan, 0, &control)
            .unwrap_or_else(|error| panic!("Owned source failed: {error:?}"));
        let result = execute_prepared(&plan, 0, &source, &mut context, &control);
        let ItemResult::Created(copy, _) = &result else {
            panic!("Later real copy did not qualify: {result:?}");
        };
        assert_eq!(copy.publication().namespace_sync().required_edges, 3);
        assert_eq!(copy.publication().namespace_sync().completed_edges, 3);
        assert_eq!(
            fs::read(root.path().join("out/Inbox/a.no-attachments.eml")).unwrap(),
            golden
        );
        assert_eq!(fs::read(root.path().join("Inbox/a.eml")).unwrap(), input);
        drop(result);
        drop(source);
        drop(parent);
        drop(plan);
        root.close().unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn real_held_publication_witness_survives_modeled_ambiguity_and_changed_requested_address() {
        let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/run-tests");
        fs::create_dir_all(&base).unwrap();
        let root = tempfile::tempdir_in(base).unwrap();
        let input = include_bytes!("../../../tests/contract/mime/inputs/01-gmail-attachment.eml");
        let golden = include_bytes!("../../../tests/contract/mime/golden/01-gmail-attachment.eml");
        fs::write(root.path().join("a.eml"), input).unwrap();
        fs::create_dir(root.path().join("out")).unwrap();
        let request = RunRequest {
            paths: vec!["a.eml".into()],
            working_directory: root.path().to_owned(),
            output: crate::OutputLocation::Directory("out".into()),
            mode: RunMode::Create,
        };
        let control = RunControl::default();
        let plan = plan_destinations(collect_sources(&request, &control).unwrap(), &control);
        let source = bind_source(&plan, 0, &control)
            .unwrap_or_else(|error| panic!("Owned source failed: {error:?}"));
        let frame = plan.read_parent(0, &control).unwrap();
        let mut pending = crate::binding::PendingLayout::default();
        let mut binding =
            crate::binding::DestinationBinding::acquire(&plan, 0, &frame, &control, &mut pending)
                .unwrap_or_else(|_| panic!("Owned destination failed"));
        let info = plan.records()[0].destination().unwrap();
        let ready = binding
            .directory()
            .stage_bytes(source.message().as_bytes(), || Ok(()))
            .unwrap();
        let identity = ready.identity();
        let actual = ready.publish(info.basename());
        assert_eq!(actual.state, eml_fs::PublicationState::Published);
        assert!(actual.error.is_none());
        binding.final_sync().unwrap();
        fs::rename(root.path().join("out"), root.path().join("moved")).unwrap();
        fs::create_dir(root.path().join("out")).unwrap();
        // Only the ambiguous native return is modeled. Publication and address
        // movement above are actual owned filesystem operations.
        let effect = reconcile_held_effect(
            eml_fs::PublicationState::Unknown,
            &binding,
            identity,
            info.basename(),
        );
        assert_eq!(effect, eml_fs::PublicationState::Published);
        assert_eq!(actual.candidate.cleanup().state, CleanupState::Complete);
        let mut confirmed = true;
        let error = verify_published_address(&plan, 0, &binding, identity, &source, &mut confirmed)
            .unwrap_err();
        assert_eq!(error.code(), ErrorCode::AddressChanged);
        assert!(confirmed);
        assert_eq!(
            fs::read(root.path().join("moved/a.no-attachments.eml")).unwrap(),
            golden
        );
        assert_eq!(fs::read_dir(root.path().join("out")).unwrap().count(), 0);
        drop(binding);
        drop(frame);
        drop(source);
        drop(plan);
        root.close().unwrap();
    }
    #[test]
    fn controlled_stop_before_and_after_the_real_edge_preserves_actual_effect() {
        for after in [false, true] {
            let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/run-tests");
            fs::create_dir_all(&base).unwrap();
            let root = tempfile::tempdir_in(base).unwrap();
            let input =
                include_bytes!("../../../tests/contract/mime/inputs/01-gmail-attachment.eml");
            let golden =
                include_bytes!("../../../tests/contract/mime/golden/01-gmail-attachment.eml");
            fs::write(root.path().join("a.eml"), input).unwrap();
            let request = RunRequest {
                paths: vec!["a.eml".into()],
                working_directory: root.path().to_owned(),
                output: crate::OutputLocation::BesideSource,
                mode: RunMode::Create,
            };
            let control = RunControl::default();
            let plan = plan_destinations(collect_sources(&request, &control).unwrap(), &control);
            let source = bind_source(&plan, 0, &control)
                .unwrap_or_else(|error| panic!("Source failed: {error:?}"));
            let mut context = RunContext::new(plan.sources());
            let signal = control.clone();
            let hook = Box::new(move || signal.request_stop());
            if after {
                context.after_edge = Some(hook);
            } else {
                context.before_edge = Some(hook);
            }
            let result = execute_prepared(&plan, 0, &source, &mut context, &control);
            assert!(control.is_stopped());
            if after {
                assert!(matches!(result, ItemResult::Created(..)), "{result:?}");
                assert_eq!(
                    fs::read(root.path().join("a.no-attachments.eml")).unwrap(),
                    golden
                );
            } else {
                let ItemResult::Failed(failure) = &result else {
                    panic!("Pre-edge stop published: {result:?}");
                };
                assert_eq!(
                    (failure.error().code(), failure.error().phase()),
                    (ErrorCode::Interrupted, ErrorPhase::Publish)
                );
                let FailureEffect::NotPublished {
                    file_sync, cleanup, ..
                } = failure.effect()
                else {
                    panic!("Wrong stop effect");
                };
                assert_eq!(file_sync.unwrap().state, eml_fs::SyncState::Succeeded);
                assert_eq!(cleanup.state, CleanupState::Complete);
                assert!(!root.path().join("a.no-attachments.eml").exists());
            }
            assert_eq!(fs::read(root.path().join("a.eml")).unwrap(), input);
            assert_eq!(
                fs::read_dir(root.path()).unwrap().count(),
                if after { 2 } else { 1 }
            );
            drop(result);
            drop(source);
            drop(plan);
            root.close().unwrap();
        }
    }
    #[cfg(unix)]
    #[test]
    fn controlled_source_change_before_and_after_edge_keeps_publication_truth() {
        for after in [false, true] {
            let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/run-tests");
            fs::create_dir_all(&base).unwrap();
            let root = tempfile::tempdir_in(base).unwrap();
            let input =
                include_bytes!("../../../tests/contract/mime/inputs/01-gmail-attachment.eml");
            let golden =
                include_bytes!("../../../tests/contract/mime/golden/01-gmail-attachment.eml");
            let source_path = root.path().join("a.eml");
            fs::write(&source_path, input).unwrap();
            let request = RunRequest {
                paths: vec!["a.eml".into()],
                working_directory: root.path().to_owned(),
                output: crate::OutputLocation::BesideSource,
                mode: RunMode::Create,
            };
            let control = RunControl::default();
            let plan = plan_destinations(collect_sources(&request, &control).unwrap(), &control);
            let source = bind_source(&plan, 0, &control)
                .unwrap_or_else(|error| panic!("Source failed: {error:?}"));
            let mut context = RunContext::new(plan.sources());
            let changed = source_path.clone();
            let hook = Box::new(move || {
                fs::write(changed, b"controlled changed owned source").unwrap();
            });
            if after {
                context.after_edge = Some(hook);
            } else {
                context.before_edge = Some(hook);
            }
            let result = execute_prepared(&plan, 0, &source, &mut context, &control);
            let ItemResult::Failed(failure) = &result else {
                panic!("Changed source qualified: {result:?}");
            };
            assert_eq!(
                (failure.error().code(), failure.error().phase()),
                (ErrorCode::SourceChanged, ErrorPhase::Publish)
            );
            if after {
                assert!(matches!(failure.effect(), FailureEffect::Published(_)));
                assert_eq!(
                    fs::read(root.path().join("a.no-attachments.eml")).unwrap(),
                    golden
                );
            } else {
                assert!(matches!(
                    failure.effect(),
                    FailureEffect::NotPublished { .. }
                ));
                assert!(!root.path().join("a.no-attachments.eml").exists());
            }
            assert_eq!(
                fs::read(&source_path).unwrap(),
                b"controlled changed owned source"
            );
            drop(result);
            drop(source);
            drop(plan);
            root.close().unwrap();
        }
    }
    #[cfg(unix)]
    #[test]
    fn controlled_requested_output_move_after_edge_keeps_complete_published_copy() {
        let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/run-tests");
        fs::create_dir_all(&base).unwrap();
        let root = tempfile::tempdir_in(base).unwrap();
        let input = include_bytes!("../../../tests/contract/mime/inputs/01-gmail-attachment.eml");
        let golden = include_bytes!("../../../tests/contract/mime/golden/01-gmail-attachment.eml");
        fs::write(root.path().join("a.eml"), input).unwrap();
        let output = root.path().join("out");
        let moved = root.path().join("moved");
        fs::create_dir(&output).unwrap();
        let request = RunRequest {
            paths: vec!["a.eml".into()],
            working_directory: root.path().to_owned(),
            output: crate::OutputLocation::Directory("out".into()),
            mode: RunMode::Create,
        };
        let control = RunControl::default();
        let plan = plan_destinations(collect_sources(&request, &control).unwrap(), &control);
        let source = bind_source(&plan, 0, &control)
            .unwrap_or_else(|error| panic!("Source failed: {error:?}"));
        let mut context = RunContext::new(plan.sources());
        let (from, to) = (output.clone(), moved.clone());
        context.after_edge = Some(Box::new(move || {
            fs::rename(&from, &to).unwrap();
            fs::create_dir(from).unwrap();
        }));
        let result = execute_prepared(&plan, 0, &source, &mut context, &control);
        let ItemResult::Failed(failure) = &result else {
            panic!("Moved address qualified: {result:?}");
        };
        assert_eq!(failure.error().code(), ErrorCode::AddressChanged);
        let FailureEffect::Published(copy) = failure.effect() else {
            panic!("Published effect lost");
        };
        assert_eq!(copy.address(), AddressState::Changed);
        assert_eq!(
            copy.file_sync().unwrap().state,
            eml_fs::SyncState::Succeeded
        );
        assert_eq!(
            fs::read(moved.join("a.no-attachments.eml")).unwrap(),
            golden
        );
        assert_eq!(fs::read_dir(&output).unwrap().count(), 0);
        assert_eq!(fs::read(root.path().join("a.eml")).unwrap(), input);
        drop(result);
        drop(source);
        drop(plan);
        root.close().unwrap();
    }
    #[cfg(windows)]
    #[test]
    fn controlled_resync_gap_distinguishes_file_replacement_and_same_id_change() {
        for replace in [false, true] {
            let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/run-tests");
            fs::create_dir_all(&base).unwrap();
            let root = tempfile::tempdir_in(base).unwrap();
            let input =
                include_bytes!("../../../tests/contract/mime/inputs/01-gmail-attachment.eml");
            let golden =
                include_bytes!("../../../tests/contract/mime/golden/01-gmail-attachment.eml");
            fs::write(root.path().join("a.eml"), input).unwrap();
            let copy = root.path().join("a.no-attachments.eml");
            fs::write(&copy, golden).unwrap();
            let request = RunRequest {
                paths: vec!["a.eml".into()],
                working_directory: root.path().to_owned(),
                output: crate::OutputLocation::BesideSource,
                mode: RunMode::Create,
            };
            let control = RunControl::default();
            let plan = plan_destinations(collect_sources(&request, &control).unwrap(), &control);
            let source = bind_source(&plan, 0, &control)
                .unwrap_or_else(|error| panic!("Source failed: {error:?}"));
            let mut context = RunContext::new(plan.sources());
            let old = match plan.records()[0].destination().unwrap().observation() {
                crate::DestinationObservation::Present(entry) => entry.identity,
                _ => panic!("Owned copy missing"),
            };
            let changed = copy.clone();
            let working = root.path().to_owned();
            context.before_sync_reopen = Some(Box::new(move || {
                if replace {
                    fs::remove_file(&changed).unwrap();
                    fs::write(&changed, golden).unwrap();
                } else {
                    fs::write(&changed, b"controlled changed copy").unwrap();
                }
                let paths = eml_fs::PathContext::capture(&working).unwrap();
                let now = eml_fs::observe_selected(&paths.resolve(&changed).unwrap()).unwrap();
                assert_eq!(now.identity != old, replace);
            }));
            let result = execute_prepared(&plan, 0, &source, &mut context, &control);
            let ItemResult::Failed(failure) = &result else {
                panic!("Gap change qualified: {result:?}");
            };
            assert_eq!(
                failure.error().code(),
                if replace {
                    ErrorCode::AddressChanged
                } else {
                    ErrorCode::DestinationExists
                }
            );
            let observed = failure.existing().unwrap();
            assert!(!observed.stable);
            assert_eq!(observed.bytes_equal, Some(true));
            assert!(observed.file_sync.is_none());
            assert_eq!(
                observed.address,
                if replace {
                    AddressState::Changed
                } else {
                    AddressState::NotChecked
                }
            );
            assert_eq!(fs::read(root.path().join("a.eml")).unwrap(), input);
            drop(result);
            drop(source);
            drop(plan);
            root.close().unwrap();
        }
    }
}
