//! Nonshipping synchronization; the ordinary Core engine supplies all outcomes.
use std::{
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Lane {
    Processing,
    Export,
}
impl Lane {
    fn label(self) -> &'static str {
        match self {
            Self::Processing => "processing",
            Self::Export => "export",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Storage(eml_app::StoragePhase),
    RequestBody,
}
fn phase_label(phase: eml_app::StoragePhase) -> &'static str {
    use eml_app::StoragePhase::*;
    match phase {
        Preflight => "preflight",
        Readback => "readback",
        StageWrite => "stage-write",
        FileSync => "file-sync",
        Publication => "publication",
        NamespaceSync => "namespace-sync",
        AfterPublication => "after-publication",
    }
}
fn parse_phase(value: &str) -> Option<Phase> {
    use eml_app::StoragePhase::*;
    Some(Phase::Storage(match value {
        "preflight" => Preflight,
        "readback" => Readback,
        "stage-write" => StageWrite,
        "file-sync" => FileSync,
        "publication" => Publication,
        "namespace-sync" => NamespaceSync,
        "after-publication" => AfterPublication,
        "request-body" => return Some(Phase::RequestBody),
        _ => return None,
    }))
}
// Exactly the two desktop lanes; this is not a dynamic observer registry.
pub(crate) struct Observation {
    processing: Option<Arc<LaneObservation>>,
    export: Option<Arc<LaneObservation>>,
    dual_root: Option<(PathBuf, (u64, u64))>,
}
impl Observation {
    fn check_directory(&self) -> io::Result<()> {
        if let Some((directory, expected)) = &self.dual_root {
            let metadata = fs::symlink_metadata(directory)?;
            if !metadata.is_dir() || (metadata.dev(), metadata.ino()) != *expected {
                return Err(io::Error::other("Owned dual-lane observation root changed"));
            }
        }
        for lane in [self.processing.as_ref(), self.export.as_ref()]
            .into_iter()
            .flatten()
        {
            lane.check_directory()?;
        }
        Ok(())
    }
    pub(crate) fn is_selected(&self) -> bool {
        [self.processing.as_ref(), self.export.as_ref()]
            .into_iter()
            .flatten()
            .any(|lane| lane.is_selected())
    }
    pub(crate) fn select(
        &self,
        operation: &eml_protocol::desktop::Operation,
        id: eml_protocol::desktop::OperationId,
    ) {
        if let Err(error) = self.check_directory() {
            fail(error);
        }
        if self.is_selected() {
            fail("Operation observation cannot select another capability");
        }
        let lane = match operation {
            eml_protocol::desktop::Operation::Processing => self.processing.as_ref(),
            eml_protocol::desktop::Operation::Export { .. } => self.export.as_ref(),
        };
        if let Some(lane) = lane {
            lane.select(operation, id);
        }
    }
    pub(crate) fn interrupt(&self, second: bool) {
        for lane in [self.processing.as_ref(), self.export.as_ref()]
            .into_iter()
            .flatten()
        {
            lane.interrupt(second);
        }
    }
    pub(crate) fn request_body(&self) {
        if let Some(export) = &self.export {
            export.request_body();
        }
    }
    pub(crate) fn observe(&self, phase: eml_app::StoragePhase) {
        if let Err(error) = self.check_directory() {
            fail(error);
        }
        for lane in [self.processing.as_ref(), self.export.as_ref()]
            .into_iter()
            .flatten()
        {
            lane.observe(phase);
        }
    }
    pub(crate) fn hooks(self: &Arc<Self>) -> eml_core::OperationObservation {
        let owned = Arc::clone(self);
        eml_core::OperationObservation {
            item_index: self
                .processing
                .as_ref()
                .expect("Selected processing lane")
                .index,
            callback: Box::new(move |phase| owned.observe(phase)),
        }
    }
}
struct LaneObservation {
    directory: PathBuf,
    identity: (u64, u64),
    lane: Lane,
    phase: Phase,
    selected: AtomicU64,
    reached: AtomicBool,
    index: usize,
    started: Instant,
}
impl LaneObservation {
    fn check_directory(&self) -> io::Result<()> {
        let metadata = fs::symlink_metadata(&self.directory)?;
        if !metadata.is_dir() || (metadata.dev(), metadata.ino()) != self.identity {
            return Err(io::Error::other("Owned observation directory changed"));
        }
        Ok(())
    }

    fn marker(&self, name: &str, bytes: &[u8]) -> io::Result<()> {
        self.check_directory()?;
        let pending = self.directory.join(format!(".{name}.pending"));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&pending)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        // Same-private-directory hard linking publishes complete marker bytes
        // without replacing an existing marker or needing another dependency.
        fs::hard_link(&pending, self.directory.join(name))?;
        fs::remove_file(pending)
    }

    pub(crate) fn interrupt(&self, second: bool) {
        if !self.is_selected() {
            return;
        }
        let name = if second {
            "second-interrupt"
        } else {
            "first-interrupt"
        };
        let bytes = format!("observed {}\n", self.started.elapsed().as_micros());
        if let Err(error) = self.marker(name, bytes.as_bytes()) {
            fail(error);
        }
    }

    fn barrier(&self) {
        let phase = match self.phase {
            Phase::Storage(value) => phase_label(value),
            Phase::RequestBody => "request-body",
        };
        let index = match self.lane {
            Lane::Processing => self.index.to_string(),
            Lane::Export => "none".into(),
        };
        let ready = format!(
            "operation={} phase={phase} index={index} operation_id={}\n",
            self.lane.label(),
            self.selected.load(Ordering::Acquire)
        );
        if let Err(error) = self.marker("ready", ready.as_bytes()) {
            fail(error);
        }
        // Native XCTest can spend about60s per interaction on hosted Intel.
        // Bound this nonshipping hold across the entire confirmation sequence;
        // the product's Stop/Quit escalation deadlines remain independent.
        let deadline = Instant::now() + Duration::from_secs(600);
        loop {
            let observation = (|| -> io::Result<bool> {
                self.check_directory()?;
                let path = self.directory.join("release");
                let metadata = match fs::symlink_metadata(&path) {
                    Ok(metadata) => metadata,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
                    Err(error) => return Err(error),
                };
                if !metadata.is_file() || metadata.len() != 8 {
                    return Err(io::Error::other("Malformed operation release marker"));
                }
                let mut bytes = Vec::new();
                fs::File::open(path)?.take(9).read_to_end(&mut bytes)?;
                if bytes != b"release\n" {
                    return Err(io::Error::other("Publication release bytes differ"));
                }
                Ok(true)
            })();
            match observation {
                Ok(true) => return,
                Ok(false) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                Ok(false) => fail("Operation observation deadline exceeded"),
                Err(error) => fail(error),
            }
        }
    }

    pub(crate) fn is_selected(&self) -> bool {
        self.selected.load(Ordering::Acquire) != 0
    }
    pub(crate) fn select(
        &self,
        operation: &eml_protocol::desktop::Operation,
        id: eml_protocol::desktop::OperationId,
    ) {
        let lane = match operation {
            eml_protocol::desktop::Operation::Processing => Lane::Processing,
            eml_protocol::desktop::Operation::Export { .. } => Lane::Export,
        };
        if lane != self.lane {
            return;
        }
        if self
            .selected
            .compare_exchange(0, id.get(), Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            fail("Operation observation selected more than once");
        }
        if let Err(error) =
            self.marker("worker-pid", format!("{}\n", std::process::id()).as_bytes())
        {
            fail(error);
        }
    }
    fn reach(&self, phase: Phase) {
        if self.is_selected() && self.phase == phase && !self.reached.swap(true, Ordering::AcqRel) {
            self.barrier();
        }
    }
    pub(crate) fn request_body(&self) {
        self.reach(Phase::RequestBody);
    }
    pub(crate) fn observe(&self, phase: eml_app::StoragePhase) {
        self.reach(Phase::Storage(phase));
    }
}

