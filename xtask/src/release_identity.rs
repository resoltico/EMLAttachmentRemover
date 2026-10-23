//! Read-only product revision comparison against caller-supplied published metadata.
//! This checks ordering and product identity, not authenticity or publication authority.
use crate::{Result, capture};
use base64::Engine as _;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{fs, io::Read, path::Path, process::Command};
use xtask::archive::ByteIdentity;

const MAX_INFO_BYTES: u64 = 16 * 1024;
// Decode the immutable bounded snapshot, not a second read of the caller's path.
// dict_type rejects duplicate XML or binary plist keys before they can collapse.
const DECODE_INFO: &str = r#"import base64,json,plistlib,sys
class Unique(dict):
 def __setitem__(self,key,value):
  if key in self: raise ValueError('Duplicate plist dictionary key')
  super().__setitem__(key,value)
p=plistlib.loads(base64.b64decode(sys.argv[1],validate=True),dict_type=Unique)
if not isinstance(p,dict): raise ValueError('Info.plist root must be a dictionary')
keys=['CFBundleIdentifier','CFBundleName','CFBundleExecutable','CFBundlePackageType','CFBundleShortVersionString','CFBundleVersion']
print(json.dumps({k:p.get(k) for k in keys},separators=(',',':')))
"#;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PublishedIdentity {
    #[serde(rename = "CFBundleIdentifier")]
    identifier: String,
    #[serde(rename = "CFBundleName")]
    name: String,
    #[serde(rename = "CFBundleExecutable")]
    executable: String,
    #[serde(rename = "CFBundlePackageType")]
    package_type: String,
    #[serde(rename = "CFBundleShortVersionString")]
    version: String,
    #[serde(rename = "CFBundleVersion")]
    build: String,
}

/// Compare current source identity with prior published app metadata supplied by
/// the owner. The owner must obtain that file from an independently authenticated
/// prior archive. This command neither authenticates that input nor authorizes a
/// distribution, and cannot establish whether product code is unchanged.
pub(crate) fn check(
    previous_info: &Path,
    current_version: &str,
    current_build: u16,
) -> Result<Value> {
    if !fs::symlink_metadata(previous_info)?.is_file() {
        return Err("Previous Info.plist must be an ordinary non-symlink file".into());
    }
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(
            (rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits() as i32,
        );
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x0020_0000); // FILE_FLAG_OPEN_REPARSE_POINT: observe the leaf itself.
    }
    let file = options.open(previous_info)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > MAX_INFO_BYTES {
        return Err("Previous Info.plist must be a bounded ordinary file (at most 16 KiB)".into());
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err("Previous Info.plist cannot be a reparse point".into());
        }
    }
    let mut bytes = Vec::new();
    file.take(MAX_INFO_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.is_empty() || bytes.len() as u64 > MAX_INFO_BYTES {
        return Err("Previous Info.plist is empty or exceeds 16 KiB".into());
    }
    let decoded = capture(Command::new("python3").args([
        "-c",
        DECODE_INFO,
        &base64::engine::general_purpose::STANDARD.encode(&bytes),
    ]))?;
    let identity: PublishedIdentity = serde_json::from_slice(&decoded)?;
    compare(&identity, current_version, current_build)?;
    Ok(json!({
        "schema":1,
        "passed":true,
        "previous_info":ByteIdentity::of(&bytes),
        "previous":{"version":identity.version,"build":identity.build},
        "current":{"version":current_version,"build":current_build},
        "trust_scope":"Caller-supplied prior Info.plist; this comparison does not authenticate its archive or publication provenance.",
        "scope":"Product identity and monotonic version/build ordering only; equal identity does not prove unchanged product code or authorize distribution."
    }))
}

fn compare(previous: &PublishedIdentity, current_version: &str, current_build: u16) -> Result<()> {
    if previous.identifier != "io.github.resoltico.emlattachmentremover"
        || previous.name != eml_core::product::DISPLAY_NAME
        || previous.executable != "EMLAttachmentRemover"
        || previous.package_type != "APPL"
    {
        return Err("Previous metadata does not identify the EML Attachment Remover app".into());
    }
    let before = version(&previous.version)?;
    let after = version(current_version)?;
    let before_build = revision(&previous.build)?;
    if !(1..=9999).contains(&current_build) {
        return Err("Current product build must be in 1..=9999".into());
    }
    if after < before || current_build < before_build {
        return Err(
            "Product version and build must not decrease from prior published metadata".into(),
        );
    }
    if after != before && current_build <= before_build {
        return Err(
            "A changed product version requires a greater product build; builds never reset".into(),
        );
    }
    Ok(())
}

