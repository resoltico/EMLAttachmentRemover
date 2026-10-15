//! Strict bounded protocol-1 input; JSON never enters the business library.
use base64::{Engine, engine::general_purpose::STANDARD};
use eml_core::{
    InvalidRequest, MAX_PATH_UNITS, OutputLocation, PathViolation, RequestPath, RunMode, RunRequest,
};
use serde::{Deserialize, Deserializer};
use std::path::PathBuf;

/// Maximum complete request bytes, including leading/trailing whitespace.
pub const MAX_REQUEST_BYTES: usize = 8 * 1024 * 1024;

/// Bounded transport failures, containing no user-supplied JSON or native path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequestDecodeError {
    /// Byte limit failed before parsing.
    TooLarge,
    /// Invalid JSON, shape, fields, duplicate keys, UTF-8 or trailing data.
    InvalidDocument {
        /// One-based parser line, or zero when unavailable.
        line: usize,
        /// One-based parser column, or zero when unavailable.
        column: usize,
    },
    /// Only protocol 1 is supported.
    UnsupportedProtocol,
    /// Native encoding must match the executing host.
    WrongEncoding {
        /// Explicit input or output expression.
        path: RequestPath,
    },
    /// The native bytes are not canonical padded RFC 4648 Base64.
    InvalidBase64 {
        /// Explicit input or output expression.
        path: RequestPath,
    },
    /// Windows UTF-16LE requires complete two-byte units.
    OddUtf16Bytes {
        /// Explicit input or output expression.
        path: RequestPath,
    },
    /// A core-native resource/representation rule rejected the decoded request.
    InvalidRequest(InvalidRequest),
}
impl std::fmt::Display for RequestDecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLarge => write!(f, "Request exceeds {MAX_REQUEST_BYTES} bytes"),
            Self::InvalidDocument { line, column } => {
                write!(
                    f,
                    "Invalid request document at line {line}, column {column}"
                )
            }
            Self::UnsupportedProtocol => f.write_str("Request must use protocol 1"),
            Self::WrongEncoding { path } => {
                write!(f, "{path:?}: native encoding differs from host")
            }
            Self::InvalidBase64 { path } => write!(f, "{path:?}: invalid canonical Base64"),
            Self::OddUtf16Bytes { path } => write!(f, "{path:?}: incomplete UTF-16LE unit"),
            Self::InvalidRequest(error) => std::fmt::Display::fmt(error, f),
        }
    }
}
impl std::error::Error for RequestDecodeError {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestDocument {
    protocol: u64,
    paths: Vec<Object<NativePath>>,
    // deserialize_with makes omission an error even though explicit null is
    // supported. A plain Option would silently accept a missing required field.
    #[serde(deserialize_with = "required_output")]
    output_directory: Option<Object<NativePath>>,
    dry_run: bool,
}

fn required_output<'de, D: Deserializer<'de>>(
    value: D,
) -> Result<Option<Object<NativePath>>, D::Error> {
    Option::deserialize(value)
}

// Serde's derived structs also accept positional sequences. Protocol objects
// must be maps; retain derived field validation behind this map-only boundary.
struct Object<T>(T);
impl<'de, T: Deserialize<'de>> Deserialize<'de> for Object<T> {
    fn deserialize<D: Deserializer<'de>>(value: D) -> Result<Self, D::Error> {
        struct MapVisitor<T>(std::marker::PhantomData<T>);
        impl<'de, T: Deserialize<'de>> serde::de::Visitor<'de> for MapVisitor<T> {
            type Value = Object<T>;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a JSON object")
            }
            fn visit_map<M: serde::de::MapAccess<'de>>(
                self,
                map: M,
            ) -> Result<Self::Value, M::Error> {
                T::deserialize(serde::de::value::MapAccessDeserializer::new(map)).map(Object)
            }
        }
        value.deserialize_map(MapVisitor(std::marker::PhantomData))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NativePath {
    encoding: NativeEncoding,
    base64: String,
}
#[derive(PartialEq, Eq)]
enum NativeEncoding {
    UnixBytes,
    WindowsUtf16Le,
}
impl<'de> Deserialize<'de> for NativeEncoding {
    fn deserialize<D: Deserializer<'de>>(value: D) -> Result<Self, D::Error> {
        // An externally tagged enum would also accept {"unix-bytes":null}.
        // The wire representation is strictly a string, never a tagged object.
        match String::deserialize(value)?.as_str() {
            "unix-bytes" => Ok(Self::UnixBytes),
            "windows-utf16le" => Ok(Self::WindowsUtf16Le),
            _ => Err(serde::de::Error::custom("Unsupported native encoding")),
        }
    }
}

impl NativePath {
    fn decode(self, location: RequestPath) -> Result<PathBuf, RequestDecodeError> {
        #[cfg(unix)]
        let (host, bytes_per_unit) = (NativeEncoding::UnixBytes, 1);
        #[cfg(windows)]
        let (host, bytes_per_unit) = (NativeEncoding::WindowsUtf16Le, 2);
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

/// Decode one complete protocol-1 request and apply pure core path limits.
///
/// This consumes a bounded buffer whose EOF has already been established by the
/// caller. It does not read stdin, install signals, resolve paths, bind output or
/// emit events. The eventual reader must separately implement interruptible EOF
/// waiting; a successful decode cannot establish that lifecycle property.
pub fn decode_request(
    bytes: &[u8],
    working_directory: PathBuf,
) -> Result<RunRequest, RequestDecodeError> {
    if bytes.len() > MAX_REQUEST_BYTES {
        return Err(RequestDecodeError::TooLarge);
    }
    let Object(document): Object<RequestDocument> =
        serde_json::from_slice(bytes).map_err(|error| RequestDecodeError::InvalidDocument {
            line: error.line(),
            column: error.column(),
        })?;
    if document.protocol != 1 {
        return Err(RequestDecodeError::UnsupportedProtocol);
    }
    let request = RunRequest {
        paths: document
            .paths
            .into_iter()
            .enumerate()
            .map(|(index, Object(path))| path.decode(RequestPath::Input(index)))
            .collect::<Result<_, _>>()?,
        working_directory,
        output: match document.output_directory {
            None => OutputLocation::BesideSource,
            Some(Object(path)) => {
                OutputLocation::Directory(path.decode(RequestPath::OutputDirectory)?)
            }
        },
        mode: if document.dry_run {
            RunMode::DryRun
        } else {
            RunMode::Create
        },
    };
    request
        .validate_path_limits()
        .map_err(RequestDecodeError::InvalidRequest)?;
    Ok(request)
}
