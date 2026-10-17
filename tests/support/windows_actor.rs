// Native verification actor only. No account/policy changes or privilege enabling.
use std::{
    io,
    mem::{offset_of, size_of, size_of_val},
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    ptr,
};
use windows_sys::Win32::System::SystemServices::{SE_GROUP_ENABLED, SE_GROUP_USE_FOR_DENY_ONLY};
use windows_sys::Win32::{
    Foundation::{ERROR_NO_TOKEN, HANDLE, LUID},
    Security::*,
    System::Threading::{GetCurrentProcess, GetCurrentThread, OpenProcessToken, OpenThreadToken},
};
pub(crate) struct Information {
    pub(crate) words: Vec<usize>,
    pub(crate) bytes: usize,
}
pub(crate) fn information(
    token: &OwnedHandle,
    class: TOKEN_INFORMATION_CLASS,
) -> io::Result<Information> {
    let mut words = vec![0usize; 8192];
    let mut bytes = 0;
    let capacity = size_of_val(words.as_slice());
    // SAFETY: live token, aligned writable bounded allocation, correct native
    // byte capacity and live length output; the token-information API is synchronous.
    if unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            class,
            words.as_mut_ptr().cast(),
            capacity as u32,
            &mut bytes,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    if bytes as usize > capacity {
        return Err(io::Error::other(
            "Native token information exceeded its buffer",
        ));
    }
    Ok(Information {
        words,
        bytes: bytes as usize,
    })
}
fn same_user(original: &OwnedHandle, restricted: &OwnedHandle) -> io::Result<()> {
    let original = information(original, TokenUser)?;
    let restricted = information(restricted, TokenUser)?;
    if original.bytes < size_of::<TOKEN_USER>() || restricted.bytes < size_of::<TOKEN_USER>() {
        return Err(io::Error::other("Short token-user information"));
    }
    // SAFETY: successful native TokenUser queries supply aligned generated
    // structures and valid contained SID pointers; both buffers remain live.
    let equal = unsafe {
        EqualSid(
            (*(original.words.as_ptr().cast::<TOKEN_USER>())).User.Sid,
            (*(restricted.words.as_ptr().cast::<TOKEN_USER>())).User.Sid,
        )
    };
    if equal == 0 {
        return Err(io::Error::other(
            "Restricted token changed the runner UserSID",
        ));
    }
    Ok(())
}
fn verify_groups(token: &OwnedHandle, admin: PSID) -> io::Result<()> {
    let info = information(token, TokenGroups)?;
    if info.bytes < offset_of!(TOKEN_GROUPS, Groups) {
        return Err(io::Error::other("Short token groups"));
    }
    // SAFETY: aligned native header is present. Array extent is checked before
    // constructing a slice; returned SID pointers remain backed by this buffer.
    let count = unsafe { (*(info.words.as_ptr().cast::<TOKEN_GROUPS>())).GroupCount } as usize;
    let end = count
        .checked_mul(size_of::<SID_AND_ATTRIBUTES>())
        .and_then(|n| n.checked_add(offset_of!(TOKEN_GROUPS, Groups)))
        .ok_or_else(|| io::Error::other("Token group count overflow"))?;
    if end > info.bytes {
        return Err(io::Error::other("Token groups exceed returned bytes"));
    }
    // SAFETY: validated aligned array extent; every element is native POD.
    let groups = unsafe {
        std::slice::from_raw_parts(
            info.words
                .as_ptr()
                .cast::<u8>()
                .add(offset_of!(TOKEN_GROUPS, Groups))
                .cast::<SID_AND_ATTRIBUTES>(),
            count,
        )
    };
    let mut found = false;
    for group in groups {
        // SAFETY: native TokenGroups provided a live valid SID, admin points to
        // the live well-known SID allocation retained by the caller.
        if unsafe { EqualSid(group.Sid, admin) } != 0 {
            if group.Attributes & SE_GROUP_USE_FOR_DENY_ONLY as u32 == 0
                || group.Attributes & SE_GROUP_ENABLED as u32 != 0
            {
                return Err(io::Error::other("Administrators SID is not deny-only"));
            }
            found = true;
        }
    }
    if !found {
        return Err(io::Error::other(
            "Administrators SID missing from restricted-token observation",
        ));
    }
    Ok(())
}
fn verify_privileges(token: &OwnedHandle) -> io::Result<()> {
    let info = information(token, TokenPrivileges)?;
    if info.bytes < offset_of!(TOKEN_PRIVILEGES, Privileges) {
        return Err(io::Error::other("Short native privileges"));
    }
    // SAFETY: aligned header is present; array checked before reading elements.
    let count =
        unsafe { (*(info.words.as_ptr().cast::<TOKEN_PRIVILEGES>())).PrivilegeCount } as usize;
    let end = count
        .checked_mul(size_of::<LUID_AND_ATTRIBUTES>())
        .and_then(|n| n.checked_add(offset_of!(TOKEN_PRIVILEGES, Privileges)))
        .ok_or_else(|| io::Error::other("Privilege count overflow"))?;
    if end > info.bytes {
        return Err(io::Error::other("Privileges exceed returned bytes"));
    }
    let mut traverse = LUID::default();
    // SAFETY: static terminated privilege name, optional system null and live LUID output.
    if unsafe {
        LookupPrivilegeValueW(
            ptr::null(),
            windows_sys::core::w!("SeChangeNotifyPrivilege"),
            &mut traverse,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: validated aligned native POD array extent, backing stays live.
    let privileges = unsafe {
        std::slice::from_raw_parts(
            info.words
                .as_ptr()
                .cast::<u8>()
                .add(offset_of!(TOKEN_PRIVILEGES, Privileges))
                .cast::<LUID_AND_ATTRIBUTES>(),
            count,
        )
    };
    let mut found = false;
    for privilege in privileges {
        if privilege.Attributes & SE_PRIVILEGE_ENABLED != 0 {
            if privilege.Luid.LowPart != traverse.LowPart
                || privilege.Luid.HighPart != traverse.HighPart
            {
                return Err(io::Error::other(
                    "Unexpected enabled privilege in restricted actor",
                ));
            }
            found = true;
        }
    }
    if !found {
        return Err(io::Error::other(
            "Restricted actor lacks enabled traversal privilege",
        ));
    }
    Ok(())
}
/// Restricted runner-account thread token; not a separate standard-user login.
pub(crate) struct RestrictedToken {
    token: OwnedHandle,
    entered: bool,
    _thread_affinity: std::marker::PhantomData<std::rc::Rc<()>>,
}
pub(crate) fn require_unimpersonated_thread() -> io::Result<()> {
    let mut previous: HANDLE = ptr::null_mut();
    // SAFETY: current-thread pseudo-handle, live output; query only.
    if unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 1, &mut previous) } != 0 {
        // SAFETY: successful query transferred exactly one owned token handle.
        drop(unsafe { OwnedHandle::from_raw_handle(previous) });
        return Err(io::Error::other(
            "Actor refuses to replace preexisting thread impersonation",
        ));
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() != Some(ERROR_NO_TOKEN as i32) {
        return Err(error);
    }
    Ok(())
}
impl RestrictedToken {
    /// Enter only a previously unimpersonated thread and verify actual SID/group/
    /// privilege state. Drop restores that known prior state, including on panic.
    pub(crate) fn enter() -> io::Result<Self> {
        require_unimpersonated_thread()?;
        let mut raw = ptr::null_mut();
        // SAFETY: valid process pseudo-handle and writable native handle slot.
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_DUPLICATE | TOKEN_QUERY, &mut raw) }
            == 0
        {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: successful call transfers a uniquely owned handle.
        let original = unsafe { OwnedHandle::from_raw_handle(raw) };
        let mut sid = [0u32; 20];
        let mut bytes = size_of_val(&sid) as u32;
        // SAFETY: writable DWORD-aligned SID buffer exceeds native maximum size.
        if unsafe {
            CreateWellKnownSid(
                WinBuiltinAdministratorsSid,
                ptr::null_mut(),
                sid.as_mut_ptr().cast(),
                &mut bytes,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let disabled = SID_AND_ATTRIBUTES {
            Sid: sid.as_mut_ptr().cast(),
            Attributes: 0,
        };
        let mut raw = ptr::null_mut();
        // SAFETY: live original token/SID and output; absent optional arrays null.
        if unsafe {
            CreateRestrictedToken(
                original.as_raw_handle(),
                DISABLE_MAX_PRIVILEGE,
                1,
                &disabled,
                0,
                ptr::null(),
                0,
                ptr::null(),
                &mut raw,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: successful restricted-token allocation transfers one owner.
        let mut actor = Self {
            token: unsafe { OwnedHandle::from_raw_handle(raw) },
            entered: false,
            _thread_affinity: std::marker::PhantomData,
        };
        // SAFETY: live owned token derived from this process, thread-local effect.
        if unsafe { ImpersonateLoggedOnUser(actor.token.as_raw_handle()) } == 0 {
            return Err(io::Error::last_os_error());
        }
        actor.entered = true;
        let mut effective = ptr::null_mut();
        // SAFETY: queries this now-impersonated thread; output handle slot is live.
        if unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 1, &mut effective) } == 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: successful query transferred one owned token handle. Verify the
        // actual effective token, not merely the originally requested token.
        let effective = unsafe { OwnedHandle::from_raw_handle(effective) };
        same_user(&original, &effective)?;
        verify_groups(&effective, disabled.Sid)?;
        verify_privileges(&effective)?;
        let mut enabled = 1;
        // SAFETY: null token selects effective thread; valid SID and BOOL output.
        if unsafe { CheckTokenMembership(ptr::null_mut(), disabled.Sid, &mut enabled) } == 0 {
            return Err(io::Error::last_os_error());
        }
        if enabled != 0 {
            return Err(io::Error::other(
                "Effective restricted actor still enables Administrators",
            ));
        }
        println!(
            "Observed restricted runner UserSID unchanged; Administrators deny-only; only enabled privilege SeChangeNotify; prior thread had no impersonation"
        );
        Ok(actor)
    }
}
impl Drop for RestrictedToken {
    fn drop(&mut self) {
        if self.entered {
            // SAFETY: restores the explicitly verified no-impersonation prior
            // state before releasing the token; failure must not leak authority.
            if unsafe { RevertToSelf() } == 0 {
                std::process::abort();
            }
        }
    }
}
