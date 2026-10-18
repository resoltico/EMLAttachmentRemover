//! Protected artifact namespace; current SID and privileged OS actors are trusted.
use super::{child, handle, root};
use crate::BoundDirectory;
use std::{
    ffi::c_void,
    fs::File,
    io,
    mem::{offset_of, size_of, size_of_val},
    os::windows::io::AsRawHandle,
    path::{Path, PathBuf},
    ptr,
};
use windows_sys::Win32::{
    Foundation::{ERROR_ALREADY_EXISTS, ERROR_FILE_EXISTS, GENERIC_ALL, GENERIC_WRITE, LocalFree},
    Security::{
        Authorization::{ConvertStringSidToSidW, GetSecurityInfo, SE_FILE_OBJECT},
        *,
    },
    Storage::FileSystem::*,
    System::SystemServices::{ACCESS_ALLOWED_ACE_TYPE, ACCESS_DENIED_ACE_TYPE},
};

/// A canonical private directory and every held ordinary ancestor. Pathname
/// operations remain protected from untrusted-user replacement while this lives.
/// Current SID, administrators/SYSTEM, TrustedInstaller and privileged OS actors
/// are trusted. Drop closes handles without deleting any files or directories.
pub struct PrivateDirectory {
    path: PathBuf,
    directory: BoundDirectory,
    ancestors: Vec<BoundDirectory>,
}
impl PrivateDirectory {
    /// Canonical address beneath the admitted and still-held ancestor chain.
    pub fn path(&self) -> &Path {
        &self.path
    }
    /// Reobserve all held ordinary/full-identity facts before a pathname operation.
    pub fn check(&self) -> io::Result<()> {
        for directory in self
            .ancestors
            .iter()
            .chain(std::iter::once(&self.directory))
        {
            if handle::directory_identity(&directory.file)? != directory.identity {
                return Err(invalid("Private directory identity changed"));
            }
        }
        Ok(())
    }
}
fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
fn denied(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, message)
}
fn checked(ok: i32) -> io::Result<()> {
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
fn within(base: usize, length: usize, pointer: usize, size: usize) -> bool {
    pointer >= base
        && pointer
            .checked_add(size)
            .is_some_and(|end| base.checked_add(length).is_some_and(|limit| end <= limit))
}
struct Local(*mut c_void);
impl Drop for Local {
    fn drop(&mut self) {
        // SAFETY: uniquely owned successful LocalAlloc-family output, never aliased
        // into another owner. Borrowed contained pointers have already expired.
        unsafe {
            LocalFree(self.0);
        }
    }
}
struct Sid(Vec<u32>);
impl Sid {
    fn pointer(&self) -> PSID {
        self.0.as_ptr().cast_mut().cast()
    }
    fn copy(pointer: PSID, base: usize, length: usize) -> io::Result<Self> {
        if !within(base, length, pointer as usize, 8) {
            return Err(invalid("SID header is outside its native allocation"));
        }
        // SAFETY: eight-byte SID header is inside the retained allocation.
        let count = unsafe { *pointer.cast::<u8>().add(1) } as usize;
        let bytes = 8 + 4 * count;
        if count > 15 || !within(base, length, pointer as usize, bytes) {
            return Err(invalid("SID exceeds its native allocation"));
        }
        // SAFETY: full variable extent was checked before native validation/copy.
        if unsafe { IsValidSid(pointer) } == 0 {
            return Err(invalid("Invalid native SID"));
        }
        let mut words = vec![0u32; bytes.div_ceil(4)];
        unsafe {
            ptr::copy_nonoverlapping(pointer.cast::<u8>(), words.as_mut_ptr().cast(), bytes);
        }
        Ok(Self(words))
    }
    fn named(value: &str) -> io::Result<Self> {
        let name = value.encode_utf16().chain([0]).collect::<Vec<_>>();
        let mut pointer = ptr::null_mut();
        // SAFETY: terminated immutable SID string and writable unique output.
        unsafe {
            checked(ConvertStringSidToSidW(name.as_ptr(), &mut pointer))?;
        }
        let allocation = Local(pointer);
        // SAFETY: ConvertStringSidToSidW returns a valid owned SID allocation.
        let length = unsafe { GetLengthSid(pointer) } as usize;
        let sid = Self::copy(pointer, pointer as usize, length)?;
        drop(allocation);
        Ok(sid)
    }
    fn user() -> io::Result<Self> {
        // GetCurrentThreadEffectiveToken is an SDK inline returning HANDLE(-6).
        // windows-sys exposes no binding for it. This query-only pseudo handle
        // refers to the actual effective token and is never owned or closed.
        let token = std::ptr::without_provenance_mut::<c_void>((-6isize) as usize);
        let mut words = vec![0usize; 8192];
        let mut used = 0;
        // SAFETY: aligned bounded buffer and live token; synchronous native query.
        unsafe {
            checked(GetTokenInformation(
                token,
                TokenUser,
                words.as_mut_ptr().cast(),
                size_of_val(words.as_slice()) as u32,
                &mut used,
            ))?;
        }
        if used as usize > size_of_val(words.as_slice())
            || (used as usize) < size_of::<TOKEN_USER>()
        {
            return Err(invalid("Unverified token-user allocation extent"));
        }
        // SAFETY: complete generated TOKEN_USER was checked; its SID is then
        // range-checked against this same still-live native output buffer.
        let pointer = unsafe { (*words.as_ptr().cast::<TOKEN_USER>()).User.Sid };
        Self::copy(pointer, words.as_ptr() as usize, used as usize)
    }
    fn equals(&self, other: &Self) -> bool {
        // SAFETY: both values own validated aligned complete SID bytes.
        unsafe { EqualSid(self.pointer(), other.pointer()) != 0 }
    }
}
fn inspect(file: &File, trusted: &[Sid], private: bool) -> io::Result<()> {
    let mut owner = ptr::null_mut();
    let mut acl = ptr::null_mut();
    let mut descriptor = ptr::null_mut();
    // SAFETY: held READ_CONTROL directory, typed requested outputs. Successful
    // GetSecurityInfo supplies one LocalFree-owned descriptor containing them.
    let code = unsafe {
        GetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            ptr::null_mut(),
            &mut acl,
            ptr::null_mut(),
            &mut descriptor,
        )
    };
    if code != 0 {
        return Err(io::Error::from_raw_os_error(code as i32));
    }
    let allocation = Local(descriptor);
    // SAFETY: successful query owns a valid descriptor until allocation drops.
    let length = unsafe { GetSecurityDescriptorLength(descriptor) } as usize;
    if !(size_of::<SECURITY_DESCRIPTOR_RELATIVE>()..=128 * 1024).contains(&length) {
        return Err(invalid("Security descriptor extent is unverified"));
    }
    let owner = Sid::copy(owner, descriptor as usize, length)?;
    if !trusted.iter().any(|sid| sid.equals(&owner)) || (private && !trusted[0].equals(&owner)) {
        return Err(denied("Directory owner is outside the admitted principals"));
    }
    let mut control = 0;
    let mut revision = 0;
    // SAFETY: valid owned descriptor and typed scalar outputs.
    unsafe {
        checked(GetSecurityDescriptorControl(
            descriptor,
            &mut control,
            &mut revision,
        ))?;
    }
    if private && control & SE_DACL_PROTECTED == 0 {
        return Err(denied(
            "New private directory inherited an unprotected DACL",
        ));
    }
    if acl.is_null() || !within(descriptor as usize, length, acl as usize, size_of::<ACL>()) {
        return Err(denied("Missing or unverified directory DACL"));
    }
    // SAFETY: complete ACL header lies in the descriptor allocation.
    let header = unsafe { ptr::read_unaligned(acl) };
    if (header.AclSize as usize) < size_of::<ACL>()
        || !within(
            descriptor as usize,
            length,
            acl as usize,
            header.AclSize as usize,
        )
    {
        return Err(invalid("Directory ACL exceeds its allocation"));
    }
    let forbidden = DELETE
        | WRITE_DAC
        | WRITE_OWNER
        | FILE_DELETE_CHILD
        | FILE_WRITE_ATTRIBUTES
        | FILE_WRITE_EA
        | GENERIC_ALL
        | GENERIC_WRITE;
    for index in 0..header.AceCount {
        let mut ace = ptr::null_mut();
        // SAFETY: bounded live native ACL and writable ACE pointer output.
        unsafe {
            checked(GetAce(acl, index as u32, &mut ace))?;
        }
        if !within(
            acl as usize,
            header.AclSize as usize,
            ace as usize,
            size_of::<ACE_HEADER>(),
        ) {
            return Err(invalid("ACE header exceeds its ACL"));
        }
        // SAFETY: exact header extent checked; do not assume additional layout.
        let entry = unsafe { ptr::read_unaligned(ace.cast::<ACE_HEADER>()) };
        if entry.AceFlags as u32 & INHERIT_ONLY_ACE != 0 {
            continue;
        }
        if !matches!(
            entry.AceType as u32,
            ACCESS_ALLOWED_ACE_TYPE | ACCESS_DENIED_ACE_TYPE
        ) {
            return Err(denied("Unsupported current-object ACL entry"));
        }
        if (entry.AceSize as usize) < size_of::<ACCESS_ALLOWED_ACE>()
            || !within(
                acl as usize,
                header.AclSize as usize,
                ace as usize,
                entry.AceSize as usize,
            )
        {
            return Err(invalid("ACE exceeds its ACL"));
        }
        if entry.AceType as u32 == ACCESS_DENIED_ACE_TYPE {
            continue;
        }
        // SAFETY: the two admitted standard ACE types share the SDK Header/Mask/
        // SidStart layout. Full entry extent is checked before reading fields.
        let mask = unsafe {
            ptr::read_unaligned(
                ace.cast::<u8>()
                    .add(offset_of!(ACCESS_ALLOWED_ACE, Mask))
                    .cast::<u32>(),
            )
        };
        let pointer = unsafe {
            ace.cast::<u8>()
                .add(offset_of!(ACCESS_ALLOWED_ACE, SidStart))
                .cast()
        };
        let sid = Sid::copy(pointer, ace as usize, entry.AceSize as usize)?;
        let allowed = if private { &trusted[..3] } else { trusted };
        if (if private {
            mask != 0
        } else {
            mask & forbidden != 0
        }) && !allowed.iter().any(|known| known.equals(&sid))
        {
            return Err(denied(
                "Untrusted SID can mutate the admitted directory namespace",
            ));
        }
    }
    drop(allocation);
    Ok(())
}

