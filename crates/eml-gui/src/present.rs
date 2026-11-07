//! Stable native row identity and projection of accepted worker observations.
use crate::{
    state::{Batch, Kind, State},
    wording::{self as words, Status},
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
pub(super) fn rows(batch: &Batch) -> Option<Vec<Row>> {
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
        })?;
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
    Some(result)
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
impl crate::DesktopController {
    pub(super) fn refresh(&self) {
        use crate::*;
        let mut r = self.runtime.borrow_mut();
        let Some(rows) = rows(&r.batch) else {
            return;
        };
        if r.batch.selected.as_ref().is_some_and(|key| {
            !rows
                .iter()
                .any(|row| &row.key == key && (!r.batch.problems_only || row.problem()))
        }) {
            r.batch.selected = None;
        }
        let header = wording::header(&r.batch, &rows);
        let visible: Vec<_> = rows
            .iter()
            .filter(|row| !r.batch.problems_only || row.problem())
            .map(|row| {
                let display = std::path::Path::new(&row.item.source.display);
                let name = display
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| row.item.source.display.clone());
                let (prefix, suffix) = name.rsplit_once('.').map_or_else(
                    || (name.clone(), String::new()),
                    |(a, b)| (a.to_owned(), format!(".{b}")),
                );
                let names: Vec<_> = row
                    .item
                    .removed
                    .iter()
                    .take(2)
                    .map(|p| {
                        eml_protocol::DisplayText::new(
                            p.filename.value().unwrap_or(&p.content_type),
                        )
                        .to_string()
                    })
                    .collect();
                EmailRow {
                    key: row_key(&row.key).into(),
                    name: name.into(),
                    prefix: prefix.into(),
                    suffix: suffix.into(),
                    folder: display
                        .parent()
                        .map_or(String::new(), |p| p.to_string_lossy().into_owned())
                        .into(),
                    chip1: names
                        .first()
                        .cloned()
                        .unwrap_or_else(|| {
                            if row.status == wording::Status::Failed {
                                "—"
                            } else {
                                "None"
                            }
                            .into()
                        })
                        .into(),
                    chip2: names.get(1).cloned().unwrap_or_default().into(),
                    more: if row.item.removed.len() > 2 {
                        format!("+{}", row.item.removed.len() - 2)
                    } else {
                        String::new()
                    }
                    .into(),
                    size_text: row.size_text().into(),
                    status: row.label().into(),
                    tone: wording::tone(row.status),
                    selected: Some(&row.key) == r.batch.selected.as_ref(),
                }
            })
            .collect();
        // Reuse controls for unchanged keys and insert streamed rows in natural order.
        for (index, row) in visible.iter().enumerate() {
            if self
                .model
                .row_data(index)
                .is_some_and(|old| old.key == row.key)
            {
                if self.model.row_data(index).as_ref() != Some(row) {
                    self.model.set_row_data(index, row.clone());
                }
            } else {
                self.model.insert(index, row.clone());
            }
        }
        while self.model.row_count() > visible.len() {
            self.model.remove(visible.len());
        }
        let detail = rows
            .iter()
            .find(|row| Some(&row.key) == r.batch.selected.as_ref())
            .map(|row| EmailDetail {
                visible: true,
                name: row
                    .key
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
                    .into(),
                sentence: wording::detail(row.status, row.code()).into(),
                path: eml_protocol::TerminalPath::new(row.copy.as_ref().unwrap_or(&row.key))
                    .to_string()
                    .into(),
                parts: strings(row.item.removed.iter().map(wording::attachment)),
                can_show_copy: row.copy.is_some(),
            })
            .unwrap_or_default();
        let state = r.batch.state();
        let (done, total) = r
            .batch
            .slot(if r.batch.create.is_some() {
                Kind::Create
            } else {
                Kind::Check
            })
            .and_then(|s| s.report.as_ref())
            .and_then(|h| {
                h.with_report(|report| {
                    (report.items().len(), report.start().map_or(0, |s| s.total))
                })
            })
            .unwrap_or((0, 0));
        let stopping = r.active.as_ref().is_some_and(|a| a.stop_at.is_some());
        let estimate = r
            .active
            .as_ref()
            .and_then(|a| a.estimate.remaining(now_ms(), Some(total)));
        let mut view = BatchView {
            state: match state {
                State::Empty => 0,
                State::Checking => 1,
                State::Ready => 2,
                State::Removing => 3,
                State::Stopped => 4,
                State::Done => 5,
            },
            title: header.title.into(),
            summary: header.summary.into(),
            problems: header.problems.into(),
            banner: banner(&r.batch).into(),
            notice: self
                .notice
                .borrow()
                .as_ref()
                .filter(|(_, at)| now_ms().saturating_sub(*at) < 6000)
                .map_or(String::new(), |(text, _)| text.clone())
                .into(),
            primary: wording::primary(state).into(),
            footer: wording::footer(state).into(),
            destination: r
                .batch
                .destination
                .as_ref()
                .map_or_else(
                    || wording::NEXT_TO.into(),
                    |p| {
                        p.file_name()
                            .unwrap_or(p.as_os_str())
                            .to_string_lossy()
                            .into_owned()
                    },
                )
                .into(),
            progress: if total == 0 {
                0.0
            } else {
                done as f32 / total as f32
            },
            progress_text: wording::progress(done, total, estimate).into(),
            filtered: r.batch.problems_only,
            quit: r.quit,
            export_running: r.export.is_some(),
            stopping,
            ..Default::default()
        };
        let recent = r
            .preferences
            .recent_destinations()
            .iter()
            .map(|p| eml_protocol::TerminalPath::new(p).to_string())
            .collect::<Vec<_>>();
        drop(r);
        *self.rows.borrow_mut() = rows;
        view.can_add = self.inputs_enabled();
        view.can_start = self.can_start();
        view.can_stop = self.can_stop();
        view.can_reset = self.can_reset();
        view.can_export = self.can_save_report();
        view.can_output = self.can_show_output();
        view.can_filter = !self.locked() && !view.problems.is_empty();
        view.can_copy = !self.locked() && detail.visible;
        view.quit_now = self.can_quit_now();
        self.ui.set_view(view);
        self.ui.set_detail(detail);
        self.ui.set_recent_destinations(strings(recent));
        self.ui.set_output_roots(strings(
            self.output_roots()
                .iter()
                .map(|p| eml_protocol::TerminalPath::new(p).to_string()),
        ));
        if !self.inputs_enabled() {
            self.ui.set_drop_hovered(false);
        }
    }
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
