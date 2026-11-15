//! Resolve one source identity for both binaries and archive timestamps.
use crate::{Result, capture};
use std::process::Command;

#[path = "../../crates/eml-core/build_input.rs"]
mod build_input;

pub(crate) fn revision() -> Result<String> {
    let supplied = match std::env::var("EML_SOURCE_REVISION") {
        Ok(value) => Some(value).filter(|value| !value.is_empty()),
        Err(std::env::VarError::NotPresent) => None,
        Err(error) => return Err(error.into()),
    };
    revision_at(&crate::workspace_root(&std::env::current_dir()?)?, supplied)
}
fn revision_at(directory: &std::path::Path, supplied: Option<String>) -> Result<String> {
    if !directory.join(".git").try_exists()? {
        let revision = supplied.ok_or("A source tree without Git requires EML_SOURCE_REVISION and SOURCE_DATE_EPOCH from SOURCE.txt")?;
        build_input::revision(&revision)?;
        return Ok(revision);
    }
    let revision = capture(Command::new("git").current_dir(directory).args([
        "rev-parse",
        "--verify",
        "HEAD",
    ]))?;
    build_input::revision(&revision)?;
    if supplied.is_some_and(|value| !value.eq_ignore_ascii_case(&revision)) {
        return Err("EML_SOURCE_REVISION differs from checked-out HEAD".into());
    }
    Ok(revision)
}

pub(crate) fn epoch(revision: &str) -> Result<u64> {
    let supplied = match std::env::var("SOURCE_DATE_EPOCH") {
        Ok(value) => Some(parse_epoch(&value)?),
        Err(std::env::VarError::NotPresent) => None,
        Err(error) => return Err(error.into()),
    };
    let directory = crate::workspace_root(&std::env::current_dir()?)?;
    if !directory.join(".git").try_exists()? {
        return supplied.ok_or_else(|| {
            "A source tree without Git requires SOURCE_DATE_EPOCH from SOURCE.txt".into()
        });
    }
    let epoch = parse_epoch(&capture(
        Command::new("git")
            .current_dir(&directory)
            .args(["show", "-s", "--format=%ct", revision]),
    )?)?;
    if supplied.is_some_and(|value| value != epoch) {
        return Err("SOURCE_DATE_EPOCH differs from the source commit timestamp".into());
    }
    Ok(epoch)
}
fn parse_epoch(value: &str) -> Result<u64> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("SOURCE_DATE_EPOCH must be decimal Unix seconds".into());
    }
    Ok(value.parse()?)
}

/// Capture working source and both workspaces' locked dependency sources.
pub(crate) fn archive(out: &std::path::Path) -> Result<()> {
    use std::{fs, path::Path};

    let destination = out.join(format!(
        "emlattachmentremover-source-{}.tar.gz",
        eml_core::product::VERSION
    ));
    if destination.try_exists()? {
        return Err(format!(
            "Source archive refuses to overwrite {}",
            destination.display()
        )
        .into());
    }
    let unknown =
        capture(Command::new("git").args(["ls-files", "--others", "--exclude-standard", "-z"]))?;
    if !unknown.is_empty() {
        return Err(format!("Source archive refuses untracked files; review and add intended source first:\n{unknown}").into());
    }
    let revision = revision()?;
    let stamp = crate::archive::Timestamp::for_revision(&revision)?;
    let scratch = tempfile::tempdir()?;
    let tree = scratch.path().join("source");
    fs::create_dir(&tree)?;
    let index = capture(Command::new("git").args(["ls-files", "--stage", "-z"]))?;
    let mut originals = Vec::new();
    for entry in index.split('\0').filter(|entry| !entry.is_empty()) {
        let (header, name) = entry.split_once('\t').ok_or("Invalid source index entry")?;
        let fields = header.split_whitespace().collect::<Vec<_>>();
        if fields.len() != 3 || fields[2] != "0" || !["100644", "100755"].contains(&fields[0]) {
            return Err(
                format!("Source archive requires an ordinary stage-0 index entry: {name}").into(),
            );
        }
        let original = Path::new(name);
        if !fs::symlink_metadata(original)?.file_type().is_file() {
            return Err(format!("Source archive requires a regular tracked file: {name}").into());
        }
        let destination = tree.join(original);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        let bytes = fs::read(original)?;
        let permissions = fs::metadata(original)?.permissions();
        fs::write(&destination, &bytes)?;
        fs::set_permissions(destination, permissions.clone())?;
        originals.push((original.to_owned(), bytes, permissions));
    }
    let config = capture(
        Command::new("cargo")
            .args([
                "vendor",
                "--locked",
                "--versioned-dirs",
                "--sync",
                "fuzz/Cargo.toml",
                "vendor",
            ])
            .current_dir(&tree),
    )?;
    unchanged(&index, &originals)?;
    let config_path = tree.join(".cargo/config.toml");
    let mut configuration = fs::read_to_string(&config_path)?;
    configuration.push('\n');
    configuration.push_str(&config);
    configuration.push('\n');
    fs::write(config_path, configuration)?;
    fs::write(tree.join("SOURCE.txt"), notice(&revision)?)?;
    let staged = scratch.path().join("source.tar.gz");
    crate::archive::write(&tree, &staged, &stamp)?;
    unchanged(&index, &originals)?;
    fs::create_dir_all(out)?;
    let mut publication = tempfile::NamedTempFile::new_in(out)?;
    std::io::copy(&mut fs::File::open(staged)?, publication.as_file_mut())?;
    publication.as_file().sync_all()?;
    publication
        .persist_noclobber(destination)
        .map_err(|error| error.error)?;
    Ok(())
}

