//! Strict, private desktop operation transport, independent of public protocol 1.
use crate::path::NativePath;
use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{self, MapAccess, Visitor},
    ser::SerializeMap,
};
use std::{
    fmt,
    io::{self, Read, Write},
    path::PathBuf,
};

/// Exact fixed protocol prefix.
pub const MAGIC: &[u8] = b"EML-DESKTOP/2\n";
/// Private desktop protocol revision.
pub const PROTOCOL: u8 = 2;
/// Inclusive encoded header byte limit.
pub const MAX_HEADER_BYTES: usize = 64 * 1024;
/// Inclusive nonzero body chunk byte limit.
pub const MAX_CHUNK_BYTES: usize = 1024 * 1024;
/// Inclusive complete technical report byte limit.
pub const MAX_REPORT_BYTES: usize = 256 * 1024 * 1024;
/// Inclusive complete export receipt byte limit.
pub const MAX_RECEIPT_BYTES: usize = 64 * 1024;
/// Checked session-local child identity, independent of a processing report ID.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct OperationId(u64);
impl OperationId {
    /// Admit a positive child identity.
    pub fn new(value: u64) -> Option<Self> {
        (value != 0).then_some(Self(value))
    }
    /// Exact numeric child identity.
    pub fn get(self) -> u64 {
        self.0
    }
    /// Advance identity, refusing exhaustion.
    pub fn next(self) -> Option<Self> {
        self.0.checked_add(1).and_then(Self::new)
    }
}
impl<'de> Deserialize<'de> for OperationId {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Self::new(u64::deserialize(d)?).ok_or_else(|| de::Error::custom("Zero operation identity"))
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
/// The two private child operation kinds.
pub enum Operation {
    /// Process a canonical protocol-1 request.
    Processing,
    /// Publish the validated metadata document at this exact absolute address.
    Export {
        /// Exact absolute report address.
        destination: PathBuf,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
/// Strict child operation identity and kind.
pub struct Header {
    /// Exact child operation identity.
    pub operation_id: OperationId,
    /// Selected operation and its operation-specific fields.
    pub operation: Operation,
}
impl Header {
    /// Inclusive complete body budget for this operation.
    pub fn body_limit(&self) -> usize {
        match self.operation {
            Operation::Processing => crate::MAX_REQUEST_BYTES,
            Operation::Export { .. } => MAX_REPORT_BYTES,
        }
    }
}
impl Serialize for Header {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut m = s.serialize_map(Some(if matches!(self.operation, Operation::Processing) {
            2
        } else {
            3
        }))?;
        m.serialize_entry("operation_id", &self.operation_id)?;
        match &self.operation {
            Operation::Processing => m.serialize_entry("operation", "processing")?,
            Operation::Export { destination } => {
                m.serialize_entry("operation", "export")?;
                m.serialize_entry("destination", &NativePath::encode(destination))?;
            }
        }
        m.end()
    }
}
impl<'de> Deserialize<'de> for Header {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct HeaderVisitor;
        impl<'de> Visitor<'de> for HeaderVisitor {
            type Value = Header;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a desktop header object")
            }
            fn visit_map<M: MapAccess<'de>>(self, mut m: M) -> Result<Header, M::Error> {
                let (mut id, mut operation, mut destination) = (None, None, None);
                while let Some(key) = m.next_key::<String>()? {
                    match key.as_str() {
                        "operation_id" if id.is_none() => id = Some(m.next_value::<OperationId>()?),
                        "operation" if operation.is_none() => {
                            operation = Some(m.next_value::<String>()?)
                        }
                        "destination" if destination.is_none() => {
                            destination = Some(m.next_value::<NativePath>()?)
                        }
                        _ => {
                            return Err(de::Error::custom(
                                "Unknown or duplicate desktop header field",
                            ));
                        }
                    }
                }
                let operation = match (operation.as_deref(), destination) {
                    (Some("processing"), None) => Operation::Processing,
                    (Some("export"), Some(path)) => {
                        let destination = path
                            .decode(eml_core::RequestPath::OutputDirectory)
                            .map_err(de::Error::custom)?;
                        if !destination.is_absolute() {
                            return Err(de::Error::custom("Export requires absolute destination"));
                        }
                        Operation::Export { destination }
                    }
                    _ => return Err(de::Error::custom("Invalid desktop operation members")),
                };
                Ok(Header {
                    operation_id: id.ok_or_else(|| de::Error::missing_field("operation_id"))?,
                    operation,
                })
            }
        }
        d.deserialize_map(HeaderVisitor)
    }
}
fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
fn length(reader: &mut impl Read) -> io::Result<usize> {
    let mut b = [0; 4];
    reader.read_exact(&mut b)?;
    Ok(u32::from_be_bytes(b) as usize)
}
/// Retain exactly one bounded body; no operation may acquire publication authority before this returns.
pub fn read_operation(reader: impl Read) -> io::Result<(Header, Vec<u8>)> {
    read_operation_engine(
        reader,
        #[cfg(feature = "native-fixtures")]
        |_| {},
    )
}
/// Observe the already validated strict header before any body read in the same
/// framing engine. This nonshipping boundary supplies no replacement bytes or
/// operation authority; all bounded framing and EOF checks still follow.
#[cfg(feature = "native-fixtures")]
pub fn read_operation_observed(
    reader: impl Read,
    observer: impl FnMut(&Header),
) -> io::Result<(Header, Vec<u8>)> {
    read_operation_engine(reader, observer)
}
fn read_operation_engine(
    mut reader: impl Read,
    #[cfg(feature = "native-fixtures")] mut observer: impl FnMut(&Header),
) -> io::Result<(Header, Vec<u8>)> {
    let mut magic = [0; 14];
    reader.read_exact(&mut magic)?;
    if magic != MAGIC {
        return Err(invalid("Wrong desktop protocol magic"));
    }
    let n = length(&mut reader)?;
    if n == 0 || n > MAX_HEADER_BYTES {
        return Err(invalid("Desktop header limit"));
    }
    let mut bytes = vec![0; n];
    reader.read_exact(&mut bytes)?;
    let header: Header =
        serde_json::from_slice(&bytes).map_err(|_| invalid("Invalid desktop header"))?;
    #[cfg(feature = "native-fixtures")]
    observer(&header);
    let mut body = Vec::new();
    loop {
        let n = length(&mut reader)?;
        if n == 0 {
            break;
        }
        let size = body
            .len()
            .checked_add(n)
            .ok_or_else(|| invalid("Desktop body overflow"))?;
        if n > MAX_CHUNK_BYTES || size > header.body_limit() {
            return Err(invalid("Desktop body limit"));
        }
        let start = body.len();
        body.resize(size, 0);
        reader.read_exact(&mut body[start..])?;
    }
    let mut tail = [0];
    if reader.read(&mut tail)? != 0 {
        return Err(invalid("Trailing desktop bytes"));
    }
    Ok((header, body))
}
/// Streaming bounded framing writer. finish is called only after successful serialization.
pub struct BodyWriter<W: Write> {
    inner: W,
    used: usize,
    limit: usize,
    buffer: Vec<u8>,
}
impl<W: Write> BodyWriter<W> {
    /// Write the fixed prefix and strict bounded header.
    pub fn new(mut inner: W, header: &Header) -> io::Result<Self> {
        let bytes = serde_json::to_vec(header).map_err(io::Error::other)?;
        if bytes.len() > MAX_HEADER_BYTES {
            return Err(invalid("Desktop header limit"));
        }
        inner.write_all(MAGIC)?;
        inner.write_all(&(bytes.len() as u32).to_be_bytes())?;
        inner.write_all(&bytes)?;
        Ok(Self {
            inner,
            used: 0,
            limit: header.body_limit(),
            buffer: Vec::with_capacity(MAX_CHUNK_BYTES),
        })
    }
    fn flush_chunk(&mut self) -> io::Result<()> {
        if !self.buffer.is_empty() {
            self.inner
                .write_all(&(self.buffer.len() as u32).to_be_bytes())?;
            self.inner.write_all(&self.buffer)?;
            self.buffer.clear();
        }
        Ok(())
    }
    /// Emit the sole body terminator after successful serialization.
    pub fn finish(mut self) -> io::Result<W> {
        self.flush_chunk()?;
        self.inner.write_all(&0u32.to_be_bytes())?;
        Ok(self.inner)
    }
}
impl<W: Write> Write for BodyWriter<W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let total = self
            .used
            .checked_add(bytes.len())
            .ok_or_else(|| invalid("Desktop body overflow"))?;
        if total > self.limit {
            return Err(invalid("Desktop body limit"));
        }
        let mut remaining = bytes;
        while !remaining.is_empty() {
            let n = remaining.len().min(MAX_CHUNK_BYTES - self.buffer.len());
            self.buffer.extend_from_slice(&remaining[..n]);
            remaining = &remaining[n..];
            if self.buffer.len() == MAX_CHUNK_BYTES {
                self.flush_chunk()?;
            }
        }
        self.used = total;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        self.flush_chunk()?;
        self.inner.flush()
    }
}
/// Exact product, compiled target, protocol and supported-operation handshake.
pub fn identity_line(version: &str, build: &str) -> String {
    format!(
        "eml-worker {version} build={build} {}-{} desktop=2 operations=processing,export\n",
        std::env::consts::ARCH,
        std::env::consts::OS
    )
}

