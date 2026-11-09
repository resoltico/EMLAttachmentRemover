//! Recent destination and report folders, replacing invalid settings on the next save.
use eml_protocol::{NativePathValue, records::NativePathRecord};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    #[serde(
        serialize_with = "serialize_recent",
        deserialize_with = "deserialize_recent"
    )]
    recent_destinations: Vec<PathBuf>,
    #[serde(
        default,
        serialize_with = "serialize_folder",
        deserialize_with = "deserialize_folder"
    )]
    last_destination: Option<PathBuf>,
    #[serde(
        default,
        serialize_with = "serialize_folder",
        deserialize_with = "deserialize_folder"
    )]
    last_report_folder: Option<PathBuf>,
}
fn folder_valid(path: &Path) -> bool {
    path.is_absolute() && eml_core::validate_native_path(path).is_ok()
}
fn decode_folder(record: NativePathRecord) -> Option<PathBuf> {
    record.native_path().filter(|path| folder_valid(path))
}
fn serialize_recent<S: Serializer>(paths: &[PathBuf], serializer: S) -> Result<S::Ok, S::Error> {
    paths
        .iter()
        .map(|path| NativePathValue::new(path))
        .collect::<Vec<_>>()
        .serialize(serializer)
}
fn deserialize_recent<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<PathBuf>, D::Error> {
    let records = Vec::<NativePathRecord>::deserialize(deserializer)?;
    if records.len() > 3 {
        return Err(serde::de::Error::custom(
            crate::wording::INVALID_SAVED_FOLDERS,
        ));
    }
    records
        .into_iter()
        .map(|record| {
            decode_folder(record)
                .ok_or_else(|| serde::de::Error::custom(crate::wording::INVALID_SAVED_FOLDERS))
        })
        .collect()
}
fn serialize_folder<S: Serializer>(
    path: &Option<PathBuf>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    path.as_deref()
        .map(NativePathValue::new)
        .serialize(serializer)
}
fn deserialize_folder<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<PathBuf>, D::Error> {
    Option::<NativePathRecord>::deserialize(deserializer)?
        .map(|record| {
            decode_folder(record)
                .ok_or_else(|| serde::de::Error::custom(crate::wording::INVALID_SAVED_FOLDERS))
        })
        .transpose()
}
/// Recent folder choices and their atomic backing file.
pub struct Preferences {
    path: PathBuf,
    settings: Settings,
}
impl Preferences {
    /// Load defaults when settings cannot be read or decoded, silently.
    pub fn load(path: PathBuf) -> Self {
        let settings = fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Settings>(&bytes).ok())
            .unwrap_or_default();
        Self { path, settings }
    }
    /// Platform configuration file; new installs start beside the originals.
    pub fn default_path() -> io::Result<PathBuf> {
        dirs::config_dir()
            .map(|p| p.join("EMLAttachmentRemover").join("preferences.json"))
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::NotFound, crate::wording::CONFIG_UNAVAILABLE)
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
                crate::wording::ABSOLUTE_DESTINATION,
            ));
        }
        if destination.as_ref().is_some_and(|path| !folder_valid(path)) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                crate::wording::INVALID_SAVED_FOLDERS,
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
                crate::wording::ABSOLUTE_REPORT_FOLDER,
            ));
        }
        if !folder_valid(&folder) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                crate::wording::INVALID_SAVED_FOLDERS,
            ));
        }
        self.settings.last_report_folder = Some(folder);
        self.save()
    }
    fn save(&self) -> io::Result<()> {
        let bytes = serde_json::to_vec_pretty(&self.settings).map_err(io::Error::other)?;
        let parent = self.path.parent().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                crate::wording::SETTINGS_PARENT_MISSING,
            )
        })?;
        fs::create_dir_all(parent)?;
        let mut staged = tempfile::NamedTempFile::new_in(parent)?;
        staged.write_all(&bytes)?;
        staged.as_file().sync_all()?;
        staged.persist(&self.path).map_err(|error| error.error)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_native_folder_round_trip(first: PathBuf, second: PathBuf, report: PathBuf) {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("settings.json");
        let mut preferences = Preferences::load(file.clone());
        preferences.set_destination(Some(first.clone())).unwrap();
        preferences.set_destination(Some(second.clone())).unwrap();
        preferences.set_destination(Some(first.clone())).unwrap();
        preferences.set_report_folder(report.clone()).unwrap();
        let loaded = Preferences::load(file);
        assert_eq!(loaded.destination(), Some(first.as_path()));
        assert_eq!(loaded.recent_destinations(), [first, second]);
        assert_eq!(loaded.last_report_folder(), Some(report.as_path()));
    }

    #[cfg(unix)]
    #[test]
    fn x3_native_destination_recents_and_report_folder_survive_reload() {
        use std::{ffi::OsString, os::unix::ffi::OsStringExt};
        let root = tempfile::tempdir().unwrap();
        assert!(
            !include_str!("preferences.rs")
                .lines()
                .any(|line| line.starts_with("use eml_protocol")),
            "Settings must own their path encoding"
        );
        assert_native_folder_round_trip(
            root.path()
                .join(OsString::from_vec(b"destination-\xff".to_vec())),
            root.path()
                .join(OsString::from_vec(b"recent-\xfe".to_vec())),
            root.path()
                .join(OsString::from_vec(b"report-\xfd".to_vec())),
        );
    }

    #[cfg(windows)]
    #[test]
    fn x3_native_destination_recents_and_report_folder_survive_reload() {
        use std::{ffi::OsString, os::windows::ffi::OsStringExt};
        let root = tempfile::tempdir().unwrap();
        assert_native_folder_round_trip(
            root.path()
                .join(OsString::from_wide(&[b'd' as u16, 0xd800])),
            root.path()
                .join(OsString::from_wide(&[b'r' as u16, 0xdc00])),
            root.path()
                .join(OsString::from_wide(&[b's' as u16, 0xd801])),
        );
    }

    fn native(path: &Path) -> serde_json::Value {
        serde_json::to_value(NativePathValue::new(path)).unwrap()
    }

    #[test]
    fn invalid_saved_native_folders_default_wholly_and_next_save_replaces_them() {
        use serde_json::json;
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("settings.json");
        let valid = native(root.path());
        let settings = json!({"recent_destinations": [valid.clone()],
            "last_destination": valid.clone(), "last_report_folder": valid.clone()});
        let mut foreign = valid.clone();
        foreign["encoding"] = if cfg!(unix) {
            "windows-utf16le"
        } else {
            "unix-bytes"
        }
        .into();
        let mut malformed = valid.clone();
        malformed["base64"] = "%%%".into();
        let oversized = root.path().join("x".repeat(eml_core::MAX_PATH_UNITS + 1));
        let deep = root
            .path()
            .join("x/".repeat(eml_core::MAX_PATH_COMPONENTS + 1));
        let bad = [
            foreign,
            malformed,
            native(Path::new("relative")),
            native(&root.path().join("bad\0")),
            native(&oversized),
            native(&deep),
        ];
        for record in bad {
            for field in [
                "recent_destinations",
                "last_destination",
                "last_report_folder",
            ] {
                let mut invalid = settings.clone();
                invalid[field] = if field == "recent_destinations" {
                    json!([record.clone()])
                } else {
                    record.clone()
                };
                fs::write(&file, serde_json::to_vec(&invalid).unwrap()).unwrap();
                let mut preferences = Preferences::load(file.clone());
                assert!(preferences.destination().is_none(), "{field}: {record}");
                assert!(preferences.recent_destinations().is_empty());
                assert!(preferences.last_report_folder().is_none());
                preferences
                    .set_destination(Some(root.path().to_owned()))
                    .unwrap();
                assert_eq!(
                    Preferences::load(file.clone()).destination(),
                    Some(root.path())
                );
            }
        }
        for invalid in [
            json!({"recent_destinations": [valid.clone(), valid.clone(), valid.clone(), valid.clone()],
            "last_destination": valid.clone(), "last_report_folder": valid}),
            json!({"recent_destinations": ["/old/destination"], "last_destination": "/old/destination", "last_report_folder": null}),
        ] {
            fs::write(&file, serde_json::to_vec(&invalid).unwrap()).unwrap();
            let mut preferences = Preferences::load(file.clone());
            assert!(preferences.destination().is_none());
            assert!(preferences.recent_destinations().is_empty());
            assert!(preferences.last_report_folder().is_none());
            preferences
                .set_report_folder(root.path().to_owned())
                .unwrap();
            assert_eq!(
                Preferences::load(file.clone()).last_report_folder(),
                Some(root.path())
            );
        }
    }

    #[test]
    fn invalid_choices_leave_settings_unchanged_and_failed_save_keeps_latest_choice() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("settings.json");
        let mut preferences = Preferences::load(file.clone());
        preferences
            .set_destination(Some(root.path().to_owned()))
            .unwrap();
        let original = fs::read(&file).unwrap();
        for bad in [
            PathBuf::from("relative"),
            root.path().join("bad\0"),
            root.path().join("x".repeat(eml_core::MAX_PATH_UNITS + 1)),
        ] {
            assert!(preferences.set_destination(Some(bad.clone())).is_err());
            assert!(preferences.set_report_folder(bad).is_err());
            assert_eq!(preferences.destination(), Some(root.path()));
            assert!(preferences.last_report_folder().is_none());
            assert_eq!(fs::read(&file).unwrap(), original);
        }
        let blocked = root.path().join("blocked");
        fs::write(&blocked, b"not a directory").unwrap();
        preferences.path = blocked.join("settings.json");
        let latest = root.path().join("latest");
        assert!(preferences.set_destination(Some(latest.clone())).is_err());
        assert_eq!(preferences.destination(), Some(latest.as_path()));
        assert_eq!(
            preferences.recent_destinations(),
            [latest.clone(), root.path().to_owned()]
        );
        assert_eq!(fs::read(&file).unwrap(), original);
        preferences.path = file.clone();
        preferences
            .set_report_folder(root.path().to_owned())
            .unwrap();
        assert_eq!(
            Preferences::load(file).destination(),
            Some(latest.as_path())
        );
    }

    #[test]
    fn recent_native_folders_keep_three_choices_and_null_destination() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("settings.json");
        let mut preferences = Preferences::load(file.clone());
        for index in 0..5 {
            preferences
                .set_destination(Some(root.path().join(index.to_string())))
                .unwrap();
        }
        preferences.set_destination(None).unwrap();
        let loaded = Preferences::load(file);
        assert!(loaded.destination().is_none());
        assert!(loaded.last_report_folder().is_none());
        assert_eq!(
            loaded.recent_destinations(),
            [
                root.path().join("4"),
                root.path().join("3"),
                root.path().join("2")
            ]
        );
    }
}
