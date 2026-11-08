# Desktop captures

Toolkit captures use the production controller, pinned Slint widgets and software
testing renderer. Stub protocol fixtures provide deterministic progress and
outcomes; they do not verify MIME classification or native operating-system UI.

| State | Light | Dark |
|---|---|---|
| Empty | [PNG](empty-light.png) | [PNG](empty-dark.png) |
| Ready | [PNG](ready-light.png) | [PNG](ready-dark.png) |
| Removing | [PNG](removing-light.png) | [PNG](removing-dark.png) |
| Done | [PNG](done-light.png) | [PNG](done-dark.png) |

The main images are 1080×700. [Ready at 760×520](ready-light-narrow.png) shows the
minimum layout with Size hidden. Geometry tests separately check two visible rows,
stable headers and details, aligned cells, command roles and anchored popups.

Regenerate outside the merge gate:

```sh
GUI_CAPTURE_DIR=/absolute/output/folder cargo test --locked -p eml-gui --test desktop \
  acceptance::s20_capture_one_list_states -- --ignored --nocapture
```

The capture writes lossless PPMs. Convert each to PNG with, for example,
`magick input.ppm output.png`; committed images are not scaled.

## Native app gallery

These window captures show the current macOS desktop with freshly built debug
executables. The archive gate separately verifies universal distribution binaries. Normal and minimum sizes
refer to client areas; the native title bar adds to the outer frame (32 px on this macOS host). Full-size images capture the host’s available 1474×938 client region; the window manager constrains larger requests.

| Window | Light | Dark |
|---|---|---|
| Full-size client, 1474×938 | [JPEG](manual/full-light.jpg) | [JPEG](manual/full-dark.jpg) |
| Normal, 1080×700 client | [JPEG](manual/normal-light.jpg) | [JPEG](manual/normal-dark.jpg) |
| Minimum, 760×520 client | [JPEG](manual/minimum-light.jpg) | [JPEG](manual/minimum-dark.jpg) |

The selected inline-image detail includes the core warning about omitted related
resources. Normal/minimum captures retain visible actions below the scrolling
details. Capture appearance and application preferences were restored afterward.
