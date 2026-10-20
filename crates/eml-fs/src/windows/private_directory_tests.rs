//! Real distinct-user ACL access; only an owned account on fresh hosted VMs.
use super::*;
use std::os::windows::io::{FromRawHandle, OwnedHandle};
use std::{
    fs,
    panic::{AssertUnwindSafe, catch_unwind},
};
use windows_sys::Win32::NetworkManagement::NetManagement::*;
use windows_sys::Win32::{
    Foundation::ERROR_NO_TOKEN,
    System::Threading::{GetCurrentThread, OpenThreadToken},
};

struct NetBuffer(*mut u8);
impl Drop for NetBuffer {
    fn drop(&mut self) {
        // SAFETY: uniquely owned successful NetAPI buffer, not LocalAlloc memory.
        unsafe {
            NetApiBufferFree(self.0.cast());
        }
    }
}
fn account_sid(name: &[u16]) -> io::Result<Sid> {
    let mut pointer = ptr::null_mut();
    // SAFETY: terminated local account name and unique typed buffer output.
    let code = unsafe { NetUserGetInfo(ptr::null(), name.as_ptr(), 23, &mut pointer) };
    if code != 0 {
        return Err(io::Error::from_raw_os_error(code as i32));
    }
    let allocation = NetBuffer(pointer);
    let mut size = 0;
    // SAFETY: live NetAPI allocation and writable byte-count output.
    let code = unsafe { NetApiBufferSize(pointer.cast(), &mut size) };
    if code != 0 {
        return Err(io::Error::from_raw_os_error(code as i32));
    }
    if (size as usize) < size_of::<USER_INFO_23>() || size > 128 * 1024 {
        return Err(invalid(
            "Temporary account information extent is unverified",
        ));
    }
    // SAFETY: complete generated structure in the verified live allocation.
    let info = unsafe { ptr::read_unaligned(pointer.cast::<USER_INFO_23>()) };
    let sid = Sid::copy(info.usri23_user_sid, pointer as usize, size as usize)?;
    drop(allocation);
    Ok(sid)
}
struct TemporaryAccount {
    name: Vec<u16>,
    sid: Option<Sid>,
    token: Option<OwnedHandle>,
    cleanup_attempted: bool,
}
impl TemporaryAccount {
    fn new() -> io::Result<Self> {
        if std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
            || std::env::var("RUNNER_ENVIRONMENT").as_deref() != Ok("github-hosted")
        {
            return Err(denied(
                "Distinct-user ACL test requires a fresh hosted VM; no local owner account change is admitted",
            ));
        }
        let mut random = [0u8; 24];
        getrandom::fill(&mut random).map_err(io::Error::other)?;
        let text = format!(
            "emlq{}",
            random[..8]
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        );
        let mut name = text.encode_utf16().chain([0]).collect::<Vec<_>>();
        let mut password = format!(
            "EmL1!{}",
            random[8..]
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        )
        .encode_utf16()
        .chain([0])
        .collect::<Vec<_>>();
        let information = USER_INFO_1 {
            usri1_name: name.as_mut_ptr(),
            usri1_password: password.as_mut_ptr(),
            usri1_priv: USER_PRIV_USER,
            usri1_flags: UF_SCRIPT | UF_NORMAL_ACCOUNT,
            ..Default::default()
        };
        let mut field = 0;
        // SAFETY: local server only, generated structure and live native strings.
        // Credentials never enter a command line, environment, file or log.
        let code = unsafe {
            NetUserAdd(
                ptr::null(),
                1,
                (&information as *const USER_INFO_1).cast(),
                &mut field,
            )
        };
        if code != 0 {
            return Err(io::Error::other(format!(
                "Owned test-account creation failed: NetAPI {code}, field {field}"
            )));
        }
        let mut account = Self {
            name,
            sid: None,
            token: None,
            cleanup_attempted: false,
        };
        account.sid = Some(account_sid(&account.name)?);
        account.join_users()?;
        let domain = [b'.' as u16, 0];
        let mut raw = ptr::null_mut();
        // SAFETY: local explicit synthetic credentials and a unique token output;
        // LogonUser does not load a profile or change account/policy settings.
        let logged = unsafe {
            LogonUserW(
                account.name.as_ptr(),
                domain.as_ptr(),
                password.as_ptr(),
                LOGON32_LOGON_INTERACTIVE,
                LOGON32_PROVIDER_DEFAULT,
                &mut raw,
            )
        };
        let logged = checked(logged);
        password.fill(0);
        logged?;
        // SAFETY: successful LogonUser transferred one uniquely owned token.
        account.token = Some(unsafe { OwnedHandle::from_raw_handle(raw) });
        Ok(account)
    }
    fn join_users(&self) -> io::Result<()> {
        let users = Sid::named("S-1-5-32-545")?;
        let mut group = vec![0u16; 256];
        let mut group_length = group.len() as u32;
        let mut domain_name = vec![0u16; 256];
        let mut domain_length = domain_name.len() as u32;
        let mut use_kind = 0;
        // Resolve the built-in Users alias by SID instead of a localized label.
        unsafe {
            checked(LookupAccountSidW(
                ptr::null(),
                users.pointer(),
                group.as_mut_ptr(),
                &mut group_length,
                domain_name.as_mut_ptr(),
                &mut domain_length,
                &mut use_kind,
            ))?;
        }
        let member = LOCALGROUP_MEMBERS_INFO_0 {
            lgrmi0_sid: self.sid.as_ref().expect("captured user SID").pointer(),
        };
        // Only this newly owned account's membership is added. Deleting its
        // still-matching SID removes that membership without changing any policy.
        let code = unsafe {
            NetLocalGroupAddMembers(
                ptr::null(),
                group.as_ptr(),
                0,
                (&member as *const LOCALGROUP_MEMBERS_INFO_0).cast(),
                1,
            )
        };
        if code != 0 && code != windows_sys::Win32::Foundation::ERROR_MEMBER_IN_ALIAS {
            return Err(io::Error::other(format!(
                "Owned account Users membership failed: NetAPI {code}"
            )));
        }
        Ok(())
    }
    fn as_user<T>(&self, operation: impl FnOnce() -> io::Result<T>) -> io::Result<T> {
        let mut prior = ptr::null_mut();
        // SAFETY: inspect only this thread; never overwrite existing impersonation.
        if unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 1, &mut prior) } != 0 {
            drop(unsafe { OwnedHandle::from_raw_handle(prior) });
            return Err(invalid("ACL actor thread was already impersonating"));
        }
        let prior_error = io::Error::last_os_error();
        if prior_error.raw_os_error() != Some(ERROR_NO_TOKEN as i32) {
            return Err(prior_error);
        }
        let original = Sid::user()?;
        let token = self
            .token
            .as_ref()
            .ok_or_else(|| invalid("Missing synthetic logon token"))?;
        // SAFETY: explicit-credential token is retained throughout this synchronous
        // thread-only scope; check success before any filesystem operation.
        unsafe {
            checked(ImpersonateLoggedOnUser(token.as_raw_handle())).map_err(|error| {
                io::Error::new(
                    error.kind(),
                    format!("Explicit-credential impersonation: {error}"),
                )
            })?;
        }
        let result = catch_unwind(AssertUnwindSafe(|| {
            let actual = Sid::user().map_err(|error| {
                io::Error::new(
                    error.kind(),
                    format!("Distinct-user identity read: {error}"),
                )
            })?;
            if actual.equals(&original)
                || !actual.equals(self.sid.as_ref().expect("captured account SID"))
            {
                return Err(invalid("ACL actor is not the distinct authenticated user"));
            }
            let administrators = Sid::named("S-1-5-32-544")?;
            let mut words = vec![0usize; 8192];
            let mut used = 0;
            let effective = std::ptr::without_provenance_mut::<c_void>((-6isize) as usize);
            // SDK effective-token pseudo handle supplies query rights directly;
            // do not reopen the token inside CheckTokenMembership(NULL,...).
            unsafe {
                checked(GetTokenInformation(
                    effective,
                    TokenGroups,
                    words.as_mut_ptr().cast(),
                    size_of_val(words.as_slice()) as u32,
                    &mut used,
                ))?;
            }
            let start = offset_of!(TOKEN_GROUPS, Groups);
            if used as usize > size_of_val(words.as_slice()) || (used as usize) < start {
                return Err(invalid("Unverified effective token-group extent"));
            }
            let count = unsafe { (*words.as_ptr().cast::<TOKEN_GROUPS>()).GroupCount } as usize;
            let extent = start
                .checked_add(
                    count
                        .checked_mul(size_of::<SID_AND_ATTRIBUTES>())
                        .ok_or_else(|| invalid("Token group count overflow"))?,
                )
                .ok_or_else(|| invalid("Token group extent overflow"))?;
            if extent > used as usize {
                return Err(invalid("Token groups exceed their allocation"));
            }
            for index in 0..count {
                let group = unsafe {
                    ptr::read_unaligned(
                        words
                            .as_ptr()
                            .cast::<u8>()
                            .add(start + index * size_of::<SID_AND_ATTRIBUTES>())
                            .cast::<SID_AND_ATTRIBUTES>(),
                    )
                };
                if Sid::copy(group.Sid, words.as_ptr() as usize, used as usize)?
                    .equals(&administrators)
                {
                    return Err(invalid("ACL actor carries an administrator group SID"));
                }
            }
            operation()
        }));
        // SAFETY: restore the previously verified non-impersonating thread. A
        // failed restore aborts all later work; never continue under another SID.
        if unsafe { RevertToSelf() } == 0 || !Sid::user().is_ok_and(|sid| sid.equals(&original)) {
            eprintln!(
                "Distinct-user ACL actor restoration unverified; aborting owned test process"
            );
            std::process::abort();
        }
        match result {
            Ok(result) => result,
            Err(panic) => std::panic::resume_unwind(panic),
        }
    }
    fn close(&mut self) -> io::Result<()> {
        if self.cleanup_attempted {
            return Err(invalid("Temporary account cleanup was already attempted"));
        }
        self.cleanup_attempted = true;
        let expected = self.sid.as_ref().ok_or_else(|| {
            invalid("Created account SID unknown; do not guess deletion ownership")
        })?;
        if !account_sid(&self.name)?.equals(expected) {
            return Err(invalid(
                "Temporary account name now denotes a different SID; no deletion",
            ));
        }
        drop(self.token.take());
        // SAFETY: only this operation's still-matching synthetic local account.
        let code = unsafe { NetUserDel(ptr::null(), self.name.as_ptr()) };
        if code != 0 {
            return Err(io::Error::from_raw_os_error(code as i32));
        }
        Ok(())
    }
}
impl Drop for TemporaryAccount {
    fn drop(&mut self) {
        if !self.cleanup_attempted
            && let Err(error) = self.close()
        {
            eprintln!("Owned synthetic account cleanup unverified: {error}");
        }
    }
}

