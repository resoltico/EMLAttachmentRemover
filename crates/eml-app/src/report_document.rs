//! Versioned metadata-only report writer and bounded structural validator.
use crate::{ProtocolFailure, RunId, WorkerCompletion};
use eml_protocol::records::{ItemRecord, StartRecord, SummaryRecord};
mod strict;
use serde::{
    Deserialize, Deserializer, Serialize,
    de::{self, MapAccess, SeqAccess, Visitor},
};
use serde_json::value::RawValue;
use std::{
    fmt,
    io::{self, Write},
};
/// Required technical report schema identifier.
pub const SCHEMA: &str = "eml-technical-report";
/// First explicitly versioned report contract.
pub const SCHEMA_VERSION: u8 = 1;
#[derive(Debug)]
/// Typed failure from bounded streaming report serialization.
pub enum ReportWriteError {
    /// Serialization exceeded the inclusive 256 MiB document budget.
    ReportTooLarge,
    /// The destination writer or JSON serializer failed.
    Serialization(serde_json::Error),
}
impl fmt::Display for ReportWriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReportTooLarge => f.write_str("Report exceeds the 256 MiB export limit"),
            Self::Serialization(e) => e.fmt(f),
        }
    }
}
impl std::error::Error for ReportWriteError {}
struct Budget<W> {
    inner: W,
    used: usize,
    exceeded: bool,
}
impl<W: Write> Write for Budget<W> {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        let Some(total) = self
            .used
            .checked_add(b.len())
            .filter(|n| *n <= eml_protocol::desktop::MAX_REPORT_BYTES)
        else {
            self.exceeded = true;
            return Err(io::Error::other("ReportTooLarge"));
        };
        self.inner.write_all(b)?;
        self.used = total;
        Ok(b.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}
#[derive(Serialize)]
struct Document<'a> {
    schema: &'static str,
    schema_version: u8,
    version: &'static str,
    build: u16,
    run_id: RunId,
    terminal: crate::RunMeaning,
    start: &'a Option<StartRecord>,
    items: &'a [ItemRecord],
    summary: &'a Option<SummaryRecord>,
    protocol_failure: Option<ProtocolFailure>,
    worker: &'a WorkerCompletion,
}
pub(crate) fn write(
    writer: impl Write,
    run_id: RunId,
    start: &Option<StartRecord>,
    items: &[ItemRecord],
    summary: &Option<SummaryRecord>,
    protocol_failure: Option<ProtocolFailure>,
    worker: &WorkerCompletion,
) -> Result<(), ReportWriteError> {
    let mut budget = Budget {
        inner: writer,
        used: 0,
        exceeded: false,
    };
    let result = serde_json::to_writer(
        &mut budget,
        &Document {
            schema: SCHEMA,
            schema_version: SCHEMA_VERSION,
            version: eml_core::product::VERSION,
            build: eml_core::product::BUILD_NUMBER,
            run_id,
            terminal: crate::lifecycle::classify_run(summary.as_ref(), protocol_failure, worker),
            start,
            items,
            summary,
            protocol_failure,
            worker,
        },
    );
    if budget.exceeded {
        Err(ReportWriteError::ReportTooLarge)
    } else {
        result.map_err(ReportWriteError::Serialization)
    }
}
#[derive(Debug)]
/// A complete retained report failed strict structural admission.
pub struct InvalidReport;
impl fmt::Display for InvalidReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Invalid versioned technical report")
    }
}
impl std::error::Error for InvalidReport {}
/// Validate retained bytes in place. Only one bounded record is decoded at a time;
/// no second report tree or source-message content is materialized.
pub fn validate(bytes: &[u8]) -> Result<(), InvalidReport> {
    if bytes.len() > eml_protocol::desktop::MAX_REPORT_BYTES {
        return Err(InvalidReport);
    }
    let mut decoder = serde_json::Deserializer::from_slice(bytes);
    decoder
        .deserialize_map(DocumentVisitor)
        .map_err(|_| InvalidReport)?;
    decoder.end().map_err(|_| InvalidReport)
}
struct DocumentVisitor;
impl<'de> Visitor<'de> for DocumentVisitor {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a versioned report object")
    }
    fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<(), M::Error> {
        let mut seen = 0u16;
        let mut start_version = None;
        let mut total = None;
        let mut collection_complete = None;
        let mut item_counts = [0u32; 4];
        let mut highest = None;
        let (mut summary, mut protocol_failure, mut worker, mut terminal) =
            (None, None, None, None);
        while let Some(key) = map.next_key::<&str>()? {
            let index = match key {
                "schema" => 0,
                "schema_version" => 1,
                "version" => 2,
                "build" => 3,
                "run_id" => 4,
                "start" => 5,
                "items" => 6,
                "summary" => 7,
                "protocol_failure" => 8,
                "worker" => 9,
                "terminal" => 10,
                _ => return Err(de::Error::custom("Unknown report field")),
            };
            if seen & (1 << index) != 0 {
                return Err(de::Error::custom("Duplicate report field"));
            }
            seen |= 1 << index;
            match index {
                0 => {
                    if map.next_value::<&str>()? != SCHEMA {
                        return Err(de::Error::custom("Unsupported schema"));
                    }
                }
                1 => {
                    if map.next_value::<u8>()? != SCHEMA_VERSION {
                        return Err(de::Error::custom("Unsupported schema version"));
                    }
                }
                2 => {
                    if map.next_value::<&str>()? != eml_core::product::VERSION {
                        return Err(de::Error::custom("Wrong product version"));
                    }
                }
                3 => {
                    if map.next_value::<u16>()? != eml_core::product::BUILD_NUMBER {
                        return Err(de::Error::custom("Wrong product build"));
                    }
                }
                4 => {
                    if map.next_value::<u64>()? == 0 {
                        return Err(de::Error::custom("Zero run identity"));
                    }
                }
                5 => {
                    let raw = map.next_value::<&RawValue>()?;
                    if raw.get() != "null" {
                        let s: StartRecord = record(raw)?;
                        if s.protocol != 1
                            || s.program != eml_protocol::PROGRAM_ID
                            || s.total as usize > eml_core::MAX_COLLECTED_ITEMS
                        {
                            return Err(de::Error::custom("Invalid report start identity"));
                        }
                        total = Some(s.total);
                        collection_complete = Some(s.collection_complete);
                        start_version = Some(s.version);
                    }
                }
                6 => {
                    let facts = map.next_value_seed(Items)?;
                    highest = facts.0;
                    item_counts = facts.1;
                }
                7 => {
                    let raw = map.next_value::<&RawValue>()?;
                    if raw.get() != "null" {
                        let value = record::<SummaryRecord, M::Error>(raw)?;
                        if let Some(error) = value.run_error.value() {
                            crate::report::validate_error(error).map_err(de::Error::custom)?;
                        }
                        summary = Some(value);
                    }
                }
                8 => {
                    let raw = map.next_value::<&RawValue>()?;
                    if raw.get() != "null" && !raw.get().starts_with('"') {
                        return Err(de::Error::custom("Invalid protocol failure kind"));
                    }
                    protocol_failure =
                        serde_json::from_str(raw.get()).map_err(de::Error::custom)?;
                }
                9 => {
                    worker = Some(
                        validate_worker(map.next_value::<&RawValue>()?)
                            .map_err(de::Error::custom)?,
                    );
                }
                10 => {
                    let raw = map.next_value::<&RawValue>()?;
                    if !raw.get().starts_with('"') {
                        return Err(de::Error::custom("Invalid terminal kind"));
                    }
                    terminal = Some(
                        serde_json::from_str::<crate::RunMeaning>(raw.get())
                            .map_err(de::Error::custom)?,
                    );
                }
                _ => unreachable!(),
            }
        }
        if seen != 2047
            || start_version
                .as_deref()
                .is_some_and(|v| v != eml_core::product::VERSION)
            || highest.is_some_and(|i| total.is_none_or(|n| i >= n))
        {
            return Err(de::Error::custom(
                "Incomplete or inconsistent report identity",
            ));
        }
        if let Some(summary) = &summary {
            let c = &summary.counts;
            let Some(total) = total else {
                return Err(de::Error::custom("Summary without start"));
            };
            if [c.created, c.up_to_date, c.planned, c.failed] != item_counts
                || c.not_processed != total - item_counts.iter().sum::<u32>()
                || collection_complete != Some(summary.collection_complete)
            {
                return Err(de::Error::custom("Inconsistent report counts"));
            }
        }
        if terminal
            != Some(crate::lifecycle::classify_run(
                summary.as_ref(),
                protocol_failure,
                &worker.ok_or_else(|| de::Error::missing_field("worker"))?,
            ))
        {
            return Err(de::Error::custom("Contradictory terminal run meaning"));
        }
        Ok(())
    }
}
fn record<'a, T: Deserialize<'a>, E: de::Error>(raw: &'a RawValue) -> Result<T, E> {
    if raw.get().len() > eml_protocol::MAX_LINE_BYTES || !raw.get().starts_with('{') {
        return Err(E::custom("Report record limit or shape"));
    }
    strict::decode(raw.get()).map_err(E::custom)
}
struct Items;
impl<'de> de::DeserializeSeed<'de> for Items {
    type Value = (Option<u32>, [u32; 4]);
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
        struct Records;
        impl<'de> Visitor<'de> for Records {
            type Value = (Option<u32>, [u32; 4]);
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("bounded report items")
            }
            fn visit_seq<S: SeqAccess<'de>>(self, mut s: S) -> Result<Self::Value, S::Error> {
                let (mut count, mut previous) = (0usize, None);
                let mut counts = [0u32; 4];
                let mut bounds = crate::RunReport::new(eml_core::product::VERSION);
                while let Some(raw) = s.next_element::<&RawValue>()? {
                    count = count
                        .checked_add(1)
                        .ok_or_else(|| de::Error::custom("Item overflow"))?;
                    if count > eml_core::MAX_COLLECTED_ITEMS {
                        return Err(de::Error::custom("Item limit"));
                    }
                    let item: ItemRecord = record(raw)?;
                    bounds
                        .admit_item_metadata(&item)
                        .map_err(de::Error::custom)?;
                    if item.index as usize != count - 1
                        || item.removed.len() > eml_core::MAX_REMOVAL_ENTRIES
                    {
                        return Err(de::Error::custom("Invalid item order or bounds"));
                    }
                    counts[match item.status {
                        eml_protocol::records::ItemStatus::Created => 0,
                        eml_protocol::records::ItemStatus::UpToDate => 1,
                        eml_protocol::records::ItemStatus::Planned => 2,
                        eml_protocol::records::ItemStatus::Failed => 3,
                    }] += 1;
                    previous = Some(item.index);
                }
                Ok((previous, counts))
            }
        }
        d.deserialize_seq(Records)
    }
}
fn validate_worker(raw: &RawValue) -> Result<WorkerCompletion, &'static str> {
    if raw.get().len() > 1024 * 1024 || !raw.get().starts_with('{') {
        return Err("Worker lifecycle limit or shape");
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Worker<'a> {
        operation_id: eml_protocol::desktop::OperationId,
        exit_code: eml_protocol::records::Nullable<i32>,
        signal: eml_protocol::records::Nullable<i32>,
        input_may_have_been_sent: bool,
        settled: bool,
        reusable: bool,
        #[serde(borrow)]
        failure: eml_protocol::records::Nullable<&'a str>,
        #[serde(deserialize_with = "faults")]
        faults: Vec<String>,
        #[serde(deserialize_with = "stderr")]
        stderr: Vec<u8>,
        stderr_truncated: bool,
        interrupt_calls_succeeded: u8,
        first_interrupt_observed: bool,
    }
    let w: Worker<'_> = serde_json::from_str(raw.get()).map_err(|_| "Invalid worker lifecycle")?;
    if w.faults.len() > 32
        || w.faults.iter().any(|s| s.chars().count() > 1000)
        || w.stderr.len() > 65536
        || w.interrupt_calls_succeeded > 2
        || w.failure
            .value()
            .is_some_and(|s| !["launch", "identity", "transport", "unverified"].contains(s))
        || !w.settled
    {
        return Err("Worker lifecycle bounds or settlement");
    }
    let failure = match w.failure.value().copied() {
        None => None,
        Some("launch") => Some(crate::WorkerFailure::Launch),
        Some("identity") => Some(crate::WorkerFailure::Identity),
        Some("transport") => Some(crate::WorkerFailure::Transport),
        Some("unverified") => Some(crate::WorkerFailure::Unverified),
        _ => return Err("Unknown lifecycle failure"),
    };
    Ok(WorkerCompletion {
        operation_id: w.operation_id,
        exit_code: w.exit_code.value().copied(),
        signal: w.signal.value().copied(),
        input_may_have_been_sent: w.input_may_have_been_sent,
        settled: w.settled,
        reusable: w.reusable,
        failure,
        faults: w.faults,
        stderr: w.stderr,
        stderr_truncated: w.stderr_truncated,
        interrupt_calls_succeeded: w.interrupt_calls_succeeded,
        first_interrupt_observed: w.first_interrupt_observed,
    })
}

