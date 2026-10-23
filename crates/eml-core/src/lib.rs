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
pub use run::OperationObservation;
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
pub mod meaning;
pub mod product;
#[cfg(any(unix, windows))]
mod selection;
#[cfg(any(unix, windows))]
pub use collection::{
    CollectedObservation, CollectedSource, CollectedSources, CollectionState, InvalidRequest,
    MAX_COLLECTED_ITEMS, MAX_COLLECTED_PATH_BYTES, MAX_DIRECTORY_ENTRIES, MAX_WALK_DEPTH,
    SourceOrigin, check_output_location, collect_sources,
};
mod grammar;
mod message;
#[cfg(any(unix, windows))]
mod naming;
mod plan;
#[cfg(any(unix, windows))]
mod request;
#[cfg(any(unix, windows))]
mod source;
mod verify;
#[cfg(test)]
mod work_bounds;
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

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// Largest admitted source message, in bytes. Equality is permitted.
pub const MAX_MESSAGE_BYTES: usize = 256 * 1024 * 1024;
// Payload scanning, copying and equality comparisons share this work-unit bound.
pub(crate) const PREPARATION_CHUNK_BYTES: usize = 64 * 1024;
/// Maximum parsed MIME entities in one message, including containers/root.
pub const MAX_MESSAGE_PARTS: usize = 10_000;

/// Per-run cooperative cancellation; independent of process-global signals.
#[derive(Clone, Default, Debug)]
pub struct RunControl(Arc<AtomicBool>);
impl RunControl {
    /// Request cooperative stop. Completed publication is never rolled back.
    pub fn request_stop(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
    /// Whether a stop has been requested for this run.
    pub fn is_stopped(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
    pub(crate) fn checkpoint(&self) -> Result<(), PreparationError> {
        if self.is_stopped() {
            Err(PreparationError::Interrupted)
        } else {
            Ok(())
        }
    }
}

/// Why immutable source bytes cannot be prepared as a body-only copy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// Required MIME syntax or structure is ambiguous or damaged.
    Malformed,
    /// An explicit resource admission bound was exceeded.
    LimitExceeded,
    /// Protected MIME occurred in a retained body context.
    Protected,
    /// A retained semantic container is outside the supported MIME profile.
    UnsupportedMime,
    /// No supported text entity would remain.
    NoBody,
    /// Independent authorization or candidate verification detected a defect.
    Internal,
}
/// A MIME refusal or an independently requested cooperative interruption.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PreparationError {
    /// Preparation stopped without producing a publishable value.
    Interrupted,
    /// A bounded refusal diagnostic, containing no source payload.
    Refused {
        /// Stable semantic category.
        kind: Refusal,
        /// Source MIME section, absent for the outer root.
        section: Option<String>,
        /// Bounded authored explanation, never raw source header text.
        detail: &'static str,
    },
}
impl PreparationError {
    /// Classify pure preparation through the shared semantic vocabulary.
    /// Native errors and prose never become protocol codes.
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::Interrupted => ErrorCode::Interrupted,
            Self::Refused { kind, .. } => match kind {
                Refusal::Malformed => ErrorCode::Malformed,
                Refusal::LimitExceeded => ErrorCode::LimitExceeded,
                Refusal::Protected => ErrorCode::Protected,
                Refusal::UnsupportedMime => ErrorCode::UnsupportedMime,
                Refusal::NoBody => ErrorCode::NoBody,
                Refusal::Internal => ErrorCode::Internal,
            },
        }
    }
    /// This capability reports only preparation failures, including its own
    /// cooperative stop. The runner owns other phases and publication effects.
    pub const fn phase(&self) -> ErrorPhase {
        ErrorPhase::Prepare
    }
}
impl std::fmt::Display for PreparationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Interrupted => f.write_str("Preparation interrupted"),
            Self::Refused {
                kind,
                section,
                detail,
            } => write!(
                f,
                "{kind:?} at {}: {detail}",
                section.as_deref().unwrap_or("root")
            ),
        }
    }
}
impl std::error::Error for PreparationError {}
pub(crate) fn error(kind: Refusal, detail: &'static str) -> PreparationError {
    PreparationError::Refused {
        kind,
        section: None,
        detail,
    }
}

