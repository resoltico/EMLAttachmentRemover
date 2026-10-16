//! Bounded ZIP directory inspection before the archive library allocates or
//! collapses duplicate records. This is structural evidence, not qualification.
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{self, Cursor, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

/// Shipping archives are bounded independently of advertised expanded sizes.
pub const MAX_ARCHIVE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_DIRECTORY_BYTES: u32 = 8 * 1024 * 1024;
const MAX_ENTRIES: u16 = 4096;
const MAX_NAME_BYTES: usize = 1024;
const MAX_PATH_DEPTH: usize = 8;
const MAX_CLAIM_TEXT_BYTES: usize = 16 * 1024 * 1024;
const MAX_MEMBER_BYTES: u64 = 256 * 1024 * 1024;
const MAX_EXPANDED_BYTES: u64 = 1024 * 1024 * 1024;

/// One immutable bounded copy used by inspection and extraction alike.
pub struct ArchiveSnapshot {
    bytes: Vec<u8>,
}

struct ExtractionRoot {
    path: PathBuf,
    #[cfg(unix)]
    _held: eml_fs::CreatedDirectory,
    #[cfg(unix)]
    _ancestors: Vec<eml_fs::BoundDirectory>,
}

/// Extracted CLI bytes inside a retained private enclosure. Dropping this value
/// closes held descriptors but does not remove useful files or failure evidence.
/// Retain this value until all pathname-based smoke operations settle. Its
/// admitted full-chain policy excludes interference by the trusted UID/root.
pub struct ExtractedCli {
    root: PathBuf,
    _enclosure: ExtractionRoot,
}
impl ExtractedCli {
    /// Package directory beneath the private enclosure.
    pub fn root(&self) -> &Path {
        &self.root
    }
}

#[cfg(unix)]
fn extraction_root(parent: &Path) -> io::Result<ExtractionRoot> {
    let parent = parent.canonicalize()?;
    let mut ancestors = vec![eml_fs::BoundDirectory::open(Path::new("/"))?];
    ancestors[0].admit_namespace()?;
    for component in parent.components() {
        let std::path::Component::Normal(name) = component else {
            continue;
        };
        if ancestors.len() >= 128 {
            return Err(invalid(
                "Private extraction ancestor chain exceeds its bound",
            ));
        }
        let held_parent = ancestors.last().expect("filesystem root retained");
        held_parent.admit_namespace()?;
        let child = held_parent.open_child(name)?;
        child.admit_namespace()?;
        let observed = held_parent.observe_child(name)?;
        if observed.kind != eml_fs::EntryKind::Directory || observed.identity != child.identity() {
            return Err(invalid(
                "Private extraction ancestor name changed while admitting endpoints",
            ));
        }
        ancestors.push(child);
    }
    let held = ancestors.last().expect("filesystem root retained");
    for _ in 0..64 {
        let mut random = [0_u8; 16];
        getrandom::fill(&mut random).map_err(io::Error::other)?;
        let suffix: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
        let name = format!("cli-extracted-{suffix}");
        match held.create_private_child(std::ffi::OsStr::new(&name)) {
            Ok(created) => {
                created.directory().admit_namespace()?;
                let observed = held.observe_child(std::ffi::OsStr::new(&name))?;
                if observed.kind != eml_fs::EntryKind::Directory
                    || observed.identity != created.directory().identity()
                {
                    return Err(invalid(
                        "Private extraction enclosure address is unverified",
                    ));
                }
                return Ok(ExtractionRoot {
                    path: parent.join(name),
                    _held: created,
                    _ancestors: ancestors,
                });
            }
            Err(error) if error.is_name_collision() => continue,
            Err(error) => return Err(io::Error::other(error)),
        }
    }
    Err(io::Error::other(
        "Private extraction name allocation exhausted",
    ))
}

#[cfg(not(unix))]
fn extraction_root(_parent: &Path) -> io::Result<ExtractionRoot> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Private Windows archive extraction requires an observed protected parent/root ACL; native qualification is unavailable",
    ))
}

