//! Bounded native-fixture evidence primitives; absent from ordinary products.
use crate::{FileSignature, Identity, SyncReceipt};
use std::{
    fs,
    io::{self, Write},
    path::Path,
};

/// Exact native identity fields, without lossy paths or a path-derived identifier.
pub fn identity_json(identity: Identity) -> String {
    match identity {
        Identity::Windows { volume, file } => format!("{{\"volume\":{volume},\"file\":{file:?}}}"),
        Identity::Posix { device, inode } => format!("{{\"device\":{device},\"inode\":{inode}}}"),
    }
}
/// Every existing signature field, including backend-native modify/change pairs.
pub fn signature_json(signature: FileSignature) -> String {
    format!(
        "{{\"identity\":{},\"size\":{},\"modified_native\":{:?},\"changed_native\":{:?}}}",
        identity_json(signature.identity),
        signature.size,
        signature.modified_native,
        signature.changed_native
    )
}
/// Numeric native cause, retaining an explicit unobserved value.
pub fn native_code_json(code: Option<i64>) -> String {
    code.map_or_else(|| "null".into(), |code| code.to_string())
}
/// The actual method/state/native cause returned by a synchronization request.
pub fn sync_json(receipt: SyncReceipt) -> String {
    format!(
        "{{\"method\":\"{:?}\",\"state\":\"{:?}\",\"native_code\":{}}}",
        receipt.method,
        receipt.state,
        native_code_json(receipt.native_code)
    )
}
/// Retain one bounded record without replacing an earlier observation.
/// Callers supply the owned fixture directory and a fixed single-component name.
pub fn write_record(directory: &Path, name: &'static str, contents: &str) -> io::Result<()> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_'))
        || contents.len() > 256 * 1024
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Invalid bounded native fixture record",
        ));
    }
    fs::create_dir_all(directory)?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(directory.join(name))?;
    file.write_all(contents.as_bytes())?;
    file.sync_all()
}

