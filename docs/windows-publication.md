# Windows same-directory publication correction

This explicitly supersedes only the Windows rename mechanism in the supplied
v6 design pack's filesystem contract §6 step 5 and the matching Win32-only
restriction. The original design archive and checksum are unchanged.

## Design and challenge

Both native Rust 1.99 GNU LLVM Windows hosts rejected the specified
SetFileInformationByHandle/FileRenameInfo relative-root call with error 87.
An absolute-name variant failed with error 32 while the destination's write
sharing remained excluded. Allowing write sharing enabled rename but also a
concurrent directory write handle, removing the required in-place reparse
protection. That diagnostic did not demonstrate a successful junction redirect.
The unsafe sharing change is excluded from the product.

The independent “Audit Rust rebuild proposal” chat reviewed the generated
structure layout, aligned buffers, access rights, lengths and actual native
runs. It recommended the narrower native same-directory operation. At
[e79277b](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38759803940),
both actual NTFS hosts passed its protected primitive, conflict, identity,
readback, file/namespace flush and owned cleanup checks. This proves that
focused boundary; it does not qualify the full product or all adversarial cases.

## Selected boundary

- Keep Win32 creation under the resolved, held ordinary ancestor chain. No
  write or delete sharing on pinned directories; no candidate sharing.
- Create the candidate directly in its bound destination, hold its exclusive
  synchronous DELETE/SYNCHRONIZE-capable handle, write/reread/synchronize it.
- Publish through generated windows-sys NtSetInformationFile and native
  FILE_RENAME_INFORMATION with FileRenameInformation (10), replacement false,
  RootDirectory null, and only a single final basename. Native same-directory
  semantics use the held candidate's directory; there is no pathname target
  reopen or temporary sharing relaxation.
- Reject empty, dot, dot-dot, NUL, separator and colon names and enforce the
  observed filesystem component-unit limit. Check size arithmetic, use native
  alignment and a conservative sizeof(struct)+name bytes+2 allocation.
- Retain buffers and handles through any pending completion. Immediate negative
  NTSTATUS is authoritative even when IO_STATUS_BLOCK was not populated. Keep
  raw status evidence; map through RtlNtStatusToDosError, never stale LastError.
- Preserve final identity/address, namespace synchronization, close/reopen,
  no-overwrite and identity-safe cleanup requirements and all typed outcomes.
- NtCreateFile, handwritten native layouts, absolute-name rename, MoveFileEx,
  replacement and relaxed sharing remain excluded. Only this rename operation
  is added to the narrow reviewed native boundary.

The restricted-token probe uses the runner account's thread token with the
Administrators SID deny-only and DISABLE_MAX_PRIVILEGE. Windows retains
SeChangeNotifyPrivilege. This is actual restricted access, not a separately
qualified standard-user interactive login. Each adversarial actor thread must
establish its own intended token; impersonation is thread-local.

Full WP3 still requires actual junction/reparse mutation and pre-existing writer
controls, source/destination/candidate substitutions, entry conflicts, post-edge
failures and cleanup cases on both native CPUs. The early gate is not a waiver.

Native API references: [FILE_RENAME_INFORMATION](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/ns-ntifs-_file_rename_information),
[NtSetInformationFile](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/nf-ntifs-ntsetinformationfile),
[CreateRestrictedToken](https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-createrestrictedtoken).
