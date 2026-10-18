//! Generated Windows system calls behind owned, checked native interfaces.
mod candidate;
mod child;
mod file;
mod handle;
mod inventory;
pub(crate) mod path;
mod publication;
mod root;
mod status;
mod writable;
pub use candidate::ReadyCandidate;
pub use file::BoundFile;
pub use inventory::{BoundDirectory, DirectoryEntries, EntryPin, observe_selected};
pub use publication::{PublicationCandidate, PublishAttempt};
pub use root::{PinnedRoot, RootBindingError};
pub use status::NtStatusError;
pub use writable::{CreatedDirectory, WritableDirectory};
#[cfg(test)]
mod test_fixture;
#[cfg(test)]
pub(crate) mod test_reparse;
#[cfg(test)]
mod test_actor {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/support/windows_actor.rs"
    ));
}
