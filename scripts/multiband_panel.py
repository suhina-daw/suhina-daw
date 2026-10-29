# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Writes devices/multiband/panel.json: the compact panel and the expanded strip.

The layout follows the design boards D (fitted to the device row) and E (the expanded view). The
panels are drawn from expressions over the device's parameters and its live values (`readout`:
per band its level and gain change, the output meter and the input spectrum), so the bands,
crossovers, thresholds, curves and the signal follow the sound. Crossovers and thresholds are
handles dragged in the band view. Every colour follows the theme: text, panel, control, border and
accent are the SDK's theme colours, and the display, the band colours and the gain line are theme
tokens (`--device-display`, `--device-series-1..3`, `--device-highlight`) a theme may set.
Run from the repository root: python scripts/multiband_panel.py
"""

import json
import math
from pathlib import Path

# The SDK's theme colours (resolved against the theme's palette).
TEXT = [236, 232, 225, 255]
PANEL = [37, 36, 33, 255]
CONTROL = [46, 45, 42, 255]
BORDER = [56, 55, 51, 255]
ACCENT = [166, 163, 255, 255]
# Theme tokens (crates/device-sdk/src/ui.rs › THEME_TOKENS): their CSS property or the design's.
DISPLAY = [3, 0, 1, 255]
HIGHLIGHT = [3, 0, 5, 255]
BANDS = [("low", "Low", [3, 0, 2, 255]), ("mid", "Mid", [3, 0, 3, 255]), ("high", "High", [3, 0, 4, 255])]
GLOBALS = ["lowCrossover", "highCrossover", "peak", "softKnee", "amount", "time", "output"]
BAND_PARAMETERS = ["On", "Input", "AboveThreshold", "AboveSlope", "BelowThreshold", "BelowSlope", "Attack", "Release", "Gain"]
# The device's readouts (devices/multiband/src/lib.rs › readout).
OUTPUT_LEFT, OUTPUT_RIGHT, SPECTRUM = 6, 7, 8
SPECTRUM_BANDS = 24
CROSSOVER_RANGES = {"lowCrossover": (30.0, 3_000.0), "highCrossover": (300.0, 15_000.0)}
INVISIBLE = [0, 0, 0, 0]


def tint(color, alpha):
    return color[:3] + [alpha]


def par(pid, normalized=False, log=False):
    return {"op": "parameter", "parameter": pid, "normalized": normalized, "logarithmic": log}


def add(a, b):
    return {"op": "add", "a": a, "b": b}


def sub(a, b):
    return add(a, mul(b, -1.0))


def mul(a, b):
    return {"op": "multiply", "a": a, "b": b}


def equal(a, b):
    return {"op": "equal", "a": a, "b": b}


def minimum(a, b):
    return {"op": "min", "a": a, "b": b}


def maximum(a, b):
    return {"op": "max", "a": a, "b": b}


def sin(v):
    return {"op": "sin", "value": v}


def cos(v):
    return {"op": "cos", "value": v}


def readout(index):
    """The device's live value `index`: per band 2b its level in dB, 2b + 1 its gain change in dB."""
    return {"op": "readout", "index": index}


def reciprocal(value):
    """1 / value for a positive value, through exp(-ln v)."""
    return {"op": "exp", "value": mul({"op": "log", "value": value}, -1.0)}


def step(value):
    """0 below 0, 1 above it (steep but continuous)."""
    return maximum(0.0, minimum(1.0, mul(value, 200.0)))


def opened():
    """1 while the device's expanded strip is open."""
    return par("#expanded")


def closed():
    return sub(1.0, opened())


def muted(bid):
    """1 while band `bid` is not heard: off, or another band soloed and not this one."""
    any_solo = maximum(par("lowSolo"), maximum(par("midSolo"), par("highSolo")))
    return maximum(sub(1.0, par(f"{bid}On")), mul(any_solo, sub(1.0, par(f"{bid}Solo"))))