fn unchanged(
    index: &str,
    originals: &[(std::path::PathBuf, Vec<u8>, std::fs::Permissions)],
) -> Result<()> {
    use std::fs;

    if capture(Command::new("git").args(["ls-files", "--stage", "-z"]))? != index
        || !capture(Command::new("git").args(["ls-files", "--others", "--exclude-standard", "-z"]))?
            .is_empty()
    {
        return Err(
            "Source index or untracked inputs changed while vendoring; no archive was published"
                .into(),
        );
    }
    for (original, bytes, permissions) in originals {
        let metadata = fs::symlink_metadata(original)?;
        if !metadata.file_type().is_file()
            || fs::read(original)? != *bytes
            || metadata.permissions() != *permissions
        {
            return Err(format!(
                "Source input changed while vendoring: {}; no archive was published",
                original.display()
            )
            .into());
        }
    }
    Ok(())
}

pub(crate) fn notice(revision: &str) -> Result<String> {
    let epoch = epoch(revision)?;
    Ok(format!(
        "EML Attachment Remover {}\nEML_SOURCE_REVISION: {revision}\nSOURCE_DATE_EPOCH: {epoch}\n\
        Corresponding source archive: emlattachmentremover-source-{}.tar.gz\n\
        Upstream: https://github.com/resoltico/EMLAttachmentRemover\n\
        Original software: AGPL-3.0-only; see LICENSE and PROJECT-NOTICE.md.\n\
        Original artwork is AGPL-3.0-only; third-party terms remain unchanged.\n\
        The source archive includes tracked working sources and locked vendored dependencies.\n\
        Modified distributions require their own matching Corresponding Source; an unchanged upstream archive is insufficient.\n",
        eml_core::product::VERSION,
        eml_core::product::VERSION,
    ))
}

/// Verify a captured archive independently of the host's registry cache.
pub(crate) fn verify(out: &std::path::Path) -> Result<()> {
    use std::fs;

    let scratch = tempfile::tempdir()?;
    let archive = out.join(format!(
        "emlattachmentremover-source-{}.tar.gz",
        eml_core::product::VERSION
    ));
    let unpack = scratch.path().join("source");
    fs::create_dir(&unpack)?;
    tar::Archive::new(flate2::read::GzDecoder::new(fs::File::open(archive)?)).unpack(&unpack)?;
    let command = || {
        let mut command = Command::new("cargo");
        command
            .current_dir(&unpack)
            .env("CARGO_HOME", scratch.path().join("cargo-home"))
            .env("CARGO_TARGET_DIR", scratch.path().join("target"));
        command
    };
    capture(command().args(["metadata", "--offline", "--locked", "--format-version", "1"]))?;
    capture(command().args([
        "metadata",
        "--offline",
        "--locked",
        "--manifest-path",
        "fuzz/Cargo.toml",
        "--format-version",
        "1",
    ]))?;
    crate::run(command().args([
        "check",
        "--offline",
        "--locked",
        "-p",
        "emlattachmentremover",
    ]))?;
    // The same metadata command must reject an archive missing its vendor sources.
    fs::rename(unpack.join("vendor"), unpack.join("vendor-held"))?;
    let rejected = command()
        .args(["metadata", "--offline", "--locked", "--format-version", "1"])
        .output()?;
    if rejected.status.success() {
        return Err("Offline source verification accepted missing vendored sources".into());
    }
    eprintln!(
        "Offline root/fuzz resolution and CLI build passed; missing-vendor negative control rejected"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracted_source_requires_explicit_valid_identity_without_using_ancestor_git() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../target");
        let directory = tempfile::tempdir_in(root).unwrap();
        let revision = "0123456789abcdef0123456789abcdef01234567";
        assert_eq!(
            revision_at(directory.path(), Some(revision.into())).unwrap(),
            revision
        );
        assert!(revision_at(directory.path(), None).is_err());
        assert!(revision_at(directory.path(), Some("invalid".into())).is_err());
        std::fs::create_dir(directory.path().join(".git")).unwrap();
        assert!(revision_at(directory.path(), Some(revision.into())).is_err());
    }

    #[test]
    fn source_epoch_is_explicit_decimal_without_signs_or_whitespace() {
        assert_eq!(parse_epoch("1794700000").unwrap(), 1_794_700_000);
        for bad in ["", "-1", "+1", " 1", "1\n", "18446744073709551616"] {
            assert!(parse_epoch(bad).is_err(), "{bad:?}");
        }
    }
}
