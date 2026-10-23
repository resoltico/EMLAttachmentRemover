use super::tar_format::*;
use super::zip_format::*;
use super::*;
use std::io::{Cursor, Write};

fn fixture_epoch() -> PackageEpoch {
    PackageEpoch::new(1_609_459_201).unwrap()
}

#[cfg(target_os = "macos")]
#[test]
fn actual_ditto_profile_checks_original_records_descriptors_and_materials() {
    let owned = tempfile::tempdir().unwrap();
    let root = "EML Attachment Remover.app";
    let (path, expected) = ditto_fixture(owned.path(), root);
    let snapshot = ArchiveSnapshot::read(&path).unwrap();
    snapshot
        .inspect_ditto_app(root, &expected, fixture_epoch())
        .unwrap();
    assert!(
        inspect_zip_directory(&mut Cursor::new(snapshot.bytes()), fixture_epoch()).is_err(),
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
        cursor += 46 + name_len + usize::from(short(header, 30)) + usize::from(short(header, 32));
    }
    let mut bad = bytes.to_vec();
    bad[descriptor.unwrap() + 4] ^= 1;
    assert!(
        ArchiveSnapshot { bytes: bad }
            .inspect_ditto_app(root, &expected, fixture_epoch())
            .unwrap_err()
            .to_string()
            .contains("descriptor differs")
    );
    let mut bad = bytes.to_vec();
    bad[central + 46 + usize::from(short(&bytes[central..central + 46], 28))] ^= 1;
    assert!(
        ArchiveSnapshot { bytes: bad }
            .inspect_ditto_app(root, &expected, fixture_epoch())
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
            .inspect_ditto_app(root, &expected, fixture_epoch())
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
            .inspect_ditto_app(root, &wrong_material, fixture_epoch())
            .unwrap_err()
            .to_string()
            .contains("expected bytes differ")
    );
}

#[cfg(target_os = "macos")]
fn ditto_fixture(parent: &Path, root: &str) -> (PathBuf, BTreeMap<String, (ByteIdentity, u32)>) {
    use std::os::unix::fs::PermissionsExt;
    let app = parent.join(root);
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
    let time = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_609_459_200);
    for relative in payloads.keys().map(String::as_str).chain([
        "",
        "Contents",
        "Contents/MacOS",
        "Contents/Resources",
    ]) {
        fs::File::open(app.join(relative))
            .unwrap()
            .set_times(fs::FileTimes::new().set_accessed(time).set_modified(time))
            .unwrap();
    }
    let path = parent.join("format.zip");
    assert!(
        std::process::Command::new("/usr/bin/ditto")
            .env("TZ", "UTC")
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
    (path, expected)
}

fn archive() -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    writer
        .start_file(
            "package/README.txt",
            zip::write::SimpleFileOptions::default()
                .last_modified_time(zip::DateTime::from_date_and_time(2021, 1, 1, 0, 0, 0).unwrap())
                .system(zip::System::Unix),
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
        fixture_epoch(),
    )
    .unwrap()
    .into_inner();
    let tar = encode_cli_tar_gz(
        Vec::new(),
        "package",
        "tool",
        binary,
        &materials,
        fixture_epoch(),
    )
    .unwrap();
    let zip = ArchiveSnapshot { bytes: zip }
        .extract_cli_zip("package", "tool", &materials, owned.path(), fixture_epoch())
        .unwrap();
    let tar = ArchiveSnapshot { bytes: tar }
        .extract_cli_tar_gz("package", "tool", &materials, owned.path(), fixture_epoch())
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
            .last_modified_time(zip::DateTime::from_date_and_time(2021, 1, 1, 0, 0, 0).unwrap())
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
fn cli_evidence_reader_requires_one_bounded_crc_checked_material() {
    let name = "licenses/windows-runtime/link-evidence.json";
    let mut materials = synthetic_materials();
    let missing = ArchiveSnapshot {
        bytes: synthetic_cli_zip(&materials),
    };
    assert!(
        missing
            .read_cli_zip_material("package", name, 1024, fixture_epoch())
            .is_err()
    );
    materials.insert(
        name.into(),
        b"owned synthetic evidence; not native provenance".to_vec(),
    );
    let bytes = synthetic_cli_zip(&materials);
    let snapshot = ArchiveSnapshot {
        bytes: bytes.clone(),
    };
    assert_eq!(
        snapshot
            .read_cli_zip_material("package", name, 1024, fixture_epoch())
            .unwrap(),
        materials[name]
    );
    assert!(
        snapshot
            .read_cli_zip_material("package", name, 2, fixture_epoch())
            .is_err()
    );
    assert!(
        snapshot
            .read_cli_zip_material("package", "../outside", 1024, fixture_epoch())
            .is_err()
    );
    let mut changed = bytes;
    let body = changed
        .windows(materials[name].len())
        .position(|window| window == materials[name])
        .unwrap();
    changed[body] ^= 1;
    assert!(
        ArchiveSnapshot { bytes: changed }
            .read_cli_zip_material("package", name, 1024, fixture_epoch())
            .is_err()
    );
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
        header.set_mtime(1_609_459_201);
        header.set_size(bytes.len() as u64);
        header.set_cksum();
        writer.append(&header, *bytes).unwrap();
    }
    writer.finish().unwrap();
    writer.into_inner().unwrap()
}

fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::GzBuilder::new()
        .mtime(1_609_459_201)
        .write(Vec::new(), flate2::Compression::fast());
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap()
}

fn inspect_tar(bytes: Vec<u8>) -> io::Result<Vec<TarMember>> {
    ArchiveSnapshot { bytes }.walk_tar(
        |_, body| io::copy(body, &mut io::sink()).map(|_| ()),
        fixture_epoch(),
    )
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
        .walk_tar(
            |_, _| {
                visited = true;
                Ok(())
            },
            fixture_epoch(),
        );
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
        .extract_cli_tar_gz("package", "tool", &materials, owned.path(), fixture_epoch())
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
                .extract_cli_tar_gz("package", "tool", &materials, owned.path(), fixture_epoch())
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
            .extract_cli_tar_gz("package", "tool", &materials, owned.path(), fixture_epoch())
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
        .extract_cli_zip("package", "tool", &materials, owned.path(), fixture_epoch())
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
            .extract_cli_zip("package", "tool", &materials, owned.path(), fixture_epoch())
            .is_err()
    );
    assert_eq!(fs::read_dir(owned.path()).unwrap().count(), 0);
    // An incomplete expected inventory cannot redefine completeness either.
    assert!(
        snapshot
            .extract_cli_zip("package", "tool", &missing, owned.path(), fixture_epoch())
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
    let result = snapshot.extract_cli_zip("package", "tool", &materials, &parent, fixture_epoch());
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
            .extract_cli_zip("package", "tool", &materials, owned.path(), fixture_epoch())
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
            .extract_cli_zip("package", "tool", &materials, owned.path(), fixture_epoch())
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
        .extract_cli_zip("package", "tool", &materials, owned.path(), fixture_epoch())
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
    let members = inspect_zip_directory(&mut Cursor::new(&bytes), fixture_epoch()).unwrap();
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
    assert!(inspect_zip_directory(&mut Cursor::new(bytes), fixture_epoch()).is_err());
}

