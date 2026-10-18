//! Read-only linker reproducer inspection; distinct from shipping archive qualification.
use crate::archive::ByteIdentity;
use std::{collections::BTreeSet, io::Read};
/// One bounded byte observation from the actual linker's reproducer.
#[derive(Debug, serde::Serialize)]
pub struct InputObservation {
    /// Opaque relative reproducer member name; never a filesystem operation path.
    pub member: String,
    /// Identity of exactly the retained member bytes, not source or legal approval.
    pub identity: ByteIdentity,
}

/// Read only the actual linker's reproducer; never extract or execute its files.
/// This is separate from the narrow shipping archive formats and their policy.
pub fn inspect(bytes: &[u8]) -> Result<Vec<InputObservation>, Box<dyn std::error::Error>> {
    if bytes.len() > 512 * 1024 * 1024 {
        return Err("Native reproducer snapshot exceeds its byte bound".into());
    }
    let mut archive = tar::Archive::new(bytes);
    let mut names = BTreeSet::new();
    let mut inputs = Vec::new();
    let mut total = 0u64;
    let mut count = 0usize;
    for entry in archive.entries()? {
        let mut entry = entry?;
        count += 1;
        if count > 4096 {
            return Err("Native reproducer member count exceeds its bound".into());
        }
        let path = entry.path_bytes();
        let member = std::str::from_utf8(&path)?.trim_end_matches('/').to_owned();
        if member.is_empty()
            || member.len() > 32768
            || member.starts_with('/')
            || member.contains(['\\', '\0'])
            || member
                .split('/')
                .any(|part| matches!(part, "" | "." | ".."))
            || !names.insert(member.clone())
        {
            return Err(
                "Native reproducer member name is ambiguous, duplicated or outside its bound"
                    .into(),
            );
        }
        if entry.header().entry_type().is_dir() {
            continue;
        }
        if !entry.header().entry_type().is_file() {
            return Err("Native reproducer contains a nonordinary input".into());
        }
        let size = entry.size();
        total = total
            .checked_add(size)
            .ok_or("Native reproducer size overflow")?;
        if size > 256 * 1024 * 1024 || total > 512 * 1024 * 1024 {
            return Err("Native reproducer input bytes exceed their bounds".into());
        }
        let mut contents = Vec::new();
        (&mut entry).take(size + 1).read_to_end(&mut contents)?;
        if contents.len() as u64 != size {
            return Err("Native reproducer input is truncated".into());
        }
        inputs.push(InputObservation {
            member,
            identity: ByteIdentity::of(&contents),
        });
    }
    if !inputs.iter().any(|input| {
        !input.member.ends_with("/response.txt") && !input.member.ends_with("/version.txt")
    }) || !inputs
        .iter()
        .any(|input| input.member.ends_with("/response.txt"))
    {
        return Err("Native reproducer omits inputs or its linker command record".into());
    }
    Ok(inputs)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn archive(extra: &[(&str, &[u8], tar::EntryType)]) -> Vec<u8> {
        let mut tar = tar::Builder::new(Vec::new());
        for (name, body, kind) in [(
            "reproducer/response.txt",
            b"not executed".as_slice(),
            tar::EntryType::Regular,
        )]
        .into_iter()
        .chain(extra.iter().copied())
        {
            let mut header = tar::Header::new_gnu();
            // Write an exact raw name so the fixture can independently exercise
            // our rejection rather than tar::Builder's path admission.
            assert!(name.len() < 100);
            header.as_mut_bytes()[..name.len()].copy_from_slice(name.as_bytes());
            header.set_size(body.len() as u64);
            header.set_mode(0o644);
            header.set_entry_type(kind);
            header.set_cksum();
            tar.append(&header, body).unwrap();
        }
        tar.into_inner().unwrap()
    }
    #[test]
    fn reproducer_member_identity_is_of_actual_payload_bytes() {
        let archive = archive(&[("reproducer/object.o", b"abc", tar::EntryType::Regular)]);
        let inputs = inspect(&archive).unwrap();
        let input = inputs
            .iter()
            .find(|input| input.member == "reproducer/object.o")
            .unwrap();
        assert_eq!(input.identity.byte_length, 3);
        assert_eq!(
            input.identity.sha256,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
    #[test]
    fn reproducer_rejects_duplicates_traversal_links_and_oversized_members() {
        assert!(inspect(&archive(&[])).is_err());
        for (name, kind) in [
            ("reproducer/response.txt", tar::EntryType::Regular),
            ("reproducer/../outside", tar::EntryType::Regular),
            ("/outside", tar::EntryType::Regular),
            ("reproducer/alias", tar::EntryType::Symlink),
        ] {
            assert!(inspect(&archive(&[(name, b"", kind)])).is_err());
        }
        let mut header = tar::Header::new_gnu();
        header.set_path("reproducer/oversized.o").unwrap();
        header.set_size(256 * 1024 * 1024 + 1);
        header.set_entry_type(tar::EntryType::Regular);
        header.set_cksum();
        let mut bytes = header.as_bytes().to_vec();
        bytes.resize(1536, 0);
        assert!(inspect(&bytes).unwrap_err().to_string().contains("bounds"));
        assert!(inspect(&archive(&[])[..511]).is_err());
    }
}
