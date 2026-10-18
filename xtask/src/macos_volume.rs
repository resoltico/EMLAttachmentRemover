//! Owned exFAT image capability and actual Core observations on fresh CI hosts.
use crate::{Result, controller, macos};
use eml_core::{
    Event, EventSink, FailureEffect, ItemResult, OutputLocation, RunControl, RunMode, RunRequest,
};
use rustix::fs::{self as native, AtFlags, Mode, OFlags};
use serde_json::{Value, json};
use std::{fs, io::Write, os::unix::fs::MetadataExt, path::Path, time::Duration};
#[derive(Default)]
struct Sink {
    items: Vec<Value>,
}
impl EventSink for Sink {
    fn emit(&mut self, event: Event<'_>) -> std::io::Result<()> {
        if let Event::Item(item) = event {
            self.items.push(match item.result {
            ItemResult::Created(ref copy,_)=>{let copy=copy.publication();json!({"status":"created","publication":"published","address":format!("{:?}",copy.address()),"file_sync":format!("{:?}",copy.file_sync()),"namespace_sync":format!("{:?}",copy.namespace_sync()),"cleanup":format!("{:?}",copy.cleanup())})},
            ItemResult::UpToDate(_,_)=>json!({"status":"up_to_date"}),
            ItemResult::Failed(ref failure)=>json!({"status":"failed","code":format!("{:?}",failure.error().code()),"phase":format!("{:?}",failure.error().phase()),"publication":match failure.effect(){FailureEffect::NotPublished{..}=>"not_published",FailureEffect::Published(_)=>"published",FailureEffect::Unknown(_)=>"unknown"},"effect":format!("{:?}",failure.effect())}),
            ItemResult::Planned(_)=>json!({"status":"unexpected_plan"}),
        });
        }
        Ok(())
    }
}
fn plist(
    root: &Path,
    out: &Path,
    program: &str,
    args: &[&std::ffi::OsStr],
    label: &str,
) -> Result<Value> {
    let path = out.join(format!("{label}.plist"));
    let mut command = std::process::Command::new(program);
    command.current_dir(root).args(args);
    let result = controller()?.run_separate_output(command, &path, Duration::from_secs(60))?;
    result.require_success(&path)?;
    let mut convert = std::process::Command::new("/usr/bin/plutil");
    convert.args(["-convert", "json", "-o", "-"]).arg(&path);
    let bytes = crate::capture(&mut convert)?;
    if bytes.len() > 4 * 1024 * 1024 {
        return Err("Disk-image observation exceeds its bound".into());
    }
    Ok(serde_json::from_slice(&bytes)?)
}
fn association(
    root: &Path,
    out: &Path,
    image: &Path,
    mount: &Path,
    label: &str,
) -> Result<Option<String>> {
    let info = plist(
        root,
        out,
        "/usr/bin/hdiutil",
        &[std::ffi::OsStr::new("info"), std::ffi::OsStr::new("-plist")],
        label,
    )?;
    select_association(&info, image, mount)
}
fn select_association(info: &Value, image: &Path, mount: &Path) -> Result<Option<String>> {
    let images = info["images"]
        .as_array()
        .ok_or("Missing native image association list")?;
    let matches: Vec<_> = images
        .iter()
        .filter(|entry| {
            entry["image-path"]
                .as_str()
                .is_some_and(|value| Path::new(value) == image)
        })
        .collect();
    if matches.is_empty() {
        return Ok(None);
    }
    if matches.len() != 1 {
        return Err("Ambiguous owned image association".into());
    }
    let entries = matches[0]["system-entities"]
        .as_array()
        .ok_or("Missing owned device entities")?;
    if !entries.iter().any(|entry| {
        entry["mount-point"]
            .as_str()
            .is_some_and(|value| Path::new(value) == mount)
    }) {
        return Err("Owned image is associated with an unexpected mountpoint".into());
    }
    let devices: Vec<_> = entries
        .iter()
        .filter_map(|entry| entry["dev-entry"].as_str())
        .filter(|device| {
            device.strip_prefix("/dev/disk").is_some_and(|suffix| {
                !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit())
            })
        })
        .collect();
    if devices.len() != 1 {
        return Err("No unique whole-device identity for the owned image".into());
    }
    Ok(Some(devices[0].to_owned()))
}

