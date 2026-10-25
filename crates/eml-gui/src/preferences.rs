//! Destination settings in one JSON file, without importing NSUserDefaults.
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    destination: Option<PathBuf>,
}
/// Loaded destination settings and their explicitly selected backing file.
pub struct Preferences {
    path: PathBuf,
    settings: Settings,
    corrupt: bool,
}
impl Preferences {
    /// Read settings; missing files use defaults and malformed files are preserved.
    pub fn load(path: PathBuf) -> io::Result<Self> {
        let (settings, corrupt) = match fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice::<Settings>(&bytes) {
                Ok(settings)
                    if settings
                        .destination
                        .as_ref()
                        .is_none_or(|path| path.is_absolute()) =>
                {
                    (settings, false)
                }
                Ok(_) => (Settings::default(), true),
                Err(_) => (Settings::default(), true),
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => (Settings::default(), false),
            Err(error) => return Err(error),
        };
        Ok(Self {
            path,
            settings,
            corrupt,
        })
    }
    /// Platform configuration file; new installs start beside the originals.
    pub fn default_path() -> io::Result<PathBuf> {
        dirs::config_dir()
            .map(|p| p.join("EMLAttachmentRemover").join("preferences.json"))
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    "Cannot locate the configuration folder",
                )
            })
    }
    /// Destination applied to later requests.
    pub fn destination(&self) -> Option<&Path> {
        self.settings.destination.as_deref()
    }
    /// A corrupt file has not been silently rewritten.
    pub fn is_corrupt(&self) -> bool {
        self.corrupt
    }
    /// Apply a user-selected destination and atomically save valid settings.
    /// A corrupt original is preserved, and the selection still applies in memory.
    pub fn set_destination(&mut self, destination: Option<PathBuf>) -> io::Result<()> {
        if destination.as_ref().is_some_and(|p| !p.is_absolute()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Choose an absolute destination folder",
            ));
        }
        self.settings.destination = destination;
        if self.corrupt {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Saved destination settings are corrupt and were preserved. Remove the preferences file to enable saving again.",
            ));
        }
        let bytes = serde_json::to_vec_pretty(&self.settings).map_err(io::Error::other)?;
        let parent = self.path.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "Settings file has no parent")
        })?;
        fs::create_dir_all(parent)?;
        let mut staged = tempfile::NamedTempFile::new_in(parent)?;
        staged.write_all(&bytes)?;
        staged.as_file().sync_all()?;
        staged.persist(&self.path).map_err(|error| error.error)?;
        Ok(())
    }
}
