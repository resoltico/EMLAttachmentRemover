//! Lossless native output transport; display text never becomes an operational path.
use crate::request::RequestDecodeError;
use base64::{Engine, engine::general_purpose::STANDARD};
use eml_core::{InvalidRequest, MAX_PATH_UNITS, PathViolation, RequestPath};
use serde::{Deserialize, Deserializer, Serialize, Serializer, ser::SerializeStruct};
use std::path::{Path, PathBuf};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct NativePath {
    encoding: NativeEncoding,
    base64: String,
}
#[derive(PartialEq, Eq)]
enum NativeEncoding {
    UnixBytes,
    WindowsUtf16Le,
}
impl NativeEncoding {
    const fn as_str(&self) -> &'static str {
        match self {
            Self::UnixBytes => "unix-bytes",
            Self::WindowsUtf16Le => "windows-utf16le",
        }
    }
    const fn host() -> Self {
        #[cfg(unix)]
        {
            Self::UnixBytes
        }
        #[cfg(windows)]
        {
            Self::WindowsUtf16Le
        }
    }
    const fn bytes_per_unit(&self) -> usize {
        match self {
            Self::UnixBytes => 1,
            Self::WindowsUtf16Le => 2,
        }
    }
}
impl Serialize for NativeEncoding {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}
impl<'de> Deserialize<'de> for NativeEncoding {
    fn deserialize<D: Deserializer<'de>>(value: D) -> Result<Self, D::Error> {
        // An externally tagged enum would also accept {"unix-bytes":null}.
        // The wire representation is strictly a string, never a tagged object.
        let text = String::deserialize(value)?;
        [Self::UnixBytes, Self::WindowsUtf16Le]
            .into_iter()
            .find(|encoding| encoding.as_str() == text)
            .ok_or_else(|| serde::de::Error::custom("Unsupported native encoding"))
    }
}

impl NativePath {
    fn encode(value: &Path) -> Self {
        #[cfg(unix)]
        let base64 = {
            use std::os::unix::ffi::OsStrExt;
            STANDARD.encode(value.as_os_str().as_bytes())
        };
        #[cfg(windows)]
        let base64 = {
            use std::os::windows::ffi::OsStrExt;
            let bytes: Vec<_> = value
                .as_os_str()
                .encode_wide()
                .flat_map(u16::to_le_bytes)
                .collect();
            STANDARD.encode(bytes)
        };
        Self {
            encoding: NativeEncoding::host(),
            base64,
        }
    }
    pub(super) fn decode(self, location: RequestPath) -> Result<PathBuf, RequestDecodeError> {
        let host = NativeEncoding::host();
        let bytes_per_unit = host.bytes_per_unit();
        if self.encoding != host {
            return Err(RequestDecodeError::WrongEncoding { path: location });
        }
        // Bound the allocation before Base64 decoding. Core remains authoritative
        // for the exact native unit count, NUL, component and aggregate bounds.
        let max_encoded = (MAX_PATH_UNITS * bytes_per_unit).div_ceil(3) * 4;
        if self.base64.len() > max_encoded {
            return Err(RequestDecodeError::InvalidRequest(
                InvalidRequest::InvalidPath {
                    path: location,
                    violation: PathViolation::TooManyUnits,
                },
            ));
        }
        let bytes = STANDARD
            .decode(self.base64)
            .map_err(|_| RequestDecodeError::InvalidBase64 { path: location })?;
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            Ok(std::ffi::OsString::from_vec(bytes).into())
        }
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStringExt;
            if bytes.len() % 2 != 0 {
                return Err(RequestDecodeError::OddUtf16Bytes { path: location });
            }
            let units: Vec<_> = bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| u16::from_le_bytes(*pair))
                .collect();
            Ok(std::ffi::OsString::from_wide(&units).into())
        }
    }
}

/// Borrowed protocol-1 path value from an already admitted native core path.
/// Serialization preserves its spelling; it does not resolve, open or validate it.
pub struct PathValue<'a>(&'a Path);
impl<'a> PathValue<'a> {
    /// Borrow a path for output. Human terminal escaping and whole-event framing
    /// are separate operations; this type writes the JSON path object only.
    pub fn new(path: &'a Path) -> Self {
        Self(path)
    }
}
impl Serialize for PathValue<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut value = serializer.serialize_struct("PathValue", 2)?;
        value.serialize_field("display", &self.0.to_string_lossy())?;
        value.serialize_field("native", &NativePath::encode(self.0))?;
        value.end()
    }
}
