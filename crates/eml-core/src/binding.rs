//! Current-item destination ownership, mirror policy and ordered namespace work.
use crate::{
    DestinationPlan, ErrorCode, ErrorPhase, NamespaceObservation, OperationFailure, RunControl,
    destination::ObservedRoute,
};
use eml_fs::{
    CleanupReceipt, CleanupState, EntryKind, Identity, SyncMethod, SyncReceipt, SyncState,
};
use std::{
    ffi::{OsStr, OsString},
    io,
    path::Component,
};
#[cfg(unix)]
type MirrorCreation = eml_fs::CreatedDirectory;
#[cfg(windows)]
type MirrorCreation = eml_fs::DirectoryCreation;
type Directory = eml_fs::BoundDirectory;
#[cfg(unix)]
type Root = eml_fs::BoundDirectory;
#[cfg(windows)]
type Root = eml_fs::PinnedRoot;

#[derive(Default)]
pub(super) struct PendingLayout {
    known: std::collections::HashSet<(Identity, Identity)>,
    unresolved: std::collections::HashSet<(Identity, OsString)>,
}
impl PendingLayout {
    fn created(&mut self, parent: Identity, child: Identity) {
        self.known.insert((parent, child));
    }
    pub(super) fn failed_creation(
        &mut self,
        parent: Identity,
        name: &OsStr,
        cleanup: &CleanupReceipt,
    ) {
        if matches!(
            cleanup.state,
            CleanupState::Residual | CleanupState::Unknown
        ) {
            self.unresolved.insert((parent, name.to_owned()));
        }
    }
    fn requires(
        &mut self,
        parent: &Directory,
        child: Identity,
        control: &RunControl,
    ) -> Result<bool, OperationFailure> {
        let mut resolved = Vec::new();
        for (identity, name) in &self.unresolved {
            if *identity != parent.identity() {
                continue;
            }
            checkpoint(control)?;
            let observed = parent
                .observe_child_if_present(name)
                .map_err(|error| native("Unresolved created edge cannot be inspected", error))?;
            if let Some(observed) = observed
                && observed.kind == EntryKind::Directory
            {
                resolved.push(((*identity, name.clone()), observed.identity));
            }
        }
        for (address, identity) in resolved {
            self.unresolved.remove(&address);
            self.known.insert((address.0, identity));
        }
        Ok(self.known.contains(&(parent.identity(), child)))
    }
    fn completed(&mut self, parent: Identity, child: Identity) {
        self.known.remove(&(parent, child));
    }
    fn removed(&mut self, directory: Identity) {
        self.known
            .retain(|(parent, child)| *parent != directory && *child != directory);
        self.unresolved.retain(|(parent, _)| *parent != directory);
    }
}

