//! Resolve one source identity for both binaries and archive timestamps.
use crate::{Result, capture};
use std::process::Command;

#[path = "../../crates/eml-core/build_input.rs"]
mod build_input;

pub(crate) fn revision() -> Result<String> {
    let revision = capture(Command::new("git").args(["rev-parse", "--verify", "HEAD"]))?;
    build_input::revision(&revision)?;
    let supplied = match std::env::var("EML_SOURCE_REVISION") {
        Ok(value) => Some(value).filter(|value| !value.is_empty()),
        Err(std::env::VarError::NotPresent) => None,
        Err(error) => return Err(error.into()),
    };
    if let Some(supplied) = supplied {
        build_input::revision(&supplied)?;
        if !supplied.eq_ignore_ascii_case(&revision) {
            return Err("EML_SOURCE_REVISION differs from checked-out HEAD".into());
        }
    }
    Ok(revision)
}
