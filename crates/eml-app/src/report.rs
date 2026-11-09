//! Stateful protocol reduction; never a second processing/publication engine.
use base64::{Engine, engine::general_purpose::STANDARD};
use eml_core::{
    MAX_COLLECTED_ITEMS, MAX_COLLECTED_PATH_BYTES, MAX_MESSAGE_BYTES, MAX_MESSAGE_PARTS,
    MAX_NAMESPACE_EDGES, MAX_REMOVAL_ENTRIES,
};
use eml_protocol::{PROGRAM_ID, records::*};
use serde::Serialize;

/// A terminal transport/contract failure; earlier accepted items remain retained.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProtocolFailure {
    /// Invalid JSON, wire map shape, closed vocabulary or required member.
    MalformedRecord,
    /// Protocol number, canonical program or expected version differs.
    IncompatibleHelper,
    /// Duplicate/out-of-order event or item index.
    InvalidOrder,
    /// Item/summary totals, interruption or collection facts disagree.
    InvalidCounts,
    /// Independent publication/address/synchronization/cleanup facts disagree.
    InvalidReceipt,
    /// Retention or encoded frame bound exceeded.
    ResourceLimit,
    /// An incomplete EOF tail was discarded.
    IncompleteTail,
    /// No accepted final summary.
    MissingSummary,
    /// The actual normal exit does not match the accepted summary.
    ExitDisagreement,
}
impl std::fmt::Display for ProtocolFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Helper protocol: {self:?}")
    }
}
impl std::error::Error for ProtocolFailure {}

