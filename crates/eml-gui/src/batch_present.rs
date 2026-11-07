//! Stable native row identity and projection of accepted worker observations.
use crate::{
    batch::{Batch, Kind, State},
    wording::batch::{self as words, Status},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use eml_core::{ErrorCode, vocabulary::MeaningKind};
use eml_protocol::records::{ItemRecord, NativePathRecord, PathEncoding};
use std::{collections::HashMap, ffi::OsString, os::unix::ffi::OsStringExt, path::PathBuf};
pub(super) struct Row {
    pub key: PathBuf,
    pub item: ItemRecord,
    pub status: Status,
    pub copy: Option<PathBuf>,
}
impl Row {
    pub(super) fn code(&self) -> Option<ErrorCode> {
        self.item
            .error
            .value()
            .and_then(|e| ErrorCode::from_wire_name(&e.code))
    }
    pub(super) fn label(&self) -> &'static str {
        words::label(self.status, self.code())
    }
    pub(super) fn problem(&self) -> bool {
        words::tone(self.status) >= 3
    }
    pub(super) fn size_text(&self) -> String {
        match (self.item.source_bytes.value(), self.item.copy_bytes.value()) {
            (Some(a), Some(b)) if !self.item.removed.is_empty() => {
                format!("{} → {}", words::size(*a), words::size(*b))
            }
            (Some(a), _) => words::size(*a),
            _ => "—".into(),
        }
    }
    pub(super) fn details(&self) -> String {
        let mut text = format!(
            "Source: {}\nDestination: {}\nStatus: {}\n",
            eml_protocol::TerminalPath::new(&self.key),
            self.item
                .destination
                .value()
                .and_then(|p| native(&p.native))
                .map(|p| eml_protocol::TerminalPath::new(&p).to_string())
                .unwrap_or_default(),
            self.label()
        );
        if let Some(error) = self.item.error.value() {
            text += &format!(
                "Code: {}\nPhase: {:?}\nDetail: {}\n",
                error.code, error.phase, error.detail
            );
        }
        if let Some(n) = self.item.source_bytes.value() {
            text += &format!("Source bytes: {n}\n");
        }
        if let Some(n) = self.item.copy_bytes.value() {
            text += &format!("Copy bytes: {n}\n");
        }
        for part in &self.item.removed {
            text += &format!(
                "{}: {} ({}, {} bytes, {:?})\n",
                part.section,
                part.filename.value().unwrap_or(&part.content_type),
                part.content_type,
                part.bytes,
                part.reason
            );
        }
        text
    }
}
pub(super) fn native(path: &NativePathRecord) -> Option<PathBuf> {
    (path.encoding == PathEncoding::UnixBytes)
        .then(|| {
            STANDARD
                .decode(&path.base64)
                .ok()
                .map(|bytes| PathBuf::from(OsString::from_vec(bytes)))
        })
        .flatten()
}
pub(super) fn rows(batch: &Batch) -> Vec<Row> {
    let mut rows = HashMap::new();
    for kind in [Kind::Check, Kind::Create] {
        let Some(slot) = batch.slot(kind) else {
            continue;
        };
        let Some(handle) = &slot.report else {
            continue;
        };
        handle.with_report(|report| {
            for (index, item) in report.items().iter().enumerate() {
                let Some(key) = native(&item.source.native) else {
                    continue;
                };
                let status = if kind == Kind::Check && batch.create.is_some() {
                    if batch.state() == State::Removing {
                        Status::Waiting
                    } else if batch.state() == State::Stopped {
                        Status::Stopped
                    } else {
                        Status::Missing
                    }
                } else {
                    words::status(
                        crate::view_model::meaning(item)
                            .map_or(MeaningKind::Unestablished, |m| m.kind),
                        item.error
                            .value()
                            .and_then(|e| ErrorCode::from_wire_name(&e.code)),
                        item.removed.len(),
                    )
                };
                rows.insert(
                    key.clone(),
                    Row {
                        key,
                        item: item.clone(),
                        status,
                        copy: (kind == Kind::Create)
                            .then(|| report.reveal_path(index))
                            .flatten(),
                    },
                );
            }
        });
    }
    let mut result: Vec<Row> = rows.into_values().collect();
    result.sort_by(|a, b| {
        let a_folder = std::path::Path::new(&a.item.source.display)
            .parent()
            .unwrap_or(std::path::Path::new(""));
        let b_folder = std::path::Path::new(&b.item.source.display)
            .parent()
            .unwrap_or(std::path::Path::new(""));
        crate::state::natural_cmp(a_folder.as_os_str(), b_folder.as_os_str())
            .then_with(|| {
                crate::state::natural_cmp(
                    std::path::Path::new(&a.item.source.display)
                        .file_name()
                        .unwrap_or_default(),
                    std::path::Path::new(&b.item.source.display)
                        .file_name()
                        .unwrap_or_default(),
                )
            })
            .then_with(|| a.key.cmp(&b.key))
    });
    result
}
pub(super) struct Header {
    pub title: String,
    pub summary: String,
    pub problems: String,
}
pub(super) fn header(batch: &Batch, rows: &[Row]) -> Header {
    let count = |s| rows.iter().filter(|row| row.status == s).count();
    let problems = rows.iter().filter(|row| row.problem()).count();
    let has_warn = rows.iter().any(|row| words::tone(row.status) == 3);
    let pill = if problems == 0 {
        String::new()
    } else if has_warn {
        format!("{} attention", words::plural(problems, "needs", "need"))
    } else {
        format!("{problems} can’t be copied")
    };
    let saved = count(Status::Saved) + count(Status::AsIs);
    let total = batch
        .slot(if batch.create.is_some() {
            Kind::Create
        } else {
            Kind::Check
        })
        .and_then(|slot| slot.report.as_ref())
        .and_then(|handle| handle.with_report(|r| r.start().map(|s| s.total)))
        .flatten();
    let attachments: usize = rows
        .iter()
        .filter(|row| matches!(row.status, Status::Ready | Status::Saved))
        .map(|row| row.item.removed.len())
        .sum();
    let removed: u64 = rows
        .iter()
        .filter(|row| matches!(row.status, Status::Ready | Status::Saved))
        .filter_map(|row| {
            Some(
                row.item
                    .source_bytes
                    .value()?
                    .saturating_sub(*row.item.copy_bytes.value()?),
            )
        })
        .sum();
    let (title, parts) = match batch.state() {
        State::Empty => (String::new(), vec![]),
        State::Checking => (
            if total.is_none() {
                "Looking for emails…"
            } else {
                "Checking emails…"
            }
            .into(),
            total
                .map(|n| {
                    vec![
                        format!("{} of {n} checked", rows.len()),
                        "nothing is written yet".into(),
                    ]
                })
                .unwrap_or_default(),
        ),
        State::Ready => (
            words::plural(rows.len(), "email", "emails"),
            if attachments == 0 {
                vec!["No attachments to remove".into()]
            } else {
                vec![
                    words::plural(attachments, "attachment", "attachments"),
                    format!("{} will be removed", words::size(removed)),
                ]
            },
        ),
        State::Removing => (
            "Removing attachments…".into(),
            vec![
                format!(
                    "{} of {} done",
                    rows.iter().filter(|r| r.status != Status::Waiting).count(),
                    total.unwrap_or(0)
                ),
                "your originals are not changed".into(),
            ],
        ),
        State::Stopped => (
            "Stopped".into(),
            vec![
                words::plural(saved, "copy saved", "copies saved"),
                format!("{} not processed", count(Status::Stopped)),
            ],
        ),
        State::Done => (
            if saved == 0 && problems > 0 {
                "No copies saved".into()
            } else {
                words::plural(saved, "copy saved", "copies saved")
            },
            {
                let mut parts = Vec::new();
                if removed > 0 {
                    parts.push(format!("{} removed", words::size(removed)));
                }
                for (n, phrase) in [
                    (count(Status::AsIs), "copied as is"),
                    (count(Status::Existing), "already existed"),
                    (count(Status::Missing), "no longer found"),
                ] {
                    if n > 0 {
                        parts.push(format!("{n} {phrase}"));
                    }
                }
                parts
            },
        ),
    };
    Header {
        title,
        summary: parts.join(" · "),
        problems: pill,
    }
}
pub(super) fn banner(batch: &Batch) -> String {
    let Some(slot) = batch.create.as_ref().or(batch.check.as_ref()) else {
        return String::new();
    };
    if slot.launch_error {
        return words::banner("launch").into();
    }
    slot.report
        .as_ref()
        .and_then(|handle| {
            handle.with_report(|r| {
                if let Some(error) = r.summary().and_then(|s| s.run_error.value()) {
                    return words::banner(&error.code).to_owned();
                }
                if r.failure().is_some()
                    || slot
                        .completion
                        .as_ref()
                        .is_some_and(|c| c.failure.is_some())
                {
                    return words::banner("worker").into();
                }
                String::new()
            })
        })
        .unwrap_or_default()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_paths_are_independent_of_display_and_decimal_sizes_are_used() {
        let path = NativePathRecord {
            encoding: PathEncoding::UnixBytes,
            base64: STANDARD.encode(b"/native/\xff.eml"),
        };
        assert_eq!(
            native(&path).unwrap().as_os_str().as_encoded_bytes(),
            b"/native/\xff.eml"
        );
        assert_eq!(words::size(1_500_000), "1.5 MB");
        assert_eq!(words::label(Status::Missing, None), "No longer found");
    }
}
