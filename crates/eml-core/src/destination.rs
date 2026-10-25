//! Frozen read-only destination layout, naming and reservations. No MIME or writes.
use crate::{
    CollectedObservation, CollectedSources, CollectionState, ErrorCode, ErrorPhase,
    OperationFailure, OutputLocation, RunControl, SourceOrigin,
    collection::{RootKind, path_total},
    copy_basename,
    request::{native_bytes, path_limits},
    selection::{SelectionError, append},
};
use eml_fs::{BoundDirectory, EntryKind, EntryObservation, Identity};
use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    ffi::{OsStr, OsString},
    io,
    path::{Component, Path, PathBuf},
};

/// Structural planning outcome, separate from discovery and terminal item results.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DestinationPlanState {
    /// Structural planning finished, including per-position failures/collisions.
    Complete,
    /// The consumed collection outcome prevents planning or any writable use.
    CollectionBlocked,
    /// Cooperative stop retained every known source position without new work.
    Interrupted,
    /// Combined report paths exceeded the run budget before file processing.
    InputLimitExceeded,
}
/// What an actual no-follow metadata inspection established; no byte/sync claim.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DestinationObservation {
    /// No successful leaf observation occurred, including inspection failures.
    Unobserved,
    /// An ordinary route's missing parent was positively observed.
    MissingParent,
    /// The final component was absent at the held existing parent.
    Missing,
    /// Exact observed entry facts; even a dangling link is present.
    Present(EntryObservation),
}
/// Immutable destination facts from structural planning, not a PlannedCopy,
/// publication receipt, stable absence promise or writable capability.
pub struct DestinationInfo {
    native: PathBuf,
    unbound_parents: usize,
    limit: usize,
    existing_prefix: Vec<Identity>,
    observation: DestinationObservation,
}
impl DestinationInfo {
    /// Reportable native path; never reconstructed from display text.
    pub fn native_path(&self) -> &Path {
        &self.native
    }
    /// Native filename frozen under the observed/planned component limit.
    pub fn basename(&self) -> &OsStr {
        self.native
            .file_name()
            .expect("private construction supplies a basename")
    }
    /// Naming assumption; later binding must refuse if this becomes invalid.
    pub fn name_limit(&self) -> usize {
        self.limit
    }
    /// Actual metadata knowledge, independent of intended path and failure.
    pub fn observation(&self) -> DestinationObservation {
        self.observation
    }
    fn closest_parent(&self) -> Identity {
        *self
            .existing_prefix
            .last()
            .expect("root prefix is always observed")
    }
    fn all_parents_observed(&self) -> bool {
        self.unbound_parents == 0
    }
    fn reservation_tail(&self) -> OsString {
        let mut parts: Vec<_> = self
            .native
            .components()
            .rev()
            .take(self.unbound_parents + 1)
            .collect();
        parts.reverse();
        parts
            .into_iter()
            .fold(PathBuf::new(), |tail, component| {
                append(&tail, Path::new(component.as_os_str()))
            })
            .into_os_string()
    }
}
enum Row {
    Unplanned,
    Failed(OperationFailure),
    Known {
        info: Box<DestinationInfo>,
        failure: Option<OperationFailure>,
    },
}
/// One position aligned with the consumed collection's work list. Public views
/// expose metadata/errors only; indices derive from this row's position.
pub struct DestinationRecord(Row);
impl DestinationRecord {
    /// Known valid destination path, including when another planning check failed.
    pub fn destination(&self) -> Option<&DestinationInfo> {
        match &self.0 {
            Row::Known { info, .. } => Some(info),
            _ => None,
        }
    }
    /// Per-position failure without hiding its known reservation/path facts.
    pub fn failure(&self) -> Option<&OperationFailure> {
        match &self.0 {
            Row::Failed(error) => Some(error),
            Row::Known { failure, .. } => failure.as_ref(),
            Row::Unplanned => None,
        }
    }
    /// Whether this position has not reached destination planning.
    pub fn is_unplanned(&self) -> bool {
        matches!(self.0, Row::Unplanned)
    }
}
/// Owns the single inventory and bounded structural records. It cannot be
/// manufactured by frontends or turned into permission to publish.
pub struct DestinationPlan<'a> {
    sources: CollectedSources<'a>,
    records: Vec<DestinationRecord>,
    state: DestinationPlanState,
    path_bytes: usize,
    error: Option<OperationFailure>,
    labels: HashMap<usize, Option<OsString>>,
}
impl DestinationPlan<'_> {
    /// Original collection state/completeness/errors, preserved through planning.
    pub fn sources(&self) -> &CollectedSources<'_> {
        &self.sources
    }
    /// One structural record per known source position, including failures.
    pub fn records(&self) -> &[DestinationRecord] {
        &self.records
    }
    /// Derived original request display expression; no duplicate full path is retained.
    pub fn display_path(&self, index: usize) -> Option<Cow<'_, Path>> {
        let info = self.records.get(index)?.destination()?;
        let display = display_destination(&self.sources, index, &self.labels, info.basename());
        Some(if display.as_os_str() == info.native.as_os_str() {
            Cow::Borrowed(&info.native)
        } else {
            Cow::Owned(display)
        })
    }
    /// Authoritative structural outcome; Complete still is not MIME verification.
    pub fn state(&self) -> DestinationPlanState {
        self.state
    }
    /// Combined full logical source/known-destination charge retained so far.
    pub fn reported_path_bytes(&self) -> usize {
        self.path_bytes
    }
    /// Planning-level failure, independent of the preserved collection failure.
    pub fn run_error(&self) -> Option<&OperationFailure> {
        self.error.as_ref()
    }
}
#[derive(PartialEq, Eq, Hash)]
struct DestinationKey {
    closest: Identity,
    tail: OsString,
}