impl ArchiveSnapshot {
    /// Read a snapshot without trusting an advertised file length for allocation.
    pub fn read(path: &Path) -> io::Result<Self> {
        let file = File::open(path)?;
        if !file.metadata()?.is_file() {
            return Err(invalid("Archive input must be an ordinary file"));
        }
        let mut bytes = Vec::new();
        file.take(MAX_ARCHIVE_BYTES + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_ARCHIVE_BYTES {
            return Err(invalid("Archive snapshot exceeds its bound"));
        }
        Ok(Self { bytes })
    }

    /// Extract this CLI ZIP profile into a fresh private directory, retaining
    /// partial evidence on failure. Material paths and bytes come from the
    /// independently generated target notice/README inputs, not the archive.
    /// This checks CRCs and exact inventory, but does not execute the CLI.
    /// Unix creation uses eml-fs namespace admission and private ACL/mode rules.
    /// Windows currently refuses pending its protected-root ACL implementation.
    pub fn extract_cli_zip(
        &self,
        root_name: &str,
        executable: &str,
        materials: &BTreeMap<String, Vec<u8>>,
        parent: &Path,
    ) -> io::Result<ExtractedCli> {
        for name in [root_name, executable] {
            validate_member_name(name)?;
            if name.contains('/') {
                return Err(invalid("CLI root/executable must be one component"));
            }
        }
        for name in [
            "README.txt",
            "LICENSE",
            "THIRD-PARTY-NOTICES.txt",
            "licenses/rust/COPYRIGHT-library.html",
            "licenses/rust/MIT.txt",
            "licenses/rust/Apache-2.0.txt",
            "licenses/rust/BSD-2-Clause.txt",
            "licenses/rust/Unicode-3.0.txt",
            "licenses/rust/source.json",
        ] {
            if !materials.get(name).is_some_and(|bytes| !bytes.is_empty()) {
                return Err(invalid("Expected CLI material inventory is incomplete"));
            }
        }
        let mut files = BTreeSet::from([format!("{root_name}/{executable}")]);
        for (name, bytes) in materials {
            validate_member_name(name)?;
            if name.ends_with('/')
                || name == executable
                || bytes.is_empty()
                || (!matches!(
                    name.as_str(),
                    "README.txt" | "LICENSE" | "THIRD-PARTY-NOTICES.txt"
                ) && !name.starts_with("licenses/"))
            {
                return Err(invalid("Invalid expected CLI material path or bytes"));
            }
            files.insert(format!("{root_name}/{name}"));
        }
        let mut directories = BTreeSet::new();
        for name in &files {
            let mut ancestor = name.as_str();
            while let Some((parent, _)) = ancestor.rsplit_once('/') {
                directories.insert(format!("{parent}/"));
                ancestor = parent;
            }
        }
        let members = inspect_zip_directory(&mut Cursor::new(&self.bytes))?;
        let mut observed = BTreeSet::new();
        let mut expanded = 0_u64;
        for member in &members {
            if member.name.ends_with('/') {
                if !directories.contains(&member.name) {
                    return Err(invalid("Unexpected CLI archive directory"));
                }
            } else if !files.contains(&member.name) || !observed.insert(member.name.clone()) {
                return Err(invalid("Unexpected CLI archive file"));
            }
            if u64::from(member.size) > MAX_MEMBER_BYTES {
                return Err(invalid("CLI archive member exceeds its bound"));
            }
            expanded += u64::from(member.size);
            if expanded > MAX_EXPANDED_BYTES {
                return Err(invalid("CLI archive expanded size exceeds its bound"));
            }
        }
        if observed != files {
            return Err(invalid("CLI archive is missing expected files"));
        }
        let mut archive = zip::ZipArchive::new(Cursor::new(&self.bytes))?;
        if archive.len() != members.len() || archive.has_overlapping_entries()? {
            return Err(invalid("ZIP library readback differs or overlaps"));
        }
        let enclosure = extraction_root(parent)?;
        let extracted = &enclosure.path;
        eprintln!("Retained owned CLI extraction: {}", extracted.display());
        for (index, member) in members.iter().enumerate() {
            let mut entry = archive.by_index(index)?;
            if entry.name_raw() != member.name.as_bytes() || entry.size() != u64::from(member.size)
            {
                return Err(invalid("ZIP member readback differs from original record"));
            }
            let directory = member.name.ends_with('/');
            let permissions = if directory || member.name == format!("{root_name}/{executable}") {
                0o755
            } else {
                0o644
            };
            if entry.unix_mode().map(|mode| mode & 0o7777) != Some(permissions) {
                return Err(invalid(
                    "CLI archive member permissions differ from fixed package policy",
                ));
            }
            let path = extracted.join(member.name.trim_end_matches('/'));
            if directory {
                fs::create_dir_all(&path)?;
                let mut contents = Vec::new();
                entry.read_to_end(&mut contents)?;
                if !contents.is_empty() {
                    return Err(invalid("Archive directory contains data"));
                }
                continue;
            }
            fs::create_dir_all(
                path.parent()
                    .ok_or_else(|| invalid("Missing extraction parent"))?,
            )?;
            let mut output = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)?;
            let count = io::copy(&mut entry.take(MAX_MEMBER_BYTES + 1), &mut output)?;
            if count != u64::from(member.size) {
                return Err(invalid("ZIP member actual size differs"));
            }
            output.flush()?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                output.set_permissions(fs::Permissions::from_mode(permissions))?;
            }
            drop(output);
            let relative = member
                .name
                .strip_prefix(&format!("{root_name}/"))
                .ok_or_else(|| invalid("Invalid CLI archive root"))?;
            if let Some(expected) = materials.get(relative)
                && fs::read(&path)? != *expected
            {
                return Err(invalid(
                    "Extracted CLI material bytes differ from reviewed inputs",
                ));
            }
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for directory in directories {
                fs::set_permissions(
                    extracted.join(directory.trim_end_matches('/')),
                    fs::Permissions::from_mode(0o755),
                )?;
            }
        }
        Ok(ExtractedCli {
            root: extracted.join(root_name),
            _enclosure: enclosure,
        })
    }
}

