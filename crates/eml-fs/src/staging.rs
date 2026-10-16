//! Shared staging-name resource contract; native storage engines stay separate.
use std::{ffi::OsString, fmt::Write, io};

pub(crate) const NAME_ATTEMPTS: usize = 16;

pub(crate) fn random_name() -> io::Result<OsString> {
    let mut bytes = [0; 16];
    getrandom::fill(&mut bytes).map_err(io::Error::other)?;
    let mut name = String::from(".eml-attachment-remover-");
    for byte in bytes {
        write!(&mut name, "{byte:02x}").expect("writing String cannot fail");
    }
    Ok(name.into())
}
