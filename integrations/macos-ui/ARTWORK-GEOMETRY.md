# Interface artwork construction reference

The editable production drawing is `Artwork.swift`. The following supplied Python construction script documents how the SVG reference exports were produced. It is a separate reference renderer, not a runtime or build input, and is covered by the proprietary artwork terms in the root LICENSE. Preserve its geometry and palettes alongside any deliberate changes to the reference exports; do not treat it as a second authority for runtime behavior. Run the script in a copy of the reference directory to avoid overwriting edited assets.

```python
"""Exports the status artwork from Artwork.swift's geometry as SVG reference files.

Mirrors Artwork.swift exactly (100-unit square; Swift uses a bottom-left origin, SVG a
top-left one, so every y here is 100 - y_swift). Artwork.swift remains the runtime source;
these SVGs are design references and must be regenerated if the Swift geometry changes.
"""

import math, os

STYLES = {  # light, dark, high-contrast light, high-contrast dark
    "light": 0,
    "dark": 1,
    "hc-light": 2,
    "hc-dark": 3,
}
ENVELOPE = ["#636366", "#AEAEB2", "#3A3A3C", "#E5E5EA"]
MARK = ["#FFFFFF", "#1C1C1E", "#FFFFFF", "#000000"]
ID_BADGE = ["#0B3D91", "#FFD166", "#0B3D91", "#FFD166"]
ID_MARK = ["#FFD166", "#0B3D91", "#FFD166", "#0B3D91"]
BADGE = {
    "accent": ["#0066D6", "#409CFF", "#0050A8", "#6BB3FF"],
    "success": ["#1F7A35", "#30D158", "#146128", "#5BE07A"],
    "warning": ["#B54700", "#FF9F0A", "#8F3800", "#FFB340"],
    "failure": ["#C4281C", "#FF6961", "#A11A12", "#FF8A80"],
    "neutral": ["#636366", "#AEAEB2", "#48484A", "#D1D1D6"],
}
DEFAULT_TONE = {
    "identity": "accent",
    "processing": "accent",
    "success": "success",
    "attention": "warning",
    "stopped": "neutral",
}


def y(v):
    return 100 - v


def metrics(compact, hc):
    return dict(
        envelope=8 + compact + hc * 1.5,
        mark=5.5 + compact + hc,
        ring=7 + hc * 0.5,
        ew=5 + compact * 1.5,
        ed=3 + compact * 0.6,
    )


def line(points, width, color):
    pts = " ".join(f"{px},{y(py)}" for px, py in points)
    return (
        f'<polyline points="{pts}" fill="none" stroke="{color}" stroke-width="{width}" '
        'stroke-linecap="round" stroke-linejoin="round"/>'
    )


def svg(kind, tone, s, compact):
    hc = 1 if s >= 2 else 0
    m = metrics(compact, hc)
    env = ENVELOPE[s]
    ink = MARK[s]
    cx, cy = 72, y(30)
    out = [
        '<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100" viewBox="0 0 100 100">',
        f'<defs><mask id="m" maskUnits="userSpaceOnUse" x="0" y="0" width="100" height="100">'
        f'<rect width="100" height="100" fill="#fff"/><circle cx="{cx}" cy="{cy}" r="25" fill="#000"/></mask></defs>',
        f'<g mask="url(#m)"><rect x="8" y="{y(80)}" width="62" height="46" rx="9" fill="none" '
        f'stroke="{env}" stroke-width="{m["envelope"]}"/>',
        line([(15, 73), (39, 54), (63, 73)], m["envelope"], env),
        "</g>",
    ]
    badge = BADGE[tone][s]
    if kind == "identity":
        out.append(f'<circle cx="{cx}" cy="{cy}" r="18" fill="{ID_BADGE[s]}"/>')
        out.append(line([(63.5, 21.5), (68.5, 26.5)], m["mark"], ID_MARK[s]))
        out.append(line([(75.5, 33.5), (80.5, 38.5)], m["mark"], ID_MARK[s]))
    elif kind == "processing":
        r = 18 - m["ring"] / 2
        op = 0.45 if hc else 0.3
        end = math.radians(-139)  # Swift: start 90 deg, clockwise to -139 deg (y-up)
        ex, ey = 72 + r * math.cos(end), y(30 + r * math.sin(end))
        out.append(
            f'<circle cx="{cx}" cy="{cy}" r="{r}" fill="none" stroke="{badge}" '
            f'stroke-opacity="{op}" stroke-width="{m["ring"]}"/>'
        )
        out.append(
            f'<path d="M72 {y(30 + r)} A{r} {r} 0 1 1 {ex:.3f} {ey:.3f}" fill="none" '
            f'stroke="{badge}" stroke-width="{m["ring"]}" stroke-linecap="round"/>'
        )
    else:
        out.append(f'<circle cx="{cx}" cy="{cy}" r="18" fill="{badge}"/>')
        if kind == "success":
            out.append(line([(63, 30), (69, 24), (81, 37)], m["mark"], ink))
        elif kind == "attention":
            out.append(
                f'<rect x="{72 - m["ew"] / 2}" y="{y(42)}" width="{m["ew"]}" height="14" rx="2" fill="{ink}"/>'
            )
            out.append(f'<circle cx="72" cy="{y(22)}" r="{m["ed"]}" fill="{ink}"/>')
        else:
            out.append(
                f'<rect x="65" y="{y(37)}" width="14" height="14" rx="3" fill="{ink}"/>'
            )
    out.append("</svg>\n")
    return "\n".join(out)


if __name__ == "__main__":
    here = os.path.dirname(os.path.abspath(__file__))
    for size_class, compact in (("regular", 0), ("compact", 1)):
        for style, s in STYLES.items():
            folder = os.path.join(here, size_class, style)
            os.makedirs(folder, exist_ok=True)
            for kind, tone in DEFAULT_TONE.items():
                with open(os.path.join(folder, f"{kind}.svg"), "w") as f:
                    f.write(svg(kind, tone, s, compact))
            with open(os.path.join(folder, "attention-failure.svg"), "w") as f:
                f.write(svg("attention", "failure", s, compact))
```
