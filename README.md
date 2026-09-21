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

## Rendering boundary

The tool does not convert HTML to text, infer semantic equivalence, fetch remote
resources, parse or sanitize HTML/CSS, or promise identical rendered appearance.

Retained HTML remains byte-for-byte source content. It can include data URIs, remote
URLs, scripts, styles, tracking markup, or references to related components that are
not retained. Mail-client security and rendering choices remain the mail client's job;
missing related-media placeholders are expected.

## Use

CPython 3.14 is required. Install from source with `uv tool install .`, or run the
release zipapp with CPython 3.14:

```sh
remove-eml-attachments -- "original.eml"
python3.14 remove-eml-attachments.pyz -- "original.eml"
```

The default destination is beside the requested source directory entry:

```text
original.mime-pruned.eml
```

The tool preserves kernel path traversal semantics. For example, a path containing a
symlink and `..` opens the object selected by the kernel, not a lexically normalized
path. Destination publication is same-directory, private-mode staging followed by an
atomic no-replace operation; the program never replaces an existing derived file.
On Windows, the bound destination directory is opened with the write access required
for its `FlushFileBuffers` durability receipt. If a filesystem still cannot provide
that receipt, the visible copy is reported as `published_with_error`, never `created`.

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
terminal-unsafe path text.

```sh
remove-eml-attachments --output-format=json -- "one.eml" "two.eml"
remove-eml-attachments --output-format=paths0 -- "one.eml" "two.eml"
```

JSON is one schema-3 report document. Its checked-in contract is
[`schema/report.schema.json`](schema/report.schema.json); it reports every input in
order, source-bound retained payload hashes, removal reasons, candidate digest,
verification evidence, and truthful publication receipts. It never reports body
contents. During processing, terminal evidence is streamed through bounded private
spools; if normal report persistence fails after a visible publication, the command
returns a complete status-preserving recovery report rather than claiming success.
Path `text` is ordinary Unicode only. For a POSIX path containing surrogate-escaped
native bytes, `text` is `null`, `native_base64` remains authoritative, and `display`
is safe presentation text. Consumers must not reconstruct a path from `display`.

One physical Unix-From envelope line at the start of one otherwise ordinary message
is supported and preserved byte-for-byte. This is not mbox import: concatenated
mailboxes, interior `From ` body lines, and duplicate leading envelopes are not
treated as a message transport feature.

## Finder Quick Action

Install `integrations/macos-shortcuts/install.sh`, then create a Shortcuts Finder
Quick Action named **Create MIME-Pruned EML Copy**. Configure its shell step with
`|| :`, then add the current **Show Content** action immediately after it so failed
and mixed batches remain visible in Finder. The launcher defaults to
`--existing=verify`, validates schema 3, forwards cancellation to the processor, and
reveals only `created` or `existing_verified` outputs with a final address receipt.
For native-only POSIX reports it decodes and validates `native_base64`; it never
uses the display string as a reveal path.
See [the Finder instructions](integrations/macos-shortcuts/README.md).

## Verification and release artifacts

The release contains the zipapp, `SHA256SUMS`, and attestations. After downloading all
assets into one directory, verify them before use:

```sh
shasum -a 256 --check SHA256SUMS
gh attestation verify remove-eml-attachments.pyz --repo resoltico/EMLAttachmentRemover
python3.14 remove-eml-attachments.pyz --version
```

The private field corpus is never part of the repository, report, test fixtures, or
release artifacts. Quality and release procedures are documented in [QA.md](QA.md).
`uv run python tools/tasks.py release` writes its local qualified candidates to a new
external temporary directory and prints that directory; GitHub releases are the
authoritative public artifacts.
