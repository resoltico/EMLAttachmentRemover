//! Atomically save the exact received stream to a user-approved report path.
use std::{
    io::{self, Write},
    path::Path,
};
/// Save the exact received JSONL, including an incomplete stream when a run failed.
/// The caller owns approval of the target, including replacement.
/// The final name exposes only complete, file-synchronized bytes. This does not
/// promise that the directory entry survives power loss. A crash can leave an
/// owned private staging file beside the requested destination.
pub fn export_report(path: &Path, jsonl: &[u8]) -> io::Result<()> {
    let directory = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut stage = tempfile::Builder::new()
        .prefix(".emlattachmentremover-report-")
        .suffix(".tmp")
        .tempfile_in(directory)?;
    if let Err(error) = stage
        .as_file_mut()
        .write_all(jsonl)
        .and_then(|()| stage.as_file().sync_all())
    {
        return Err(discard(stage, error));
    }
    stage
        .persist(path)
        .map(|_| ())
        .map_err(|failure| discard(failure.file, failure.error))
}

fn discard(stage: tempfile::NamedTempFile, error: io::Error) -> io::Error {
    let path = stage.path().to_path_buf();
    match stage.close() {
        Ok(()) => error,
        Err(cleanup) => io::Error::new(
            error.kind(),
            format!(
                "{error}; could not remove private report staging file {}: {cleanup}",
                eml_protocol::DisplayPath::new(&path)
            ),
        ),
    }
}