def log_x(hz_value, x0, width):
    """x of a frequency (an expression in Hz) on a 20 Hz - 20 kHz log axis."""
    return add(mul(add({"op": "log", "value": hz_value}, -math.log(20.0)), width / math.log(1000.0)), x0)


def log_x_number(hz, x0, width):
    return x0 + (math.log(hz) - math.log(20.0)) * width / math.log(1000.0)


def level_y(value, y0, height):
    """y of a level in dB, 0 at the top and -80 at the bottom."""
    return add(mul(maximum(-80.0, minimum(0.0, value)), -height / 80.0), y0)


def rect(x, y, w, h, color, radius=0.0):
    return {"kind": "rect", "rect": [x, y, w, h], "color": color, "radius": radius}


def line(a, b, color, width=1.0):
    return {"kind": "line", "from": a, "to": b, "color": color, "width": width}


def circle(c, r, color):
    return {"kind": "circle", "center": c, "radius": r, "color": color}


def path(points, color, width=1.0, fill=None, closed_path=False):
    return {"kind": "path", "points": points, "color": color, "width": width, "closed": closed_path, "fill": fill}


def text(x, y, caption, color=TEXT, size=10.0, center=False, parameter=None, fmt="number", decimals=0):
    return {"kind": "text", "position": [x, y], "text": caption, "color": color, "size": size,
            "align": "center" if center else "left", "format": fmt, "parameter": parameter, "decimals": decimals}


def value_text(x, y, value, prefix="", suffix="", color=TEXT, size=9.0, decimals=1, center=False):
    return {"kind": "value", "position": [x, y], "value": value, "prefix": prefix, "suffix": suffix, "color": color,
            "size": size, "decimals": decimals, "align": "center" if center else "left"}


def edges(x0, w):
    return [x0, log_x(par("lowCrossover"), x0, w), log_x(par("highCrossover"), x0, w), x0 + w]


def dashed(x_from, x_to, y, color, width=1.3, dashes=14):
    """A dashed line between two x expressions at `y`: `dashes` dashes, each 60 % of its share."""
    span = sub(x_to, x_from)
    return [line([add(x_from, mul(span, k / dashes)), y], [add(x_from, mul(span, (k + 0.6) / dashes)), y], color, width)
            for k in range(dashes)]


def band_change_at(centre, xs_hz):
    """The gain change of the band a frequency `centre` (Hz) falls in."""
    ln_c = math.log(centre)
    above_low = step(sub(ln_c, {"op": "log", "value": par("lowCrossover")}))
    above_high = step(sub(ln_c, {"op": "log", "value": par("highCrossover")}))
    return add(readout(1), add(mul(sub(readout(3), readout(1)), above_low), mul(sub(readout(5), readout(3)), above_high)))


