//! Stable semantic diagnostics; no JSON, native error parsing or process state.

/// Operation responsible for a core failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ErrorPhase {
    /// Whole-request/source/output admission.
    Admission,
    /// Ordered source discovery.
    Collection,
    /// Stable bounded reading or binding of a required file.
    Read,
    /// Pure MIME preparation and independent verification.
    Prepare,
    /// Existing-copy comparison and resynchronization.
    Existing,
    /// Private candidate resource creation.
    Stage,
    /// Candidate storage write and readback.
    Write,
    /// Required file or namespace synchronization.
    Sync,
    /// Atomic no-replace edge and source stability checks around it.
    Publish,
    /// Requested destination address qualification.
    Address,
    /// Owned resource cleanup.
    Cleanup,
    /// Event delivery to the frontend.
    Delivery,
}
impl ErrorPhase {
    /// Exhaustive phase vocabulary; independent of frontend serialization.
    pub const ALL: &'static [Self] = &[
        Self::Admission,
        Self::Collection,
        Self::Read,
        Self::Prepare,
        Self::Existing,
        Self::Stage,
        Self::Write,
        Self::Sync,
        Self::Publish,
        Self::Address,
        Self::Cleanup,
        Self::Delivery,
    ];

    /// Stable protocol-1 spelling, without a JSON dependency.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Admission => "admission",
            Self::Collection => "collection",
            Self::Read => "read",
            Self::Prepare => "prepare",
            Self::Existing => "existing",
            Self::Stage => "stage",
            Self::Write => "write",
            Self::Sync => "sync",
            Self::Publish => "publish",
            Self::Address => "address",
            Self::Cleanup => "cleanup",
            Self::Delivery => "delivery",
        }
    }
}
impl std::fmt::Display for ErrorPhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