/// Freeze layout/names and reserve all known outputs without reading message
/// bodies, creating parents, probing writes or synchronizing. Consume rather than
/// duplicate inventory; later execution must revalidate every native binding,
/// naming assumption and actual no-replace outcome.
pub fn plan_destinations<'a>(
    mut sources: CollectedSources<'a>,
    control: &RunControl,
) -> DestinationPlan<'a> {
    sources.items.sort_by_cached_key(|item| {
        crate::collection::native_order_key(item.native_path().as_os_str())
    });
    let path_bytes = sources.source_path_bytes();
    let records = sources
        .items()
        .iter()
        .map(|item| {
            DestinationRecord(if let Some(error) = &item.version_failure {
                Row::Failed(error.clone())
            } else {
                match item.observation() {
                    CollectedObservation::Failed(error) => Row::Failed(error.clone()),
                    _ => Row::Unplanned,
                }
            })
        })
        .collect();
    let mut result = DestinationPlan {
        sources,
        records,
        state: DestinationPlanState::Complete,
        path_bytes,
        error: None,
        labels: HashMap::new(),
    };
    if control.is_stopped() {
        result.state = DestinationPlanState::Interrupted;
        return result;
    }
    if result.sources.state() != CollectionState::Collected {
        result.state = DestinationPlanState::CollectionBlocked;
        return result;
    }
    let ids = source_identities(&result.sources);
    let mut reservations: HashMap<DestinationKey, Vec<usize>> = HashMap::new();
    for index in 0..result.records.len() {
        if control.is_stopped() {
            result.state = DestinationPlanState::Interrupted;
            break;
        }
        if result.records[index].failure().is_some() {
            continue;
        }
        match plan_one(&result.sources, index, control, &mut result.labels) {
            Ok(planned) => {
                if !result.admit_destination(index, planned, &ids, &mut reservations, control) {
                    break;
                }
            }
            Err(error) => {
                if control.is_stopped() {
                    result.state = DestinationPlanState::Interrupted;
                    break;
                }
                result.records[index] = DestinationRecord(Row::Failed(error));
            }
        }
    }
    // Reserve every known edge before execution, including existing hard links
    // and a final filename used as another contender's mirrored parent.
    let mut conflicts = HashSet::new();
    let mut existing: HashMap<Identity, Vec<usize>> = HashMap::new();
    for (index, record) in result.records.iter().enumerate() {
        if let Some(info) = record.destination()
            && let DestinationObservation::Present(entry) = info.observation
        {
            existing.entry(entry.identity).or_default().push(index);
        }
    }
    for contenders in reservations
        .values()
        .chain(existing.values())
        .filter(|group| group.len() > 1)
    {
        conflicts.extend(contenders.iter().copied());
    }
    for (key, contenders) in &reservations {
        for parent in Path::new(&key.tail)
            .ancestors()
            .skip(1)
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            let ancestor = DestinationKey {
                closest: key.closest,
                tail: parent.as_os_str().to_owned(),
            };
            if let Some(other) = reservations.get(&ancestor) {
                conflicts.extend(contenders.iter().chain(other).copied());
            }
        }
    }
    for index in conflicts {
        if let Row::Known { failure, .. } = &mut result.records[index].0 {
            *failure = Some(OperationFailure::new(
                ErrorCode::DestinationCollision,
                ErrorPhase::Admission,
                "Distinct sources contend for the same destination edge or identity",
            ));
        }
    }
    if control.is_stopped() {
        result.state = DestinationPlanState::Interrupted;
    }
    result
}
impl DestinationPlan<'_> {
    fn admit_destination(
        &mut self,
        index: usize,
        planned: (DestinationInfo, PathBuf, Option<OperationFailure>),
        ids: &HashSet<Identity>,
        reservations: &mut HashMap<DestinationKey, Vec<usize>>,
        control: &RunControl,
    ) -> bool {
        let (info, display, mut failure) = planned;
        if control.is_stopped() {
            self.state = DestinationPlanState::Interrupted;
            return false;
        }
        let Some(total) = path_total(
            self.path_bytes,
            native_bytes(&display),
            native_bytes(&info.native),
        )
        .filter(|_| path_limits(&display).is_ok()) else {
            self.state = DestinationPlanState::InputLimitExceeded;
            self.error = Some(OperationFailure::new(
                ErrorCode::InputLimitExceeded,
                ErrorPhase::Admission,
                "Combined reported destination paths exceed run bounds",
            ));
            return false;
        };
        if is_source_alias(&self.sources, &info, ids, control) {
            failure = Some(OperationFailure::new(
                ErrorCode::SourceDestinationAlias,
                ErrorPhase::Admission,
                "Planned output aliases a selected source reservation",
            ));
        }
        if control.is_stopped() {
            self.state = DestinationPlanState::Interrupted;
            return false;
        }
        let key = DestinationKey {
            closest: info.closest_parent(),
            tail: info.reservation_tail(),
        };
        // Keep valid keys even if inspection/binding rejected the item.
        // All contenders become collisions in a separate complete pass.
        reservations.entry(key).or_default().push(index);
        // Presence is an observation, never inferred from failure=None.
        if let DestinationObservation::Present(observed) = info.observation
            && observed.kind != EntryKind::File
            && failure.is_none()
        {
            failure = Some(OperationFailure::new(
                ErrorCode::DestinationExists,
                ErrorPhase::Admission,
                "Existing output is not an ordinary regular file",
            ));
        }
        self.records[index] = DestinationRecord(Row::Known {
            info: Box::new(info),
            failure,
        });
        self.path_bytes = total;
        true
    }
}
fn native_failure(detail: &'static str, error: io::Error) -> OperationFailure {
    OperationFailure::io(ErrorCode::BindFailed, ErrorPhase::Admission, detail, error)
}
fn selected_failure(error: SelectionError) -> OperationFailure {
    match error {
        SelectionError::Io(error) => {
            native_failure("Selected source parent could not be rebound", error)
        }
        #[cfg(windows)]
        SelectionError::Changed => changed("Selected source root identity changed"),
    }
}
fn changed(detail: &'static str) -> OperationFailure {
    OperationFailure::new(ErrorCode::SourceChanged, ErrorPhase::Admission, detail)
}
fn checkpoint(control: &RunControl) -> Result<(), OperationFailure> {
    if control.is_stopped() {
        Err(OperationFailure::new(
            ErrorCode::BindFailed,
            ErrorPhase::Admission,
            "Structural planning stopped",
        ))
    } else {
        Ok(())
    }
}
pub(super) enum ParentBindingError {
    Interrupted,
    Failure(OperationFailure),
}
impl From<OperationFailure> for ParentBindingError {
    fn from(error: OperationFailure) -> Self {
        Self::Failure(error)
    }
}
impl ParentBindingError {
    pub(super) fn during_execution(self, phase: ErrorPhase) -> OperationFailure {
        match self {
            Self::Interrupted => OperationFailure::new(
                ErrorCode::Interrupted,
                phase,
                "Source parent binding interrupted before publication",
            ),
            Self::Failure(error) => error,
        }
    }
    fn during_planning(self) -> OperationFailure {
        match self {
            Self::Interrupted => OperationFailure::new(
                ErrorCode::BindFailed,
                ErrorPhase::Admission,
                "Structural planning stopped",
            ),
            Self::Failure(error) => error,
        }
    }
}
pub(super) fn source_parent(
    sources: &CollectedSources<'_>,
    index: usize,
    control: &RunControl,
) -> Result<BoundDirectory, ParentBindingError> {
    if control.is_stopped() {
        return Err(ParentBindingError::Interrupted);
    }
    let item = &sources.items()[index];
    let CollectedObservation::File { parent, .. } = item.observation() else {
        unreachable!("caller skips collection failures")
    };
    let context = sources
        .context
        .as_ref()
        .expect("collected sources retain context");
    let directory = if item.origin() == SourceOrigin::Explicit {
        let parent_path = item
            .native_path()
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        context.directory(parent_path).map_err(selected_failure)?
    } else {
        let root = &sources.roots[item.root];
        let selected = context
            .directory(&sources.request.paths[root.request_index])
            .map_err(selected_failure)?;
        let RootKind::Directory(expected) = root.kind else {
            unreachable!("walked file has directory root")
        };
        if selected.identity() != expected {
            return Err(changed("Selected source directory differs from inventory").into());
        }
        let mut current = selected;
        if let Some(parent_suffix) = item.relative_path().parent() {
            for part in parent_suffix.components() {
                if control.is_stopped() {
                    return Err(ParentBindingError::Interrupted);
                }
                let Component::Normal(name) = part else {
                    return Err(native_failure(
                        "Invalid discovered source-parent suffix",
                        io::Error::from(io::ErrorKind::InvalidInput),
                    )
                    .into());
                };
                let observed = current.observe_child(name).map_err(|error| {
                    native_failure("Discovered source parent could not be inspected", error)
                })?;
                if observed.kind != EntryKind::Directory {
                    return Err(changed(
                        "Discovered source parent is no longer an ordinary directory",
                    )
                    .into());
                }
                let opened = current.open_child(name).map_err(|error| {
                    native_failure("Discovered source parent could not be opened", error)
                })?;
                if opened.identity() != observed.identity {
                    return Err(changed("Discovered parent changed while reopening").into());
                }
                current = opened;
            }
        }
        current
    };
    if directory.identity() != *parent {
        return Err(changed("Source parent differs from inventory").into());
    }
    Ok(directory)
}
fn root_label(
    sources: &CollectedSources<'_>,
    root_index: usize,
) -> Result<Option<OsString>, OperationFailure> {
    let context = sources.context.as_ref().expect("collected context");
    let root = &sources.request.paths[sources.roots[root_index].request_index];
    let operand = context
        .operand(root, Path::new(""))
        .map_err(|error| native_failure("Source root label could not be determined", error))?;
    if root.has_root()
        && root
            .components()
            .all(|part| matches!(part, Component::RootDir | Component::Prefix(_)))
    {
        return Ok(None);
    }
    let dot = root
        .components()
        .all(|part| matches!(part, Component::CurDir));
    let component = if dot {
        sources.request.working_directory.file_name()
    } else if root.file_name().is_none() {
        None
    } else {
        operand.file_name()
    };
    Ok(Some(
        component
            .filter(|name| !name.is_empty())
            .unwrap_or(OsStr::new("selected-folder"))
            .to_owned(),
    ))
}
fn mirrored_route(
    sources: &CollectedSources<'_>,
    index: usize,
    labels: &HashMap<usize, Option<OsString>>,
) -> PathBuf {
    let item = &sources.items()[index];
    let mut route = PathBuf::new();
    if item.origin() == SourceOrigin::Walked {
        if let Some(label) = labels
            .get(&item.root)
            .expect("walked root label frozen before destination")
            .as_deref()
        {
            route = append(&route, Path::new(label));
        }
        if let Some(parent) = item.relative_path().parent() {
            route = append(&route, parent);
        }
    }
    route
}
fn display_destination(
    sources: &CollectedSources<'_>,
    index: usize,
    labels: &HashMap<usize, Option<OsString>>,
    basename: &OsStr,
) -> PathBuf {
    match &sources.request.output {
        OutputLocation::BesideSource => {
            let display = sources.display_path(index).expect("known source index");
            append(
                display.parent().unwrap_or(Path::new("")),
                Path::new(basename),
            )
        }
        OutputLocation::Directory(root) => append(
            root,
            &append(&mirrored_route(sources, index, labels), Path::new(basename)),
        ),
    }
}
fn plan_one(
    sources: &CollectedSources<'_>,
    index: usize,
    control: &RunControl,
    labels: &mut HashMap<usize, Option<OsString>>,
) -> Result<(DestinationInfo, PathBuf, Option<OperationFailure>), OperationFailure> {
    checkpoint(control)?;
    let item = &sources.items()[index];
    let mut route = PathBuf::new();
    let beside;
    let base = match &sources.request.output {
        OutputLocation::BesideSource => {
            beside = source_parent(sources, index, control)
                .map_err(ParentBindingError::during_planning)?;
            &beside
        }
        OutputLocation::Directory(_) => {
            if item.origin() == SourceOrigin::Walked
                && let std::collections::hash_map::Entry::Vacant(entry) = labels.entry(item.root)
            {
                entry.insert(root_label(sources, item.root)?);
            }
            route = mirrored_route(sources, index, labels);
            sources
                .output
                .as_ref()
                .expect("explicit output retained after admission")
        }
    };
    let components: Vec<_> = route
        .components()
        .map(|part| match part {
            Component::Normal(name) => Ok(name),
            _ => Err(native_failure(
                "Invalid mirrored component",
                io::Error::from(io::ErrorKind::InvalidInput),
            )),
        })
        .collect::<Result<_, _>>()?;
    let (current, prefix, missing) = inspect_mirrored_route(base, &components, control)?;
    let mut failure = None;
    let directory = current.as_ref().unwrap_or(base);
    let limit = directory.name_limit().map_err(|error| {
        native_failure("Destination component limit could not be queried", error)
    })?;
    let basename = copy_basename(
        item.native_path().file_name().ok_or_else(|| {
            native_failure(
                "Source has no filename component",
                io::Error::from(io::ErrorKind::InvalidInput),
            )
        })?,
        limit,
    )
    .map_err(|error| {
        native_failure(
            "Copy name cannot fit the destination",
            io::Error::new(io::ErrorKind::InvalidInput, error),
        )
    })?;
    #[cfg(windows)]
    eml_fs::validate_component(&basename, limit)
        .map_err(|error| native_failure("Copy name is unsupported after truncation", error))?;
    let display = display_destination(sources, index, labels, &basename);
    let native = native_destination_path(sources, index, &route, &basename)?;
    let observation = if missing {
        DestinationObservation::MissingParent
    } else {
        match directory.observe_child_if_present(&basename) {
            Ok(None) => DestinationObservation::Missing,
            Ok(Some(value)) => DestinationObservation::Present(value),
            Err(error) => {
                failure = Some(native_failure(
                    "Existing output metadata could not be inspected",
                    error,
                ));
                DestinationObservation::Unobserved
            }
        }
    };
    let unbound_parents = components.len() + 1 - prefix.len();
    Ok((
        DestinationInfo {
            native,
            unbound_parents,
            limit,
            existing_prefix: prefix,
            observation,
        },
        display,
        failure,
    ))
}
fn native_destination_path(
    sources: &CollectedSources<'_>,
    index: usize,
    route: &Path,
    basename: &OsStr,
) -> Result<PathBuf, OperationFailure> {
    let item = &sources.items()[index];
    let context = sources.context.as_ref().expect("collected context");
    Ok(match &sources.request.output {
        OutputLocation::BesideSource => {
            let parent = item.native_path().parent().unwrap_or(Path::new(""));
            let native = append(parent, Path::new(basename));
            #[cfg(windows)]
            let native = context.operand(&native, Path::new("")).map_err(|error| {
                native_failure("Complete destination path is unsupported", error)
            })?;
            native
        }
        OutputLocation::Directory(root) => {
            let suffix = append(route, Path::new(basename));
            context.operand(root, &suffix).map_err(|error| {
                native_failure("Complete destination path is unsupported", error)
            })?
        }
    })
}
fn inspect_mirrored_route(
    base: &BoundDirectory,
    components: &[&OsStr],
    control: &RunControl,
) -> Result<(Option<BoundDirectory>, Vec<Identity>, bool), OperationFailure> {
    let mut current: Option<BoundDirectory> = None;
    let mut prefix = vec![base.identity()];
    let mut missing = false;
    let mut failure = None;
    for name in components {
        checkpoint(control)?;
        let directory = current.as_ref().unwrap_or(base);
        match directory.observe_child_if_present(name) {
            Ok(None) => {
                missing = true;
                break;
            }
            Ok(Some(observed)) if observed.kind == EntryKind::Directory => {
                match directory.open_child(name) {
                    Ok(child) if child.identity() == observed.identity => {
                        prefix.push(child.identity());
                        current = Some(child);
                    }
                    Ok(_) => {
                        failure = Some(changed("Mirrored parent changed while opening"));
                        break;
                    }
                    Err(error) => {
                        failure =
                            Some(native_failure("Mirrored parent could not be opened", error));
                        break;
                    }
                }
            }
            Ok(Some(_)) => {
                failure = Some(OperationFailure::new(
                    ErrorCode::DestinationExists,
                    ErrorPhase::Admission,
                    "Mirrored parent is not an ordinary directory",
                ));
                break;
            }
            Err(error) => {
                failure = Some(native_failure(
                    "Mirrored parent could not be inspected",
                    error,
                ));
                break;
            }
        }
    }
    // Only a positively missing tail may use the closest existing parent's
    // limit as an assumption. An existing/unverified blocked component does not
    // establish the actual naming parent or a valid frozen basename/key.
    if let Some(error) = failure.take() {
        return Err(error);
    }
    Ok((current, prefix, missing))
}
fn is_source_alias(
    sources: &CollectedSources<'_>,
    info: &DestinationInfo,
    ids: &HashSet<Identity>,
    control: &RunControl,
) -> bool {
    if matches!(info.observation, DestinationObservation::Present(value) if ids.contains(&value.identity))
    {
        return true;
    }
    let context = sources.context.as_ref().expect("collected context");
    for item in sources.items() {
        if control.is_stopped() {
            return false;
        }
        if item.native_path().is_absolute() == info.native.is_absolute()
            && context.same_spelling(item.native_path(), &info.native)
        {
            return true;
        }
        if info.all_parents_observed()
            && let CollectedObservation::File { parent, .. } = item.observation()
            && *parent == info.closest_parent()
            && item.native_path().file_name() == Some(info.basename())
        {
            return true;
        }
    }
    false
}