fn everyone_acl(path: &Path) -> io::Result<()> {
    let sid = Sid::named("S-1-1-0")?;
    let bytes = size_of::<ACL>() + offset_of!(ACCESS_ALLOWED_ACE, SidStart) + sid.0.len() * 4;
    let mut words = vec![0u32; bytes.div_ceil(4)];
    let acl = words.as_mut_ptr().cast::<ACL>();
    unsafe {
        checked(InitializeAcl(acl, (words.len() * 4) as u32, ACL_REVISION))?;
        checked(AddAccessAllowedAceEx(
            acl,
            ACL_REVISION,
            OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE,
            FILE_ALL_ACCESS,
            sid.pointer(),
        ))?;
    }
    let file = handle::open(
        path,
        READ_CONTROL | WRITE_DAC | FILE_READ_ATTRIBUTES,
        FILE_SHARE_READ | FILE_SHARE_WRITE,
        FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
    )?;
    // SAFETY: only a newly owned synthetic control directory is modified; no
    // production path calls this test helper or alters an existing parent ACL.
    let code = unsafe {
        windows_sys::Win32::Security::Authorization::SetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            acl,
            ptr::null_mut(),
        )
    };
    if code != 0 {
        return Err(io::Error::from_raw_os_error(code as i32));
    }
    Ok(())
}

