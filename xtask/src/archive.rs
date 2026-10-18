//! Bounded native archive inspection/readback on one immutable snapshot.
//! Structural/material evidence is separate from executable qualification.
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, OpenOptions},
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

/// Byte identity of the immutable inspected input, not review/behavior quality.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ByteIdentity {
    /// Exact snapshot byte length.
    pub byte_length: usize,
    /// SHA-256 of these same immutable bytes.
    pub sha256: String,
}
impl ByteIdentity {
    /// Identify bytes without implying source provenance or review quality.
    pub fn of(bytes: &[u8]) -> Self {
        Self {
            byte_length: bytes.len(),
            sha256: Sha256::digest(bytes)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
        }
    }
}

/// Owned private directory with its admitted ancestor chain. Drop closes handles
/// without removing files; retain through bounded observation, then abort/preserve
/// resources if settlement is unknown. Trusted UID/root interference is excluded.
pub struct ArtifactDirectory {
    path: PathBuf,
    #[cfg(unix)]
    _held: eml_fs::CreatedDirectory,
    #[cfg(unix)]
    _ancestors: Vec<eml_fs::BoundDirectory>,
    #[cfg(windows)]
    _windows: eml_fs::PrivateDirectory,
}

impl ArtifactDirectory {
    /// Current requested address under the admitted other-UID protection model.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Extracted CLI bytes inside a retained private enclosure. Dropping this value
/// closes held descriptors but does not remove useful files or failure evidence.
/// Retain it through bounded pathname-operation observation. If settlement is
/// unknown, abort later work and preserve files; handles can then close without
/// sweeping/removing unverified resources. The full-chain policy excludes
/// interference by the trusted UID/root.
pub struct ExtractedCli {
    root: PathBuf,
    _enclosure: ArtifactDirectory,
}
impl ExtractedCli {
    /// Package directory beneath the private enclosure.
    pub fn root(&self) -> &Path {
        &self.root
    }
}

/// Create an exclusive private artifact directory after admitting every ancestor.
#[cfg(unix)]
pub fn create_artifact_directory(parent: &Path) -> io::Result<ArtifactDirectory> {
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
        let name = format!("artifact-{suffix}");
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
                return Ok(ArtifactDirectory {
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

/// Create only beneath a natively admitted protected Windows ancestor chain.
#[cfg(windows)]
pub fn create_artifact_directory(parent: &Path) -> io::Result<ArtifactDirectory> {
    let directory = eml_fs::create_private_directory(parent)?;
    directory.check()?;
    Ok(ArtifactDirectory {
        path: directory.path().to_owned(),
        _windows: directory,
    })
}

/// Refuse hosts without a protected-root implementation.
#[cfg(not(any(unix, windows)))]
pub fn create_artifact_directory(_parent: &Path) -> io::Result<ArtifactDirectory> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Private Windows archive extraction requires an observed protected parent/root ACL; native qualification is unavailable",
    ))
}

impl ArchiveSnapshot {
    /// Borrow the immutable bytes used by inspection; native extraction receives
    /// an owned copy of these bytes, never a second read of the caller's path.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Inspect the finite native ditto app profile, including every CRC, exact
    /// expected file identity and mode. References come from the frozen producer;
    /// hashes prove identity, not review quality. This does not execute the app.
    /// Expected keys are paths relative to the one app root; modes are 0644/0755.
    pub fn inspect_ditto_app(
        &self,
        root_name: &str,
        expected: &BTreeMap<String, (ByteIdentity, u32)>,
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
        let members = inspect_zip_profile(&mut Cursor::new(&self.bytes), ZipProfile::DittoApp)?;
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
    /// Identify the same snapshot used by all parsing and extraction passes.
    pub fn identity(&self) -> ByteIdentity {
        ByteIdentity::of(&self.bytes)
    }
    /// Read a snapshot without trusting an advertised file length for allocation.
    pub fn read(path: &Path) -> io::Result<Self> {
        if !fs::symlink_metadata(path)?.is_file() {
            return Err(invalid(
                "Archive input must be an ordinary non-symlink file",
            ));
        }
        let mut open = OpenOptions::new();
        open.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            open.custom_flags(
                (rustix::fs::OFlags::NONBLOCK | rustix::fs::OFlags::NOFOLLOW).bits() as i32,
            );
        }
        let file = open.open(path)?;
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
    /// Windows requires admitted ancestor ACLs and a protected owner-created root.
    pub fn extract_cli_zip(
        &self,
        root_name: &str,
        executable: &str,
        materials: &BTreeMap<String, Vec<u8>>,
        parent: &Path,
    ) -> io::Result<ExtractedCli> {
        let layout = cli_layout(root_name, executable, materials)?;
        let files = &layout.files;
        let directories = &layout.directories;
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

    /// Inspect and extract the generated single-member gzip/USTAR CLI profile.
    /// All framing and gzip integrity are checked before creating the enclosure;
    /// the second pass reads the same immutable bytes. No executable runs here.
    pub fn extract_cli_tar_gz(
        &self,
        root_name: &str,
        executable: &str,
        materials: &BTreeMap<String, Vec<u8>>,
        parent: &Path,
    ) -> io::Result<ExtractedCli> {
        let layout = cli_layout(root_name, executable, materials)?;
        let members = self.walk_tar(|member, body| {
            if io::copy(body, &mut io::sink())? != member.size {
                return Err(invalid("Truncated USTAR member body"));
            }
            Ok(())
        })?;
        let mut observed = BTreeSet::new();
        for member in &members {
            if member.directory {
                if !layout.directories.contains(&member.name) {
                    return Err(invalid("Unexpected CLI tar directory"));
                }
            } else if !layout.files.contains(&member.name) || !observed.insert(member.name.clone())
            {
                return Err(invalid("Unexpected CLI tar file"));
            }
            let mode = if member.directory || member.name == format!("{root_name}/{executable}") {
                0o755
            } else {
                0o644
            };
            if member.mode != mode {
                return Err(invalid("CLI tar member mode differs from package policy"));
            }
        }
        if observed != layout.files {
            return Err(invalid("CLI tar archive is missing expected files"));
        }
        let enclosure = create_artifact_directory(parent)?;
        eprintln!(
            "Retained owned CLI extraction: {}",
            enclosure.path.display()
        );
        let mut index = 0;
        self.walk_tar(|member, body| {
            if members.get(index) != Some(member) {
                return Err(invalid("USTAR second-pass metadata differs"));
            }
            index += 1;
            let path = enclosure.path.join(member.name.trim_end_matches('/'));
            if member.directory {
                fs::create_dir_all(path)?;
                return Ok(());
            }
            fs::create_dir_all(
                path.parent()
                    .ok_or_else(|| invalid("Missing tar extraction parent"))?,
            )?;
            let mut output = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)?;
            if io::copy(body, &mut output)? != member.size {
                return Err(invalid("USTAR actual member size differs"));
            }
            output.flush()?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                output.set_permissions(fs::Permissions::from_mode(member.mode))?;
            }
            drop(output);
            let relative = member
                .name
                .strip_prefix(&format!("{root_name}/"))
                .ok_or_else(|| invalid("Invalid tar package root"))?;
            if let Some(expected) = materials.get(relative)
                && fs::read(path)? != *expected
            {
                return Err(invalid(
                    "Extracted tar legal material differs from reviewed inputs",
                ));
            }
            Ok(())
        })?;
        if index != members.len() {
            return Err(invalid("USTAR second pass omitted a member"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for directory in layout.directories {
                fs::set_permissions(
                    enclosure.path.join(directory.trim_end_matches('/')),
                    fs::Permissions::from_mode(0o755),
                )?;
            }
        }
        Ok(ExtractedCli {
            root: enclosure.path.join(root_name),
            _enclosure: enclosure,
        })
    }

    fn walk_tar(
        &self,
        mut visit: impl FnMut(&TarMember, &mut dyn Read) -> io::Result<()>,
    ) -> io::Result<Vec<TarMember>> {
        // The explicit producer emits no optional gzip metadata. This is a
        // narrow format boundary, not a claim that the decoder is unbounded.
        if self.bytes.len() < 18 || self.bytes[..4] != [0x1f, 0x8b, 8, 0] {
            return Err(invalid("Expected the generated single-member gzip header"));
        }
        let decoder = flate2::bufread::GzDecoder::new(self.bytes.as_slice());
        let mut stream = ExpandedReader {
            inner: decoder,
            total: 0,
            limit: MAX_EXPANDED_BYTES,
            exceeded: false,
        };
        let mut members = Vec::new();
        let mut names = BTreeSet::new();
        let mut claims = PathClaims::default();
        loop {
            let mut block = [0_u8; 512];
            stream.read_exact(&mut block)?;
            if block.iter().all(|byte| *byte == 0) {
                stream.read_exact(&mut block)?;
                if block.iter().any(|byte| *byte != 0) {
                    return Err(invalid("USTAR requires two complete zero end blocks"));
                }
                loop {
                    let count = stream.read(&mut block)?;
                    if count == 0 {
                        break;
                    }
                    if block[..count].iter().any(|byte| *byte != 0) {
                        return Err(invalid("Nonzero data follows the USTAR end marker"));
                    }
                }
                if stream.total % 512 != 0 {
                    return Err(invalid("USTAR zero tail is not block aligned"));
                }
                if !stream.inner.into_inner().is_empty() {
                    return Err(invalid(
                        "Unexpected trailing compressed bytes or second gzip member",
                    ));
                }
                return Ok(members);
            }
            if members.len() >= usize::from(MAX_ENTRIES) {
                return Err(invalid("USTAR member count exceeds its bound"));
            }
            let member = tar_member(&block)?;
            if !names.insert(member.name.trim_end_matches('/').to_ascii_lowercase()) {
                return Err(invalid("Duplicate original USTAR member name"));
            }
            claim_path(&mut claims, &member.name, member.directory)?;
            {
                let mut body = (&mut stream).take(member.size);
                visit(&member, &mut body)?;
                if body.limit() != 0 {
                    return Err(invalid("USTAR body was not fully consumed"));
                }
            }
            let padding = ((512 - member.size % 512) % 512) as usize;
            stream.read_exact(&mut block[..padding])?;
            if block[..padding].iter().any(|byte| *byte != 0) {
                return Err(invalid("USTAR member alignment bytes are nonzero"));
            }
            members.push(member);
        }
    }
}

struct ExpandedReader<R> {
    inner: R,
    total: u64,
    limit: u64,
    exceeded: bool,
}
impl<R: Read> Read for ExpandedReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        if self.exceeded {
            return Err(invalid("Expanded tar container exceeds its bound"));
        }
        let remaining = self.limit.saturating_sub(self.total);
        let capacity = (buffer.len() as u64).min(remaining.saturating_add(1)) as usize;
        let count = self.inner.read(&mut buffer[..capacity])?;
        self.total += count as u64;
        if self.total > self.limit {
            self.exceeded = true;
            return Err(invalid("Expanded tar container exceeds its bound"));
        }
        Ok(count)
    }
}

#[derive(Debug, PartialEq, Eq)]
struct TarMember {
    name: String,
    size: u64,
    directory: bool,
    mode: u32,
}

fn terminated_field(field: &[u8]) -> io::Result<&[u8]> {
    let end = field
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(field.len());
    if field[end..].iter().any(|byte| *byte != 0) {
        return Err(invalid("Nonzero suffix follows a USTAR field NUL"));
    }
    Ok(&field[..end])
}
fn octal_field(field: &[u8], empty: bool) -> io::Result<()> {
    let end = field
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(field.len());
    if field[end..].iter().any(|byte| !matches!(byte, 0 | b' ')) {
        return Err(invalid("Nonzero numeric suffix after USTAR NUL"));
    }
    let mut digits = &field[..end];
    while digits.first() == Some(&b' ') {
        digits = &digits[1..];
    }
    while digits.last() == Some(&b' ') {
        digits = &digits[..digits.len() - 1];
    }
    if (!empty && digits.is_empty()) || digits.iter().any(|byte| !matches!(byte, b'0'..=b'7')) {
        return Err(invalid("USTAR numeric field must be bounded octal"));
    }
    Ok(())
}
fn tar_member(block: &[u8; 512]) -> io::Result<TarMember> {
    let mut header = tar::Header::new_ustar();
    header.as_mut_bytes().copy_from_slice(block);
    let raw = header
        .as_ustar()
        .ok_or_else(|| invalid("Expected generated USTAR header"))?;
    if raw.magic != *b"ustar\0" || raw.version != *b"00" {
        return Err(invalid("Unsupported USTAR magic/version"));
    }
    for field in [
        &raw.mode[..],
        &raw.uid,
        &raw.gid,
        &raw.size,
        &raw.mtime,
        &raw.cksum,
    ] {
        octal_field(field, false)?;
    }
    octal_field(&raw.dev_major, true)?;
    octal_field(&raw.dev_minor, true)?;
    let checksum: u32 = block
        .iter()
        .enumerate()
        .map(|(index, byte)| {
            if (148..156).contains(&index) {
                32
            } else {
                u32::from(*byte)
            }
        })
        .sum();
    if header.cksum()? != checksum {
        return Err(invalid("USTAR header checksum differs"));
    }
    let directory = match raw.typeflag[0] {
        b'0' => false,
        b'5' => true,
        _ => {
            return Err(invalid(
                "USTAR extensions, sparse files, links and special types are unsupported",
            ));
        }
    };
    if raw.linkname.iter().any(|byte| *byte != 0) {
        return Err(invalid("USTAR CLI members cannot name a link target"));
    }
    let prefix = terminated_field(&raw.prefix)?;
    let name = terminated_field(&raw.name)?;
    let mut original = prefix.to_vec();
    if !original.is_empty() {
        original.push(b'/');
    }
    original.extend_from_slice(name);
    let name = std::str::from_utf8(&original)
        .map_err(|_| invalid("USTAR name is not UTF-8"))?
        .to_owned();
    validate_member_name(&name)?;
    let size = header.size()?;
    let mode = header.mode()?;
    if directory != name.ends_with('/')
        || size > MAX_MEMBER_BYTES
        || (directory && size != 0)
        || mode & !0o777 != 0
    {
        return Err(invalid(
            "USTAR type, size or mode is outside the CLI profile",
        ));
    }
    Ok(TarMember {
        name,
        size,
        directory,
        mode,
    })
}

struct CliLayout {
    files: BTreeSet<String>,
    directories: BTreeSet<String>,
}

fn producer_layout(
    root_name: &str,
    executable: &str,
    binary: &[u8],
    materials: &BTreeMap<String, Vec<u8>>,
) -> io::Result<CliLayout> {
    let layout = cli_layout(root_name, executable, materials)?;
    if binary.is_empty()
        || binary.len() as u64 > MAX_MEMBER_BYTES
        || layout.files.len() + layout.directories.len() > usize::from(MAX_ENTRIES)
    {
        return Err(invalid(
            "CLI archive producer input exceeds its member/count bounds",
        ));
    }
    let mut total = binary.len() as u64;
    for bytes in materials.values() {
        if bytes.len() as u64 > MAX_MEMBER_BYTES {
            return Err(invalid("CLI material exceeds archive member bound"));
        }
        total = total
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| invalid("CLI archive size overflow"))?;
    }
    if total > MAX_EXPANDED_BYTES {
        return Err(invalid("CLI archive payload exceeds its bound"));
    }
    Ok(layout)
}