def band_view(x0, y0, w, h, big=False):
    """Design D's band view over [x0, x0 + w] x [y0, y0 + h]: the dark display, the bands tinted,
    a grid, the input spectrum filled in the accent with the output dotted over it, the 0 dB line
    and the gain line under it (each band's change, shaded), each band's thresholds dashed with a
    ring on the above one, the crossovers with their grips and frequencies, the band names with
    their change (the chosen one boxed), the axis, and a grey veil over a band that is not heard."""
    xs = edges(x0, w)
    label_size, value_size = (13.0, 12.0) if big else (11.0, 10.0)
    draws = [rect(x0, y0, w, h, DISPLAY, 3.0)]
    for i, (_, _, color) in enumerate(BANDS):
        draws.append(rect(xs[i], y0, sub(xs[i + 1], xs[i]), h, tint(color, 26 if i == 1 else 15)))
    for k in (1, 2, 3):
        y = y0 + h * k / 4.0
        draws.append(line([x0, y], [x0 + w, y], tint(TEXT, 15)))
    for hz in (50.0, 200.0, 1000.0, 5000.0):
        x = log_x_number(hz, x0, w)
        draws.append(line([x, y0], [x, y0 + h], tint(TEXT, 15)))
    # The spectrum: the analyser's bands sit evenly on the log axis; one convex quad per step.
    step_w = w / SPECTRUM_BANDS
    centres_x = [x0] + [x0 + (i + 0.5) * step_w for i in range(SPECTRUM_BANDS)] + [x0 + w]
    index = [0] + list(range(SPECTRUM_BANDS)) + [SPECTRUM_BANDS - 1]
    centres_hz = [20.0 * 1000.0 ** ((i + 0.5) / SPECTRUM_BANDS) for i in index]
    level_in = [readout(SPECTRUM + i) for i in index]
    ys_in = [level_y(v, y0, h) for v in level_in]
    ys_out = [level_y(add(v, band_change_at(c, None)), y0, h) for v, c in zip(level_in, centres_hz)]
    for i in range(len(centres_x) - 1):
        quad = [[centres_x[i], ys_in[i]], [centres_x[i + 1], ys_in[i + 1]], [centres_x[i + 1], y0 + h], [centres_x[i], y0 + h]]
        draws.append(path(quad, INVISIBLE, 0.0, fill=tint(ACCENT, 70), closed_path=True))
    draws.append(path([[x, y] for x, y in zip(centres_x, ys_in)], tint(ACCENT, 210), 1.4))
    for i in range(0, len(centres_x) - 1, 2):
        draws.append(line([centres_x[i], ys_out[i]], [centres_x[i + 1], ys_out[i + 1]], tint(TEXT, 100), 1.0))
    # 0 dB of gain change, and the gain line under it: 3 px per dB (3.4 in the design's 178 px).
    zero = y0 + h * 0.17
    per_db = h * 3.4 / 178.0
    draws.append(line([x0, zero], [x0 + w, zero], tint(TEXT, 50)))
    for i in range(3):
        left, right = xs[i], xs[i + 1]
        cut = minimum(mul(maximum(0.0, mul(readout(2 * i + 1), -1.0)), per_db), h - (zero - y0))
        gain_y = add(zero, cut)
        draws.append(rect(left, zero, sub(right, left), cut, tint(HIGHLIGHT, 40)))
        draws.append(line([left, gain_y], [right, gain_y], HIGHLIGHT, 2.0 if big else 1.8))
    # Thresholds: dashed in the band's colour, a ring on the above one in the middle of its band.
    for i, (bid, _, color) in enumerate(BANDS):
        left, right = xs[i], xs[i + 1]
        above = level_y(par(f"{bid}AboveThreshold"), y0, h)
        below = level_y(par(f"{bid}BelowThreshold"), y0, h)
        draws += dashed(left, right, above, color, 1.5 if big else 1.3)
        draws += dashed(left, right, below, tint(color, 120), 1.0)
        middle = mul(add(left, right), 0.5)
        ring = 6.0 if big else 5.0
        draws += [circle([middle, above], ring, color), circle([middle, above], ring - 2.0, DISPLAY)]
    # The crossovers: a line, a grip at the bottom and the frequency beside it.
    for i, pid in ((1, "lowCrossover"), (2, "highCrossover")):
        x = xs[i]
        draws.append(line([x, y0], [x, y0 + h], ACCENT, 2.0))
        draws.append(circle([x, y0 + h - (16.0 if big else 12.0)], 7.0 if big else 5.0, ACCENT))
        draws.append(text(add(x, -46.0), y0 + h - (34.0 if big else 28.0), "", ACCENT, value_size, parameter=pid,
                          fmt="frequency", decimals=0))
    # Band names with their gain change; the chosen band's is boxed in its colour.
    for i, (bid, name, color) in enumerate(BANDS):
        left = xs[i]
        chosen = equal(par("band"), float(i))
        box_w = 78.0 if big else 64.0
        box_h = 18.0 if big else 15.0
        draws.append(rect(add(left, 4.0), y0 + 3.0, mul(chosen, box_w), box_h, color, 3.0))
        draws.append(rect(add(left, 5.0), y0 + 4.0, mul(chosen, box_w - 2.0), box_h - 2.0, DISPLAY, 3.0))
        draws.append(text(add(left, 8.0), y0 + 4.0, name, TEXT, label_size))
        draws.append(value_text(add(left, 38.0 if big else 32.0), y0 + 5.0, readout(2 * i + 1),
                                suffix=" dB" if big else "", color=HIGHLIGHT, size=value_size))
        draws.append(rect(left, y0, sub(xs[i + 1], left), mul(muted(bid), h), tint(DISPLAY, 190)))
    ticks = (20, 50, 100, 200, 500, 1000, 2000, 5000, 10000, 20000) if big else (20, 100, 500, 2000, 10000, 20000)
    for hz in ticks:
        caption = f"{hz // 1000}k" if hz >= 1000 else str(hz)
        x = min(max(log_x_number(hz, x0, w), x0 + 6.0), x0 + w - 24.0)
        draws.append(text(x, y0 + h - 12.0, caption, tint(TEXT, 110), 9.0 if not big else 10.0))
    if big:
        for k, db in enumerate((0, -12, -24, -36)):
            draws.append(text(x0 + w - 28.0, zero - 6.0 + k * 12.0 * per_db, str(db), tint(TEXT, 110), 10.0))
    return draws


