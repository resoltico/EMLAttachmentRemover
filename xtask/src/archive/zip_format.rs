//! Original-record ZIP parsing and immutable-snapshot ZIP operations.
use super::*;

impl ArchiveSnapshot {
    /// Inspect the finite native ditto app profile, including every CRC, exact
    /// expected file identity and mode. References come from the frozen producer;
    /// hashes prove identity, not review quality. This does not execute the app.
    /// Expected keys are paths relative to the one app root; modes are 0644/0755.
    /// DOS and native Unix fields must equal the source epoch rounded down to
    /// two seconds. The native producer stages those times and runs in UTC.
    pub fn inspect_ditto_app(
        &self,
        root_name: &str,
        expected: &BTreeMap<String, (ByteIdentity, u32)>,
        epoch: PackageEpoch,
    ) -> io::Result<()> {
        validate_member_name(root_name)?;
        if root_name.contains('/')
            || expected.is_empty()
            || expected.len() > usize::from(MAX_ENTRIES)
        {
            return Err(invalid("Invalid expected app root or file inventory"));
        }
        let mut files = BTreeMap::new();
        let mut directories = BTreeSet::new();
        for (relative, (identity, mode)) in expected {
            let name = format!("{root_name}/{relative}");
            validate_member_name(&name)?;
            if relative.ends_with('/')
                || !matches!(*mode, 0o644 | 0o755)
                || identity.byte_length as u64 > MAX_MEMBER_BYTES
            {
                return Err(invalid("Invalid expected app file or mode"));
            }
            let mut remaining = name.as_str();
            while let Some((parent, _)) = remaining.rsplit_once('/') {
                directories.insert(format!("{parent}/"));
                remaining = parent;
            }
            files.insert(name, (identity, mode));
        }
        if files.len() + directories.len() > usize::from(MAX_ENTRIES) {
            return Err(invalid("App inventory exceeds its entry bound"));
        }
        let members =
            inspect_zip_profile(&mut Cursor::new(&self.bytes), ZipProfile::DittoApp, epoch)?;
        if members.len() != files.len() + directories.len() {
            return Err(invalid(
                "App archive inventory differs from expected files/directories",
            ));
        }
        let mut archive = zip::ZipArchive::new(Cursor::new(&self.bytes))?;
        if archive.len() != members.len() || archive.has_overlapping_entries()? {
            return Err(invalid(
                "App ZIP library view differs from original records",
            ));
        }
        for index in 0..archive.len() {
            let entry = archive.by_index(index)?;
            let name = entry.name()?.into_owned();
            let mode = entry
                .unix_mode()
                .ok_or_else(|| invalid("App entry lacks Unix mode"))?
                & 0o7777;
            if name.ends_with('/') {
                if !directories.remove(&name) || mode != 0o755 {
                    return Err(invalid("Unexpected app directory or permissions"));
                }
            } else {
                let (expected, expected_mode) = files
                    .remove(&name)
                    .ok_or_else(|| invalid("Unexpected app file"))?;
                if mode != *expected_mode || entry.size() != expected.byte_length as u64 {
                    return Err(invalid("App file size or permissions differ"));
                }
                let mut actual = Vec::new();
                entry.take(MAX_MEMBER_BYTES + 1).read_to_end(&mut actual)?;
                if ByteIdentity::of(&actual) != *expected {
                    return Err(invalid("App file CRC/readback or expected bytes differ"));
                }
            }
        }
        if !files.is_empty() || !directories.is_empty() {
            return Err(invalid("App ZIP omits expected entries"));
        }
        Ok(())
    }

