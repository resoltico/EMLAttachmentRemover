//! Shared receipt qualification. These predicates do not perform I/O or grant
//! construction of private execution successes; native binding/readback remains required.
use crate::{
    AddressState, CleanupReceipt, NamespaceObservation, SyncReceipt, vocabulary::Publication,
};

/// Both the file and every required namespace operation consistently succeeded.
pub fn synchronized(file: Option<SyncReceipt>, namespace: NamespaceObservation) -> bool {
    file.is_some_and(SyncReceipt::succeeded) && namespace.succeeded()
}
/// Qualify observed publication receipts, independently of final readback and errors.
pub fn publication(
    state: Publication,
    identity_known: bool,
    address: AddressState,
    file: Option<SyncReceipt>,
    namespace: NamespaceObservation,
    cleanup: &CleanupReceipt,
) -> bool {
    state == Publication::Published
        && identity_known
        && address == AddressState::Verified
        && synchronized(file, namespace)
        && cleanup.succeeded()
}
/// Qualify stable matching existing bytes and their address/synchronization receipts.
pub fn existing(
    identity_known: bool,
    bytes_equal: Option<bool>,
    stable: bool,
    address: AddressState,
    file: Option<SyncReceipt>,
    namespace: NamespaceObservation,
) -> bool {
    identity_known
        && bytes_equal == Some(true)
        && stable
        && address == AddressState::Verified
        && synchronized(file, namespace)
}