fn primitive(mount: &Path) -> Result<Value> {
    let directory = native::open(
        mount,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )?;
    let filesystem = native::fstatfs(&directory)?;
    let name: Vec<u8> = filesystem
        .f_fstypename
        .iter()
        .map(|byte| *byte as u8)
        .take_while(|byte| *byte != 0)
        .collect();
    if !String::from_utf8(name.clone())?.eq_ignore_ascii_case("exfat") {
        return Err("Owned mounted filesystem is not actual exFAT".into());
    }
    let mut candidate = fs::File::from(native::openat(
        &directory,
        "primitive-candidate",
        OFlags::CREATE | OFlags::EXCL | OFlags::RDWR | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::from_raw_mode(0o600),
    )?);
    let bytes = b"owned synthetic exFAT atomic primitive\n";
    candidate.write_all(bytes)?;
    candidate.flush()?;
    let stat = native::fstat(&candidate)?;
    let sync = native::fsync(&candidate).map_err(std::io::Error::from);
    let full = native::fcntl_fullfsync(&candidate).map_err(std::io::Error::from);
    let edge = native::renameat_with(
        &directory,
        "primitive-candidate",
        &directory,
        "primitive-copy",
        native::RenameFlags::NOREPLACE,
    );
    let first = match edge {
        Ok(()) => {
            if fs::read(mount.join("primitive-copy"))? != bytes {
                return Err("exFAT exclusive primitive published incomplete/wrong bytes".into());
            }
            let observed = native::statat(&directory, "primitive-copy", AtFlags::SYMLINK_NOFOLLOW)?;
            if (observed.st_dev, observed.st_ino) != (stat.st_dev, stat.st_ino) {
                return Err("exFAT primitive changed object identity".into());
            }
            json!({"outcome":"succeeded"})
        }
        Err(error) => {
            let error = std::io::Error::from(error);
            if !matches!(error.raw_os_error(), Some(22 | 45 | 78 | 95)) {
                return Err(format!("Unexpected exFAT native primitive refusal: {error}").into());
            }
            if fs::read(mount.join("primitive-candidate"))? != bytes
                || mount.join("primitive-copy").try_exists()?
            {
                return Err("Refused exFAT primitive changed its candidate/final state".into());
            }
            fs::write(mount.join("primitive-copy"), b"owned existing winner")?;
            json!({"outcome":"unsupported","native_code":error.raw_os_error(),"kind":format!("{:?}",error.kind())})
        }
    };
    let winner = fs::read(mount.join("primitive-copy"))?;
    fs::write(mount.join("primitive-second"), b"owned losing candidate")?;
    let conflict = native::renameat_with(
        &directory,
        "primitive-second",
        &directory,
        "primitive-copy",
        native::RenameFlags::NOREPLACE,
    )
    .err()
    .ok_or("exFAT no-replace overwrote an existing winner")?;
    if first["outcome"] == "succeeded" && conflict != rustix::io::Errno::EXIST {
        return Err("Supported exFAT no-replace conflict did not report EEXIST".into());
    }
    if fs::read(mount.join("primitive-copy"))? != winner
        || fs::read(mount.join("primitive-second"))? != b"owned losing candidate"
    {
        return Err("exFAT conflict changed winner/candidate bytes".into());
    }
    let namespace_sync = native::fsync(&directory).map_err(std::io::Error::from);
    let sync_fact = |result: &std::io::Result<()>| json!({"succeeded":result.is_ok(),"native_code":result.as_ref().err().and_then(std::io::Error::raw_os_error),"error":result.as_ref().err().map(ToString::to_string)});
    Ok(
        json!({"filesystem":String::from_utf8(name)?,"flags":filesystem.f_flags,"candidate_mode":stat.st_mode&0o7777,"uid":stat.st_uid,"gid":stat.st_gid,"primitive":first,"conflict_native_code":conflict.raw_os_error(),"file_fsync":sync_fact(&sync),"file_fullfsync":sync_fact(&full),"namespace_fsync":sync_fact(&namespace_sync)}),
    )
}
fn core(root: &Path, mount: &Path) -> Result<Value> {
    let source = root.join("message.eml");
    let input = include_bytes!("../../tests/contract/mime/inputs/01-gmail-attachment.eml");
    let golden = include_bytes!("../../tests/contract/mime/golden/01-gmail-attachment.eml");
    fs::write(&source, input)?;
    let request = RunRequest {
        paths: vec![source.clone()],
        working_directory: root.to_owned(),
        output: OutputLocation::Directory(mount.to_owned()),
        mode: RunMode::Create,
    };
    let mut sink = Sink::default();
    let result = eml_core::run(&request, &RunControl::default(), &mut sink)?;
    if fs::read(&source)? != input {
        return Err("exFAT Core probe changed its original".into());
    }
    let copy = mount.join("message.no-attachments.eml");
    let present = copy.try_exists()?;
    if present && fs::read(&copy)? != golden {
        return Err("exFAT Core produced a partial or wrong final copy".into());
    }
    if result.counts().created() == 1 {
        if !present || sink.items[0]["status"] != "created" {
            return Err("exFAT Created lacks its complete physical copy".into());
        }
        let mut again = Sink::default();
        if eml_core::run(&request, &RunControl::default(), &mut again)?
            .counts()
            .up_to_date()
            != 1
        {
            return Err("Supported exFAT copy failed actual UpToDate resynchronization".into());
        }
    } else if result.counts().created() != 0
        || (present
            && !sink
                .items
                .iter()
                .any(|item| item["publication"] == "published" || item["publication"] == "unknown"))
    {
        return Err("exFAT refusal disagrees with observed physical publication".into());
    }
    Ok(
        json!({"created":result.counts().created(),"failed":result.counts().failed(),"physical_complete_copy":present,"original_unchanged":true,"items":sink.items,"scope":"actual reusable Core workflow, independent golden and physical effects; no predetermined unsupported result"}),
    )
}
pub(crate) fn run(root: &Path, out: &Path) -> Result<()> {
    if std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
        || std::env::var("RUNNER_ENVIRONMENT").as_deref() != Ok("github-hosted")
    {
        return Err("Owned exFAT integration requires a fresh hosted runner; local disk-image privilege remains unrun".into());
    }
    fs::create_dir(out)?;
    let out = out.canonicalize()?;
    let enclosure = xtask::archive::create_artifact_directory(&out)?;
    let image = enclosure.path().join(format!(
        "{}.dmg",
        enclosure
            .path()
            .file_name()
            .ok_or("Missing owned image enclosure")?
            .to_str()
            .ok_or("Invalid owned image enclosure")?
    ));
    let mount = enclosure.path().join("mount");
    fs::create_dir(&mount)?;
    let mut create = std::process::Command::new("/usr/bin/hdiutil");
    create
        .args([
            "create",
            "-size",
            "128m",
            "-fs",
            "ExFAT",
            "-volname",
            "EMLOwnedProbe",
        ])
        .arg(&image);
    macos::checked_run(create, &out.join("create.log"), 60)?;
    let held = fs::File::open(&image)?;
    let identity = held.metadata()?;
    let image = image.canonicalize()?;
    let mut device: Option<String> = None;
    let observation = (|| -> Result<Value> {
        plist(
            root,
            &out,
            "/usr/bin/hdiutil",
            &[
                std::ffi::OsStr::new("attach"),
                std::ffi::OsStr::new("-nobrowse"),
                std::ffi::OsStr::new("-noautoopen"),
                std::ffi::OsStr::new("-mountpoint"),
                mount.as_os_str(),
                std::ffi::OsStr::new("-plist"),
                image.as_os_str(),
            ],
            "attach",
        )?;
        device = association(root, &out, &image, &mount, "attached-association")?;
        if device.is_none() {
            return Err("Owned image attach has no verified device/mount association".into());
        }
        let mounted = fs::symlink_metadata(&mount)?;
        if !mounted.is_dir() {
            return Err("Owned mount is not an ordinary directory".into());
        }
        let primitive = primitive(&mount)?;
        let core = core(enclosure.path(), &mount)?;
        Ok(
            json!({"image":image,"device":device,"mount":mount,"image_identity":{"device":identity.dev(),"inode":identity.ino()},"mount_identity":{"device":mounted.dev(),"inode":mounted.ino(),"uid":mounted.uid(),"gid":mounted.gid()},"primitive":primitive,"core":core}),
        )
    })();
    let cleanup = (|| -> Result<Value> {
        if !controller()?.admits_work() {
            return Err("Disk-image operation lifetime is uncertain; retain image and do not guess a detach target".into());
        }
        let current = fs::symlink_metadata(&image)?;
        if !current.is_file() || (current.dev(), current.ino()) != (identity.dev(), identity.ino())
        {
            return Err("Owned image identity changed before cleanup".into());
        }
        let actual = association(root, &out, &image, &mount, "cleanup-association")?;
        if let Some(actual) = actual {
            if device.as_ref().is_some_and(|device| device != &actual) {
                return Err(
                    "Owned image device reassociated; do not detach an unverified device".into(),
                );
            }
            let mut detach = std::process::Command::new("/usr/bin/hdiutil");
            detach.args(["detach", &actual]);
            macos::checked_run(detach, &out.join("detach.log"), 60)?;
        }
        if association(root, &out, &image, &mount, "detached-association")?.is_some() {
            return Err("Owned image remains associated after detach".into());
        }
        let current = fs::symlink_metadata(&image)?;
        if (current.dev(), current.ino()) != (identity.dev(), identity.ino()) {
            return Err("Image changed before exact owned-file deletion".into());
        }
        fs::remove_file(&image)?;
        Ok(
            json!({"detached":true,"owned_image_removed":true,"device":device,"forced_detach":false}),
        )
    })();
    let record = json!({"source":crate::git(root,&["rev-parse","HEAD"])?,"observation":observation.as_ref().ok(),"probe_error":observation.as_ref().err().map(ToString::to_string),"cleanup":cleanup.as_ref().ok(),"cleanup_error":cleanup.as_ref().err().map(ToString::to_string)});
    fs::write(out.join("result.json"), serde_json::to_vec_pretty(&record)?)?;
    match (observation, cleanup) {
        (Ok(_), Ok(_)) => Ok(()),
        (a, b) => Err(format!(
            "Owned exFAT probe failed: {}; cleanup: {}",
            a.err()
                .map(|e| e.to_string())
                .unwrap_or_else(|| "probe passed".into()),
            b.err()
                .map(|e| e.to_string())
                .unwrap_or_else(|| "complete".into())
        )
        .into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_one_exact_owned_image_mount_and_whole_device_authorize_cleanup() {
        let image = Path::new("/owned/image.dmg");
        let mount = Path::new("/owned/mount");
        let entry = json!({"image-path":"/owned/image.dmg","system-entities":[{"dev-entry":"/dev/disk123"},{"dev-entry":"/dev/disk123s1","mount-point":"/owned/mount"}]});
        assert_eq!(
            select_association(&json!({"images":[entry.clone()]}), image, mount).unwrap(),
            Some("/dev/disk123".into())
        );
        assert_eq!(
            select_association(&json!({"images":[]}), image, mount).unwrap(),
            None
        );
        assert_eq!(select_association(&json!({"images":[{"image-path":"/unrelated/image.dmg","system-entities":[{"dev-entry":"/dev/disk4","mount-point":"/owned/mount"}]}]}),image,mount).unwrap(),None);
        assert!(
            select_association(
                &json!({"images":[entry.clone(),entry.clone()]}),
                image,
                mount
            )
            .is_err()
        );
        assert!(
            select_association(
                &json!({"images":[entry.clone()]}),
                image,
                Path::new("/unrelated/mount")
            )
            .is_err()
        );
        assert!(select_association(&json!({"images":[{"image-path":"/owned/image.dmg","system-entities":[{"dev-entry":"/dev/disk123s1","mount-point":"/owned/mount"}]}]}),image,mount).is_err());
        assert!(select_association(&json!({"images":[{"image-path":"/owned/image.dmg","system-entities":[{"dev-entry":"/dev/disk123"},{"dev-entry":"/dev/disk124","mount-point":"/owned/mount"}]}]}),image,mount).is_err());
    }
}