#[derive(Default)]
struct PathClaims {
    by_name: BTreeMap<String, (bool, String)>,
    text_bytes: usize,
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
fn short(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}
fn long(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().expect("fixed header field"))
}

/// One original central-directory record, before any filename-keyed map.
#[derive(Debug, PartialEq, Eq)]
pub struct ZipMember {
    /// Exact validated UTF-8 archive spelling, including a directory's slash.
    pub name: String,
    /// Uncompressed data size advertised by the central record.
    pub size: u32,
    /// Compressed data size advertised by the central record.
    pub compressed_size: u32,
    /// Local-header offset in this single-disk archive.
    pub local_header: u32,
}

/// Inspect the classic single-disk, uncommented ZIP format used by CLI packages.
/// Reject ZIP64 rather than allowing unbounded metadata before validation.
/// The reader must refer to one stable owned archive throughout later readback.
/// No filesystem objects are extracted here and data CRCs are checked separately.
pub fn inspect_zip_directory(reader: &mut (impl Read + Seek)) -> io::Result<Vec<ZipMember>> {
    let length = reader.seek(SeekFrom::End(0))?;
    if !(22..=MAX_ARCHIVE_BYTES).contains(&length) {
        return Err(invalid("ZIP archive size is outside the supported bound"));
    }
    reader.seek(SeekFrom::End(-22))?;
    let mut end = [0_u8; 22];
    reader.read_exact(&mut end)?;
    if end[..4] != *b"PK\x05\x06"
        || short(&end, 4) != 0
        || short(&end, 6) != 0
        || short(&end, 8) != short(&end, 10)
        || short(&end, 20) != 0
    {
        return Err(invalid(
            "Expected one uncommented single-disk ZIP directory",
        ));
    }
    let count = short(&end, 10);
    let size = long(&end, 12);
    let offset = long(&end, 16);
    if count == 0
        || count > MAX_ENTRIES
        || size > MAX_DIRECTORY_BYTES
        || u64::from(offset) + u64::from(size) != length - 22
        || u64::from(count) * 46 > u64::from(size)
    {
        return Err(invalid("ZIP directory count, size or extent is invalid"));
    }
    reader.seek(SeekFrom::Start(u64::from(offset)))?;
    let mut directory = vec![0_u8; size as usize];
    reader.read_exact(&mut directory)?;
    let mut cursor = 0_usize;
    let mut names = BTreeSet::new();
    let mut claims = PathClaims::default();
    let mut members = Vec::with_capacity(usize::from(count));
    let mut regions = Vec::with_capacity(usize::from(count));
    for _ in 0..count {
        let header = directory
            .get(cursor..cursor.saturating_add(46))
            .ok_or_else(|| invalid("Truncated ZIP central header"))?;
        if header[..4] != *b"PK\x01\x02" || short(header, 34) != 0 {
            return Err(invalid("Unsupported ZIP central header or disk"));
        }
        // The seekable CLI writer records final sizes in both headers, without
        // data descriptors. Apple's ditto profile is reviewed separately.
        let flags = short(header, 8);
        if flags & !0x0800 != 0 || !matches!(short(header, 10), 0 | 8) {
            return Err(invalid("Unsupported ZIP flags or compression"));
        }
        let name_length = usize::from(short(header, 28));
        let extra_length = usize::from(short(header, 30));
        let comment_length = usize::from(short(header, 32));
        if name_length == 0 || name_length > MAX_NAME_BYTES || comment_length != 0 {
            return Err(invalid("Unsupported ZIP member name or comment"));
        }
        let record_end = cursor
            .checked_add(46 + name_length + extra_length + comment_length)
            .ok_or_else(|| invalid("ZIP record extent overflow"))?;
        if record_end > directory.len() {
            return Err(invalid("Truncated ZIP central record"));
        }
        let name = std::str::from_utf8(&directory[cursor + 46..cursor + 46 + name_length])
            .map_err(|_| invalid("ZIP member name is not UTF-8"))?;
        validate_member_name(name)?;
        // Check original records here: ZipArchive's filename map discards
        // earlier identical names, so checking its iterator would be too late.
        if !names.insert(name.trim_end_matches('/').to_ascii_lowercase()) {
            return Err(invalid("Duplicate original ZIP member name"));
        }
        let directory_entry = name.ends_with('/');
        claim_path(&mut claims, name, directory_entry)?;
        let mode = long(header, 38) >> 16;
        let expected_type = if directory_entry { 0o040000 } else { 0o100000 };
        if header[5] != 3 || mode & 0o170000 != expected_type || mode & 0o7000 != 0 {
            return Err(invalid(
                "ZIP member must have an ordinary Unix file/directory type",
            ));
        }
        let size = long(header, 24);
        let compressed_size = long(header, 20);
        let local_header = long(header, 42);
        if [size, compressed_size, local_header].contains(&u32::MAX)
            || u64::from(local_header) + 30 > u64::from(offset)
            || u64::from(compressed_size) > MAX_ARCHIVE_BYTES
        {
            return Err(invalid("Unsupported ZIP64 or invalid local member extent"));
        }
        if directory_entry && (size != 0 || compressed_size != 0) {
            return Err(invalid("ZIP directory must have no payload"));
        }
        reader.seek(SeekFrom::Start(u64::from(local_header)))?;
        let mut local = [0_u8; 30];
        reader.read_exact(&mut local)?;
        if local[..4] != *b"PK\x03\x04"
            || local[4..26] != header[6..28]
            || usize::from(short(&local, 26)) != name_length
        {
            return Err(invalid("ZIP local and central headers disagree"));
        }
        let local_extra_length = u64::from(short(&local, 28));
        let data_start = u64::from(local_header) + 30 + name_length as u64 + local_extra_length;
        let data_end = data_start + u64::from(compressed_size);
        if data_end > u64::from(offset) {
            return Err(invalid("ZIP member overlaps central directory"));
        }
        let mut local_name = vec![0; name_length];
        reader.read_exact(&mut local_name)?;
        if local_name != name.as_bytes() {
            return Err(invalid("ZIP local and central member names disagree"));
        }
        if local_extra_length != 0 || extra_length != 0 {
            return Err(invalid("CLI ZIP metadata extensions are unsupported"));
        }
        regions.push((u64::from(local_header), data_end));
        members.push(ZipMember {
            name: name.to_owned(),
            size,
            compressed_size,
            local_header,
        });
        cursor = record_end;
    }
    if cursor != directory.len() {
        return Err(invalid("Unaccounted ZIP central-directory bytes"));
    }
    regions.sort_unstable();
    let mut previous_end = 0;
    for (start, end) in regions {
        if start != previous_end {
            return Err(invalid(
                "ZIP local entries overlap or leave unaccounted bytes",
            ));
        }
        previous_end = end;
    }
    if previous_end != u64::from(offset) {
        return Err(invalid("Unaccounted bytes before ZIP central directory"));
    }
    Ok(members)
}

