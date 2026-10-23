//! Real Win32 creation interleavings through the complete Core workflow.
#![cfg(all(windows, feature = "native-fixtures"))]
use eml_core::{
    AddressState, ErrorCode, Event, EventSink, FailureEffect, ItemResult, OutputLocation,
    RunControl, RunMode, RunRequest, run,
};
use eml_fs::{
    BoundDirectory, CleanupReceipt, CleanupState, Identity, PathContext, PublicationState,
    SyncState,
    creation_observer::{
        CreationBoundary, RestrictedToken, full_identity, make_junction, observe_creations,
        restore_directory,
    },
};
use std::{
    cell::RefCell,
    fs::{self, File},
    io,
    path::{Path, PathBuf},
    rc::Rc,
};
const INPUT: &[u8] = include_bytes!("../../../tests/contract/mime/inputs/01-gmail-attachment.eml");
const GOLDEN: &[u8] = include_bytes!("../../../tests/contract/mime/golden/01-gmail-attachment.eml");
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Profile {
    Normal,
    FileRedirect,
    MirrorRedirect,
    MirrorGuardRedirect,
    OccupiedMirror,
}
impl Profile {
    fn label(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::FileRedirect => "file",
            Self::MirrorRedirect => "mirror",
            Self::MirrorGuardRedirect => "mirror-guard",
            Self::OccupiedMirror => "occupied-mirror",
        }
    }
}
#[derive(Debug)]
struct ItemFacts {
    created: bool,
    code: Option<ErrorCode>,
    publication: PublicationState,
    cleanup: Option<CleanupReceipt>,
}
#[derive(Default)]
struct Sink {
    items: Vec<ItemFacts>,
}
impl EventSink for Sink {
    fn emit(&mut self, event: Event<'_>) -> io::Result<()> {
        if let Event::Item(item) = event {
            let facts = match &item.result {
                ItemResult::Created(copy, _) => {
                    let copy = copy.publication();
                    assert_eq!(copy.address(), AddressState::Verified);
                    assert_eq!(copy.file_sync().unwrap().state, SyncState::Succeeded);
                    assert_eq!(
                        copy.namespace_sync().completed_edges,
                        copy.namespace_sync().required_edges
                    );
                    assert_eq!(
                        copy.namespace_sync().receipt.unwrap().state,
                        SyncState::Succeeded
                    );
                    ItemFacts {
                        created: true,
                        code: None,
                        publication: PublicationState::Published,
                        cleanup: Some(copy.cleanup().clone()),
                    }
                }
                ItemResult::Failed(failure) => {
                    let (publication, cleanup) = match failure.effect() {
                        FailureEffect::NotPublished { cleanup, .. } => {
                            (PublicationState::NotPublished, Some(cleanup.clone()))
                        }
                        FailureEffect::Published(_) => (PublicationState::Published, None),
                        FailureEffect::Unknown(_) => (PublicationState::Unknown, None),
                    };
                    ItemFacts {
                        created: false,
                        code: Some(failure.error().code()),
                        publication,
                        cleanup,
                    }
                }
                ItemResult::UpToDate(_, _) => ItemFacts {
                    created: false,
                    code: None,
                    publication: PublicationState::NotPublished,
                    cleanup: None,
                },
                ItemResult::Planned(_) => panic!("Create must not become a plan"),
            };
            self.items.push(facts);
        }
        Ok(())
    }
}
fn operation(path: &Path) -> PathBuf {
    PathContext::capture(&std::env::current_dir().unwrap())
        .unwrap()
        .resolve(path)
        .unwrap()
}
fn outside_bytes(path: &Path) -> io::Result<u64> {
    let mut bytes = 0;
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let metadata = entry.metadata()?;
        bytes += if metadata.is_dir() {
            outside_bytes(&entry.path())?
        } else {
            metadata.len()
        };
    }
    Ok(bytes)
}
struct Observation {
    destination: PathBuf,
    target: PathBuf,
    parent_id: Identity,
    mirror: Option<(Identity, PathBuf)>,
    mutation: Option<File>,
    redirected: bool,
    restored: bool,
    saw_outside_empty: bool,
    outside_object: Option<Identity>,
    original_had_no_candidate: bool,
    write_samples: usize,
    max_outside: u64,
    occupied_refusals: usize,
    unexpected_occupied_conversion: bool,
}
impl Observation {
    fn observe_creation(
        &mut self,
        profile: Profile,
        event: eml_fs::creation_observer::CreationObservation<'_>,
    ) {
        let parent_id = full_identity(event.parent).unwrap();
        if event.boundary == CreationBoundary::MirrorOpened && self.mirror.is_none() {
            self.mirror = Some((
                full_identity(event.object.unwrap()).unwrap(),
                self.destination.join(event.name),
            ));
        }
        if event.boundary == CreationBoundary::BeforeMirrorCreate
            && profile == Profile::OccupiedMirror
            && self.occupied_refusals == 0
        {
            match make_junction(&self.destination, &self.target) {
                Err(error) => {
                    assert_eq!(
                        error.raw_os_error(),
                        Some(145),
                        "Unrelated refusal is not a nonempty-directory proof: {error}"
                    );
                    self.occupied_refusals += 1;
                }
                Ok(file) => {
                    self.unexpected_occupied_conversion = true;
                    restore_directory(&file).unwrap();
                }
            }
        }
        if event.boundary == CreationBoundary::BeforeFileCreate && !self.redirected {
            let subject = match profile {
                Profile::FileRedirect | Profile::MirrorGuardRedirect
                    if parent_id == self.parent_id =>
                {
                    Some(self.destination.clone())
                }
                Profile::MirrorRedirect => self
                    .mirror
                    .as_ref()
                    .filter(|(id, _)| *id == parent_id)
                    .map(|(_, path)| path.clone()),
                _ => None,
            };
            if let Some(subject) = subject {
                self.mutation=Some(make_junction(&subject,&self.target).expect("The attribute-only controlled conversion must actually occur AFTER the last validation"));
                self.redirected = true;
            }
        }
        if event.boundary == CreationBoundary::FileOpened && self.mutation.is_some() {
            // Independent placement oracle: exact returned object ID versus target
            // lookup; original directory is restored through its retained mutation
            // handle before looking up its contents. No creation/path comparator reused.
            let target_view = BoundDirectory::open(&operation(&self.target)).unwrap();
            let placed = target_view
                .observe_child(event.name)
                .expect("The native call must actually have created at the redirect target");
            assert_eq!(
                placed.identity,
                full_identity(event.object.unwrap()).unwrap()
            );
            assert_eq!(placed.file.unwrap().size, 0);
            self.outside_object = Some(placed.identity);
            self.saw_outside_empty = true;
            self.restore();
            let original = if profile == Profile::MirrorRedirect {
                self.mirror.as_ref().unwrap().1.clone()
            } else {
                self.destination.clone()
            };
            let original_view = BoundDirectory::open(&operation(&original)).unwrap();
            assert_eq!(original_view.identity(), parent_id);
            self.original_had_no_candidate = original_view
                .observe_child_if_present(event.name)
                .unwrap()
                .is_none();
            assert!(self.original_had_no_candidate);
            self.sample();
        }
        if event.boundary == CreationBoundary::AfterWrite {
            self.write_samples += 1;
            self.sample();
            let object = event
                .object
                .expect("A completed write retains its exact native file");
            if self.outside_object == Some(full_identity(object).unwrap()) {
                // Windows directory enumeration can retain the size from before an
                // exclusive writer closes. Query the exact object already independently
                // located at the redirect target, while the write is still observable.
                self.max_outside = self.max_outside.max(object.metadata().unwrap().len());
            }
        }
    }
    fn sample(&mut self) {
        self.max_outside = self.max_outside.max(
            outside_bytes(&self.target)
                .expect("Independent redirect-target metadata must remain observable"),
        );
    }
    fn restore(&mut self) {
        if let Some(file) = self.mutation.take() {
            restore_directory(&file).expect("Restore only the exact owned mutation handle");
            self.restored = true;
        }
    }
}
fn record(profile: Profile, observation: &Observation, source_unchanged: bool) {
    println!(
        "P-W {}: redirected={}, restored={}, outside_bytes={}, write_samples={}",
        profile.label(),
        observation.redirected,
        observation.restored,
        observation.max_outside,
        observation.write_samples
    );
    if let Some(path) = std::env::var_os("EML_WINDOWS_EVIDENCE_DIR") {
        let root = PathBuf::from(path);
        assert!(root.is_absolute());
        fs::create_dir_all(&root).unwrap();
        let path = root.join(format!("P-W-{}.json", profile.label()));
        use std::io::Write;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .unwrap();
        writeln!(file,"{{\"property\":\"P-W\",\"profile\":\"{}\",\"outside_bytes\":{},\"source_unchanged\":{},\"redirected\":{},\"restored\":{},\"write_samples\":{}}}",profile.label(),observation.max_outside,source_unchanged,observation.redirected,observation.restored,observation.write_samples).unwrap();
    }
}
#[test]
fn redirected_destination_receives_no_bytes() {
    for profile in [
        Profile::Normal,
        Profile::FileRedirect,
        Profile::MirrorRedirect,
        Profile::MirrorGuardRedirect,
        Profile::OccupiedMirror,
    ] {
        run_redirect_profile(profile);
    }
}