    /// Read one bounded untrusted CLI evidence material from this immutable ZIP.
    /// No extraction occurs; callers must validate its contents independently.
    pub fn read_cli_zip_material(
        &self,
        root: &str,
        relative: &str,
        maximum: usize,
        epoch: PackageEpoch,
    ) -> io::Result<Vec<u8>> {
        validate_member_name(root)?;
        validate_member_name(relative)?;
        if root.contains('/') || relative.ends_with('/') || maximum > 16 * 1024 * 1024 {
            return Err(invalid("Invalid bounded CLI evidence request"));
        }
        let members = inspect_zip_directory(&mut Cursor::new(&self.bytes), epoch)?;
        let name = format!("{root}/{relative}");
        let matches = members
            .iter()
            .filter(|member| member.name == name)
            .collect::<Vec<_>>();
        if matches.len() != 1 || matches[0].size as usize > maximum {
            return Err(invalid(
                "Missing, duplicate or oversized CLI evidence material",
            ));
        }
        let mut archive = zip::ZipArchive::new(Cursor::new(&self.bytes))?;
        if archive.len() != members.len() || archive.has_overlapping_entries()? {
            return Err(invalid("CLI evidence ZIP records differ or overlap"));
        }
        let mut bytes = Vec::new();
        archive
            .by_name(&name)?
            .take(maximum as u64 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() != matches[0].size as usize {
            return Err(invalid("CLI evidence material actual size differs"));
        }
        Ok(bytes)
    }

    /// Extract this CLI ZIP profile into a fresh private directory, retaining
    /// partial evidence on failure. Material paths and bytes come from the
    /// independently generated target notice/README inputs, not the archive.
    /// This checks CRCs and exact inventory, but does not execute the CLI.
    /// Unix creation uses eml-fs namespace admission and private ACL/mode rules.
    /// Windows requires admitted ancestor ACLs and a protected owner-created root.
    pub fn extract_cli_zip(
        &self,
        root_name: &str,
        executable: &str,
        materials: &BTreeMap<String, Vec<u8>>,
        parent: &Path,
        epoch: PackageEpoch,
    ) -> io::Result<ExtractedCli> {
        let layout = cli_layout(root_name, executable, materials)?;
        let files = &layout.files;
        let directories = &layout.directories;
        let members = inspect_zip_directory(&mut Cursor::new(&self.bytes), epoch)?;
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
        if &observed != files {
            return Err(invalid("CLI archive is missing expected files"));
        }
        let mut archive = zip::ZipArchive::new(Cursor::new(&self.bytes))?;
        if archive.len() != members.len() || archive.has_overlapping_entries()? {
            return Err(invalid("ZIP library readback differs or overlaps"));
        }
        let enclosure = create_artifact_directory(parent)?;
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
        set_directory_modes(extracted, directories)?;
        Ok(ExtractedCli {
            root: extracted.join(root_name),
            _enclosure: enclosure,
        })
    }
}

/// Encode the explicit seekable ZIP32 CLI profile to an already owned staging
/// writer. Every DOS timestamp is the source UTC epoch rounded down to two
/// seconds. Callers publish only after readback and actual behavior verification.
pub fn encode_cli_zip<W: Write + Seek>(
    writer: W,
    root_name: &str,
    executable: &str,
    binary: &[u8],
    materials: &BTreeMap<String, Vec<u8>>,
    epoch: PackageEpoch,
) -> io::Result<W> {
    let layout = producer_layout(root_name, executable, binary, materials)?;
    let mut archive = zip::ZipWriter::new(writer);
    let options = |mode| {
        zip::write::SimpleFileOptions::default()
            .last_modified_time(epoch.zip_datetime())
            .system(zip::System::Unix)
            .compression_method(zip::CompressionMethod::Deflated)
            .unix_permissions(mode)
    };
    for directory in &layout.directories {
        archive.add_directory(directory, options(0o755))?;
    }
    for name in layout.files {
        let relative = name
            .strip_prefix(&format!("{root_name}/"))
            .ok_or_else(|| invalid("Invalid producer root"))?;
        let (bytes, mode) = if relative == executable {
            (binary, 0o755)
        } else {
            (
                materials
                    .get(relative)
                    .ok_or_else(|| invalid("Missing producer material"))?
                    .as_slice(),
                0o644,
            )
        };
        archive.start_file(name, options(mode))?;
        archive.write_all(bytes)?;
    }
    Ok(archive.finish()?)
}

pub(super) fn short(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}
pub(super) fn long(bytes: &[u8], at: usize) -> u32 {
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
/// Every local/central DOS date must match the explicitly supplied UTC epoch
/// at two-second precision; this profile does not admit timestamp extensions.
pub fn inspect_zip_directory(
    reader: &mut (impl Read + Seek),
    epoch: PackageEpoch,
) -> io::Result<Vec<ZipMember>> {
    inspect_zip_profile(reader, ZipProfile::Cli, epoch)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ZipProfile {
    Cli,
    DittoApp,
}

pub(super) fn inspect_zip_profile(
    reader: &mut (impl Read + Seek),
    profile: ZipProfile,
    epoch: PackageEpoch,
) -> io::Result<Vec<ZipMember>> {
    let (count, offset, directory) = read_zip_directory(reader)?;
    let mut cursor = 0_usize;
    let mut names = BTreeSet::new();
    let mut claims = PathClaims::default();
    let mut members = Vec::with_capacity(usize::from(count));
    let mut regions = Vec::with_capacity(usize::from(count));
    let mut expanded = 0_u64;
    for _ in 0..count {
        let record = central_record(&directory, cursor, offset, profile, epoch)?;
        let name = record.name;
        let directory_entry = record.directory_entry;
        // Check original records here: ZipArchive's filename map discards
        // earlier identical names, so checking its iterator would be too late.
        if !names.insert(name.trim_end_matches('/').to_ascii_lowercase()) {
            return Err(invalid("Duplicate original ZIP member name"));
        }
        claim_path(&mut claims, name, directory_entry)?;
        let size = record.member.size;
        expanded = expanded
            .checked_add(u64::from(size))
            .ok_or_else(|| invalid("ZIP expanded size overflow"))?;
        if expanded > MAX_EXPANDED_BYTES {
            return Err(invalid("ZIP member/expanded size exceeds its bound"));
        }
        let end = local_record_end(reader, &record, &directory, cursor, offset, profile, epoch)?;
        regions.push((u64::from(record.member.local_header), end));
        cursor = record.record_end;
        members.push(record.member);
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
fn read_zip_directory(reader: &mut (impl Read + Seek)) -> io::Result<(u16, u32, Vec<u8>)> {
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
    Ok((count, offset, directory))
}

struct CentralRecord<'a> {
    header: &'a [u8],
    name: &'a str,
    record_end: usize,
    extra_length: usize,
    flags: u16,
    directory_entry: bool,
    member: ZipMember,
}
fn central_record(
    directory: &[u8],
    cursor: usize,
    offset: u32,
    profile: ZipProfile,
    epoch: PackageEpoch,
) -> io::Result<CentralRecord<'_>> {
    let header = directory
        .get(cursor..cursor.saturating_add(46))
        .ok_or_else(|| invalid("Truncated ZIP central header"))?;
    if header[..4] != *b"PK\x01\x02" || short(header, 34) != 0 {
        return Err(invalid("Unsupported ZIP central header or disk"));
    }
    let expected_time = epoch.zip_datetime();
    if short(header, 12) != expected_time.timepart()
        || short(header, 14) != expected_time.datepart()
    {
        return Err(invalid(
            "ZIP timestamp differs from the source epoch at two-second precision",
        ));
    }
    let flags = short(header, 8);
    let allowed = if profile == ZipProfile::DittoApp {
        0x0808
    } else {
        0x0800
    };
    if flags & !allowed != 0
        || !matches!(short(header, 10), 0 | 8)
        || !matches!(short(header, 6), 10 | 20)
    {
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
    let directory_entry = name.ends_with('/');
    let mode = long(header, 38) >> 16;
    let expected_type = if directory_entry { 0o040000 } else { 0o100000 };
    if header[5] != 3 || mode & 0o170000 != expected_type || mode & 0o7000 != 0 {
        return Err(invalid(
            "ZIP member must have an ordinary Unix file/directory type",
        ));
    }
    let size = long(header, 24);
    if u64::from(size) > MAX_MEMBER_BYTES {
        return Err(invalid("ZIP member/expanded size exceeds its bound"));
    }
    let compressed_size = long(header, 20);
    let local_header = long(header, 42);
    if [size, compressed_size, local_header].contains(&u32::MAX)
        || u64::from(local_header) + 30 > u64::from(offset)
        || u64::from(compressed_size) > MAX_ARCHIVE_BYTES
    {
        return Err(invalid("Unsupported ZIP64 or invalid local member extent"));
    }
    if directory_entry
        && (size != 0 || compressed_size != 0 || long(header, 16) != 0 || short(header, 10) != 0)
    {
        return Err(invalid("ZIP directory must have no payload"));
    }
    Ok(CentralRecord {
        header,
        name,
        record_end,
        extra_length,
        flags,
        directory_entry,
        member: ZipMember {
            name: name.to_owned(),
            size,
            compressed_size,
            local_header,
        },
    })
}
fn local_record_end(
    reader: &mut (impl Read + Seek),
    record: &CentralRecord<'_>,
    directory: &[u8],
    cursor: usize,
    offset: u32,
    profile: ZipProfile,
    epoch: PackageEpoch,
) -> io::Result<u64> {
    let CentralRecord {
        header,
        name,
        record_end,
        extra_length,
        flags,
        directory_entry,
        member,
    } = record;
    let (name_length, local_header, compressed_size) =
        (name.len(), member.local_header, member.compressed_size);
    let (record_end, extra_length, flags, directory_entry) =
        (*record_end, *extra_length, *flags, *directory_entry);
    reader.seek(SeekFrom::Start(u64::from(local_header)))?;
    let mut local = [0_u8; 30];
    reader.read_exact(&mut local)?;
    if local[..4] != *b"PK\x03\x04"
        || local[4..14] != header[6..16]
        || usize::from(short(&local, 26)) != name_length
    {
        return Err(invalid("ZIP local and central headers disagree"));
    }
    let descriptor = flags & 8 != 0;
    if (descriptor && local[14..26].iter().any(|byte| *byte != 0))
        || (!descriptor && local[14..26] != header[16..28])
        || (directory_entry && descriptor)
    {
        return Err(invalid(
            "ZIP local CRC/size or descriptor declaration disagrees",
        ));
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
    if profile == ZipProfile::Cli {
        if local_extra_length != 0 || extra_length != 0 {
            return Err(invalid("CLI ZIP metadata extensions are unsupported"));
        }
    } else {
        // Native ditto emits one Info-ZIP Unix time field: central length8,
        // local length12 with uid/gid. Both UTC fields must carry the
        // same source-derived two-second epoch as the DOS fields. Ownership
        // values confer no authority;
        // native extraction's resulting ownership is checked separately.
        if local_extra_length != 16 || extra_length != 12 {
            return Err(invalid("Unsupported ditto metadata extent"));
        }
        let central = &directory[cursor + 46 + name_length..record_end];
        let mut local_extra = [0_u8; 16];
        reader.read_exact(&mut local_extra)?;
        if short(central, 0) != 0x5855
            || short(central, 2) != 8
            || short(&local_extra, 0) != 0x5855
            || short(&local_extra, 2) != 12
            || central[4..12] != local_extra[4..12]
            || long(central, 4) != epoch.floor_seconds()
            || long(central, 8) != epoch.floor_seconds()
        {
            return Err(invalid("Unsupported or inconsistent ditto metadata"));
        }
    }
    let end = if descriptor {
        if data_end + 16 > u64::from(offset) {
            return Err(invalid("Truncated ditto data descriptor"));
        }
        reader.seek(SeekFrom::Start(data_end))?;
        let mut record = [0_u8; 16];
        reader.read_exact(&mut record)?;
        if record[..4] != *b"PK\x07\x08" || record[4..16] != header[16..28] {
            return Err(invalid(
                "Ditto data descriptor differs from central CRC/sizes",
            ));
        }
        data_end + 16
    } else {
        data_end
    };
    Ok(end)
}
