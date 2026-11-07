//! One-list product language; protocol diagnostics remain verbatim in exports.
use crate::{
    present::Row,
    state::{Batch, Kind, State},
};
use eml_core::{ErrorCode, vocabulary::MeaningKind};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Status {
    Ready,
    None,
    Comparison,
    Saved,
    AsIs,
    Existing,
    NeedsChecking,
    Unknown,
    Failed,
    Waiting,
    Stopped,
    Missing,
    StartFailed,
}
pub(crate) fn status(kind: MeaningKind, code: Option<ErrorCode>, removed: usize) -> Status {
    use MeaningKind::*;
    match kind {
        Prepared => {
            if removed > 0 {
                Status::Ready
            } else {
                Status::None
            }
        }
        Comparison => Status::Comparison,
        Created => {
            if removed > 0 {
                Status::Saved
            } else {
                Status::AsIs
            }
        }
        ExistingVerified => Status::Existing,
        PublishedIncomplete | ExistingMatched => Status::NeedsChecking,
        PublicationUncertain | Unestablished => Status::Unknown,
        NotPublished if code == Some(ErrorCode::Interrupted) => Status::Stopped,
        NotPublished => Status::Failed,
    }
}
pub(crate) fn label(status: Status, code: Option<ErrorCode>) -> &'static str {
    match status {
        Status::Ready => "Ready",
        Status::None => "No attachments",
        Status::Comparison => "Copy exists, will verify",
        Status::Saved => "Copy saved",
        Status::AsIs => "Copied as is",
        Status::Existing => "Copy already exists",
        Status::NeedsChecking => "Saved, needs checking",
        Status::Unknown => "Result unknown",
        Status::Waiting => "Waiting",
        Status::Stopped => "Not processed",
        Status::Missing => "No longer found",
        Status::Failed => failure(code).0,
        Status::StartFailed => failure(None).0,
    }
}
pub(crate) fn tone(status: Status) -> i32 {
    match status {
        Status::Ready => 1,
        Status::Saved | Status::AsIs | Status::Existing => 2,
        Status::NeedsChecking | Status::Unknown => 3,
        Status::Failed | Status::StartFailed => 4,
        _ => 0,
    }
}
pub(crate) fn detail(status: Status, code: Option<ErrorCode>) -> &'static str {
    match status {
        Status::Ready => "These will be removed from the copy:",
        Status::None | Status::AsIs => "Nothing to remove. The copy keeps the whole message.",
        Status::Comparison => {
            "A copy with this name already exists. It will be checked, never replaced."
        }
        Status::Saved => "Copy saved. Removed:",
        Status::Existing => "An identical copy already exists. Nothing was written.",
        Status::Waiting => "Waiting to be copied.",
        Status::Stopped => "Not copied because the batch was stopped. Continue to finish it.",
        Status::Missing => "This file was no longer there when the copies were made.",
        Status::Unknown => {
            "The app couldn’t confirm whether a copy was written. Check the save folder, then try again. Existing copies are never replaced."
        }
        Status::NeedsChecking => {
            "The copy was written, but the final checks didn’t complete. Check the save folder, then use Continue or try again. Existing copies are never replaced."
        }
        Status::Failed => failure(code).1,
        Status::StartFailed => banner("launch"),
    }
}
fn failure(code: Option<ErrorCode>) -> (&'static str, &'static str) {
    use ErrorCode::*;
    match code {
        Some(Malformed) => (
            "Not a valid email",
            "This file isn’t a valid email, so no copy can be made. Export it again from your mail app and add the new file.",
        ),
        Some(NoBody) => (
            "No text to keep",
            "This email has no plain-text or HTML body, so a copy would be empty.",
        ),
        Some(Protected) => (
            "Protected content",
            "This email contains signed or encrypted content that can’t be changed safely.",
        ),
        Some(UnsupportedMime) => (
            "Unsupported format",
            "This email uses a structure the app doesn’t support.",
        ),
        Some(LimitExceeded) => (
            "Too large or complex",
            "This email is larger or more deeply nested than the app supports (256 MB maximum).",
        ),
        Some(ReadFailed | BindFailed) => (
            "Can’t be read",
            "The file couldn’t be read. Check that it still exists and that you have permission to open it.",
        ),
        Some(SourceNotRegular) => (
            "Not a file",
            "This isn’t a regular file. Add the email file itself.",
        ),
        Some(SourceChanged) => (
            "Changed while reading",
            "The file changed while it was being read. Try again when nothing else is using it.",
        ),
        Some(DestinationExists) => (
            "Name already taken",
            "A different file already has the copy’s name. Move it or choose another save folder.",
        ),
        Some(DestinationCollision) => (
            "Duplicate copy name",
            "Two emails would get the same copy name. Choose a save folder, or keep copies next to the originals.",
        ),
        Some(SourceDestinationAlias) => (
            "Would overwrite original",
            "The copy would replace an original, so it was skipped.",
        ),
        Some(DestinationUnsupported) => (
            "Folder not supported",
            "The save folder doesn’t support safe copying. Choose another folder.",
        ),
        Some(StageFailed | WriteFailed) => (
            "Couldn’t save copy",
            "The copy couldn’t be written. Check free space and permissions in the save folder.",
        ),
        Some(Internal) => (
            "Couldn’t copy",
            "An internal check failed, so no copy was made. Save Report… and send it with a bug report.",
        ),
        _ => (
            "Couldn’t copy",
            "No copy was made. Use Copy Details for more information.",
        ),
    }
}
pub(crate) fn size(bytes: u64) -> String {
    if bytes < 1000 {
        return format!("{bytes} B");
    }
    for (scale, unit) in [(1_000_000_000, "GB"), (1_000_000, "MB"), (1000, "kB")] {
        if bytes >= scale {
            return format!("{:.1} {unit}", bytes as f64 / scale as f64);
        }
    }
    unreachable!()
}
pub(crate) fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}
pub(crate) fn banner(code: &str) -> &'static str {
    match code {
        "NO_INPUTS" => "No .eml files were found in what you added. Add other files or folders.",
        "INPUT_LIMIT_EXCEEDED" => {
            "Too many files. Up to 4,096 emails can be processed at once. Add fewer folders."
        }
        "OUTPUT_OVERLAP" => {
            "The save folder is inside a folder you added. Choose a different save folder."
        }
        "INPUT_ADMISSION_FAILED" => {
            "Some items you added can’t be opened. Check that they still exist, then Start Over."
        }
        "OUTPUT_FAILED" => {
            "The copy process stopped while reporting results. Save Report… and try again."
        }
        "launch" => "The copy process couldn’t start. Reinstall the app if this keeps happening.",
        _ => {
            "The copy process stopped unexpectedly. Results shown may be incomplete. Save Report… for details."
        }
    }
}

