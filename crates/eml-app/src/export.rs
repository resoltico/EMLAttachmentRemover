//! Export the received protocol stream without replacing any existing file.
use std::{
    fs::OpenOptions,
    io::{self, Write},
    path::Path,
};
/// Save the exact received JSONL, including an incomplete stream when a run failed.
/// Existing files are never replaced, even after a Save dialog offers Replace.
pub fn export_report(path: &Path, jsonl: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(jsonl)?;
    file.sync_all()
}