/// One independently authorized reason for removing an entity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemovalReason {
    /// Present non-inline disposition on an ordinary entity, including extensions.
    Attachment,
    /// Non-root direct child of a retained related compound.
    RelatedResource,
    /// A leaf that is neither plain nor HTML body content.
    NonBodyPart,
}
impl RemovalReason {
    /// Canonical user-facing category; the parser owns the decision itself.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Attachment => "Attachment",
            Self::RelatedResource => "Related image or resource",
            Self::NonBodyPart => "Other non-body content",
        }
    }
    /// Canonical protocol-1 name; rendering does not duplicate MIME decisions.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Attachment => "attachment",
            Self::RelatedResource => "related_resource",
            Self::NonBodyPart => "non_body_part",
        }
    }
}
/// Bounded display metadata describing one source decision, not a destination path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemovedPart {
    /// Original source section; surviving siblings can renumber in the copy.
    pub section: String,
    /// Lowercase MIME media type.
    pub content_type: String,
    /// Sanitized display name, never used for filesystem operations.
    pub filename: Option<String>,
    /// Entity-content bytes, excluding multipart delimiter and framing.
    pub bytes: u64,
    /// Exactly one policy reason.
    pub reason: RemovalReason,
}
/// Complete independently verified bytes. Callers cannot forge this value.
#[derive(Debug)]
pub struct PreparedMessage {
    bytes: Vec<u8>,
    removed: Vec<RemovedPart>,
    stripped_fields: Vec<String>,
}
impl PreparedMessage {
    /// Verified copy bytes; retained content is not decoded or regenerated.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
    /// Source-order authorized removal decisions.
    pub fn removed(&self) -> &[RemovedPart] {
        &self.removed
    }
    /// Sorted unique stale field names removed from retained ancestors.
    pub fn stripped_fields(&self) -> &[String] {
        &self.stripped_fields
    }
}

/// Prepare an immutable message without filesystem access or frontend policy.
///
/// A successful value has passed independent source authorization, exact-byte
/// reconstruction, output reparsing, retained-structure comparison and policy checks.
pub fn prepare_message(
    input: &[u8],
    control: &RunControl,
) -> Result<PreparedMessage, PreparationError> {
    control.checkpoint()?;
    if input.len() > MAX_MESSAGE_BYTES {
        return Err(error(
            Refusal::LimitExceeded,
            "Raw message exceeds the admitted byte limit",
        ));
    }
    #[cfg(test)]
    work_bounds::stage(work_bounds::Stage::SourceParse);
    let source = message::parse(input, control)?;
    #[cfg(test)]
    work_bounds::stage(work_bounds::Stage::Planning);
    let plan = plan::build(&source, control)?;
    #[cfg(test)]
    work_bounds::stage(work_bounds::Stage::Authorization);
    let authorization = verify::authorize_deletions(&source, &plan.claims, control)?;
    if authorization.stripped != plan.stripped {
        return Err(error(
            Refusal::Internal,
            "Stripped-field metadata disagrees with independent authorization",
        ));
    }
    #[cfg(test)]
    work_bounds::stage(work_bounds::Stage::CandidateCopy);
    let bytes = plan::apply(input, &plan.edits, control)?;
    verify::verify_candidate(&source, &authorization, &bytes, control)?;
    let removed = plan
        .claims
        .iter()
        .map(|claim| {
            let node = &source.nodes[claim.id];
            RemovedPart {
                section: node.section.clone().unwrap_or_default(),
                content_type: node.media.clone(),
                filename: node.filename.clone(),
                bytes: node.span.len() as u64,
                reason: claim.reason,
            }
        })
        .collect();
    Ok(PreparedMessage {
        bytes,
        removed,
        stripped_fields: plan.stripped.into_iter().collect(),
    })
}
