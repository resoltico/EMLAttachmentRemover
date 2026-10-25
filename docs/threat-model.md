# Filesystem threat model

## Who uses the tool and what they need

One person copies their own `.eml` files, from folders they own, into folders they
own. They must be able to trust that originals and unrelated files are never harmed
and that a reported copy is complete.

## Guarantees

| # | Guarantee | Mechanism (POSIX / Windows) | Required test (positive + rejected case) |
|---|---|---|---|
| G1 | Originals are never opened for writing, renamed, moved or deleted. | Read-only open; no metadata writes. | Original bytes and mtime unchanged after create, refuse, stop and force stop. |
| G2 | An existing file at a destination is never replaced or modified. A byte-identical existing copy is reported up to date; the tool may `fsync` it but never writes to it. | `O_CREAT|O_EXCL` / `CREATE_NEW` for temps; no-replace rename. | Destination pre-created with different bytes → refused, file untouched. |
| G3 | A published copy is complete: readers never see a partial copy at the final name. | Write temp in the destination directory → `fsync` file → no-replace rename (`renameat_with(NOREPLACE)`; Apple `RENAME_EXCL`; Linux `RENAME_NOREPLACE` with `linkat`+`unlinkat` fallback on `EINVAL`) → `fsync` directory (POSIX). Windows: `MoveFileExW` without `MOVEFILE_REPLACE_EXISTING`, `FlushFileBuffers`. | Injected failure before rename → no final name exists; fallback path tested via injected `EINVAL`. |
| G4 | Links are not followed at the destination edge. | Open the destination directory once and create relative to its handle (`openat` with `O_NOFOLLOW` on the final component; `O_DIRECTORY`). Windows: `FILE_FLAG_OPEN_REPARSE_POINT`, refuse reparse points at the edge. | Symlink / junction planted as the destination name → refused, link target untouched. |
| G5 | A source that changes while being read is detected and refused. | Compare size + mtime + file identity (dev/inode; volume/file ID) before and after the bounded read. | Source rewritten between reads (test hook in the test, not product) → refused, no copy. |
| G6 | Output inside a selected source tree, and source/destination aliases, are refused. | Identity comparison of directories (existing behavior). | Output dir = a selected folder / its child → refused before writes. |
| G7 | Staging residue is recognizable and never auto-deleted. | Temp names `.emlattachmentremover-<random>.tmp` in the destination directory; removed on every handled failure path; may remain after SIGKILL or power loss. | Handled failure → temp removed; documentation lists the residue name. |
| G8 | Bounded resources. | Existing limits (message bytes, parts, collection counts, path bytes). | Over-limit inputs refused with the documented codes. |

Creation of mirrored output directories uses `mkdirat` relative to held directory
handles; an existing non-directory at that name is refused.

## Non-goals (explicitly not defended)

- Hostile local actors with write access to the same directories racing the tool
  (namespace swaps between checks, ACL manipulation, reparse conversion of
  directories the tool created, attribute-only actors).
- Other users' processes on multi-user machines writing into the user's folders.
- Administrator/root interference, filesystem bugs, power loss beyond `fsync` semantics.
- Network filesystems that ignore `fsync` or no-replace semantics (reported as
  failures where the OS reports them; not otherwise detected).
