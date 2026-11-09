//! One-list product language; protocol diagnostics remain verbatim in exports.
use crate::state::{Batch, State};
use eml_core::vocabulary::{
    BatchTotals, HeadlineMode, ORIGINALS_UNCHANGED, attachment_count, format_bytes,
};
pub(super) struct Header {
    pub title: String,
    pub summary: String,
    pub problems: String,
}
pub(super) fn header(
    batch: &Batch,
    totals: &BatchTotals,
    inventory: Option<(u32, bool)>,
    stopped_totals: Option<&BatchTotals>,
    failure: Option<&eml_core::OperationFailure>,
    missing: usize,
) -> Header {
    let total = inventory.map(|(total, _complete)| total);
    let problems = totals.problems;
    let pill = if problems == 0 {
        String::new()
    } else {
        totals.problem_label()
    };
    let incomplete_create = batch
        .create
        .as_ref()
        .and_then(|slot| slot.completion.as_ref())
        .is_some_and(|completion| {
            matches!(
                completion.state,
                crate::worker::WorkerState::Incomplete
                    | crate::worker::WorkerState::Rejected
                    | crate::worker::WorkerState::Panicked
            )
        });
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
                        format!(
                            "{} of {} checked",
                            eml_core::vocabulary::format_count(totals.emails),
                            eml_core::vocabulary::format_count(n as usize)
                        ),
                        "nothing is written yet".into(),
                    ]
                })
                .unwrap_or_default(),
        ),
        State::Ready => {
            let h = totals.run_headline(
                HeadlineMode::Check {
                    total: inventory
                        .filter(|(n, complete)| *complete || *n > 0)
                        .map(|(n, _)| n as usize),
                },
                failure,
            );
            (h.verdict, h.parts)
        }
        State::Removing => (
            "Removing attachments…".into(),
            vec![ORIGINALS_UNCHANGED.into()],
        ),
        State::Stopped => {
            let h = stopped_totals
                .unwrap_or(totals)
                .run_headline(HeadlineMode::Stopped, failure);
            (h.verdict, h.parts)
        }
        State::Done => {
            let mut h = totals.run_headline(HeadlineMode::Done { incomplete_create }, failure);
            if missing > 0 {
                h.parts.push(format!(
                    "{} no longer found",
                    eml_core::vocabulary::format_count(missing)
                ));
            }
            (h.verdict, h.parts)
        }
    };
    Header {
        title,
        summary: if batch.problems_only {
            eml_core::vocabulary::showing_results(problems, totals.emails, &parts.join(" · "))
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
        showing_problems: eml_core::vocabulary::SHOWING_PROBLEMS.into(),
        about_prefix: ABOUT_PREFIX.into(),
        version_prefix: "Version ".into(),
        select_all: "Select All".into(),
        application_icon: "Application icon".into(),
        purpose: eml_core::product::PURPOSE.into(),
        project_website: "Project website".into(),
        hide_details: "Hide details".into(),
        details: "Details…".into(),
        processing_details: "Processing details".into(),
        website_details: "Website opening details".into(),
        license_title: "Software and artwork licenses".into(),
        license_notice:
            "Third-party terms and source references are included with the application.".into(),
        license_terms: "Software and artwork license terms".into(),
        notices_details: "Notices opening details".into(),
        third_party: "Third-party licenses…".into(),
        show_report: "Show".into(),
        show_saved_report: "Show Saved Report".into(),
        add_files: "Add Files…".into(),
        add_folder: "Add Folder…".into(),
        start_over: "Start Over".into(),
        empty_title: "Drop emails or folders here".into(),
        empty_body: eml_core::product::PURPOSE.into(),
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
        destination_hint: format!("as name{} · never overwrites", eml_core::COPY_SUFFIX).into(),
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
        quit_body: format!("Copies already saved are kept. {ORIGINALS_UNCHANGED}").into(),
        export_body: "A report is still being saved. The app quits when it finishes.".into(),
        cancel: "Cancel".into(),
        stop_quit: "Stop and Quit".into(),
        quit_now: "Quit Now".into(),
    }
}

pub(super) const BUSY_ADDING: &str = "Wait for this batch to finish before adding more emails.";
pub(super) const NEXT_TO: &str = "Next to originals";
pub(super) const ABSOLUTE_REPORT: &str = "Choose an absolute report path.";
pub(super) const REPORT_SAVED: &str = "Report saved.";
pub(super) const CONTINUE: &str = "Continue";
pub(super) const SETTINGS_UNREADABLE: &str =
    "Saved settings couldn't be read; using Next to originals.";
pub(super) fn footer(state: State) -> String {
    match state {
        State::Stopped => "Continue checks existing copies and saves the remaining copies.".into(),
        State::Done => format!("{ORIGINALS_UNCHANGED} Drop more emails to start a new batch."),
        _ => String::new(),
    }
}
pub(super) fn progress(done: usize, total: u32, remaining: Option<u64>) -> String {
    format!(
        "{} of {} done · {}",
        eml_core::vocabulary::format_count(done),
        eml_core::vocabulary::format_count(total as usize),
        estimate(remaining)
    )
}
pub(super) fn attachment(part: &eml_core::RemovedPart) -> crate::AttachmentView {
    let kind = eml_core::vocabulary::part_kind(part.reason, &part.content_type);
    crate::AttachmentView {
        name: attachment_name(part).into(),
        kind: kind.into(),
        size: format_bytes(part.bytes).into(),
    }
}

