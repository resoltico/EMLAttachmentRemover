# EML Attachment Remover

`remove-eml-attachments` creates a structurally verified **MIME-pruned** EML
working copy. It preserves every retained original `text/plain` and `text/html`
representation, in source order and as original payload bytes. It removes only:

- a part with an exact `Content-Disposition: attachment`; and
- a direct non-root component of `multipart/related`.

Everything else is kept or rejected. In particular, filenames, media types,
`inline`, Content-ID, Content-Location, `cid:` references, HTML, CSS, and message
position are never used as weaker evidence that content is disposable. Originals are
never modified.

For a prebuilt native macOS application, download the Apple Silicon (`arm64`) or Intel (`x86_64`) ZIP matching your Mac from the GitHub Release. It requires macOS 14 or later and CPython 3.14, but no Swift compiler; see the [first-launch and Gatekeeper setup steps](integrations/macos-ui/README.md#first-launch-and-macos-approval) and [release pipeline](integrations/macos-ui/RELEASE.md).

## Rendering boundary

The tool does not convert HTML to text, infer semantic equivalence, fetch remote
resources, parse or sanitize HTML/CSS, or promise identical rendered appearance.

Retained HTML remains byte-for-byte source content. It can include data URIs, remote
URLs, scripts, styles, tracking markup, or references to related components that are
not retained. Mail-client security and rendering choices remain the mail client's job;
missing related-media placeholders are expected.

## Use

CPython 3.14 is required. Install the CLI from a clean source checkout with `uv tool install --python 3.14 .`, or install a downloaded release wheel with `uv tool install --python 3.14 /absolute/path/to/eml_attachment_remover-VERSION-cp314-none-any.whl`. Replace `VERSION` with the downloaded release version. The macOS app installer does not install this CLI. Alternatively, invoke the release zipapp directly with CPython 3.14:

```sh
remove-eml-attachments -- "original.eml"
python3.14 remove-eml-attachments.pyz -- "original.eml"
```

The default destination is beside the requested source directory entry:

```text
original.mime-pruned.eml
```

When that derived name would exceed the destination directory's filename limit (255
bytes on most POSIX filesystems, 255 UTF-16 units on Windows), only the derived name
is shortened: a readable prefix of the source name, a stable 16-hex digest of the
complete source name, and the usual suffix, for example
`long-subject-line-c15a952383e18790.mime-pruned.eml`. The digest keeps the name the
same on every run, so `--existing=verify` finds it again, and keeps different long
names apart. An explicit `--output` name is never altered.

The tool preserves kernel path traversal semantics. For example, a path containing a
symlink and `..` opens the object selected by the kernel, not a lexically normalized
path. Destination publication is same-directory, private-mode staging followed by an
atomic no-replace operation; the program never replaces an existing derived file.
On Windows, the bound destination directory is opened with the write access required
for its `FlushFileBuffers` durability receipt. If a filesystem still cannot provide
that receipt, the visible copy is reported as `published_with_error`, never `created`.

### Input and filesystem limits

Each source file is limited to 128 MiB of raw bytes. Supported transfer encodings do not expand beyond their encoded input, and retained leaf payload spans are disjoint; there is no separate 96 MiB retained-payload limit. This is an input-size limit, not a peak-RAM guarantee: parsing and verification can hold several representations concurrently. Batch processing is limited to 4,096 requests and 4 MiB of cumulative native path bytes; OS command-line limits can be lower. The native app accepts at most 4,095 file paths per launch because Foundation’s 4,096-argument limit also includes the launcher script. Oversized launches are refused before processing; select fewer files and try again.

Publication requires a filesystem that supports the backend’s exclusive atomic publication primitive. If a destination volume does not support it, the command refuses publication rather than exposing a partial copy. For example, macOS exFAT can refuse exclusive rename. Use `--output-dir` to write on a compatible local volume while reading the original from the other volume; the source remains unchanged. Network and FUSE support depends on the specific filesystem and server and has not been universally qualified.

### Destination and existing policy

