//! Reusable immutable message preparation for EML Attachment Remover.
#![forbid(unsafe_code)]

#[cfg(any(unix, windows))]
mod binding;
#[cfg(any(unix, windows))]
mod collection;
#[cfg(any(unix, windows))]
mod destination;
#[cfg(any(unix, windows))]
mod existing;
#[cfg(any(unix, windows))]
mod outcome;
#[cfg(any(unix, windows))]
mod run;
#[cfg(any(unix, windows))]
pub use existing::{
    ComparedExisting, ExistingComparisonError, ExistingComparisonFailure, compare_existing,
};
#[cfg(any(unix, windows))]
pub use outcome::{
    AddressState, Counts, CreatedCopy, DeliveryFailure, DeliveryPhase, Event, EventSink,
    ExistingCopy, ExistingObservation, ExitClass, FailureEffect, ItemEvent, ItemFailure,
    ItemResult, MessageMetadata, NamespaceObservation, PlanAction, PlannedCopy, PublishedCopy,
    ReportPath, RunOutcome, RunStart, UnknownPublication,
};
#[cfg(any(test, feature = "native-fixtures"))]
pub use run::PublicationObservation;
#[cfg(feature = "native-fixtures")]
pub use run::run_observed;
#[cfg(any(unix, windows))]
pub use run::{MAX_REMOVAL_ENTRIES, dry_run, run};
#[cfg(any(unix, windows))]
mod failure;
#[cfg(any(unix, windows))]
pub use destination::{
    DestinationInfo, DestinationObservation, DestinationPlan, DestinationPlanState,
    DestinationRecord, plan_destinations,
};
#[cfg(any(unix, windows))]
pub use eml_fs::{CleanupReceipt, CleanupState, Identity, SyncMethod, SyncReceipt, SyncState};
#[cfg(any(unix, windows))]
pub use failure::{NativeDiagnostic, OperationFailure};
mod diagnostic;
#[cfg(any(unix, windows))]
mod selection;
#[cfg(any(unix, windows))]
pub use collection::{
    CollectedObservation, CollectedSource, CollectedSources, CollectionState, InvalidRequest,
    MAX_COLLECTED_ITEMS, MAX_COLLECTED_PATH_BYTES, MAX_DIRECTORY_ENTRIES, MAX_WALK_DEPTH,
    SourceOrigin, check_output_location, collect_sources,
};
#[cfg(any(unix, windows))]
mod naming;
#[cfg(any(unix, windows))]
mod request;
#[cfg(any(unix, windows))]
mod source;
pub use diagnostic::{ErrorCode, ErrorPhase};
#[cfg(any(unix, windows))]
pub use naming::{COPY_SUFFIX, NamingError, copy_basename};
#[cfg(any(unix, windows))]
pub use request::{
    MAX_INPUT_PATH_BYTES, MAX_INPUT_PATHS, MAX_PATH_COMPONENTS, MAX_PATH_UNITS, OutputLocation,
    PathViolation, RequestPath, RequestPathError, RunMode, RunRequest,
};
#[cfg(any(unix, windows))]
pub use source::{PreparedSource, SourcePreparationError, prepare_source};

pub mod product;
pub mod vocabulary;
pub use eml_message::{
    MAX_MESSAGE_BYTES, MAX_MESSAGE_PARTS, PreparationError, PreparedMessage, Refusal,
    RemovalReason, RemovedPart, RunControl, prepare_message,
};
pub(crate) fn error(kind: Refusal, detail: &'static str) -> PreparationError {
    PreparationError::Refused {
        kind,
        section: None,
        detail,
    }
}