// One entry owns each variant's documentation, wire name and permitted phases.
// Frontends consume these values rather than maintaining another code registry.
macro_rules! error_codes {
    ($($name:ident => $wire:literal, [$($phase:ident),+], $doc:literal $(, $hint:literal)?;)+) => {
        /// Exhaustive protocol-1 semantic code, distinct from native OS errors.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
        pub enum ErrorCode {
            $(#[doc = $doc] $name,)+
        }
        impl ErrorCode {
            /// Exhaustive code vocabulary, in contract order.
            pub const ALL: &'static [Self] = &[$(Self::$name,)+];
            /// Stable protocol-1 spelling, without a JSON dependency.
            pub const fn as_str(self) -> &'static str {
                match self { $(Self::$name => $wire,)+ }
            }
            /// Resolve only a known wire name; future frontend codes remain unknown.
            pub fn from_wire_name(name: &str) -> Option<Self> {
                match name { $($wire => Some(Self::$name),)+ _ => None }
            }
            /// A recovery action appropriate to this observed failure, when known.
            /// This complements the exact diagnostic; it does not establish effects.
            pub const fn recovery_hint(self) -> Option<&'static str> {
                match self { $(Self::$name => error_codes!(@hint $( $hint )?),)+ }
            }
            /// Phases allowed by this semantic code's contract.
            pub const fn allowed_phases(self) -> &'static [ErrorPhase] {
                match self { $(Self::$name => &[$(ErrorPhase::$phase,)+],)+ }
            }
            /// Whether a phase/code pair is meaningful. This does not establish
            /// filesystem effects or whether an atomic edge has happened.
            pub fn allows_phase(self, phase: ErrorPhase) -> bool {
                self.allowed_phases().contains(&phase)
            }
        }
    };
    (@hint $hint:literal) => { Some($hint) };
    (@hint) => { None };
}
error_codes! {
    ReadFailed => "READ_FAILED", [Admission, Collection, Read], "A required file could not be read.", "Check that the selected file or folder still exists and that you have permission to read it, then retry.";
    SourceNotRegular => "SOURCE_NOT_REGULAR", [Admission, Collection, Read], "The selected object is not a regular disk file.";
    SourceChanged => "SOURCE_CHANGED", [Admission, Collection, Read, Publish], "An observable source identity, binding or signature changed.";
    Malformed => "MALFORMED", [Prepare], "MIME syntax or structure is ambiguous or damaged.", "This message's MIME structure is damaged or incomplete. Export it again from your mail app and retry with that copy.";
    LimitExceeded => "LIMIT_EXCEEDED", [Prepare], "Message or retained metadata exceeds a preparation bound.";
    Protected => "PROTECTED", [Prepare], "Protected content occurred in a retained body context.", "Protected content would need to change in the retained message body. Obtain an unprotected export if appropriate.";
    NoBody => "NO_BODY", [Prepare], "No supported text body would remain.", "No supported plain-text or HTML message body would remain. Choose a message that contains a text body.";
    Internal => "INTERNAL", [Prepare], "Independent preparation verification detected a defect.";
    InputAdmissionFailed => "INPUT_ADMISSION_FAILED", [Admission, Collection], "At least one explicit source root could not bind.";
    InputLimitExceeded => "INPUT_LIMIT_EXCEEDED", [Admission, Collection], "Whole-run collection exceeded a resource bound.", "Select fewer messages or a smaller folder and try again.";
    NoInputs => "NO_INPUTS", [Admission, Collection], "Discovery found no work items.", "No .eml messages were found. Select messages or another folder; hidden entries and existing .no-attachments.eml copies are skipped during folder scans.";
    OutputOverlap => "OUTPUT_OVERLAP", [Admission], "The output root overlaps a selected directory tree.", "The copy folder overlaps a selected source folder. Choose a destination outside the selected tree, or save copies beside the originals.";
    DestinationCollision => "DESTINATION_COLLISION", [Admission, Existing, Publish], "Distinct inputs contend for one output.", "Split colliding messages into separate runs and destination folders so each copy has its own path.";
    DestinationExists => "DESTINATION_EXISTS", [Admission, Existing, Publish], "An existing entry cannot qualify as the requested copy.", "Choose another copy folder, or move the existing destination before retrying. Existing entries will not be overwritten.";
    SourceDestinationAlias => "SOURCE_DESTINATION_ALIAS", [Admission, Existing, Publish], "An output aliases a collected source.";
    DestinationUnsupported => "DESTINATION_UNSUPPORTED", [Stage, Publish], "The required destination primitive is unsupported.";
    BindFailed => "BIND_FAILED", [Admission, Read, Stage, Address], "A required native object or address binding failed.";
    StageFailed => "STAGE_FAILED", [Stage], "Owned private candidate resources could not be established.", "Check destination write permissions and available disk space, or choose another copy folder.";
    WriteFailed => "WRITE_FAILED", [Write, Publish], "Candidate storage or publication failed.";
    FileSyncFailed => "FILE_SYNC_FAILED", [Sync], "Required candidate file synchronization failed.";
    NamespaceSyncFailed => "NAMESPACE_SYNC_FAILED", [Sync], "A required namespace synchronization failed.";
    AddressChanged => "ADDRESS_CHANGED", [Address], "The requested address now resolves differently.";
    AddressUnverified => "ADDRESS_UNVERIFIED", [Address], "The requested address could not be qualified.";
    CleanupFailed => "CLEANUP_FAILED", [Cleanup], "Owned resource cleanup did not complete.";
    ExistingSyncFailed => "EXISTING_SYNC_FAILED", [Existing], "Existing-copy resynchronization failed.";
    Interrupted => "INTERRUPTED", [Read, Prepare, Existing, Stage, Write, Sync, Address, Publish], "A started item stopped before its atomic publication edge.";
    OutputFailed => "OUTPUT_FAILED", [Delivery], "Frontend delivery failed and further work must stop.";
}
impl std::fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