fn bounded<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>, const N: usize>(
    d: D,
) -> Result<Vec<T>, D::Error> {
    struct Values<T, const N: usize>(std::marker::PhantomData<T>);
    impl<'de, T: Deserialize<'de>, const N: usize> Visitor<'de> for Values<T, N> {
        type Value = Vec<T>;
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("a bounded lifecycle sequence")
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<Self::Value, A::Error> {
            let mut out = Vec::new();
            while out.len() < N {
                let Some(value) = a.next_element()? else {
                    return Ok(out);
                };
                out.push(value);
            }
            if a.next_element::<de::IgnoredAny>()?.is_some() {
                return Err(de::Error::custom("Lifecycle sequence bound"));
            }
            Ok(out)
        }
    }
    d.deserialize_seq(Values::<T, N>(std::marker::PhantomData))
}
fn faults<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    bounded::<D, String, 32>(d)
}
fn stderr<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
    bounded::<D, u8, 65536>(d)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn report_budget_accepts_equality_and_refuses_next_byte_without_truncation() {
        let mut writer = Budget {
            inner: io::sink(),
            used: 0,
            exceeded: false,
        };
        let bytes = vec![0; 1024 * 1024];
        for _ in 0..256 {
            writer.write_all(&bytes).unwrap();
        }
        assert_eq!(writer.used, eml_protocol::desktop::MAX_REPORT_BYTES);
        assert!(!writer.exceeded);
        assert!(writer.write_all(b"x").is_err());
        assert!(writer.exceeded);
        assert_eq!(writer.used, eml_protocol::desktop::MAX_REPORT_BYTES);
    }
}
