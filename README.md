# EML Attachment Remover

`remove-eml-attachments` creates a verified text-only EML working copy. It binds the
sender's safe plain-text body as the content source and, only when an HTML alternative
has exactly the same non-whitespace text, uses that local HTML structure to restore
readable paragraphs and lists. It discards the HTML representation with its embedded
resources and removes ordinary file attachments elsewhere.

It uses Python's standard-library MIME parser rather than regular expressions.
The selection is based on MIME structure and dependency references, not filenames,
image recognition, sender rules, or payload-content heuristics. Payloads are decoded
only to validate transfer encoding and bind the transformation to cryptographic
fingerprints before an output is published.

> **Back up EML files before processing them.** This application is designed never
> to overwrite an input and writes a derived copy instead, but an independent backup
> still protects against accidental deletion, storage failure, and user error.

## Key properties

- CPython 3.14.x baseline; no third-party runtime dependencies.
- One file or a whole Finder selection can be processed in one command.
- Outputs default to `SOURCE.text-only.eml` beside each source.
- Exactly one safe plain-text body representation anchors the source-content
  precondition for the derived message.
- When one unselected HTML representation has identical non-whitespace text, its
  local paragraph, list, table, and quote structure is projected into canonical plain
  text. Otherwise the selected plain body is retained as-is.
- Whenever transformation is required, the verified text is promoted to a
  wrapper-free, non-multipart root `text/plain` entity for mail-client interoperability.
  An already-canonical root plain message is copied byte-for-byte.
- Unselected HTML representations are discarded atomically with their complete
  `cid:`, Content-Location, and `multipart/related` resource closure.
- Explicit file attachments, attached images, and attached EML messages are removed
  outside discarded body representations.
- If a safe text-only representation cannot be proven—including HTML-only input or
  protected content needed by the selected body—the input fails without an output.
- The complete transformation is planned at immutable source-tree MIME paths before
  any part is discarded.
- The original EML is never overwritten, directly or through a hard link or symbolic
  link.
- The selected decoded text is bound to its source by SHA-256 after canonical newline
  normalization. Any eligible HTML layout projection is separately bound as the exact
  expected output text. Output is written through a temporary file, flushed, reparsed,
  and required to match the retained root's leaf fingerprint and structure; a second
  text-only planning pass must be a complete no-op, proving the stored message is
  canonical root `text/plain`, before atomic placement.
- Existing DKIM and ARC transport signatures are left as historical headers, but a
  warning is issued because changing the body invalidates them.
- Signed, encrypted, and opaque security MIME is never guessed at or partially
  rewritten. A protected subtree needed by the selected body, or whose role is
  ambiguous, makes the transformation unavailable; an explicit attachment or
  unselected protected subtree may instead be discarded atomically.

The text-only file is a **working copy**, not a replacement for the evidential
original.

## Zero-install use

