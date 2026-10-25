//! Real filesystem guarantees through the public core and held-object boundaries.
use eml_core::fs::{BoundDirectory, BoundFileError, CleanupState, PublicationState};
use eml_core::{Event, EventSink, OutputLocation, RunControl, RunMode, RunRequest, run};
use std::{fs, io, path::Path};
const MESSAGE: &[u8] = b"Content-Type: text/plain\r\n\r\nbody\r\n";
struct Sink;
impl EventSink for Sink {
    fn emit(&mut self, _: Event<'_>) -> io::Result<()> {
        Ok(())
    }
}
fn request(root: &Path) -> RunRequest {
    RunRequest {
        paths: vec!["source.eml".into()],
        working_directory: root.to_owned(),
        output: OutputLocation::BesideSource,
        mode: RunMode::Create,
    }
}

#[test]
fn originals_keep_their_bytes_and_mtime_after_create_refuse_and_stop() {
    for action in ["create", "refuse", "stop"] {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source.eml");
        fs::write(&source, MESSAGE).unwrap();
        let before = fs::metadata(&source).unwrap().modified().unwrap();
        if action == "refuse" {
            fs::write(root.path().join("source.no-attachments.eml"), b"winner").unwrap();
        }
        let control = RunControl::default();
        if action == "stop" {
            control.request_stop();
        }
        let result = run(&request(root.path()), &control, &mut Sink).unwrap();
        assert_eq!(fs::read(&source).unwrap(), MESSAGE);
        assert_eq!(fs::metadata(&source).unwrap().modified().unwrap(), before);
        match action {
            "create" => assert_eq!(result.counts().created(), 1),
            "refuse" => {
                assert_eq!(result.counts().failed(), 1);
                assert_eq!(
                    fs::read(root.path().join("source.no-attachments.eml")).unwrap(),
                    b"winner"
                );
            }
            _ => {
                assert!(result.interrupted());
                assert!(!root.path().join("source.no-attachments.eml").exists());
            }
        }
    }
}

#[test]
fn only_complete_staged_bytes_reach_a_new_final_name_and_existing_files_win() {
    let root = tempfile::tempdir().unwrap();
    let directory = BoundDirectory::open(root.path()).unwrap();
    let ready = directory
        .stage_bytes(b"complete verified copy", || Ok(()))
        .unwrap();
    let names: Vec<_> = fs::read_dir(root.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names.len(), 1);
    let name = names[0].to_string_lossy();
    assert!(name.starts_with(".emlattachmentremover-") && name.ends_with(".tmp"));
    assert!(!root.path().join("copy.eml").exists());
    let attempt = ready.publish("copy.eml".as_ref());
    assert_eq!(attempt.state, PublicationState::Published);
    assert!(attempt.error.is_none());
    assert_eq!(attempt.candidate.cleanup().state, CleanupState::Complete);
    assert_eq!(
        fs::read(root.path().join("copy.eml")).unwrap(),
        b"complete verified copy"
    );
    let loser = directory
        .stage_bytes(b"must not replace", || Ok(()))
        .unwrap()
        .publish("copy.eml".as_ref());
    assert_eq!(loser.state, PublicationState::NotPublished);
    assert!(loser.is_name_collision());
    assert_eq!(loser.candidate.cleanup().state, CleanupState::Complete);
    assert_eq!(
        fs::read(root.path().join("copy.eml")).unwrap(),
        b"complete verified copy"
    );
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
}

#[test]
fn handled_staging_failure_cleans_its_file_and_never_sweeps_prior_residue() {
    let root = tempfile::tempdir().unwrap();
    let directory = BoundDirectory::open(root.path()).unwrap();
    let prior = root.path().join(".emlattachmentremover-prior.tmp");
    fs::write(&prior, b"prior run residue").unwrap();
    let mut calls = 0;
    let failed = directory
        .stage_bytes(&vec![b'x'; 128 * 1024], || {
            calls += 1;
            if calls == 3 {
                Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "stop during staging",
                ))
            } else {
                Ok(())
            }
        })
        .err()
        .expect("Injected checkpoint must reject the stage");
    assert_eq!(failed.cleanup.state, CleanupState::Complete);
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    assert_eq!(fs::read(prior).unwrap(), b"prior run residue");
    assert!(!root.path().join("copy.eml").exists());
}