```sh
remove-eml-attachments --output "working.eml" -- "original.eml"
remove-eml-attachments --output-dir "working-copies" -- "one.eml" "two.eml"
remove-eml-attachments --existing=verify -- "original.eml"
remove-eml-attachments --dry-run -- "original.eml"
```

`--existing=error` is the default. `--existing=verify` accepts an existing path only
when its regular-file bytes exactly equal the freshly built, independently verified
candidate for the current source. A dirty, attachment-bearing, truncated, changing,
symbolic, special, or source-aliasing entry is never returned as usable.

The supported report formats are `human`, `json`, and raw NUL-delimited `paths0`.
`paths0` is not available with `--dry-run` and emits only `created` or
`existing_verified` paths. JSON is always canonical ASCII JSON transported through
the binary stdout channel; `paths0` is always native path bytes followed by NUL.
Human output uses the final display channel's codec and escapes unrepresentable or
terminal-unsafe path text. Each line names where the copy is, or would be:
`created: source.eml -> /path/to/source.mime-pruned.eml`; a dry run shows the
planned destination.

Each report is staged in the requested format and mode and validated before
delivery. Machine-readable sealing uses bytes throughout, including native POSIX
filenames. If staging fails after a
visible publication, retained terminal receipts produce a complete recovery report
in bounded memory, independently of staging files and spool reads, in the same byte
format. A staged read failure before stdout accepts its first report byte can also
recover through fresh memory channels. Stderr progress is tracked separately;
already emitted diagnostics are not repeated. After any report bytes have been
accepted, a failure never restarts or appends a replacement document. Receipt
storage stays owned until delivery resolves. Each recovery channel is limited to
128 MiB. The CLI owns its authoritative ledger before any input can publish;
cancellation during reservation, inventory, final archiving, or transition to
reporting retains every known outcome. Receipt spools use OS-owned temporary
handles: anonymous/unlinked-open storage on POSIX and delete-on-close storage on
Windows. Forced termination does not depend on Python cleanup to remove them.
Human and `paths0` output explain batch failures once on stderr while
retaining truthful successful item statuses and paths. Delivery still depends on an
available output endpoint; a closed pipe or channel error can prevent completion.
Signals record cancellation requests; explicit checkpoints acknowledge them before
publication or after a complete publication outcome is committed. A proven copy
keeps its successful receipt, and genuine post-publication errors keep their error
status. The API returns completed item evidence with invocation interruption metadata. A late signal preserves any report bytes already delivered and produces one bounded interruption explanation. Complete or partial error documents are never replaced.
The public API keeps the caller's signal policy outside its owned controller.
Unresponsive processing is bounded to 10 seconds after the first signal;
a second signal ends it immediately. An interruption before delivery reports the
interruption and exits 130. Human and `paths0` output include one bounded, safely
escaped invocation notice on stderr, even when all inputs completed. A signal during
delivery leaves the document whole and unchanged (its `exit_code` is the processing
outcome), then exits 130 with an `interrupted by SIGINT after the report was delivered`
diagnostic. Delivery never waits forever for a reader that stopped draining: after the
first signal the process allows 10 seconds without output progress, and a second
signal ends it at once, both with status 130 and possibly a partial document, which is
never followed by another. Without a signal there is no deadline, so a pager works.

