//! Bounded USTAR/gzip codecs and retained-enclosure extraction.
use super::*;

impl ArchiveSnapshot {
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
        set_directory_modes(&enclosure.path, &layout.directories)?;
        Ok(ExtractedCli {
            root: enclosure.path.join(root_name),
            _enclosure: enclosure,
        })
    }

    pub(super) fn walk_tar(
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

pub(super) struct ExpandedReader<R> {
    pub(super) inner: R,
    pub(super) total: u64,
    pub(super) limit: u64,
    pub(super) exceeded: bool,
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
pub(super) struct TarMember {
    pub(super) name: String,
    pub(super) size: u64,
    pub(super) directory: bool,
    pub(super) mode: u32,
}

pub(super) fn terminated_field(field: &[u8]) -> io::Result<&[u8]> {
    let end = field
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(field.len());
    if field[end..].iter().any(|byte| *byte != 0) {
        return Err(invalid("Nonzero suffix follows a USTAR field NUL"));
    }
    Ok(&field[..end])
}
pub(super) fn octal_field(field: &[u8], empty: bool) -> io::Result<()> {
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
pub(super) fn tar_member(block: &[u8; 512]) -> io::Result<TarMember> {
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
