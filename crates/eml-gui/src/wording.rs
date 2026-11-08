//! One-list product language; protocol diagnostics remain verbatim in exports.
use crate::state::{Batch, State};
use eml_core::vocabulary::{BatchTotals, Outcome, Severity, format_bytes};
pub(crate) fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}
pub(super) struct Header {
    pub title: String,
    pub summary: String,
    pub problems: String,
}
pub(super) fn header(
    batch: &Batch,
    totals: &BatchTotals,
    total: Option<u32>,
    missing: usize,
) -> Header {
    let count = |outcome| totals.count(outcome);
    let problems = totals.problems;
    let has_warn = totals
        .counts
        .keys()
        .any(|outcome| outcome.severity() == Severity::Attention);
    let pill = if problems == 0 {
        String::new()
    } else if has_warn {
        format!("{} attention", plural(problems, "needs", "need"))
    } else {
        format!("{problems} can’t be copied")
    };
    let saved = count(Outcome::CopySaved) + count(Outcome::CopiedAsIs);
    let attachments = totals.attachments;
    let removed = totals.bytes_removed;
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
                        format!("{} of {n} checked", totals.emails),
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
                    format!("{} will be removed", format_bytes(removed)),
                ]
            };
            let existing = count(Outcome::WillVerify);
            if existing > 0 {
                parts.push(format!("{existing} already copied"));
            }
            (plural(totals.emails, "email", "emails"), parts)
        }
        State::Removing => (
            "Removing attachments…".into(),
            vec!["Your originals are not changed".into()],
        ),
        State::Stopped => (
            "Stopped".into(),
            vec![
                plural(saved, "copy saved", "copies saved"),
                format!("{} not processed", count(Outcome::NotProcessed)),
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
                    parts.push(format!("{} removed", format_bytes(removed)));
                }
                for (n, phrase) in [
                    (count(Outcome::CopiedAsIs), "copied as is"),
                    (count(Outcome::AlreadyExists), "already existed"),
                    (missing, "no longer found"),
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
        summary: if batch.problems_only {
            format!(
                "Showing {problems} of {} · {}",
                totals.emails,
                parts.join(" · ")
            )
        } else {
            parts.join(" · ")
        },
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
        show_all: "Show all".into(),
        about_prefix: ABOUT_PREFIX.into(),
        version_prefix: "Version ".into(),
        select_all: "Select All".into(),
        application_icon: "Application icon".into(),
        purpose: "Create body-only copies of EML messages.\nKeep original messages unchanged.".into(),
        project_website: "Project website".into(),
        hide_details: "Hide details".into(),
        details: "Details…".into(),
        website_details: "Website opening details".into(),
        license_title: "Software and artwork licenses".into(),
        license_notice: "Third-party terms and source references are included with the application.".into(),
        license_terms: "Software and artwork license terms".into(),
        notices_details: "Notices opening details".into(),
        third_party: "Third-party licenses…".into(),
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
    status: eml_core::vocabulary::fields::STATUS.into(),
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
    let kind = eml_core::vocabulary::part_kind(part_reason(part.reason), &part.content_type);
    crate::AttachmentView {
        name: attachment_name(part).into(),
        kind: kind.into(),
        size: format_bytes(part.bytes).into(),
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

#[cfg(any(feature = "desktop", test))]
pub(super) const OPENING: &str = "Opening…";
pub(super) const OPEN_SENT: &str = "Open command sent.";
pub(super) const OPEN_FAILED: &str = "The native open operation did not complete.";

pub(super) fn attachment_name(part: &eml_protocol::records::RemovedRecord) -> String {
    eml_protocol::DisplayText::new(eml_core::vocabulary::part_name(
        part.filename.value().map(String::as_str),
        part_reason(part.reason),
        &part.content_type,
    ))
    .to_string()
}
fn part_reason(reason: eml_protocol::records::Removal) -> eml_core::RemovalReason {
    match reason {
        eml_protocol::records::Removal::Attachment => eml_core::RemovalReason::Attachment,
        eml_protocol::records::Removal::RelatedResource => eml_core::RemovalReason::RelatedResource,
        eml_protocol::records::Removal::NonBodyPart => eml_core::RemovalReason::NonBodyPart,
    }
}
pub(super) const WAITING: &str = "Waiting";
pub(super) const WAITING_DETAIL: &str = "Waiting to be copied.";
pub(super) const MISSING: &str = "No longer found";
pub(super) const MISSING_DETAIL: &str = "This file was no longer there when the copies were made.";
pub(super) const START_FAILED: &str = "Couldn’t copy";
pub(super) const LAUNCH_FAILED: &str =
    "The copy process couldn’t start. Reinstall the app if this keeps happening.";
pub(super) const WORKER_FAILED: &str = "The copy process stopped unexpectedly. Results shown may be incomplete. Save Report… for details.";
pub(super) const STARTING: &str = "Starting…";
pub(super) const CHECK_METADATA: &str = "Metadata from: read-only check\n";
pub(super) const CREATE_METADATA: &str = "Metadata from: copy process\n";

pub(crate) const INVALID_SAVED_FOLDERS: &str =
    "Saved folders must use absolute paths; at most three recent destinations are supported.";

pub(crate) const CONFIG_UNAVAILABLE: &str = "Cannot locate the configuration folder";

pub(crate) const ABSOLUTE_DESTINATION: &str = "Choose an absolute destination folder";

pub(crate) const ABSOLUTE_REPORT_FOLDER: &str = "Choose an absolute report folder";

pub(crate) const SETTINGS_PARENT_MISSING: &str = "Settings file has no parent";

pub(crate) const ABSOLUTE_LAUNCH_DIRECTORY: &str =
    "Launch operands need an absolute working directory";

#[cfg(feature = "desktop")]
pub(crate) const BUNDLE_CONTENTS_MISSING: &str = "Desktop executable has no Contents directory";

pub(crate) fn report_filename(date: impl std::fmt::Display) -> String {
    format!("EML emails {date}.jsonl")
}

pub(crate) const REVISION_EXHAUSTED: &str = "batch revision exhausted";

pub(crate) const GENERATION_EXHAUSTED: &str = "worker generation exhausted";

/// Failure to locate the desktop home folder.
pub const HOME_UNAVAILABLE: &str = "Cannot locate your home folder";

#[cfg(all(feature = "desktop", target_os = "macos"))]
pub(crate) const QUIT_MAIN_THREAD: &str = "Quit adapter requires the main thread";

#[cfg(all(feature = "desktop", target_os = "macos"))]
pub(crate) const DELEGATE_MISSING: &str = "Winit application delegate is absent";

#[cfg(all(feature = "desktop", target_os = "macos"))]
pub(crate) const QUIT_DELEGATE_UNSUPPORTED: &str =
    "Cannot install the Quit adapter on this application delegate";

#[cfg(all(feature = "desktop", target_os = "macos"))]
pub(crate) const QUIT_INSTALL_FAILED: &str = "Could not install the Quit adapter";

#[cfg(all(feature = "desktop", target_os = "macos"))]
pub(crate) const DOCUMENT_DELEGATE_UNSUPPORTED: &str =
    "Cannot install document opening on this application delegate";

#[cfg(all(feature = "desktop", target_os = "macos"))]
pub(crate) const DOCUMENT_INSTALL_FAILED: &str = "Could not install document opening";

#[cfg(all(feature = "desktop", target_os = "macos"))]
pub(crate) const NSOBJECT_MISSING: &str = "NSObject is unavailable";

#[cfg(all(feature = "desktop", target_os = "macos"))]
pub(crate) const ABOUT_TARGET_FAILED: &str = "Cannot register the application About target";

#[cfg(target_os = "macos")]
pub(crate) fn bundle_required() -> String {
    format!(
        "Launch {}.app from the GUI archive, rather than the bare GUI executable. Its bundle metadata and Contents/Helpers CLI are required. To build it, run cargo xtask dist --app --out target/desktop.",
        eml_core::product::DISPLAY_NAME
    )
}

pub(crate) const SHOW_FINDER: &str = "Show in Finder";

pub(crate) const SHOW_FILE_MANAGER: &str = "Show in File Manager";

pub(crate) const ABOUT_PREFIX: &str = "About ";

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
