//! One source identity and its packaging timestamp; declarations are not Git proof.
use super::SourceCommit;
use crate::Result;
use std::{ffi::OsStr, path::Path};
use xtask::archive::PackageEpoch;

pub(crate) struct PackageSource {
    commit: SourceCommit,
    epoch: PackageEpoch,
    provenance: &'static str,
}

fn text(value: &OsStr) -> Result<&str> {
    value
        .to_str()
        .ok_or_else(|| "Source inputs must be ASCII text".into())
}
fn epoch(value: &str) -> Result<PackageEpoch> {
    if value.is_empty() || value.len() > 10 || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err("SOURCE_DATE_EPOCH must contain decimal UTC Unix seconds".into());
    }
    Ok(PackageEpoch::new(value.parse()?)?)
}
fn git(root: &Path, arguments: &[&str]) -> Result<String> {
    let output = crate::git_command(root).args(arguments).output()?;
    if !output.status.success() || output.stdout.len() > 1024 * 1024 {
        return Err("Cannot establish the exact local Git source identity/timestamp".into());
    }
    Ok(String::from_utf8(output.stdout)?.trim_end().to_owned())
}

impl PackageSource {
    pub(crate) fn observe(root: &Path) -> Result<Self> {
        Self::resolve(
            root,
            std::env::var_os("EML_SOURCE_COMMIT").as_deref(),
            std::env::var_os("SOURCE_DATE_EPOCH").as_deref(),
        )
    }