pub(super) const NOTHING_ADDED: &str = "Nothing was added.";
pub(super) const NO_ATTACHMENTS_CHIP: &str = "None";

pub(super) fn request_error(error: &eml_core::RequestPathError) -> String {
    error.to_string()
}

pub(super) fn row_description(folder: &str, count: Option<usize>, size: &str) -> String {
    format!(
        "{folder}; {}; {size}",
        count.map_or_else(|| "attachments unknown".into(), attachment_count)
    )
}

#[cfg(any(feature = "native", test))]
pub(super) const OPENING: &str = "Opening…";

pub(super) fn attachment_name(part: &eml_core::RemovedPart) -> String {
    eml_core::display::DisplayText::new(eml_core::vocabulary::part_name(
        part.filename.as_deref(),
        part.reason,
        &part.content_type,
    ))
    .to_string()
}
pub(super) const WAITING: &str = "Waiting";
pub(super) const WAITING_DETAIL: &str = "Waiting to be copied.";
pub(super) const MISSING: &str = "No longer found";
pub(super) const MISSING_DETAIL: &str = "This file was no longer there when the copies were made.";
pub(super) const START_FAILED: &str = "Couldn’t copy";
pub(super) const LAUNCH_FAILED: &str = "Couldn’t start processing. Try again.";
pub(super) const STOPPED_UNEXPECTEDLY: &str =
    "Processing stopped unexpectedly. Results shown may be incomplete. Save Report… for details.";
pub(super) const STARTING: &str = "Starting…";
pub(super) const CHECK_METADATA: &str = "Metadata from: read-only check\n";
pub(super) const CREATE_METADATA: &str = "Metadata from: attachment removal\n";

pub(crate) const INVALID_SAVED_FOLDERS: &str = "Saved folders must use absolute, NUL-free native paths within the path limits; at most three recent destinations are supported.";

pub(crate) const CONFIG_UNAVAILABLE: &str = "Cannot locate the configuration folder";

pub(crate) const SETTINGS_PARENT_MISSING: &str = "Settings file has no parent";

pub(crate) const ABSOLUTE_LAUNCH_DIRECTORY: &str =
    "Launch operands need an absolute working directory";

#[cfg(all(feature = "native", target_os = "macos"))]
pub(crate) const BUNDLE_CONTENTS_MISSING: &str = "Desktop executable has no Contents directory";

pub(crate) fn report_filename(date: impl std::fmt::Display) -> String {
    format!("EML emails {date}.jsonl")
}

pub(crate) const REVISION_EXHAUSTED: &str = "batch revision exhausted";

pub(crate) const GENERATION_EXHAUSTED: &str = "worker generation exhausted";

/// Failure to locate the desktop home folder.
pub const HOME_UNAVAILABLE: &str = "Cannot locate your home folder";

#[cfg(all(feature = "native", target_os = "macos"))]
pub(crate) const QUIT_MAIN_THREAD: &str = "Quit adapter requires the main thread";

#[cfg(all(feature = "native", target_os = "macos"))]
pub(crate) const DELEGATE_MISSING: &str = "Winit application delegate is absent";

#[cfg(all(feature = "native", target_os = "macos"))]
pub(crate) const QUIT_DELEGATE_UNSUPPORTED: &str =
    "Cannot install the Quit adapter on this application delegate";

#[cfg(all(feature = "native", target_os = "macos"))]
pub(crate) const QUIT_INSTALL_FAILED: &str = "Could not install the Quit adapter";

#[cfg(all(feature = "native", target_os = "macos"))]
pub(crate) const DOCUMENT_DELEGATE_UNSUPPORTED: &str =
    "Cannot install document opening on this application delegate";

#[cfg(all(feature = "native", target_os = "macos"))]
pub(crate) const DOCUMENT_INSTALL_FAILED: &str = "Could not install document opening";

#[cfg(all(feature = "native", target_os = "macos"))]
pub(crate) const NSOBJECT_MISSING: &str = "NSObject is unavailable";

#[cfg(all(feature = "native", target_os = "macos"))]
pub(crate) const ABOUT_TARGET_FAILED: &str = "Cannot register the application About target";

pub(crate) const SHOW_FINDER: &str = "Show in Finder";

pub(crate) const SHOW_FILE_MANAGER: &str = "Show in File Manager";

pub(crate) const ABOUT_PREFIX: &str = "About ";

#[cfg(feature = "native")]
pub(super) const EML_FILTER: &str = "EML messages";
#[cfg(feature = "native")]
pub(super) const REPORT_FILTER: &str = "JSON Lines report";

pub(super) fn failure_details(error: &eml_core::OperationFailure) -> String {
    use eml_core::vocabulary::fields;
    let detail = eml_core::display::DisplayText::new(error.detail());
    let native = error
        .native_diagnostic()
        .map(|n| format!(" ({n})"))
        .unwrap_or_default();
    format!(
        "{}: {}\n{}: {}\n{}: {detail}{native}\n",
        fields::CODE,
        error.code(),
        fields::PHASE,
        error.phase(),
        fields::DETAIL
    )
}
