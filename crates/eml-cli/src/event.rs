//! Protocol-1 projection of borrowed typed business observations; no filesystem policy.
use crate::PathValue;
use eml_core::{
    AddressState, CleanupReceipt, CleanupState, Counts, Event, ExistingObservation, FailureEffect,
    Identity, ItemEvent, ItemResult, MessageMetadata, NamespaceObservation, NativeDiagnostic,
    OperationFailure, PlanAction, PublishedCopy, RemovedPart, RunOutcome, RunStart, SyncMethod,
    SyncReceipt, SyncState,
};
use serde::Serialize;
use std::borrow::Cow;

pub(super) const PROGRAM: &str = env!("CARGO_PKG_NAME");
pub(super) const VERSION: &str = env!("CARGO_PKG_VERSION");
#[derive(Serialize)]
struct Start<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    protocol: u8,
    program: &'a str,
    version: &'a str,
    dry_run: bool,
    total: usize,
    collection_complete: bool,
}
#[derive(Serialize)]
struct Summary<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    counts: CountFields,
    interrupted: bool,
    collection_complete: bool,
    run_error: Option<ErrorFields<'a>>,
    exit_code: u8,
}
#[derive(Serialize)]
struct CountFields {
    created: usize,
    up_to_date: usize,
    planned: usize,
    failed: usize,
    not_processed: usize,
}
impl From<Counts> for CountFields {
    fn from(value: Counts) -> Self {
        Self {
            created: value.created(),
            up_to_date: value.up_to_date(),
            planned: value.planned(),
            failed: value.failed(),
            not_processed: value.not_processed(),
        }
    }
}
#[derive(Serialize)]
struct ErrorFields<'a> {
    code: &'static str,
    phase: &'static str,
    detail: Cow<'a, str>,
    section: Option<&'a str>,
}
fn error<'a>(value: &'a OperationFailure, section: Option<&'a str>) -> ErrorFields<'a> {
    let suffix = match value.native_diagnostic() {
        None => String::new(),
        Some(NativeDiagnostic::OsCode(code)) => format!(" (OS error {code})"),
        #[cfg(windows)]
        Some(NativeDiagnostic::NtStatus { status, win32 }) => {
            format!(" (NTSTATUS 0x{:08X}; Win32 {win32})", status as u32)
        }
    };
    let detail = bounded_detail(value.detail(), &suffix);
    ErrorFields {
        code: value.code().as_str(),
        phase: value.phase().as_str(),
        detail,
        section,
    }
}
fn bounded_detail<'a>(detail: &'a str, suffix: &str) -> Cow<'a, str> {
    let remaining = 1000 - suffix.chars().count();
    let prefix = detail
        .char_indices()
        .nth(remaining)
        .map_or(detail, |(byte, _)| &detail[..byte]);
    if suffix.is_empty() && prefix.len() == detail.len() {
        Cow::Borrowed(detail)
    } else {
        Cow::Owned(format!("{prefix}{suffix}"))
    }
}
#[derive(Serialize)]
struct IdentityFields {
    kind: &'static str,
    value: String,
}
fn identity(value: Identity) -> IdentityFields {
    match value {
        Identity::Posix { device, inode } => IdentityFields {
            kind: "posix",
            value: format!("{device}:{inode}"),
        },
        Identity::Windows { volume, file } => {
            use std::fmt::Write;
            let mut value = format!("{volume:016X}:");
            for byte in file {
                write!(&mut value, "{byte:02X}").expect("Writing an owned String cannot fail");
            }
            IdentityFields {
                kind: "windows",
                value,
            }
        }
    }
}
#[derive(Serialize)]
struct SyncFields {
    method: &'static str,
    state: &'static str,
    native_code: Option<i64>,
}
fn sync(value: Option<SyncReceipt>) -> SyncFields {
    let Some(value) = value else {
        return SyncFields {
            method: "not_requested",
            state: "not_attempted",
            native_code: None,
        };
    };
    SyncFields {
        method: match value.method {
            SyncMethod::Fsync => "fsync",
            SyncMethod::FsyncFullfsync => "fsync_fullfsync",
            SyncMethod::FsyncFullfsyncUnsupported => "fsync_fullfsync_unsupported",
            SyncMethod::FlushFileBuffers => "flush_file_buffers",
        },
        state: match value.state {
            SyncState::Succeeded => "succeeded",
            SyncState::Unsupported => "unsupported",
            SyncState::Failed => "failed",
        },
        native_code: value.native_code,
    }
}
#[derive(Serialize)]
struct NamespaceFields {
    #[serde(flatten)]
    sync: SyncFields,
    required_edges: u16,
    completed_edges: u16,
    failed_edge_index: Option<u16>,
}
fn namespace(value: NamespaceObservation) -> NamespaceFields {
    NamespaceFields {
        sync: sync(value.receipt),
        required_edges: value.required_edges,
        completed_edges: value.completed_edges,
        failed_edge_index: value.failed_edge_index,
    }
}
#[derive(Serialize)]
struct CleanupFields {
    state: &'static str,
    residual_name: Option<NativeComponent>,
    native_code: Option<i64>,
}
// Reuse the existing lossless native encoder. Components have no display/absolute path copy.
struct NativeComponent(std::path::PathBuf);
impl Serialize for NativeComponent {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        crate::path::NativePath::encode(&self.0).serialize(serializer)
    }
}
fn cleanup(value: &CleanupReceipt) -> CleanupFields {
    CleanupFields {
        state: match value.state {
            CleanupState::NotNeeded => "not_needed",
            CleanupState::Complete => "complete",
            CleanupState::Residual => "residual",
            CleanupState::Unknown => "unknown",
        },
        residual_name: value
            .residual_name
            .as_ref()
            .map(|name| NativeComponent(name.into())),
        native_code: value.native_code,
    }
}
fn address(value: AddressState) -> &'static str {
    match value {
        AddressState::NotChecked => "not_checked",
        AddressState::Verified => "verified",
        AddressState::Changed => "changed",
        AddressState::Unknown => "unknown",
    }
}
#[derive(Serialize)]
struct PublicationFields {
    state: &'static str,
    candidate_identity: Option<IdentityFields>,
    address: &'static str,
    file_sync: SyncFields,
    namespace_sync: NamespaceFields,
    cleanup: CleanupFields,
}
fn unattempted_namespace() -> NamespaceFields {
    NamespaceFields {
        sync: sync(None),
        required_edges: 0,
        completed_edges: 0,
        failed_edge_index: None,
    }
}
fn unattempted_publication() -> PublicationFields {
    PublicationFields {
        state: "not_published",
        candidate_identity: None,
        address: "not_checked",
        file_sync: sync(None),
        namespace_sync: unattempted_namespace(),
        cleanup: CleanupFields {
            state: "not_needed",
            residual_name: None,
            native_code: None,
        },
    }
}
fn published(value: &PublishedCopy) -> PublicationFields {
    PublicationFields {
        state: "published",
        candidate_identity: Some(identity(value.identity())),
        address: address(value.address()),
        file_sync: sync(value.file_sync()),
        namespace_sync: namespace(value.namespace_sync()),
        cleanup: cleanup(value.cleanup()),
    }
}
fn failed_publication(value: &FailureEffect) -> PublicationFields {
    match value {
        FailureEffect::NotPublished {
            candidate_identity,
            address: value_address,
            file_sync,
            namespace_sync,
            cleanup: value_cleanup,
        } => PublicationFields {
            state: "not_published",
            candidate_identity: candidate_identity.map(identity),
            address: address(*value_address),
            file_sync: sync(*file_sync),
            namespace_sync: namespace(*namespace_sync),
            cleanup: cleanup(value_cleanup),
        },
        FailureEffect::Published(value) => published(value),
        FailureEffect::Unknown(value) => PublicationFields {
            state: "unknown",
            candidate_identity: value.candidate_identity().map(identity),
            address: address(value.address()),
            file_sync: sync(value.file_sync()),
            namespace_sync: namespace(value.namespace_sync()),
            cleanup: cleanup(value.cleanup()),
        },
    }
}
#[derive(Serialize)]
struct ExistingFields {
    identity: Option<IdentityFields>,
    bytes_equal: Option<bool>,
    stable: bool,
    address: &'static str,
    file_sync: SyncFields,
    namespace_sync: NamespaceFields,
}
fn existing(value: &ExistingObservation) -> ExistingFields {
    ExistingFields {
        identity: value.identity.map(identity),
        bytes_equal: value.bytes_equal,
        stable: value.stable,
        address: address(value.address),
        file_sync: sync(value.file_sync),
        namespace_sync: namespace(value.namespace_sync),
    }
}
#[derive(Serialize)]
struct RemovedFields<'a> {
    section: &'a str,
    content_type: &'a str,
    filename: Option<&'a str>,
    bytes: u64,
    reason: &'static str,
}
impl<'a> From<&'a RemovedPart> for RemovedFields<'a> {
    fn from(value: &'a RemovedPart) -> Self {
        Self {
            section: &value.section,
            content_type: &value.content_type,
            filename: value.filename.as_deref(),
            bytes: value.bytes,
            reason: value.reason.as_str(),
        }
    }
}
#[derive(Serialize)]
struct Item<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    index: usize,
    source: PathValue<'a>,
    status: &'static str,
    destination: Option<PathValue<'a>>,
    source_bytes: Option<u64>,
    copy_bytes: Option<u64>,
    removed: Vec<RemovedFields<'a>>,
    stripped_fields: &'a [String],
    plan: Option<&'static str>,
    publication: PublicationFields,
    existing_observation: Option<ExistingFields>,
    error: Option<ErrorFields<'a>>,
}
fn item<'a>(value: &'a ItemEvent<'a>) -> Item<'a> {
    let (status, data, plan, publication, observed, failure): (
        _,
        Option<MessageMetadata<'a>>,
        _,
        _,
        _,
        _,
    ) = match &value.result {
        ItemResult::Created(copy, data) => (
            "created",
            Some(*data),
            None,
            published(copy.publication()),
            None,
            None,
        ),
        ItemResult::UpToDate(copy, data) => (
            "up_to_date",
            Some(*data),
            None,
            unattempted_publication(),
            Some(existing(copy.observation())),
            None,
        ),
        ItemResult::Planned(copy) => (
            "planned",
            Some(copy.metadata()),
            Some(match copy.action() {
                PlanAction::Create => "create",
                PlanAction::VerifyExisting => "verify_existing",
            }),
            unattempted_publication(),
            copy.existing().map(existing),
            None,
        ),
        ItemResult::Failed(failure) => (
            "failed",
            failure.metadata(),
            None,
            failed_publication(failure.effect()),
            failure.existing().map(existing),
            Some(error(failure.error(), failure.section())),
        ),
    };
    Item {
        kind: "item",
        index: value.index,
        source: PathValue::new(value.source.native, value.source.display),
        status,
        destination: value
            .destination
            .map(|path| PathValue::new(path.native, path.display)),
        source_bytes: data.map(|data| data.source_bytes),
        copy_bytes: data.map(|data| data.copy_bytes),
        removed: data.map_or_else(Vec::new, |data| {
            data.removed.iter().map(RemovedFields::from).collect()
        }),
        stripped_fields: data.map_or(&[], |data| data.stripped_fields),
        plan,
        publication,
        existing_observation: observed,
        error: failure,
    }
}
fn start(value: &RunStart) -> Start<'static> {
    Start {
        kind: "start",
        protocol: 1,
        program: PROGRAM,
        version: VERSION,
        dry_run: value.dry_run,
        total: value.total,
        collection_complete: value.collection_complete,
    }
}
fn summary(value: &RunOutcome) -> Summary<'_> {
    Summary {
        kind: "summary",
        counts: value.counts().into(),
        interrupted: value.interrupted(),
        collection_complete: value.collection_complete(),
        run_error: value.run_error().map(|value| error(value, None)),
        exit_code: value.exit_class().code(),
    }
}
pub(super) fn write_json(
    writer: &mut impl std::io::Write,
    event: Event<'_>,
) -> serde_json::Result<()> {
    match event {
        Event::Start(value) => serde_json::to_writer(writer, &start(value)),
        Event::Item(value) => serde_json::to_writer(writer, &item(value)),
        Event::Summary(value) => serde_json::to_writer(writer, &summary(value)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};
    fn metadata() -> (String, String, String) {
        (
            std::iter::repeat_n("9999", 64)
                .collect::<Vec<_>>()
                .join("."),
            format!("{}/{}", "t".repeat(127), "s".repeat(127)),
            "🦀".repeat(255),
        )
    }
    fn worst_item<'a>(
        paths: (&'a Path, &'a Path),
        section: &'a str,
        media: &'a str,
        filename: &'a str,
        removed: usize,
        detail: &'a str,
        stripped: &'a [String],
    ) -> Item<'a> {
        let (source, destination) = paths;
        let mut publication = unattempted_publication();
        publication.state = "unknown";
        publication.candidate_identity = Some(identity(Identity::Windows {
            volume: u64::MAX,
            file: [255; 16],
        }));
        publication.address = "unknown";
        publication.file_sync = SyncFields {
            method: "fsync_fullfsync_unsupported",
            state: "unsupported",
            native_code: Some(i64::MIN),
        };
        publication.namespace_sync = NamespaceFields {
            sync: SyncFields {
                method: "flush_file_buffers",
                state: "unsupported",
                native_code: Some(i64::MIN),
            },
            required_edges: 513,
            completed_edges: 512,
            failed_edge_index: Some(512),
        };
        publication.cleanup = CleanupFields {
            state: "unknown",
            residual_name: Some(NativeComponent(PathBuf::from("x".repeat(255)))),
            native_code: Some(i64::MIN),
        };
        Item {
            kind: "item",
            index: 4095,
            source: PathValue::new(source, source),
            status: "failed",
            destination: Some(PathValue::new(destination, destination)),
            source_bytes: Some(268_435_456),
            copy_bytes: Some(268_435_456),
            removed: (0..removed)
                .map(|_| RemovedFields {
                    section,
                    content_type: media,
                    filename: Some(filename),
                    bytes: 268_435_456,
                    reason: "related_resource",
                })
                .collect(),
            stripped_fields: stripped,
            plan: None,
            publication,
            existing_observation: Some(ExistingFields {
                identity: Some(identity(Identity::Windows {
                    volume: u64::MAX,
                    file: [255; 16],
                })),
                bytes_equal: None,
                stable: false,
                address: "unknown",
                file_sync: SyncFields {
                    method: "fsync_fullfsync_unsupported",
                    state: "unsupported",
                    native_code: Some(i64::MIN),
                },
                namespace_sync: NamespaceFields {
                    sync: SyncFields {
                        method: "fsync_fullfsync_unsupported",
                        state: "unsupported",
                        native_code: Some(i64::MIN),
                    },
                    required_edges: 513,
                    completed_edges: 512,
                    failed_edge_index: Some(512),
                },
            }),
            error: Some(ErrorFields {
                code: "DESTINATION_UNSUPPORTED",
                phase: "publish",
                detail: Cow::Borrowed(detail),
                section: Some(section),
            }),
        }
    }
    fn stripped() -> Vec<String> {
        [
            "Content-Length",
            "Lines",
            "Content-MD5",
            "DKIM-Signature",
            "DomainKey-Signature",
            "ARC-Seal",
            "ARC-Message-Signature",
            "ARC-Authentication-Results",
            "X-MS-Has-Attach",
        ]
        .map(String::from)
        .into()
    }
    #[test]
    fn independent_maximal_item_envelope_stays_below_thirty_two_mib() {
        // Independent contract bounds. Dimension maxima form a conservative
        // superset; this is not an ordinary fixture measurement or MIME oracle.
        let source = PathBuf::from("\u{1}".repeat(32_768));
        let destination = source.clone();
        let (section, media, filename) = metadata();
        let detail = "\u{1}".repeat(1000);
        let stripped = stripped();
        let item = worst_item(
            (&source, &destination),
            &section,
            &media,
            &filename,
            10_000,
            &detail,
            &stripped,
        );
        let bytes = serde_json::to_vec(&item).unwrap();
        assert!(
            bytes.len() + 1 + 2048 < 32 * 1024 * 1024,
            "{} encoded bytes",
            bytes.len() + 1
        );
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["removed"].as_array().unwrap().len(), 10_000);
        assert!(value["existing_observation"]["bytes_equal"].is_null());
    }
    #[test]
    fn independent_maximal_run_envelope_includes_framing_and_stays_below_retention_bound() {
        let (section, media, filename) = metadata();
        let detail = "\u{1}".repeat(1000);
        let stripped = stripped();
        // 4096 items share exactly 8MiB source+destination native path charge.
        let factor = if cfg!(windows) { 2 } else { 1 };
        let units = 1024 / factor;
        let path = PathBuf::from("\u{1}".repeat(units));
        assert_eq!(4096 * 2 * units * factor, 8_388_608);
        let mut total = 0usize;
        for index in 0..4096 {
            let removed = if index < 2 { 10_000 } else { 0 };
            let item = worst_item(
                (&path, &path),
                &section,
                &media,
                &filename,
                removed,
                &detail,
                &stripped,
            );
            // Contract fixed-envelope margin covers enum/null spelling variants
            // not selected by these conservative metadata/path dimensions.
            total += serde_json::to_vec(&item).unwrap().len() + 1 + 2048;
        }
        // Independent generous start/summary allowance plus their LF framing.
        total += 2 * 16_384;
        assert!(total < 256 * 1024 * 1024, "{total} encoded bytes");
    }
    #[test]
    fn identity_domains_and_bounded_numeric_details_do_not_parse_os_prose() {
        assert_eq!(
            serde_json::to_string(&identity(Identity::Posix {
                device: 1,
                inode: 2
            }))
            .unwrap(),
            r#"{"kind":"posix","value":"1:2"}"#
        );
        let windows = identity(Identity::Windows {
            volume: u64::MAX,
            file: [255; 16],
        });
        assert_eq!(windows.kind, "windows");
        assert_eq!(
            windows.value,
            "FFFFFFFFFFFFFFFF:FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF"
        );
        assert!(windows.value.len() <= 128);
        let long = "🦀".repeat(1000);
        let suffix = " (OS error -2147483648)";
        let detail = bounded_detail(&long, suffix);
        assert_eq!(detail.chars().count(), 1000);
        assert!(detail.ends_with(suffix));
        assert!(matches!(bounded_detail("short", ""), Cow::Borrowed(_)));
    }
}
