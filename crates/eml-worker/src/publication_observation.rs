//! Nonshipping synchronization; the ordinary Core engine supplies all outcomes.
use std::{
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

pub(crate) struct Observation {
    directory: PathBuf,
    identity: (u64, u64),
    before: bool,
    index: usize,
    started: Instant,
}
impl Observation {
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
        if let Err(error) = self.marker("ready", b"real publication boundary\n") {
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
                    return Err(io::Error::other("Malformed publication release marker"));
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
                Ok(false) => fail("Publication observation deadline exceeded"),
                Err(error) => fail(error),
            }
        }
    }

    pub(crate) fn hooks(self: &Arc<Self>) -> eml_core::PublicationObservation {
        let owned = Arc::clone(self);
        let action: Box<dyn FnOnce()> = Box::new(move || owned.barrier());
        if self.before {
            eml_core::PublicationObservation {
                item_index: self.index,
                before: Some(action),
                after: None,
            }
        } else {
            eml_core::PublicationObservation {
                item_index: self.index,
                before: None,
                after: Some(action),
            }
        }
    }
}

fn fail(error: impl std::fmt::Display) -> ! {
    eprintln!("Nonshipping publication observation failed: {error}");
    std::process::exit(70)
}

pub(crate) fn load() -> io::Result<Option<Arc<Observation>>> {
    let Some(path) = std::env::var_os("EML_PUBLICATION_OBSERVATION") else {
        return Ok(None);
    };
    let directory = PathBuf::from(path);
    let observation = admit(directory)?;
    observation.marker("worker-pid", format!("{}\n", std::process::id()).as_bytes())?;
    Ok(Some(observation))
}

fn admit(directory: PathBuf) -> io::Result<Arc<Observation>> {
    if !directory.is_absolute() || directory.canonicalize()? != directory {
        return Err(io::Error::other(
            "Observation directory must be canonical and absolute",
        ));
    }
    let metadata = fs::symlink_metadata(&directory)?;
    let executable = fs::metadata(std::env::current_exe()?)?;
    if !metadata.is_dir()
        || metadata.permissions().mode() & 0o077 != 0
        || metadata.uid() != executable.uid()
    {
        return Err(io::Error::other(
            "Observation directory is not private to the fixture owner",
        ));
    }
    let recipe = directory.join("observation");
    let facts = fs::symlink_metadata(&recipe)?;
    if !facts.is_file() || facts.len() > 16 {
        return Err(io::Error::other(
            "Observation recipe is not an ordinary bounded file",
        ));
    }
    let mut bytes = Vec::new();
    fs::File::open(recipe)?.take(17).read_to_end(&mut bytes)?;
    let (before, index) = match bytes.as_slice() {
        b"before:0\n" => (true, 0),
        b"before:1\n" => (true, 1),
        b"after:0\n" => (false, 0),
        b"after:1\n" => (false, 1),
        _ => return Err(io::Error::other("Unknown publication observation recipe")),
    };
    Ok(Arc::new(Observation {
        directory,
        identity: (metadata.dev(), metadata.ino()),
        before,
        index,
        started: Instant::now(),
    }))
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
        fs::write(&recipe, b"after:0\n").unwrap();
        let owned = admit(directory.clone()).unwrap();
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(admit(directory.clone()).is_err());
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(&recipe, b"after:2\n").unwrap();
        assert!(admit(directory.clone()).is_err());
        fs::write(&recipe, [b'x'; 17]).unwrap();
        assert!(admit(directory.clone()).is_err());
        let target = root.path().join("owned-recipe");
        fs::write(&target, b"after:0\n").unwrap();
        fs::remove_file(&recipe).unwrap();
        std::os::unix::fs::symlink(&target, &recipe).unwrap();
        assert!(admit(directory.clone()).is_err());
        let moved = root.path().join("moved-control");
        fs::rename(&directory, moved).unwrap();
        fs::create_dir(&directory).unwrap();
        assert!(owned.check_directory().is_err());
    }
}