enum Mirror {
    Existing(Directory),
    Added(MirrorCreation),
}
impl Mirror {
    fn directory(&self) -> &Directory {
        match self {
            Self::Existing(directory) => directory,
            #[cfg(unix)]
            Self::Added(created) => created.directory(),
            #[cfg(windows)]
            Self::Added(created) => created.directory().observed(),
        }
    }
}
/// Ownership is separate from whether a newly observed edge needs synchronization.
pub(super) struct PublicationBinding {
    route: HeldRoute,
    namespace: NamespaceProgress,
}
pub(super) struct BindingFailure {
    pub(super) error: OperationFailure,
    pub(super) namespace: NamespaceObservation,
    pub(super) cleanup: CleanupReceipt,
}
#[derive(Clone, Copy)]
struct NamespaceProgress(NamespaceObservation);
impl NamespaceProgress {
    fn new() -> Self {
        Self(NamespaceObservation {
            receipt: None,
            required_edges: 1,
            completed_edges: 0,
            failed_edge_index: None,
        })
    }
    fn mirror_edge(&mut self) {
        self.0.required_edges += 2;
        assert!(
            self.0.required_edges <= 513,
            "admitted mirror route bounds namespace operations"
        );
    }
    fn record(&mut self, receipt: SyncReceipt) -> Result<(), OperationFailure> {
        if receipt.state != SyncState::Succeeded {
            self.0.failed_edge_index = Some(self.0.completed_edges);
            self.0.receipt = Some(receipt);
            return Err(OperationFailure::new(
                ErrorCode::NamespaceSyncFailed,
                ErrorPhase::Sync,
                "A required directory synchronization failed or is unsupported",
            ));
        }
        self.0.receipt = Some(
            self.0
                .receipt
                .map_or(receipt, |previous| weakest_success(previous, receipt)),
        );
        self.0.completed_edges += 1;
        Ok(())
    }
}
fn weakest_success(previous: SyncReceipt, next: SyncReceipt) -> SyncReceipt {
    use SyncMethod::*;
    match (previous.method, next.method) {
        (a, b) if a == b => previous,
        (FsyncFullfsyncUnsupported, Fsync | FsyncFullfsync) => previous,
        (Fsync | FsyncFullfsync, FsyncFullfsyncUnsupported) => next,
        (Fsync, FsyncFullfsync) => previous,
        (FsyncFullfsync, Fsync) => next,
        _ => unreachable!("one native backend cannot change synchronization families mid-item"),
    }
}
fn checkpoint(control: &RunControl) -> Result<(), OperationFailure> {
    if control.is_stopped() {
        Err(OperationFailure::new(
            ErrorCode::Interrupted,
            ErrorPhase::Stage,
            "Destination acquisition interrupted before publication",
        ))
    } else {
        Ok(())
    }
}
fn native(detail: &'static str, error: io::Error) -> OperationFailure {
    OperationFailure::io(ErrorCode::BindFailed, ErrorPhase::Stage, detail, error)
}
fn changed(detail: &'static str) -> OperationFailure {
    OperationFailure::new(ErrorCode::AddressChanged, ErrorPhase::Address, detail)
}
fn not_needed() -> CleanupReceipt {
    CleanupReceipt {
        state: CleanupState::NotNeeded,
        residual_name: None,
        native_code: None,
    }
}
fn root_directory(root: &Root) -> &Directory {
    #[cfg(unix)]
    {
        root
    }
    #[cfg(windows)]
    {
        root.directory()
    }
}
fn root_from(read: eml_fs::BoundDirectory, control: &RunControl) -> Result<Root, OperationFailure> {
    checkpoint(control)?;
    #[cfg(unix)]
    {
        Ok(read)
    }
    #[cfg(windows)]
    {
        let resolved = read
            .resolved_path()
            .map_err(|error| native("Destination root cannot be resolved", error))?;
        eml_fs::PinnedRoot::bind_resolved(&resolved, read.identity(), || control.is_stopped())
            .map_err(|error| match error {
                eml_fs::RootBindingError::Interrupted => OperationFailure::new(
                    ErrorCode::Interrupted,
                    ErrorPhase::Stage,
                    "Destination ancestry binding interrupted",
                ),
                eml_fs::RootBindingError::Native(error) => {
                    native("Destination ancestry cannot be bound", error)
                }
            })
    }
}
fn open_existing(parent: &Directory, name: &OsStr, expected: Identity) -> io::Result<Directory> {
    #[cfg(unix)]
    {
        let child = parent.open_child(name)?;
        if child.identity() != expected {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Opened mirror differs from observed identity",
            ));
        }
        Ok(child)
    }
    #[cfg(windows)]
    {
        parent.pin_directory(name, expected)
    }
}
fn create(parent: &Directory, name: &OsStr) -> Result<MirrorCreation, eml_fs::CreationFailure> {
    let creation =
        eml_fs::CreationDirectory::acquire(parent).map_err(|error| eml_fs::CreationFailure {
            error,
            cleanup: not_needed(),
        })?;
    creation.create_child(name)
}