/// Lossless units of a fixture name: Unix bytes or Windows UTF-16 units.
pub fn native_units(name: &std::ffi::OsStr) -> Vec<u32> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        name.as_bytes().iter().map(|&x| u32::from(x)).collect()
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        name.encode_wide().map(u32::from).collect()
    }
}
/// Actual cleanup fields, including an unowned/residual native name and first cause.
pub fn cleanup_json(receipt: &crate::CleanupReceipt) -> String {
    let name = receipt
        .residual_name
        .as_ref()
        .map_or_else(|| "null".into(), |name| format!("{:?}", native_units(name)));
    format!(
        "{{\"state\":\"{:?}\",\"residual_name\":{name},\"native_code\":{}}}",
        receipt.state,
        native_code_json(receipt.native_code)
    )
}
/// Literal native API return, distinct from engine phases and final qualification.
pub fn native_edge_json(receipt: Option<crate::NativePublicationReceipt>) -> String {
    receipt.map_or_else(
        || "null".into(),
        |r| {
            format!(
                "{{\"primitive\":\"{}\",\"succeeded\":{},\"raw_return\":{},\"native_code\":{}}}",
                r.primitive,
                r.succeeded,
                native_code_json(r.raw_return.map(i64::from)),
                native_code_json(r.native_code)
            )
        },
    )
}
/// Read the complete small synthetic candidate through its already held file.
/// This is fixture observation, not a pathname reopen or mutation capability.
pub fn held_file_bytes(file: &fs::File) -> io::Result<Vec<u8>> {
    let length = usize::try_from(file.metadata()?.len())
        .map_err(|_| io::Error::other("Fixture length overflow"))?;
    if length > 4096 {
        return Err(io::Error::other(
            "Publication fixture exceeds its byte oracle",
        ));
    }
    let mut bytes = vec![0; length];
    let read = |buffer: &mut [u8], offset: u64| {
        #[cfg(unix)]
        {
            use std::os::unix::fs::FileExt;
            file.read_at(buffer, offset)
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::FileExt;
            file.seek_read(buffer, offset)
        }
    };
    let mut count = 0;
    while count < length {
        let got = read(&mut bytes[count..], count as u64)?;
        if got == 0 {
            return Err(io::Error::other("Short publication fixture read"));
        }
        count += got;
    }
    if read(&mut [0; 1], count as u64)? != 0 {
        return Err(io::Error::other("Publication fixture grew"));
    }
    Ok(bytes)
}
/// Sorted no-follow inventory of one finite owned publication fixture.
/// These fixtures have at most six entries/depththree; reject unexpected growth.
pub fn inventory_json(root: &Path) -> io::Result<String> {
    fn walk(
        directory: &mut crate::BoundDirectory,
        path: &[Vec<u32>],
        rows: &mut Vec<(Vec<Vec<u32>>, String)>,
    ) -> io::Result<()> {
        if path.len() > 4 || rows.len() > 32 {
            return Err(io::Error::other(
                "Publication inventory exceeds finite fixture",
            ));
        }
        let names = directory
            .read_entries()?
            .take(35)
            .collect::<io::Result<Vec<_>>>()?;
        if names.len() > 34 {
            return Err(io::Error::other(
                "Publication inventory exceeds finite fixture",
            ));
        }
        for name in names {
            if name == "." || name == ".." {
                continue;
            }
            let observed = directory.observe_child(&name)?;
            let mut child_path = path.to_vec();
            child_path.push(native_units(&name));
            let signature = observed.file.map_or_else(|| "null".into(), signature_json);
            rows.push((child_path.clone(), format!("{{\"path\":{child_path:?},\"kind\":\"{:?}\",\"identity\":{},\"signature\":{signature}}}", observed.kind, identity_json(observed.identity))));
            if observed.kind == crate::EntryKind::Directory {
                let mut child = directory.open_child(&name)?;
                if child.identity() != observed.identity {
                    return Err(io::Error::other("Publication inventory changed binding"));
                }
                walk(&mut child, &child_path, rows)?;
            }
        }
        Ok(())
    }
    let mut directory = crate::BoundDirectory::open(&root.canonicalize()?)?;
    let mut rows = Vec::new();
    walk(&mut directory, &[], &mut rows)?;
    if rows.len() > 32 {
        return Err(io::Error::other(
            "Publication inventory exceeds finite fixture",
        ));
    }
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(format!(
        "[{}]",
        rows.into_iter()
            .map(|(_, row)| row)
            .collect::<Vec<_>>()
            .join(",")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_signature_fields_and_sync_cause_have_exact_literal_encodings() {
        let signature = FileSignature {
            identity: Identity::Windows {
                volume: 7,
                file: [1; 16],
            },
            size: 19,
            modified_native: [10, 0],
            changed_native: [11, 0],
        };
        assert_eq!(
            signature_json(signature),
            "{\"identity\":{\"volume\":7,\"file\":[1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1]},\"size\":19,\"modified_native\":[10, 0],\"changed_native\":[11, 0]}"
        );
        assert_eq!(
            identity_json(Identity::Posix {
                device: 3,
                inode: 4
            }),
            "{\"device\":3,\"inode\":4}"
        );
        assert_eq!(
            sync_json(SyncReceipt {
                method: crate::SyncMethod::FlushFileBuffers,
                state: crate::SyncState::Failed,
                native_code: Some(5)
            }),
            "{\"method\":\"FlushFileBuffers\",\"state\":\"Failed\",\"native_code\":5}"
        );
    }
    #[test]
    fn evidence_writer_preserves_winner_and_rejects_escape_and_oversize() {
        let root = tempfile::tempdir().unwrap();
        write_record(root.path(), "case.json", "{\"native\":true}").unwrap();
        assert_eq!(
            write_record(root.path(), "case.json", "replacement")
                .unwrap_err()
                .kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(
            fs::read(root.path().join("case.json")).unwrap(),
            b"{\"native\":true}"
        );
        assert!(write_record(root.path(), "../outside.json", "{}").is_err());
        assert!(write_record(root.path(), "oversize.json", &"x".repeat(256 * 1024 + 1)).is_err());
        assert!(!root.path().join("oversize.json").exists());
    }
}