def curve(bid, size, color):
    """A band's transfer curve in a size x size box: the below stage, 1:1 between, the above stage,
    the signal's place on it, and a grey veil while the band is not heard."""
    s = size / 80.0
    bt, bs, at, aslope = par(f"{bid}BelowThreshold"), par(f"{bid}BelowSlope"), par(f"{bid}AboveThreshold"), par(f"{bid}AboveSlope")

    def point(x_db, y_db):
        return [add(mul(x_db, s), size), mul(y_db, -s)]

    out_low = sub(bt, mul(add(bt, 80.0), bs))
    in_low = add(bt, mul(sub(-80.0, bt), reciprocal(add(bs, 0.001))))
    out_top = mul(at, sub(1.0, aslope))
    in_top = sub(at, mul(at, reciprocal(add(aslope, 0.001))))
    points = [point(maximum(-80.0, in_low), maximum(-80.0, out_low)), point(bt, bt), point(at, at),
              point(minimum(0.0, in_top), minimum(0.0, out_top))]
    index = [b for b, _, _ in BANDS].index(bid)
    level_in = maximum(-80.0, minimum(0.0, readout(2 * index)))
    level_out = maximum(-80.0, minimum(0.0, add(readout(2 * index), readout(2 * index + 1))))
    draws = [rect(0.0, 0.0, size, size, DISPLAY, 3.0), rect(0.0, 0.0, size, 1.0, BORDER)]
    draws += [line([size * k / 12.0, size - size * k / 12.0], [size * (k + 0.5) / 12.0, size - size * (k + 0.5) / 12.0],
                   tint(TEXT, 50)) for k in range(12)]
    draws += [
        path(points, color, 2.4),
        circle(point(level_in, level_out), 3.0, TEXT),
        rect(0.0, 0.0, size, mul(muted(bid), size), tint(DISPLAY, 190)),
    ]
    return draws


