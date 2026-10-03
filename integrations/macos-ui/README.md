# Native macOS application

Open the app and choose files, use File → Open EML Files, or invoke a Finder Quick Action. Cancelling the file picker does not interrupt another batch.

The native application presents processing progress and final results for Finder Quick Actions. It bundles the same processor as the CLI and a private, JSON-only processing launcher; it does not parse human output or implement another MIME processor. About shows the app release and build from bundle metadata. Readable Details show that app identity; copied technical reports contain the processor’s schema-3 JSON and its program version, not the native build number. The app menu also exposes the complete bundled MIT license; the Finder copyright field remains a copyright notice.

## Install a prebuilt release

Download `eml_attachment_remover-VERSION-macos-arm64.zip` for Apple Silicon or `eml_attachment_remover-VERSION-macos-x86_64.zip` for Intel from the official GitHub Release and extract it. Each prebuilt app requires macOS 14 or later and CPython 3.14; Swift and Xcode are not installation prerequisites. Install CPython 3.14 first, quit an older app before updating, and run the archive's `INSTALL.txt` instructions to record your interpreter and install into `~/Applications`. The installer refuses the wrong CPU. You can also copy the matching app there and rely on runtime discovery. Install the wheel or invoke the standalone zipapp for Terminal use.

The release app is already ad-hoc signed by CI. Customers do not re-sign it. It has no Developer ID and is not notarized, so Gatekeeper may block an internet download. If you trust the official download, follow [Apple's app-specific Open Anyway instructions](https://support.apple.com/102445); device-management policy may prohibit an exception. Signature integrity and Gatekeeper approval are separate checks. Do not disable Gatekeeper or strip quarantine to install this application.

### First launch and macOS approval

1. Install CPython 3.14, verify and extract the official ZIP, quit the previous app, and run the bundled `INSTALL.txt` installation command with your real Python executable path.
2. Open `~/Applications/EML Attachment Remover.app` in Finder. If macOS blocks it with an unidentified-developer or cannot-check-for-malicious-software warning, dismiss the warning with **Done** or **OK**, keeping the app.
3. Open **System Settings → Privacy & Security**, scroll to **Security**, and select **Open Anyway** for **EML Attachment Remover**. Authenticate with your Mac password or Touch ID if requested, then confirm **Open** in the additional warning. Attempt the launch first; the exception normally appears only after a blocked launch.
4. If no exception appears, check whether the app already opens. If it remains blocked, follow [Apple's current instructions](https://support.apple.com/102445); a managed Mac may require administrator assistance or prohibit this exception.
5. After the app opens, choose files or set up the optional Finder Quick Action below. A missing-Python error requires installing CPython 3.14 and rerunning the installer with its actual executable path; it is separate from Gatekeeper approval.

For each update, quit the app, verify and extract the new ZIP, rerun the installer, and launch the replacement. A new build may require another app-specific approval. Do not re-sign the release, strip quarantine, or disable Gatekeeper globally as routine setup steps. A damaged/invalid signature calls for investigation or a clean official download, not approval of the damaged copy. See the complete [bundled setup instructions](INSTALL.txt).

## Build from source

From a clean checkout, run `uv sync --locked --group dev` first, then install the pinned Swift.org compiler below. For the full quality gate, also install the quality tools described under [Qualification](#qualification).

From a source checkout, building requires macOS, full Xcode 26 or later for the macOS SDK, Icon Composer `actool` and signing tools, the pinned official Swift.org toolchain, and CPython 3.14. Standalone Icon Composer and Apple Command Line Tools alone cannot compile the layered icon. Install the compiler with `uv run /bin/sh integrations/macos-ui/install-swift-toolchain.sh`; the installed compiler occupies about 5.2 GB in the locally measured installation, excluding Xcode, downloads and build products. Select `arm64` or `x86_64`; each resulting app contains one CPU executable targeting macOS 14 or later. The app still requires CPython 3.14 at runtime; it does not bundle Python. Only the host architecture and OS used for qualification are live-tested locally.

Use a fresh destination for every build. The build reads the current working tree, creates a fresh zipapp, compiles the selected architecture and layered icon, and ad-hoc signs the complete bundle. The same signing command runs in GitHub macOS CI; it requires no developer certificate or secret. Ad-hoc signing is not Developer ID signing or notarization.

```sh
uv run /bin/sh integrations/macos-ui/build.sh "$HOME/Downloads/EML Attachment Remover.app" "$(uname -m)"
EML_REMOVER_PYTHON=/absolute/path/to/python3.14 /bin/sh integrations/macos-ui/install.sh "$HOME/Downloads/EML Attachment Remover.app"
```

Replace the install command’s Python placeholder with a persistent CPython 3.14 executable; do not select a temporary QA environment or a checkout environment you intend to delete.

Quit the application before updating it; the installer refuses to replace a running installed bundle. The installer records the selected CPython interpreter in a private runtime configuration outside the app, so Finder launches do not depend on shell environment inheritance. Reinstall with `EML_REMOVER_PYTHON` to change that selection.

An explicit interpreter selection must exist and be executable. If it disappears, processing fails with runtime guidance instead of silently selecting another interpreter. Automatic discovery applies only when no interpreter was selected. It looks for `python3.14` in Homebrew’s usual directories, `~/.local/bin`, pyenv shims and the launch environment’s PATH; it does not search every installed Python. Use the installer with an absolute executable path for a dependable Finder setup.

The installer defaults to `~/Applications/EML Attachment Remover.app`, refuses an existing bundle that fails the ownership, marker or bundle-identifier checks, and retains the previous application in a private application-support backup directory. The CLI is installed separately from the wheel or used directly as a zipapp. `EML_REMOVER_PYTHON` selects the build/install interpreter; `EML_REMOVER_UI_APP` selects an alternative app installation path.

## Finder Quick Action

Create a shortcut that receives Files from Finder and uses one Run Shell Script action with Shortcut Input passed **as arguments**:

```sh
/usr/bin/open -a "$HOME/Applications/EML Attachment Remover.app" -- "$@"
```

Use only this launch action. The application accepts Finder file-open events and creates a separate processing/report window for each batch; an already open report does not hold the Quick Action open. The shortcut finishes when macOS accepts the application launch, not when processing completes. Automation requiring processing results or exit status must use the CLI or standalone zipapp.

The app requests exact existing-copy matching by default, whereas the CLI defaults to refusing an existing destination. This makes repeated Finder processing reuse an unchanged matching copy while still refusing a conflicting file. `EML_REMOVER_EXISTING=error` selects refusal when supplied in the app’s launch environment; shell environment settings are not automatically inherited by Finder launches.

A native-app launch accepts at most 4,095 file paths because Foundation’s 4,096-argument limit includes the launcher script. Very long paths can exceed macOS’s argument-byte budget with fewer files. The app asks you to select fewer files; this is separate from the processor’s 4,096-item/4-MiB batch admission limits. Startup failures use a typed launcher error, and conflict explanations use the stable error code rather than matching diagnostic prose.

The running window offers **Stop processing**. This forwards interruption through its private launcher and preserves completed item receipts; it does not undo copies. The close control is unavailable while that window's run is active. A private lifetime pipe triggers launcher cancellation if the app disappears, including forced termination. Quitting the application stops its active batches and waits for the launcher to finish. A final report uses **Done**, which closes the report without changing files.

Final reports distinguish created copies, verified existing copies, failures, unprocessed or stopped files, and copies published with an error. Invocation interruption and output-finalization failure remain visible even when item receipts succeeded. Invalid or incomplete reports never imply that no files were created. Finder reveal of a copy is offered only for an accepted verified address during an ordinary completed invocation; other reports can show the source folder.

Details show the app's release and build, file outcomes, problem and warning messages, and paths. **Copy technical report** copies the launcher’s JSON envelope: the observed `process_status` and the complete public schema-3 object under `report`, including its processing exit status, diagnostic codes and exact native path evidence; **Copy technical details** supplies troubleshooting information when a complete report is unavailable. Large detail displays are bounded with an explicit truncation notice; copied technical data remains complete. File lists are virtualized, the content scrolls, and final controls remain accessible below the scrolling content. The application does not upload reports or EML contents.

## Artwork

Custom interface graphics are drawn directly from the original geometry in `Artwork.swift`. The application icon has separate editable SVG layers in `EML.icon`, compiled by Xcode’s `actool`; the runtime renderer does not build the shipped icon. No system symbol images or third-party icon library are used. See [artwork provenance and licensing](ARTWORK.md). The native operating-system controls and fonts remain runtime platform components.

## Qualification

Maintainers install the pinned quality tools with `uv run /bin/sh integrations/macos-ui/install-quality-tools.sh`, then run `uv run python tools/tasks.py quality`. See [quality and exception governance](../../QA.md#swift-quality-and-exception-governance). The prebuilt app does not require SwiftLint or swift-format on customer machines.

```sh
/bin/sh integrations/macos-ui/test.sh
```

The current macOS producer lane compiles and tests the presentation model, builds and verifies signed single-CPU bundles, and processes a synthetic EML through its bundled launcher. Older-OS lanes run the downloaded matching app without needing the newer icon compiler. GUI lifecycle, cancellation, Finder integration, appearance, and keyboard checks require live macOS qualification.

Coverage-guided receipt and address fuzzing runs locally and in a separate macOS CI job using the same pinned official Swift.org compiler as production builds, with AddressSanitizer. Customer installs need none of these development tools. See [fuzz setup, limits, reproduction and CI artifacts](../../QA.md#native-receipt-fuzzing).

## Release production

Maintainers use `uv run python -B -m tools.release_delivery --output-directory /absolute/fresh/output` on macOS. The same tool reverifies downloaded CI artifacts with `--verify-directory`. See the [release pipeline and audit contract](RELEASE.md), including the exact asset set, archive checks, signature checks, and publication boundary. Portable-only CLI qualification remains available through `tools/qualify_release.py`; it is not accepted as a complete v4 publication.

## Core and UI boundary

The Python package owns MIME transformation, publication, verification, cancellation outcomes, and the public human/JSON/path reporting interfaces. Filesystem differences remain behind the POSIX and Windows bindings; they do not select a GUI or depend on AppKit or Shortcuts. A GUI invokes the processor, consumes its structured receipts and observed process status, and owns selection, presentation and OS-specific file-reveal actions. The native app's private launcher handles macOS process ownership and transports admitted JSON; it does not provide another processing implementation. Other operating-system frontends can use the same public CLI and report schema without adding macOS presentation code to the core.

If a destination filesystem explicitly declines atomic publication, the report explains that the destination cannot create the required safe copy. Copy the EML files into a writable local folder and process those copies, or use the CLI with `--output-dir` to select a compatible destination while keeping the source on its original volume. The app does not expose an output-folder chooser; it never substitutes an operation that exposes partial final files.