fn fail(error: impl std::fmt::Display) -> ! {
    eprintln!("Nonshipping operation observation failed: {error}");
    std::process::exit(70)
}

pub(crate) fn load() -> io::Result<Option<Arc<Observation>>> {
    let Some(path) = std::env::var_os("EML_OPERATION_OBSERVATION") else {
        return Ok(None);
    };
    let directory = PathBuf::from(path);
    admit(directory)
}

fn private_directory(directory: &std::path::Path) -> io::Result<fs::Metadata> {
    if !directory.is_absolute() || directory.canonicalize()? != directory {
        return Err(io::Error::other(
            "Observation directory must be canonical and absolute",
        ));
    }
    let metadata = fs::symlink_metadata(directory)?;
    let executable = fs::metadata(std::env::current_exe()?)?;
    if !metadata.is_dir()
        || metadata.permissions().mode() & 0o077 != 0
        || metadata.uid() != executable.uid()
    {
        return Err(io::Error::other(
            "Observation directory is not private to the fixture owner",
        ));
    }
    Ok(metadata)
}
fn recipe_bytes(directory: &std::path::Path) -> io::Result<Option<Vec<u8>>> {
    let recipe = directory.join("observation");
    let facts = match fs::symlink_metadata(&recipe) {
        Ok(value) => value,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if !facts.is_file() || facts.len() > 64 {
        return Err(io::Error::other(
            "Observation recipe is not an ordinary bounded file",
        ));
    }
    let mut bytes = Vec::new();
    fs::File::open(recipe)?.take(65).read_to_end(&mut bytes)?;
    Ok(Some(bytes))
}
fn lane_observation(
    directory: PathBuf,
    metadata: fs::Metadata,
    bytes: Vec<u8>,
) -> io::Result<Arc<LaneObservation>> {
    let recipe = std::str::from_utf8(&bytes)
        .ok()
        .and_then(|value| value.strip_suffix('\n'))
        .ok_or_else(|| io::Error::other("Malformed observation recipe"))?;
    let fields: Vec<_> = recipe.split(':').collect();
    let (lane, phase, index) = match fields.as_slice() {
        ["processing", phase, index @ ("0" | "1")] => (
            Lane::Processing,
            parse_phase(phase),
            usize::from(*index == "1"),
        ),
        ["export", phase] => (Lane::Export, parse_phase(phase), 0),
        _ => return Err(io::Error::other("Unknown operation observation recipe")),
    };
    let phase = phase.ok_or_else(|| io::Error::other("Unknown storage observation phase"))?;
    if phase == Phase::RequestBody && lane != Lane::Export {
        return Err(io::Error::other("Request body observation requires export"));
    }

    Ok(Arc::new(LaneObservation {
        directory,
        identity: (metadata.dev(), metadata.ino()),
        lane,
        phase,
        selected: AtomicU64::new(0),
        reached: AtomicBool::new(false),
        index,
        started: Instant::now(),
    }))
}

fn admit(directory: PathBuf) -> io::Result<Option<Arc<Observation>>> {
    let metadata = private_directory(&directory)?;
    let Some(bytes) = recipe_bytes(&directory)? else {
        return Ok(None);
    };
    if bytes == b"dual-quit\n" {
        let child = |name: &str, lane: Lane| -> io::Result<Arc<LaneObservation>> {
            let path = directory.join(name);
            let metadata = private_directory(&path)?;
            let bytes = recipe_bytes(&path)?
                .ok_or_else(|| io::Error::other("Fixed dual-lane recipe is absent"))?;
            let observer = lane_observation(path, metadata, bytes)?;
            if observer.lane != lane
                || observer.phase != Phase::Storage(eml_app::StoragePhase::Preflight)
                || observer.index != 0
            {
                return Err(io::Error::other(
                    "Dual-quit requires the two exact preflight recipes",
                ));
            }
            Ok(observer)
        };
        let processing = child("processing", Lane::Processing)?;
        let export = child("export", Lane::Export)?;
        return Ok(Some(Arc::new(Observation {
            processing: Some(processing),
            export: Some(export),
            dual_root: Some((directory, (metadata.dev(), metadata.ino()))),
        })));
    }
    let lane = lane_observation(directory, metadata, bytes)?;
    let (processing, export) = match lane.lane {
        Lane::Processing => (Some(lane), None),
        Lane::Export => (None, Some(lane)),
    };
    Ok(Some(Arc::new(Observation {
        processing,
        export,
        dual_root: None,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_admission_rejects_public_recipe_symlink_and_replaced_directory() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("control");
        fs::create_dir(&directory).unwrap();
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
        let directory = directory.canonicalize().unwrap();
        let recipe = directory.join("observation");
        fs::write(&recipe, b"export:publication\n").unwrap();
        let owned = admit(directory.clone()).unwrap().unwrap();
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(admit(directory.clone()).is_err());
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(&recipe, b"processing:publication:2\n").unwrap();
        assert!(admit(directory.clone()).is_err());
        fs::write(&recipe, [b'x'; 65]).unwrap();
        assert!(admit(directory.clone()).is_err());
        let target = root.path().join("owned-recipe");
        fs::write(&target, b"export:publication\n").unwrap();
        fs::remove_file(&recipe).unwrap();
        std::os::unix::fs::symlink(&target, &recipe).unwrap();
        assert!(admit(directory.clone()).is_err());
        let moved = root.path().join("moved-control");
        fs::rename(&directory, moved).unwrap();
        fs::create_dir(&directory).unwrap();
        assert!(owned.check_directory().is_err());
    }
    #[test]
    fn finite_lane_activation_and_single_phase_preserve_exact_operation_markers() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("control");
        fs::create_dir(&directory).unwrap();
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
        let directory = directory.canonicalize().unwrap();
        assert!(admit(directory.clone()).unwrap().is_none());
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 0);
        fs::write(directory.join("observation"), b"export:publication\n").unwrap();
        let owned = admit(directory.clone()).unwrap().unwrap();
        owned.select(
            &eml_protocol::desktop::Operation::Processing,
            eml_protocol::desktop::OperationId::new(41).unwrap(),
        );
        owned.interrupt(false);
        assert!(!owned.is_selected());
        assert!(!directory.join("worker-pid").exists());
        owned.select(
            &eml_protocol::desktop::Operation::Export {
                destination: directory.join("report.json"),
            },
            eml_protocol::desktop::OperationId::new(42).unwrap(),
        );
        assert!(owned.is_selected());
        owned.observe(eml_app::StoragePhase::Readback);
        assert!(!directory.join("ready").exists());
        fs::write(directory.join("release"), b"release\n").unwrap();
        owned.observe(eml_app::StoragePhase::Publication);
        owned.observe(eml_app::StoragePhase::Publication);
        assert_eq!(
            fs::read(directory.join("ready")).unwrap(),
            b"operation=export phase=publication index=none operation_id=42\n"
        );
        assert_eq!(
            fs::read(directory.join("worker-pid")).unwrap(),
            format!("{}\n", std::process::id()).as_bytes()
        );
    }
    #[test]
    fn dual_quit_requires_two_fixed_private_preflight_lanes_and_retains_root_identity() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("control");
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let root = root.canonicalize().unwrap();
        assert!(admit(root.clone()).unwrap().is_none());
        fs::write(root.join("observation"), b"dual-quit\n").unwrap();
        assert!(admit(root.clone()).is_err());
        for (name, recipe) in [
            ("processing", b"processing:preflight:0\n".as_slice()),
            ("export", b"export:preflight\n".as_slice()),
        ] {
            let child = root.join(name);
            fs::create_dir(&child).unwrap();
            fs::set_permissions(&child, fs::Permissions::from_mode(0o700)).unwrap();
            fs::write(child.join("observation"), recipe).unwrap();
        }
        let owned = admit(root.clone()).unwrap().unwrap();
        fs::write(root.join("export/observation"), b"export:readback\n").unwrap();
        assert!(admit(root.clone()).is_err());
        fs::write(root.join("export/observation"), b"export:preflight\n").unwrap();
        owned.select(
            &eml_protocol::desktop::Operation::Export {
                destination: root.join("report.json"),
            },
            eml_protocol::desktop::OperationId::new(99).unwrap(),
        );
        assert!(!root.join("processing/worker-pid").exists());
        assert!(!root.join("worker-pid").exists());
        assert!(root.join("export/worker-pid").is_file());
        let moved = fixture.path().join("moved-control");
        fs::rename(&root, &moved).unwrap();
        fs::create_dir(&root).unwrap();
        assert!(owned.check_directory().is_err());
    }
}
