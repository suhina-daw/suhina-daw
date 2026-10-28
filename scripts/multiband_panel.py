# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Writes devices/multiband/panel.json: the compact panel and the expanded one.

The panels are drawn from expressions over the device's parameters, so the band areas, the
crossover lines, the thresholds and each band's transfer curve follow the knobs. Colours that the
SDK resolves against the theme (text, panel, control, border, accent) are used for everything but
the three band colours. Run from the repository root: python scripts/multiband_panel.py
"""

import json
import math
from pathlib import Path

TEXT = [236, 232, 225, 255]
PANEL = [37, 36, 33, 255]
CONTROL = [46, 45, 42, 255]
BORDER = [56, 55, 51, 255]
ACCENT = [166, 163, 255, 255]
GLOBALS = ["lowCrossover", "highCrossover", "peak", "softKnee", "amount", "time", "output"]
BAND_PARAMETERS = ["On", "Input", "AboveThreshold", "AboveSlope", "BelowThreshold", "BelowSlope", "Attack", "Release", "Gain"]
BANDS = [("low", "Low", [242, 166, 90, 255]), ("mid", "Mid", [95, 211, 192, 255]), ("high", "High", [159, 180, 255, 255])]


def tint(color, alpha):
    return color[:3] + [alpha]


def par(pid):
    return {"op": "parameter", "parameter": pid, "normalized": False, "logarithmic": False}


def add(a, b):
    return {"op": "add", "a": a, "b": b}


def mul(a, b):
    return {"op": "multiply", "a": a, "b": b}


def equal(a, b):
    return {"op": "equal", "a": a, "b": b}


def freq_x(value, x0, width):
    """x of a frequency (a parameter or a number in Hz) on a 20 Hz - 20 kHz log axis."""
    ln = {"op": "log", "value": value}
    return add(mul(add(ln, -math.log(20.0)), width / math.log(1000.0)), x0)


def level_y(value, y0, height):
    """y of a level in dB (0 at the top, -80 at the bottom)."""
    return add(mul(value, -height / 80.0), y0)


def rect(x, y, w, h, color, radius=0.0):
    return {"kind": "rect", "rect": [x, y, w, h], "color": color, "radius": radius}


def line(a, b, color, width=1.0):
    return {"kind": "line", "from": a, "to": b, "color": color, "width": width}


def text(x, y, label, color=TEXT, size=10.0, center=False, parameter=None, decimals=0, fmt="number"):
    return {"kind": "text", "position": [x, y], "text": label, "color": color, "size": size,
            "align": "center" if center else "left", "format": fmt, "parameter": parameter, "decimals": decimals}


def spectrum(x0, y0, w, h, labels):
    """The band areas, grid, crossover lines and each band's two thresholds over [x0, x0 + w]."""
    low_x = freq_x(par("lowCrossover"), x0, w)
    high_x = freq_x(par("highCrossover"), x0, w)
    edges = [x0, low_x, high_x, x0 + w]
    draws = [rect(x0, y0, w, h, CONTROL, 3.0)]
    for i, (_, _, color) in enumerate(BANDS):
        left, right = edges[i], edges[i + 1]
        draws.append(rect(left, y0, add(right, mul(left, -1.0)), h, tint(color, 26)))
    for hz in (100.0, 1000.0, 10000.0):
        x = freq_x(hz, x0, w)
        draws.append(line([x, y0], [x, y0 + h], tint(TEXT, 18)))
    for db in (-20.0, -40.0, -60.0):
        y = level_y(db, y0, h)
        draws.append(line([x0, y], [x0 + w, y], tint(TEXT, 18)))
    for i, (bid, _, color) in enumerate(BANDS):
        left, right = edges[i], edges[i + 1]
        for which, alpha in (("AboveThreshold", 255), ("BelowThreshold", 150)):
            y = level_y(par(f"{bid}{which}"), y0, h)
            draws.append(line([left, y], [right, y], tint(color, alpha), 1.5))
    for x in (low_x, high_x):
        draws.append(line([x, y0], [x, y0 + h], ACCENT, 2.0))
    if labels:
        for hz, label in ((100.0, "100"), (1000.0, "1k"), (10000.0, "10k")):
            draws.append(text(add(freq_x(hz, x0, w), 3.0), y0 + h - 13.0, label, tint(TEXT, 120), 9.0))
    return draws


