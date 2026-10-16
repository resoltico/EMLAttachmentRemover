//! Ordered bounded read-only inventory and explicit-output separation.
use crate::{
    COPY_SUFFIX, ErrorCode, ErrorPhase, MAX_PATH_UNITS, OperationFailure, OutputLocation,
    RequestPathError, RunControl, RunRequest,
    request::{native_bytes, path_limits},
    selection::{SelectionError, WorkingContext, append},
};
use eml_fs::{BoundDirectory, EntryKind, FileSignature, Identity};
use std::{
    borrow::Cow,
    collections::{HashMap, hash_map::RandomState},
    ffi::{OsStr, OsString},
    hash::BuildHasher,
    io,
    path::{Path, PathBuf},
};

/// Maximum complete reported source/destination path charge across a run.
pub const MAX_COLLECTED_PATH_BYTES: usize = 8 * 1024 * 1024;
/// Maximum retained file/failure work positions, distinct from raw argument count.
pub const MAX_COLLECTED_ITEMS: usize = 4096;
/// Maximum names examined, including entries excluded from file work.
pub const MAX_DIRECTORY_ENTRIES: usize = 100_000;
/// Maximum directory nesting within a selected root, whose depth is one.
pub const MAX_WALK_DEPTH: usize = 64;

/// A request cannot enter collection; no event or write has started.
#[derive(Debug)]
pub enum InvalidRequest {
    /// The explicit read-only operation requires a read-only request.
    DryRunModeRequired,
    /// Pure native representation/resource validation failed.
    Paths(RequestPathError),
    /// The supplied absolute working directory could not bind.
    WorkingDirectory(io::Error),
    /// The existing output directory could not bind as an ordinary directory.
    OutputDirectory(io::Error),
}
impl std::fmt::Display for InvalidRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DryRunModeRequired => f.write_str("dry_run requires RunMode::DryRun"),
            Self::Paths(error) => error.fmt(f),
            Self::WorkingDirectory(error) => {
                write!(f, "Cannot bind the supplied working directory: {error}")
            }
            Self::OutputDirectory(error) => {
                write!(f, "Cannot bind the existing output directory: {error}")
            }
        }
    }
}
impl std::error::Error for InvalidRequest {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(match self {
            Self::DryRunModeRequired => return None,
            Self::Paths(error) => error,
            Self::WorkingDirectory(error) | Self::OutputDirectory(error) => error,
        })
    }
}

/// Actual inventory/separation outcome. Collected is not permission to publish:
/// naming, reservations, source preparation and final bindings remain required.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CollectionState {
    /// Read-only inventory and required output separation completed.
    Collected,
    /// At least one explicit source root could not bind.
    InputAdmissionFailed,
    /// A shared discovery or retained-path resource bound stopped collection.
    InputLimitExceeded,
    /// Cooperative stop left a bounded known prefix; no file processing started.
    Interrupted,
    /// A selected ordinary-directory route reached the output's full identity.
    OutputOverlap,
    /// Required separation or captured-context observations could not complete.
    Unverified,
}
/// Which collection contract produced a retained row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceOrigin {
    /// Explicit file selection or failed explicit-root admission.
    Explicit,
    /// Work or discovery failure below a successfully bound selected directory.
    Walked,
}
/// Ordinary-file metadata or an actual failure retained in discovered order.
#[derive(Clone, Debug)]
pub enum CollectedObservation {
    /// Data remains unopened. Both facts must be revalidated before preparation.
    File {
        /// Inventory signature, excluding access time.
        signature: FileSignature,
        /// Physical parent of the selected entry, including selected file aliases.
        parent: Identity,
    },
    /// Failure provenance is separately retained in SourceOrigin.
    Failed(OperationFailure),
}
/// One immutable bounded work/failure position. Its index is not an emission ordinal.
pub struct CollectedSource<'a> {
    pub(super) root: usize,
    suffix: PathBuf,
    native: Cow<'a, Path>,
    origin: SourceOrigin,
    observation: CollectedObservation,
}
impl CollectedSource<'_> {
    /// Explicit selection versus descendant discovery provenance.
    pub fn origin(&self) -> SourceOrigin {
        self.origin
    }
    /// Actual inventory metadata or typed collection failure.
    pub fn observation(&self) -> &CollectedObservation {
        &self.observation
    }
    /// Admitted reported native expression, or intended failed-root input.
    pub fn native_path(&self) -> &Path {
        &self.native
    }
    /// Flat native suffix below its selected directory; empty for explicit roots.
    pub fn relative_path(&self) -> &Path {
        &self.suffix
    }
}
pub(super) enum RootKind {
    File(FileSignature, Identity),
    Directory(Identity),
    Failed(OperationFailure),
}
pub(super) struct Root {
    pub(super) request_index: usize,
    pub(super) kind: RootKind,
}
/// Read-only collection observations borrowing the immutable request. Captured
/// filesystem handles remain private and cannot become a frontend write capability.
pub struct CollectedSources<'a> {
    pub(super) request: &'a RunRequest,
    pub(super) context: Option<WorkingContext>,
    pub(super) output: Option<BoundDirectory>,
    pub(super) roots: Vec<Root>,
    items: Vec<CollectedSource<'a>>,
    state: CollectionState,
    complete: bool,
    examined: usize,
    path_bytes: usize,
    error: Option<OperationFailure>,
}
impl<'a> CollectedSources<'a> {
    fn empty(request: &'a RunRequest) -> Self {
        Self {
            request,
            context: None,
            output: None,
            roots: Vec::new(),
            items: Vec::new(),
            state: CollectionState::Collected,
            complete: false,
            examined: 0,
            path_bytes: 0,
            error: None,
        }
    }
    /// Authoritative outcome; Ok from collect_sources does not imply completion.
    pub fn state(&self) -> CollectionState {
        self.state
    }
    /// Whether the required discovery pass actually completed.
    pub fn collection_complete(&self) -> bool {
        self.complete
    }
    /// Known bounded work/failure positions only, never unexpanded folder contents.
    pub fn items(&self) -> &[CollectedSource<'_>] {
        &self.items
    }
    /// Raw explicit argument index for an existing retained work position.
    pub fn request_index(&self, index: usize) -> Option<usize> {
        self.items
            .get(index)
            .map(|item| self.roots[item.root].request_index)
    }
    /// Original request-derived display expression; never use it for native I/O.
    pub fn display_path(&self, index: usize) -> Option<Cow<'_, Path>> {
        let item = self.items.get(index)?;
        let root = &self.request.paths[self.roots[item.root].request_index];
        Some(if item.suffix.as_os_str().is_empty() {
            Cow::Borrowed(root)
        } else {
            Cow::Owned(append(root, &item.suffix))
        })
    }
    /// Names actually examined, including skipped/qualification names.
    pub fn examined_entries(&self) -> usize {
        self.examined
    }
    /// Full logical source path charge so far; planning must also admit destinations.
    pub fn source_path_bytes(&self) -> usize {
        self.path_bytes
    }
    /// Aggregate failure, separate from per-position observations and stop.
    pub fn run_error(&self) -> Option<&OperationFailure> {
        self.error.as_ref()
    }
}

