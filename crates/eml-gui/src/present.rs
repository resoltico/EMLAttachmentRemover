//! Stable native row identity and projection of accepted worker observations.
use crate::{
    state::{Batch, Kind, State},
    wording::{self as words},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use eml_core::{
    ErrorCode,
    vocabulary::{BatchTotals, Outcome, Severity, fields, format_bytes},
};
use eml_protocol::records::{ItemRecord, NativePathRecord, PathEncoding};
use std::{collections::HashMap, ffi::OsString, os::unix::ffi::OsStringExt, path::PathBuf};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RowStatus {
    Settled(Outcome),
    Waiting,
    Missing,
    StartFailed,
}
impl RowStatus {
    fn label(self) -> &'static str {
        match self {
            Self::Settled(o) => o.label(),
            Self::Waiting => words::WAITING,
            Self::Missing => words::MISSING,
            Self::StartFailed => words::START_FAILED,
        }
    }
    fn explanation(self) -> &'static str {
        match self {
            Self::Settled(o) => o.explanation(),
            Self::Waiting => words::WAITING_DETAIL,
            Self::Missing => words::MISSING_DETAIL,
            Self::StartFailed => words::LAUNCH_FAILED,
        }
    }
    pub(crate) fn severity(self) -> Severity {
        match self {
            Self::Settled(o) => o.severity(),
            Self::Waiting => Severity::Pending,
            Self::Missing => Severity::Neutral,
            Self::StartFailed => Severity::Failure,
        }
    }
    fn tone(self) -> crate::Tone {
        match self.severity() {
            Severity::Neutral => crate::Tone::Muted,
            Severity::Pending => crate::Tone::Accent,
            Severity::Success => crate::Tone::Ok,
            Severity::Attention => crate::Tone::Warn,
            Severity::Failure => crate::Tone::Bad,
        }
    }
}
pub(super) struct Row {
    pub key: PathBuf,
    pub item: ItemRecord,
    pub kind: Kind,
    pub status: RowStatus,
    pub copy: Option<PathBuf>,
}
impl Row {
    pub(super) fn label(&self) -> &'static str {
        self.status.label()
    }
    pub(super) fn problem(&self) -> bool {
        matches!(
            self.status.severity(),
            Severity::Attention | Severity::Failure
        )
    }
    fn explanation(&self) -> String {
        let mut lines = vec![self.status.explanation()];
        // A waiting/missing/unconfirmed replacement must not reuse old outcome claims.
        if self.status == RowStatus::Settled(crate::view_model::outcome(&self.item))
            && let Some(facts) = crate::view_model::facts(&self.item)
        {
            let meaning = eml_core::vocabulary::classify(&facts);
            lines.extend(meaning.consequences.iter().map(|c| c.text()));
            lines.extend(meaning.related_notice);
        }
        lines.join("\n")
    }
    pub(super) fn size_text(&self) -> String {
        match (self.item.source_bytes.value(), self.item.copy_bytes.value()) {
            (Some(a), Some(b)) if !self.item.removed.is_empty() => {
                format!("{} → {}", format_bytes(*a), format_bytes(*b))
            }
            (Some(a), _) => format_bytes(*a),
            _ => "—".into(),
        }
    }
    pub(super) fn details(&self) -> String {
        let mut text = format!(
            "{}: {}\n{}: {}\n{}: {}\n",
            fields::SOURCE,
            eml_protocol::TerminalPath::new(&self.key),
            fields::DESTINATION,
            self.item
                .destination
                .value()
                .and_then(|p| native(&p.native))
                .map(|p| eml_protocol::TerminalPath::new(&p).to_string())
                .unwrap_or_default(),
            fields::STATUS,
            self.label()
        );
        text += match self.kind {
            Kind::Check => words::CHECK_METADATA,
            Kind::Create => words::CREATE_METADATA,
        };
        text += &self.explanation();
        text.push('\n');
        if let Some(error) = self.item.error.value() {
            text += &format!(
                "{}: {}\n{}: {}\n{}: {}\n",
                fields::CODE,
                eml_protocol::DisplayText::new(&error.code),
                fields::PHASE,
                error.phase,
                fields::DETAIL,
                eml_protocol::DisplayText::new(&error.detail)
            );
        }
        if let Some(facts) = crate::view_model::facts(&self.item)
            && facts.cleanup.state != eml_core::CleanupState::NotNeeded
        {
            text += &format!(
                "{}: {}\n",
                fields::CLEANUP,
                eml_core::vocabulary::cleanup_label(facts.cleanup.state)
            );
            if let Some(code) = facts.cleanup.native_code {
                text += &format!("{}: {code}\n", fields::NATIVE_CODE);
            }
            if let Some(name) = facts.cleanup.residual_name {
                text += &format!(
                    "{}: {}\n",
                    fields::RESIDUAL_RESOURCE,
                    eml_protocol::TerminalPath::new(std::path::Path::new(&name))
                );
            }
        }
        if let Some(n) = self.item.source_bytes.value() {
            text += &format!("{}: {n}\n", fields::SOURCE_BYTES);
        }
        if let Some(n) = self.item.copy_bytes.value() {
            text += &format!("{}: {n}\n", fields::COPY_BYTES);
        }
        for part in &self.item.removed {
            text += &format!(
                "{}: {} ({}, {}, {})\n",
                eml_protocol::DisplayText::new(&part.section),
                words::attachment_name(part),
                eml_protocol::DisplayText::new(&part.content_type),
                format_bytes(part.bytes),
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
    let absent_create_status = batch.create.as_ref().map(|slot| {
        if batch.state() == State::Removing {
            return RowStatus::Waiting;
        }
        if batch.state() == State::Stopped {
            return RowStatus::Settled(Outcome::NotProcessed);
        }
        if slot.launch_error {
            return RowStatus::StartFailed;
        }
        let complete = slot
            .completion
            .as_ref()
            .filter(|c| c.failure.is_none())
            .and_then(|c| {
                c.report.with_report(|r| {
                    r.failure().is_none()
                        && r.summary().is_some_and(|summary| {
                            summary.collection_complete
                                && !summary.interrupted
                                && summary
                                    .run_error
                                    .value()
                                    .is_none_or(|error| error.code == "NO_INPUTS")
                        })
                })
            })
            .unwrap_or(false);
        if complete {
            RowStatus::Missing
        } else {
            RowStatus::Settled(Outcome::Unknown)
        }
    });
    let mut rows = HashMap::new();
    for (kind, handle, previous) in [
        (
            Kind::Check,
            batch.check.as_ref().and_then(|s| s.report.as_ref()),
            false,
        ),
        (Kind::Create, batch.previous_create.as_ref(), true),
        (
            Kind::Create,
            batch.create.as_ref().and_then(|s| s.report.as_ref()),
            false,
        ),
    ] {
        let Some(handle) = handle else {
            continue;
        };
        handle.with_report(|report| {
            for (index, item) in report.items().iter().enumerate() {
                let Some(key) = native(&item.source.native) else {
                    continue;
                };
                let outcome = crate::view_model::outcome(item);
                let status = if let Some(fallback) =
                    absent_create_status.filter(|_| kind == Kind::Check || previous)
                {
                    if batch.state() == State::Removing
                        && (outcome.severity() == Severity::Failure || previous)
                    {
                        RowStatus::Settled(outcome)
                    } else {
                        fallback
                    }
                } else {
                    RowStatus::Settled(outcome)
                };
                if previous && !matches!(batch.state(), State::Removing) {
                    continue;
                }
                rows.insert(
                    key.clone(),
                    Row {
                        key,
                        item: item.clone(),
                        kind,
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
        a.item
            .index
            .cmp(&b.item.index)
            .then_with(|| a.key.cmp(&b.key))
    });
    Some(result)
}
pub(super) fn banner(batch: &Batch) -> String {
    let Some(slot) = batch.create.as_ref().or(batch.check.as_ref()) else {
        return String::new();
    };
    if slot.launch_error {
        return words::LAUNCH_FAILED.into();
    }
    if slot
        .completion
        .as_ref()
        .is_some_and(|c| c.state == eml_app::WorkerState::RequestRefused)
    {
        return words::REQUEST_REFUSED.into();
    }
    slot.report
        .as_ref()
        .and_then(|handle| {
            handle.with_report(|r| {
                if let Some(error) = r.summary().and_then(|s| s.run_error.value()) {
                    return ErrorCode::from_wire_name(&error.code)
                        .map_or(words::WORKER_FAILED, |code| {
                            eml_core::vocabulary::RunFailure(code).explanation()
                        })
                        .to_owned();
                }
                if r.failure().is_some()
                    || slot
                        .completion
                        .as_ref()
                        .is_some_and(|c| c.failure.is_some())
                {
                    return words::WORKER_FAILED.into();
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
        if !rows.iter().any(Row::problem) {
            r.batch.filter(false);
        }
        if r.batch.selected.as_ref().is_some_and(|key| {
            !rows
                .iter()
                .any(|row| &row.key == key && (!r.batch.problems_only || row.problem()))
        }) {
            r.batch.select(None);
        }
        let progress = r
            .batch
            .slot(if r.batch.create.is_some() {
                Kind::Create
            } else {
                Kind::Check
            })
            .and_then(|s| s.report.as_ref())
            .map(|h| {
                h.with_report(|report| (report.items().len(), report.start().map(|s| s.total)))
            })
            .unwrap_or(Some((0, None)));
        let Some((done, total)) = progress else {
            return;
        };
        let totals = BatchTotals::from(rows.iter().map(|row| {
            (
                match row.status {
                    RowStatus::Settled(o) => Some(o),
                    _ => None,
                },
                row.item.removed.len(),
                row.item
                    .source_bytes
                    .value()
                    .copied()
                    .zip(row.item.copy_bytes.value().copied()),
            )
        }));
        let header = wording::header(
            &r.batch,
            &totals,
            total,
            rows.iter()
                .filter(|r| r.status == RowStatus::Missing)
                .count(),
        );
        let visible: Vec<_> = rows
            .iter()
            .filter(|row| !r.batch.problems_only || row.problem())
            .map(|row| {
                let display = std::path::Path::new(&row.item.source.display);
                let name = display
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| row.item.source.display.clone());
                let name = eml_protocol::DisplayText::new(&name).to_string();
                let (prefix, suffix) = name.rsplit_once('.').map_or_else(
                    || (name.clone(), String::new()),
                    |(a, b)| (a.to_owned(), format!(".{b}")),
                );
                let names: Vec<_> = row
                    .item
                    .removed
                    .iter()
                    .filter(|_| !(row.status.severity() == Severity::Failure))
                    .take(2)
                    .map(wording::attachment_name)
                    .collect();
                let folder = display.parent().map_or(String::new(), |p| {
                    p.strip_prefix(&self.home).map_or_else(
                        |_| p.to_string_lossy().into_owned(),
                        |relative| {
                            if relative.as_os_str().is_empty() {
                                "~".into()
                            } else {
                                format!("~/{}", relative.to_string_lossy())
                            }
                        },
                    )
                });
                let folder = eml_protocol::DisplayText::new(&folder).to_string();
                let size_text = row.size_text();
                let description = wording::row_description(
                    &folder,
                    (!(row.status.severity() == Severity::Failure))
                        .then_some(row.item.removed.len()),
                    &size_text,
                );
                EmailRow {
                    source_size: row
                        .item
                        .source_bytes
                        .value()
                        .map(|n| format_bytes(*n))
                        .unwrap_or_default()
                        .into(),
                    copy_size: row
                        .item
                        .copy_bytes
                        .value()
                        .map(|n| format_bytes(*n))
                        .unwrap_or_default()
                        .into(),
                    size_reduction_known: !row.item.removed.is_empty()
                        && row.item.source_bytes.value().is_some()
                        && row.item.copy_bytes.value().is_some(),
                    description: description.into(),
                    has_chips: !names.is_empty(),
                    key: row_key(&row.key).into(),
                    name: name.into(),
                    prefix: prefix.into(),
                    suffix: suffix.into(),
                    folder_prefix: folder
                        .rsplit_once('/')
                        .map_or("", |(p, _)| &folder[..p.len() + 1])
                        .into(),
                    folder_suffix: folder.rsplit('/').next().unwrap_or(&folder).into(),
                    folder: folder.into(),
                    chip1: names
                        .first()
                        .cloned()
                        .unwrap_or_else(|| {
                            if row.status.severity() == Severity::Failure {
                                "—"
                            } else {
                                wording::NO_ATTACHMENTS_CHIP
                            }
                            .into()
                        })
                        .into(),
                    chip2: names.get(1).cloned().unwrap_or_default().into(),
                    more: if !(row.status.severity() == Severity::Failure)
                        && row.item.removed.len() > 2
                    {
                        format!("+{}", row.item.removed.len() - 2)
                    } else {
                        String::new()
                    }
                    .into(),
                    size_text: size_text.into(),
                    status: row.label().into(),
                    tone: row.status.tone(),
                    selected: Some(&row.key) == r.batch.selected.as_ref(),
                }
            })
            .collect();
        // Reuse controls for unchanged keys and insert streamed rows in worker index order.
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
                name: eml_protocol::TerminalPath::new(Path::new(
                    row.key.file_name().unwrap_or_default(),
                ))
                .to_string()
                .into(),
                sentence: row.explanation().into(),
                path_prefix: {
                    let path = row.copy.as_ref().unwrap_or(&row.key);
                    path.parent().map_or(String::new(), |p| {
                        format!("{}/", eml_protocol::TerminalPath::new(p))
                    })
                }
                .into(),
                path_suffix: row
                    .copy
                    .as_ref()
                    .unwrap_or(&row.key)
                    .file_name()
                    .map(|n| eml_protocol::TerminalPath::new(Path::new(n)).to_string())
                    .unwrap_or_default()
                    .into(),
                path: eml_protocol::TerminalPath::new(row.copy.as_ref().unwrap_or(&row.key))
                    .to_string()
                    .into(),
                parts: Rc::new(VecModel::from(
                    row.item
                        .removed
                        .iter()
                        .map(wording::attachment)
                        .collect::<Vec<_>>(),
                ))
                .into(),
                can_show_copy: row.copy.is_some(),
            })
            .unwrap_or_default();
        let state = r.batch.state();
        let stopping = r.active.as_ref().is_some_and(|a| a.stop_at.is_some());
        let estimate = r
            .active
            .as_ref()
            .and_then(|a| a.estimate.remaining(now_ms(), total));
        let mut view = BatchView {
            selected_index: visible
                .iter()
                .position(|row| row.selected)
                .map_or(-1, |i| i as i32),
            state: state.into(),
            title: header.title.into(),
            summary: header.summary.into(),
            problems: header.problems.into(),
            banner: banner(&r.batch).into(),
            diagnostics: r
                .batch
                .create
                .as_ref()
                .or(r.batch.check.as_ref())
                .and_then(|slot| slot.completion.as_ref())
                .map(|completion| {
                    let mut text = String::from_utf8_lossy(&completion.stderr)
                        .lines()
                        .map(|line| eml_protocol::DisplayText::new(line).to_string())
                        .collect::<Vec<_>>()
                        .join("\n");
                    if completion.stderr_truncated {
                        text.push_str(words::DIAGNOSTICS_TRUNCATED);
                    }
                    text
                })
                .unwrap_or_default()
                .into(),
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
            progress: if total.is_none_or(|n| n == 0) {
                0.0
            } else {
                done as f32 / total.unwrap() as f32
            },
            progress_text: total
                .map_or_else(
                    || wording::STARTING.into(),
                    |n| wording::progress(done, n, estimate),
                )
                .into(),
            filtered: r.batch.problems_only,
            quit: r.quit,
            quit_committed: r.quitting,
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
        let revision = r.batch.revision();
        drop(r);
        *self.rows.borrow_mut() = rows;
        view.can_add = self.inputs_enabled();
        view.can_start = self.can_start();
        view.can_stop = self.can_stop();
        view.can_reset = self.can_reset();
        view.can_export = self.can_save_report();
        view.can_output = self.can_show_output();
        view.can_filter = self.can_filter();
        view.can_reveal_report = self.can_show_report();
        view.can_copy = self.can_copy_details();
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
        self.rendered_revision.set(revision);
        self.presentation_dirty.set(false);
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
        assert_eq!(format_bytes(1_500_000), "1.5 MB");
        assert_eq!(RowStatus::Missing.label(), "No longer found");
    }
}
