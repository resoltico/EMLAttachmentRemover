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

## Important v3 boundary

Version 3 is a hard break from the unsafe v2 text-only product. Do not rely on v2.0.1
derived files as faithful copies: retain original EML files and regenerate from those
originals. v3 does not convert HTML to text, infer semantic equivalence, fetch remote
resources, parse or sanitize HTML/CSS, or promise identical rendered appearance.

Retained HTML remains byte-for-byte source content. It can include data URIs, remote
URLs, scripts, styles, tracking markup, or references to related components that v3
removed. Mail-client security and rendering choices remain the mail client's job;
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
atomic no-replace operation; v3 never replaces an existing derived file.

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

`--force`, `--skip-existing`, and newline-delimited `--output-format=paths` were
removed. v3 provides `human`, `json`, and raw NUL-delimited `paths0`; `paths0` is not
available with `--dry-run` and emits only `created` or `existing_verified` paths.

```sh
remove-eml-attachments --output-format=json -- "one.eml" "two.eml"
remove-eml-attachments --output-format=paths0 -- "one.eml" "two.eml"
```

JSON is one schema-3 report document. Its checked-in contract is
[`schema/report.schema.json`](schema/report.schema.json); it reports every input in
order, source-bound retained payload hashes, removal reasons, candidate digest,
verification evidence, and truthful publication receipts. It never reports body
contents.

## Finder Quick Action

Install `integrations/macos-shortcuts/install.sh`, then create a Shortcuts Finder
Quick Action named **Create MIME-Pruned EML Copy**. The launcher defaults to
`--existing=verify`, validates schema 3, forwards cancellation to the processor, and
reveals only `created` or `existing_verified` outputs with a final address receipt.
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
