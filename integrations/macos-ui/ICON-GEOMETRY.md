# Split-paperclip construction

The SVG layers in `EML.icon/Assets` are the editable production source. The following supplied construction script records the measurements used to draw them. It requires NumPy and Shapely and writes two SVG files into its working directory; it is not part of the application build. Exact regeneration across dependency versions has not been established. The script and generated drawings have the proprietary artwork terms in the root LICENSE.

```python
"""EML Attachment Remover app icon - original paperclip geometry.
Authored from scratch for this project: four parallel wires joined by three
semicircular turns, defined below by explicit coordinates (no third-party path data).
Builds two filled-outline SVG layers (the clip cut in half) on a 1024x1024 canvas."""

import math, numpy as np
from shapely.geometry import LineString, Polygon
from shapely import affinity

# --- Original clip skeleton (upright, local units) ---------------------------
SPACING = 96  # distance between neighbouring wires
WIRE = 50  # wire thickness
xs = [-1.5 * SPACING, -0.5 * SPACING, 0.5 * SPACING, 1.5 * SPACING]  # -144,-48,48,144


def arc(cx, cy, r, a0, a1, n=96):
    return [
        (cx + r * math.cos(t), cy + r * math.sin(t)) for t in np.linspace(a0, a1, n)
    ]


R1, R2, R3 = 0.5 * SPACING, SPACING, 1.5 * SPACING  # turn radii 48, 96, 144
pts = [(xs[2], -170), (xs[2], 170)]  # inner wire, down
pts += arc(0, 170, R1, 0, math.pi)  # small turn (bottom)
pts += [(xs[1], -230)]  # second wire, up
pts += arc(xs[2], -230, R2, math.pi, 2 * math.pi)  # medium turn (top)
pts += [(xs[3], 220)]  # third wire, down
pts += arc(0, 220, R3, 0, math.pi)  # large turn (bottom)
pts += [(xs[0], -240)]  # outer wire, up (free end)
# Note: y grows downward (SVG convention); arcs sweep through the bottom/top as labelled.

clip = LineString(pts).buffer(WIRE / 2, cap_style=1, join_style=1, quad_segs=32)
minx, miny, maxx, maxy = clip.bounds
clip = affinity.translate(
    clip, -(minx + maxx) / 2, -(miny + maxy) / 2
)  # centre on origin

# --- Cut across the clip's long axis, pull halves apart ----------------------
GAP = 53  # each half moves this far along the axis
top = clip.intersection(Polygon([(-999, -999), (999, -999), (999, 0), (-999, 0)]))
bottom = clip.intersection(Polygon([(-999, 0), (999, 0), (999, 999), (-999, 999)]))
top = affinity.translate(top, 0, -GAP)
bottom = affinity.translate(bottom, 0, GAP)

# --- Place on the 1024 canvas: rotate 45 deg clockwise, scale, centre -------
SCALE = 1.0
top, bottom = [
    affinity.scale(affinity.rotate(g, 45, origin=(0, 0)), SCALE, SCALE, origin=(0, 0))
    for g in (top, bottom)
]
bx0, by0, bx1, by1 = top.union(bottom).bounds  # optical centring on the canvas
top, bottom = [
    affinity.translate(g, 512 - (bx0 + bx1) / 2, 512 - (by0 + by1) / 2)
    for g in (top, bottom)
]


def d(g):
    polys = [g] if g.geom_type == "Polygon" else list(g.geoms)
    out = []
    for pg in polys:
        for ring in [pg.exterior, *pg.interiors]:
            out.append(
                "M" + " L".join(f"{x:.2f} {y:.2f}" for x, y in ring.coords) + " Z"
            )
    return " ".join(out)


def svg(g, fill):
    return (
        '<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 1024 1024">'
        f'<path fill="{fill}" fill-rule="evenodd" d="{d(g)}"/></svg>\n'
    )


open("2-clip-half-yellow.svg", "w").write(svg(top, "#FFD166"))
open("1-clip-half-white.svg", "w").write(svg(bottom, "#FFFFFF"))
u = top.union(bottom).bounds
print(
    "combined bounds",
    [round(v) for v in u],
    "size",
    round(u[2] - u[0]),
    round(u[3] - u[1]),
)
```
