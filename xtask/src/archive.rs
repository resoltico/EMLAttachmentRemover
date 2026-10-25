use crate::{Result, capture};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
};

pub(crate) struct Timestamp {
    epoch: u64,
    zip: zip::DateTime,
}
impl Timestamp {
    pub(crate) fn commit() -> Result<Self> {
        let epoch =
            capture(Command::new("git").args(["show", "-s", "--format=%ct", "HEAD"]))?.parse()?;
        let date = capture(Command::new("git").env("TZ", "UTC").args([
            "show",
            "-s",
            "--date=format-local:%Y %m %d %H %M %S",
            "--format=%cd",
            "HEAD",
        ]))?;
        let parts = date
            .split_whitespace()
            .map(str::parse::<u16>)
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let zip = zip::DateTime::from_date_and_time(
            parts[0],
            parts[1].try_into()?,
            parts[2].try_into()?,
            parts[3].try_into()?,
            parts[4].try_into()?,
            (parts[5] / 2 * 2).try_into()?,
        )?;
        Ok(Self { epoch, zip })
    }
}
fn files(directory: &Path, result: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if path.is_dir() {
            files(&path, result)?;
        } else {
            result.push(path);
        }
    }
    Ok(())
}
pub(crate) fn write(stage: &Path, out: &Path, stamp: &Timestamp) -> Result<()> {
    let mut paths = Vec::new();
    files(stage, &mut paths)?;
    paths.sort();
    let file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(out)?;
    if out.extension().is_some_and(|extension| extension == "zip") {
        let mut archive = zip::ZipWriter::new(file);
        for path in paths {
            let options = zip::write::SimpleFileOptions::default()
                .last_modified_time(stamp.zip)
                .unix_permissions(mode(&path)?)
                .compression_method(zip::CompressionMethod::Deflated);
            let name = path
                .strip_prefix(stage)?
                .to_str()
                .ok_or("non-UTF8 archive path")?
                .replace('\\', "/");
            archive.start_file(name, options)?;
            let mut bytes = Vec::new();
            fs::File::open(path)?.read_to_end(&mut bytes)?;
            archive.write_all(&bytes)?;
        }
        archive.finish()?.sync_all()?;
    } else {
        let gzip = flate2::GzBuilder::new()
            .mtime(stamp.epoch.try_into()?)
            .write(file, flate2::Compression::default());
        let mut archive = tar::Builder::new(gzip);
        for path in paths {
            let mut header = tar::Header::new_gnu();
            header.set_size(fs::metadata(&path)?.len());
            header.set_mode(mode(&path)?);
            header.set_mtime(stamp.epoch);
            header.set_cksum();
            archive.append_data(
                &mut header,
                path.strip_prefix(stage)?,
                fs::File::open(&path)?,
            )?;
        }
        archive.into_inner()?.finish()?.sync_all()?;
    }
    Ok(())
}
fn mode(path: &Path) -> Result<u32> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        Ok(fs::metadata(path)?.permissions().mode() & 0o777)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(0o644)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn archives_use_commit_time_contents_and_refuse_replacement() {
        let scratch = tempfile::tempdir().unwrap();
        let stage = scratch.path().join("stage");
        fs::create_dir(&stage).unwrap();
        fs::write(stage.join("LICENSE"), b"license text").unwrap();
        let stamp = Timestamp::commit().unwrap();
        for extension in ["tar.gz", "zip"] {
            let path = scratch.path().join(format!("test.{extension}"));
            write(&stage, &path, &stamp).unwrap();
            assert!(write(&stage, &path, &stamp).is_err());
            if extension == "zip" {
                let mut zip = zip::ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
                assert_eq!(zip.len(), 1);
                let mut member = zip.by_name("LICENSE").unwrap();
                assert_eq!(member.last_modified().unwrap(), stamp.zip);
                let mut text = String::new();
                member.read_to_string(&mut text).unwrap();
                assert_eq!(text, "license text");
            } else {
                let gzip = flate2::read::GzDecoder::new(fs::File::open(path).unwrap());
                assert_eq!(u64::from(gzip.header().unwrap().mtime()), stamp.epoch);
                let mut tar = tar::Archive::new(gzip);
                let mut entries = tar.entries().unwrap();
                let member = entries.next().unwrap().unwrap();
                assert_eq!(member.path().unwrap(), Path::new("LICENSE"));
                assert_eq!(member.header().mtime().unwrap(), stamp.epoch);
                assert!(entries.next().is_none());
            }
        }
    }
}
