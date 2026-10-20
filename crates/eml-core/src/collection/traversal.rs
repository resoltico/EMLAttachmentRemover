//! Directory traversal under the held collection context.
use super::*;

impl<'a, F: Fn(&OsStr) -> u64> Collector<'a, '_, F> {
    pub(super) fn walk(
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
            self.walk_entry(root, &directory, name, &suffix, eligible, depth);
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
    fn walk_entry(
        &mut self,
        root: usize,
        directory: &BoundDirectory,
        name: OsString,
        suffix: &Path,
        eligible: bool,
        depth: usize,
    ) {
        if matches!(name.as_encoded_bytes(), b"." | b"..") {
            return;
        }
        let child_eligible = eligible && !skipped(&name);
        if !child_eligible && self.result.output.is_none() {
            return;
        }
        let child_suffix = append(suffix, Path::new(&name));
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
                return;
            }
        };
        match observed.kind {
            EntryKind::Directory => self.descend(
                root,
                directory,
                child_suffix,
                child_eligible,
                depth,
                observed.identity,
            ),
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
    }
    fn descend(
        &mut self,
        root: usize,
        directory: &BoundDirectory,
        child_suffix: PathBuf,
        child_eligible: bool,
        depth: usize,
        identity: Identity,
    ) {
        // A proved ordinary-directory match is already overlap;
        // refusing a later depth/open cannot erase that observation.
        if self.block_observed_overlap(identity) {
            return;
        }
        if depth == MAX_WALK_DEPTH {
            self.limit("Directory nesting exceeds the walk bound");
            return;
        }
        match directory.open_child(child_suffix.file_name().expect("discovered child name")) {
            Ok(child) if child.identity() == identity => {
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
}
