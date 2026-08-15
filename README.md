# EML Attachment Remover

`remove-eml-attachments` creates a smaller, derived EML copy with downloadable file
attachments removed while preserving the resources needed to render the message
body—especially inline company logos, social icons, and other email-signature images.

It uses Python's standard-library MIME parser rather than regular expressions.
Removal decisions use MIME structure, metadata, and body-resource references—not
payload-content inspection. Payloads are still decoded to validate transfer encoding
and fingerprint every retained part before an output is published.

> **Back up EML files before processing them.** This application is designed never
> to overwrite an input and writes a derived copy instead, but an independent backup
> still protects against accidental deletion, storage failure, and user error.

## Key properties

- CPython 3.14.x baseline; no third-party runtime dependencies.
- One file or a whole Finder selection can be processed in one command.
- Outputs default to `SOURCE.attachments-removed.eml` beside each source.
- Explicit file attachments, attached images, and attached EML messages are removed.
- Inline resources, referenced Content-IDs, Content-Locations, and
  `multipart/related` resources are preserved.
- Outside opaque cryptographic MIME, a part labelled `attachment` is retained only
  when the body actually references its Content-ID or Content-Location. This covers
  malformed-but-rendered signature graphics without retaining unrelated image
  attachments; protected content remains opaque.
- The original EML is never overwritten, directly or through a hard link or symbolic
  link.
- Output is written through a temporary file, flushed, reparsed, and checked against
  SHA-256 fingerprints of every retained MIME payload before atomic placement.
- Existing DKIM and ARC transport signatures are left as historical headers, but a
  warning is issued because changing the body invalidates them.
- Encrypted MIME remains opaque. The program refuses operations that would require
  reserialising `multipart/signed` content after an attachment removal.

The attachment-removed file is a **working copy**, not a replacement for the evidential
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
message.attachments-removed.eml
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
successful and failed items in the document.

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

Then create a Finder Quick Action in **Shortcuts** named **Remove EML Attachments**.
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

## What is preserved

The default policy retains a non-text MIME part when one or more of these conditions
apply:

- the HTML body references its `Content-ID` through a `cid:` URI;
- the HTML body references its `Content-Location`;
- it has `Content-Disposition: inline`;
- it is a non-attachment resource under `multipart/related`;
- it carries Content-ID or Content-Location metadata and is not explicitly an
  attachment;
- it is protected cryptographic MIME content.

The program does not use image recognition to decide whether an image is a
"signature". It preserves body resources based on MIME structure and actual body
references. That is more predictable and avoids deleting legitimate rendered
content.

## Exit codes

| Code | Meaning |
| ---: | --- |
| 0 | Complete success, including requested skips |
| 2 | Command-line usage error |
| 3 | Input file error |
| 4 | Output conflict or unsafe source/output alias |
| 5 | Unsafe or malformed MIME / transfer encoding |
| 6 | Signed or otherwise protected MIME cannot be safely rewritten |
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
