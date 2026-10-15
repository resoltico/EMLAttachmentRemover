//! Captured Win32 path normalization. No filesystem mutation or lossy Unicode.
use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    io,
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::{Component, Path, PathBuf, Prefix},
};
use windows_sys::Win32::{
    Globalization::{CSTR_EQUAL, CompareStringOrdinal},
    Storage::FileSystem::GetFullPathNameW,
};
const MAX_UNITS: usize = 32_768;
// Conservative operational content ceiling, excluding NUL and including the
// extended prefix. This is below the documented approximate32767-character API
// maximum; it is not a promise every filesystem accepts every length.
const MAX_EXTENDED_CONTENT: usize = 32_766;
fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "Choose an ordinary DOS/UNC path without device names, streams or unsupported verbatim components",
    )
}
fn ordinal_eq(value: &[u16], expected: &str) -> io::Result<bool> {
    let expected: Vec<_> = expected.encode_utf16().collect();
    if value.len() != expected.len() {
        return Ok(false);
    }
    // SAFETY: both live slices cover their explicit checked i32 lengths. The OS
    // ordinal comparison operates on UTF-16 units; no terminating NUL is needed.
    let result = unsafe {
        CompareStringOrdinal(
            value.as_ptr(),
            i32::try_from(value.len()).map_err(|_| invalid())?,
            expected.as_ptr(),
            i32::try_from(expected.len()).map_err(|_| invalid())?,
            1,
        )
    };
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(result == CSTR_EQUAL)
    }
}
fn structural_component(value: &[u16], maximum: usize) -> io::Result<()> {
    if value.is_empty()
        || value.len() > maximum
        || value == [46]
        || value == [46, 46]
        || value.iter().any(|unit| matches!(*unit, 0 | 47 | 58 | 92))
    {
        return Err(invalid());
    }
    Ok(())
}
fn component_units(value: &[u16], maximum: usize) -> io::Result<()> {
    structural_component(value, maximum)?;
    let mut end = value
        .iter()
        .position(|unit| *unit == 46)
        .unwrap_or(value.len());
    while end != 0 && value[end - 1] == 32 {
        end -= 1;
    }
    let stem = &value[..end];
    for name in ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$", "CLOCK$"] {
        if ordinal_eq(stem, name)? {
            return Err(invalid());
        }
    }
    if stem.len() == 4
        && (ordinal_eq(&stem[..3], "COM")? || ordinal_eq(&stem[..3], "LPT")?)
        && ((48..=57).contains(&stem[3]) || [0xb9, 0xb2, 0xb3].contains(&stem[3]))
    {
        return Err(invalid());
    }
    Ok(())
}
/// Validate one native component before an open/create, including generated
/// components after truncation. COM0/LPT0 and CLOCK$ are conservative exclusions
/// in addition to documented reserved stems and console aliases.
pub fn validate_component(value: &OsStr, maximum: usize) -> io::Result<()> {
    component_units(&value.encode_wide().collect::<Vec<_>>(), maximum)
}
fn unc_prefix(server: &OsStr, share: &OsStr) -> io::Result<()> {
    structural_component(&server.encode_wide().collect::<Vec<_>>(), MAX_UNITS)?;
    let share: Vec<_> = share.encode_wide().collect();
    structural_component(&share, MAX_UNITS)?;
    let end = share
        .iter()
        .rposition(|unit| ![32, 46].contains(unit))
        .map_or(0, |position| position + 1);
    // Conservative namespace comparison; original units are never rewritten.
    for namespace in ["PIPE", "MAILSLOT", "IPC$"] {
        if ordinal_eq(&share[..end], namespace)? {
            return Err(invalid());
        }
    }
    Ok(())
}
fn inspect(path: &Path, verbatim: bool) -> io::Result<()> {
    let units: Vec<_> = path.as_os_str().encode_wide().collect();
    if units.is_empty()
        || units.len()
            > (if verbatim {
                MAX_EXTENDED_CONTENT
            } else {
                MAX_UNITS
            })
        || units.contains(&0)
    {
        return Err(invalid());
    }
    if verbatim
        && (units.contains(&47)
            || units
                .split(|unit| *unit == 92)
                .any(|part| part == [46] || part == [46, 46]))
    {
        return Err(invalid());
    }
    for part in path.components() {
        match part {
            Component::Prefix(prefix) => match prefix.kind() {
                Prefix::Disk(drive) if !verbatim && drive.is_ascii_alphabetic() => {}
                Prefix::VerbatimDisk(drive) if verbatim && drive.is_ascii_alphabetic() => {}
                Prefix::UNC(server, share) if !verbatim => {
                    unc_prefix(server, share)?;
                }
                Prefix::VerbatimUNC(server, share) if verbatim => {
                    unc_prefix(server, share)?;
                }
                _ => return Err(invalid()),
            },
            Component::Normal(name) => validate_component(name, MAX_UNITS)?,
            Component::CurDir | Component::ParentDir if verbatim => return Err(invalid()),
            _ => {}
        }
    }
    if verbatim && !path.is_absolute() {
        return Err(invalid());
    }
    Ok(())
}
fn full_path(path: &Path) -> io::Result<PathBuf> {
    let input: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut output = vec![0; MAX_UNITS + 1];
    // SAFETY: terminated live input, writable output covers its stated capacity;
    // optional file-part output is null. One bounded call avoids a relative
    // sizing/retry race against ambient directory/drive state.
    let written = unsafe {
        GetFullPathNameW(
            input.as_ptr(),
            output.len() as u32,
            output.as_mut_ptr(),
            std::ptr::null_mut(),
        )
    };
    if written == 0 {
        return Err(io::Error::last_os_error());
    }
    if written as usize >= output.len() || written as usize > MAX_UNITS {
        return Err(invalid());
    }
    output.truncate(written as usize);
    Ok(PathBuf::from(OsString::from_wide(&output)))
}
fn extended(path: &Path) -> io::Result<PathBuf> {
    inspect(path, false)?;
    if !path.is_absolute() {
        return Err(invalid());
    }
    let input: Vec<_> = path.as_os_str().encode_wide().collect();
    let mut result: Vec<u16> = if input.starts_with(&[92, 92]) {
        "\\\\?\\UNC\\".encode_utf16().collect()
    } else {
        "\\\\?\\".encode_utf16().collect()
    };
    result.extend_from_slice(if input.starts_with(&[92, 92]) {
        &input[2..]
    } else {
        &input
    });
    let result = PathBuf::from(OsString::from_wide(&result));
    inspect(&result, true)?;
    Ok(result)
}
/// Captured native address expressions, separate from physical directory bindings.
/// The core separately holds/binds its captured working directory and explicit roots.
pub struct PathContext {
    working: PathBuf,
    rooted: PathBuf,
    drives: BTreeMap<u8, PathBuf>,
}
impl PathContext {
    /// Capture Win32 directory/drive resolution state once, before admitting work.
    /// No process working directory, registry or saved preference is changed.
    /// The caller must not mutate process cwd/drive state during admission; there
    /// is no OS atomic snapshot spanning every per-drive current directory.
    pub fn capture() -> io::Result<Self> {
        let working = full_path(Path::new("."))?;
        let rooted = working
            .ancestors()
            .last()
            .filter(|path| path.is_absolute())
            .ok_or_else(invalid)?
            .to_owned();
        let mut drives = BTreeMap::new();
        let working_drive = match working.components().next() {
            Some(Component::Prefix(prefix)) => match prefix.kind() {
                Prefix::Disk(drive) => Some(drive.to_ascii_uppercase()),
                _ => None,
            },
            _ => None,
        };
        for drive in b'A'..=b'Z' {
            if working_drive == Some(drive) {
                drives.insert(drive, working.clone());
                continue;
            }
            let expression = OsString::from_wide(&[u16::from(drive), 58, 46]);
            drives.insert(drive, full_path(Path::new(&expression))?);
        }
        inspect(&working, false)?;
        inspect(&rooted, false)?;
        Ok(Self {
            working,
            rooted,
            drives,
        })
    }
    /// Captured ordinary absolute working-directory spelling, not an identity proof.
    pub fn working_directory(&self) -> &Path {
        &self.working
    }
    /// Resolve one native request using captured state and ordinary Win32
    /// normalization, or the conservative dot-free user-verbatim subset.
    pub fn resolve(&self, value: &Path) -> io::Result<PathBuf> {
        let raw: Vec<_> = value.as_os_str().encode_wide().collect();
        let verbatim = raw.starts_with(&[92, 92, 63, 92]);
        if verbatim {
            inspect(value, true)?;
            return Ok(value.to_owned());
        }
        let normalized_slashes = OsString::from_wide(
            &raw.iter()
                .map(|unit| if *unit == 47 { 92 } else { *unit })
                .collect::<Vec<_>>(),
        );
        let value = Path::new(&normalized_slashes);
        inspect(value, false)?;
        let anchored = match value.components().next() {
            Some(Component::Prefix(prefix)) => match prefix.kind() {
                Prefix::Disk(drive) if !value.has_root() => {
                    let base = self
                        .drives
                        .get(&drive.to_ascii_uppercase())
                        .ok_or_else(invalid)?;
                    let units: Vec<_> = value.as_os_str().encode_wide().collect();
                    base.join(OsString::from_wide(&units[2..]))
                }
                Prefix::Disk(_) | Prefix::UNC(..) => value.to_owned(),
                _ => return Err(invalid()),
            },
            _ if value.has_root() => self.rooted.join(value),
            _ => self.working.join(value),
        };
        extended(&full_path(&anchored)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_component_validation_rejects_devices_streams_and_generated_reserved_stems() {
        for name in [
            "NUL",
            "con.txt",
            "CONIN$.eml",
            "conout$",
            "CLOCK$",
            "COM0",
            "com9.ext",
            "LPT¹.eml",
            "com²",
            "LPT³",
            "a:b",
            "a/b",
            "a\\b",
            ".",
            "..",
            "a\0b",
            "CON.no-attachments.eml",
            "COM1.no-attachments.eml",
        ] {
            assert!(validate_component(OsStr::new(name), 255).is_err(), "{name}");
        }
        for name in [
            "normal.eml",
            ".con",
            "COM10",
            "combine",
            "note.no-attachments.eml",
        ] {
            validate_component(OsStr::new(name), 255).unwrap();
        }
        for length in [2, 3, 4, 6, 7] {
            let mut units = vec![120; length];
            units[0] = 0xd800;
            validate_component(&OsString::from_wide(&units), 255).unwrap();
        }
        assert!(validate_component(OsStr::new("abcd"), 3).is_err());
    }
    #[test]
    fn native_capture_preserves_units_and_rejects_verbatim_dots_before_io() {
        let context = PathContext::capture().unwrap();
        for path in [
            r"\\?\C:relative",
            r"\\?\C:\a\..\b",
            r"\\?\C:\a\.\b",
            r"\\?\C:/a",
            r"\\.\pipe\owned",
            r"\\?\GLOBALROOT\Device\owned",
            r"C:\a:stream",
            r"C:\NUL\..\ordinary",
            r"\\server\pipe\owned.eml",
            r"\\server\PiPe\owned.eml",
            r"\\server\MAILSLOT\owned.eml",
            r"\\?\UNC\server\pipe\owned.eml",
            r"\\?\UNC\server\MaIlSlOt\owned.eml",
            r"\\server\IPC$\PIPE\owned.eml",
            r"\\server\pIpE. \owned.eml",
            r"\\?\UNC\server\MAILSLOT.\owned.eml",
            r"\\?\UNC\server\iPc$ \owned.eml",
        ] {
            assert!(context.resolve(Path::new(path)).is_err(), "{path}");
        }
        let unpaired = PathBuf::from(OsString::from_wide(&[120, 0xd800]));
        let actual = context.resolve(&unpaired).unwrap();
        assert!(
            actual
                .as_os_str()
                .encode_wide()
                .collect::<Vec<_>>()
                .ends_with(&[120, 0xd800])
        );
        assert_eq!(
            context
                .resolve(Path::new(r"\\server\share\a\..\b"))
                .unwrap(),
            Path::new(r"\\?\UNC\server\share\b")
        );
        let ordinary = context.resolve(Path::new("owned\\..\\sentinel")).unwrap();
        let expected =
            extended(&full_path(&context.working_directory().join("sentinel")).unwrap()).unwrap();
        assert_eq!(ordinary, expected);
    }
    #[test]
    fn captured_state_survives_owned_child_cwd_change() {
        const CHILD_ROOT: &str = "EML_OWNED_PATH_CAPTURE_TEST_ROOT";
        if let Some(root) = std::env::var_os(CHILD_ROOT) {
            let root = PathBuf::from(root);
            let context = PathContext::capture().unwrap();
            let before = context.resolve(Path::new("sentinel")).unwrap();
            assert_eq!(std::fs::read(&before).unwrap(), b"captured first directory");
            // This is the ONLY cwd mutation: an isolated, exact-one-test child
            // process using only its parent's fresh owned fixture directories.
            std::env::set_current_dir(root.join("second")).unwrap();
            assert_eq!(context.resolve(Path::new("sentinel")).unwrap(), before);
            let Some(Component::Prefix(prefix)) = context.working_directory().components().next()
            else {
                panic!("Native local fixture lacks drive prefix");
            };
            let Prefix::Disk(drive) = prefix.kind() else {
                panic!("Native local fixture requires DOS volume");
            };
            let mut relative = vec![u16::from(drive), 58];
            relative.extend("sentinel".encode_utf16());
            let relative = PathBuf::from(OsString::from_wide(&relative));
            assert_eq!(context.resolve(&relative).unwrap(), before);
            assert_eq!(
                std::fs::read(full_path(&relative).unwrap()).unwrap(),
                b"changed second directory"
            );
            let first: Vec<_> = context
                .working_directory()
                .join("sentinel")
                .as_os_str()
                .encode_wide()
                .collect();
            let rooted = PathBuf::from(OsString::from_wide(&first[2..]));
            assert_eq!(context.resolve(&rooted).unwrap(), before);
            return;
        }
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("first")).unwrap();
        std::fs::create_dir(root.path().join("second")).unwrap();
        std::fs::write(
            root.path().join("first/sentinel"),
            b"captured first directory",
        )
        .unwrap();
        std::fs::write(
            root.path().join("second/sentinel"),
            b"changed second directory",
        )
        .unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "windows::path::tests::captured_state_survives_owned_child_cwd_change",
                "--test-threads=1",
                "--nocapture",
            ])
            .env(CHILD_ROOT, root.path())
            .current_dir(root.path().join("first"))
            .status()
            .unwrap();
        assert!(status.success());
        root.close().unwrap();
    }
    fn read_direct_ordinary(path: &Path) -> io::Result<Vec<u8>> {
        use std::{
            io::Read,
            os::windows::io::{FromRawHandle, OwnedHandle},
        };
        use windows_sys::Win32::{
            Foundation::{GENERIC_READ, INVALID_HANDLE_VALUE},
            Storage::FileSystem::{
                CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_TYPE_DISK, GetFileType,
                OPEN_EXISTING,
            },
        };
        let path: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        // SAFETY: owned-fixture ordinary absolute expression, terminated/live;
        // no inherited/template handle. Successful handle is owned immediately.
        let raw = unsafe {
            CreateFileW(
                path.as_ptr(),
                GENERIC_READ,
                FILE_SHARE_READ,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                std::ptr::null_mut(),
            )
        };
        if raw == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: successful native call transfers exactly one owned handle.
        let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
        // SAFETY: the just-owned live handle remains valid through this query.
        if unsafe { GetFileType(raw) } != FILE_TYPE_DISK {
            return Err(io::Error::other(
                "Owned direct path control did not open a disk file",
            ));
        }
        let mut file = std::fs::File::from(handle);
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        Ok(bytes)
    }
    #[test]
    fn actual_junction_dotdot_matches_direct_win32_lexical_selection() {
        let root = tempfile::tempdir().unwrap();
        let ordinary_root = ordinary_owned_dos_root(root.path());
        let a = ordinary_root.join("a");
        let b = ordinary_root.join("b");
        let deep = b.join("deep");
        let link = a.join("link");
        std::fs::create_dir(&a).unwrap();
        std::fs::create_dir(&b).unwrap();
        std::fs::create_dir(&deep).unwrap();
        std::fs::create_dir(&link).unwrap();
        std::fs::write(a.join("sentinel"), b"lexical a sentinel").unwrap();
        std::fs::write(b.join("sentinel"), b"junction target parent b sentinel").unwrap();
        std::fs::write(
            deep.join("through-junction"),
            b"actual junction I/O sentinel",
        )
        .unwrap();
        let writer = crate::windows::test_reparse::writer(&link).unwrap();
        crate::windows::test_reparse::set_mount_point(&writer, &deep).unwrap();
        drop(writer);
        assert_eq!(
            std::fs::canonicalize(&link).unwrap(),
            std::fs::canonicalize(&deep).unwrap()
        );
        assert_eq!(
            read_direct_ordinary(&link.join("through-junction")).unwrap(),
            b"actual junction I/O sentinel"
        );
        let expression = link.join("..").join("sentinel");
        let direct = read_direct_ordinary(&expression).unwrap();
        assert_eq!(direct, b"lexical a sentinel");
        let captured = PathContext::capture()
            .unwrap()
            .resolve(&expression)
            .unwrap();
        assert_eq!(std::fs::read(captured).unwrap(), direct);
        assert_ne!(direct, std::fs::read(b.join("sentinel")).unwrap());
        std::fs::remove_dir(&link).unwrap();
        assert_eq!(
            std::fs::read(deep.join("through-junction")).unwrap(),
            b"actual junction I/O sentinel"
        );
        root.close().unwrap();
    }
    fn ordinary_owned_dos_root(path: &Path) -> PathBuf {
        assert!(
            path.is_absolute(),
            "Native junction fixture must be absolute"
        );
        let Some(Component::Prefix(prefix)) = path.components().next() else {
            panic!("Owned fixture has no DOS prefix");
        };
        match prefix.kind() {
            Prefix::Disk(_) => path.to_owned(),
            Prefix::VerbatimDisk(_) => {
                let raw: Vec<_> = path.as_os_str().encode_wide().collect();
                assert!(raw.starts_with(&[92, 92, 63, 92]));
                PathBuf::from(OsString::from_wide(&raw[4..]))
            }
            _ => panic!("Native local junction fixture requires DOS volume; no UNC recasting"),
        }
    }
    #[test]
    fn long_ordinary_capture_and_user_verbatim_read_owned_native_file() {
        let root = tempfile::tempdir().unwrap();
        let context = PathContext::capture().unwrap();
        let ordinary = ordinary_owned_dos_root(root.path());
        let mut directory = ordinary.clone();
        for letter in ['a', 'b', 'c'] {
            directory.push(letter.to_string().repeat(100));
        }
        let bound_directory = context.resolve(&directory).unwrap();
        std::fs::create_dir_all(&bound_directory).unwrap();
        let file = directory.join("owned.eml");
        let operation = context.resolve(&file).unwrap();
        assert!(file.as_os_str().encode_wide().count() > 260);
        std::fs::write(&operation, b"long owned path bytes").unwrap();
        assert_eq!(std::fs::read(&operation).unwrap(), b"long owned path bytes");
        assert_eq!(context.resolve(&operation).unwrap(), operation);
        match read_direct_ordinary(&file) {
            Ok(bytes) => {
                assert_eq!(bytes, b"long owned path bytes");
                eprintln!(
                    "Native direct ordinary long-path open succeeded under actual runner policy"
                );
            }
            Err(error) => eprintln!(
                "Native direct ordinary long-path open refused under actual runner policy: {error}; captured extended path succeeded"
            ),
        }
        std::fs::remove_file(&operation).unwrap();
        for _ in 0..3 {
            std::fs::remove_dir(context.resolve(&directory).unwrap()).unwrap();
            directory.pop();
        }
        root.close().unwrap();
    }
}