impl PublicationBinding {
    pub(super) fn acquire_for_create(
        plan: &DestinationPlan<'_>,
        index: usize,
        inspected: &ObservedRoute,
        control: &RunControl,
        pending: &mut PendingLayout,
    ) -> Result<Self, Box<BindingFailure>> {
        let empty = |error| {
            Box::new(BindingFailure {
                error,
                namespace: NamespaceProgress::new().0,
                cleanup: not_needed(),
            })
        };
        let read = plan.rebind_root(index, control).map_err(empty)?;
        if read.identity() != inspected.prefix[0] {
            return Err(empty(changed("Destination root changed since inspection")));
        }
        let root = root_from(read, control).map_err(empty)?;
        let mut binding = Self {
            route: HeldRoute {
                root,
                mirrors: Vec::new(),
            },
            namespace: NamespaceProgress::new(),
        };
        let mut uncertain_cleanup = not_needed();
        let result = (|| {
            // Resolve the original selected root again after physical pinning.
            plan.rebind_root(index, control)?;
            let route = plan.mirror_route(index);
            let info = plan.records()[index]
                .destination()
                .expect("known destination");
            for (offset, part) in route.components().enumerate() {
                checkpoint(control)?;
                let Component::Normal(name) = part else {
                    unreachable!("frozen ordinary mirror names");
                };
                let actual_limit = binding
                    .directory()
                    .name_limit()
                    .map_err(|error| native("Mirror name limit cannot be revalidated", error))?;
                if actual_limit != info.mirror_limits()[offset] {
                    return Err(native(
                        "Mirror invalidates frozen name budget",
                        io::Error::from(io::ErrorKind::InvalidInput),
                    ));
                }
                let expected = inspected.prefix.get(offset + 1).copied();
                let observed = binding
                    .directory()
                    .observe_child_if_present(name)
                    .map_err(|error| native("Mirror metadata cannot be inspected", error))?;
                let parent_identity = binding.directory().identity();
                let created = binding.acquire_mirror(
                    name,
                    expected,
                    observed,
                    pending,
                    &mut uncertain_cleanup,
                )?;
                // Windows also flushes a possibly-new unowned edge. POSIX keeps
                // its narrower run-created-only namespace contract.
                let parent = if binding.route.mirrors.len() == 1 {
                    root_directory(&binding.route.root)
                } else {
                    binding.route.mirrors[binding.route.mirrors.len() - 2].directory()
                };
                let child_identity = binding.directory().identity();
                let needs_sync = created
                    || pending.requires(parent, child_identity, control)?
                    || (cfg!(windows) && offset + 1 >= info.planned_prefix().len());
                if needs_sync {
                    binding.namespace.mirror_edge();
                    checkpoint(control)?;
                    let receipt = sync_directory(binding.directory())?;
                    binding.namespace.record(receipt)?;
                    checkpoint(control)?;
                    let parent = if binding.route.mirrors.len() == 1 {
                        root_directory(&binding.route.root)
                    } else {
                        binding.route.mirrors[binding.route.mirrors.len() - 2].directory()
                    };
                    let receipt = sync_directory(parent)?;
                    binding.namespace.record(receipt)?;
                    pending.completed(parent_identity, child_identity);
                }
            }
            checkpoint(control)?;
            if binding
                .directory()
                .name_limit()
                .map_err(|error| native("Destination name limit cannot be queried", error))?
                != info.name_limit()
            {
                return Err(native(
                    "Destination invalidates the frozen naming assumption",
                    io::Error::from(io::ErrorKind::InvalidInput),
                ));
            }
            binding.check_held(control, false)?;
            let current = plan.read_parent(index, control)?;
            if !current.complete || current.prefix != binding.identities() {
                return Err(changed("Requested mirror route changed during acquisition"));
            }
            Ok(())
        })();
        match result {
            Ok(()) => Ok(binding),
            Err(error) => {
                let namespace = binding.namespace.0;
                // This constructor has not staged message content. Its failed
                // create receipt describes layout, never a candidate component.
                uncertain_cleanup.residual_name = None;
                let remaining = binding.cleanup_before_publication(not_needed(), pending);
                let cleanup = merge_cleanup(uncertain_cleanup, remaining);
                Err(Box::new(BindingFailure {
                    error,
                    namespace,
                    cleanup,
                }))
            }
        }
    }
    fn acquire_mirror(
        &mut self,
        name: &OsStr,
        expected: Option<Identity>,
        observed: Option<eml_fs::EntryObservation>,
        pending: &mut PendingLayout,
        uncertain_cleanup: &mut CleanupReceipt,
    ) -> Result<bool, OperationFailure> {
        let parent_identity = self.directory().identity();
        let (mirror, created) = match observed {
            Some(observed) => {
                if observed.kind != EntryKind::Directory {
                    return Err(OperationFailure::new(
                        ErrorCode::DestinationExists,
                        ErrorPhase::Existing,
                        "Mirror component is not an ordinary directory",
                    ));
                }
                if expected.is_some_and(|expected| expected != observed.identity) {
                    return Err(changed("A bound mirror changed since inspection"));
                }
                let opened =
                    open_existing(self.directory(), name, observed.identity).map_err(|error| {
                        native("Existing mirror cannot be bound for mutation", error)
                    })?;
                (Mirror::Existing(opened), false)
            }
            None if expected.is_some() => {
                return Err(changed("A bound mirror disappeared since inspection"));
            }
            None => match create(self.directory(), name) {
                Ok(created) => {
                    pending.created(parent_identity, created.directory().identity());
                    (Mirror::Added(created), true)
                }
                Err(error) if error.is_name_collision() => {
                    let entry = self
                        .directory()
                        .observe_child_if_present(name)
                        .map_err(|error| native("Concurrent mirror cannot be inspected", error))?
                        .ok_or_else(|| {
                            native(
                                "Concurrent mirror could not be confirmed",
                                io::Error::from(io::ErrorKind::NotFound),
                            )
                        })?;
                    if entry.kind != EntryKind::Directory {
                        return Err(OperationFailure::new(
                            ErrorCode::DestinationExists,
                            ErrorPhase::Existing,
                            "Concurrent mirror is not an ordinary directory",
                        ));
                    }
                    let opened =
                        open_existing(self.directory(), name, entry.identity).map_err(|error| {
                            native("Concurrent ordinary mirror cannot be bound", error)
                        })?;
                    (Mirror::Existing(opened), false)
                }
                Err(error) => {
                    pending.failed_creation(parent_identity, name, &error.cleanup);
                    *uncertain_cleanup = error.cleanup;
                    return Err(namespace_failure(
                        "Mirror creation could not be admitted",
                        error.error,
                    ));
                }
            },
        };
        self.route.mirrors.push(mirror);
        Ok(created)
    }
    pub(super) fn creation_directory(&self) -> Result<eml_fs::CreationDirectory, OperationFailure> {
        eml_fs::CreationDirectory::acquire(self.directory()).map_err(|error| {
            namespace_failure(
                "Destination immediate mutation parent cannot admit creation",
                error,
            )
        })
    }
    pub(super) fn directory(&self) -> &Directory {
        self.route.directory()
    }
    pub(super) fn identities(&self) -> Vec<Identity> {
        self.route.identities()
    }
    pub(super) fn namespace(&self) -> NamespaceObservation {
        self.namespace.0
    }
    pub(super) fn final_sync(&mut self) -> Result<(), OperationFailure> {
        let receipt = sync_directory(self.directory())?;
        self.namespace.record(receipt)
    }
    pub(super) fn check_held(
        &self,
        control: &RunControl,
        after_edge: bool,
    ) -> Result<(), OperationFailure> {
        self.route.check_held(control, after_edge)
    }
    pub(super) fn cleanup_before_publication(
        mut self,
        candidate: CleanupReceipt,
        pending: &mut PendingLayout,
    ) -> CleanupReceipt {
        let mut aggregate = candidate;
        while let Some(mirror) = self.route.mirrors.pop() {
            if let Mirror::Added(created) = mirror {
                let identity = created.directory().identity();
                #[cfg(unix)]
                let layout = created.remove_empty();
                #[cfg(windows)]
                let layout = created.release_unverified();
                if layout.state == CleanupState::Complete {
                    pending.removed(identity);
                }
                aggregate = merge_cleanup(aggregate, layout);
            }
        }
        aggregate
    }
}
fn namespace_failure(detail: &'static str, error: io::Error) -> OperationFailure {
    let code = if error.kind() == io::ErrorKind::Unsupported {
        ErrorCode::DestinationUnsupported
    } else {
        ErrorCode::BindFailed
    };
    OperationFailure::io(code, ErrorPhase::Stage, detail, error)
}
fn sync_directory(directory: &Directory) -> Result<SyncReceipt, OperationFailure> {
    eml_fs::SyncDirectory::acquire(directory)
        .map(|sync| sync.sync_namespace())
        .map_err(|error| native("Directory synchronization access cannot be acquired", error))
}
/// Complete held ordinary route; this owner has no stage, mirror creation or cleanup surface.
struct HeldRoute {
    root: Root,
    mirrors: Vec<Mirror>,
}
impl HeldRoute {
    fn directory(&self) -> &Directory {
        self.mirrors
            .last()
            .map_or_else(|| root_directory(&self.root), Mirror::directory)
    }
    fn identities(&self) -> Vec<Identity> {
        std::iter::once(root_directory(&self.root).identity())
            .chain(
                self.mirrors
                    .iter()
                    .map(|mirror| mirror.directory().identity()),
            )
            .collect()
    }
    fn check_held(&self, control: &RunControl, after_edge: bool) -> Result<(), OperationFailure> {
        #[cfg(windows)]
        self.root
            .check_held(|| !after_edge && control.is_stopped())
            .map_err(|error| match error {
                eml_fs::RootBindingError::Interrupted => OperationFailure::new(
                    ErrorCode::Interrupted,
                    ErrorPhase::Existing,
                    "Held route verification interrupted",
                ),
                eml_fs::RootBindingError::Native(error) => {
                    native("Held route ancestry changed", error)
                }
            })?;
        #[cfg(unix)]
        self.root
            .check_held()
            .map_err(|error| native("Held route root changed", error))?;
        for mirror in &self.mirrors {
            if !after_edge {
                checkpoint(control)?;
            }
            mirror
                .directory()
                .check_held()
                .map_err(|error| native("Held route mirror changed", error))?;
        }
        Ok(())
    }
}
fn existing_binding_error(error: OperationFailure) -> OperationFailure {
    if error.code() == ErrorCode::Interrupted {
        error.with_context(
            ErrorCode::Interrupted,
            ErrorPhase::Existing,
            "Existing-copy route binding interrupted",
        )
    } else {
        error
    }
}
/// Existing-only synchronization owner. Missing routes fail; construction cannot repair layout.
pub(super) struct ExistingSyncBinding {
    route: HeldRoute,
    endpoint: eml_fs::SyncDirectory,
    debt_edges: Vec<usize>,
    namespace: NamespaceProgress,
}
impl ExistingSyncBinding {
    pub(super) fn acquire_existing(
        plan: &DestinationPlan<'_>,
        index: usize,
        inspected: &ObservedRoute,
        control: &RunControl,
        pending: &mut PendingLayout,
    ) -> Result<Self, OperationFailure> {
        if !inspected.complete {
            return Err(changed("Existing route is incomplete"));
        }
        let read = plan.rebind_root(index, control)?;
        if read.identity() != inspected.prefix[0] {
            return Err(changed("Existing root changed since comparison"));
        }
        let root = root_from(read, control).map_err(existing_binding_error)?;
        let mut route = HeldRoute {
            root,
            mirrors: Vec::new(),
        };
        for (offset, part) in plan.mirror_route(index).components().enumerate() {
            checkpoint(control).map_err(existing_binding_error)?;
            let expected = inspected
                .prefix
                .get(offset + 1)
                .copied()
                .ok_or_else(|| changed("Existing route became incomplete"))?;
            let child = open_existing(route.directory(), part.as_os_str(), expected)
                .map_err(|error| native("Existing mirror cannot be retained", error))?;
            route.mirrors.push(Mirror::Existing(child));
        }
        let current = plan.read_parent(index, control)?;
        if !current.complete || current.prefix != route.identities() {
            return Err(changed("Existing route changed during acquisition"));
        }
        route
            .check_held(control, false)
            .map_err(existing_binding_error)?;
        let mut namespace = NamespaceProgress::new();
        let mut debt_edges = Vec::new();
        for (offset, mirror) in route.mirrors.iter().enumerate() {
            let parent = if offset == 0 {
                root_directory(&route.root)
            } else {
                route.mirrors[offset - 1].directory()
            };
            if pending
                .requires(parent, mirror.directory().identity(), control)
                .map_err(existing_binding_error)?
            {
                namespace.mirror_edge();
                debt_edges.push(offset);
            }
        }
        let endpoint = eml_fs::SyncDirectory::acquire(route.directory()).map_err(|error| {
            native(
                "Existing endpoint synchronization access unavailable",
                error,
            )
        })?;
        Ok(Self {
            route,
            endpoint,
            debt_edges,
            namespace,
        })
    }
    #[cfg(windows)]
    pub(super) fn directory(&self) -> &Directory {
        self.route.directory()
    }
    pub(super) fn identities(&self) -> Vec<Identity> {
        self.route.identities()
    }
    pub(super) fn namespace(&self) -> NamespaceObservation {
        self.namespace.0
    }
    pub(super) fn check_held(
        &self,
        control: &RunControl,
        _after_edge: bool,
    ) -> Result<(), OperationFailure> {
        self.route
            .check_held(control, false)
            .map_err(existing_binding_error)
    }
    pub(super) fn final_sync(
        &mut self,
        pending: &mut PendingLayout,
        control: &RunControl,
    ) -> Result<(), OperationFailure> {
        for &offset in &self.debt_edges {
            checkpoint(control).map_err(existing_binding_error)?;
            let child = self.route.mirrors[offset].directory();
            let parent = if offset == 0 {
                root_directory(&self.route.root)
            } else {
                self.route.mirrors[offset - 1].directory()
            };
            self.namespace
                .record(sync_directory(child).map_err(existing_binding_error)?)?;
            checkpoint(control).map_err(existing_binding_error)?;
            self.namespace
                .record(sync_directory(parent).map_err(existing_binding_error)?)?;
            pending.completed(parent.identity(), child.identity());
        }
        checkpoint(control).map_err(existing_binding_error)?;
        self.namespace.record(self.endpoint.sync_namespace())
    }
}
fn merge_cleanup(mut candidate: CleanupReceipt, layout: CleanupReceipt) -> CleanupReceipt {
    let rank = |state| match state {
        CleanupState::NotNeeded => 0,
        CleanupState::Complete => 1,
        CleanupState::Residual => 2,
        CleanupState::Unknown => 3,
    };
    if rank(layout.state) > rank(candidate.state) {
        candidate.state = layout.state;
    }
    if candidate.native_code.is_none() {
        candidate.native_code = layout.native_code;
    }
    // Only the candidate's name has the fixed residual slot's location meaning.
    candidate
}

