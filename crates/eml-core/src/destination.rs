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
    sources: CollectedSources<'a>,
    control: &RunControl,
) -> DestinationPlan<'a> {
    let path_bytes = sources.source_path_bytes();
    let records = sources
        .items()
        .iter()
        .map(|item| {
            DestinationRecord(match item.observation() {
                CollectedObservation::Failed(error) => Row::Failed(error.clone()),
                _ => Row::Unplanned,
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
    let mut ids: HashSet<Identity> = result
        .sources
        .items()
        .iter()
        .filter_map(|item| match item.observation() {
            CollectedObservation::File { signature, .. } => Some(signature.identity),
            _ => None,
        })
        .collect();
    for root in &result.sources.roots {
        if let RootKind::File(signature, _) = root.kind {
            ids.insert(signature.identity);
        }
    }
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
            Ok((info, display, mut failure)) => {
                if control.is_stopped() {
                    result.state = DestinationPlanState::Interrupted;
                    break;
                }
                let Some(total) = path_total(
                    result.path_bytes,
                    native_bytes(&display),
                    native_bytes(&info.native),
                )
                .filter(|_| path_limits(&display).is_ok()) else {
                    result.state = DestinationPlanState::InputLimitExceeded;
                    result.error = Some(OperationFailure::new(
                        ErrorCode::InputLimitExceeded,
                        ErrorPhase::Admission,
                        "Combined reported destination paths exceed run bounds",
                    ));
                    break;
                };
                if is_source_alias(&result.sources, &info, &ids, control) {
                    failure = Some(OperationFailure::new(
                        ErrorCode::SourceDestinationAlias,
                        ErrorPhase::Admission,
                        "Planned output aliases a selected source reservation",
                    ));
                }
                if control.is_stopped() {
                    result.state = DestinationPlanState::Interrupted;
                    break;
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
                result.records[index] = DestinationRecord(Row::Known {
                    info: Box::new(info),
                    failure,
                });
                result.path_bytes = total;
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
    for contenders in reservations
        .values()
        .filter(|contenders| contenders.len() > 1)
    {
        for &index in contenders {
            if let Row::Known { failure, .. } = &mut result.records[index].0 {
                *failure = Some(OperationFailure::new(
                    ErrorCode::DestinationCollision,
                    ErrorPhase::Admission,
                    "Distinct sources contend for the same planned output",
                ));
            }
        }
    }
    if control.is_stopped() {
        result.state = DestinationPlanState::Interrupted;
    }
    result
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
fn source_parent(
    sources: &CollectedSources<'_>,
    index: usize,
    control: &RunControl,
) -> Result<BoundDirectory, OperationFailure> {
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
            return Err(changed("Selected source directory changed before planning"));
        }
        let mut current = selected;
        if let Some(parent_suffix) = item.relative_path().parent() {
            for part in parent_suffix.components() {
                checkpoint(control)?;
                let Component::Normal(name) = part else {
                    return Err(native_failure(
                        "Invalid discovered source-parent suffix",
                        io::Error::from(io::ErrorKind::InvalidInput),
                    ));
                };
                let observed = current.observe_child(name).map_err(|error| {
                    native_failure("Discovered source parent could not be inspected", error)
                })?;
                if observed.kind != EntryKind::Directory {
                    return Err(changed(
                        "Discovered source parent is no longer an ordinary directory",
                    ));
                }
                let opened = current.open_child(name).map_err(|error| {
                    native_failure("Discovered source parent could not be opened", error)
                })?;
                if opened.identity() != observed.identity {
                    return Err(changed("Discovered parent changed while reopening"));
                }
                current = opened;
            }
        }
        current
    };
    if directory.identity() != *parent {
        return Err(changed("Source parent differs from inventory"));
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
    let context = sources.context.as_ref().expect("collected context");
    let mut route = PathBuf::new();
    let beside;
    let base = match &sources.request.output {
        OutputLocation::BesideSource => {
            beside = source_parent(sources, index, control)?;
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
    let mut current: Option<BoundDirectory> = None;
    let mut prefix = vec![base.identity()];
    let mut missing = false;
    let mut failure = None;
    for name in &components {
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
    let native = match &sources.request.output {
        OutputLocation::BesideSource => {
            let parent = item.native_path().parent().unwrap_or(Path::new(""));
            let native = append(parent, Path::new(&basename));
            #[cfg(windows)]
            let native = context.operand(&native, Path::new("")).map_err(|error| {
                native_failure("Complete destination path is unsupported", error)
            })?;
            native
        }
        OutputLocation::Directory(root) => {
            let suffix = append(&route, Path::new(&basename));
            context.operand(root, &suffix).map_err(|error| {
                native_failure("Complete destination path is unsupported", error)
            })?
        }
    };
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