def knob(pid, caption, bounds, value, suffix="", decimals=1, log=False, visible=None, tag="", big=False, prefix=""):
    """Design D's knob: its name, a ring with a pointer (the accent ring for a big one), its value.
    Dragging it up or down changes `pid`; `value` is the expression written under it."""
    _, _, w, h = bounds
    r = 19.0 if big else 13.0
    cx, cy = w / 2.0, 13.0 + r
    angle = add(-0.75 * math.pi, mul(par(pid, normalized=True, log=log), 1.5 * math.pi))
    tip = [add(cx, mul(sin(angle), r - 3.0)), sub(cy, mul(cos(angle), r - 3.0))]
    drawing = [
        text(cx, 0.0, caption, tint(TEXT, 170), 9.5, center=True),
        circle([cx, cy], r, ACCENT if big else BORDER),
        circle([cx, cy], r - (3.0 if big else 2.0), CONTROL),
        line([cx, cy], tip, TEXT, 3.0 if big else 2.0),
        value_text(cx, cy + r + 3.0, value, prefix=prefix, suffix=suffix, size=9.5, decimals=decimals, center=True),
    ]
    return {"visible_when": visible, "id": f"{pid}{tag}", "name": caption,
            "tooltip": f"{caption}: drag vertically; Shift for precision; double click resets", "bounds": bounds,
            "gesture": "drag", "x": None, "y": {"parameter": pid, "logarithmic": log, "drag_scale": 1.0}, "drawing": drawing}


def button(cid, pid, caption, bounds, gesture="toggle", visible=None, off_label=None):
    return {"widget": {"kind": "button", "label": caption, "off_label": off_label, "indicator": False},
            "visible_when": visible, "id": cid, "name": caption, "tooltip": caption, "bounds": bounds,
            "gesture": gesture, "x": {"parameter": pid, "logarithmic": False, "drag_scale": 1.0}, "y": None,
            "drawing": []}


def power(cid, bid, name, bounds, visible=None):
    """The band's power button: a power symbol, grey while off and in the accent while on."""
    _, _, w, h = bounds
    cx, cy, r = w / 2.0, h / 2.0, min(w, h) / 2.0 - 4.0
    drawing = [rect(0.0, 0.0, w, h, CONTROL, 3.0)]
    for color, scale in ((tint(TEXT, 110), 1.0), (ACCENT, par(f"{bid}On"))):
        drawing += [
            circle([cx, cy], mul(scale, r), color),
            circle([cx, cy], mul(scale, r - 1.6), CONTROL),
            rect(cx - 2.2, cy - r - 1.0, 4.4, mul(scale, r), CONTROL),
            line([cx, cy - r + 0.5], [cx, add(cy - r + 0.5, mul(scale, r * 0.9))], color, 1.6),
        ]
    return {"visible_when": visible, "id": cid, "name": f"{name} on", "tooltip": f"Turn the {name.lower()} band on or off",
            "bounds": bounds, "gesture": "toggle", "x": {"parameter": f"{bid}On", "logarithmic": False, "drag_scale": 1.0},
            "y": None, "drawing": drawing}


def display(cid, name, bounds, drawing, bind, visible=None):
    """A drawing; dragging it up or down moves `bind` (a drawn control needs a binding)."""
    return {"visible_when": visible, "id": cid, "name": name, "tooltip": f"{name}: drag up or down to change it",
            "bounds": bounds, "gesture": "drag", "x": None,
            "y": {"parameter": bind, "logarithmic": False, "drag_scale": 1.0}, "drawing": drawing}


def handle(cid, name, bounds, pid, axis, log=False, span=None, visible=None):
    """A line dragged directly; the band view draws it, so the handle itself is invisible."""
    binding = {"parameter": pid, "logarithmic": log, "drag_scale": 1.0}
    return {"widget": {"kind": "handle", "color": INVISIBLE, "span": span}, "visible_when": visible, "id": cid,
            "name": name, "tooltip": f"{name}: drag to move it; double click resets", "bounds": bounds,
            "gesture": "position", "x": binding if axis == "x" else None, "y": binding if axis == "y" else None,
            "drawing": []}