def curve(bid, size, color):
    """A band's transfer curve in a size x size box: the below stage, 1:1 between, the above stage."""
    s = size / 80.0
    bt, bs, at, aslope = par(f"{bid}BelowThreshold"), par(f"{bid}BelowSlope"), par(f"{bid}AboveThreshold"), par(f"{bid}AboveSlope")

    def point(x_db, y_db):
        return [add(mul(x_db, s), size), add(mul(y_db, -s), 0.0)]

    # output at -80 dB in: Tb - (Tb + 80) * slope; at 0 dB in: Ta * (1 - slope).
    out_low = add(bt, mul(mul(add(bt, 80.0), bs), -1.0))
    out_top = mul(at, add(1.0, mul(aslope, -1.0)))
    points = [point(-80.0, out_low), point(bt, bt), point(at, at), point(0.0, out_top)]
    return [
        rect(0.0, 0.0, size, size, PANEL, 3.0),
        line([0.0, size], [size, 0.0], BORDER),
        {"kind": "path", "points": points, "color": color, "width": 2.0, "closed": False, "fill": None},
    ]


def dial(pid, label, bounds, log=False, decimals=1, fmt="number", visible=None):
    return {"widget": {"kind": "dial", "label": label, "decimals": decimals, "format": fmt}, "visible_when": visible,
            "id": pid if visible is None else pid, "name": label,
            "tooltip": "Drag vertically; Shift for precision; double click resets", "bounds": bounds,
            "gesture": "drag", "x": None, "y": {"parameter": pid, "logarithmic": log, "drag_scale": 1.0}, "drawing": []}


def button(cid, pid, label, bounds, gesture="toggle", visible=None):
    return {"widget": {"kind": "button", "label": label, "off_label": None, "indicator": False},
            "visible_when": visible, "id": cid, "name": label, "tooltip": label, "bounds": bounds,
            "gesture": gesture, "x": {"parameter": pid, "logarithmic": False, "drag_scale": 1.0}, "y": None, "drawing": []}


def number(pid, label, bounds, fmt, decimals, log=True, visible=None):
    return {"widget": {"kind": "number", "decimals": decimals, "format": fmt, "color": ACCENT, "center": True,
                       "bordered": True, "prefix": "", "suffix": ""},
            "visible_when": visible, "id": pid, "name": label,
            "tooltip": f"{label}: drag vertically; double click resets", "bounds": bounds, "gesture": "drag",
            "x": None, "y": {"parameter": pid, "logarithmic": log, "drag_scale": 1.0}, "drawing": []}


def display(cid, name, bounds, drawing, bind, visible=None):
    """A drawing; dragging it up or down moves `bind` (a drawn control needs a binding)."""
    return {"visible_when": visible, "id": cid, "name": name, "tooltip": f"{name}: drag up or down to change it",
            "bounds": bounds, "gesture": "drag", "x": None,
            "y": {"parameter": bind, "logarithmic": False, "drag_scale": 1.0}, "drawing": drawing}


def label(cid, caption, bounds):
    return {"widget": {"kind": "label", "label": caption}, "visible_when": None, "id": cid, "name": caption,
            "tooltip": "", "bounds": bounds, "gesture": "drag", "x": None, "y": None}


def every_parameter():
    """Every parameter id, so the panel shows only its canvas and not the generated list under it."""
    return GLOBALS + [f"{bid}{suffix}" for bid, _, _ in BANDS for suffix in BAND_PARAMETERS] + ["band"]