fn run_redirect_profile(profile: Profile) {
    let parent = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/windows-creation-gate");
    fs::create_dir_all(&parent).unwrap();
    let root = tempfile::tempdir_in(parent).unwrap();
    let scope = RestrictedToken::enter().unwrap();
    let input = root.path().join("input");
    let destination = root.path().join("destination");
    let target = root.path().join("redirect-target");
    for path in [&input, &destination, &target] {
        fs::create_dir(path).unwrap();
    }
    let source = input.join("message.eml");
    fs::write(&source, INPUT).unwrap();
    let view = BoundDirectory::open(&operation(&destination)).unwrap();
    let parent_id = view.identity();
    drop(view);
    let state = Rc::new(RefCell::new(Observation {
        destination: destination.clone(),
        target: target.clone(),
        parent_id,
        mirror: None,
        mutation: None,
        redirected: false,
        restored: false,
        saw_outside_empty: false,
        outside_object: None,
        original_had_no_candidate: false,
        write_samples: 0,
        max_outside: 0,
        occupied_refusals: 0,
        unexpected_occupied_conversion: false,
    }));
    let observed = Rc::clone(&state);
    let observer =
        observe_creations(move |event| observed.borrow_mut().observe_creation(profile, event))
            .unwrap();

    let tree = matches!(
        profile,
        Profile::MirrorRedirect | Profile::MirrorGuardRedirect | Profile::OccupiedMirror
    );
    let request = RunRequest {
        paths: vec![if tree { input.clone() } else { source.clone() }],
        working_directory: root.path().to_owned(),
        output: OutputLocation::Directory(destination.clone()),
        mode: RunMode::Create,
    };
    let mut sink = Sink::default();
    let outcome = run(&request, &RunControl::default(), &mut sink).unwrap();
    drop(observer);
    let mut observed = state.borrow_mut();
    observed.restore();
    observed.sample();
    let original_ok = fs::read(&source).unwrap() == INPUT;
    record(profile, &observed, original_ok);
    assert!(original_ok, "Original message changed");
    assert_eq!(
        observed.max_outside, 0,
        "P-W violated: actual message bytes reached the redirect target before cleanup/publication"
    );
    assert!(
        !observed.unexpected_occupied_conversion,
        "The relied-on nonempty-directory premise was disproved"
    );
    assert_eq!(
        sink.items.len(),
        1,
        "The one known message must have a terminal observation"
    );
    if matches!(profile, Profile::Normal | Profile::OccupiedMirror) {
        assert_eq!(
            outcome.counts().created(),
            1,
            "Normal qualified Created must work: {:?}",
            sink.items
        );
        assert!(sink.items[0].created);
        let copy = if tree {
            destination.join("input.eml-dir/message.zyfuhqa2jklehicoxfjsn2u54y.no-attachments.eml")
        } else {
            destination.join("message.eudtfo5quf3d2hlohgknt7qpaa.no-attachments.eml")
        };
        assert_eq!(fs::read(&copy).unwrap(), GOLDEN);
        if profile == Profile::OccupiedMirror {
            assert_eq!(observed.occupied_refusals, 1);
        }
        drop(observed);
        if profile == Profile::Normal {
            assert_existing_reuse(&request, &copy, &source);
        }
    } else {
        assert_redirect_refusal(&observed, &sink, &outcome, &target);
        drop(observed);
    }
    drop(state);
    drop(scope);
    root.close().unwrap();
}

