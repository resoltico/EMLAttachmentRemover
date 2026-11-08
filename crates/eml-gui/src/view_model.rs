//! Map accepted protocol facts into structured Slint values using core vocabulary.
use base64::{Engine, engine::general_purpose::STANDARD};
use eml_core::{
    AddressState, CleanupReceipt, CleanupState, NamespaceObservation, PlanAction, SyncMethod,
    SyncReceipt, SyncState,
    vocabulary::{self, ExistingFacts, OutcomeFacts, Publication, ResultKind},
};
use eml_protocol::records as wire;
use std::{ffi::OsString, os::unix::ffi::OsStringExt};
fn address(value: wire::Address) -> AddressState {
    match value {
        wire::Address::NotChecked => AddressState::NotChecked,
        wire::Address::Verified => AddressState::Verified,
        wire::Address::Changed => AddressState::Changed,
        wire::Address::Unknown => AddressState::Unknown,
    }
}
fn receipt(
    method: wire::SynchronizationMethod,
    state: wire::Synchronization,
    code: Option<i64>,
) -> Option<SyncReceipt> {
    let method = match method {
        wire::SynchronizationMethod::NotRequested => return None,
        wire::SynchronizationMethod::Fsync => SyncMethod::Fsync,
        wire::SynchronizationMethod::FsyncFullfsync => SyncMethod::FsyncFullfsync,
        wire::SynchronizationMethod::FsyncFullfsyncUnsupported => {
            SyncMethod::FsyncFullfsyncUnsupported
        }
        wire::SynchronizationMethod::FlushFileBuffers => SyncMethod::FlushFileBuffers,
    };
    let state = match state {
        wire::Synchronization::Succeeded => SyncState::Succeeded,
        wire::Synchronization::Unsupported => SyncState::Unsupported,
        wire::Synchronization::Failed => SyncState::Failed,
        wire::Synchronization::NotAttempted => return None,
    };
    Some(SyncReceipt {
        method,
        state,
        native_code: code,
    })
}
fn sync(value: &wire::SyncRecord) -> Option<SyncReceipt> {
    receipt(
        value.method,
        value.state,
        value.native_code.value().copied(),
    )
}
fn namespace(value: &wire::NamespaceRecord) -> Option<NamespaceObservation> {
    Some(NamespaceObservation {
        receipt: receipt(
            value.method,
            value.state,
            value.native_code.value().copied(),
        ),
        required_edges: value.required_edges.try_into().ok()?,
        completed_edges: value.completed_edges.try_into().ok()?,
        failed_edge_index: value
            .failed_edge_index
            .value()
            .map(|value| (*value).try_into())
            .transpose()
            .ok()?,
    })
}
fn cleanup(value: &wire::CleanupRecord) -> Option<CleanupReceipt> {
    let residual_name = if let Some(native) = value.residual_name.value() {
        if native.encoding != wire::PathEncoding::UnixBytes {
            return None;
        }
        Some(OsString::from_vec(STANDARD.decode(&native.base64).ok()?))
    } else {
        None
    };
    Some(CleanupReceipt {
        state: match value.state {
            wire::Cleanup::NotNeeded => CleanupState::NotNeeded,
            wire::Cleanup::Complete => CleanupState::Complete,
            wire::Cleanup::Residual => CleanupState::Residual,
            wire::Cleanup::Unknown => CleanupState::Unknown,
        },
        residual_name,
        native_code: value.native_code.value().copied(),
    })
}
pub(super) fn facts(item: &wire::ItemRecord) -> Option<OutcomeFacts> {
    let existing = match item.existing_observation.value() {
        Some(value) => Some(ExistingFacts {
            identity_known: value.identity.value().is_some(),
            bytes_equal: value.bytes_equal.value().copied(),
            stable: value.stable,
            address: address(value.address),
            file_sync: sync(&value.file_sync),
            namespace_sync: namespace(&value.namespace_sync)?,
        }),
        None => None,
    };
    Some(OutcomeFacts {
        result: match item.status {
            wire::ItemStatus::Created => ResultKind::Created,
            wire::ItemStatus::UpToDate => ResultKind::Existing,
            wire::ItemStatus::Planned => ResultKind::Planned,
            wire::ItemStatus::Failed => ResultKind::Failed,
        },
        plan: item.plan.value().map(|value| match value {
            wire::Plan::Create => PlanAction::Create,
            wire::Plan::VerifyExisting => PlanAction::VerifyExisting,
        }),
        publication: match item.publication.state {
            wire::Publication::NotPublished => Publication::NotPublished,
            wire::Publication::Published => Publication::Published,
            wire::Publication::Unknown => Publication::Unknown,
        },
        address: address(item.publication.address),
        candidate_identity_known: item.publication.candidate_identity.value().is_some(),
        final_readback: (item.status == wire::ItemStatus::Created).then_some(true),
        file_sync: sync(&item.publication.file_sync),
        namespace_sync: namespace(&item.publication.namespace_sync)?,
        cleanup: cleanup(&item.publication.cleanup)?,
        existing,
        error: item
            .error
            .value()
            .and_then(|error| eml_core::ErrorCode::from_wire_name(&error.code)),
        related_resources: item
            .removed
            .iter()
            .any(|part| part.reason == wire::Removal::RelatedResource),
    })
}
pub(super) fn outcome(item: &wire::ItemRecord) -> vocabulary::Outcome {
    facts(item).map_or(vocabulary::Outcome::Unknown, |facts| {
        vocabulary::Outcome::from(&facts, item.removed.len())
    })
}

/// Project settled current-run facts, including work whose source rows were never observed.
pub(super) fn settled_totals(report: &eml_app::RunReport) -> Option<vocabulary::BatchTotals> {
    let summary = report.summary()?;
    Some(vocabulary::BatchTotals::from(
        report.items().iter().map(|item| {
            (
                Some(outcome(item)),
                item.removed.len(),
                item.source_bytes
                    .value()
                    .copied()
                    .zip(item.copy_bytes.value().copied()),
            )
        }),
        summary.counts.not_processed as usize,
    ))
}