#[test]
fn invalid_directory_extents_and_unsupported_zip_features_fail() {
    let original = archive();
    for (field, value) in [(4, 1), (10, u16::MAX), (20, 1)] {
        let mut bytes = original.clone();
        let end = bytes.len() - 22;
        bytes[end + field..end + field + 2].copy_from_slice(&value.to_le_bytes());
        assert!(inspect_zip_directory(&mut Cursor::new(bytes), fixture_epoch()).is_err());
    }
    for count in [0, 1, 21] {
        assert!(inspect_zip_directory(&mut Cursor::new(vec![0; count]), fixture_epoch()).is_err());
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
        assert!(inspect_zip_directory(&mut Cursor::new(bytes), fixture_epoch()).is_err());
    }
    for mode in [0o120777_u32, 0o104755, 0o040755] {
        let mut bytes = original.clone();
        bytes[central + 38..central + 42].copy_from_slice(&(mode << 16).to_le_bytes());
        assert!(inspect_zip_directory(&mut Cursor::new(bytes), fixture_epoch()).is_err());
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

#[test]
fn source_epoch_producers_are_repeatable_and_independent_readers_observe_utc_precision() {
    let materials = synthetic_materials();
    let zip_bytes = || {
        encode_cli_zip(
            Cursor::new(Vec::new()),
            "package",
            "tool",
            b"exact executable",
            &materials,
            fixture_epoch(),
        )
        .unwrap()
        .into_inner()
    };
    let tar_bytes = || {
        encode_cli_tar_gz(
            Vec::new(),
            "package",
            "tool",
            b"exact executable",
            &materials,
            fixture_epoch(),
        )
        .unwrap()
    };
    let zip = zip_bytes();
    assert_eq!(zip, zip_bytes());
    let mut independent_zip = zip::ZipArchive::new(Cursor::new(&zip)).unwrap();
    for index in 0..independent_zip.len() {
        let mut entry = independent_zip.by_index(index).unwrap();
        let date = entry.last_modified().unwrap();
        assert_eq!(
            (
                date.year(),
                date.month(),
                date.day(),
                date.hour(),
                date.minute(),
                date.second()
            ),
            (2021, 1, 1, 0, 0, 0)
        );
        let expected_mode = if entry.is_dir() || entry.name().unwrap() == "package/tool" {
            0o755
        } else {
            0o644
        };
        assert_eq!(entry.unix_mode().unwrap() & 0o7777, expected_mode);
        let mut body = Vec::new();
        entry.read_to_end(&mut body).unwrap(); // includes the library's CRC verification
        if entry.name().unwrap() == "package/tool" {
            assert_eq!(body, b"exact executable");
        }
    }
    let tar = tar_bytes();
    assert_eq!(tar, tar_bytes());
    assert_eq!(&tar[4..8], &1_609_459_201_u32.to_le_bytes());
    let mut independent_tar = tar::Archive::new(flate2::read::GzDecoder::new(tar.as_slice()));
    for entry in independent_tar.entries().unwrap() {
        let mut entry = entry.unwrap();
        assert_eq!(entry.header().mtime().unwrap(), 1_609_459_201);
        let executable = entry.path().unwrap().as_ref() == Path::new("package/tool");
        assert_eq!(
            entry.header().mode().unwrap(),
            if executable || entry.header().entry_type().is_dir() {
                0o755
            } else {
                0o644
            }
        );
        let mut body = Vec::new();
        entry.read_to_end(&mut body).unwrap();
        if executable {
            assert_eq!(body, b"exact executable");
        }
    }
}

#[test]
fn epoch_mismatches_fail_even_when_zip_headers_and_tar_checksums_agree() {
    let bytes = archive();
    let central = long(&bytes[bytes.len() - 22..], 16) as usize;
    // Both local and central claim the same valid but wrong calendar date.
    let mut wrong = bytes.clone();
    wrong[12..14].copy_from_slice(&0x5222_u16.to_le_bytes());
    wrong[central + 14..central + 16].copy_from_slice(&0x5222_u16.to_le_bytes());
    assert!(
        inspect_zip_directory(&mut Cursor::new(&wrong), fixture_epoch())
            .unwrap_err()
            .to_string()
            .contains("timestamp")
    );
    assert!(
        inspect_zip_directory(
            &mut Cursor::new(&bytes),
            PackageEpoch::new(1_609_459_202).unwrap()
        )
        .is_err()
    );
    let mut local_only = bytes;
    local_only[10..12].copy_from_slice(&1_u16.to_le_bytes());
    assert!(
        inspect_zip_directory(&mut Cursor::new(local_only), fixture_epoch())
            .unwrap_err()
            .to_string()
            .contains("disagree")
    );

    let raw = ustar(&[("package/file", b"abc")]);
    let mut wrong_tar = raw.clone();
    // Valid octal and recomputed header checksum must not authorize a wrong epoch.
    wrong_tar[136..148].copy_from_slice(b"13773463002\0");
    fix_tar_checksum(&mut wrong_tar);
    assert!(
        inspect_tar(gzip(&wrong_tar))
            .unwrap_err()
            .to_string()
            .contains("timestamp")
    );
    let mut wrong_gzip = gzip(&raw);
    wrong_gzip[4..8].copy_from_slice(&1_609_459_200_u32.to_le_bytes());
    assert!(
        inspect_tar(wrong_gzip)
            .unwrap_err()
            .to_string()
            .contains("Gzip timestamp")
    );
}

#[cfg(target_os = "macos")]
#[test]
fn native_unix_times_must_match_source_zip_precision_even_when_both_copies_agree() {
    let owned = tempfile::tempdir().unwrap();
    let root = "EML Attachment Remover.app";
    let (path, expected) = ditto_fixture(owned.path(), root);
    let mut bytes = ArchiveSnapshot::read(&path).unwrap().bytes;
    let central = long(&bytes[bytes.len() - 22..], 16) as usize;
    let name_length = usize::from(short(&bytes[central..central + 46], 28));
    let local = long(&bytes[central..central + 46], 42) as usize;
    let local_extra = local + 30 + name_length;
    let central_extra = central + 46 + name_length;
    for relative in [4, 8] {
        bytes[local_extra + relative..local_extra + relative + 4]
            .copy_from_slice(&1_609_459_201_u32.to_le_bytes());
        bytes[central_extra + relative..central_extra + relative + 4]
            .copy_from_slice(&1_609_459_201_u32.to_le_bytes());
    }
    assert!(
        ArchiveSnapshot { bytes }
            .inspect_ditto_app(root, &expected, fixture_epoch())
            .unwrap_err()
            .to_string()
            .contains("ditto metadata")
    );
}