#[cfg(test)]
mod tests {
    use super::*;
    fn receipt(method: SyncMethod) -> SyncReceipt {
        SyncReceipt {
            method,
            state: SyncState::Succeeded,
            native_code: None,
        }
    }
    #[test]
    fn ordered_namespace_receipts_preserve_limitations_and_first_failed_edge() {
        let mut progress = NamespaceProgress::new();
        progress.mirror_edge();
        let limited = SyncReceipt {
            method: SyncMethod::FsyncFullfsyncUnsupported,
            state: SyncState::Succeeded,
            native_code: Some(22),
        };
        progress.record(limited).unwrap();
        progress
            .record(receipt(SyncMethod::FsyncFullfsync))
            .unwrap();
        assert_eq!(progress.0.receipt, Some(limited));
        assert_eq!(progress.0.completed_edges, 2);
        let failed = SyncReceipt {
            method: SyncMethod::Fsync,
            state: SyncState::Unsupported,
            native_code: Some(95),
        };
        let error = progress.record(failed).unwrap_err();
        assert_eq!(error.code(), ErrorCode::NamespaceSyncFailed);
        assert_eq!(progress.0.failed_edge_index, Some(2));
        assert_eq!(progress.0.completed_edges, 2);
        assert_eq!(progress.0.receipt, Some(failed));
    }
    #[test]
    fn layout_cleanup_preserves_candidate_location_and_first_failure_without_guessing_names() {
        let candidate = CleanupReceipt {
            state: CleanupState::Residual,
            residual_name: Some(".owned-stage".into()),
            native_code: Some(5),
        };
        let layout = CleanupReceipt {
            state: CleanupState::Residual,
            residual_name: Some("Inbox".into()),
            native_code: Some(13),
        };
        let combined = merge_cleanup(candidate, layout);
        assert_eq!(combined.state, CleanupState::Residual);
        assert_eq!(combined.residual_name, Some(".owned-stage".into()));
        assert_eq!(combined.native_code, Some(5));
        let only_layout = merge_cleanup(
            not_needed(),
            CleanupReceipt {
                state: CleanupState::Residual,
                residual_name: Some("Inbox".into()),
                native_code: Some(13),
            },
        );
        assert_eq!(only_layout.state, CleanupState::Residual);
        assert!(only_layout.residual_name.is_none());
        assert_eq!(only_layout.native_code, Some(13));
        let unknown = merge_cleanup(
            combined,
            CleanupReceipt {
                state: CleanupState::Unknown,
                residual_name: None,
                native_code: Some(2),
            },
        );
        assert_eq!(unknown.state, CleanupState::Unknown);
        assert_eq!(unknown.native_code, Some(5));
        assert_eq!(unknown.residual_name, Some(".owned-stage".into()));
    }
    #[test]
    fn pending_creation_epochs_do_not_overwrite_or_grant_cleanup_ownership() {
        let parent = Identity::Posix {
            device: 1,
            inode: 2,
        };
        let old = Identity::Posix {
            device: 1,
            inode: 3,
        };
        let next = Identity::Posix {
            device: 1,
            inode: 4,
        };
        let mut pending = PendingLayout::default();
        pending.created(parent, old);
        pending.failed_creation(
            parent,
            OsStr::new("Inbox"),
            &CleanupReceipt {
                state: CleanupState::Unknown,
                residual_name: None,
                native_code: None,
            },
        );
        pending.created(parent, next);
        assert!(pending.known.contains(&(parent, old)));
        assert!(pending.known.contains(&(parent, next)));
        assert_eq!(pending.unresolved.len(), 1);
        pending.completed(parent, next);
        assert!(pending.known.contains(&(parent, old)));
        assert_eq!(pending.unresolved.len(), 1);
        pending.removed(next);
        assert!(pending.known.contains(&(parent, old)));
        assert_eq!(pending.unresolved.len(), 1);
        pending.removed(parent);
        assert!(pending.known.is_empty());
        assert!(pending.unresolved.is_empty());
    }
}