/// Collect in request-order/native-name DFS without reading message bodies or
/// creating/synchronizing entries. Explicit output also requires a complete
/// unfiltered ordinary-directory separation pass. Final planning and all write
/// capabilities remain separate; no CLI/JSON/signal/global-cwd state is consulted.
pub fn collect_sources<'a>(
    request: &'a RunRequest,
    control: &RunControl,
) -> Result<CollectedSources<'a>, InvalidRequest> {
    let random = RandomState::new();
    collect_with_hash(request, control, move |path| random.hash_one(path))
}
fn collect_with_hash<'a>(
    request: &'a RunRequest,
    control: &RunControl,
    hash: impl Fn(&OsStr) -> u64,
) -> Result<CollectedSources<'a>, InvalidRequest> {
    request
        .validate_path_limits()
        .map_err(InvalidRequest::Paths)?;
    let empty = CollectedSources::empty(request);
    let mut collector = Collector {
        result: empty,
        control,
        hash,
        seen: HashMap::new(),
        unknown: false,
    };
    if !collector.checkpoint() {
        return Ok(collector.result);
    }
    let context = WorkingContext::capture(&request.working_directory)
        .map_err(InvalidRequest::WorkingDirectory)?;
    if let OutputLocation::Directory(path) = &request.output {
        let observed = context
            .observe(path)
            .map_err(|error| InvalidRequest::OutputDirectory(selection_io(error)))?;
        if observed.kind != EntryKind::Directory {
            return Err(InvalidRequest::OutputDirectory(io::Error::new(
                io::ErrorKind::NotADirectory,
                "Output is not an ordinary directory",
            )));
        }
        let directory = context
            .directory(path)
            .map_err(|error| InvalidRequest::OutputDirectory(selection_io(error)))?;
        if directory.identity() != observed.identity {
            return Err(InvalidRequest::OutputDirectory(io::Error::new(
                io::ErrorKind::InvalidData,
                "Output root identity changed",
            )));
        }
        collector.result.output = Some(directory);
    }
    collector.result.context = Some(context);
    if !collector.bind_roots() {
        return Ok(collector.result);
    }
    let failed = collector
        .result
        .roots
        .iter()
        .any(|root| matches!(root.kind, RootKind::Failed(_)));
    for root in 0..collector.result.roots.len() {
        if !collector.checkpoint() {
            break;
        }
        match &collector.result.roots[root].kind {
            RootKind::File(signature, parent) => {
                let observation = CollectedObservation::File {
                    signature: *signature,
                    parent: *parent,
                };
                collector.push(root, PathBuf::new(), SourceOrigin::Explicit, observation);
            }
            RootKind::Failed(error) => {
                let error = error.clone();
                collector.push(
                    root,
                    PathBuf::new(),
                    SourceOrigin::Explicit,
                    CollectedObservation::Failed(error),
                );
            }
            RootKind::Directory(expected) if !failed && !collector.root_failed() => {
                let expected = *expected;
                let expression = collector.expression(root);
                match collector.context().directory(expression) {
                    Ok(directory) if directory.identity() == expected => {
                        collector.walk(root, directory, PathBuf::new(), true, 1)
                    }
                    Ok(_) => collector.fail_root(
                        root,
                        OperationFailure::new(
                            ErrorCode::SourceChanged,
                            ErrorPhase::Collection,
                            "Selected directory changed before walking",
                        ),
                    ),
                    Err(error) => {
                        collector.fail_root(root, selection_failure(ErrorPhase::Collection, error))
                    }
                }
            }
            RootKind::Directory(_) => {}
        }
        if !collector.active() {
            break;
        }
    }
    if collector.active() && collector.root_failed() {
        collector.block(
            CollectionState::InputAdmissionFailed,
            OperationFailure::new(
                ErrorCode::InputAdmissionFailed,
                ErrorPhase::Admission,
                "At least one explicit source root could not bind",
            ),
        );
    } else if collector.active() {
        collector.result.complete = !collector.unknown;
        match collector.context().check_working_address() {
            Ok(true) => {}
            Ok(false) => collector.block(
                CollectionState::Unverified,
                OperationFailure::new(
                    ErrorCode::BindFailed,
                    ErrorPhase::Admission,
                    "Captured working-directory address changed",
                ),
            ),
            Err(error) => collector.unverified(
                "Captured working-directory address could not be verified",
                error,
            ),
        }
    }
    collector.checkpoint();
    Ok(collector.result)
}
fn selection_io(error: SelectionError) -> io::Error {
    match error {
        SelectionError::Io(error) => error,
        #[cfg(windows)]
        SelectionError::Changed => io::Error::new(
            io::ErrorKind::InvalidData,
            "Selected directory identity changed",
        ),
    }
}
fn selection_failure(phase: ErrorPhase, error: SelectionError) -> OperationFailure {
    match error {
        SelectionError::Io(error) => OperationFailure::io(
            ErrorCode::ReadFailed,
            phase,
            "Selected path could not be inspected or opened",
            error,
        ),
        #[cfg(windows)]
        SelectionError::Changed => OperationFailure::new(
            ErrorCode::SourceChanged,
            phase,
            "Selected directory identity changed",
        ),
    }
}
struct Collector<'a, 'c, F> {
    result: CollectedSources<'a>,
    control: &'c RunControl,
    hash: F,
    seen: HashMap<u64, Vec<usize>>,
    unknown: bool,
}
impl<'a, F: Fn(&OsStr) -> u64> Collector<'a, '_, F> {
    fn context(&self) -> &WorkingContext {
        self.result
            .context
            .as_ref()
            .expect("context captured before native selection")
    }
    fn expression(&self, root: usize) -> &'a Path {
        &self.result.request.paths[self.result.roots[root].request_index]
    }
    fn active(&self) -> bool {
        self.result.state == CollectionState::Collected
    }
    fn checkpoint(&mut self) -> bool {
        if self.control.is_stopped() {
            self.result.state = CollectionState::Interrupted;
            return false;
        }
        self.active()
    }
    fn block(&mut self, state: CollectionState, error: OperationFailure) {
        self.result.state = state;
        self.result.error = Some(error);
        if matches!(
            state,
            CollectionState::InputAdmissionFailed | CollectionState::InputLimitExceeded
        ) {
            self.result.complete = false;
        }
    }
    fn limit(&mut self, detail: &'static str) {
        self.result.complete = false;
        self.block(
            CollectionState::InputLimitExceeded,
            OperationFailure::new(
                ErrorCode::InputLimitExceeded,
                ErrorPhase::Collection,
                detail,
            ),
        );
    }
    fn unverified(&mut self, detail: &'static str, error: io::Error) {
        let failure =
            OperationFailure::io(ErrorCode::BindFailed, ErrorPhase::Admission, detail, error);
        self.block(CollectionState::Unverified, failure);
    }
    fn block_observed_overlap(&mut self, identity: Identity) -> bool {
        if self
            .result
            .output
            .as_ref()
            .is_some_and(|output| output.identity() == identity)
        {
            self.block(
                CollectionState::OutputOverlap,
                OperationFailure::new(
                    ErrorCode::OutputOverlap,
                    ErrorPhase::Admission,
                    "Output directory is reachable below a selected directory",
                ),
            );
            true
        } else {
            false
        }
    }
    fn root_failed(&self) -> bool {
        self.result
            .roots
            .iter()
            .any(|root| matches!(root.kind, RootKind::Failed(_)))
    }
    fn bind_roots(&mut self) -> bool {
        let mut seen: HashMap<u64, Vec<usize>> = HashMap::new();
        'arguments: for request_index in 0..self.result.request.paths.len() {
            if !self.checkpoint() {
                return false;
            }
            let expression = &self.result.request.paths[request_index];
            let operand = self.context().operand(expression, Path::new(""));
            let mut key_hash = None;
            if let Ok(native) = &operand {
                let key = self.context().key(native);
                let hash = (self.hash)(key.as_os_str());
                key_hash = Some(hash);
                for &old in seen.get(&hash).into_iter().flatten() {
                    if self.control.is_stopped() {
                        self.checkpoint();
                        return false;
                    }
                    // Equality is optional: inability to rebuild an older
                    // candidate means bind/traverse this root independently,
                    // not a fabricated failure of this usable selection.
                    if let Ok(previous) =
                        self.context().operand(self.expression(old), Path::new(""))
                    {
                        // A cross-anchor route can expose a different mounted
                        // view even with the same directory identity. Bind
                        // and walk both; only observed leaf work may coalesce.
                        if previous.is_absolute() == native.is_absolute()
                            && self.context().same_spelling(&previous, native)
                        {
                            continue 'arguments;
                        }
                    }
                }
            }
            let kind = (|| -> Result<RootKind, OperationFailure> {
                operand.map_err(|error| {
                    OperationFailure::io(
                        ErrorCode::ReadFailed,
                        ErrorPhase::Admission,
                        "Selected expression could not be admitted",
                        error,
                    )
                })?;
                let observed = self
                    .context()
                    .observe(expression)
                    .map_err(|error| selection_failure(ErrorPhase::Admission, error))?;
                match observed.kind {
                    EntryKind::File => {
                        let parent = self
                            .context()
                            .parent_identity(expression)
                            .map_err(|error| selection_failure(ErrorPhase::Admission, error))?;
                        Ok(RootKind::File(
                            observed.file.expect("ordinary observation has signature"),
                            parent,
                        ))
                    }
                    EntryKind::Directory => {
                        let directory = self
                            .context()
                            .directory(expression)
                            .map_err(|error| selection_failure(ErrorPhase::Admission, error))?;
                        if directory.identity() != observed.identity {
                            return Err(OperationFailure::new(
                                ErrorCode::SourceChanged,
                                ErrorPhase::Admission,
                                "Selected root identity changed",
                            ));
                        }
                        Ok(RootKind::Directory(observed.identity))
                    }
                    _ => Err(OperationFailure::new(
                        ErrorCode::SourceNotRegular,
                        ErrorPhase::Admission,
                        "Explicit selection is not an ordinary file or directory",
                    )),
                }
            })();
            if !self.checkpoint() {
                return false;
            }
            let index = self.result.roots.len();
            self.result.roots.push(Root {
                request_index,
                kind: kind.unwrap_or_else(RootKind::Failed),
            });
            if let Some(hash) = key_hash {
                seen.entry(hash).or_default().push(index);
            }
        }
        true
    }
    fn fail_root(&mut self, root: usize, error: OperationFailure) {
        self.result.roots[root].kind = RootKind::Failed(error.clone());
        self.push(
            root,
            PathBuf::new(),
            SourceOrigin::Explicit,
            CollectedObservation::Failed(error),
        );
    }
    fn push(
        &mut self,
        root: usize,
        suffix: PathBuf,
        origin: SourceOrigin,
        observation: CollectedObservation,
    ) {
        if !self.checkpoint() {
            return;
        }
        let expression = self.expression(root);
        let display = append(expression, &suffix);
        if path_limits(&display).is_err() {
            self.limit("Discovered path exceeds native spelling bounds");
            return;
        }
        let (native, key_available) = match self.context().operand(expression, &suffix) {
            Ok(path) => (path, true),
            Err(_)
                if origin == SourceOrigin::Explicit
                    && matches!(observation, CollectedObservation::Failed(_)) =>
            {
                (expression.to_owned(), false)
            }
            Err(error) => {
                self.unverified(
                    "Discovered native expression could not be represented",
                    error,
                );
                return;
            }
        };
        let native_size = native_bytes(&native);
        if native_size > MAX_PATH_UNITS * if cfg!(windows) { 2 } else { 1 } {
            self.limit("Reported native path exceeds the native unit bound");
            return;
        }
        // A failed requested-root binding is a mandatory diagnostic, not another
        // admitted file-work occurrence. It must neither be suppressed by nor
        // become a representative in the ordinary work dedupe index.
        let admitted_work = key_available
            && !(origin == SourceOrigin::Explicit
                && matches!(observation, CollectedObservation::Failed(_)));
        let key = admitted_work.then(|| self.context().key(&native));
        let hash = key.as_ref().map(|key| (self.hash)(key.as_os_str()));
        for &old in hash
            .and_then(|hash| self.seen.get(&hash))
            .into_iter()
            .flatten()
        {
            if self.control.is_stopped() {
                self.checkpoint();
                return;
            }
            let previous = &self.result.items[old];
            if self.context().same_spelling(&previous.native, &native)
                && (previous.native.is_absolute() == native.is_absolute()
                    || same_file_binding(&previous.observation, &observation))
            {
                return;
            }
        }
        if self.result.items.len() == MAX_COLLECTED_ITEMS {
            self.limit("Collected work exceeds the item bound");
            return;
        }
        let Some(total) = path_total(self.result.path_bytes, native_bytes(&display), native_size)
        else {
            self.limit("Collected source paths exceed the report budget");
            return;
        };
        if !self.checkpoint() {
            return;
        }
        let native =
            if suffix.as_os_str().is_empty() && native.as_os_str() == expression.as_os_str() {
                Cow::Borrowed(expression)
            } else {
                Cow::Owned(native)
            };
        let index = self.result.items.len();
        self.result.items.push(CollectedSource {
            root,
            suffix,
            native,
            origin,
            observation,
        });
        self.result.path_bytes = total;
        if let Some(hash) = hash {
            self.seen.entry(hash).or_default().push(index);
        }
    }
    fn walk(
        &mut self,
        root: usize,
        directory: BoundDirectory,
        suffix: PathBuf,
        eligible: bool,
        depth: usize,
    ) {
        #[cfg(windows)]
        let mut directory = directory;
        if !self.checkpoint() {
            return;
        }
        if self.block_observed_overlap(directory.identity()) {
            return;
        }
        let mut names = Vec::new();
        let entries = match directory.read_entries() {
            Ok(entries) => entries,
            Err(error) => {
                self.discovery_error(
                    root,
                    suffix,
                    "Selected directory could not be enumerated",
                    error,
                );
                return;
            }
        };
        for entry in entries {
            if !self.checkpoint() {
                return;
            }
            match entry {
                Ok(name) => {
                    if self.result.examined == MAX_DIRECTORY_ENTRIES {
                        self.limit("Directory examination exceeds the entry bound");
                        return;
                    }
                    self.result.examined += 1;
                    names.push(name);
                }
                Err(error) => {
                    self.discovery_error(root, suffix, "Directory enumeration failed", error);
                    return;
                }
            }
        }
        names.sort_by(native_order);
        for name in names {
            if !self.checkpoint() {
                return;
            }
            if matches!(name.as_encoded_bytes(), b"." | b"..") {
                continue;
            }
            let child_eligible = eligible && !skipped(&name);
            if !child_eligible && self.result.output.is_none() {
                continue;
            }
            let child_suffix = append(&suffix, Path::new(&name));
            if path_limits(&append(self.expression(root), &child_suffix)).is_err() {
                self.limit("Discovered path exceeds native spelling bounds");
                return;
            }
            let observed = match directory.observe_child(&name) {
                Ok(observed) => observed,
                Err(error) => {
                    self.discovery_error(
                        root,
                        child_suffix,
                        "Directory entry could not be inspected",
                        error,
                    );
                    if !self.active() {
                        return;
                    }
                    continue;
                }
            };
            match observed.kind {
                EntryKind::Directory => {
                    // A proved ordinary-directory match is already overlap;
                    // refusing a later depth/open cannot erase that observation.
                    if self.block_observed_overlap(observed.identity) {
                        return;
                    }
                    if depth == MAX_WALK_DEPTH {
                        self.limit("Directory nesting exceeds the walk bound");
                        return;
                    }
                    match directory.open_child(&name) {
                        Ok(child) if child.identity() == observed.identity => {
                            self.walk(root, child, child_suffix, child_eligible, depth + 1)
                        }
                        Ok(_) => {
                            let failure = OperationFailure::new(
                                ErrorCode::SourceChanged,
                                ErrorPhase::Collection,
                                "Discovered directory identity changed before descent",
                            );
                            if self.result.output.is_some() {
                                self.block(CollectionState::Unverified, OperationFailure::new(
                                    ErrorCode::BindFailed, ErrorPhase::Admission,
                                    "Output separation could not be qualified after a directory identity changed",
                                ));
                            } else {
                                self.unknown = true;
                                self.push(
                                    root,
                                    child_suffix,
                                    SourceOrigin::Walked,
                                    CollectedObservation::Failed(failure),
                                );
                            }
                        }
                        Err(error) => self.discovery_error(
                            root,
                            child_suffix,
                            "Discovered directory could not be opened",
                            error,
                        ),
                    }
                }
                EntryKind::Symlink | EntryKind::Reparse => {}
                EntryKind::File if child_eligible && eml_name(&name) => self.push(
                    root,
                    child_suffix,
                    SourceOrigin::Walked,
                    CollectedObservation::File {
                        signature: observed.file.expect("ordinary observation has signature"),
                        parent: directory.identity(),
                    },
                ),
                EntryKind::Special if child_eligible && eml_name(&name) => self.push(
                    root,
                    child_suffix,
                    SourceOrigin::Walked,
                    CollectedObservation::Failed(OperationFailure::new(
                        ErrorCode::SourceNotRegular,
                        ErrorPhase::Collection,
                        "Discovered input is not an ordinary file",
                    )),
                ),
                _ => {}
            }
            if !self.active() {
                return;
            }
        }
    }
    fn discovery_error(
        &mut self,
        root: usize,
        suffix: PathBuf,
        detail: &'static str,
        error: io::Error,
    ) {
        self.unknown = true;
        if self.result.output.is_some() {
            self.unverified(detail, error);
        } else {
            self.push(
                root,
                suffix,
                SourceOrigin::Walked,
                CollectedObservation::Failed(OperationFailure::io(
                    ErrorCode::ReadFailed,
                    ErrorPhase::Collection,
                    detail,
                    error,
                )),
            );
        }
    }
}
fn skipped(name: &OsStr) -> bool {
    name.as_encoded_bytes().starts_with(b".") || ascii_suffix(name, COPY_SUFFIX.as_bytes())
}
pub(super) fn path_total(current: usize, display: usize, native: usize) -> Option<usize> {
    current
        .checked_add(display.max(native))
        .filter(|total| *total <= MAX_COLLECTED_PATH_BYTES)
}
fn same_file_binding(left: &CollectedObservation, right: &CollectedObservation) -> bool {
    matches!((left, right), (CollectedObservation::File { signature: a, parent: ap }, CollectedObservation::File { signature: b, parent: bp }) if a == b && ap == bp)
}
fn eml_name(name: &OsStr) -> bool {
    ascii_suffix(name, b".eml")
}
fn ascii_suffix(name: &OsStr, suffix: &[u8]) -> bool {
    let bytes = name.as_encoded_bytes();
    bytes
        .get(bytes.len().saturating_sub(suffix.len())..)
        .is_some_and(|tail| tail.eq_ignore_ascii_case(suffix))
}
fn native_order(a: &OsString, b: &OsString) -> std::cmp::Ordering {
    #[cfg(unix)]
    {
        a.as_encoded_bytes().cmp(b.as_encoded_bytes())
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        a.encode_wide().cmp(b.encode_wide())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request(root: &Path, output: OutputLocation) -> RunRequest {
        RunRequest {
            paths: vec![PathBuf::from("source")],
            working_directory: root.to_owned(),
            output,
            mode: crate::RunMode::Create,
        }
    }
    fn scratch() -> tempfile::TempDir {
        let parent = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/collection-tests");
        std::fs::create_dir_all(&parent).unwrap();
        tempfile::tempdir_in(parent).unwrap()
    }
    #[test]
    fn report_charge_accepts_exact_bound_rejects_one_over_and_uses_larger_fact() {
        assert_eq!(MAX_COLLECTED_PATH_BYTES, 8_388_608);
        assert_eq!(path_total(8_388_598, 10, 9), Some(8_388_608));
        assert_eq!(path_total(8_388_598, 9, 10), Some(8_388_608));
        assert_eq!(path_total(8_388_598, 11, 9), None);
        assert_eq!(path_total(8_388_598, 9, 11), None);
        assert_eq!(path_total(8_388_408, 5, 200), Some(8_388_608));
        assert_eq!(path_total(usize::MAX, 1, 1), None);
    }
    #[test]
    fn forced_hash_collision_does_not_merge_distinct_paths_or_hardlink_spellings() {
        let root = scratch();
        std::fs::write(root.path().join("a.eml"), b"owned a").unwrap();
        std::fs::write(root.path().join("b.eml"), b"owned b").unwrap();
        std::fs::hard_link(root.path().join("a.eml"), root.path().join("hard.eml")).unwrap();
        let mut req = request(root.path(), OutputLocation::BesideSource);
        req.paths = ["a.eml", "./a.eml", "b.eml", "a.eml", "hard.eml"]
            .map(PathBuf::from)
            .to_vec();
        let found = collect_with_hash(&req, &RunControl::default(), |_| 0).unwrap();
        assert_eq!(found.state(), CollectionState::Collected);
        #[cfg(unix)]
        let expected = ["a.eml", "./a.eml", "b.eml", "hard.eml"]
            .map(PathBuf::from)
            .to_vec();
        #[cfg(windows)]
        let expected = ["a.eml", "b.eml", "hard.eml"].map(PathBuf::from).to_vec();
        assert_eq!(found.items().len(), expected.len());
        assert_eq!(
            (0..expected.len())
                .map(|index| found.display_path(index).unwrap().into_owned())
                .collect::<Vec<_>>(),
            expected
        );
        let ids: Vec<_> = found
            .items()
            .iter()
            .map(|item| match item.observation() {
                CollectedObservation::File { signature, .. } => signature.identity,
                _ => panic!("ordinary file"),
            })
            .collect();
        #[cfg(unix)]
        {
            assert_eq!(ids[0], ids[1]);
            assert_eq!(ids[0], ids[3]);
            assert_ne!(ids[0], ids[2]);
        }
        #[cfg(windows)]
        {
            assert_eq!(ids[0], ids[2]);
            assert_ne!(ids[0], ids[1]);
        }
        drop(found);
        root.close().unwrap();
    }
    #[test]
    fn stop_during_real_discovery_retains_only_committed_work_and_no_output() {
        let root = scratch();
        std::fs::create_dir(root.path().join("source")).unwrap();
        std::fs::create_dir(root.path().join("output")).unwrap();
        for name in ["a.eml", "b.eml", "c.eml"] {
            std::fs::write(root.path().join("source").join(name), b"owned").unwrap();
        }
        let req = request(
            root.path(),
            OutputLocation::Directory(PathBuf::from("output")),
        );
        let control = RunControl::default();
        // Private hash injection chooses a deterministic semantic checkpoint;
        // all selection/enumeration/metadata operations remain actual filesystem I/O.
        let found = collect_with_hash(&req, &control, |path| {
            if Path::new(path).file_name() == Some(OsStr::new("b.eml")) {
                control.request_stop();
            }
            0
        })
        .unwrap();
        assert_eq!(found.state(), CollectionState::Interrupted);
        assert_eq!(found.items().len(), 1);
        assert_eq!(
            found.display_path(0).unwrap().as_ref(),
            Path::new("source/a.eml")
        );
        assert!(!found.collection_complete());
        assert!(found.run_error().is_none());
        assert_eq!(
            std::fs::read_dir(root.path().join("output"))
                .unwrap()
                .count(),
            0
        );
        drop(found);
        root.close().unwrap();
    }
    #[test]
    fn actual_uninspectable_entry_blocks_explicit_output_without_fabricating_root_failure() {
        for explicit_output in [false, true] {
            let root = scratch();
            std::fs::create_dir_all(root.path().join("source/z-removed")).unwrap();
            std::fs::create_dir(root.path().join("output")).unwrap();
            std::fs::write(root.path().join("source/a.eml"), b"owned").unwrap();
            let req = request(
                root.path(),
                if explicit_output {
                    OutputLocation::Directory(PathBuf::from("output"))
                } else {
                    OutputLocation::BesideSource
                },
            );
            let removed = std::sync::atomic::AtomicBool::new(false);
            let found = collect_with_hash(&req, &RunControl::default(), |path| {
                if Path::new(path).file_name() == Some(OsStr::new("a.eml"))
                    && !removed.swap(true, std::sync::atomic::Ordering::Relaxed)
                {
                    std::fs::remove_dir(root.path().join("source/z-removed")).unwrap();
                }
                0
            })
            .unwrap();
            assert!(removed.load(std::sync::atomic::Ordering::Relaxed));
            assert!(!found.collection_complete());
            if explicit_output {
                assert_eq!(found.state(), CollectionState::Unverified);
                assert_eq!(found.items().len(), 1);
                let error = found.run_error().unwrap();
                assert_eq!(error.code(), ErrorCode::BindFailed);
                assert_eq!(error.phase(), ErrorPhase::Admission);
                assert_eq!(
                    error.native_error().unwrap().kind(),
                    io::ErrorKind::NotFound
                );
            } else {
                assert_eq!(found.state(), CollectionState::Collected);
                assert_eq!(found.items().len(), 2);
                assert_eq!(found.items()[1].origin(), SourceOrigin::Walked);
                let CollectedObservation::Failed(error) = found.items()[1].observation() else {
                    panic!("discovery failure")
                };
                assert_eq!(error.code(), ErrorCode::ReadFailed);
                assert_eq!(error.phase(), ErrorPhase::Collection);
                assert!(found.run_error().is_none());
            }
            assert_eq!(
                std::fs::read_dir(root.path().join("output"))
                    .unwrap()
                    .count(),
                0
            );
            drop(found);
            root.close().unwrap();
        }
    }
    #[test]
    fn repeated_unadmitted_inputs_do_not_gain_an_operational_dedupe_key() {
        let root = scratch();
        let mut req = request(root.path(), OutputLocation::BesideSource);
        req.paths = vec![PathBuf::new(), PathBuf::new()];
        let found = collect_with_hash(&req, &RunControl::default(), |_| 0).unwrap();
        assert_eq!(found.state(), CollectionState::InputAdmissionFailed);
        assert_eq!(found.items().len(), 2);
        for (index, item) in found.items().iter().enumerate() {
            assert_eq!(found.request_index(index), Some(index));
            assert!(matches!(
                item.observation(),
                CollectedObservation::Failed(_)
            ));
            assert!(item.native_path().as_os_str().is_empty());
        }
        drop(found);
        root.close().unwrap();
    }
    #[test]
    fn late_explicit_root_failure_is_not_hidden_by_an_earlier_walked_file() {
        let root = scratch();
        std::fs::create_dir_all(root.path().join("source/z.eml")).unwrap();
        std::fs::write(root.path().join("source/a.eml"), b"owned").unwrap();
        let mut req = request(root.path(), OutputLocation::BesideSource);
        req.paths.push(PathBuf::from("source/z.eml"));
        let changed = std::sync::atomic::AtomicBool::new(false);
        let found = collect_with_hash(&req, &RunControl::default(), |path| {
            if Path::new(path).file_name() == Some(OsStr::new("a.eml"))
                && !changed.swap(true, std::sync::atomic::Ordering::Relaxed)
            {
                std::fs::remove_dir(root.path().join("source/z.eml")).unwrap();
                std::fs::write(root.path().join("source/z.eml"), b"owned changed type").unwrap();
            }
            0
        })
        .unwrap();
        assert!(changed.load(std::sync::atomic::Ordering::Relaxed));
        assert_eq!(found.state(), CollectionState::InputAdmissionFailed);
        assert_eq!(found.items().len(), 3);
        assert_eq!(found.items()[1].origin(), SourceOrigin::Walked);
        assert!(matches!(
            found.items()[1].observation(),
            CollectedObservation::File { .. }
        ));
        assert_eq!(found.items()[2].origin(), SourceOrigin::Explicit);
        assert_eq!(found.request_index(2), Some(1));
        assert!(matches!(
            found.items()[2].observation(),
            CollectedObservation::Failed(_)
        ));
        assert!(!found.collection_complete());
        drop(found);
        root.close().unwrap();
    }
    #[test]
    fn scripted_late_stop_preserves_observed_failure_and_completed_discovery() {
        let root = scratch();
        let req = request(root.path(), OutputLocation::BesideSource);
        let control = RunControl::default();
        let mut collector = Collector {
            result: CollectedSources::empty(&req),
            control: &control,
            hash: |_: &OsStr| 0,
            seen: HashMap::new(),
            unknown: false,
        };
        collector.result.complete = true;
        // State mapping only: this does not claim an OS permission failure or
        // an actual late signal. The original scripted native cause stays intact.
        collector.unverified(
            "Captured context could not be verified",
            io::Error::from_raw_os_error(5),
        );
        assert!(collector.result.complete);
        control.request_stop();
        assert!(!collector.checkpoint());
        assert_eq!(collector.result.state(), CollectionState::Interrupted);
        assert!(collector.result.collection_complete());
        let failure = collector.result.run_error().unwrap();
        assert_eq!(failure.code(), ErrorCode::BindFailed);
        assert_eq!(failure.native_error().unwrap().raw_os_error(), Some(5));
        drop(collector);
        root.close().unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn held_relative_and_replaced_absolute_cwd_names_are_not_deduplicated() {
        for hardlink in [false, true] {
            let root = scratch();
            let working = root.path().join("working");
            let moved = root.path().join("moved");
            std::fs::create_dir(&working).unwrap();
            std::fs::write(working.join("a.eml"), b"A").unwrap();
            let mut req = request(&working, OutputLocation::BesideSource);
            req.paths = vec![PathBuf::from("a.eml"), working.join("a.eml")];
            let changed = std::sync::atomic::AtomicBool::new(false);
            let found = collect_with_hash(&req, &RunControl::default(), |_| {
                if !changed.swap(true, std::sync::atomic::Ordering::Relaxed) {
                    std::fs::rename(&working, &moved).unwrap();
                    std::fs::create_dir(&working).unwrap();
                    if hardlink {
                        std::fs::hard_link(moved.join("a.eml"), working.join("a.eml")).unwrap();
                    } else {
                        std::fs::write(working.join("a.eml"), b"replacement").unwrap();
                    }
                }
                0
            })
            .unwrap();
            assert_eq!(found.state(), CollectionState::Collected);
            assert_eq!(found.items().len(), 2);
            let facts: Vec<_> = found
                .items()
                .iter()
                .map(|item| match item.observation() {
                    CollectedObservation::File { signature, parent } => (*signature, *parent),
                    _ => panic!("ordinary distinct-context source"),
                })
                .collect();
            assert_eq!(facts[0].0.size, 1);
            if hardlink {
                assert_eq!(facts[0].0, facts[1].0);
            } else {
                assert_eq!(facts[1].0.size, 11);
                assert_ne!(facts[0].0.identity, facts[1].0.identity);
            }
            assert_ne!(facts[0].1, facts[1].1);
            assert_eq!(
                found.items()[0].native_path().as_os_str(),
                OsStr::new("a.eml")
            );
            assert_eq!(
                found.items()[1].native_path().as_os_str(),
                working.join("a.eml").as_os_str()
            );
            assert_eq!(std::fs::read(moved.join("a.eml")).unwrap(), b"A");
            let expected: &[u8] = if hardlink { b"A" } else { b"replacement" };
            assert_eq!(std::fs::read(working.join("a.eml")).unwrap(), expected);
            drop(found);
            root.close().unwrap();
        }
    }
    #[cfg(unix)]
    #[test]
    fn cross_anchor_directory_routes_walk_independently_then_compare_leaf_bindings() {
        let root = scratch();
        std::fs::create_dir(root.path().join("source")).unwrap();
        std::fs::write(root.path().join("source/a.eml"), b"owned").unwrap();
        let mut req = request(root.path(), OutputLocation::BesideSource);
        let first = collect_with_hash(&req, &RunControl::default(), |_| 0).unwrap();
        let examined_once = first.examined_entries();
        drop(first);
        req.paths.push(root.path().join("source"));
        let found = collect_with_hash(&req, &RunControl::default(), |_| 0).unwrap();
        assert_eq!(found.state(), CollectionState::Collected);
        assert_eq!(found.roots.len(), 2);
        assert_eq!(found.examined_entries(), 2 * examined_once);
        assert_eq!(found.items().len(), 1);
        assert_eq!(found.request_index(0), Some(0));
        assert_eq!(found.items()[0].origin(), SourceOrigin::Walked);
        drop(found);
        root.close().unwrap();
    }
}
