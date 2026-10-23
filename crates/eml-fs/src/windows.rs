//! Generated Windows system calls behind owned, checked native interfaces.
mod candidate;
mod child;
mod containment;
#[cfg(any(test, feature = "native-fixtures"))]
pub mod creation_observer;
mod file;
mod handle;
mod inventory;
pub(crate) mod path;
mod private_directory;
mod publication;
pub use private_directory::{
    PrivateDirectory, create_private_directory, rename_artifact_noreplace,
};
mod creation;
mod name_domain;
mod root;
mod status;
pub(crate) mod sync_directory;
pub use candidate::ReadyCandidate;
pub use containment::ContainmentFailure;
pub use creation::{CreationDirectory, DirectoryCreation};
pub use file::BoundFile;
pub use inventory::{BoundDirectory, DirectoryEntries, EntryPin, observe_selected};
pub use publication::{PublicationCandidate, PublishAttempt};
pub use root::{PinnedRoot, RootBindingError};
pub use status::NtStatusError;
#[cfg(test)]
mod test_fixture;
#[cfg(any(test, feature = "native-fixtures"))]
pub(crate) mod test_reparse;
#[cfg(any(test, feature = "native-fixtures"))]
mod test_actor {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/support/windows_actor.rs"
    ));
}