fn claim_path(claims: &mut PathClaims, name: &str, directory: bool) -> io::Result<()> {
    let components: Vec<_> = name.trim_end_matches('/').split('/').collect();
    if components.len() > MAX_PATH_DEPTH {
        return Err(invalid("Archive package path is too deep"));
    }
    let mut path = String::new();
    for (index, component) in components.iter().enumerate() {
        if index != 0 {
            path.push('/');
        }
        path.push_str(component);
        let is_directory = index + 1 != components.len() || directory;
        let key = path.to_ascii_lowercase();
        if let Some((prior_directory, spelling)) = claims.by_name.get(&key) {
            if !prior_directory || !is_directory || spelling != &path {
                return Err(invalid("Archive file/directory claims collide"));
            }
        } else {
            let text_bytes = claims
                .text_bytes
                .checked_add(key.len() + path.len())
                .ok_or_else(|| invalid("Archive path claim budget overflow"))?;
            if text_bytes > MAX_CLAIM_TEXT_BYTES {
                return Err(invalid("Archive path claim text exceeds its bound"));
            }
            claims.by_name.insert(key, (is_directory, path.clone()));
            claims.text_bytes = text_bytes;
        }
    }
    Ok(())
}

/// Require a relative ASCII package path with no platform-dependent aliases.
pub fn validate_member_name(name: &str) -> io::Result<()> {
    if name.is_empty()
        || name.len() > MAX_NAME_BYTES
        || !name.is_ascii()
        || name.contains(['\\', '\0', ':', '<', '>', '"', '|', '?', '*'])
        || name.starts_with('/')
    {
        return Err(invalid("Invalid archive member spelling"));
    }
    let path = name.strip_suffix('/').unwrap_or(name);
    if path.split('/').any(|part| {
        part.is_empty()
            || matches!(part, "." | "..")
            || part.ends_with(['.', ' '])
            || part.chars().any(char::is_control)
            || reserved_windows_component(part)
    }) {
        return Err(invalid("Invalid archive path component"));
    }
    Ok(())
}

