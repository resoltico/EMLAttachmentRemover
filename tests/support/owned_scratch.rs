//! Retain synthetic native-test resources when a failed boundary is unsettled.
use std::{io, path::Path};
pub(crate) struct OwnedScratch(Option<tempfile::TempDir>);
impl OwnedScratch {
    pub(crate) fn new(parent: &Path) -> io::Result<Self> {
        Ok(Self(Some(tempfile::tempdir_in(parent)?)))
    }
    pub(crate) fn path(&self) -> &Path {
        self.0.as_ref().expect("live owned scratch").path()
    }
    pub(crate) fn close(mut self) -> io::Result<()> {
        self.0.take().expect("live owned scratch").close()
    }
}
impl Drop for OwnedScratch {
    fn drop(&mut self) {
        if std::thread::panicking()
            && let Some(directory) = self.0.take()
        {
            let retained = directory.keep();
            eprintln!(
                "Retained owned native failure resources: {}",
                retained.display()
            );
        }
    }
}
