//! A private kill-on-close Job contains each process before its first instruction.
use std::{
    cmp::Ordering,
    ffi::{OsStr, OsString},
    fs::File,
    io,
    mem::{size_of, size_of_val},
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::Path,
    ptr,
};
use windows_sys::Win32::{
    Foundation::{
        DUPLICATE_SAME_ACCESS, DuplicateHandle, ERROR_INSUFFICIENT_BUFFER, HANDLE, WAIT_OBJECT_0,
        WAIT_TIMEOUT,
    },
    Globalization::{CSTR_EQUAL, CSTR_GREATER_THAN, CSTR_LESS_THAN, CompareStringOrdinal},
    System::{JobObjects::*, Threading::*},
};

/// Explicit native launch facts; no shell, inherited stdin or display-string path.
/// Environment is complete, not a patch; callers decide what they pass to tools.
pub struct ProcessSpec<'a> {
    /// Absolute selected executable address.
    pub executable: &'a Path,
    /// Arguments excluding argv0, preserving native UTF16 including surrogates.
    pub arguments: &'a [OsString],
    /// Absolute captured working directory.
    pub directory: &'a Path,
    /// Complete native environment. Duplicate case-insensitive names are refused.
    pub environment: &'a [(OsString, OsString)],
}
/// One unnamed, non-inherited Windows Job. Dropping its last handle kills members.
/// This is lifetime containment, not a privilege or security sandbox.
pub struct Job(OwnedHandle);
/// A child process whose lifetime cannot outlive its owning Job borrow.
pub struct Process<'a> {
    handle: OwnedHandle,
    _job: &'a Job,
}