fn reserved_windows_component(component: &str) -> bool {
    let base = component
        .split('.')
        .next()
        .unwrap_or("")
        .to_ascii_uppercase();
    matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (base.len() == 4
            && (base.starts_with("COM") || base.starts_with("LPT"))
            && matches!(base.as_bytes()[3], b'1'..=b'9'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};

    fn archive() -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .start_file(
                "package/README.txt",
                zip::write::SimpleFileOptions::default().system(zip::System::Unix),
            )
            .unwrap();
        writer.write_all(b"ordinary owned fixture").unwrap();
        writer.finish().unwrap().into_inner()
    }

    fn synthetic_materials() -> BTreeMap<String, Vec<u8>> {
        [
            "README.txt",
            "LICENSE",
            "THIRD-PARTY-NOTICES.txt",
            "licenses/rust/COPYRIGHT-library.html",
            "licenses/rust/MIT.txt",
            "licenses/rust/Apache-2.0.txt",
            "licenses/rust/BSD-2-Clause.txt",
            "licenses/rust/Unicode-3.0.txt",
            "licenses/rust/source.json",
        ]
        .into_iter()
        .map(|name| {
            (
                name.to_owned(),
                format!("synthetic test material for {name}\n").into_bytes(),
            )
        })
        .collect()
    }

    fn synthetic_cli(materials: &BTreeMap<String, Vec<u8>>) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, bytes) in
            std::iter::once(("tool", &b"synthetic executable fixture; never invoked"[..])).chain(
                materials
                    .iter()
                    .map(|(name, bytes)| (name.as_str(), bytes.as_slice())),
            )
        {
            let options = zip::write::SimpleFileOptions::default()
                .system(zip::System::Unix)
                .compression_method(zip::CompressionMethod::Stored)
                .unix_permissions(if name == "tool" { 0o755 } else { 0o644 });
            writer
                .start_file(format!("package/{name}"), options)
                .unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    #[test]
    #[cfg(unix)]
    fn immutable_cli_snapshot_extracts_exact_owned_materials_and_modes() {
        let owned = tempfile::tempdir().unwrap();
        let materials = synthetic_materials();
        let path = owned.path().join("source.zip");
        fs::write(&path, synthetic_cli(&materials)).unwrap();
        let snapshot = ArchiveSnapshot::read(&path).unwrap();
        fs::write(&path, b"changed owned archive after snapshot").unwrap();
        let extracted = snapshot
            .extract_cli_zip("package", "tool", &materials, owned.path())
            .unwrap();
        for (name, bytes) in materials {
            assert_eq!(fs::read(extracted.root().join(name)).unwrap(), bytes);
        }
        assert_eq!(
            fs::read(extracted.root().join("tool")).unwrap(),
            b"synthetic executable fixture; never invoked"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(extracted.root().join("tool"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o755
            );
            assert_eq!(
                fs::metadata(extracted.root().join("LICENSE"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o644
            );
            assert_eq!(
                fs::metadata(extracted.root()).unwrap().permissions().mode() & 0o777,
                0o755
            );
            assert_eq!(
                fs::metadata(extracted.root().parent().unwrap())
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
        }
    }

    #[test]
    fn missing_required_material_is_rejected_before_extraction() {
        let owned = tempfile::tempdir().unwrap();
        let materials = synthetic_materials();
        let mut missing = materials.clone();
        missing.remove("licenses/rust/COPYRIGHT-library.html");
        let snapshot = ArchiveSnapshot {
            bytes: synthetic_cli(&missing),
        };
        assert!(
            snapshot
                .extract_cli_zip("package", "tool", &materials, owned.path())
                .is_err()
        );
        assert_eq!(fs::read_dir(owned.path()).unwrap().count(), 0);
        // An incomplete expected inventory cannot redefine completeness either.
        assert!(
            snapshot
                .extract_cli_zip("package", "tool", &missing, owned.path())
                .is_err()
        );
        assert_eq!(fs::read_dir(owned.path()).unwrap().count(), 0);
    }

    #[test]
    #[cfg(unix)]
    fn writable_nonsticky_upper_ancestor_is_refused_before_extraction() {
        use std::os::unix::fs::PermissionsExt;
        let owned = tempfile::tempdir().unwrap();
        let upper = owned.path().join("shared");
        let parent = upper.join("private");
        fs::create_dir(&upper).unwrap();
        fs::create_dir(&parent).unwrap();
        fs::set_permissions(&upper, fs::Permissions::from_mode(0o777)).unwrap();
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o700)).unwrap();
        let materials = synthetic_materials();
        let snapshot = ArchiveSnapshot {
            bytes: synthetic_cli(&materials),
        };
        let result = snapshot.extract_cli_zip("package", "tool", &materials, &parent);
        fs::set_permissions(&upper, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(result.is_err());
        assert_eq!(fs::read_dir(&parent).unwrap().count(), 0);
    }

    #[test]
    #[cfg(unix)]
    fn crc_and_material_byte_changes_fail_real_readback() {
        let materials = synthetic_materials();
        let mut bytes = synthetic_cli(&materials);
        let name_length = usize::from(short(&bytes, 26));
        bytes[30 + name_length] ^= 1; // Change stored payload without updating its CRC.
        let owned = tempfile::tempdir().unwrap();
        let snapshot = ArchiveSnapshot { bytes };
        assert!(
            snapshot
                .extract_cli_zip("package", "tool", &materials, owned.path())
                .is_err()
        );

        let mut different = materials.clone();
        different.insert(
            "LICENSE".into(),
            b"different owned synthetic material".to_vec(),
        );
        let snapshot = ArchiveSnapshot {
            bytes: synthetic_cli(&different),
        };
        assert!(
            snapshot
                .extract_cli_zip("package", "tool", &materials, owned.path())
                .is_err()
        );
        assert_eq!(fs::read_dir(owned.path()).unwrap().count(), 2); // Useful failed extraction evidence retained.
    }

    #[test]
    #[cfg(windows)]
    fn windows_extraction_requires_protected_acl_evidence() {
        let owned = tempfile::tempdir().unwrap();
        let materials = synthetic_materials();
        let snapshot = ArchiveSnapshot {
            bytes: synthetic_cli(&materials),
        };
        let result = snapshot.extract_cli_zip("package", "tool", &materials, owned.path());
        assert_eq!(result.err().unwrap().kind(), io::ErrorKind::Unsupported);
        assert_eq!(fs::read_dir(owned.path()).unwrap().count(), 0);
    }

    #[test]
    fn original_directory_is_bounded_and_preserves_each_record() {
        let bytes = archive();
        let members = inspect_zip_directory(&mut Cursor::new(&bytes)).unwrap();
        assert_eq!(members.len(), 1);
        assert_eq!(members[0].name, "package/README.txt");
        assert_eq!(members[0].size, 22);
    }

    #[test]
    fn duplicates_are_rejected_before_filename_map_collapses_them() {
        let mut bytes = archive();
        let end = bytes.len() - 22;
        let offset = long(&bytes[end..], 16) as usize;
        let central = bytes[offset..end].to_vec();
        bytes.splice(end..end, central.iter().copied());
        let end = bytes.len() - 22;
        bytes[end + 8..end + 12].copy_from_slice(&[2, 0, 2, 0]);
        bytes[end + 12..end + 16]
            .copy_from_slice(&u32::try_from(central.len() * 2).unwrap().to_le_bytes());
        // The dependency accepts this and exposes one entry; the original
        // directory guard must reject the two records rather than count one.
        assert_eq!(zip::ZipArchive::new(Cursor::new(&bytes)).unwrap().len(), 1);
        assert!(inspect_zip_directory(&mut Cursor::new(bytes)).is_err());
    }

    #[test]
    fn invalid_directory_extents_and_unsupported_zip_features_fail() {
        let original = archive();
        for (field, value) in [(4, 1), (10, u16::MAX), (20, 1)] {
            let mut bytes = original.clone();
            let end = bytes.len() - 22;
            bytes[end + field..end + field + 2].copy_from_slice(&value.to_le_bytes());
            assert!(inspect_zip_directory(&mut Cursor::new(bytes)).is_err());
        }
        for count in [0, 1, 21] {
            assert!(inspect_zip_directory(&mut Cursor::new(vec![0; count])).is_err());
        }
    }

    #[test]
    fn local_header_disagreement_and_special_file_types_fail() {
        let original = archive();
        let central = long(&original[original.len() - 22..], 16) as usize;
        // Local flags, method, CRC, size, and name each disagree independently.
        for field in [6, 8, 14, 22, 30] {
            let mut bytes = original.clone();
            bytes[field] ^= 1;
            assert!(inspect_zip_directory(&mut Cursor::new(bytes)).is_err());
        }
        for mode in [0o120777_u32, 0o104755, 0o040755] {
            let mut bytes = original.clone();
            bytes[central + 38..central + 42].copy_from_slice(&(mode << 16).to_le_bytes());
            assert!(inspect_zip_directory(&mut Cursor::new(bytes)).is_err());
        }
    }

    #[test]
    fn path_claims_reject_ancestors_and_case_aliases_in_either_order() {
        for paths in [
            [("package/a", false), ("package/a/b", false)],
            [("package/a/b", false), ("package/a", false)],
            [("package/A/b", false), ("package/a/c", false)],
        ] {
            let mut claims = PathClaims::default();
            claim_path(&mut claims, paths[0].0, paths[0].1).unwrap();
            assert!(claim_path(&mut claims, paths[1].0, paths[1].1).is_err());
        }
        let mut claims = PathClaims::default();
        claim_path(&mut claims, "package/a/b", false).unwrap();
        claim_path(&mut claims, "package/a/", true).unwrap();
        claim_path(&mut claims, "package/a/c", false).unwrap();
    }

    #[test]
    fn path_claim_depth_and_text_budgets_reject_before_insertion() {
        let mut claims = PathClaims::default();
        let at_depth = ["a"; MAX_PATH_DEPTH].join("/");
        claim_path(&mut claims, &at_depth, false).unwrap();
        let before = (claims.by_name.len(), claims.text_bytes);
        assert!(claim_path(&mut claims, &format!("{at_depth}/b"), false).is_err());
        assert_eq!((claims.by_name.len(), claims.text_bytes), before);

        let mut claims = PathClaims {
            text_bytes: MAX_CLAIM_TEXT_BYTES - 2,
            ..PathClaims::default()
        };
        claim_path(&mut claims, "a/", true).unwrap();
        assert_eq!(claims.text_bytes, MAX_CLAIM_TEXT_BYTES);
        assert!(claim_path(&mut claims, "b", false).is_err());
        assert_eq!(claims.text_bytes, MAX_CLAIM_TEXT_BYTES);
        assert_eq!(claims.by_name.len(), 1);
    }

    #[test]
    fn reused_directory_claims_are_not_charged_twice() {
        let mut claims = PathClaims::default();
        claim_path(&mut claims, "root/licenses/MIT.txt", false).unwrap();
        let expected = 2 * ("root".len() + "root/licenses".len() + "root/licenses/MIT.txt".len());
        assert_eq!(claims.text_bytes, expected);
        claim_path(&mut claims, "root/", true).unwrap();
        claim_path(&mut claims, "root/licenses/", true).unwrap();
        assert_eq!(claims.text_bytes, expected);
    }

    #[test]
    fn package_paths_reject_ambiguous_components() {
        for path in [
            "",
            "/a",
            "../a",
            "a/../b",
            "a//b",
            "a/./b",
            "a\\b",
            "C:a",
            "a\0b",
            "a./b",
            "a /b",
            "a\nb",
            "package/NUL.txt",
            "package/com1",
            "package/LPT9.log",
            "package/é",
            "package/a<b",
            "package/a>b",
            "package/a\"b",
            "package/a|b",
            "package/a?b",
            "package/a*b",
        ] {
            assert!(validate_member_name(path).is_err(), "{path:?}");
        }
        for path in [
            "package/",
            "package/licenses/rust/COPYRIGHT-library.html",
            "EML Attachment Remover.app/Contents/Info.plist",
        ] {
            validate_member_name(path).unwrap();
        }
    }
}