pub(super) fn source_identities(sources: &CollectedSources<'_>) -> HashSet<Identity> {
    let mut ids: HashSet<_> = sources
        .items()
        .iter()
        .filter_map(|item| match item.observation() {
            CollectedObservation::File { signature, .. } => Some(signature.identity),
            _ => None,
        })
        .collect();
    ids.extend(sources.observed_identities.iter().copied());
    for root in &sources.roots {
        if let RootKind::File(signature, _) = root.kind {
            ids.insert(signature.identity);
        }
    }
    ids
}

/// Current-item read-only facts, including mirrors first observed after planning.
pub(super) struct ReadOnlyParent {
    pub(super) directory: BoundDirectory,
    pub(super) prefix: Vec<Identity>,
    pub(super) complete: bool,
}
impl DestinationInfo {
    pub(super) fn planned_prefix(&self) -> &[Identity] {
        &self.existing_prefix
    }
}
impl DestinationPlan<'_> {
    pub(super) fn mirror_route(&self, index: usize) -> PathBuf {
        match self.sources.request.output {
            OutputLocation::BesideSource => PathBuf::new(),
            OutputLocation::Directory(_) => mirrored_route(&self.sources, index, &self.labels),
        }
    }
    pub(super) fn rebind_root(
        &self,
        index: usize,
        control: &RunControl,
    ) -> Result<BoundDirectory, OperationFailure> {
        if control.is_stopped() {
            return Err(execution_interrupted());
        }
        let directory = match &self.sources.request.output {
            OutputLocation::BesideSource => source_parent(&self.sources, index, control)
                .map_err(|error| error.during_execution(ErrorPhase::Existing))?,
            OutputLocation::Directory(root) => self
                .sources
                .context
                .as_ref()
                .expect("complete context")
                .directory(root)
                .map_err(destination_binding_failure)?,
        };
        if directory.identity()
            != self.records[index]
                .destination()
                .expect("known destination")
                .existing_prefix[0]
        {
            return Err(address_changed(
                "Destination root differs from its planned binding",
            ));
        }
        Ok(directory)
    }
    /// Retain every observed identity for later acquisition; create nothing.
    pub(super) fn read_parent(
        &self,
        index: usize,
        control: &RunControl,
    ) -> Result<ReadOnlyParent, OperationFailure> {
        let info = self.records[index]
            .destination()
            .expect("known destination");
        let mut current = self.rebind_root(index, control)?;
        let mut prefix = vec![current.identity()];
        for (offset, part) in self.mirror_route(index).components().enumerate() {
            if control.is_stopped() {
                return Err(execution_interrupted());
            }
            let Component::Normal(name) = part else {
                unreachable!("frozen mirror route");
            };
            let observed = current.observe_child_if_present(name).map_err(|error| {
                execution_native_failure("Destination parent cannot be inspected", error)
            })?;
            let Some(observed) = observed else {
                if offset + 1 < info.existing_prefix.len() {
                    return Err(address_changed(
                        "Previously observed destination parent is missing",
                    ));
                }
                check_parent_assumption(&current, info.limit)?;
                return Ok(ReadOnlyParent {
                    directory: current,
                    prefix,
                    complete: false,
                });
            };
            if observed.kind != EntryKind::Directory {
                return Err(OperationFailure::new(
                    ErrorCode::DestinationExists,
                    ErrorPhase::Existing,
                    "Destination parent is not an ordinary directory",
                ));
            }
            let opened = current.open_child(name).map_err(|error| {
                execution_native_failure("Destination parent cannot be opened", error)
            })?;
            if opened.identity() != observed.identity
                || info
                    .existing_prefix
                    .get(offset + 1)
                    .is_some_and(|expected| *expected != opened.identity())
            {
                return Err(address_changed(
                    "Destination parent differs from its planned binding",
                ));
            }
            prefix.push(opened.identity());
            current = opened;
        }
        check_parent_assumption(&current, info.limit)?;
        Ok(ReadOnlyParent {
            directory: current,
            prefix,
            complete: true,
        })
    }
}