#[test]
fn a_changed_source_signature_is_refused_after_an_actual_read() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("source.eml"), MESSAGE).unwrap();
    let directory = BoundDirectory::open(root.path()).unwrap();
    let source = directory.open_file("source.eml".as_ref(), false).unwrap();
    let mut prefix = [0; 8];
    assert_eq!(source.read_at(&mut prefix, 0).unwrap(), 8);
    source.check_binding().unwrap();
    // Only the fixture author changes the source; production holds a read-only handle.
    fs::write(
        root.path().join("source.eml"),
        b"replacement with another length",
    )
    .unwrap();
    assert!(matches!(
        source.check_binding(),
        Err(BoundFileError::Changed)
    ));
    assert!(!root.path().join("source.no-attachments.eml").exists());
}

#[cfg(unix)]
#[test]
fn destination_links_are_never_followed_for_files_or_mirrored_directories() {
    let root = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    fs::write(target.path().join("winner.eml"), b"untouched").unwrap();
    std::os::unix::fs::symlink(
        target.path().join("winner.eml"),
        root.path().join("copy.eml"),
    )
    .unwrap();
    std::os::unix::fs::symlink(target.path(), root.path().join("mirror")).unwrap();
    let directory = BoundDirectory::open(root.path()).unwrap();
    assert!(directory.open_child("mirror".as_ref()).is_err());
    assert!(directory.open_child("mirror/".as_ref()).is_err());
    assert!(directory.open_destination(Path::new("mirror/")).is_err());
    fs::create_dir(root.path().join("ordinary")).unwrap();
    assert!(directory.open_destination(Path::new("ordinary/")).is_ok());
    let attempt = directory
        .stage_bytes(b"loser", || Ok(()))
        .unwrap()
        .publish("copy.eml".as_ref());
    assert_eq!(attempt.state, PublicationState::NotPublished);
    assert!(attempt.is_name_collision());
    assert_eq!(attempt.candidate.cleanup().state, CleanupState::Complete);
    assert_eq!(
        fs::read(target.path().join("winner.eml")).unwrap(),
        b"untouched"
    );
    assert_eq!(fs::read_dir(target.path()).unwrap().count(), 1);
}

#[cfg(windows)]
#[test]
fn a_destination_junction_is_refused_without_writing_into_its_target() {
    let root = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let junction = root.path().join("mirror");
    let output = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&junction)
        .arg(target.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "junction creation: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let directory = BoundDirectory::open(root.path()).unwrap();
    assert!(directory.open_child("mirror".as_ref()).is_err());
    assert!(directory.open_child("mirror/".as_ref()).is_err());
    assert!(directory.open_destination(Path::new("mirror/")).is_err());
    fs::create_dir(root.path().join("ordinary")).unwrap();
    assert!(directory.open_destination(Path::new("ordinary/")).is_ok());
    let attempt = directory
        .stage_bytes(b"loser", || Ok(()))
        .unwrap()
        .publish("mirror".as_ref());
    assert_ne!(attempt.state, PublicationState::Published);
    assert!(attempt.error.is_some());
    assert_eq!(attempt.candidate.cleanup().state, CleanupState::Complete);
    assert_eq!(fs::read_dir(target.path()).unwrap().count(), 0);
}

#[cfg(windows)]
#[test]
fn moved_parent_never_reports_staging_cleanup_as_complete_at_a_replacement_address() {
    let root = tempfile::tempdir().unwrap();
    let parent = root.path().join("out");
    let moved = root.path().join("moved");
    fs::create_dir(&parent).unwrap();
    let directory = BoundDirectory::open(&parent).unwrap();
    let ready = directory.stage_bytes(b"owned staging", || Ok(())).unwrap();
    fs::rename(&parent, &moved).unwrap();
    fs::create_dir(&parent).unwrap();
    fs::write(parent.join("unrelated"), b"winner").unwrap();
    let cleanup = ready.cleanup();
    assert_eq!(cleanup.state, CleanupState::Unknown);
    assert_eq!(fs::read(parent.join("unrelated")).unwrap(), b"winner");
    assert_eq!(fs::read_dir(&parent).unwrap().count(), 1);
    let residue: Vec<_> = fs::read_dir(&moved).unwrap().collect();
    assert_eq!(residue.len(), 1);
    assert!(
        residue[0]
            .as_ref()
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".emlattachmentremover-")
    );
}