    fn resolve(
        root: &Path,
        declared_commit: Option<&OsStr>,
        declared_epoch: Option<&OsStr>,
    ) -> Result<Self> {
        match std::fs::symlink_metadata(root.join(".git")) {
            Ok(_) => {
                let actual_root =
                    std::path::PathBuf::from(git(root, &["rev-parse", "--show-toplevel"])?)
                        .canonicalize()?;
                if actual_root != root.canonicalize()? {
                    return Err("Git source root differs from the Cargo workspace root".into());
                }
                let commit = SourceCommit::parse(git(root, &["rev-parse", "--verify", "HEAD"])?)?;
                let epoch = epoch(&git(
                    root,
                    &["show", "-s", "--format=%ct", commit.as_str()],
                )?)?;
                if declared_commit
                    .map(text)
                    .transpose()?
                    .is_some_and(|value| value != commit.as_str())
                    || declared_epoch
                        .map(text)
                        .transpose()?
                        .map(super::source::epoch)
                        .transpose()?
                        .is_some_and(|value| value != epoch)
                {
                    return Err("Declared source inputs contradict the admitted Git commit or commit timestamp".into());
                }
                Ok(Self {
                    commit,
                    epoch,
                    provenance: "git_commit",
                })
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let commit = SourceCommit::parse(text(declared_commit.ok_or("Source archives require explicit EML_SOURCE_COMMIT and SOURCE_DATE_EPOCH; no Git provenance is inferred")?)?.to_owned())?;
                let epoch = epoch(text(declared_epoch.ok_or("Source archives require explicit EML_SOURCE_COMMIT and SOURCE_DATE_EPOCH; no timestamp default is used")?)?)?;
                // These inputs describe the caller's source archive. They do not
                // establish that its bytes correspond to a verified Git tree.
                Ok(Self {
                    commit,
                    epoch,
                    provenance: "declared_archive",
                })
            }
            Err(error) => Err(error.into()),
        }
    }
    pub(crate) fn commit(&self) -> &str {
        self.commit.as_str()
    }
    pub(crate) fn epoch(&self) -> PackageEpoch {
        self.epoch
    }
    pub(crate) fn provenance(&self) -> &str {
        self.provenance
    }
    pub(crate) fn working_tree(&self, root: &Path) -> Result<Option<String>> {
        if self.provenance == "declared_archive" {
            return match std::fs::symlink_metadata(root.join(".git")) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
                _ => Err(
                    "Declared source archive acquired Git metadata; reload its source context"
                        .into(),
                ),
            };
        }
        // Re-observe consistency against the held input; environment declarations
        // never acquire fresh authority during a consuming operation.
        if git(root, &["rev-parse", "--verify", "HEAD"])? != self.commit()
            || epoch(&git(root, &["show", "-s", "--format=%ct", self.commit()])?)? != self.epoch
        {
            return Err("Source commit or commit timestamp changed after admission".into());
        }
        Ok(Some(git(
            root,
            &["status", "--porcelain=v1", "--untracked-files=all"],
        )?))
    }
    pub(crate) fn verify_packaging_source(&self, root: &Path) -> Result<()> {
        if self
            .working_tree(root)?
            .is_some_and(|status| !status.is_empty())
        {
            return Err("Packaging requires the admitted clean committed source; preserve and commit intended changes first".into());
        }
        Ok(())
    }
    pub(crate) fn require_qualified_source(&self, root: &Path) -> Result<()> {
        if self.provenance != "git_commit" {
            return Err("Full qualification and release admission require a verified Git checkout; declared archive inputs are not Git provenance".into());
        }
        self.verify_packaging_source(root)
    }
    #[cfg(test)]
    pub(crate) fn fixture() -> Self {
        Self {
            commit: SourceCommit::parse("a".repeat(40)).unwrap(),
            epoch: PackageEpoch::new(1_609_459_201).unwrap(),
            provenance: "git_commit",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    #[test]
    fn source_archive_requires_both_valid_inputs_and_does_not_claim_git_verification() {
        let root = tempfile::tempdir().unwrap();
        for (commit, time) in [
            (None, None),
            (Some("a".repeat(40)), None),
            (None, Some("1609459201".to_owned())),
            (Some("bad".into()), Some("1609459201".into())),
            (Some("a".repeat(40)), Some("0".into())),
        ] {
            assert!(
                PackageSource::resolve(
                    root.path(),
                    commit.as_deref().map(OsStr::new),
                    time.as_deref().map(OsStr::new)
                )
                .is_err()
            );
        }
        let source = PackageSource::resolve(
            root.path(),
            Some(OsStr::new(&"a".repeat(40))),
            Some(OsStr::new("1609459201")),
        )
        .unwrap();
        assert_eq!(source.provenance(), "declared_archive");
        assert_eq!(source.epoch().seconds(), 1_609_459_201);
    }
    #[test]
    fn actual_git_timestamp_cannot_be_overridden_or_replaced_by_an_environment_claim() {
        let root = tempfile::tempdir().unwrap();
        for args in [
            vec!["init", "--quiet"],
            vec!["config", "user.email", "fixture@example.invalid"],
            vec!["config", "user.name", "Fixture"],
        ] {
            assert!(
                Command::new("git")
                    .current_dir(root.path())
                    .args(args)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        std::fs::write(root.path().join("source"), b"fixed source").unwrap();
        assert!(
            Command::new("git")
                .current_dir(root.path())
                .args(["add", "source"])
                .status()
                .unwrap()
                .success()
        );
        assert!(
            Command::new("git")
                .current_dir(root.path())
                .args(["commit", "--quiet", "-m", "source fixture"])
                .env("GIT_AUTHOR_DATE", "1609459201 +0000")
                .env("GIT_COMMITTER_DATE", "1609459201 +0000")
                .status()
                .unwrap()
                .success()
        );
        let source = PackageSource::resolve(root.path(), None, None).unwrap();
        assert_eq!(source.provenance(), "git_commit");
        assert_eq!(source.epoch().seconds(), 1_609_459_201);
        assert!(
            PackageSource::resolve(
                root.path(),
                Some(OsStr::new(source.commit())),
                Some(OsStr::new("1609459201"))
            )
            .is_ok()
        );
        assert!(PackageSource::resolve(root.path(), None, Some(OsStr::new("1609459202"))).is_err());
        assert!(
            PackageSource::resolve(
                root.path(),
                Some(OsStr::new(&"b".repeat(40))),
                Some(OsStr::new("1609459201"))
            )
            .is_err()
        );
        // An invalid .git marker must not silently become a declared archive.
        std::fs::remove_dir_all(root.path().join(".git")).unwrap();
        std::fs::write(root.path().join(".git"), b"not Git metadata").unwrap();
        assert!(
            PackageSource::resolve(
                root.path(),
                Some(OsStr::new(source.commit())),
                Some(OsStr::new("1609459201"))
            )
            .is_err()
        );
    }
}
