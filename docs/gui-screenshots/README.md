# Desktop capture process

Capture the production controller and views from the source revision under
review. The [capture producer](../../crates/eml-gui/tests/desktop/acceptance.rs)
uses the software testing renderer, temporary MIME inputs and temporary
preferences. Light and dark appearance are selected on the view itself.
The toolkit comes from [the lockfile](../../Cargo.lock), and the language edition
and widget style come from [the GUI build settings](../../crates/eml-gui/build.rs).

## Produce captures

From the workspace root on macOS or Linux:

```sh
GUI_CAPTURE_DIR="$PWD/target/ui-captures" cargo test --locked -p eml-gui --test desktop \
  acceptance::s20_capture_one_list_states -- --ignored --exact --nocapture
```

This selects the capture producer alone. Cargo can compile missing or changed
inputs. When the exact test executable from the reviewed revision's Cargo
invocation is available, run it directly:

```sh
GUI_CAPTURE_DIR=/absolute/output/folder /absolute/path/to/desktop-test-executable \
  --ignored --exact acceptance::s20_capture_one_list_states --nocapture
```

Keep the source revision, target, compiler and renderer information with the
capture artifacts. Use the executable produced by that revision's invocation;
a cache filename or modification time alone does not establish its inputs.

## Output mapping

The producer writes lossless PPM files. Sizes below are logical client sizes;
the PPM header records the actual pixel dimensions.

| State | Light output | Dark output | Logical size |
|---|---|---|---|
| Empty | `empty-light.ppm` | `empty-dark.ppm` | 1080×700 |
| Ready | `ready-light.ppm` | `ready-dark.ppm` | 1080×700 |
| Removing | `removing-light.ppm` | `removing-dark.ppm` | 1080×700 |
| Done | `done-light.ppm` | `done-dark.ppm` | 1080×700 |
| Processing details | `processing-details-light.ppm` | `processing-details-dark.ppm` | 1080×700 |
| Ready at minimum width | `ready-light-narrow.ppm` | — | 760×520 |

For a PNG preview, convert each output without resizing, for example:

```sh
magick /absolute/output/folder/ready-light.ppm /absolute/output/folder/ready-light.png
```

Review the complete client area in both appearances and at minimum width.
Preserve native pixel dimensions and application content during conversion;
verify dimensions before comparing layouts. Store the reviewed images with the
revision's QA artifacts.

## Verification scope

Geometry and behavior tests separately check visible rows at minimum size,
stable headers and details, aligned cells, command roles and anchored popups.
Software captures show the application client views. Native menus, window
chrome, file panels and operating-system document delivery belong to the
[release acceptance checklist](../release.md#ci-and-owner-release-steps).

For native captures, use the architecture-matched qualified app in a disposable
macOS user or VM, with synthetic inputs. The application reads normal per-user
settings, so use an isolated environment for preferences and appearance.
Separate resizing from capture, and verify the actual image dimensions to exclude
stale frames. Capture the complete client area at its native pixel dimensions.
Keep source, architecture, size and appearance with each output; preserve
application content without scaling or retouching it.