/// Encode the explicit seekable ZIP32 CLI profile to an already owned staging
/// writer. Callers publish only after readback and actual behavior verification.
pub fn encode_cli_zip<W: Write + Seek>(
    writer: W,
    root_name: &str,
    executable: &str,
    binary: &[u8],
    materials: &BTreeMap<String, Vec<u8>>,
) -> io::Result<W> {
    let layout = producer_layout(root_name, executable, binary, materials)?;
    let mut archive = zip::ZipWriter::new(writer);
    let options = |mode| {
        zip::write::SimpleFileOptions::default()
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

/// Encode one gzip member containing only explicit classic USTAR records.
/// Names are validated before set_path; no GNU/PAX convenience fallback is used.
pub fn encode_cli_tar_gz<W: Write>(
    writer: W,
    root_name: &str,
    executable: &str,
    binary: &[u8],
    materials: &BTreeMap<String, Vec<u8>>,
) -> io::Result<W> {
    let layout = producer_layout(root_name, executable, binary, materials)?;
    let gzip = flate2::write::GzEncoder::new(writer, flate2::Compression::default());
    let mut archive = tar::Builder::new(gzip);
    let mut append = |name: &str, bytes: &[u8], directory: bool, mode: u32| -> io::Result<()> {
        let mut header = tar::Header::new_ustar();
        header.set_path(name)?;
        header.set_entry_type(if directory {
            tar::EntryType::Directory
        } else {
            tar::EntryType::Regular
        });
        header.set_mode(mode);
        header.set_uid(0);
        header.set_gid(0);
        header.set_mtime(0);
        header.set_size(bytes.len() as u64);
        header.set_cksum();
        // Slice EOF is independently fixed to the advertised length.
        archive.append(&header, bytes)
    };
    for directory in &layout.directories {
        append(directory, &[], true, 0o755)?;
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
        append(&name, bytes, false, mode)?;
    }
    archive.finish()?;
    archive.into_inner()?.finish()
}

fn cli_layout(
    root_name: &str,
    executable: &str,
    materials: &BTreeMap<String, Vec<u8>>,
) -> io::Result<CliLayout> {
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
    Ok(CliLayout { files, directories })
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
    inspect_zip_profile(reader, ZipProfile::Cli)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ZipProfile {
    Cli,
    DittoApp,
}

fn inspect_zip_profile(
    reader: &mut (impl Read + Seek),
    profile: ZipProfile,
) -> io::Result<Vec<ZipMember>> {
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
    let mut expanded = 0_u64;
    for _ in 0..count {
        let header = directory
            .get(cursor..cursor.saturating_add(46))
            .ok_or_else(|| invalid("Truncated ZIP central header"))?;
        if header[..4] != *b"PK\x01\x02" || short(header, 34) != 0 {
            return Err(invalid("Unsupported ZIP central header or disk"));
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
        expanded = expanded
            .checked_add(u64::from(size))
            .ok_or_else(|| invalid("ZIP expanded size overflow"))?;
        if u64::from(size) > MAX_MEMBER_BYTES || expanded > MAX_EXPANDED_BYTES {
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
            && (size != 0
                || compressed_size != 0
                || long(header, 16) != 0
                || short(header, 10) != 0)
        {
            return Err(invalid("ZIP directory must have no payload"));
        }
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
            // local length12 with uid/gid. These values confer no authority;
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
        regions.push((u64::from(local_header), end));
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

    #[cfg(target_os = "macos")]
    #[test]
    fn actual_ditto_profile_checks_original_records_descriptors_and_materials() {
        use std::os::unix::fs::PermissionsExt;
        let owned = tempfile::tempdir().unwrap();
        let root = "EML Attachment Remover.app";
        let app = owned.path().join(root);
        let payloads: BTreeMap<String, (Vec<u8>, u32)> = BTreeMap::from([
            (
                "Contents/MacOS/product".into(),
                (
                    b"owned format fixture; not an executable app\n".to_vec(),
                    0o755,
                ),
            ),
            (
                "Contents/Resources/a.txt".into(),
                (b"first owned source material\n".to_vec(), 0o644),
            ),
            (
                "Contents/Resources/b.txt".into(),
                (b"second owned source material\n".to_vec(), 0o644),
            ),
        ]);
        for (name, (bytes, mode)) in &payloads {
            let path = app.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, bytes).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(*mode)).unwrap();
        }
        for name in ["", "Contents", "Contents/MacOS", "Contents/Resources"] {
            fs::set_permissions(app.join(name), fs::Permissions::from_mode(0o755)).unwrap();
        }
        let path = owned.path().join("format.zip");
        assert!(
            std::process::Command::new("/usr/bin/ditto")
                .args([
                    "-c",
                    "-k",
                    "--keepParent",
                    "--norsrc",
                    "--noextattr",
                    "--noqtn",
                    "--noacl"
                ])
                .arg(&app)
                .arg(&path)
                .status()
                .unwrap()
                .success()
        );
        let expected: BTreeMap<_, _> = payloads
            .iter()
            .map(|(name, (bytes, mode))| (name.clone(), (ByteIdentity::of(bytes), *mode)))
            .collect();
        let snapshot = ArchiveSnapshot::read(&path).unwrap();
        snapshot.inspect_ditto_app(root, &expected).unwrap();
        assert!(
            inspect_zip_directory(&mut Cursor::new(snapshot.bytes())).is_err(),
            "The CLI profile must retain its no-extension/no-descriptor contract"
        );
        let bytes = snapshot.bytes();
        let central = long(&bytes[bytes.len() - 22..], 16) as usize;
        let mut cursor = central;
        let mut descriptor = None;
        let mut duplicate = None;
        let duplicate_name = format!("{root}/Contents/Resources/b.txt");
        while &bytes[cursor..cursor + 4] == b"PK\x01\x02" {
            let header = &bytes[cursor..cursor + 46];
            let name_len = usize::from(short(header, 28));
            let local = long(header, 42) as usize;
            if descriptor.is_none() && short(header, 8) & 8 != 0 {
                descriptor = Some(
                    local
                        + 30
                        + name_len
                        + usize::from(short(&bytes[local..local + 30], 28))
                        + long(header, 20) as usize,
                );
            }
            if &bytes[cursor + 46..cursor + 46 + name_len] == duplicate_name.as_bytes() {
                duplicate = Some((cursor + 46, local + 30));
            }
            cursor +=
                46 + name_len + usize::from(short(header, 30)) + usize::from(short(header, 32));
        }
        let mut bad = bytes.to_vec();
        bad[descriptor.unwrap() + 4] ^= 1;
        assert!(
            ArchiveSnapshot { bytes: bad }
                .inspect_ditto_app(root, &expected)
                .unwrap_err()
                .to_string()
                .contains("descriptor differs")
        );
        let mut bad = bytes.to_vec();
        bad[central + 46 + usize::from(short(&bytes[central..central + 46], 28))] ^= 1;
        assert!(
            ArchiveSnapshot { bytes: bad }
                .inspect_ditto_app(root, &expected)
                .unwrap_err()
                .to_string()
                .contains("ditto metadata")
        );
        let mut bad = bytes.to_vec();
        let (central_name, local_name) = duplicate.unwrap();
        let replacement = format!("{root}/Contents/Resources/a.txt");
        bad[central_name..central_name + replacement.len()].copy_from_slice(replacement.as_bytes());
        bad[local_name..local_name + replacement.len()].copy_from_slice(replacement.as_bytes());
        assert!(
            ArchiveSnapshot { bytes: bad }
                .inspect_ditto_app(root, &expected)
                .unwrap_err()
                .to_string()
                .contains("Duplicate original ZIP member")
        );
        let mut wrong_material = expected.clone();
        wrong_material
            .get_mut("Contents/Resources/a.txt")
            .unwrap()
            .0
            .sha256 = ByteIdentity::of(b"different owned source material").sha256;
        assert!(
            snapshot
                .inspect_ditto_app(root, &wrong_material)
                .unwrap_err()
                .to_string()
                .contains("expected bytes differ")
        );
    }

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

    #[test]
    fn snapshot_identity_is_of_retained_bytes_and_nonfiles_are_refused() {
        let owned = tempfile::tempdir().unwrap();
        assert!(ArchiveSnapshot::read(owned.path()).is_err());
        let path = owned.path().join("archive.zip");
        fs::write(&path, b"abc").unwrap();
        let snapshot = ArchiveSnapshot::read(&path).unwrap();
        assert_eq!(
            snapshot.identity().sha256,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        fs::write(&path, b"changed owned path").unwrap();
        assert_eq!(snapshot.identity().byte_length, 3);
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&path, owned.path().join("alias.zip")).unwrap();
            assert!(ArchiveSnapshot::read(&owned.path().join("alias.zip")).is_err());
            assert!(
                std::process::Command::new("/usr/bin/mkfifo")
                    .args(["-m", "600"])
                    .arg(owned.path().join("pipe.zip"))
                    .status()
                    .unwrap()
                    .success()
            );
            assert!(ArchiveSnapshot::read(&owned.path().join("pipe.zip")).is_err());
        }
    }

    #[test]
    #[cfg(unix)]
    fn explicit_cli_producers_roundtrip_their_narrow_profiles_and_materials() {
        let owned = tempfile::tempdir().unwrap();
        let materials = synthetic_materials();
        let binary = b"owned fixture executable, not invoked";
        let zip = encode_cli_zip(
            Cursor::new(Vec::new()),
            "package",
            "tool",
            binary,
            &materials,
        )
        .unwrap()
        .into_inner();
        let tar = encode_cli_tar_gz(Vec::new(), "package", "tool", binary, &materials).unwrap();
        let zip = ArchiveSnapshot { bytes: zip }
            .extract_cli_zip("package", "tool", &materials, owned.path())
            .unwrap();
        let tar = ArchiveSnapshot { bytes: tar }
            .extract_cli_tar_gz("package", "tool", &materials, owned.path())
            .unwrap();
        for extracted in [&zip, &tar] {
            assert_eq!(fs::read(extracted.root().join("tool")).unwrap(), binary);
            for (name, bytes) in &materials {
                assert_eq!(fs::read(extracted.root().join(name)).unwrap(), *bytes);
            }
        }
    }

    fn synthetic_cli_zip(materials: &BTreeMap<String, Vec<u8>>) -> Vec<u8> {
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

    fn ustar(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut writer = tar::Builder::new(Vec::new());
        for (name, bytes) in entries {
            let mut header = tar::Header::new_ustar();
            header.set_path(name).unwrap();
            header.set_entry_type(tar::EntryType::Regular);
            header.set_mode(if *name == "package/tool" {
                0o755
            } else {
                0o644
            });
            header.set_uid(0);
            header.set_gid(0);
            header.set_mtime(0);
            header.set_size(bytes.len() as u64);
            header.set_cksum();
            writer.append(&header, *bytes).unwrap();
        }
        writer.finish().unwrap();
        writer.into_inner().unwrap()
    }

    fn gzip(bytes: &[u8]) -> Vec<u8> {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(bytes).unwrap();
        encoder.finish().unwrap()
    }

    fn inspect_tar(bytes: Vec<u8>) -> io::Result<Vec<TarMember>> {
        ArchiveSnapshot { bytes }.walk_tar(|_, body| io::copy(body, &mut io::sink()).map(|_| ()))
    }

    fn fix_tar_checksum(bytes: &mut [u8]) {
        let mut header = tar::Header::new_ustar();
        header.as_mut_bytes().copy_from_slice(&bytes[..512]);
        header.set_cksum();
        bytes[..512].copy_from_slice(header.as_bytes());
    }

    #[test]
    fn ustar_prefix_split_and_zero_tail_preserve_original_names() {
        let name = format!("{}/{}", "a".repeat(95), "b".repeat(20));
        let raw = ustar(&[(&name, b"owned")]);
        assert_ne!(raw[345], 0); // Original USTAR prefix is actually populated.
        let members = inspect_tar(gzip(&raw)).unwrap();
        assert_eq!(members[0].name, name);
        assert_eq!(members[0].size, 5);
        let mut padded = raw;
        padded.extend_from_slice(&[0; 512]);
        assert_eq!(inspect_tar(gzip(&padded)).unwrap(), members);
    }

    #[test]
    fn ustar_framing_rejects_bad_padding_end_blocks_and_truncated_body() {
        let raw = ustar(&[("package/file", b"abc")]);
        assert!(inspect_tar(gzip(&raw)).is_ok());
        let mut padding = raw.clone();
        padding[515] = 1;
        assert!(inspect_tar(gzip(&padding)).is_err());
        for truncated in [
            &raw[..513],
            &raw[..1024],
            &raw[..raw.len() - 512],
            &raw[..raw.len() - 1],
        ] {
            assert!(inspect_tar(gzip(truncated)).is_err()); // Valid gzip, incomplete TAR.
        }
        for tail in [0, 1] {
            let mut changed = raw.clone();
            changed.push(tail);
            assert!(inspect_tar(gzip(&changed)).is_err());
        }
        let mut second_end = raw;
        second_end[1536] = 1;
        assert!(inspect_tar(gzip(&second_end)).is_err());
    }

    #[test]
    fn gzip_crc_isize_trailer_and_single_member_boundaries_are_checked() {
        let compressed = gzip(&ustar(&[("package/file", b"abc")]));
        for offset in [8, 4] {
            let mut changed = compressed.clone();
            let index = changed.len() - offset;
            changed[index] ^= 1;
            assert!(inspect_tar(changed).is_err());
        }
        assert!(inspect_tar(compressed[..compressed.len() - 1].to_vec()).is_err());
        for tail in [&compressed[..], &[0][..], b"unexpected".as_slice()] {
            let mut changed = compressed.clone();
            changed.extend_from_slice(tail);
            assert!(inspect_tar(changed).is_err());
        }
    }

    #[test]
    fn ustar_headers_reject_extensions_links_hidden_suffixes_and_path_aliases() {
        let raw = ustar(&[("package/file", b"abc")]);
        let mut checksum = raw.clone();
        checksum[0] = b'q';
        assert!(inspect_tar(gzip(&checksum)).is_err());
        for flag in *b"xgLKS12346" {
            let mut changed = raw.clone();
            changed[156] = flag;
            fix_tar_checksum(&mut changed);
            let mut visited = false;
            let result = ArchiveSnapshot {
                bytes: gzip(&changed),
            }
            .walk_tar(|_, _| {
                visited = true;
                Ok(())
            });
            assert!(result.is_err());
            assert!(!visited);
        }
        let mut hidden_suffix = raw.clone();
        hidden_suffix[13] = b'x';
        fix_tar_checksum(&mut hidden_suffix);
        assert!(inspect_tar(gzip(&hidden_suffix)).is_err());
        for field in [
            [0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 3],
            [b'3', 0, b'7', 0, 0, 0, 0, 0, 0, 0, 0, 0],
            [b'\t', b'3', 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        ] {
            let mut changed = raw.clone();
            changed[124..136].copy_from_slice(&field);
            fix_tar_checksum(&mut changed);
            let mut header = tar::Header::new_ustar();
            header.as_mut_bytes().copy_from_slice(&changed[..512]);
            assert_eq!(header.size().unwrap(), 3); // Accessor alone accepts it.
            assert!(inspect_tar(gzip(&changed)).is_err());
        }
        for path in ["package\\file", "package/../file", "package/file/"] {
            let mut changed = raw.clone();
            changed[..100].fill(0);
            changed[..path.len()].copy_from_slice(path.as_bytes());
            fix_tar_checksum(&mut changed);
            assert!(inspect_tar(gzip(&changed)).is_err());
        }
        for entries in [
            [
                ("package/file", b"a".as_slice()),
                ("package/file", b"b".as_slice()),
            ],
            [
                ("package/file", b"a".as_slice()),
                ("package/file/child", b"b".as_slice()),
            ],
        ] {
            assert!(inspect_tar(gzip(&ustar(&entries))).is_err());
        }
    }

    #[test]
    fn expanded_reader_proves_real_eof_at_limit_and_rejects_one_more_byte() {
        let mut exact = ExpandedReader {
            inner: b"12345678".as_slice(),
            total: 0,
            limit: 8,
            exceeded: false,
        };
        let mut bytes = Vec::new();
        exact.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"12345678");
        let mut over = ExpandedReader {
            inner: b"123456789".as_slice(),
            total: 0,
            limit: 8,
            exceeded: false,
        };
        assert!(over.read_to_end(&mut Vec::new()).is_err());
        assert!(over.read(&mut [0; 1]).is_err());
    }

    #[test]
    fn cli_ustar_extracts_exact_materials_inside_the_retained_private_enclosure() {
        let owned = tempfile::tempdir().unwrap();
        let materials = synthetic_materials();
        let names: Vec<_> = std::iter::once("package/tool".to_owned())
            .chain(materials.keys().map(|name| format!("package/{name}")))
            .collect();
        let payloads: Vec<_> = std::iter::once(b"owned executable fixture, not invoked".to_vec())
            .chain(materials.values().cloned())
            .collect();
        let entries: Vec<_> = names
            .iter()
            .zip(&payloads)
            .map(|(name, bytes)| (name.as_str(), bytes.as_slice()))
            .collect();
        let snapshot = ArchiveSnapshot {
            bytes: gzip(&ustar(&entries)),
        };
        let extracted = snapshot
            .extract_cli_tar_gz("package", "tool", &materials, owned.path())
            .unwrap();
        for (name, bytes) in materials {
            assert_eq!(fs::read(extracted.root().join(name)).unwrap(), bytes);
        }
        assert_eq!(
            fs::read(extracted.root().join("tool")).unwrap(),
            payloads[0]
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for (path, mode) in [
                (extracted.root().to_owned(), 0o755),
                (extracted.root().parent().unwrap().to_owned(), 0o700),
                (extracted.root().join("tool"), 0o755),
                (extracted.root().join("LICENSE"), 0o644),
                (extracted.root().join("licenses/rust"), 0o755),
            ] {
                assert_eq!(
                    fs::metadata(path).unwrap().permissions().mode() & 0o777,
                    mode
                );
            }
        }
    }

    #[test]
    fn cli_tar_rejects_missing_extra_and_changed_materials() {
        let owned = tempfile::tempdir().unwrap();
        let materials = synthetic_materials();
        for extra in [false, true] {
            let mut packaged = materials.clone();
            if extra {
                packaged.insert("unexpected".into(), b"owned extra".to_vec());
            } else {
                packaged.remove("licenses/rust/COPYRIGHT-library.html");
            }
            let mut entries: Vec<_> = packaged
                .iter()
                .map(|(name, bytes)| (format!("package/{name}"), bytes.clone()))
                .collect();
            entries.push((
                "package/tool".into(),
                b"owned executable fixture, not invoked".to_vec(),
            ));
            let borrowed: Vec<_> = entries
                .iter()
                .map(|(name, bytes)| (name.as_str(), bytes.as_slice()))
                .collect();
            let snapshot = ArchiveSnapshot {
                bytes: gzip(&ustar(&borrowed)),
            };
            assert!(
                snapshot
                    .extract_cli_tar_gz("package", "tool", &materials, owned.path())
                    .is_err()
            );
            assert_eq!(fs::read_dir(owned.path()).unwrap().count(), 0);
        }
        let mut changed = materials.clone();
        changed.insert("LICENSE".into(), b"changed owned legal fixture".to_vec());
        let mut entries: Vec<_> = changed
            .iter()
            .map(|(name, bytes)| (format!("package/{name}"), bytes.clone()))
            .collect();
        entries.push((
            "package/tool".into(),
            b"owned executable fixture, not invoked".to_vec(),
        ));
        let borrowed: Vec<_> = entries
            .iter()
            .map(|(name, bytes)| (name.as_str(), bytes.as_slice()))
            .collect();
        let snapshot = ArchiveSnapshot {
            bytes: gzip(&ustar(&borrowed)),
        };
        assert!(
            snapshot
                .extract_cli_tar_gz("package", "tool", &materials, owned.path())
                .is_err()
        );
        assert_eq!(fs::read_dir(owned.path()).unwrap().count(), 1);
    }

    #[test]
    #[cfg(unix)]
    fn immutable_cli_snapshot_extracts_exact_owned_materials_and_modes() {
        let owned = tempfile::tempdir().unwrap();
        let materials = synthetic_materials();
        let path = owned.path().join("source.zip");
        fs::write(&path, synthetic_cli_zip(&materials)).unwrap();
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
            bytes: synthetic_cli_zip(&missing),
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
            bytes: synthetic_cli_zip(&materials),
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
        let mut bytes = synthetic_cli_zip(&materials);
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
            bytes: synthetic_cli_zip(&different),
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
            bytes: synthetic_cli_zip(&materials),
        };
        let result = snapshot
            .extract_cli_zip("package", "tool", &materials, owned.path())
            .unwrap();
        for (name, bytes) in materials {
            assert_eq!(fs::read(result.root().join(name)).unwrap(), bytes);
        }
        assert_eq!(
            fs::read(result.root().join("tool")).unwrap(),
            b"synthetic executable fixture; never invoked"
        );
        drop(result);
        assert_eq!(fs::read_dir(owned.path()).unwrap().count(), 1);
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
