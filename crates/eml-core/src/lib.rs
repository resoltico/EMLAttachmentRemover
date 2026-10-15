//! Reusable message preparation and copy workflows for EML Attachment Remover.
#![forbid(unsafe_code)]

mod grammar;
mod message;
mod plan;
mod verify;

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// Largest admitted source message, in bytes. Equality is permitted.
pub const MAX_MESSAGE_BYTES: usize = 256 * 1024 * 1024;

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
    /// Explicit attachment disposition on an ordinary entity.
    Attachment,
    /// Non-root direct child of a retained related compound.
    RelatedResource,
    /// A leaf that is neither plain nor HTML body content.
    NonBodyPart,
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
    let source = message::parse(input, control)?;
    let plan = plan::build(&source, control)?;
    let authorization = verify::authorize_deletions(&source, &plan.claims, control)?;
    if authorization.stripped != plan.stripped {
        return Err(error(
            Refusal::Internal,
            "Stripped-field metadata disagrees with independent authorization",
        ));
    }
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