def handles(prefix, x0, y0, w, h, visible=None):
    """The crossovers dragged sideways and each band's two thresholds up and down. A handle's
    bounds are its parameter's range on the axis, so its line sits where the value is."""
    controls = []
    for pid, name in (("lowCrossover", "Low crossover"), ("highCrossover", "High crossover")):
        low, high = CROSSOVER_RANGES[pid]
        left, right = log_x_number(low, x0, w), log_x_number(high, x0, w)
        controls.append(handle(f"{prefix}{pid}_handle", name, [left, y0, right - left, h], pid, "x", log=True,
                               visible=visible))
    xs = edges(x0, w)
    for i, (bid, name, _) in enumerate(BANDS):
        for which, caption in (("AboveThreshold", "above threshold"), ("BelowThreshold", "below threshold")):
            controls.append(handle(f"{prefix}{bid}{which}_handle", f"{name} {caption}", [x0, y0, w, h], f"{bid}{which}",
                                   "y", span=[xs[i], xs[i + 1]], visible=visible))
    return controls


def separator(cid, x, y, h, visible=None):
    return {"widget": {"kind": "separator"}, "visible_when": visible, "id": cid, "name": "Separator", "tooltip": "",
            "bounds": [x - 4.0, y, 8.0, h], "gesture": "drag", "x": None, "y": None}


def preset_button(cid, caption, index, bounds, visible=None):
    return {"widget": {"kind": "button", "label": caption, "off_label": None, "indicator": False}, "visible_when": visible,
            "id": cid, "name": f"{caption} preset", "tooltip": f"Apply the {caption} quick preset", "bounds": bounds,
            "gesture": {"preset": index}, "x": None, "y": None, "drawing": []}


def meter(cid, bounds, visible=None):
    """The output's two peak meters (accent and text, as the design), over `meterRange` dB with its
    scale; dragging up or down widens or narrows the range."""
    _, _, w, h = bounds
    top, bottom = 2.0, h - 2.0
    span = bottom - top
    rng = par("meterRange")
    drawing = []
    for i, (index, color) in enumerate(((OUTPUT_LEFT, tint(ACCENT, 160)), (OUTPUT_RIGHT, TEXT))):
        bx = 1.0 + i * 6.0
        level = maximum(0.0, add(1.0, mul(maximum(mul(rng, -1.0), minimum(0.0, readout(index))), reciprocal(rng))))
        drawing.append(rect(bx, top, 5.0, span, CONTROL, 2.0))
        drawing.append(rect(bx, sub(bottom, mul(level, span)), 5.0, mul(level, span), color, 2.0))
    for k in (0.0, 0.5, 1.0):
        drawing.append(value_text(14.0, top + span * k - 4.0, mul(rng, -k), size=8.0, decimals=0, color=tint(TEXT, 120)))
    return {"visible_when": visible, "id": cid, "name": "Output meter",
            "tooltip": "Output level; drag up or down to change its range, double click resets", "bounds": bounds,
            "gesture": "drag", "x": None, "y": {"parameter": "meterRange", "logarithmic": False, "drag_scale": 1.0},
            "drawing": drawing}


def ott():
    """Heavy upward and downward compression on all three bands, Suhina's own rounded values."""
    values = {"lowCrossover": 90.0, "highCrossover": 2500.0, "peak": 0.0, "softKnee": 1.0, "amount": 1.0,
              "time": 1.0, "output": 0.0}
    bands = {"low": (-34.0, 0.015, 50.0, 280.0, 10.0), "mid": (-30.0, 0.015, 22.0, 280.0, 6.0),
             "high": (-35.0, 0.0, 13.0, 130.0, 10.0)}
    for bid, (above, slope, attack, release, gain) in bands.items():
        values.update({f"{bid}On": 1.0, f"{bid}Input": 5.0, f"{bid}AboveThreshold": above, f"{bid}AboveSlope": slope,
                       f"{bid}BelowThreshold": -41.0, f"{bid}BelowSlope": 0.24, f"{bid}Attack": attack / 1000.0,
                       f"{bid}Release": release / 1000.0, f"{bid}Gain": gain, f"{bid}Solo": 0.0})
    return {"name": "OTT", "values": values}