fn native(ok: i32) -> io::Result<()> {
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
fn wide(value: &OsStr) -> io::Result<Vec<u16>> {
    let mut units: Vec<_> = value.encode_wide().take(32767).collect();
    if units.contains(&0) || units.len() > 32766 {
        return Err(invalid(
            "Native launch expression contains NUL or exceeds32766 units",
        ));
    }
    units.push(0);
    Ok(units)
}
fn quoted(output: &mut Vec<u16>, value: &OsStr) -> io::Result<()> {
    output.push(b'"' as u16);
    let mut slashes = 0;
    for unit in value.encode_wide() {
        if unit == 0 {
            return Err(invalid("Native process argument contains NUL"));
        }
        if unit == b'\\' as u16 {
            slashes += 1;
            if output.len() + slashes > 32766 {
                return Err(invalid("Native process command line exceeds its bound"));
            }
            continue;
        }
        let charge = if unit == b'"' as u16 {
            slashes * 2 + 1
        } else {
            slashes
        };
        if output.len() + charge + 1 > 32766 {
            return Err(invalid("Native process command line exceeds its bound"));
        }
        output.extend(std::iter::repeat_n(
            b'\\' as u16,
            if unit == b'"' as u16 {
                slashes * 2 + 1
            } else {
                slashes
            },
        ));
        slashes = 0;
        output.push(unit);
        if output.len() > 32766 {
            return Err(invalid("Native process command line exceeds its bound"));
        }
    }
    if output.len() + slashes * 2 + 1 > 32766 {
        return Err(invalid("Native process command line exceeds its bound"));
    }
    output.extend(std::iter::repeat_n(b'\\' as u16, slashes * 2));
    output.push(b'"' as u16);
    if output.len() > 32766 {
        return Err(invalid("Native process command line exceeds its bound"));
    }
    Ok(())
}
fn ordinal(left: &[u16], right: &[u16]) -> io::Result<Ordering> {
    // SAFETY: explicit bounded lengths of live UTF16 arrays; no NUL termination
    // or Unicode repair is assumed. Windows defines environment-name comparison.
    match unsafe {
        CompareStringOrdinal(
            left.as_ptr(),
            left.len() as i32,
            right.as_ptr(),
            right.len() as i32,
            1,
        )
    } {
        CSTR_LESS_THAN => Ok(Ordering::Less),
        CSTR_EQUAL => Ok(Ordering::Equal),
        CSTR_GREATER_THAN => Ok(Ordering::Greater),
        _ => Err(io::Error::last_os_error()),
    }
}

/// Compare native environment names using the system's ordinal ignore-case rule.
pub fn environment_name_equal(left: &OsStr, right: &OsStr) -> io::Result<bool> {
    let mut left = wide(left)?;
    left.pop();
    let mut right = wide(right)?;
    right.pop();
    Ok(ordinal(&left, &right)? == Ordering::Equal)
}
fn environment(values: &[(OsString, OsString)]) -> io::Result<Vec<u16>> {
    if values.len() > 4096 {
        return Err(invalid("Native environment entry limit exceeded"));
    }
    let mut entries: Vec<(Vec<u16>, Vec<u16>)> = Vec::new();
    let mut total = 1usize;
    for (key, value) in values {
        let mut key = wide(key)?;
        key.pop();
        let mut value = wide(value)?;
        value.pop();
        if key.is_empty() || key.contains(&(b'=' as u16)) {
            return Err(invalid("Invalid native environment name"));
        }
        total = total
            .checked_add(key.len() + value.len() + 2)
            .filter(|n| *n <= 1024 * 1024)
            .ok_or_else(|| invalid("Native environment unit limit exceeded"))?;
        let mut position = entries.len();
        for (index, (existing, _)) in entries.iter().enumerate() {
            match ordinal(&key, existing)? {
                Ordering::Equal => {
                    return Err(invalid("Duplicate case-insensitive environment name"));
                }
                Ordering::Less => {
                    position = index;
                    break;
                }
                Ordering::Greater => {}
            }
        }
        entries.insert(position, (key, value));
    }
    let mut result = Vec::with_capacity(total.max(2));
    for (key, value) in entries {
        result.extend(key);
        result.push(b'=' as u16);
        result.extend(value);
        result.push(0);
    }
    result.push(0);
    if result.len() == 1 {
        result.push(0);
    }
    Ok(result)
}
fn inherited(file: &File) -> io::Result<OwnedHandle> {
    let mut raw = ptr::null_mut();
    // SAFETY: borrow a real File; duplicate into this process with inheritance,
    // preserving access. Only the explicit handle list passes it to the child.
    unsafe {
        native(DuplicateHandle(
            GetCurrentProcess(),
            file.as_raw_handle(),
            GetCurrentProcess(),
            &mut raw,
            0,
            1,
            DUPLICATE_SAME_ACCESS,
        ))?;
        Ok(OwnedHandle::from_raw_handle(raw))
    }
}
struct Attributes {
    storage: Vec<usize>,
}
impl Attributes {
    fn new() -> io::Result<Self> {
        let mut size = 0;
        // SAFETY: size query intentionally returns ERROR_INSUFFICIENT_BUFFER.
        let ok = unsafe { InitializeProcThreadAttributeList(ptr::null_mut(), 2, 0, &mut size) };
        if ok != 0
            || io::Error::last_os_error().raw_os_error() != Some(ERROR_INSUFFICIENT_BUFFER as i32)
            || size == 0
            || size > 65536
        {
            return Err(io::Error::other(
                "Native process attribute size is unverified",
            ));
        }
        let mut storage = vec![0usize; size.div_ceil(size_of::<usize>())];
        // SAFETY: aligned owned storage has at least the queried extent.
        unsafe {
            native(InitializeProcThreadAttributeList(
                storage.as_mut_ptr().cast(),
                2,
                0,
                &mut size,
            ))?;
        }
        Ok(Self { storage })
    }
    fn pointer(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.storage.as_mut_ptr().cast()
    }
    fn set(&mut self, key: u32, values: &[HANDLE]) -> io::Result<()> {
        // SAFETY: caller keeps each backing handle array alive until this
        // initialized attribute list is destroyed. No previous-value slot.
        unsafe {
            native(UpdateProcThreadAttribute(
                self.pointer(),
                0,
                key as usize,
                values.as_ptr().cast(),
                size_of_val(values),
                ptr::null_mut(),
                ptr::null(),
            ))
        }
    }
}
impl Drop for Attributes {
    fn drop(&mut self) {
        // SAFETY: exactly one successfully initialized list in its live allocation.
        unsafe {
            DeleteProcThreadAttributeList(self.pointer());
        }
    }
}

impl Job {
    /// Create a private job with kill-on-last-close and no breakaway permission.
    pub fn new() -> io::Result<Self> {
        // SAFETY: unnamed object; no inheritable security attributes.
        let raw = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
        if raw.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: successful creation transferred one unique handle.
        let job = Self(unsafe { OwnedHandle::from_raw_handle(raw) });
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: exact generated structure and extent; no active members yet.
        unsafe {
            native(SetInformationJobObject(
                job.0.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of_val(&limits) as u32,
            ))?;
        }
        Ok(job)
    }
    /// Launch inside this job before user code runs, with a hidden private console
    /// and only three explicitly inherited standard handles. stdout/stderr must
    /// be owned output files; this call never opens/truncates their pathnames.
    pub fn launch<'a>(
        &'a self,
        spec: &ProcessSpec<'_>,
        stdout: &File,
        stderr: &File,
    ) -> io::Result<Process<'a>> {
        if !spec.executable.is_absolute() || !spec.directory.is_absolute() {
            return Err(invalid(
                "Native executable and working directory must be absolute",
            ));
        }
        let executable = wide(spec.executable.as_os_str())?;
        let directory = wide(spec.directory.as_os_str())?;
        let mut line = Vec::new();
        quoted(&mut line, spec.executable.as_os_str())?;
        if spec.arguments.len() > 4096 {
            return Err(invalid("Native argument count exceeds its bound"));
        }
        for argument in spec.arguments {
            line.push(b' ' as u16);
            quoted(&mut line, argument)?;
        }
        line.push(0);
        let environment = environment(spec.environment)?;
        let stdin = File::open(r"\\.\NUL")?;
        let input = inherited(&stdin)?;
        let output = inherited(stdout)?;
        let errors = inherited(stderr)?;
        let handles = [
            input.as_raw_handle(),
            output.as_raw_handle(),
            errors.as_raw_handle(),
        ];
        let jobs = [self.0.as_raw_handle()];
        // Declare the list after its value arrays so it is destroyed first.
        let mut attributes = Attributes::new()?;
        attributes.set(PROC_THREAD_ATTRIBUTE_HANDLE_LIST, &handles)?;
        attributes.set(PROC_THREAD_ATTRIBUTE_JOB_LIST, &jobs)?;
        let mut startup = STARTUPINFOEXW::default();
        startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
        startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES | STARTF_USESHOWWINDOW;
        startup.StartupInfo.wShowWindow = 0;
        startup.StartupInfo.hStdInput = handles[0];
        startup.StartupInfo.hStdOutput = handles[1];
        startup.StartupInfo.hStdError = handles[2];
        startup.lpAttributeList = attributes.pointer();
        let mut information = PROCESS_INFORMATION::default();
        // SAFETY: all native arrays/attributes/handles remain live. The Job-list
        // attribute assigns containment before the suspended primary can run.
        // This avoids the CreateProcess->AssignProcessToJobObject race entirely.
        unsafe {
            native(CreateProcessW(
                executable.as_ptr(),
                line.as_mut_ptr(),
                ptr::null(),
                ptr::null(),
                1,
                CREATE_SUSPENDED
                    | CREATE_NEW_CONSOLE
                    | CREATE_UNICODE_ENVIRONMENT
                    | EXTENDED_STARTUPINFO_PRESENT,
                environment.as_ptr().cast(),
                directory.as_ptr(),
                &startup.StartupInfo,
                &mut information,
            ))?;
        }
        // SAFETY: successful CreateProcessW transferred the two unique handles.
        let process = Process {
            handle: unsafe { OwnedHandle::from_raw_handle(information.hProcess) },
            _job: self,
        };
        let thread = unsafe { OwnedHandle::from_raw_handle(information.hThread) };
        // SAFETY: sole owned primary thread of a contained suspended child.
        if unsafe { ResumeThread(thread.as_raw_handle()) } == u32::MAX {
            let primary = io::Error::last_os_error();
            let cleanup = self.terminate(125);
            return Err(io::Error::other(format!(
                "Native resume failed: {primary}; owned job termination: {cleanup:?}"
            )));
        }
        Ok(process)
    }
    /// Current active member count from this exact job object, not a PID census.
    pub fn active_processes(&self) -> io::Result<u32> {
        let mut information = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
        // SAFETY: exact generated writable structure and held job.
        unsafe {
            native(QueryInformationJobObject(
                self.0.as_raw_handle(),
                JobObjectBasicAccountingInformation,
                (&mut information as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                size_of_val(&information) as u32,
                ptr::null_mut(),
            ))?;
        }
        Ok(information.ActiveProcesses)
    }
    /// Terminate only members of this exact owned job. Success is an attempted
    /// termination; callers must still observe actual exit and zero active members.
    pub fn terminate(&self, exit_code: u32) -> io::Result<()> {
        // SAFETY: held private job is the complete termination authority.
        unsafe { native(TerminateJobObject(self.0.as_raw_handle(), exit_code)) }
    }
}
impl Process<'_> {
    /// Observe actual termination and its unsigned native exit code. A numeric
    /// STILL_ACTIVE exit code is valid after a signaled process handle.
    pub fn exit_code(&self) -> io::Result<Option<u32>> {
        // SAFETY: held exact process; no numeric PID is used for authority.
        match unsafe { WaitForSingleObject(self.handle.as_raw_handle(), 0) } {
            WAIT_TIMEOUT => Ok(None),
            WAIT_OBJECT_0 => {
                let mut code = 0;
                unsafe {
                    native(GetExitCodeProcess(self.handle.as_raw_handle(), &mut code))?;
                }
                Ok(Some(code))
            }
            _ => Err(io::Error::last_os_error()),
        }
    }
}
