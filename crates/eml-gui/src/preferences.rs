//! Destination settings with preserved, visibly unavailable configuration states.
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
/// Session destination and its backing file, which is preserved when unavailable.
pub struct Preferences {
    path: PathBuf,
    settings: Settings,
    unavailable: Option<String>,
}
impl Preferences {
    /// Load defaults when settings cannot be read or decoded, preserving the object.
    pub fn load(path: PathBuf) -> Self {
        let loaded = match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice::<Settings>(&bytes)
                .map_err(|error| error.to_string())
                .and_then(|settings| {
                    if settings
                        .destination
                        .as_ref()
                        .is_none_or(|p| p.is_absolute())
                    {
                        Ok(settings)
                    } else {
                        Err("The saved destination is not an absolute path.".into())
                    }
                }),
            Err(error)
                if error.kind() == io::ErrorKind::NotFound
                    && fs::symlink_metadata(&path)
                        .is_err_and(|e| e.kind() == io::ErrorKind::NotFound) =>
            {
                Ok(Settings::default())
            }
            Err(error) => Err(error.to_string()),
        };
        let (settings, unavailable) = match loaded {
            Ok(settings) => (settings, None),
            Err(reason) => (
                Settings::default(),
                Some(format!(
                    "Could not load settings; started with the default destination (Beside originals). {reason}"
                )),
            ),
        };
        Self {
            path,
            settings,
            unavailable,
        }
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
    /// Destination applied to later requests in this session.
    pub fn destination(&self) -> Option<&Path> {
        self.settings.destination.as_deref()
    }
    /// Persistent recovery guidance, including the exact affected path and reason.
    pub fn warning(&self) -> Option<String> {
        self.unavailable.as_ref().map(|reason| format!(
            "Destination settings unavailable: {}\n{}\nDestination changes apply only to this session and will not be saved. The settings object was preserved. Correct its contents, path or read permissions, or move it aside, then restart to enable saving.",
            eml_protocol::TerminalPath::new(&self.path), eml_protocol::DisplayText::new(reason)
        ))
    }
    /// Apply the destination in memory; save only while the backing settings are available.
    pub fn set_destination(&mut self, destination: Option<PathBuf>) -> io::Result<()> {
        if destination.as_ref().is_some_and(|p| !p.is_absolute()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Choose an absolute destination folder",
            ));
        }
        self.settings.destination = destination;
        if self.unavailable.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Destination applies only to this session; settings remain unavailable.",
            ));
        }
        if let Err(error) = self.save() {
            self.unavailable = Some(error.to_string());
            return Err(error);
        }
        Ok(())
    }
    fn save(&self) -> io::Result<()> {
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