fn revision(text: &str) -> Result<u16> {
    if text.is_empty()
        || text.len() > 4
        || text.starts_with('0')
        || !text.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(
            "Published product build must be a canonical decimal string in 1..=9999".into(),
        );
    }
    let number = text.parse::<u16>()?;
    if !(1..=9999).contains(&number) {
        return Err("Published product build must be in 1..=9999".into());
    }
    Ok(number)
}

// CFBundleShortVersionString is the app's three-number released-version field.
// This deliberately does not interpret a source SHA, CI run, or prerelease label.
fn version(text: &str) -> Result<[u32; 3]> {
    let pieces: Vec<_> = text.split('.').collect();
    if pieces.len() != 3 {
        return Err("Released app version must have three canonical decimal components; prerelease forms are unsupported".into());
    }
    let mut result = [0; 3];
    for (index, piece) in pieces.into_iter().enumerate() {
        if piece.is_empty()
            || piece.len() > 10
            || (piece.len() > 1 && piece.starts_with('0'))
            || !piece.bytes().all(|b| b.is_ascii_digit())
        {
            return Err("Released app version must have three canonical decimal components; prerelease forms are unsupported".into());
        }
        result[index] = piece.parse()?;
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn released() -> PublishedIdentity {
        // These six literal fields are from the authenticated v6 app inspected in
        // the published-package audit, not derived from the current constants.
        serde_json::from_value(json!({"CFBundleIdentifier":"io.github.resoltico.emlattachmentremover","CFBundleName":"EML Attachment Remover","CFBundleExecutable":"EMLAttachmentRemover","CFBundlePackageType":"APPL","CFBundleShortVersionString":"6.0.0","CFBundleVersion":"35"})).unwrap()
    }
    #[test]
    fn released_build_35_to_staged_build_36_and_same_identity_rebuild_are_valid() {
        assert!(compare(&released(), "7.0.0", 36).is_ok());
        assert!(compare(&released(), "6.0.0", 35).is_ok());
        assert!(compare(&released(), "6.0.0", 36).is_ok());
    }
    #[test]
    fn lower_version_build_reset_and_version_change_without_increment_are_refused() {
        for (version, build) in [
            ("5.9.9", 36),
            ("7.0.0", 1),
            ("7.0.0", 35),
            ("6.0.0", 34),
            ("6.0.0", 0),
            ("7.0.0", 10_000),
        ] {
            assert!(
                compare(&released(), version, build).is_err(),
                "{version}/{build}"
            );
        }
    }
    #[test]
    fn wrong_product_malformed_versions_and_build_types_are_refused() {
        for field in [
            "CFBundleIdentifier",
            "CFBundleName",
            "CFBundleExecutable",
            "CFBundlePackageType",
        ] {
            let mut value = json!({"CFBundleIdentifier":"io.github.resoltico.emlattachmentremover","CFBundleName":"EML Attachment Remover","CFBundleExecutable":"EMLAttachmentRemover","CFBundlePackageType":"APPL","CFBundleShortVersionString":"6.0.0","CFBundleVersion":"35"});
            value[field] = json!("another product");
            assert!(compare(&serde_json::from_value(value).unwrap(), "7.0.0", 36).is_err());
        }
        for invalid in [
            "", "0", "00", "035", "10000", "-35", "35.0", " 35", "+35", "٣٥",
        ] {
            assert!(revision(invalid).is_err(), "{invalid}");
        }
        for invalid in [
            "",
            "6",
            "6.0",
            "06.0.0",
            "6.0.0.0",
            "6.0.0-beta",
            "6.0.0+meta",
            "6.0.-1",
            "6.0.4294967296",
        ] {
            assert!(version(invalid).is_err(), "{invalid}");
        }
        let mut value = json!({"CFBundleIdentifier":"io.github.resoltico.emlattachmentremover","CFBundleName":"EML Attachment Remover","CFBundleExecutable":"EMLAttachmentRemover","CFBundlePackageType":"APPL","CFBundleShortVersionString":"6.0.0","CFBundleVersion":35});
        assert!(serde_json::from_value::<PublishedIdentity>(value.clone()).is_err());
        value["CFBundleVersion"] = Value::Null;
        assert!(serde_json::from_value::<PublishedIdentity>(value).is_err());
    }
}
