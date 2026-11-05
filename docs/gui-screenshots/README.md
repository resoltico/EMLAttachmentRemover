# Desktop review captures

These eight PNGs show the production controller and Slint views at 1080 × 700,
using the pinned Cupertino widgets and software testing backend. The palette is
explicitly light or dark. They are toolkit captures, not screenshots of native
macOS panels. Captured from the production UI in this change after the final
accessibility and caption corrections.

| State | Content | Light | Dark |
|---|---|---|---|
| A | Empty draft | [PNG](A-light.png) | [PNG](A-dark.png) |
| B | Two inputs, ready to create | [PNG](B-light.png) | [PNG](B-dark.png) |
| C | Active run, 6 of 15 processed | [PNG](C-light.png) | [PNG](C-dark.png) |
| D | Settled run, 12 created and 3 needing attention | [PNG](D-light.png) | [PNG](D-dark.png) |

C and D use controlled CLI protocol records so progress and all attention states
can be reviewed consistently. They do not establish native-panel behavior or MIME
classification. All eight images were visually inspected. The separate local app
check uses the built bundle and real processor.

To reproduce outside the merge gate:

```sh
GUI_CAPTURE_DIR=/absolute/output/folder cargo test -p eml-gui --test desktop \
  captures::capture_redesign_states_in_light_and_dark -- --ignored
```

The capture writes lossless PPMs; the checked-in PNGs are lossless conversions.

See the [F3 investigation](f3-investigation.md) for the confirmed clipping cause
and the distinction between clipped verdicts and painted overlap.