#[test]
fn native_private_artifact_acl_denies_a_real_distinct_user_and_rejects_writable_ancestry() {
    let parent = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/private-directory-tests");
    fs::create_dir_all(&parent).unwrap();
    let scratch = tempfile::tempdir_in(parent).unwrap();
    let protected = create_private_directory(scratch.path()).unwrap();
    let inside = protected.path().join("owned.txt");
    fs::write(&inside, b"owned unchanged bytes").unwrap();
    let permissive = protected.path().join("synthetic-writable-parent");
    fs::create_dir(&permissive).unwrap();
    everyone_acl(&permissive).unwrap();
    let error = create_private_directory(&permissive)
        .err()
        .expect("Writable parent was admitted");
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    assert_eq!(fs::read_dir(&permissive).unwrap().count(), 0);
    let mut account = TemporaryAccount::new().unwrap();
    let observed = account.as_user(|| {
        let positive = permissive.join("positive");
        fs::write(&positive, b"actual distinct-user control").map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("Distinct-user writable positive control: {error}"),
            )
        })?;
        let read = fs::read(&inside)
            .err()
            .ok_or_else(|| invalid("Different user read private bytes"))?;
        let write = fs::write(protected.path().join("foreign-write"), b"must be denied")
            .err()
            .ok_or_else(|| invalid("Different user wrote into private enclosure"))?;
        if read.kind() != io::ErrorKind::PermissionDenied
            || write.kind() != io::ErrorKind::PermissionDenied
        {
            return Err(invalid(
                "Distinct-user failures were not actual access denials",
            ));
        }
        Ok((read.raw_os_error(), write.raw_os_error()))
    });
    let cleanup = account.close();
    assert!(
        cleanup.is_ok(),
        "Synthetic account cleanup failed: {cleanup:?}"
    );
    let observed = observed.unwrap();
    assert_eq!(observed, (Some(5), Some(5)));
    assert_eq!(fs::read(&inside).unwrap(), b"owned unchanged bytes");
    assert!(!protected.path().join("foreign-write").exists());
    assert_eq!(
        fs::read(permissive.join("positive")).unwrap(),
        b"actual distinct-user control"
    );
    protected.check().unwrap();
    drop(protected);
    scratch.close().unwrap();
}

