//! Field projection for independently validated records; core owns interpretation.
use crate::records::*;
use eml_core::meaning::{ExistingFacts, OutcomeFacts, Publication as Effect, ResultKind};

/// Convert the closed transport vocabulary to the core's semantic category.
pub const fn removal_reason(reason: Removal) -> eml_core::RemovalReason {
    match reason {
        Removal::Attachment => eml_core::RemovalReason::Attachment,
        Removal::RelatedResource => eml_core::RemovalReason::RelatedResource,
        Removal::NonBodyPart => eml_core::RemovalReason::NonBodyPart,
    }
}

fn address(value: Address) -> eml_core::AddressState {
    match value {
        Address::NotChecked => eml_core::AddressState::NotChecked,
        Address::Verified => eml_core::AddressState::Verified,
        Address::Changed => eml_core::AddressState::Changed,
        Address::Unknown => eml_core::AddressState::Unknown,
    }
}
fn sync(
    method: SynchronizationMethod,
    state: Synchronization,
    code: &Nullable<i64>,
) -> Option<eml_core::SyncReceipt> {
    Some(eml_core::SyncReceipt {
        method: match method {
            SynchronizationMethod::NotRequested => return None,
            SynchronizationMethod::Fsync => eml_core::SyncMethod::Fsync,
            SynchronizationMethod::FsyncFullfsync => eml_core::SyncMethod::FsyncFullfsync,
            SynchronizationMethod::FsyncFullfsyncUnsupported => {
                eml_core::SyncMethod::FsyncFullfsyncUnsupported
            }
            SynchronizationMethod::FlushFileBuffers => eml_core::SyncMethod::FlushFileBuffers,
        },
        state: match state {
            Synchronization::NotAttempted => return None,
            Synchronization::Succeeded => eml_core::SyncState::Succeeded,
            Synchronization::Unsupported => eml_core::SyncState::Unsupported,
            Synchronization::Failed => eml_core::SyncState::Failed,
        },
        native_code: code.value().copied(),
    })
}
fn namespace(value: &NamespaceRecord) -> Option<eml_core::NamespaceObservation> {
    Some(eml_core::NamespaceObservation {
        receipt: sync(value.method, value.state, &value.native_code),
        required_edges: value.required_edges.try_into().ok()?,
        completed_edges: value.completed_edges.try_into().ok()?,
        failed_edge_index: value
            .failed_edge_index
            .value()
            .map(|i| u16::try_from(*i))
            .transpose()
            .ok()?,
    })
}
fn cleanup(value: &CleanupRecord) -> Option<eml_core::CleanupReceipt> {
    use base64::Engine;
    let residual_name = match value.residual_name.value() {
        None => None,
        Some(name) => Some({
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(&name.base64)
                .ok()?;
            #[cfg(unix)]
            {
                use std::os::unix::ffi::OsStringExt;
                (name.encoding == PathEncoding::UnixBytes)
                    .then(|| std::ffi::OsString::from_vec(bytes))
            }
            #[cfg(windows)]
            {
                use std::os::windows::ffi::OsStringExt;
                if name.encoding != PathEncoding::WindowsUtf16Le || bytes.len() % 2 != 0 {
                    return None;
                }
                Some(std::ffi::OsString::from_wide(
                    &bytes
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .copied()
                        .map(u16::from_le_bytes)
                        .collect::<Vec<_>>(),
                ))
            }
        }?),
    };
    Some(eml_core::CleanupReceipt {
        state: match value.state {
            Cleanup::NotNeeded => eml_core::CleanupState::NotNeeded,
            Cleanup::Complete => eml_core::CleanupState::Complete,
            Cleanup::Residual => eml_core::CleanupState::Residual,
            Cleanup::Unknown => eml_core::CleanupState::Unknown,
        },
        residual_name,
        native_code: value.native_code.value().copied(),
    })
}
/// Map accepted wire facts to domain display facts, without classification.
///
/// The caller must first perform independent stream/receipt validation. This
/// function cannot validate a run, construct success capabilities or authorize I/O.
/// Unrepresentable native facts fail the projection instead of acquiring defaults.
pub fn outcome_facts(item: &ItemRecord) -> Option<OutcomeFacts> {
    let p = &item.publication;
    Some(OutcomeFacts {
        result: match item.status {
            ItemStatus::Created => ResultKind::Created,
            ItemStatus::UpToDate => ResultKind::Existing,
            ItemStatus::Planned => ResultKind::Planned,
            ItemStatus::Failed => ResultKind::Failed,
        },
        plan: item.plan.value().map(|p| match p {
            Plan::Create => eml_core::PlanAction::Create,
            Plan::VerifyExisting => eml_core::PlanAction::VerifyExisting,
        }),
        publication: match p.state {
            Publication::NotPublished => Effect::NotPublished,
            Publication::Published => Effect::Published,
            Publication::Unknown => Effect::Unknown,
        },
        address: address(p.address),
        candidate_identity_known: p.candidate_identity.value().is_some(),
        // A validated Created record carries the producer's full successful
        // qualification contract, including exact readback. Failed records do not.
        final_readback: (item.status == ItemStatus::Created).then_some(true),
        file_sync: sync(
            p.file_sync.method,
            p.file_sync.state,
            &p.file_sync.native_code,
        ),
        namespace_sync: namespace(&p.namespace_sync)?,
        cleanup: cleanup(&p.cleanup)?,
        existing: match item.existing_observation.value() {
            None => None,
            Some(e) => Some(ExistingFacts {
                identity_known: e.identity.value().is_some(),
                bytes_equal: e.bytes_equal.value().copied(),
                stable: e.stable,
                address: address(e.address),
                file_sync: sync(
                    e.file_sync.method,
                    e.file_sync.state,
                    &e.file_sync.native_code,
                ),
                namespace_sync: namespace(&e.namespace_sync)?,
            }),
        },
        error: item
            .error
            .value()
            .and_then(|e| eml_core::ErrorCode::from_wire_name(&e.code)),
        related_resources: item
            .removed
            .iter()
            .any(|r| r.reason == Removal::RelatedResource),
    })
}
