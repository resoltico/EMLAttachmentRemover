//! Native installer qualification against the CLI archive produced by this checkout.
use crate::{Result, dist, host, run};
use std::process::Command;

pub(crate) fn check() -> Result<()> {
    let out = tempfile::tempdir()?;
    dist::build(true, false, Some(host()?), out.path())?;
    run(Command::new("python3")
        .arg("install/prepare-test-release.py")
        .arg(out.path()))?;
    run(Command::new("bash")
        .arg("install/install_test.sh")
        .arg(out.path()))
}