fn address_changed(detail: &'static str) -> OperationFailure {
    OperationFailure::new(ErrorCode::AddressChanged, ErrorPhase::Address, detail)
}
fn execution_interrupted() -> OperationFailure {
    OperationFailure::new(
        ErrorCode::Interrupted,
        ErrorPhase::Existing,
        "Destination binding interrupted before publication",
    )
}
fn check_parent_assumption(current: &BoundDirectory, limit: usize) -> Result<(), OperationFailure> {
    if current.name_limit().map_err(|error| {
        execution_native_failure("Destination name limit cannot be revalidated", error)
    })? != limit
    {
        return Err(OperationFailure::new(
            ErrorCode::BindFailed,
            ErrorPhase::Read,
            "Destination name limit invalidates the frozen naming assumption",
        ));
    }
    Ok(())
}
fn execution_native_failure(detail: &'static str, error: io::Error) -> OperationFailure {
    OperationFailure::io(ErrorCode::BindFailed, ErrorPhase::Read, detail, error)
}
fn destination_binding_failure(error: SelectionError) -> OperationFailure {
    match error {
        SelectionError::Io(error) => {
            execution_native_failure("Destination root cannot be bound", error)
        }
        #[cfg(windows)]
        SelectionError::Changed => OperationFailure::new(
            ErrorCode::AddressUnverified,
            ErrorPhase::Address,
            "Destination root cannot be consistently resolved",
        ),
    }
}