pub(super) struct Header {
    pub title: String,
    pub summary: String,
    pub problems: String,
}
pub(super) fn header(batch: &Batch, rows: &[Row]) -> Header {
    let count = |s| rows.iter().filter(|row| row.status == s).count();
    let problems = rows.iter().filter(|row| row.problem()).count();
    let has_warn = rows.iter().any(|row| tone(row.status) == 3);
    let pill = if problems == 0 {
        String::new()
    } else if has_warn {
        format!("{} attention", plural(problems, "needs", "need"))
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
        State::Ready => {
            let mut parts = if attachments == 0 {
                vec!["No attachments to remove".into()]
            } else {
                vec![
                    plural(attachments, "attachment", "attachments"),
                    format!("{} will be removed", size(removed)),
                ]
            };
            let existing = count(Status::Comparison);
            if existing > 0 {
                parts.push(format!("{existing} already copied"));
            }
            (plural(rows.len(), "email", "emails"), parts)
        }
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
                plural(saved, "copy saved", "copies saved"),
                format!("{} not processed", count(Status::Stopped)),
            ],
        ),
        State::Done => (
            if saved == 0 && problems > 0 {
                "No copies saved".into()
            } else {
                plural(saved, "copy saved", "copies saved")
            },
            {
                let mut parts = Vec::new();
                if removed > 0 {
                    parts.push(format!("{} removed", size(removed)));
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

pub(super) fn estimate(seconds: Option<u64>) -> String {
    match seconds {
        None => "Estimating time…".into(),
        Some(0..=59) => "Less than a minute".into(),
        Some(seconds) => format!(
            "About {} {}",
            seconds.div_ceil(60),
            if seconds.div_ceil(60) == 1 {
                "minute"
            } else {
                "minutes"
            }
        ),
    }
}

pub(super) fn controls() -> crate::AppWords {
    crate::AppWords {
        show_report: "Show".into(), show_saved_report: "Show Saved Report".into(),
    add_files: "Add Files…".into(),
    add_folder: "Add Folder…".into(),
    start_over: "Start Over".into(),
    empty_title: "Drop emails or folders here".into(),
    empty_body: "Each .eml file gets a copy without its attachments. Message text is kept and your originals are never changed.".into(),
    email: "Email".into(),
    emails: "Emails".into(),
    details_label: "Email details".into(),
    attachments: "Attachments".into(),
    size: "Size".into(),
    status: "Status".into(),
    show_copy: "Show Copy".into(),
    show_original: "Show Original".into(),
    copy_details: "Copy Details".into(),
    save_copies: "Save copies".into(),
    next_to: "Next to originals".into(),
    choose_folder: "Choose Folder…".into(),
    destination_hint: "as name.no-attachments.eml · never overwrites".into(),
    stop: "Stop".into(),
    stopping: "Stopping…".into(),
    show_output: crate::platform::file_manager_label().into(),
    file: "File".into(),
    edit: "Edit".into(),
    batch: "Batch".into(),
    window: "Window".into(),
    help: "Help".into(),
    save_report: "Save Report…".into(),
    close: "Close Window".into(),
    copy: "Copy".into(),
    problems_only: "Show Problems Only".into(),
    minimize: "Minimize".into(),
    website: "Project Website".into(),
    licenses: "Licenses".into(),
    about: "About".into(),
    quit_title: "Stop and quit?".into(),
    quit_body: "Copies already saved are kept. Your originals are not changed.".into(),
    export_body: "A report is still being saved. The app quits when it finishes.".into(),
    cancel: "Cancel".into(),
    stop_quit: "Stop and Quit".into(),
    quit_now: "Quit Now".into(),
}
}

pub(super) const BUSY_ADDING: &str = "Wait for this batch to finish before adding more emails.";
pub(super) const DESTINATION_UNAVAILABLE: &str = "Choose an existing folder to save copies.";
pub(super) const NEXT_TO: &str = "Next to originals";
pub(super) const ABSOLUTE_REPORT: &str = "Choose an absolute report path.";
pub(super) const REPORT_SAVED: &str = "Report saved.";
pub(super) const EXPORT_FAILED: &str = "The report writer did not complete.";
pub(super) fn report_failed(reason: &str) -> String {
    format!("Couldn’t save the report. {reason}")
}
pub(super) fn primary(state: State) -> &'static str {
    if state == State::Stopped {
        "Continue"
    } else {
        "Remove Attachments"
    }
}
pub(super) fn footer(state: State) -> &'static str {
    match state {
        State::Stopped => "Continue skips the copies that already exist.",
        State::Done => "Your originals are unchanged. Drop more emails to start a new batch.",
        _ => "",
    }
}
pub(super) fn progress(done: usize, total: u32, remaining: Option<u64>) -> String {
    format!("{done} of {total} done · {}", estimate(remaining))
}
pub(super) fn attachment(part: &eml_protocol::records::RemovedRecord) -> crate::AttachmentView {
    use eml_protocol::records::Removal;
    let kind = match part.reason {
        Removal::RelatedResource
            if part
                .content_type
                .get(..6)
                .is_some_and(|kind| kind.eq_ignore_ascii_case("image/")) =>
        {
            "Inline image"
        }
        Removal::RelatedResource => "Inline resource",
        Removal::NonBodyPart if part.content_type.eq_ignore_ascii_case("text/calendar") => {
            "Calendar invite"
        }
        Removal::NonBodyPart => "Other content",
        Removal::Attachment => "Attachment",
    };
    crate::AttachmentView {
        name: attachment_name(part).into(),
        kind: kind.into(),
        size: size(part.bytes).into(),
    }
}

pub(super) const NOTHING_ADDED: &str = "Nothing was added.";
pub(super) const NO_ATTACHMENTS_CHIP: &str = "None";

pub(super) fn request_error(_: &eml_app::RequestError) -> String {
    "Too many items at once. Add fewer files or folders.".into()
}

pub(super) fn row_description(folder: &str, count: Option<usize>, size: &str) -> String {
    format!(
        "{folder}; {}; {size}",
        count.map_or_else(
            || "attachments unknown".into(),
            |n| plural(n, "attachment", "attachments")
        )
    )
}

pub(crate) fn is_failed(status: Status) -> bool {
    matches!(status, Status::Failed | Status::StartFailed)
}

#[cfg(any(feature = "desktop", test))]
pub(super) const OPENING: &str = "Opening…";
pub(super) const OPEN_SENT: &str = "Open command sent.";
pub(super) const OPEN_FAILED: &str = "The native open operation did not complete.";

pub(super) fn attachment_name(part: &eml_protocol::records::RemovedRecord) -> String {
    eml_protocol::DisplayText::new(
        part.filename
            .value()
            .filter(|name| !name.is_empty())
            .unwrap_or(&part.content_type),
    )
    .to_string()
}
#[cfg(feature = "desktop")]
pub(super) const EML_FILTER: &str = "EML messages";
#[cfg(feature = "desktop")]
pub(super) const REPORT_FILTER: &str = "JSON Lines report";
#[cfg(feature = "desktop")]
pub(super) const SHOW_EMAIL_FAILED: &str = "Couldn’t show the email. Check that it still exists.";
#[cfg(feature = "desktop")]
pub(super) const SHOW_OUTPUT_FAILED: &str = "Couldn’t open the save folder.";
#[cfg(feature = "desktop")]
pub(super) const SHOW_REPORT_FAILED: &str = "Couldn’t show the saved report.";
#[cfg(feature = "desktop")]
pub(super) const WEBSITE_FAILED: &str = "Couldn’t open the project website.";
#[cfg(feature = "desktop")]
pub(super) const LICENSES_FAILED: &str = "Couldn’t open bundled third-party notices.";
