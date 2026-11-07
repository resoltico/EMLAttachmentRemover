//! Recent destination and report folders, replacing invalid settings on the next save.
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    recent_destinations: Vec<PathBuf>,
    last_destination: Option<PathBuf>,
    last_report_folder: Option<PathBuf>,
}
/// Recent folder choices and their atomic backing file.
pub struct Preferences {
    path: PathBuf,
    settings: Settings,
}
impl Preferences {
    /// Load defaults when settings cannot be read or decoded, silently.
    pub fn load(path: PathBuf) -> Self {
        let loaded = match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice::<Settings>(&bytes)
                .map_err(|error| error.to_string())
                .and_then(|settings| {
                    if settings.recent_destinations.len() <= 3
                        && settings
                            .recent_destinations
                            .iter()
                            .all(|path| path.is_absolute())
                        && settings
                            .last_destination
                            .as_ref()
                            .is_none_or(|p| p.is_absolute())
                        && settings
                            .last_report_folder
                            .as_ref()
                            .is_none_or(|p| p.is_absolute())
                    {
                        Ok(settings)
                    } else {
                        Err("Saved folders must use absolute paths; at most three recent destinations are supported.".into())
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
        Self {
            path,
            settings: loaded.unwrap_or_default(),
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
    /// Destination used for the next preview or create.
    pub fn destination(&self) -> Option<&Path> {
        self.settings.last_destination.as_deref()
    }
    /// Apply the destination in memory; save atomically.
    pub fn set_destination(&mut self, destination: Option<PathBuf>) -> io::Result<()> {
        if destination.as_ref().is_some_and(|p| !p.is_absolute()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Choose an absolute destination folder",
            ));
        }
        if let Some(path) = &destination {
            self.settings
                .recent_destinations
                .retain(|recent| recent != path);
            self.settings.recent_destinations.insert(0, path.clone());
            self.settings.recent_destinations.truncate(3);
        }
        self.settings.last_destination = destination;
        self.save()
    }
    /// Recent destination folders in most-recent-first order.
    pub fn recent_destinations(&self) -> &[PathBuf] {
        &self.settings.recent_destinations
    }
    /// Most recently chosen report folder for the native save panel.
    pub fn last_report_folder(&self) -> Option<&Path> {
        self.settings.last_report_folder.as_deref()
    }
    /// Remember a native report folder for the next save panel.
    pub fn set_report_folder(&mut self, folder: PathBuf) -> io::Result<()> {
        if !folder.is_absolute() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Choose an absolute report folder",
            ));
        }
        self.settings.last_report_folder = Some(folder);
        self.save()
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