macro_rules! receipt_record {
    ($name:ident {$($field:ident:$ty:ty),*$(,)?})=> {
        #[derive(Clone,Debug,Serialize)]
        #[doc=concat!("Strict private ",stringify!($name)," document.")]
        pub struct $name {$(#[doc=concat!("Observed `",stringify!($field),"` export fact.")] pub $field:$ty),*}
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D:Deserializer<'de>>(d:D)->Result<Self,D::Error> {
                #[derive(Deserialize)] #[serde(deny_unknown_fields)] struct Fields {$($field:$ty),*}
                struct Map;
                impl<'de> Visitor<'de> for Map {
                    type Value=$name;
                    fn expecting(&self,f:&mut fmt::Formatter<'_>)->fmt::Result{f.write_str("a strict export receipt object")}
                    fn visit_map<A:MapAccess<'de>>(self,a:A)->Result<Self::Value,A::Error>{let x=Fields::deserialize(de::value::MapAccessDeserializer::new(a))?;Ok($name{$($field:x.$field),*})}
                }
                d.deserialize_map(Map)
            }
        }
    }
}
receipt_record!(ReceiptSync {method:crate::records::SynchronizationMethod,state:crate::records::Synchronization,native_code:crate::records::Nullable<i64>});
receipt_record!(ReceiptIdentity {
    kind: String,
    value: String
});
receipt_record!(ReceiptNativePath {
    encoding: crate::records::PathEncoding,
    base64: String
});
receipt_record!(ReceiptCleanup {state:crate::records::Cleanup,residual_name:crate::records::Nullable<ReceiptNativePath>,native_code:crate::records::Nullable<i64>});
receipt_record!(ExportReceipt {
    operation_id:OperationId,
    publication:String,
    file_sync:crate::records::Nullable<ReceiptSync>,
    namespace_sync:crate::records::Nullable<ReceiptSync>,
    address_verified:crate::records::Nullable<bool>,
    candidate_identity:crate::records::Nullable<ReceiptIdentity>,
    final_file_verified:crate::records::Nullable<bool>,
    cleanup:ReceiptCleanup,
    failures:Vec<ExportFailure>
});
receipt_record!(ExportFailure {stage:String,kind:crate::records::Nullable<String>,native_code:crate::records::Nullable<i32>,detail:String});
impl ExportReceipt {
    /// All independent publication qualifications were observed without failures.
    pub fn qualified(&self) -> bool {
        self.publication == "published"
            && self.address_verified.value() == Some(&true)
            && self.final_file_verified.value() == Some(&true)
            && self.candidate_identity.value().is_some()
            && self.cleanup.state == crate::records::Cleanup::Complete
            && self.cleanup.residual_name.value().is_none()
            && self.cleanup.native_code.value().is_none()
            && self.file_sync.value().is_some_and(|s| {
                crate::meaning::synchronization_receipt(
                    s.method,
                    s.state,
                    s.native_code.value().copied(),
                )
                .is_some_and(|receipt| {
                    receipt.state == eml_core::SyncState::Unsupported || receipt.succeeded()
                })
            })
            && self.namespace_sync.value().is_some_and(|s| {
                crate::meaning::synchronization_receipt(
                    s.method,
                    s.state,
                    s.native_code.value().copied(),
                )
                .is_some_and(|receipt| {
                    receipt.state == eml_core::SyncState::Unsupported || receipt.succeeded()
                })
            })
            && self.failures.is_empty()
    }
}
/// Serialize one receipt with an explicit inclusive byte bound.
pub fn encode_receipt(receipt: &ExportReceipt) -> io::Result<Vec<u8>> {
    struct Bounded(Vec<u8>);
    impl Write for Bounded {
        fn write(&mut self, b: &[u8]) -> io::Result<usize> {
            if self
                .0
                .len()
                .checked_add(b.len())
                .is_none_or(|n| n > MAX_RECEIPT_BYTES)
            {
                return Err(invalid("Export receipt limit"));
            }
            self.0.extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut writer = Bounded(Vec::new());
    serde_json::to_writer(&mut writer, receipt).map_err(io::Error::other)?;
    Ok(writer.0)
}
/// Accept a complete bounded receipt only for the exact child operation.
pub fn decode_receipt(bytes: &[u8], expected: OperationId) -> io::Result<ExportReceipt> {
    let (receipt, complete) = observe_receipt(bytes, expected)?;
    if !complete {
        return Err(invalid("Trailing export receipt bytes"));
    }
    Ok(receipt)
}
/// Retain a valid first receipt independently of subsequent stdout corruption.
/// The boolean requires clean document EOF; false is always a transport failure.
pub fn observe_receipt(bytes: &[u8], expected: OperationId) -> io::Result<(ExportReceipt, bool)> {
    if bytes.len() > MAX_RECEIPT_BYTES {
        return Err(invalid("Export receipt limit"));
    }
    let mut decoder = serde_json::Deserializer::from_slice(bytes);
    let r =
        ExportReceipt::deserialize(&mut decoder).map_err(|_| invalid("Invalid export receipt"))?;
    let complete = decoder.end().is_ok();
    if r.operation_id != expected
        || !["not_published", "published", "unknown"].contains(&r.publication.as_str())
        || r.failures.len() > 8
        || r.failures.iter().any(|f| {
            f.detail.chars().count() > 1000
                || ![
                    "destination",
                    "serialization",
                    "staging",
                    "before_publication",
                    "publication",
                    "verification",
                    "cleanup",
                    "namespace_sync",
                    "address",
                ]
                .contains(&f.stage.as_str())
        })
    {
        return Err(invalid("Export receipt identity or bounds"));
    }
    if r.final_file_verified.value() == Some(&true)
        && (r.publication == "not_published" || r.candidate_identity.value().is_none())
    {
        return Err(invalid("Contradictory export identity readback"));
    }
    if let Some(identity) = r.candidate_identity.value() {
        let valid = match identity.kind.as_str() {
            "posix" => identity
                .value
                .split_once(':')
                .is_some_and(|(a, b)| a.parse::<u64>().is_ok() && b.parse::<u64>().is_ok()),
            "windows" => {
                identity.value.len() == 49
                    && identity.value.as_bytes()[16] == b':'
                    && identity
                        .value
                        .bytes()
                        .enumerate()
                        .all(|(i, b)| i == 16 || b.is_ascii_hexdigit())
            }
            _ => false,
        };
        if !valid {
            return Err(invalid("Invalid export candidate identity"));
        }
    }
    if [&r.file_sync, &r.namespace_sync]
        .iter()
        .filter_map(|s| s.value())
        .any(|s| {
            s.method == crate::records::SynchronizationMethod::NotRequested
                || s.state == crate::records::Synchronization::NotAttempted
                || (s.state == crate::records::Synchronization::Succeeded
                    && !crate::meaning::synchronization_receipt(
                        s.method,
                        s.state,
                        s.native_code.value().copied(),
                    )
                    .is_some_and(eml_core::SyncReceipt::succeeded))
        })
    {
        return Err(invalid("Invalid export synchronization receipt"));
    }
    if let Some(name) = r.cleanup.residual_name.value() {
        use base64::{Engine, engine::general_purpose::STANDARD};
        let raw = STANDARD
            .decode(&name.base64)
            .map_err(|_| invalid("Invalid residual component"))?;
        if STANDARD.encode(&raw) != name.base64 || raw.is_empty() || raw.len() > 510 {
            return Err(invalid("Residual component bound"));
        }
        let valid = match name.encoding {
            crate::records::PathEncoding::UnixBytes => {
                raw.len() <= 255 && !raw.contains(&0) && !raw.contains(&b'/')
            }
            crate::records::PathEncoding::WindowsUtf16Le => {
                raw.len().is_multiple_of(2)
                    && raw
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .all(|p| ![[0, 0], [47, 0], [92, 0]].contains(p))
            }
        };
        if !valid {
            return Err(invalid("Invalid residual native component"));
        }
    }
    Ok((r, complete))
}

/// Native export observations projected through the same vocabulary as protocol 1.
pub struct ExportObservations<'a> {
    /// Actual no-replace edge state.
    pub publication: &'static str,
    /// Actual candidate synchronization.
    pub file_sync: Option<eml_core::SyncReceipt>,
    /// Actual parent namespace synchronization.
    pub namespace_sync: Option<eml_core::SyncReceipt>,
    /// Requested address qualification.
    pub address_verified: Option<bool>,
    /// Held candidate identity.
    pub candidate_identity: Option<eml_core::Identity>,
    /// Exact final identity and byte readback.
    pub final_file_verified: Option<bool>,
    /// Identity-qualified temporary cleanup.
    pub cleanup: &'a eml_core::CleanupReceipt,
}
/// Encode native observations using the authoritative shared protocol-1 projections.
pub fn export_receipt(
    operation_id: OperationId,
    observations: ExportObservations<'_>,
    failures: Vec<ExportFailure>,
) -> io::Result<ExportReceipt> {
    let ExportObservations {
        publication,
        file_sync,
        namespace_sync,
        address_verified,
        candidate_identity,
        final_file_verified,
        cleanup,
    } = observations;
    #[derive(Serialize)]
    struct Projection<'a> {
        operation_id: OperationId,
        publication: &'static str,
        file_sync: Option<crate::event::SyncFields>,
        namespace_sync: Option<crate::event::SyncFields>,
        address_verified: Option<bool>,
        candidate_identity: Option<crate::event::IdentityFields>,
        final_file_verified: Option<bool>,
        cleanup: crate::event::CleanupFields,
        failures: &'a [ExportFailure],
    }
    let p = Projection {
        operation_id,
        publication,
        file_sync: file_sync.map(|s| crate::event::sync(Some(s))),
        namespace_sync: namespace_sync.map(|s| crate::event::sync(Some(s))),
        address_verified,
        candidate_identity: candidate_identity.map(crate::event::identity),
        final_file_verified,
        cleanup: crate::event::cleanup(cleanup),
        failures: &failures,
    };
    let bytes = serde_json::to_vec(&p).map_err(io::Error::other)?;
    decode_receipt(&bytes, operation_id)
}
