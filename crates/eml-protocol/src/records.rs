//! Owned protocol-1 records for frontend consumers. Processing policy stays in
//! eml-core; independent fixtures check this decoder and the borrowed producer.
use serde::{
    Deserialize, Deserializer, Serialize,
    de::{MapAccess, Visitor, value::MapAccessDeserializer},
};

// Serde structs normally accept positional arrays. Every record in this wire
// contract is a map; keep that representation decision in one decoder boundary.
macro_rules! record {
    ($(#[$doc:meta])* $name:ident { $($(#[$attr:meta])* $field:ident : $ty:ty),* $(,)? }) => {
        $(#[$doc])*
        #[derive(Clone, Debug, Serialize)]
        pub struct $name {
            $($(#[$attr])* #[doc=concat!("Protocol 1 `",stringify!($field),"` observation.")] pub $field:$ty),*
        }
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D:Deserializer<'de>>(deserializer:D)->Result<Self,D::Error> {
                #[derive(Deserialize)]
                struct Fields { $($(#[$attr])* $field:$ty),* }
                struct Map;
                impl<'de> Visitor<'de> for Map {
                    type Value=$name;
                    fn expecting(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result {f.write_str("a protocol record object")}
                    fn visit_map<A:MapAccess<'de>>(self,map:A)->Result<$name,A::Error> {
                        let value=Fields::deserialize(MapAccessDeserializer::new(map))?;
                        Ok($name { $($field:value.$field),* })
                    }
                }
                deserializer.deserialize_map(Map)
            }
        }
    };
}
macro_rules! vocabulary {
    ($(#[$doc:meta])* $name:ident { $($variant:ident=>$wire:literal),* $(,)? }) => {
        $(#[$doc])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
        pub enum $name { $(#[doc=$wire] #[serde(rename=$wire)] $variant),* }
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D:Deserializer<'de>>(deserializer:D)->Result<Self,D::Error> {
                let value=String::deserialize(deserializer)?;
                match value.as_str() {$($wire=>Ok(Self::$variant),)* _=>Err(serde::de::Error::custom("Unknown protocol vocabulary value"))}
            }
        }
    };
}

/// An explicitly present nullable wire member. This is not Serde's missing-key
/// default for Option: the untagged boundary requires an actual JSON value.
#[derive(Clone, Debug, Serialize)]
#[serde(untagged)]
pub enum Nullable<T> {
    /// Explicit JSON null.
    Null(()),
    /// A present value with its ordinary strict wire representation.
    Value(T),
}
impl<'de, T: Deserialize<'de>> Deserialize<'de> for Nullable<T> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Explicit<T>(std::marker::PhantomData<T>);
        impl<'de, T: Deserialize<'de>> Visitor<'de> for Explicit<T> {
            type Value = Nullable<T>;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("an explicit nullable observation")
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                Ok(Nullable::Null(()))
            }
            fn visit_bool<E: serde::de::Error>(self, v: bool) -> Result<Self::Value, E> {
                T::deserialize(serde::de::value::BoolDeserializer::new(v)).map(Nullable::Value)
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Self::Value, E> {
                T::deserialize(serde::de::value::I64Deserializer::new(v)).map(Nullable::Value)
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Self::Value, E> {
                T::deserialize(serde::de::value::U64Deserializer::new(v)).map(Nullable::Value)
            }
            fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<Self::Value, E> {
                T::deserialize(serde::de::value::F64Deserializer::new(v)).map(Nullable::Value)
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
                T::deserialize(serde::de::value::StrDeserializer::new(v)).map(Nullable::Value)
            }
            fn visit_borrowed_str<E: serde::de::Error>(
                self,
                v: &'de str,
            ) -> Result<Self::Value, E> {
                T::deserialize(serde::de::value::BorrowedStrDeserializer::new(v))
                    .map(Nullable::Value)
            }
            fn visit_string<E: serde::de::Error>(self, v: String) -> Result<Self::Value, E> {
                T::deserialize(serde::de::value::StringDeserializer::new(v)).map(Nullable::Value)
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                a: A,
            ) -> Result<Self::Value, A::Error> {
                T::deserialize(serde::de::value::SeqAccessDeserializer::new(a)).map(Nullable::Value)
            }
            fn visit_map<A: MapAccess<'de>>(self, a: A) -> Result<Self::Value, A::Error> {
                T::deserialize(MapAccessDeserializer::new(a)).map(Nullable::Value)
            }
        }
        d.deserialize_any(Explicit(std::marker::PhantomData))
    }
}
impl<T> Nullable<T> {
    /// Borrow the present value, retaining explicit-null semantics on the wire.
    pub fn value(&self) -> Option<&T> {
        match self {
            Self::Null(()) => None,
            Self::Value(value) => Some(value),
        }
    }
}
vocabulary! { /// Native path encoding, independent of the displaying host.
    PathEncoding { UnixBytes=>"unix-bytes", WindowsUtf16Le=>"windows-utf16le" }
}
record! { /// A lossless native path value. Display text never becomes authority.
    NativePathRecord { encoding:PathEncoding, base64:String }
}
record! { /// Human expression and distinct operational native path.
    ReportPathRecord { display:String, native:NativePathRecord }
}
record! { /// Native file identity observed by core.
    IdentityRecord { kind:String, value:String }
}
vocabulary! { /// Requested address knowledge.
    Address {NotChecked=>"not_checked",Verified=>"verified",Changed=>"changed",Unknown=>"unknown"}
}
vocabulary! { /// Synchronization observation.
    Synchronization {NotAttempted=>"not_attempted",Succeeded=>"succeeded",Unsupported=>"unsupported",Failed=>"failed"}
}
vocabulary! { /// Actual requested native synchronization method.
    SynchronizationMethod {NotRequested=>"not_requested",Fsync=>"fsync",FsyncFullfsync=>"fsync_fullfsync",FsyncFullfsyncUnsupported=>"fsync_fullfsync_unsupported",FlushFileBuffers=>"flush_file_buffers"}
}
record! { /// One file synchronization observation.
    SyncRecord {method:SynchronizationMethod,state:Synchronization,native_code:Nullable<i64>}
}
record! { /// Namespace synchronization tracks independently required edges.
    NamespaceRecord {method:SynchronizationMethod,state:Synchronization,native_code:Nullable<i64>,required_edges:u32,completed_edges:u32,failed_edge_index:Nullable<u32>}
}
vocabulary! { /// Cleanup knowledge; residual/unknown never mean rollback.
    Cleanup {NotNeeded=>"not_needed",Complete=>"complete",Residual=>"residual",Unknown=>"unknown"}
}
record! { /// Private staging cleanup observations.
    CleanupRecord {state:Cleanup,residual_name:Nullable<NativePathRecord>,native_code:Nullable<i64>}
}
vocabulary! { /// Independent publication knowledge.
    Publication {NotPublished=>"not_published",Published=>"published",Unknown=>"unknown"}
}
record! { /// Core's publication evidence, not a frontend success Boolean.
    PublicationRecord {state:Publication,candidate_identity:Nullable<IdentityRecord>,address:Address,file_sync:SyncRecord,namespace_sync:NamespaceRecord,cleanup:CleanupRecord}
}
record! { /// Qualification of a previously existing copy.
    ExistingRecord {identity:Nullable<IdentityRecord>,bytes_equal:Nullable<bool>,stable:bool,address:Address,file_sync:SyncRecord,namespace_sync:NamespaceRecord}
}
vocabulary! { /// Closed operation phases; error codes remain open vocabulary.
    ErrorPhase {Admission=>"admission",Collection=>"collection",Read=>"read",Prepare=>"prepare",Existing=>"existing",Stage=>"stage",Write=>"write",Sync=>"sync",Publish=>"publish",Address=>"address",Cleanup=>"cleanup",Delivery=>"delivery"}
}
record! { /// Bounded diagnostic facts without message payloads.
    ErrorRecord {code:String,phase:ErrorPhase,detail:String,section:Nullable<String>}
}
vocabulary! { /// Policy reason supplied by the business library.
    Removal {Attachment=>"attachment",RelatedResource=>"related_resource",NonBodyPart=>"non_body_part"}
}
record! { /// Metadata for one removed entity; not the entity's contents.
    RemovedRecord {section:String,content_type:String,filename:Nullable<String>,bytes:u64,reason:Removal}
}
vocabulary! { /// Closed protocol item outcomes.
    ItemStatus {Created=>"created",UpToDate=>"up_to_date",Planned=>"planned",Failed=>"failed"}
}
vocabulary! { /// Read-only plan which never qualifies an existing copy.
    Plan {Create=>"create",VerifyExisting=>"verify_existing"}
}
record! { /// One terminal item observation; indices are contiguous in emitted execution order.
    ItemRecord {index:u32,source:ReportPathRecord,status:ItemStatus,destination:Nullable<ReportPathRecord>,source_bytes:Nullable<u64>,copy_bytes:Nullable<u64>,removed:Vec<RemovedRecord>,stripped_fields:Vec<String>,plan:Nullable<Plan>,publication:PublicationRecord,existing_observation:Nullable<ExistingRecord>,error:Nullable<ErrorRecord>}
}
record! { /// Start facts emitted after core collection/admission.
    StartRecord {protocol:u32,program:String,version:String,dry_run:bool,total:u32,collection_complete:bool}
}
record! { /// Summary counts checked against accepted item observations.
    CountRecord {created:u32,up_to_date:u32,planned:u32,failed:u32,not_processed:u32}
}
record! { /// One final summary, whose exit must match actual normal process exit.
    SummaryRecord {counts:CountRecord,interrupted:bool,collection_complete:bool,run_error:Nullable<ErrorRecord>,exit_code:i32}
}
/// Current protocol events. Unknown event kinds are refused; additive object
/// fields may be ignored. No old protocol or compatibility decoder exists.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventRecord {
    /// Opening event.
    Start(StartRecord),
    /// One terminal item.
    Item(Box<ItemRecord>),
    /// Closing summary.
    Summary(SummaryRecord),
}
/// Decode one complete LF-free protocol record with strict map/value shapes.
/// Stateful order, receipts, aggregate bounds and exit checks belong to eml-app.
pub fn decode_record(bytes: &[u8]) -> Result<EventRecord, serde_json::Error> {
    serde_json::from_slice(bytes)
}
