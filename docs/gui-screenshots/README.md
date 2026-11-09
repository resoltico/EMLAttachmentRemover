# Desktop captures

Toolkit captures use the production controller, pinned Slint widgets and software
testing renderer. Real temporary MIME inputs drive core; an injected runner holds
progress for deterministic captures. Native operating-system UI is checked separately.

| State | Light | Dark |
|---|---|---|
| Empty | [PNG](empty-light.png) | [PNG](empty-dark.png) |
| Ready | [PNG](ready-light.png) | [PNG](ready-dark.png) |
| Removing | [PNG](removing-light.png) | [PNG](removing-dark.png) |
| Done | [PNG](done-light.png) | [PNG](done-dark.png) |
| Processing details | [PNG](processing-details-light.png) | [PNG](processing-details-dark.png) |

The main images are 1080×700. [Ready at 760×520](ready-light-narrow.png) shows the
minimum layout with Size hidden. Geometry tests separately check three visible
rows at minimum size, stable headers and details, aligned cells, command roles
and anchored popups.

Regenerate outside the merge gate:

```sh
GUI_CAPTURE_DIR=/absolute/output/folder cargo test --locked -p eml-gui --test desktop \
  acceptance::s20_capture_one_list_states -- --ignored --nocapture
```

The capture writes lossless PPMs. Convert each to PNG with, for example,
`magick input.ppm output.png`; committed images are not scaled.

## Native app gallery

Native captures show the ad hoc signed macOS debug app with its production
controller and core facts. The size/theme gallery retains the complete client
area, excluding the OS title bar. Application content is not retouched or scaled.

| Window | Light | Dark |
|---|---|---|
| Full-size client, 1474×938 | [JPEG](manual/full-light.jpg) | [JPEG](manual/full-dark.jpg) |
| Normal, 1080×700 client | [JPEG](manual/normal-light.jpg) | [JPEG](manual/normal-dark.jpg) |
| Minimum, 760×520 client | [JPEG](manual/minimum-light.jpg) | [JPEG](manual/minimum-dark.jpg) |
| Processing details, 1080×700 client | [PNG](manual/processing-details-light.png) | [PNG](manual/processing-details-dark.png) |

The selected inline-image detail includes core's warning about omitted related
resources. Normal and minimum windows keep actions beside the email name above
the scrolling details. Processing details shows core's Code, Phase and Detail.

## Native scenarios

These complete 1080×732 window captures show checked and settled batches,
filesystem failures and settings/input admission. Done and rerun show the problems
filter enabled; the action count still represents the entire eligible batch.

| State | Capture |
|---|---|
| Checked problems filter and whole-batch scope | [PNG](manual/field-filter.png) |
| Finished mixed batch with truthful failure count | [PNG](manual/field-done.png) |
| Existing copies plus refusals | [PNG](manual/field-rerun.png) |
| Full-disk cause and completed cleanup | [PNG](manual/disk-full.png) |
| Unavailable folder retains checked emails | [PNG](manual/unavailable-folder.png) |
| Unreadable settings note | [PNG](manual/settings-unreadable.png) |
| Selection limit without a partial count | [PNG](manual/selection-limit.png) |

Native capture work must preserve and restore user preferences and appearance.
Separate resize from capture and verify actual image dimensions; exclude stale
post-resize frames. Do not scale or retouch application content.
