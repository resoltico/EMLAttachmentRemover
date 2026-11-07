//! One-list product language; protocol diagnostics remain verbatim in exports.
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
    }
}
pub(crate) fn tone(status: Status) -> i32 {
    match status {
        Status::Ready => 1,
        Status::Saved | Status::AsIs | Status::Existing => 2,
        Status::NeedsChecking | Status::Unknown => 3,
        Status::Failed => 4,
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