/// Create one exclusive protected child after admitting and pinning every
/// canonical ancestor. Preexisting ACLs/owners are never changed. Only actual
/// name collisions retry; refusal leaves useful failure resources untouched.
pub fn create_private_directory(parent: &Path) -> io::Result<PrivateDirectory> {
    let parent = parent.canonicalize()?;
    let (root, names) = root::root_and_names(&parent)?;
    let trusted = vec![
        Sid::user()?,
        Sid::named("S-1-5-18")?,
        Sid::named("S-1-5-32-544")?,
        Sid::named("S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464")?,
    ];
    let file = handle::open(
        &root,
        child::ANCESTOR_PIN_ACCESS | READ_CONTROL,
        FILE_SHARE_READ | FILE_SHARE_WRITE,
        FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
    )?;
    let identity = handle::directory_identity(&file)?;
    inspect(&file, &trusted, false).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("Cannot admit private ancestor {}: {error}", root.display()),
        )
    })?;
    let mut ancestors = vec![BoundDirectory { file, identity }];
    let mut route = root;
    for name in names {
        route.push(name);
        if ancestors.len() >= 128 {
            return Err(invalid("Private directory ancestry exceeds128"));
        }
        let file = child::security_pin(&ancestors.last().expect("root pin").file, name)?;
        let identity = handle::directory_identity(&file)?;
        inspect(&file, &trusted, false).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("Cannot admit private ancestor {}: {error}", route.display()),
            )
        })?;
        ancestors.push(BoundDirectory { file, identity });
    }
    // New children grant only this actual user, SYSTEM and administrators;
    // TrustedInstaller is admitted on OS ancestors, not added to private output.
    let bytes = size_of::<ACL>()
        + trusted[..3]
            .iter()
            .map(|sid| offset_of!(ACCESS_ALLOWED_ACE, SidStart) + sid.0.len() * 4)
            .sum::<usize>();
    let mut words = vec![0u32; bytes.div_ceil(4)];
    let acl = words.as_mut_ptr().cast::<ACL>();
    // SAFETY: aligned owned allocation covers the explicit ACL and SID extents.
    unsafe {
        checked(InitializeAcl(acl, (words.len() * 4) as u32, ACL_REVISION))?;
    }
    for sid in &trusted[..3] {
        unsafe {
            checked(AddAccessAllowedAceEx(
                acl,
                ACL_REVISION,
                OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE,
                FILE_ALL_ACCESS,
                sid.pointer(),
            ))?;
        }
    }
    let mut descriptor = SECURITY_DESCRIPTOR::default();
    let descriptor = (&mut descriptor as *mut SECURITY_DESCRIPTOR).cast();
    // SAFETY: typed absolute descriptor and all pointed-to owned buffers live
    // across synchronous CreateDirectoryW. Inheritance is explicitly protected.
    unsafe {
        checked(InitializeSecurityDescriptor(descriptor, 1))?;
        checked(SetSecurityDescriptorOwner(
            descriptor,
            trusted[0].pointer(),
            0,
        ))?;
        checked(SetSecurityDescriptorDacl(descriptor, 1, acl, 0))?;
        checked(SetSecurityDescriptorControl(
            descriptor,
            SE_DACL_PROTECTED,
            SE_DACL_PROTECTED,
        ))?;
    }
    let security = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor,
        bInheritHandle: 0,
    };
    for _ in 0..32 {
        let mut random = [0u8; 16];
        getrandom::fill(&mut random).map_err(io::Error::other)?;
        let name = format!(
            "artifact-{}",
            random
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        );
        let path = parent.join(&name);
        let units = super::path::openable(&path)?;
        // SAFETY: validated terminated absolute name under the still-held,
        // admitted parent. No untrusted SID can delete/replace a created child.
        if unsafe { CreateDirectoryW(units.as_ptr(), &security) } == 0 {
            let error = io::Error::last_os_error();
            if matches!(error.raw_os_error(),Some(code) if code==ERROR_ALREADY_EXISTS as i32||code==ERROR_FILE_EXISTS as i32)
            {
                continue;
            }
            return Err(error);
        }
        let file = child::security_pin(
            &ancestors.last().expect("parent pin").file,
            std::ffi::OsStr::new(&name),
        )?;
        let identity = handle::directory_identity(&file)?;
        inspect(&file, &trusted, true)?;
        let result = PrivateDirectory {
            path,
            directory: BoundDirectory { file, identity },
            ancestors,
        };
        result.check()?;
        return Ok(result);
    }
    Err(io::Error::other(
        "Exclusive private directory allocation exhausted",
    ))
}

#[cfg(all(test, feature = "native-fixtures"))]
#[path = "private_directory_tests.rs"]
mod tests;

/// Move one verified artifact without replacement or copy fallback. Callers must
/// retain protected source/destination ancestry through this path edge and
/// reobserve the published bytes. This is not a business-copy receipt.
pub fn rename_artifact_noreplace(source: &Path, destination: &Path) -> io::Result<()> {
    let source = super::path::openable(source)?;
    let destination = super::path::openable(destination)?;
    // SAFETY: terminated validated ordinary absolute paths. Zero flags prohibit
    // replacement and cross-volume copy fallback; no delete-on-failure fallback.
    checked(unsafe { MoveFileExW(source.as_ptr(), destination.as_ptr(), 0) })
}