#[test]
fn empty_tagged_candidate_is_refused_before_message_bytes() {
    let parent = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/windows-creation-gate");
    fs::create_dir_all(&parent).unwrap();
    let root = tempfile::tempdir_in(parent).unwrap();
    let scope = RestrictedToken::enter().unwrap();
    let source = root.path().join("message.eml");
    let destination = root.path().join("destination");
    fs::create_dir(&destination).unwrap();
    fs::write(&source, INPUT).unwrap();
    let tagged = Rc::new(std::cell::Cell::new(false));
    let wrote = Rc::new(std::cell::Cell::new(false));
    let tag_flag = Rc::clone(&tagged);
    let write_flag = Rc::clone(&wrote);
    let output = destination.clone();
    let observer = observe_creations(move |event| {
        if event.boundary == CreationBoundary::BeforeWrite && !tag_flag.get() {
            eml_fs::creation_observer::tag_empty_file(&output.join(event.name))
                .expect("A real native tag must be set before testing rejection");
            tag_flag.set(true);
        }
        if event.boundary == CreationBoundary::AfterWrite {
            write_flag.set(true);
        }
    })
    .unwrap();
    let request = RunRequest {
        paths: vec![source.clone()],
        working_directory: root.path().to_owned(),
        output: OutputLocation::Directory(destination.clone()),
        mode: RunMode::Create,
    };
    let mut sink = Sink::default();
    let result = run(&request, &RunControl::default(), &mut sink).unwrap();
    drop(observer);
    assert!(tagged.get());
    assert!(!wrote.get());
    assert_eq!(result.counts().created(), 0);
    assert_eq!(sink.items.len(), 1);
    assert!(!sink.items[0].created);
    assert_eq!(sink.items[0].publication, PublicationState::NotPublished);
    assert_eq!(fs::read(&source).unwrap(), INPUT);
    assert_eq!(fs::read_dir(&destination).unwrap().count(), 0);
    drop(scope);
    root.close().unwrap();
}

