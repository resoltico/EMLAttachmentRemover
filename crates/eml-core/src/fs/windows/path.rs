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
pub(super) const MAX_EXTENDED_CONTENT: usize = 32_766;
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
                Prefix::VerbatimDisk(drive)
                    if verbatim && drive.is_ascii_alphabetic() && units.get(6) == Some(&92) => {}
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
pub(crate) fn openable(path: &Path) -> io::Result<Vec<u16>> {
    validate_operation(path)?;
    Ok(path.as_os_str().encode_wide().chain(Some(0)).collect())
}
pub(super) fn validate_operation(path: &Path) -> io::Result<()> {
    inspect(path, true)
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
    /// Capture resolution from the caller's absolute working-directory expression
    /// and Win32 other-drive state once, before admitting work.
    /// No process working directory, registry or saved preference is changed.
    /// The caller must not mutate process cwd/drive state during admission; there
    /// is no OS atomic snapshot spanning every per-drive current directory.
    pub fn capture(working_directory: &Path) -> io::Result<Self> {
        let verbatim = has_verbatim_prefix(working_directory);
        inspect(working_directory, verbatim)?;
        if !working_directory.is_absolute() {
            return Err(invalid());
        }
        let working = if verbatim {
            working_directory.to_owned()
        } else {
            full_path(working_directory)?
        };
        let rooted = working
            .ancestors()
            .last()
            .filter(|path| path.is_absolute())
            .ok_or_else(invalid)?
            .to_owned();
        let mut drives = BTreeMap::new();
        let working_drive = match working.components().next() {
            Some(Component::Prefix(prefix)) => match prefix.kind() {
                Prefix::Disk(drive) | Prefix::VerbatimDisk(drive) => {
                    Some(drive.to_ascii_uppercase())
                }
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
        inspect(&working, verbatim)?;
        inspect(&rooted, verbatim)?;
        Ok(Self {
            working,
            rooted,
            drives,
        })
    }
    /// Captured absolute ordinary/verbatim working spelling, not an identity proof.
    pub fn working_directory(&self) -> &Path {
        &self.working
    }
    /// Resolve one native request using captured state and ordinary Win32
    /// normalization for ordinary bases. A verbatim base retains literal tail
    /// dots/spaces while resolving dot navigation. Standalone user-verbatim paths
    /// retain the conservative dot-free subset.
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
        // An incomplete UNC introducer must not become a local root-relative
        // operand merely because Rust could not parse its missing server/share.
        if value.as_os_str().as_encoded_bytes().starts_with(b"\\\\")
            && !matches!(value.components().next(), Some(Component::Prefix(prefix)) if matches!(prefix.kind(), Prefix::UNC(..)))
        {
            return Err(invalid());
        }
        inspect(value, false)?;
        let anchored = match value.components().next() {
            Some(Component::Prefix(prefix)) => match prefix.kind() {
                Prefix::Disk(drive) if !value.has_root() => {
                    let base = self
                        .drives
                        .get(&drive.to_ascii_uppercase())
                        .ok_or_else(invalid)?;
                    let units: Vec<_> = value.as_os_str().encode_wide().collect();
                    join_captured(base, Path::new(&OsString::from_wide(&units[2..])))
                }
                Prefix::Disk(_) | Prefix::UNC(..) => value.to_owned(),
                _ => return Err(invalid()),
            },
            _ if value.has_root() => join_captured(&self.rooted, value),
            _ => join_captured(&self.working, value),
        };
        if has_verbatim_prefix(&anchored) {
            inspect(&anchored, true)?;
            Ok(anchored)
        } else {
            extended(&full_path(&anchored)?)
        }
    }
}
fn has_verbatim_prefix(path: &Path) -> bool {
    matches!(path.components().next(), Some(Component::Prefix(prefix)) if prefix.kind().is_verbatim())
}
fn join_captured(base: &Path, tail: &Path) -> PathBuf {
    // Bare drive-relative C: denotes the captured drive directory itself. It is
    // distinct from a literally empty request, which resolve refuses before here.
    if tail.as_os_str().is_empty() {
        return base.to_owned();
    }
    let mut joined = base.join(tail);
    // Rust's verbatim push resolves dot navigation but reconstructs components,
    // dropping a terminal separator. Preserve its file-versus-directory meaning.
    if has_verbatim_prefix(base)
        && tail.as_os_str().encode_wide().last() == Some(92)
        && joined.as_os_str().encode_wide().last() != Some(92)
    {
        joined.as_mut_os_string().push(r"\");
    }
    joined
}

pub(super) fn absolute(path: &Path) -> io::Result<PathBuf> {
    if !path.is_absolute() {
        return Err(invalid());
    }
    if has_verbatim_prefix(path) {
        inspect(path, true)?;
        Ok(path.to_owned())
    } else {
        extended(&full_path(path)?)
    }
}