/// Bounded accepted facts from one worker stream. Controls cannot rewrite them.
pub struct RunReport {
    expected_version: String,
    start: Option<StartRecord>,
    items: Vec<ItemRecord>,
    summary: Option<SummaryRecord>,
    failure: Option<ProtocolFailure>,
    counts: [u32; 4],
    removed: usize,
    path_charge_floor: usize,
    finished: bool,
    native_operands: bool,
}
impl RunReport {
    /// Start a report for the expected CLI version.
    pub fn new(expected_version: impl Into<String>) -> Self {
        Self {
            expected_version: expected_version.into(),
            start: None,
            items: Vec::new(),
            summary: None,
            failure: None,
            counts: [0; 4],
            removed: 0,
            path_charge_floor: 0,
            finished: false,
            native_operands: false,
        }
    }
    /// The bundled CLI must produce native operand encodings for this host.
    /// General report decoding remains available for independent cross-host fixtures.
    pub fn for_native_worker(expected_version: impl Into<String>) -> Self {
        let mut report = Self::new(expected_version);
        report.native_operands = true;
        report
    }
    /// Accepted item facts, retained even after a later malformed record.
    pub fn items(&self) -> &[ItemRecord] {
        &self.items
    }
    /// Accepted discovery facts, absent while Core is still scanning.
    pub fn start(&self) -> Option<&StartRecord> {
        self.start.as_ref()
    }
    /// Accepted final summary, if any.
    pub fn summary(&self) -> Option<&SummaryRecord> {
        self.summary.as_ref()
    }
    /// Known discovered items lacking individual results after a stream ends
    /// without an accepted summary. This is reporting incompleteness, never
    /// evidence that native processing/publication did not happen. A valid
    /// summary remains authoritative, including its not_processed count.
    pub fn unreported_item_count(&self) -> Option<u32> {
        if !self.finished || self.summary.is_some() {
            return None;
        }
        self.start
            .as_ref()?
            .total
            .checked_sub(u32::try_from(self.items.len()).ok()?)
    }
    /// The first irreversible protocol failure.
    pub fn failure(&self) -> Option<ProtocolFailure> {
        self.failure
    }
    /// Prepared source/copy size reduction, independent of publication.
    /// This is not disk space freed or a claim that a copy exists.
    pub fn copy_size_reduction(&self, position: usize) -> Option<u64> {
        let item = self.items.get(position)?;
        item.source_bytes
            .value()?
            .checked_sub(*item.copy_bytes.value()?)
    }
    /// Exact native navigation from an earlier usable receipt. This does not
    /// requalify current identity/address. Relative or foreign-host paths refuse.
    pub fn reveal_path(&self, position: usize) -> Option<std::path::PathBuf> {
        let item = self.items.get(position)?;
        if !item.publication.is_qualified()
            && !item
                .existing_observation
                .value()
                .is_some_and(ExistingRecord::is_qualified)
        {
            return None;
        }
        self.destination_path(position)
    }
    /// Native intended source path, without claiming current existence or identity.
    pub fn source_path(&self, position: usize) -> Option<std::path::PathBuf> {
        self.items.get(position)?.source.native_absolute_path()
    }
    /// Native intended destination, independent of publication/reveal qualification.
    pub fn destination_path(&self, position: usize) -> Option<std::path::PathBuf> {
        self.items
            .get(position)?
            .destination
            .value()?
            .native_absolute_path()
    }
    /// Domain totals for one accepted settled report, including work without item events.
    pub fn settled_totals(&self) -> Option<eml_core::vocabulary::BatchTotals> {
        let summary = self.summary()?;
        Some(eml_core::vocabulary::BatchTotals::from(
            self.items.iter().map(|item| {
                (
                    Some(item.outcome()),
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
    /// Advisory preview availability; core always readmits any subsequent request.
    pub fn can_offer_copy(&self) -> bool {
        eml_core::vocabulary::can_offer_copy(
            self.items.iter().map(ItemRecord::outcome),
            self.summary()
                .and_then(|s| s.run_error.value())
                .and_then(|e| eml_core::ErrorCode::from_wire_name(&e.code)),
        )
    }
    /// All discovery results are accounted for only after framing and exit settlement.
    pub fn results_cover_inventory(&self) -> bool {
        self.finished
            && self.failure.is_none()
            && self.summary().is_some_and(|s| {
                let error = s
                    .run_error
                    .value()
                    .map(|e| eml_core::ErrorCode::from_wire_name(&e.code));
                if error.is_some_and(|e| e.is_none()) {
                    return false;
                }
                eml_core::vocabulary::results_cover_inventory(
                    s.collection_complete,
                    s.interrupted,
                    s.counts.not_processed as usize,
                    error.flatten(),
                )
            })
    }
    /// Latch framing/reader failures without discarding accepted observations.
    pub fn invalidate(&mut self, failure: ProtocolFailure) {
        if self.failure.is_none() {
            self.failure = Some(failure);
        }
    }
    /// Decode and accept exactly one complete LF-free event.
    pub fn accept_frame(&mut self, frame: &[u8]) -> Result<(), ProtocolFailure> {
        if let Some(failure) = self.failure {
            return Err(failure);
        }
        let result = if frame.len() >= eml_protocol::MAX_LINE_BYTES {
            Err(ProtocolFailure::ResourceLimit)
        } else {
            decode_record(frame)
                .map_err(|_| ProtocolFailure::MalformedRecord)
                .and_then(|event| self.accept(event))
        };
        if let Err(failure) = result {
            self.invalidate(failure);
        }
        result
    }
    fn accept(&mut self, event: EventRecord) -> Result<(), ProtocolFailure> {
        if self.finished || self.summary.is_some() {
            return Err(ProtocolFailure::InvalidOrder);
        }
        match event {
            EventRecord::Start(start) => {
                if self.start.is_some() || !self.items.is_empty() {
                    return Err(ProtocolFailure::InvalidOrder);
                }
                if start.protocol != 1
                    || start.program != PROGRAM_ID
                    || start.version != self.expected_version
                {
                    return Err(ProtocolFailure::IncompatibleHelper);
                }
                if start.total as usize > MAX_COLLECTED_ITEMS {
                    return Err(ProtocolFailure::InvalidCounts);
                }
                self.start = Some(start);
            }
            EventRecord::Item(item) => {
                let start = self.start.as_ref().ok_or(ProtocolFailure::InvalidOrder)?;
                if item.index >= start.total
                    || self
                        .items
                        .last()
                        .is_some_and(|previous| previous.index >= item.index)
                {
                    return Err(ProtocolFailure::InvalidOrder);
                }
                let dry_run = start.dry_run;
                self.validate_item(&item, dry_run)?;
                let index = match item.status {
                    ItemStatus::Created => 0,
                    ItemStatus::UpToDate => 1,
                    ItemStatus::Planned => 2,
                    ItemStatus::Failed => 3,
                };
                self.counts[index] += 1;
                self.items.push(*item);
            }
            EventRecord::Summary(summary) => {
                let start = self.start.as_ref().ok_or(ProtocolFailure::InvalidOrder)?;
                let c = &summary.counts;
                if [
                    c.created,
                    c.up_to_date,
                    c.planned,
                    c.failed,
                    c.not_processed,
                ]
                .iter()
                .any(|count| *count as usize > MAX_COLLECTED_ITEMS)
                    || [c.created, c.up_to_date, c.planned, c.failed] != self.counts
                    || c.not_processed != start.total - self.counts.iter().sum::<u32>()
                    || summary.collection_complete != start.collection_complete
                {
                    return Err(ProtocolFailure::InvalidCounts);
                }
                if let Some(error) = summary.run_error.value() {
                    validate_error(error)?;
                }
                let expected = if summary.interrupted {
                    130
                } else if c.failed > 0 || summary.run_error.value().is_some() {
                    1
                } else {
                    0
                };
                if summary.exit_code != expected
                    || (!summary.interrupted
                        && summary.run_error.value().is_none()
                        && (c.not_processed != 0 || start.total == 0))
                    || (expected == 0 && !start.collection_complete)
                {
                    return Err(ProtocolFailure::InvalidCounts);
                }
                self.summary = Some(summary);
            }
        }
        Ok(())
    }
    /// Finalize only after actual terminal reap, both EOFs and writer settlement.
    /// A signalled exit is None, never normalized to a matching numeric code.
    pub fn finish(
        &mut self,
        normal_exit: Option<i32>,
        incomplete_tail: bool,
    ) -> Result<(), ProtocolFailure> {
        self.finished = true;
        let result = if let Some(failure) = self.failure {
            Err(failure)
        } else if incomplete_tail {
            Err(ProtocolFailure::IncompleteTail)
        } else {
            self.summary
                .as_ref()
                .ok_or(ProtocolFailure::MissingSummary)
                .and_then(|summary| {
                    if normal_exit == Some(summary.exit_code) {
                        Ok(())
                    } else {
                        Err(ProtocolFailure::ExitDisagreement)
                    }
                })
        };
        if let Err(failure) = result {
            self.invalidate(failure);
        }
        result
    }
    fn admit_path(&mut self, path: &ReportPathRecord) -> Result<(), ProtocolFailure> {
        if self.native_operands && !path.native.encoding.is_native() {
            return Err(ProtocolFailure::IncompatibleHelper);
        }
        let native = validate_native(&path.native, MAX_COLLECTED_PATH_BYTES)?;
        if self.native_operands && path.native_absolute_path().is_none() {
            return Err(ProtocolFailure::IncompatibleHelper);
        }
        // Original Unix display bytes cannot be recovered from lossy text:
        // each U+FFFD might represent one bad byte or a literal three bytes.
        // Windows replacement preserves the exact original UTF16-unit count.
        // This necessary floor never rejects valid Core producer accounting;
        // it also bounds retained native8MiB/displayUTF8 at most24MiB.
        let display_floor = match path.native.encoding {
            PathEncoding::UnixBytes => {
                path.display.len()
                    - 2 * path
                        .display
                        .chars()
                        .filter(|character| *character == '\u{fffd}')
                        .count()
            }
            PathEncoding::WindowsUtf16Le => path.display.encode_utf16().count() * 2,
        };
        let charge = native.len().max(display_floor);
        if charge > MAX_COLLECTED_PATH_BYTES - self.path_charge_floor {
            return Err(ProtocolFailure::ResourceLimit);
        }
        self.path_charge_floor += charge;
        Ok(())
    }
    fn validate_item(&mut self, item: &ItemRecord, dry_run: bool) -> Result<(), ProtocolFailure> {
        self.admit_path(&item.source)?;
        if let Some(path) = item.destination.value() {
            self.admit_path(path)?;
        }
        if self.native_operands
            && item
                .publication
                .cleanup
                .residual_name
                .value()
                .is_some_and(|name| !name.encoding.is_native())
        {
            return Err(ProtocolFailure::IncompatibleHelper);
        }
        validate_publication(&item.publication)?;
        if let Some(existing) = item.existing_observation.value() {
            validate_existing(existing)?;
        }
        if let Some(error) = item.error.value() {
            validate_error(error)?;
        }
        let source = item.source_bytes.value().copied();
        let copy = item.copy_bytes.value().copied();
        if item.removed.len() > MAX_MESSAGE_PARTS
            || item.removed.len() > MAX_REMOVAL_ENTRIES - self.removed
            || item.stripped_fields.len() > 9
            || item.stripped_fields.iter().any(|field| field.len() > 128)
            || source.is_some_and(|bytes| bytes > MAX_MESSAGE_BYTES as u64)
            || copy.is_some_and(|bytes| bytes > MAX_MESSAGE_BYTES as u64)
            || source.is_some() != copy.is_some()
            || copy.zip(source).is_some_and(|(copy, source)| copy > source)
        {
            return Err(ProtocolFailure::MalformedRecord);
        }
        for part in &item.removed {
            if part.section.is_empty()
                || part.section.len() > 319
                || part.content_type.is_empty()
                || part.content_type.len() > 255
                || part
                    .filename
                    .value()
                    .is_some_and(|name| name.chars().count() > 255)
                || part.bytes > MAX_MESSAGE_BYTES as u64
            {
                return Err(ProtocolFailure::MalformedRecord);
            }
        }
        self.removed += item.removed.len();
        let p = &item.publication;
        let no_sync = p.file_sync.state == Synchronization::NotAttempted
            && p.namespace_sync.state == Synchronization::NotAttempted
            && item.existing_observation.value().is_none_or(|existing| {
                existing.file_sync.state == Synchronization::NotAttempted
                    && existing.namespace_sync.state == Synchronization::NotAttempted
            });
        let meaning = item
            .outcome_facts()
            .map(|facts| eml_core::vocabulary::classify(&facts).kind);
        use eml_core::vocabulary::MeaningKind;
        let valid = match item.status {
            ItemStatus::Created => {
                !dry_run
                    && source.is_some()
                    && item.destination.value().is_some()
                    && meaning == Some(MeaningKind::Created)
            }
            ItemStatus::UpToDate => {
                !dry_run
                    && source.is_some()
                    && item.destination.value().is_some()
                    && meaning == Some(MeaningKind::ExistingVerified)
                    && p.file_sync.state == Synchronization::NotAttempted
                    && p.namespace_sync.state == Synchronization::NotAttempted
            }
            ItemStatus::Planned => {
                dry_run
                    && source.is_some()
                    && item.destination.value().is_some()
                    && matches!(
                        meaning,
                        Some(MeaningKind::Prepared | MeaningKind::Comparison)
                    )
            }
            ItemStatus::Failed => {
                item.error.value().is_some()
                    && item.plan.value().is_none()
                    && (!dry_run || (p.state == Publication::NotPublished && no_sync))
            }
        };
        if valid {
            Ok(())
        } else {
            Err(ProtocolFailure::InvalidReceipt)
        }
    }
}
fn validate_native(path: &NativePathRecord, maximum: usize) -> Result<Vec<u8>, ProtocolFailure> {
    if path.base64.len() > maximum.div_ceil(3) * 4 {
        return Err(ProtocolFailure::ResourceLimit);
    }
    let bytes = STANDARD
        .decode(&path.base64)
        .map_err(|_| ProtocolFailure::MalformedRecord)?;
    if bytes.len() > maximum || STANDARD.encode(&bytes) != path.base64 {
        return Err(ProtocolFailure::MalformedRecord);
    }
    if match path.encoding {
        PathEncoding::UnixBytes => bytes.contains(&0),
        PathEncoding::WindowsUtf16Le => {
            !bytes.len().is_multiple_of(2) || bytes.as_chunks::<2>().0.contains(&[0, 0])
        }
    } {
        return Err(ProtocolFailure::MalformedRecord);
    }
    Ok(bytes)
}
fn validate_identity(identity: &IdentityRecord) -> Result<(), ProtocolFailure> {
    if !["posix", "windows"].contains(&identity.kind.as_str())
        || identity.value.is_empty()
        || identity.value.len() > 128
        || !identity
            .value
            .bytes()
            .all(|byte| (0x21..0x7f).contains(&byte))
    {
        Err(ProtocolFailure::InvalidReceipt)
    } else {
        Ok(())
    }
}
fn validate_sync(
    method: SynchronizationMethod,
    state: Synchronization,
    code: Option<&i64>,
) -> Result<(), ProtocolFailure> {
    let record = SyncRecord {
        method,
        state,
        native_code: code.copied().map_or(Nullable::Null(()), Nullable::Value),
    };
    let receipt = record
        .core_receipt()
        .ok_or(ProtocolFailure::InvalidReceipt)?;
    if state == Synchronization::Succeeded && !receipt.is_some_and(eml_core::SyncReceipt::succeeded)
    {
        Err(ProtocolFailure::InvalidReceipt)
    } else {
        Ok(())
    }
}
fn validate_file(sync: &SyncRecord) -> Result<(), ProtocolFailure> {
    validate_sync(sync.method, sync.state, sync.native_code.value())
}
fn validate_namespace(sync: &NamespaceRecord) -> Result<(), ProtocolFailure> {
    validate_sync(sync.method, sync.state, sync.native_code.value())?;
    if sync.required_edges > u32::from(MAX_NAMESPACE_EDGES)
        || sync.completed_edges > sync.required_edges
        || sync
            .failed_edge_index
            .value()
            .is_some_and(|index| *index >= sync.required_edges)
        || (sync.state == Synchronization::NotAttempted
            && (sync.completed_edges != 0 || sync.failed_edge_index.value().is_some()))
        || (sync.state == Synchronization::Succeeded && sync.failed_edge_index.value().is_some())
    {
        Err(ProtocolFailure::InvalidReceipt)
    } else {
        Ok(())
    }
}
fn validate_publication(publication: &PublicationRecord) -> Result<(), ProtocolFailure> {
    if let Some(identity) = publication.candidate_identity.value() {
        validate_identity(identity)?;
    }
    validate_file(&publication.file_sync)?;
    validate_namespace(&publication.namespace_sync)?;
    let cleanup = &publication.cleanup;
    if let Some(name) = cleanup.residual_name.value() {
        let maximum = match name.encoding {
            PathEncoding::UnixBytes => 255,
            PathEncoding::WindowsUtf16Le => 510,
        };
        let bytes = validate_native(name, maximum)?;
        if !matches!(cleanup.state, Cleanup::Residual | Cleanup::Unknown)
            || bytes.is_empty()
            || match name.encoding {
                PathEncoding::UnixBytes => bytes.len() > 255 || bytes.contains(&b'/'),
                PathEncoding::WindowsUtf16Le => {
                    bytes.len() > 510
                        || bytes
                            .as_chunks::<2>()
                            .0
                            .iter()
                            .any(|pair| pair[1] == 0 && b"/\\".contains(&pair[0]))
                }
            }
        {
            return Err(ProtocolFailure::InvalidReceipt);
        }
    }
    if (cleanup.state == Cleanup::Complete && cleanup.residual_name.value().is_some())
        || (cleanup.state == Cleanup::NotNeeded
            && (cleanup.residual_name.value().is_some() || cleanup.native_code.value().is_some()))
        || (publication.state == Publication::Published
            && publication.candidate_identity.value().is_none())
        || (publication.state == Publication::Unknown && publication.address == Address::Verified)
    {
        return Err(ProtocolFailure::InvalidReceipt);
    }
    Ok(())
}
fn validate_existing(existing: &ExistingRecord) -> Result<(), ProtocolFailure> {
    if let Some(identity) = existing.identity.value() {
        validate_identity(identity)?;
    }
    validate_file(&existing.file_sync)?;
    validate_namespace(&existing.namespace_sync)
}
fn validate_error(error: &ErrorRecord) -> Result<(), ProtocolFailure> {
    if error.code.is_empty()
        || error.code.len() > 128
        || error.detail.chars().count() > 1000
        || error
            .section
            .value()
            .is_some_and(|section| section.len() > 319)
    {
        Err(ProtocolFailure::MalformedRecord)
    } else if eml_core::ErrorCode::from_wire_name(&error.code)
        .is_some_and(|code| !code.allows_phase(error.phase))
    {
        Err(ProtocolFailure::InvalidReceipt)
    } else {
        Ok(())
    }
}