These are application-selected statuses. CPython can change the final process
exit to **120** if interpreter cleanup fails while flushing a buffered standard
stream after `SystemExit`; see [Python's exit contract](https://docs.python.org/3.14/library/sys.html#sys.exit).
Treat 120 as an output-finalization failure. Validate any available complete report
and retain its truthful processing evidence, while treating the invocation as
unsuccessful. Closed report pipes select status 1, including Windows pipe writes
that raise an invalid-argument error. A broken stdout endpoint may provide no usable report; a partial
document remains invalid and is never replaced. A complete JSON document keeps its
original `exit_code`, which can differ from the parent's observed process status.
Unbuffered controls can retain 130 on the same failed endpoints, but do not make
those endpoints deliverable. Library callers own interpreter and stream shutdown.

```sh
remove-eml-attachments --output-format=json -- "one.eml" "two.eml"
remove-eml-attachments --output-format=paths0 -- "one.eml" "two.eml"
```

JSON is one schema-3 report document. Its checked-in contract is
[`schema/report.schema.json`](schema/report.schema.json); it reports every input in
order, source-bound retained payload hashes, removal reasons, candidate digest,
verification evidence, and truthful publication receipts. It never reports body
contents. Error messages are capped at 2,048 characters and 12,288 canonical ASCII
JSON content bytes, including any truncation marker. Admission reserves the same
encoded-byte budget. During processing, terminal evidence is streamed through
bounded private spools; if normal report persistence fails after a visible publication, the command
returns a complete status-preserving recovery report rather than claiming success.
That evidence is measured before any copy is written: an input whose report record
would exceed 1 MiB (about 3,700 retained MIME parts), or the 64 MiB report capacity,
fails with `PARSE_ERROR` and no copy is created.
Path `text` is ordinary Unicode only. For a POSIX path containing surrogate-escaped
native bytes, `text` is `null`, `native_base64` remains authoritative, and `display`
is safe presentation text. Consumers must not reconstruct a path from `display`.

Expected validation and report-storage startup errors preserve the selected JSON
format and mode, including `--dry-run`, without depending on temporary-file
allocation. Options after `--` are filenames and do not change report intent.
Repeated output-format options use the final effective known choice, including
usage-error inference for both joined and separated option-value forms.

The exported Python facade `process_file(source, destination=None, existing="error",
dry_run=False)` returns a detached single-item ledger. Internal report files are
closed before it returns; no private cleanup API is needed. The result retains
bounded removal, retained-payload, candidate-digest, verification, warning, and
publication evidence. Large source and candidate byte buffers are released. The
same prepublication report-capacity checks apply to this convenience API.
Relative native paths are anchored to the working directory captured when the
backend is imported, preserving that handle while filesystem binding proceeds.
Pass absolute source/destination paths when an API caller changes its working
directory or dispatches work from other threads.

Physical optional header names follow RFC 5322: printable ASCII other than space
and colon, including punctuation such as `X-Trace/Id` and `X(Trace)`. Unknown optional
fields are preserved as exact source spans. Structured MIME values retain their
strict token rules, singleton checks, and independent structural validation;
unsupported forms such as comments after a disposition token are conservatively
refused.

One physical Unix-From envelope line at the start of one otherwise ordinary message
is supported and preserved byte-for-byte. This is not mbox import: concatenated
mailboxes, interior `From ` body lines, and duplicate leading envelopes are not
treated as a message transport feature.

## Finder Quick Action

Use the [native macOS application](integrations/macos-ui/README.md) for processing progress, Stop processing, and final report windows. Install the prebuilt app or build it from source, create a Finder Quick Action that passes Shortcut Input as arguments to the documented application launch command, as its sole action. Each batch gets its own report window; a previous open report does not hold the shortcut open. The shortcut's completion confirms launch, so synchronous automation must use the CLI or zipapp.

## Verification and release artifacts

The v4 delivery contains the standalone zipapp, a wheel, a source tarball, separate Apple Silicon and Intel app ZIPs, and `SHA256SUMS`; the published release also has GitHub provenance attestations. The complete names and audiences are listed in the [release asset table](integrations/macos-ui/RELEASE.md#exact-release-assets). Download all five artifacts and the manifest into one directory for the full checksum check below. For an individual artifact, compare its SHA-256 digest with its manifest entry. Attestations are verified through GitHub’s CLI, rather than treated as another member of the six-file delivery:

```sh
shasum -a 256 --check SHA256SUMS
gh attestation verify remove-eml-attachments.pyz --repo resoltico/EMLAttachmentRemover
python3.14 remove-eml-attachments.pyz --version
```

The private field corpus is never part of the repository, report, test fixtures, or
release artifacts. Quality and release procedures are documented in [QA.md](QA.md).
Repository QA and release commands require UV 0.12.21.
`uv run python tools/tasks.py release` on macOS writes the complete native and portable candidate to a new external temporary directory and prints that directory; GitHub releases are the authoritative public artifacts. For portable CLI artifacts alone on any supported host, run `uv run python -B tools/qualify_release.py --output-directory /absolute/fresh/output`.
