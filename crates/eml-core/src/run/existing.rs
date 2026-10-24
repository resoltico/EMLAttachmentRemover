//! Existing-copy inspection and qualification.
use super::*;

pub(super) fn existing_error(error: ExistingComparisonError) -> OperationFailure {
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
pub(super) fn observation(
    identity: Identity,
    bytes_equal: Option<bool>,
    stable: bool,
) -> ExistingObservation {
    ExistingObservation {
        identity: Some(identity),
        bytes_equal,
        stable,
        address: AddressState::NotChecked,
        file_sync: None,
        namespace_sync: NamespaceObservation::unattempted(),
    }
}
pub(super) fn open_existing(
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
pub(super) struct MatchingInspection {
    pub(super) frame: crate::destination::ObservedRoute,
    pub(super) comparison: crate::ComparedExisting,
}
pub(super) enum Inspection {
    Missing(crate::destination::ObservedRoute),
    Matching(Box<MatchingInspection>),
}
impl Inspection {
    pub(super) fn existing(&self) -> Option<ExistingObservation> {
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
pub(super) fn inspect_existing<'a>(
    plan: &DestinationPlan<'_>,
    index: usize,
    source: &'a PreparedSource,
    context: &mut RunContext,
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
    let bound = bind_inspected_existing(
        &frame,
        info,
        entry,
        &context.source_ids,
        &context.output_candidates,
    )
    .map_err(|error| fail(error, initial))?;
    #[cfg(any(test, feature = "native-fixtures"))]
    context.observe(eml_fs::StoragePhase::Readback);
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
    let final_check = qualify_inspected_existing(
        plan,
        index,
        &frame,
        &compared,
        source,
        control,
        &mut observed,
    );
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
pub(super) fn check_existing_observation(
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
pub(super) fn comparison_failure(error: crate::ExistingComparisonFailure) -> OperationFailure {
    // Into-error preserves native causes without formatting source/header payload.
    existing_error(error.into_error())
}
pub(super) fn existing_failure<'a>(
    error: OperationFailure,
    data: MessageMetadata<'a>,
    observed: ExistingObservation,
) -> ItemResult<'a> {
    let mut item = failure(error);
    item.metadata = Some(data);
    item.existing = Some(observed);
    ItemResult::Failed(item)
}
pub(super) fn sync_access_error(error: OperationFailure) -> OperationFailure {
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
pub(super) fn final_address_error(error: OperationFailure) -> OperationFailure {
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
pub(super) fn guard_existing_identity(
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
pub(super) fn qualify_existing_address(
    plan: &DestinationPlan<'_>,
    index: usize,
    binding: &crate::binding::ExistingSyncBinding,
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
pub(super) fn resynchronize_item<'a>(
    plan: &DestinationPlan<'_>,
    index: usize,
    source: &'a PreparedSource,
    matched: MatchingInspection,
    context: &mut RunContext,
    control: &RunControl,
    data: MessageMetadata<'a>,
) -> ItemResult<'a> {
    #[cfg(all(test, windows))]
    {
        let mut step = eml_fs::existing_sync_evidence::Step::new("comparison_matched", "equal");
        step.signature = Some(matched.comparison.signature());
        step.parent = Some(matched.frame.directory.identity());
        step.bytes_equal = Some(matched.comparison.bytes_equal());
        observe_existing_sync(&mut context.existing_sync_evidence, source, step);
    }
    let mut observed = observation(matched.comparison.signature().identity, Some(true), true);
    let mut binding = match crate::binding::ExistingSyncBinding::acquire_existing(
        plan,
        index,
        &matched.frame,
        control,
        &mut context.pending_layout,
    ) {
        Ok(binding) => binding,
        Err(error) => {
            return existing_failure(sync_access_error(error), data, observed);
        }
    };
    observed.namespace_sync = binding.namespace();
    #[cfg(unix)]
    let compared = matched.comparison;
    #[cfg(windows)]
    let compared = {
        let info = plan.records()[index]
            .destination()
            .expect("known destination");
        match handoff_existing_for_sync(
            source,
            matched,
            &binding,
            info,
            context,
            control,
            &mut observed,
        ) {
            Ok(compared) => compared,
            Err(error) => return existing_failure(error, data, observed),
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
    #[cfg(any(test, feature = "native-fixtures"))]
    context.observe(eml_fs::StoragePhase::FileSync);
    if let Err(error) = synchronize_existing_storage(
        &mut binding,
        &compared,
        control,
        &mut observed,
        &mut context.pending_layout,
        #[cfg(any(test, feature = "native-fixtures"))]
        &mut context.observation,
        #[cfg(all(test, windows))]
        &mut context.existing_sync_evidence,
        #[cfg(all(test, windows))]
        source,
    ) {
        return existing_failure(error, data, observed);
    }
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
    #[cfg(all(test, windows))]
    {
        let mut step = eml_fs::existing_sync_evidence::Step::new("requalified", "succeeded");
        step.signature = Some(compared.signature());
        step.parent = Some(binding.directory().identity());
        step.bytes_equal = Some(compared.bytes_equal());
        observe_existing_sync(&mut context.existing_sync_evidence, source, step);
    }
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

fn bind_inspected_existing(
    frame: &crate::destination::ObservedRoute,
    info: &crate::DestinationInfo,
    entry: EntryObservation,
    ids: &HashSet<Identity>,
    reserved_outputs: &HashSet<Identity>,
) -> Result<BoundFile, OperationFailure> {
    if ids.contains(&entry.identity) {
        return Err(OperationFailure::new(
            ErrorCode::SourceDestinationAlias,
            ErrorPhase::Existing,
            "Existing output aliases a selected source",
        ));
    }
    if reserved_outputs.contains(&entry.identity) {
        return Err(OperationFailure::new(
            ErrorCode::DestinationCollision,
            ErrorPhase::Existing,
            "Existing output belongs to another item in this run",
        ));
    }
    if entry.kind != EntryKind::File {
        return Err(OperationFailure::new(
            ErrorCode::DestinationExists,
            ErrorPhase::Existing,
            "Existing output is not an ordinary regular file",
        ));
    }
    let bound = open_existing(&frame.directory, info)
        .map_err(|error| existing_error(ExistingComparisonError::Existing(error)))?;
    if Some(bound.initial_signature()) != entry.file {
        return Err(OperationFailure::new(
            ErrorCode::DestinationExists,
            ErrorPhase::Existing,
            "Existing output changed while opening",
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
        return Err(OperationFailure::new(
            code,
            ErrorPhase::Existing,
            "Opened existing output violates a run identity reservation",
        ));
    }
    Ok(bound)
}

fn qualify_inspected_existing(
    plan: &DestinationPlan<'_>,
    index: usize,
    frame: &crate::destination::ObservedRoute,
    compared: &crate::ComparedExisting,
    source: &PreparedSource,
    control: &RunControl,
    observed: &mut ExistingObservation,
) -> Result<(), OperationFailure> {
    let info = plan.records()[index]
        .destination()
        .expect("known destination");
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
}

#[cfg(windows)]
fn handoff_existing_for_sync(
    source: &PreparedSource,
    matched: MatchingInspection,
    binding: &crate::binding::ExistingSyncBinding,
    info: &crate::DestinationInfo,
    context: &mut RunContext,
    control: &RunControl,
    observed: &mut ExistingObservation,
) -> Result<crate::ComparedExisting, OperationFailure> {
    let original = matched.comparison.signature();
    if control.is_stopped() {
        return Err(interrupted(ErrorPhase::Existing));
    }
    if let Err(error) = matched.comparison.into_file().close() {
        return Err(OperationFailure::io(
            ErrorCode::ExistingSyncFailed,
            ErrorPhase::Existing,
            "Existing comparison holder could not be checked closed",
            error,
        ));
    }
    #[cfg(all(test, windows))]
    observe_existing_sync(
        &mut context.existing_sync_evidence,
        source,
        eml_fs::existing_sync_evidence::Step::new("comparison_closed", "succeeded"),
    );
    #[cfg(test)]
    if let Some(hook) = context.before_sync_reopen.take() {
        hook();
    }
    if control.is_stopped() {
        return Err(interrupted(ErrorPhase::Existing));
    }
    let fresh = match BoundFile::open_for_sync(info.native_path()) {
        Ok(file) => file,
        Err(error) => {
            return Err(match error {
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
            });
        }
    };
    #[cfg(all(test, windows))]
    {
        let mut step = eml_fs::existing_sync_evidence::Step::new("sync_opened", "succeeded");
        step.signature = Some(fresh.initial_signature());
        step.parent = Some(fresh.parent_identity());
        observe_existing_sync(&mut context.existing_sync_evidence, source, step);
    }
    if fresh.parent_identity() != matched.frame.directory.identity()
        || fresh.parent_identity() != binding.directory().identity()
    {
        observed.stable = false;
        observed.address = AddressState::Changed;
        return Err(OperationFailure::new(
            ErrorCode::AddressChanged,
            ErrorPhase::Address,
            "Existing parent changed during synchronization handoff",
        ));
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
        #[cfg(all(test, windows))]
        {
            let mut step = eml_fs::existing_sync_evidence::Step::new("binding_refused", "refused");
            step.signature = Some(signature);
            step.parent = Some(fresh.parent_identity());
            step.native_code = error
                .native_error()
                .and_then(|native| native.raw_os_error())
                .map(i64::from);
            observe_existing_sync(&mut context.existing_sync_evidence, source, step);
        }
        observed.stable = false;
        if error.code() == ErrorCode::AddressChanged {
            observed.address = AddressState::Changed;
        }
        return Err(error);
    }
    #[cfg(all(test, windows))]
    {
        let mut step = eml_fs::existing_sync_evidence::Step::new("binding_matched", "succeeded");
        step.signature = Some(signature);
        step.parent = Some(fresh.parent_identity());
        observe_existing_sync(&mut context.existing_sync_evidence, source, step);
    }
    #[cfg(all(test, windows))]
    let fresh_parent = fresh.parent_identity();
    guard_existing_identity(
        fresh.initial_signature().identity,
        &context.source_ids,
        &context.output_candidates,
        ErrorPhase::Existing,
    )?;
    match compare_existing(source, fresh, control) {
        Ok(compared) if compared.bytes_equal() => {
            #[cfg(all(test, windows))]
            {
                let mut step = eml_fs::existing_sync_evidence::Step::new("recompared", "equal");
                step.signature = Some(compared.signature());
                step.parent = Some(fresh_parent);
                step.bytes_equal = Some(compared.bytes_equal());
                observe_existing_sync(&mut context.existing_sync_evidence, source, step);
            }
            Ok(compared)
        }
        Ok(_) => {
            observed.bytes_equal = Some(false);
            Err(OperationFailure::new(
                ErrorCode::DestinationExists,
                ErrorPhase::Existing,
                "Existing bytes changed during synchronization handoff",
            ))
        }
        Err(error) => {
            observed.stable = false;
            Err(comparison_failure(error))
        }
    }
}

#[cfg_attr(
    all(test, windows),
    allow(
        clippy::too_many_arguments,
        reason = "Only the native unit fixture adds a finite evidence vector and the still-held source to the existing storage inputs."
    )
)]
fn synchronize_existing_storage(
    binding: &mut crate::binding::ExistingSyncBinding,
    compared: &crate::ComparedExisting,
    control: &RunControl,
    observed: &mut ExistingObservation,
    pending: &mut crate::binding::PendingLayout,
    #[cfg(any(test, feature = "native-fixtures"))] observer: &mut Option<
        Box<dyn FnMut(eml_fs::StoragePhase)>,
    >,
    #[cfg(all(test, windows))] evidence: &mut Option<Vec<eml_fs::existing_sync_evidence::Step>>,
    #[cfg(all(test, windows))] source: &PreparedSource,
) -> Result<(), OperationFailure> {
    checkpoint(control, ErrorPhase::Existing)?;
    let receipt = compared.sync_file();
    #[cfg(all(test, windows))]
    {
        let mut step = eml_fs::existing_sync_evidence::Step::new(
            "file_flush",
            if receipt.succeeded() {
                "succeeded"
            } else {
                "failed"
            },
        );
        step.native_code = receipt.native_code;
        step.sync = Some(receipt);
        observe_existing_sync(evidence, source, step);
    }
    observed.file_sync = Some(receipt);
    if !receipt.succeeded() {
        return Err(OperationFailure::new(
            ErrorCode::ExistingSyncFailed,
            ErrorPhase::Existing,
            "Existing copy file synchronization failed or is unsupported",
        ));
    }
    checkpoint(control, ErrorPhase::Existing)?;
    #[cfg(any(test, feature = "native-fixtures"))]
    if let Some(observe) = observer.as_mut() {
        observe(eml_fs::StoragePhase::NamespaceSync);
    }
    if let Err(error) = binding.final_sync(pending, control) {
        observed.namespace_sync = binding.namespace();
        return Err(sync_access_error(error));
    }
    observed.namespace_sync = binding.namespace();
    #[cfg(all(test, windows))]
    {
        let mut step = eml_fs::existing_sync_evidence::Step::new("endpoint_flush", "succeeded");
        step.parent = Some(binding.directory().identity());
        step.namespace = Some(observed.namespace_sync.fixture_json());
        observe_existing_sync(evidence, source, step);
    }
    Ok(())
}

#[cfg(all(test, windows))]
fn observe_existing_sync(
    evidence: &mut Option<Vec<eml_fs::existing_sync_evidence::Step>>,
    source: &PreparedSource,
    mut step: eml_fs::existing_sync_evidence::Step,
) {
    if let Some(evidence) = evidence {
        step.source = Some(
            source
                .fixture_source_observation()
                .expect("Observe the same held qualification source"),
        );
        evidence.push(step);
    }
}
