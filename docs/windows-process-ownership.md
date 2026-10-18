# Windows verification process ownership

Windows distribution verification needs real process-tree cleanup and a private
console; its success cannot be inferred from the filesystem product tests. The
native boundary belongs in eml-windows rather than unsafe xtask code or filesystem
business policy. It supplies owned Job/process objects to actual verification
callers; it is not a frontend or speculative GUI scaffold.

The chosen process launch uses an unnamed, non-inherited kill-on-close Job and
PROC_THREAD_ATTRIBUTE_JOB_LIST. Containment is assigned before the suspended
process can execute, avoiding a launch/assignment race. Only explicitly listed
standard handles are inherited. Operational paths/arguments remain native UTF16;
the complete environment is bounded and sorted using Windows's case-insensitive
ordinal comparison. A hidden private console isolates later actual console
control tests from the owner's console. Job membership is lifetime containment,
not a privilege or security sandbox.

Challenge: a terminated primary does not establish descendant settlement. Query
the exact Job's active-member count, retain its actual primary process handle,
and require both real exit and zero members. A termination return only establishes
the attempted operation. Unknown cleanup must fail and close further admission.
Kernel last-handle closure must also work when the owning process exits without
Rust Drop. No name-based kill, PID census or broad resource sweep is used.

Microsoft's [Job-list attribute documentation](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-updateprocthreadattribute)
supports Windows10/Server2016 and newer. Its
[process-creation explanation](https://devblogs.microsoft.com/oldnewthing/20230209-00/?p=107812)
states the before-first-instruction ordering. These are design/API sources;
actual native x64/ARM execution, console controls and rejection controls remain
required. The verification controller uses this boundary for real tool commands,
with separate primary-exit/member observations, a250ms ordinary teardown allowance
and five-second cleanup observation. Any original API error remains failed after
cleanup; unknown settlement closes further admission. Native evidence sidecars
are create-new and never overwrite an existing file. The command subset uses
program/arguments/captured cwd/named environment changes, with owned standard
handles and NUL stdin; repository constructors do not use env_clear. Actual
console-event delivery, protected archive roots and final-link/runtime checks
remain open. Source checks are not Windows archive qualification.

The private console test sender is a separate detached process inside the same
owned Job. It inherits only an actual query/synchronization process handle,
derives the console attachment ID from that still-held object, and sends the
selected console event only within the target's private console. It never
detaches an existing caller console and never inherits a Job handle which could
prolong the Job after parent death. Sender acceptance is not proof of CLI handler
completion; tests independently require actual process exit and zero members.

The two real CLI cases use anonymous owned pipes. One observes actual consumption
of an unfinished request before CTRL_C. The other observes a partial real report
and independently golden-checked complete copy, then sends distinct CTRL_C and
CTRL_BREAK deliveries through the same CLI handler while output is blocked.
Both require actual130 status, preserved original/copy facts and complete owned
settlement. They remain unrun until recorded on both native CPUs. The helper and
pipe fixtures are nonshipping; ordinary CLI product features do not enable them.