def compact():
    w, h = 820.0, 168.0
    sx, sy, sw, sh = 4.0, 4.0, 372.0, 140.0
    controls = [
        number("lowCrossover", "Low Crossover", [sx + 4.0, sy + sh + 2.0, 72.0, 18.0], "frequency", 0),
        number("highCrossover", "High Crossover", [sx + sw - 76.0, sy + sh + 2.0, 72.0, 18.0], "frequency", 0),
    ]
    for i, (bid, name, _) in enumerate(BANDS):
        controls.append(button(f"tab_{bid}", "band", name, [384.0 + i * 54.0, 4.0, 52.0, 18.0], {"set": i}))
    controls.append(button("peak_toggle", "peak", "Peak", [554.0, 4.0, 52.0, 18.0]))
    controls.append(button("knee_toggle", "softKnee", "Soft knee", [608.0, 4.0, 70.0, 18.0]))
    knobs = [("AboveThreshold", "Above", False, 1, "decibels"), ("AboveSlope", "Above slope", False, 2, "number"),
             ("BelowThreshold", "Below", False, 1, "decibels"), ("BelowSlope", "Below slope", False, 2, "number"),
             ("Attack", "Attack", True, 1, "seconds"), ("Release", "Release", True, 1, "seconds"),
             ("Input", "Input", False, 1, "decibels"), ("Gain", "Gain", False, 1, "decibels")]
    for i, (bid, name, color) in enumerate(BANDS):
        shown = equal(par("band"), float(i))
        controls.append(display(f"{bid}_curve", f"{name} curve", [384.0, 28.0, 76.0, 76.0], curve(bid, 76.0, color), f"{bid}AboveSlope", shown))
        controls.append(button(f"{bid}_on", f"{bid}On", "On", [384.0, 110.0, 76.0, 20.0], visible=shown))
        for k, (suffix, label, log, decimals, fmt) in enumerate(knobs):
            col, row = k % 4, k // 4
            controls.append(dial(f"{bid}{suffix}", label, [468.0 + col * 62.0, 28.0 + row * 68.0, 60.0, 66.0], log, decimals, fmt, shown))
    controls.append(dial("amount", "Amount", [720.0, 28.0, 96.0, 66.0], False, 0, "percent"))
    controls.append(dial("time", "Time", [720.0, 96.0, 48.0, 66.0], True, 0, "percent"))
    controls.append(dial("output", "Output", [768.0, 96.0, 48.0, 66.0], False, 1, "decibels"))
    return {"size": [w, h], "viewport_size": None, "background": PANEL, "assets": [],
            "layers": spectrum(sx, sy, sw, sh, True), "controls": controls, "hidden_parameters": every_parameter()}


def expanded():
    w, h = 980.0, 320.0
    sx, sy, sw, sh = 8.0, 28.0, 964.0, 190.0
    controls = []
    for i, (bid, name, color) in enumerate(BANDS):
        x = 8.0 + i * 326.0
        controls.append(label(f"{bid}_title", name, [x, 4.0, 60.0, 20.0]))
        controls.append(number(f"{bid}AboveThreshold", f"{name} Above", [x + 64.0, 4.0, 70.0, 20.0], "decibels", 1, False))
        controls.append(number(f"{bid}BelowThreshold", f"{name} Below", [x + 138.0, 4.0, 70.0, 20.0], "decibels", 1, False))
        controls.append(display(f"{bid}_curve_x", f"{name} curve", [x + 121.0, sy + sh + 32.0, 84.0, 84.0], curve(bid, 84.0, color), f"{bid}AboveSlope"))
    controls.append(number("lowCrossover", "Low Crossover", [sx, sy + sh + 6.0, 80.0, 20.0], "frequency", 0))
    controls.append(number("highCrossover", "High Crossover", [sx + sw - 80.0, sy + sh + 6.0, 80.0, 20.0], "frequency", 0))
    return {"size": [w, h], "viewport_size": None, "background": PANEL, "assets": [],
            "layers": spectrum(sx, sy, sw, sh, True), "controls": controls, "hidden_parameters": every_parameter()}


def main():
    panel = {"ui": compact(), "expanded": expanded()}
    out = Path("devices/multiband/panel.json")
    out.write_text(json.dumps(panel, indent=1) + "\n", encoding="utf-8")
    print(f"wrote {out}")


if __name__ == "__main__":
    main()
