//! Strict bounded protocol-1 input; JSON never enters the business library.
use crate::path::NativePath;
use eml_core::{OutputLocation, RequestPath, RequestPathError, RunMode, RunRequest};
use serde::{Deserialize, Deserializer};
use std::path::PathBuf;

/// Maximum complete request bytes, including leading/trailing whitespace.
pub const MAX_REQUEST_BYTES: usize = 8 * 1024 * 1024;

/// Bounded transport failures, containing no user-supplied JSON or native path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RequestDecodeError {
    /// Byte limit failed before parsing.
    TooLarge,
    /// Invalid JSON, shape, fields, duplicate keys, UTF-8 or trailing data.
    InvalidDocument {
        /// One-based parser line, or zero when unavailable.
        line: usize,
        /// One-based parser column, or zero when unavailable.
        column: usize,
        /// Structured, payload-free reason owned by core vocabulary.
        reason: eml_core::vocabulary::RequestDocumentReason,
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
    InvalidRequest(RequestPathError),
}
impl std::fmt::Display for RequestDecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLarge => write!(f, "Request exceeds {MAX_REQUEST_BYTES} bytes"),
            Self::InvalidDocument {
                line,
                column,
                reason,
            } => f.write_str(&eml_core::vocabulary::request_document_error(
                *line, *column, reason,
            )),
            Self::UnsupportedProtocol => f.write_str("Request must use protocol 1"),
            Self::WrongEncoding { path } => {
                write!(f, "{path}: native encoding differs from host")
            }
            Self::InvalidBase64 { path } => write!(f, "{path}: invalid canonical Base64"),
            Self::OddUtf16Bytes { path } => write!(f, "{path}: incomplete UTF-16LE unit"),
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
            reason: document_reason(&error),
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

fn document_reason(error: &serde_json::Error) -> eml_core::vocabulary::RequestDocumentReason {
    use eml_core::vocabulary::RequestDocumentReason as Reason;
    let text = error.to_string();
    let message = text
        .rsplit_once(" at line ")
        .map_or(text.as_str(), |(message, _)| message);
    if let Some(field) = message
        .strip_prefix("unknown field `")
        .and_then(|s| s.rsplit_once("`, expected").map(|(field, _)| field))
    {
        return Reason::unknown_field(field);
    }
    if let Some(field) = message
        .strip_prefix("duplicate field `")
        .and_then(|s| s.strip_suffix('`'))
    {
        return Reason::duplicate_field(field);
    }
    if message.starts_with("trailing characters") {
        return Reason::TrailingCharacters;
    }
    match error.classify() {
        serde_json::error::Category::Eof => Reason::EndOfDocument,
        serde_json::error::Category::Data => Reason::Type,
        _ => Reason::Syntax,
    }
}
