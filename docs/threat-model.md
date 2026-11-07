# Filesystem threat model

The supported user processes their own EML files in folders they own, with a
stable ordinary filesystem namespace. The tool protects originals, existing
email-copy outputs and complete-copy publication within that environment.

## Guarantees and mechanisms

- Originals are opened read-only and never renamed, moved, deleted or written,
  including metadata writes. Tests compare bytes and mtime after creation,
  refusals, cooperative stop and forced termination.
- Existing email-copy destinations are never replaced or modified. Identical bytes can be
  reported up to date after verification and synchronization. Different bytes,
  aliases and conflicting reservations are refused. Exclusive staging creation
  and no-replace publication protect a destination created by another normal run.
- A final copy name exposes complete prepared bytes. Staging bytes are independently
  verified and file-synchronized before publication. Namespace synchronization
  follows; its result is reported separately. Unix uses no-replace rename, with a
  tested Linux EINVAL link/unlink fallback. Windows uses MoveFileExW without
  replacement or copy fallback and FlushFileBuffers. A synchronization failure
  after publication still reports the published copy.
- Destination edges refuse links/reparse points. Unix uses held directory
  descriptors and no-follow relative operations. Windows uses reparse-aware opens,
  held file IDs and requested-directory address checks before path-based child
  operations. Mirrored directories refuse existing non-directory edges.
- Observed source identity, size or mtime changes across a read cause refusal.
  Metadata comparison detects ordinary concurrent changes; it cannot identify
  every possible rewrite that preserves all observed metadata.
- Overlapping selected trees/output roots and source/destination aliases are
  refused before writes. Collection, native paths, source bytes, MIME structure
  and report storage have explicit resource bounds.
- Staging names are recognizable: `.emlattachmentremover-<random>.tmp`. Handled
  failures attempt owned-entry cleanup and report its result. Force kill, crash,
  uncertain addresses or power loss can leave residue. Earlier residue is never
  swept automatically; inspect it after processing has settled before removal.

Filesystem regression tests cover existing-winner preservation, links, aliases,
changed sources, no-replace fallback and cleanup failures. Actual CLI subprocess
tests check completed-copy and original preservation across forced termination.

Current batch reports stay in bounded memory; they are not automatically written to disk. An outstanding explicit save retains its captured stream until settlement.

Report exports use a user-approved save path, including replacement approved by the
native save panel. Complete bytes are staged privately in the same folder and
file-synchronized before atomic rename. A failed write preserves any prior target;
a crash can leave an owned staging file. Directory-entry survival across power
loss is not promised. This report-save policy does not change the email processor’s collision
or original-preservation guarantees.

## Excluded threats and limitations

Hostile concurrent local actors, other users changing the same folders, ACL
manipulation, reparse conversion races and administrator/root interference are
outside this model. Held-handle and identity checks serve ordinary ownership and
change detection; hostile namespace containment requires a different design.

Filesystem bugs and power-loss behavior beyond the platform's synchronization
semantics are outside the guarantee. Network filesystems can violate those
semantics without reporting an error. Native operations can block beyond a
cooperative stop request. Observed failures and synchronization/address limitations
remain explicit in results.

Retained HTML and text can contain unsafe content or remote references. MIME body
classification supplies neither HTML sanitization nor malware detection.