The [official GitHub Release](https://github.com/resoltico/EMLAttachmentRemover/releases/latest)
includes a dependency-free Python zipapp (`.pyz`). It is a standard ZIP archive
containing the application and a `__main__.py` entry point; it is **not** a native
executable and does not bundle Python. Download it from the release, then run it with
CPython 3.14:

```sh
python3.14 remove-eml-attachments.pyz "message.eml"
```

Windows:

```powershell
py -3.14 .\remove-eml-attachments.pyz "message.eml"
```

For independent verification, download `SHA256SUMS` and all three checksummed assets
into one directory. On macOS or Linux, verify the bytes, GitHub provenance, and
reported application version before use:

```sh
shasum -a 256 --check SHA256SUMS
gh attestation verify remove-eml-attachments.pyz \
  --repo resoltico/EMLAttachmentRemover
gh attestation verify SHA256SUMS \
  --repo resoltico/EMLAttachmentRemover
python3.14 remove-eml-attachments.pyz --version
```

The default output is:

```text
message.text-only.eml
```

The archive is portable across macOS, Linux, and Windows when that interpreter is
available. The portable core is tested by CI on all three operating systems; the
Finder and Shortcuts integration remains macOS-specific.

## Install as a command

Using `uv`:

```sh
uv tool install .
remove-eml-attachments "message.eml"
```

Or install into an existing CPython 3.14 environment:

```sh
python3.14 -m pip install .
remove-eml-attachments "message.eml"
```

## Command examples

### One file and an explicit output

```sh
remove-eml-attachments \
  --output "working copy.eml" \
  -- "original message.eml"
```

### Multiple files

```sh
remove-eml-attachments -- "one.eml" "two.eml" "three.eml"
```

Each output is written beside its source. Batch processing continues after an input
failure and returns exit code `9` when only part of the batch succeeded. Add
`--fail-fast` to stop on the first failure and return that failure's specific code.

### Shared output directory

```sh
remove-eml-attachments \
  --output-dir "/path/to/working copies" \
  -- "one.eml" "two.eml"
```

The directory must already exist. Before writing, the command completely preflights
the batch's source/output mapping and rejects duplicate output basenames or any
mapping that would use a selected source as an output. Per-file input, MIME, and
storage failures are still handled when each item is processed.

### Existing outputs

```sh
remove-eml-attachments --skip-existing -- "one.eml" "two.eml"
remove-eml-attachments --force -- "one.eml" "two.eml"
```

`--force` can replace outputs but can never overwrite any selected source file.
`--skip-existing` leaves the existing destination untouched and does not inspect or
verify it; reports identify it as skipped rather than as a newly verified output.

### Inspect without writing

```sh
remove-eml-attachments --dry-run -- "message.eml"
```

### Automation output

A single JSON document:

```sh
remove-eml-attachments --output-format json -- "one.eml" "two.eml"
```

One output path per line:

```sh
remove-eml-attachments --output-format paths -- "one.eml" "two.eml"
```

NUL-delimited paths for filenames containing newlines:

```sh
remove-eml-attachments --output-format paths0 -- "one.eml" "two.eml"
```

Diagnostics remain on standard error for `paths` and `paths0`. JSON includes both
successful and failed items in the document. Its top level declares
`"schema_version": 2` and `"scope": "text-only"`. Each successful result reports
`selected_plain_text_bodies`, `discarded_body_representations`,
`discarded_body_resources`, and `removed_attachments`; discarded resources include
the ordered source MIME paths that referenced them.

## macOS Finder and Shortcuts

A ready-to-install Finder launcher is included in
`integrations/macos-shortcuts/`. Install it:

```sh
/bin/sh integrations/macos-shortcuts/install.sh
```

The installer builds the archive from the public source tree with `python3.14`, then
copies it with the launcher. To copy a verified official release archive instead,
set `EML_REMOVER_ZIPAPP` to that downloaded `.pyz`; set `EML_REMOVER_PYTHON` when
your CPython 3.14 command is not named `python3.14`.

Then create a Finder Quick Action in **Shortcuts** named **Create Text-Only EML Copy**.
Configure it to receive **Files** from Finder, add **Run Shell Script**, set the
action's **Input** to **Shortcut Input**, and set **Pass Input** to **as arguments**.
Paste the exact command printed by the installer. For the default installation,
this shorter command is equivalent:

```sh
/bin/sh "$HOME/Library/Application Support/EML Attachment Remover/run-from-finder.sh" "$@"
```

The launcher accepts all selected files in one process, works around Shortcuts'
restricted `PATH`, produces outputs beside the sources, and reveals completed files
in Finder. Detailed instructions and optional settings are in
[`integrations/macos-shortcuts/README.md`](integrations/macos-shortcuts/README.md).

## What text-only means

A successful modified output is one non-multipart root `text/plain` entity, without
the source's `multipart/mixed`, `multipart/alternative`, or `multipart/related`
wrappers. This canonical shape is intentionally suitable for mail clients and file
previewers such as Apple Mail and Quick Look. When no equivalent HTML formatting is
available, the selected content-transfer encoding, encoded payload, and applicable
`Content-*` representation headers are promoted to the root. An equivalent HTML layout
projection instead becomes canonical UTF-8 plain text. Safe message and envelope
headers remain in their original order; invalidated size/attachment markers and wrapper
preamble or epilogue text are removed. A source that is already canonical root plain
text requires no rewrite and is copied byte-for-byte.

The selected decoded content remains exact after canonical MIME newline normalization
(`CRLF`, `CR`, and `LF` become `LF`) and is bound by SHA-256 during verification. An
eligible HTML projection must have identical non-whitespace text and is bound as the
expected stored payload. The output contains no unselected HTML body, dependent embedded
body resource, ordinary attachment, or redundant MIME wrapper. Reported MIME paths
continue to identify the source tree, before promotion; they are audit locations, not
the derived root path.

An image uploaded through an email editor can be declared `inline`, placed below
`multipart/related`, and referenced from HTML with `cid:` even when a person thinks
of it as an attachment. MIME provides no reliable bit that distinguishes such an
image from a signature logo. When the message also supplies a safe plain-text
alternative, the application therefore discards the whole unselected HTML
representation and its resource closure instead of guessing image-by-image.

The application never trusts arbitrary HTML as a content source. It uses HTML only as
a local formatting projection after proving exact non-whitespace equality with the
safe plain-text source; it never fetches remote content, performs OCR, or inspects an
image semantically. If the source offers no safe resource-free plain-text
representation, processing fails closed and publishes nothing.

## Exit codes

| Code | Meaning |
| ---: | --- |
| 0 | Complete success, including requested skips |
| 2 | Command-line usage error |
| 3 | Input file error |
| 4 | Output conflict or unsafe source/output alias |
| 5 | Unsafe or malformed MIME / transfer encoding |
| 6 | A verified text-only transformation is unavailable |
| 7 | Output write or directory error |
| 8 | Generated EML failed post-write verification |
| 9 | At least one item failed in a multi-file batch |
| 70 | Unexpected internal failure |
| 130 | Interrupted by the user |

## Development and assurance

Contributor quick start:

```sh
uv sync --locked --group dev --python 3.14.7
uv run python tools/tasks.py test
```

[QA.md](QA.md) is the single detailed assurance contract and contains the full
qualification commands, test profiles, acceptance thresholds, generated-report
policy, CI boundaries, and release-trust model.

## Project layout

```text
src/eml_attachment_remover/    MIME processor and CLI
tests/                          pytest, property, stateful, and black-box tests
.github/workflows/             quality, exploration, mutation, and release gates
tools/tasks.py                  cross-platform local task entry point
tools/build_zipapp.py           deterministic zero-install archive builder
tools/qualify_release.py        portable exact-set builder and verifier
tools/check_repository_hygiene.py  public-workspace policy gate
tools/repository_hygiene_policy.py public filename and text-content rules
tools/repository_path_policy.py    cross-platform source-path rules
tools/archive_surface_policy.py    portable distribution-member rules
build/                          ignored local build output
integrations/macos-shortcuts/   Finder launcher, installer, and instructions
QA.md                           quality-assurance contract
```

## License

MIT © 2026 Ervins Strauhmanis. See [LICENSE](LICENSE).
