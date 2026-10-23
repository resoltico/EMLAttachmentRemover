//! Candidate publication, independent effects and final qualification.
use super::*;

pub(super) fn no_cleanup() -> CleanupReceipt {
    CleanupReceipt {
        state: CleanupState::NotNeeded,
        residual_name: None,
        native_code: None,
    }
}
pub(super) fn new_copy_failure<'a>(
    error: OperationFailure,
    data: MessageMetadata<'a>,
    identity: Option<Identity>,
    file_sync: Option<eml_fs::SyncReceipt>,
    namespace_sync: NamespaceObservation,
    cleanup: CleanupReceipt,
) -> Box<ItemFailure<'a>> {
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
    item
}
pub(super) fn source_before_publication(source: &PreparedSource) -> Result<(), OperationFailure> {
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
pub(super) fn destination_still_absent(
    binding: &crate::binding::PublicationBinding,
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
pub(super) fn stage_failure(
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
pub(super) fn reconcile_held_effect(
    state: eml_fs::PublicationState,
    binding: &crate::binding::PublicationBinding,
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
pub(super) fn record_publication_address(
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
pub(super) fn verify_published_address(
    plan: &DestinationPlan<'_>,
    index: usize,
    binding: &crate::binding::PublicationBinding,
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
    let compared = qualify_published_file(&frame, info, identity, source, &finish)?;
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
pub(super) fn publish_item<'a>(
    plan: &DestinationPlan<'_>,
    index: usize,
    source: &'a PreparedSource,
    frame: crate::destination::ObservedRoute,
    context: &mut RunContext,
    control: &RunControl,
    data: MessageMetadata<'a>,
) -> ItemResult<'a> {
    let (mut binding, ready) =
        match prepare_candidate(plan, index, source, frame, context, control, data) {
            Ok(value) => value,
            Err(error) => return ItemResult::Failed(error),
        };
    let info = plan.records()[index]
        .destination()
        .expect("known destination");
    let identity = ready.identity();
    let file_sync = ready.file_sync();
    #[cfg(any(test, feature = "native-fixtures"))]
    context.observe(eml_fs::StoragePhase::Publication);
    let before_edge =
        check_publication_edge(plan, index, source, &binding, identity, context, control);
    if let Err(error) = before_edge {
        let namespace = binding.namespace();
        let cleanup =
            binding.cleanup_before_publication(ready.cleanup(), &mut context.pending_layout);
        return ItemResult::Failed(new_copy_failure(
            error,
            data,
            Some(identity),
            Some(file_sync),
            namespace,
            cleanup,
        ));
    }
    let attempt = ready.publish(info.basename());
    #[cfg(any(test, feature = "native-fixtures"))]
    context.observe(eml_fs::StoragePhase::AfterPublication);
    let (mut evidence, candidate) = PublicationEvidence::from_attempt(attempt, &binding, info);
    evidence.observe_effect(source, &mut binding, context);
    // Keep the candidate alive through final identity observations. Stop cannot roll back the edge.
    #[cfg(unix)]
    if evidence.state != eml_fs::PublicationState::NotPublished {
        evidence.record_address(plan, index, &binding, source);
    }
    #[cfg(windows)]
    let identity_pin = evidence.pin_identity(&binding, info);
    #[cfg(unix)]
    let cleanup = candidate.cleanup();
    #[cfg(windows)]
    let cleanup = {
        let (actual, cleanup) = candidate.cleanup();
        assert_eq!(actual, evidence.native_state);
        cleanup
    };
    if evidence.state == eml_fs::PublicationState::NotPublished {
        let namespace = binding.namespace();
        let cleanup = binding.cleanup_before_publication(cleanup, &mut context.pending_layout);
        return ItemResult::Failed(new_copy_failure(
            evidence.error.unwrap_or_else(|| {
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
        ));
    }
    evidence.record_cleanup(&cleanup);
    #[cfg(windows)]
    if let Some(pin) = identity_pin {
        evidence.release_identity(pin, plan, index, &binding, source);
    }
    evidence.into_result(data, binding.namespace(), cleanup)
}

fn qualify_published_file(
    frame: &crate::destination::ObservedRoute,
    info: &crate::DestinationInfo,
    identity: Identity,
    source: &PreparedSource,
    finish: &RunControl,
) -> Result<crate::ComparedExisting, OperationFailure> {
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
        compare_existing(source, file, finish).map_err(|error| match error.into_error() {
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
    Ok(compared)
}

fn prepare_candidate<'a>(
    plan: &DestinationPlan<'_>,
    index: usize,
    source: &'a PreparedSource,
    frame: crate::destination::ObservedRoute,
    context: &mut RunContext,
    control: &RunControl,
    data: MessageMetadata<'a>,
) -> Result<(crate::binding::PublicationBinding, eml_fs::ReadyCandidate), Box<ItemFailure<'a>>> {
    let binding = match crate::binding::PublicationBinding::acquire_for_create(
        plan,
        index,
        &frame,
        control,
        &mut context.pending_layout,
    ) {
        Ok(value) => value,
        Err(error) => {
            return Err(new_copy_failure(
                error.error,
                data,
                None,
                None,
                error.namespace,
                error.cleanup,
            ));
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
        return Err(new_copy_failure(
            error, data, None, None, namespace, cleanup,
        ));
    }
    let creation = match binding.creation_directory() {
        Ok(creation) => creation,
        Err(error) => {
            let namespace = binding.namespace();
            let cleanup =
                binding.cleanup_before_publication(no_cleanup(), &mut context.pending_layout);
            return Err(new_copy_failure(
                error, data, None, None, namespace, cleanup,
            ));
        }
    };
    let mut checkpoint_stopped = false;
    let checkpoint = || {
        if control.is_stopped() {
            checkpoint_stopped = true;
            Err(std::io::Error::from(std::io::ErrorKind::Interrupted))
        } else {
            Ok(())
        }
    };
    #[cfg(any(test, feature = "native-fixtures"))]
    let staged = creation.stage_bytes_observed(source.message().as_bytes(), checkpoint, |phase| {
        context.observe(phase)
    });
    #[cfg(not(any(test, feature = "native-fixtures")))]
    let staged = creation.stage_bytes(source.message().as_bytes(), checkpoint);
    let ready = match staged {
        Ok(value) => value,
        Err(error) => {
            let (error, sync, cleanup) = stage_failure(error, checkpoint_stopped);
            let namespace = binding.namespace();
            let cleanup = binding.cleanup_before_publication(cleanup, &mut context.pending_layout);
            return Err(new_copy_failure(
                error, data, None, sync, namespace, cleanup,
            ));
        }
    };
    Ok((binding, ready))
}

fn check_publication_edge(
    plan: &DestinationPlan<'_>,
    index: usize,
    source: &PreparedSource,
    binding: &crate::binding::PublicationBinding,
    identity: Identity,
    context: &RunContext,
    control: &RunControl,
) -> Result<(), OperationFailure> {
    let info = plan.records()[index]
        .destination()
        .expect("known destination");
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
        binding,
        info,
        &context.source_ids,
        &context.output_candidates,
    )?;
    checkpoint(control, ErrorPhase::Publish)
}

/// Observed facts while a real candidate continues to hold native identity.
/// Receipt qualification remains independent from whether publication happened.
struct PublicationEvidence {
    state: eml_fs::PublicationState,
    confirmed: bool,
    error: Option<OperationFailure>,
    address: AddressState,
    identity: Identity,
    file_sync: eml_fs::SyncReceipt,
    #[cfg(windows)]
    native_state: eml_fs::PublicationState,
}
impl PublicationEvidence {
    fn from_attempt(
        attempt: eml_fs::PublishAttempt,
        binding: &crate::binding::PublicationBinding,
        info: &crate::DestinationInfo,
    ) -> (Self, eml_fs::PublicationCandidate) {
        let collision = attempt.is_name_collision();
        let unsupported = attempt.is_unsupported();
        #[cfg(unix)]
        let (state, candidate, native_error) = (attempt.state, attempt.candidate, attempt.error);
        #[cfg(windows)]
        let (state, candidate, native_error) = {
            let state = attempt.state();
            let (candidate, error) = attempt.into_parts();
            (state, candidate, error)
        };
        #[cfg(windows)]
        let native_state = state;
        let identity = candidate.identity();
        let state = reconcile_held_effect(state, binding, identity, info.basename());
        let confirmed = state == eml_fs::PublicationState::Published;
        let error = native_error.map(|error| {
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
        let file_sync = candidate.file_sync();
        (
            Self {
                state,
                confirmed,
                error,
                address: AddressState::Unknown,
                identity,
                file_sync,
                #[cfg(windows)]
                native_state,
            },
            candidate,
        )
    }
    fn observe_effect(
        &mut self,
        source: &PreparedSource,
        binding: &mut crate::binding::PublicationBinding,
        context: &mut RunContext,
    ) {
        if self.state != eml_fs::PublicationState::NotPublished {
            context.output_candidates.insert(self.identity);
            #[cfg(any(test, feature = "native-fixtures"))]
            context.observe(eml_fs::StoragePhase::NamespaceSync);
            if let Err(failure) = binding.final_sync()
                && self.error.is_none()
            {
                self.error = Some(failure);
            }
            if let Err(failure) = source_before_publication(source)
                && self.error.is_none()
            {
                self.error = Some(failure);
            }
        }
    }
    fn record_address(
        &mut self,
        plan: &DestinationPlan<'_>,
        index: usize,
        binding: &crate::binding::PublicationBinding,
        source: &PreparedSource,
    ) {
        self.address = record_publication_address(
            verify_published_address(
                plan,
                index,
                binding,
                self.identity,
                source,
                &mut self.confirmed,
            ),
            &mut self.state,
            &mut self.error,
        );
    }
    #[cfg(windows)]
    fn pin_identity(
        &mut self,
        binding: &crate::binding::PublicationBinding,
        info: &crate::DestinationInfo,
    ) -> Option<eml_fs::EntryPin> {
        if self.state != eml_fs::PublicationState::NotPublished {
            match binding.directory().pin_child(info.basename()) {
                Ok(pin) => {
                    let observed = pin.observation();
                    if observed.identity == self.identity && observed.kind == EntryKind::File {
                        self.confirmed = true;
                    } else if self.error.is_none() {
                        self.error = Some(OperationFailure::new(
                            ErrorCode::AddressChanged,
                            ErrorPhase::Address,
                            "Published final name changed before identity could be retained",
                        ));
                    }
                    Some(pin)
                }
                Err(native) => {
                    if self.error.is_none() {
                        self.error = Some(OperationFailure::io(
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
        }
    }
    fn record_cleanup(&mut self, cleanup: &CleanupReceipt) {
        if (!matches!(
            cleanup.state,
            CleanupState::NotNeeded | CleanupState::Complete
        ) || cleanup.native_code.is_some())
            && self.error.is_none()
        {
            self.error = Some(OperationFailure::new(
                ErrorCode::CleanupFailed,
                ErrorPhase::Cleanup,
                "Owned staging cleanup or candidate release did not complete",
            ));
        }
    }
    #[cfg(windows)]
    fn release_identity(
        &mut self,
        pin: eml_fs::EntryPin,
        plan: &DestinationPlan<'_>,
        index: usize,
        binding: &crate::binding::PublicationBinding,
        source: &PreparedSource,
    ) {
        let observed = pin.observation();
        if observed.identity == self.identity && observed.kind == EntryKind::File {
            self.record_address(plan, index, binding, source);
        }
        if let Err(native) = pin.close()
            && self.error.is_none()
        {
            self.error = Some(OperationFailure::io(
                ErrorCode::CleanupFailed,
                ErrorPhase::Cleanup,
                "Final identity metadata handle could not be released",
                native,
            ));
        }
    }
    fn into_result<'a>(
        mut self,
        data: MessageMetadata<'a>,
        namespace: NamespaceObservation,
        cleanup: CleanupReceipt,
    ) -> ItemResult<'a> {
        if self.confirmed {
            self.state = eml_fs::PublicationState::Published;
        }
        if self.state == eml_fs::PublicationState::Published {
            let publication = crate::PublishedCopy::observed(
                self.identity,
                self.address,
                Some(self.file_sync),
                namespace,
                cleanup,
            );
            if self.error.is_none() {
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
            let mut item = failure(self.error.expect("known publication failure"));
            item.metadata = Some(data);
            item.effect = FailureEffect::Published(publication);
            ItemResult::Failed(item)
        } else {
            let mut item = failure(self.error.unwrap_or_else(|| {
                OperationFailure::new(
                    ErrorCode::WriteFailed,
                    ErrorPhase::Publish,
                    "Publication could not be confirmed",
                )
            }));
            item.metadata = Some(data);
            item.effect = FailureEffect::Unknown(crate::UnknownPublication::observed(
                Some(self.identity),
                self.address,
                Some(self.file_sync),
                namespace,
                cleanup,
            ));
            ItemResult::Failed(item)
        }
    }
}