#[cfg(test)]
mod execution_tests {
    use super::*;
    #[test]
    fn parent_stop_and_failed_observation_have_distinct_execution_results() {
        for phase in [ErrorPhase::Read, ErrorPhase::Existing] {
            let stopped = ParentBindingError::Interrupted.during_execution(phase);
            assert_eq!(
                (stopped.code(), stopped.phase()),
                (ErrorCode::Interrupted, phase)
            );
            let native = ParentBindingError::Failure(execution_native_failure(
                "Required parent observation failed",
                io::Error::from_raw_os_error(5),
            ))
            .during_execution(phase);
            assert_eq!(
                (native.code(), native.phase()),
                (ErrorCode::BindFailed, ErrorPhase::Read)
            );
            assert_eq!(native.native_error().unwrap().raw_os_error(), Some(5));
        }
    }
    #[test]
    fn invalidated_name_limit_is_a_binding_refusal_without_address_change() {
        let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/destination-tests");
        std::fs::create_dir_all(&base).unwrap();
        let root = tempfile::tempdir_in(base).unwrap();
        let context = crate::selection::WorkingContext::capture(root.path()).unwrap();
        let directory = context
            .directory(Path::new("."))
            .unwrap_or_else(|_| panic!("Owned ordinary directory must bind"));
        let observed = directory.name_limit().unwrap();
        let error = check_parent_assumption(&directory, observed + 1).unwrap_err();
        assert_eq!(
            (error.code(), error.phase()),
            (ErrorCode::BindFailed, ErrorPhase::Read)
        );
        drop(directory);
        drop(context);
        root.close().unwrap();
    }
}