#[test]
fn native_artifact_move_preserves_existing_winner_and_same_file_identity() {
    let parent = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/artifact-edge-tests");
    fs::create_dir_all(&parent).unwrap();
    let scratch = tempfile::tempdir_in(parent).unwrap();
    let private = create_private_directory(scratch.path()).unwrap();
    let source = private.path().join("source.zip");
    let final_path = private.path().join("final.zip");
    fs::write(&source, b"complete owned candidate").unwrap();
    fs::write(&final_path, b"owned existing winner").unwrap();
    let held = handle::open(
        &source,
        FILE_READ_ATTRIBUTES,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        FILE_FLAG_OPEN_REPARSE_POINT,
    )
    .unwrap();
    let before = handle::entry_observation(&held).unwrap();
    let error = rename_artifact_noreplace(&source, &final_path).unwrap_err();
    assert!(crate::name_collision(&error));
    assert_eq!(fs::read(&source).unwrap(), b"complete owned candidate");
    assert_eq!(fs::read(&final_path).unwrap(), b"owned existing winner");
    fs::remove_file(&final_path).unwrap();
    rename_artifact_noreplace(&source, &final_path).unwrap();
    assert!(!source.exists());
    assert_eq!(fs::read(&final_path).unwrap(), b"complete owned candidate");
    let after = handle::entry_observation(&held).unwrap();
    assert_eq!(after.identity, before.identity);
    assert_eq!(after.kind, before.kind);
    assert_eq!(after.file.unwrap().size, before.file.unwrap().size);
    private.check().unwrap();
    drop(held);
    drop(private);
    scratch.close().unwrap();
}