def default_preset():
    """Every sound parameter at the device's default (devices/multiband/src/lib.rs › PARAMS)."""
    values = {"lowCrossover": 120.0, "highCrossover": 2500.0, "peak": 0.0, "softKnee": 1.0, "amount": 1.0,
              "time": 1.0, "output": 0.0}
    for bid, attack, release in (("low", 0.05, 0.3), ("mid", 0.01, 0.2), ("high", 0.005, 0.1)):
        values.update({f"{bid}On": 1.0, f"{bid}Input": 0.0, f"{bid}AboveThreshold": -20.0, f"{bid}AboveSlope": 1.0,
                       f"{bid}BelowThreshold": -60.0, f"{bid}BelowSlope": 1.0, f"{bid}Attack": attack,
                       f"{bid}Release": release, f"{bid}Gain": 0.0, f"{bid}Solo": 0.0})
    return {"name": "Default", "values": values}


def every_parameter():
    """Every parameter id, so the panel shows only its canvas and not the generated list under it."""
    return (GLOBALS + [f"{bid}{suffix}" for bid, _, _ in BANDS for suffix in BAND_PARAMETERS] + ["band"]
            + [f"{bid}Solo" for bid, _, _ in BANDS] + ["meterRange"])


def band_knobs(bid, x0, y0, visible, tag):
    """Design D's 4 x 2 knob grid for band `bid`, each cell 48 x 70."""
    p = lambda s: f"{bid}{s}"
    ratio = lambda s: reciprocal(add(par(p(s)), 0.001))
    cells = [
        (p("AboveThreshold"), "Above", par(p("AboveThreshold")), " dB", 1, False),
        (p("AboveSlope"), "Ratio", ratio("AboveSlope"), ":1", 1, False),
        (p("BelowThreshold"), "Below", par(p("BelowThreshold")), " dB", 1, False),
        (p("BelowSlope"), "Upward", ratio("BelowSlope"), ":1", 1, False),
        (p("Attack"), "Attack", mul(par(p("Attack")), 1000.0), " ms", 0, True),
        (p("Release"), "Release", mul(par(p("Release")), 1000.0), " ms", 0, True),
        (p("Input"), "Input", par(p("Input")), " dB", 1, False),
        (p("Gain"), "Gain", par(p("Gain")), " dB", 1, False),
    ]
    controls = []
    for k, (pid, caption, value, suffix, decimals, log) in enumerate(cells):
        col, row = k % 4, k // 4
        controls.append(knob(pid, caption, [x0 + col * 49.0, y0 + row * 72.0, 48.0, 70.0], value, suffix, decimals, log,
                             visible, tag))
    return controls


