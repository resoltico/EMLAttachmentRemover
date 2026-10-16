//! Generated Windows system calls behind owned, checked native interfaces.
mod child;
mod file;
mod handle;
mod inventory;
pub(crate) mod path;
pub use child::NtOpenError;
pub use file::BoundFile;
pub use inventory::{BoundDirectory, DirectoryEntries};
#[cfg(test)]
pub(crate) mod test_reparse;
#[cfg(test)]
mod test_actor {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/support/windows_actor.rs"
    ));
}