#[test]
fn mirror_replacement_and_cancellation_preserve_unowned_entries() {
    let parent = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/windows-creation-gate");
    fs::create_dir_all(&parent).unwrap();
    let root = tempfile::tempdir_in(parent).unwrap();
    let scope = RestrictedToken::enter().unwrap();
    let input = root.path().join("input");
    let destination = root.path().join("destination");
    let moved = root.path().join("moved-by-fixture");
    for path in [&input, &destination] {
        fs::create_dir(path).unwrap();
    }
    let source = input.join("message.eml");
    fs::write(&source, INPUT).unwrap();
    let control = RunControl::default();
    let cancel = control.clone();
    let replaced = Rc::new(std::cell::Cell::new(false));
    let did_replace = Rc::clone(&replaced);
    let output = destination.clone();
    let moved_path = moved.clone();
    let observer = observe_creations(move |event| {
        if event.boundary == CreationBoundary::MirrorCreated && !did_replace.get() {
            fs::rename(output.join(event.name), &moved_path)
                .expect("Replacement must occur in the real Boolean-create/open gap");
            fs::write(moved_path.join("moved-marker"), b"owned fixture actor data").unwrap();
            fs::create_dir(output.join(event.name)).unwrap();
            fs::write(
                output.join(event.name).join("replacement-marker"),
                b"preserve this replacement",
            )
            .unwrap();
            did_replace.set(true);
        }
        if event.boundary == CreationBoundary::MirrorOpened {
            cancel.request_stop();
        }
    })
    .unwrap();
    let request = RunRequest {
        paths: vec![input],
        working_directory: root.path().to_owned(),
        output: OutputLocation::Directory(destination.clone()),
        mode: RunMode::Create,
    };
    let mut sink = Sink::default();
    let result = run(&request, &control, &mut sink).unwrap();
    drop(observer);
    assert!(replaced.get());
    assert_eq!(result.counts().created(), 0);
    assert_eq!(sink.items.len(), 1);
    assert_eq!(sink.items[0].code, Some(ErrorCode::Interrupted));
    let cleanup = sink.items[0].cleanup.as_ref().unwrap();
    assert_eq!(cleanup.state, CleanupState::Unknown);
    assert!(cleanup.residual_name.is_none());
    assert_eq!(
        fs::read(destination.join("input/replacement-marker")).unwrap(),
        b"preserve this replacement"
    );
    assert_eq!(
        fs::read(moved.join("moved-marker")).unwrap(),
        b"owned fixture actor data"
    );
    assert_eq!(fs::read(&source).unwrap(), INPUT);
    let names: Vec<_> = fs::read_dir(&destination)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names, ["input"]);
    drop(scope);
    root.close().unwrap();
}

fn assert_existing_reuse(request: &RunRequest, copy: &Path, source: &Path) {
    let mut again = Sink::default();
    assert_eq!(
        run(request, &RunControl::default(), &mut again)
            .unwrap()
            .counts()
            .up_to_date(),
        1
    );
    fs::write(copy, b"owned collision winner").unwrap();
    let mut conflict = Sink::default();
    assert_eq!(
        run(request, &RunControl::default(), &mut conflict)
            .unwrap()
            .counts()
            .created(),
        0
    );
    assert_eq!(fs::read(copy).unwrap(), b"owned collision winner");
    assert_eq!(fs::read(source).unwrap(), INPUT);
}

fn assert_redirect_refusal(
    observed: &Observation,
    sink: &Sink,
    outcome: &eml_core::RunOutcome,
    target: &Path,
) {
    assert!(
        observed.redirected
            && observed.restored
            && observed.saw_outside_empty
            && observed.original_had_no_candidate,
        "The precise native interleaving was not exercised"
    );
    assert_eq!(
        observed.write_samples, 0,
        "A contained-refusal case must not write message bytes"
    );
    assert_eq!(outcome.counts().created(), 0);
    assert_eq!(sink.items[0].code, Some(ErrorCode::AddressChanged));
    assert_eq!(sink.items[0].publication, PublicationState::NotPublished);
    assert!(matches!(
        sink.items[0].cleanup.as_ref().unwrap().state,
        CleanupState::Complete | CleanupState::Unknown
    ));
    assert_eq!(
        fs::read_dir(target).unwrap().count(),
        0,
        "Only known-owned empty file cleanup may remove these entries"
    );
}