def compact():
    """Design D in the device row (820 x 168): the band view at the left; a row of switches and
    presets over the chosen band's column (name, curve, power, solo), its knob grid, the global
    column (Amount, Time, Output) and the meters. While the strip is open the band view gives way
    to the three bands' curves and the rest moves left into the chosen band's column."""
    w, h = 820.0, 168.0
    sx, sy, sw, sh = 4.0, 4.0, 416.0, 160.0
    controls = [
        display("band_view", "Band view", [sx, sy, sw, sh], band_view(0.0, 0.0, sw, sh), "amount", closed()),
        *handles("", sx, sy, sw, sh, closed()),
    ]
    for i, (bid, name, color) in enumerate(BANDS):
        x = sx + i * 140.0
        controls.append(display(f"{bid}_curve_open", f"{name} curve", [x, sy, 132.0, 132.0], curve(bid, 132.0, color),
                                f"{bid}AboveSlope", opened()))
        controls.append(power(f"{bid}_power_open", bid, name, [x, sy + 138.0, 64.0, 22.0], opened()))
        controls.append(button(f"{bid}_solo_open", f"{bid}Solo", "Solo", [x + 68.0, sy + 138.0, 64.0, 22.0],
                               visible=opened()))
    controls.append(separator("sep_view", 424.0, 4.0, 160.0))
    # The row of switches over the right part: bands, detector, knee; presets at its right end.
    for i, (bid, name, _) in enumerate(BANDS):
        controls.append(button(f"tab_{bid}", "band", name, [430.0 + i * 38.0, 3.0, 36.0, 16.0], {"set": i}))
    controls.append(button("rms", "peak", "RMS", [550.0, 3.0, 36.0, 16.0], {"set": 0}))
    controls.append(button("peak_toggle", "peak", "Peak", [588.0, 3.0, 36.0, 16.0], {"set": 1}))
    controls.append(button("knee_toggle", "softKnee", "Soft knee", [628.0, 3.0, 56.0, 16.0]))
    controls.append(preset_button("preset_default", "Default", 0, [728.0, 3.0, 46.0, 16.0]))
    controls.append(preset_button("preset_ott", "OTT", 1, [776.0, 3.0, 40.0, 16.0]))
    # The chosen band's column (design D: 74 px): its dot and name, its curve, power and solo.
    for i, (bid, name, color) in enumerate(BANDS):
        shown = mul(equal(par("band"), float(i)), closed())
        heading = [circle([4.0, 6.0], 4.0, color), text(12.0, 0.0, name, TEXT, 11.0)]
        controls.append(display(f"{bid}_heading", f"{name} band", [430.0, 24.0, 64.0, 14.0], heading, f"{bid}Gain", shown))
        controls.append(display(f"{bid}_curve", f"{name} curve", [430.0, 40.0, 64.0, 64.0], curve(bid, 64.0, color),
                                f"{bid}AboveSlope", shown))
        controls.append(power(f"{bid}_power", bid, name, [430.0, 110.0, 31.0, 20.0], shown))
        controls.append(button(f"{bid}_solo", f"{bid}Solo", "Solo", [463.0, 110.0, 31.0, 20.0], visible=shown))
    # The knob grid, the global column and the meters: from x 500 while the band view shows, 70 px
    # further left (into the chosen band's column) while the strip is open.
    for tag, dx, state in (("", 0.0, closed()), ("_open", -70.0, opened())):
        for i, (bid, _, _) in enumerate(BANDS):
            controls += band_knobs(bid, 500.0 + dx, 22.0, mul(equal(par("band"), float(i)), state), tag)
        controls.append(separator(f"sep_global{tag}", 698.0 + dx, 24.0, 140.0, state))
        controls.append(knob("amount", "Amount", [702.0 + dx, 22.0, 84.0, 74.0], mul(par("amount"), 100.0), " %", 0,
                             False, state, tag, big=True))
        controls.append(knob("time", "Time", [702.0 + dx, 96.0, 42.0, 70.0], mul(par("time"), 100.0), " %", 0, True,
                             state, tag))
        controls.append(knob("output", "Output", [744.0 + dx, 96.0, 42.0, 70.0], par("output"), "", 1, False, state, tag))
        controls.append(meter(f"meter{tag}", [790.0 + dx, 22.0, 28.0, 144.0], state))
    return {"size": [w, h], "viewport_size": None, "background": PANEL, "assets": [], "layers": [], "controls": controls,
            "hidden_parameters": every_parameter(), "presets": [default_preset(), ott()], "accent_on": True}


def expanded():
    """Design E as a strip over the device row (1200 x 160, scaled to its width): the same band
    view large, its handles and the dB scale; the device's header carries its name and Close."""
    w, h = 1200.0, 160.0
    return {"size": [w, h], "viewport_size": None, "background": PANEL, "assets": [],
            "layers": band_view(0.0, 0.0, w, h, big=True), "controls": handles("x_", 0.0, 0.0, w, h),
            "hidden_parameters": every_parameter(), "strip": True, "accent_on": True}


def main():
    panel = {"ui": compact(), "expanded": expanded()}
    out = Path("devices/multiband/panel.json")
    out.write_text(json.dumps(panel, indent=1) + "\n", encoding="utf-8")
    print(f"wrote {out}")


if __name__ == "__main__":
    main()
