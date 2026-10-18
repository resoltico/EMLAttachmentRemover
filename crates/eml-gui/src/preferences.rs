//! The fresh native destination String preference and argument-domain override.
use base64::{Engine, engine::general_purpose::STANDARD};
use objc2::rc::Retained;
use objc2_foundation::{NSArgumentDomain, NSString, NSUserDefaults};
use std::{
    os::unix::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
};
const KEY: &str = "copyDestinationBytes";
fn decode_string(text: &NSString) -> Result<Option<PathBuf>, PreferenceError> {
    let length = text.length();
    if length > eml_core::MAX_PATH_UNITS.div_ceil(3) * 4 {
        return Err(PreferenceError);
    }
    // Read bounded UTF16 code units directly. This avoids UTF8String formatting
    // and NSString subclass BOOL/NSRange encoding differences in native bridges.
    let mut bytes = Vec::with_capacity(length);
    for index in 0..length {
        let unit = text.characterAtIndex(index);
        if unit == 0 || unit > 127 {
            return Err(PreferenceError);
        }
        bytes.push(unit as u8);
    }
    decode(std::str::from_utf8(&bytes).map_err(|_| PreferenceError)?)
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct PreferenceError;
impl std::fmt::Display for PreferenceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(
            "The copy destination could not be read. Choose a folder or Beside originals again.",
        )
    }
}
impl std::error::Error for PreferenceError {}
fn decode(value: &str) -> Result<Option<PathBuf>, PreferenceError> {
    if value.is_empty() {
        return Ok(None);
    }
    if value.len() > eml_core::MAX_PATH_UNITS.div_ceil(3) * 4 {
        return Err(PreferenceError);
    }
    let bytes = STANDARD.decode(value).map_err(|_| PreferenceError)?;
    if bytes.is_empty()
        || bytes.len() > eml_core::MAX_PATH_UNITS
        || bytes.contains(&0)
        || STANDARD.encode(&bytes) != value
    {
        return Err(PreferenceError);
    }
    let path = PathBuf::from(std::ffi::OsString::from_vec(bytes));
    if !path.is_absolute() {
        return Err(PreferenceError);
    }
    Ok(Some(path))
}
fn encode(path: Option<&Path>) -> Result<String, PreferenceError> {
    if path.is_some_and(|path| {
        !path.is_absolute()
            || path.as_os_str().as_bytes().len() > eml_core::MAX_PATH_UNITS
            || path.as_os_str().as_bytes().contains(&0)
    }) {
        return Err(PreferenceError);
    }
    let value = path.map_or_else(String::new, |path| {
        STANDARD.encode(path.as_os_str().as_bytes())
    });
    decode(&value)?;
    Ok(value)
}
pub(crate) struct DestinationPreference {
    defaults: Retained<NSUserDefaults>,
    session_only: bool,
    destination: Result<Option<PathBuf>, PreferenceError>,
}
impl DestinationPreference {
    /// Read only the fresh key. No registration, synchronization or migration.
    pub(crate) fn load() -> Self {
        Self::from_defaults(NSUserDefaults::standardUserDefaults())
    }
    pub(crate) fn from_defaults(defaults: Retained<NSUserDefaults>) -> Self {
        let key = NSString::from_str(KEY);
        // SAFETY: Foundation exports this immutable NSString constant.
        let argument_domain = defaults.volatileDomainForName(unsafe { NSArgumentDomain });
        let session_only = argument_domain.objectForKey(&key).is_some();
        let destination = defaults.objectForKey(&key).map_or(Ok(None), |object| {
            let text = object.downcast::<NSString>().map_err(|_| PreferenceError)?;
            decode_string(&text)
        });
        Self {
            defaults,
            session_only,
            destination,
        }
    }
    pub(crate) fn destination(&self) -> Result<Option<&Path>, PreferenceError> {
        self.destination
            .as_ref()
            .map(|value| value.as_deref())
            .map_err(|error| *error)
    }
    /// Invoke only for an explicit user destination action. An argument-domain
    /// override is session-only even when the user chooses a different folder.
    pub(crate) fn choose(&mut self, path: Option<PathBuf>) -> Result<(), PreferenceError> {
        let encoded = encode(path.as_deref())?;
        if !self.session_only {
            let value = NSString::from_str(&encoded);
            let key = NSString::from_str(KEY);
            // SAFETY: NSString is an allowed property-list value; only this
            // exact fresh destination key is written after explicit user action.
            unsafe {
                self.defaults.setObject_forKey(Some(&value), &key);
            }
        }
        self.destination = Ok(path);
        Ok(())
    }
    pub(crate) fn session_only(&self) -> bool {
        self.session_only
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fresh_destination_string_preserves_native_bytes_and_empty_means_beside_originals() {
        assert_eq!(decode("").unwrap(), None);
        let path = PathBuf::from(std::ffi::OsString::from_vec(
            b"/owned-synthetic/\xff copies".to_vec(),
        ));
        let value = encode(Some(&path)).unwrap();
        assert_eq!(decode(&value).unwrap(), Some(path));
    }
    #[test]
    fn invalid_destination_strings_are_not_reinterpreted_or_repaired() {
        assert!(encode(Some(Path::new(""))).is_err());
        assert!(encode(Some(Path::new("relative"))).is_err());
        for value in ["not base64", "YQ==", "L2EAAA==", "L2E", " L2E="] {
            assert!(decode(value).is_err());
        }
        let mut bytes = vec![b'/'];
        bytes.extend(vec![b'a'; eml_core::MAX_PATH_UNITS]);
        assert!(decode(&STANDARD.encode(bytes)).is_err());
    }
    #[test]
    fn native_string_conversion_refuses_non_ascii_unpaired_utf16_and_embedded_nul() {
        use objc2::AnyThread;
        assert!(decode_string(&NSString::from_str("é")).is_err());
        assert!(decode_string(&NSString::from_str("\0L2E=")).is_err());
        let mut units = [0xd800u16];
        // SAFETY: NSString copies this initialized UTF16 array. An unpaired
        // scalar is deliberate native input to the refusing ASCII boundary.
        let invalid = unsafe {
            NSString::initWithCharacters_length(
                NSString::alloc(),
                std::ptr::NonNull::from(&mut units[0]),
                units.len(),
            )
        };
        assert!(decode_string(&invalid).is_err());
        let mut bytes = vec![b'/'];
        bytes.extend(vec![b'a'; 32_767]);
        let text = NSString::from_str(&STANDARD.encode(&bytes));
        assert_eq!(
            decode_string(&text)
                .unwrap()
                .unwrap()
                .as_os_str()
                .as_bytes(),
            bytes
        );
        bytes.push(b'a');
        assert!(decode_string(&NSString::from_str(&STANDARD.encode(bytes))).is_err());
    }
}
