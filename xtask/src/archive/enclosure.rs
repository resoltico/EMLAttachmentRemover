//! Protected artifact directories retain their native ancestor authority.
use super::*;
/// Owned private directory with its admitted ancestor chain. Drop closes handles
/// without removing files; retain through bounded observation, then abort/preserve
/// resources if settlement is unknown. Trusted UID/root interference is excluded.
pub struct ArtifactDirectory {
    pub(super) path: PathBuf,
    #[cfg(unix)]
    _held: eml_fs::CreatedDirectory,
    #[cfg(unix)]
    _ancestors: Vec<eml_fs::BoundDirectory>,
    #[cfg(windows)]
    _windows: eml_fs::PrivateDirectory,
}

impl ArtifactDirectory {
    /// Current requested address under the admitted other-UID protection model.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Create an exclusive private artifact directory after admitting every ancestor.
#[cfg(unix)]
pub fn create_artifact_directory(parent: &Path) -> io::Result<ArtifactDirectory> {
    let parent = parent.canonicalize()?;
    let mut ancestors = vec![eml_fs::BoundDirectory::open(Path::new("/"))?];
    ancestors[0].inspect_creation_support()?;
    for component in parent.components() {
        let std::path::Component::Normal(name) = component else {
            continue;
        };
        if ancestors.len() >= 128 {
            return Err(invalid(
                "Private extraction ancestor chain exceeds its bound",
            ));
        }
        let held_parent = ancestors.last().expect("filesystem root retained");
        held_parent.inspect_creation_support()?;
        let child = held_parent.open_child(name)?;
        child.inspect_creation_support()?;
        let observed = held_parent.observe_child(name)?;
        if observed.kind != eml_fs::EntryKind::Directory || observed.identity != child.identity() {
            return Err(invalid(
                "Private extraction ancestor name changed while admitting endpoints",
            ));
        }
        ancestors.push(child);
    }
    let held = ancestors.last().expect("filesystem root retained");
    for _ in 0..64 {
        let mut random = [0_u8; 16];
        getrandom::fill(&mut random).map_err(io::Error::other)?;
        let suffix: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
        let name = format!("artifact-{suffix}");
        match eml_fs::CreationDirectory::acquire(held)?.create_child(std::ffi::OsStr::new(&name)) {
            Ok(created) => {
                created.directory().inspect_creation_support()?;
                let observed = held.observe_child(std::ffi::OsStr::new(&name))?;
                if observed.kind != eml_fs::EntryKind::Directory
                    || observed.identity != created.directory().identity()
                {
                    return Err(invalid(
                        "Private extraction enclosure address is unverified",
                    ));
                }
                return Ok(ArtifactDirectory {
                    path: parent.join(name),
                    _held: created,
                    _ancestors: ancestors,
                });
            }
            Err(error) if error.is_name_collision() => continue,
            Err(error) => return Err(io::Error::other(error)),
        }
    }
    Err(io::Error::other(
        "Private extraction name allocation exhausted",
    ))
}

/// Create only beneath a natively admitted protected Windows ancestor chain.
#[cfg(windows)]
pub fn create_artifact_directory(parent: &Path) -> io::Result<ArtifactDirectory> {
    let directory = eml_fs::create_private_directory(parent)?;
    directory.check()?;
    Ok(ArtifactDirectory {
        path: directory.path().to_owned(),
        _windows: directory,
    })
}

/// Refuse hosts without a protected-root implementation.
#[cfg(not(any(unix, windows)))]
pub fn create_artifact_directory(_parent: &Path) -> io::Result<ArtifactDirectory> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Private Windows archive extraction requires an observed protected parent/root ACL; native qualification is unavailable",
    ))
}
