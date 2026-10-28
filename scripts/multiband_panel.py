# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Writes devices/multiband/panel.json: the compact panel and the expanded strip.

The panels are drawn from expressions over the device's parameters and its live values
(`readout`: per band its level and gain change, the output meter and the input spectrum), so the
band areas, crossovers, thresholds, curves and the signal follow the sound. Crossovers and
thresholds are handles dragged in the band view. Colours the SDK resolves against the theme (text,
panel, control, border, accent) are used for everything but the band colours and the always-dark
display. Run from the repository root: python scripts/multiband_panel.py
"""

import json
import math
from pathlib import Path

TEXT = [236, 232, 225, 255]
PANEL = [37, 36, 33, 255]
CONTROL = [46, 45, 42, 255]
BORDER = [56, 55, 51, 255]
ACCENT = [166, 163, 255, 255]
GREY = [120, 118, 112, 255]
GLOBALS = ["lowCrossover", "highCrossover", "peak", "softKnee", "amount", "time", "output"]
BAND_PARAMETERS = ["On", "Input", "AboveThreshold", "AboveSlope", "BelowThreshold", "BelowSlope", "Attack", "Release", "Gain"]
# The band view's own background: always dark whatever the theme, so the lines and the signal on
# it read; the bands only tint it.
DISPLAY = [16, 16, 18, 255]
BANDS = [("low", "Low", [242, 166, 90, 255]), ("mid", "Mid", [95, 211, 192, 255]), ("high", "High", [159, 180, 255, 255])]
# The device's readouts (devices/multiband/src/lib.rs › readout).
OUTPUT_LEFT, OUTPUT_RIGHT, SPECTRUM = 6, 7, 8
SPECTRUM_BANDS = 24
# The range of each crossover (the device's parameters).
CROSSOVER_RANGES = {"lowCrossover": (30.0, 3_000.0), "highCrossover": (300.0, 15_000.0)}


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


def minimum(a, b):
    return {"op": "min", "a": a, "b": b}


def maximum(a, b):
    return {"op": "max", "a": a, "b": b}


def readout(index):
    """The device's live value `index`: per band 2b its level in dB, 2b + 1 its gain change in dB."""
    return {"op": "readout", "index": index}


def reciprocal(value):
    """1 / value for a positive value, through exp(-ln v)."""
    return {"op": "exp", "value": mul({"op": "log", "value": value}, -1.0)}


def opened():
    """1 while the device's expanded strip is open."""
    return par("#expanded")


def closed():
    return add(1.0, mul(opened(), -1.0))


def muted(bid):
    """1 while band `bid` is not heard: off, or another band soloed and not this one."""
    any_solo = maximum(par("lowSolo"), maximum(par("midSolo"), par("highSolo")))
    by_solo = mul(any_solo, add(1.0, mul(par(f"{bid}Solo"), -1.0)))
    return maximum(add(1.0, mul(par(f"{bid}On"), -1.0)), by_solo)


def freq_x(value, x0, width):
    """x of a frequency (a parameter in Hz) on a 20 Hz - 20 kHz log axis."""
    ln = {"op": "log", "value": value}
    return add(mul(add(ln, -math.log(20.0)), width / math.log(1000.0)), x0)


def freq_x_number(hz, x0, width):
    return x0 + (math.log(hz) - math.log(20.0)) * width / math.log(1000.0)


def level_y(value, y0, height):
    """y of a level in dB (0 at the top, -80 at the bottom)."""
    return add(mul(value, -height / 80.0), y0)


def clamp_db(value):
    return maximum(-80.0, minimum(0.0, value))


def rect(x, y, w, h, color, radius=0.0):
    return {"kind": "rect", "rect": [x, y, w, h], "color": color, "radius": radius}


def line(a, b, color, width=1.0):
    return {"kind": "line", "from": a, "to": b, "color": color, "width": width}


def text(x, y, caption, color=TEXT, size=10.0):
    return {"kind": "text", "position": [x, y], "text": caption, "color": color, "size": size, "align": "left",
            "format": "number", "parameter": None, "decimals": 0}


def value_text(x, y, value, prefix="", suffix="", color=TEXT, size=9.0, decimals=1):
    return {"kind": "value", "position": [x, y], "value": value, "prefix": prefix, "suffix": suffix, "color": color,
            "size": size, "decimals": decimals, "align": "left"}


def edges(x0, w):
    return [x0, freq_x(par("lowCrossover"), x0, w), freq_x(par("highCrossover"), x0, w), x0 + w]


def band_view(x0, y0, w, h):
    """The band view's drawings over [x0, x0 + w]: the dark display, band tints, grid, the input
    spectrum as bars, each band's level line and gain-change bar with its change in dB, and a grey
    veil over bands that are not heard. Crossovers and thresholds are handles the host draws."""
    xs = edges(x0, w)
    draws = [rect(x0, y0, w, h, DISPLAY, 3.0)]
    for i, (_, _, color) in enumerate(BANDS):
        draws.append(rect(xs[i], y0, add(xs[i + 1], mul(xs[i], -1.0)), h, tint(color, 10)))
    for hz in (100.0, 1000.0, 10000.0):
        x = freq_x_number(hz, x0, w)
        draws.append(line([x, y0], [x, y0 + h], tint(TEXT, 18)))
    for db in (-20.0, -40.0, -60.0):
        y = y0 + h * -db / 80.0
        draws.append(line([x0, y], [x0 + w, y], tint(TEXT, 18)))
    # The input spectrum: one bar per band of the device's analyser, from its level to the floor.
    bar = w / SPECTRUM_BANDS
    for i in range(SPECTRUM_BANDS):
        top = level_y(clamp_db(readout(SPECTRUM + i)), y0, h)
        draws.append(rect(x0 + i * bar + 0.5, top, bar - 1.0, add(y0 + h, mul(top, -1.0)), tint(TEXT, 34)))
    for i, (bid, _, color) in enumerate(BANDS):
        left, right = xs[i], xs[i + 1]
        width = add(right, mul(left, -1.0))
        level = level_y(clamp_db(readout(2 * i)), y0, h)
        draws.append(line([left, level], [right, level], tint(TEXT, 210), 2.0))
        cut = mul(maximum(0.0, mul(readout(2 * i + 1), -1.0)), h / 80.0)
        lift = mul(maximum(0.0, readout(2 * i + 1)), h / 80.0)
        draws.append(rect(left, y0, width, minimum(cut, h), tint(color, 90)))
        draws.append(rect(left, add(y0 + h, mul(minimum(lift, h), -1.0)), width, minimum(lift, h), tint(color, 90)))
        draws.append(value_text(add(left, 4.0), y0 + 3.0, readout(2 * i + 1), suffix=" dB", color=tint(color, 255)))
        draws.append(rect(left, y0, width, mul(muted(bid), h), tint(DISPLAY, 190)))
    for hz, caption in ((100.0, "100"), (1000.0, "1k"), (10000.0, "10k")):
        draws.append(text(freq_x_number(hz, x0, w) + 3.0, y0 + h - 13.0, caption, tint(TEXT, 120), 9.0))
    return draws


def curve(bid, size, color):
    """A band's transfer curve in a size x size box: the below stage, 1:1 between, the above stage,
    the signal's place on it, the two stages as ratios, and a grey veil while the band is not
    heard."""
    s = size / 80.0
    bt, bs, at, aslope = par(f"{bid}BelowThreshold"), par(f"{bid}BelowSlope"), par(f"{bid}AboveThreshold"), par(f"{bid}AboveSlope")

    def point(x_db, y_db):
        return [add(mul(x_db, s), size), add(mul(y_db, -s), 0.0)]

    # Output at -80 dB in: Tb - (Tb + 80) * slope; at 0 dB in: Ta * (1 - slope). Where a stage
    # leaves the box first, it ends at the box's edge: the input where it meets -80 or 0 dB.
    out_low = add(bt, mul(mul(add(bt, 80.0), bs), -1.0))
    in_low = add(bt, mul(add(-80.0, mul(bt, -1.0)), reciprocal(add(bs, 0.001))))
    out_top = mul(at, add(1.0, mul(aslope, -1.0)))
    in_top = add(at, mul(mul(at, -1.0), reciprocal(add(aslope, 0.001))))
    points = [point(maximum(-80.0, in_low), maximum(-80.0, out_low)), point(bt, bt), point(at, at),
              point(minimum(0.0, in_top), minimum(0.0, out_top))]
    index = [b for b, _, _ in BANDS].index(bid)
    level_in = clamp_db(readout(2 * index))
    level_out = clamp_db(add(readout(2 * index), readout(2 * index + 1)))
    return [
        rect(0.0, 0.0, size, size, DISPLAY, 3.0),
        line([0.0, size], [size, 0.0], tint(TEXT, 30)),
        {"kind": "path", "points": points, "color": color, "width": 2.0, "closed": False, "fill": None},
        {"kind": "circle", "center": point(level_in, level_out), "radius": 3.5, "color": TEXT},
        # Slope s over the above threshold is a ratio of 1/s : 1; under the below one of 1 : s.
        value_text(size - 42.0, 3.0, reciprocal(add(aslope, 0.001)), suffix=" : 1", color=tint(TEXT, 170), size=8.0),
        value_text(3.0, size - 12.0, bs, prefix="1 : ", color=tint(TEXT, 170), size=8.0, decimals=2),
        rect(0.0, 0.0, size, mul(muted(bid), size), tint(DISPLAY, 190)),
    ]


def dial(pid, caption, bounds, log=False, decimals=1, fmt="number", visible=None):
    return {"widget": {"kind": "dial", "label": caption, "decimals": decimals, "format": fmt}, "visible_when": visible,
            "id": pid, "name": caption, "tooltip": "Drag vertically; Shift for precision; double click resets",
            "bounds": bounds, "gesture": "drag", "x": None,
            "y": {"parameter": pid, "logarithmic": log, "drag_scale": 1.0}, "drawing": []}


def button(cid, pid, caption, bounds, gesture="toggle", visible=None, off_label=None, indicator=False):
    return {"widget": {"kind": "button", "label": caption, "off_label": off_label, "indicator": indicator},
            "visible_when": visible, "id": cid, "name": caption, "tooltip": caption, "bounds": bounds,
            "gesture": gesture, "x": {"parameter": pid, "logarithmic": False, "drag_scale": 1.0}, "y": None,
            "drawing": []}


def power(cid, bid, name, bounds, visible=None):
    """The band's power button: a power symbol, grey while off and in the accent while on."""
    _, _, w, h = bounds
    cx, cy, r = w / 2.0, h / 2.0, min(w, h) / 2.0 - 4.0
    drawing = [rect(0.0, 0.0, w, h, CONTROL, 3.0)]
    for color, scale in ((GREY, 1.0), (ACCENT, par(f"{bid}On"))):
        drawing += [
            {"kind": "circle", "center": [cx, cy], "radius": mul(scale, r), "color": color},
            {"kind": "circle", "center": [cx, cy], "radius": mul(scale, r - 1.6), "color": CONTROL},
            rect(cx - 2.2, cy - r - 1.0, 4.4, mul(scale, r), CONTROL),
            line([cx, cy - r + 0.5], [cx, add(cy - r + 0.5, mul(scale, r * 0.9))], color, 1.6),
        ]
    return {"visible_when": visible, "id": cid, "name": f"{name} on", "tooltip": f"Turn the {name.lower()} band on or off",
            "bounds": bounds, "gesture": "toggle", "x": {"parameter": f"{bid}On", "logarithmic": False, "drag_scale": 1.0},
            "y": None, "drawing": drawing}


def number(pid, caption, bounds, fmt, decimals, log=True, visible=None):
    return {"widget": {"kind": "number", "decimals": decimals, "format": fmt, "color": ACCENT, "center": True,
                       "bordered": True, "prefix": "", "suffix": ""},
            "visible_when": visible, "id": pid, "name": caption,
            "tooltip": f"{caption}: drag vertically; double click resets", "bounds": bounds, "gesture": "drag",
            "x": None, "y": {"parameter": pid, "logarithmic": log, "drag_scale": 1.0}, "drawing": []}


def display(cid, name, bounds, drawing, bind, visible=None):
    """A drawing; dragging it up or down moves `bind` (a drawn control needs a binding)."""
    return {"visible_when": visible, "id": cid, "name": name, "tooltip": f"{name}: drag up or down to change it",
            "bounds": bounds, "gesture": "drag", "x": None,
            "y": {"parameter": bind, "logarithmic": False, "drag_scale": 1.0}, "drawing": drawing}


def handle(cid, name, bounds, pid, axis, color, log=False, span=None, visible=None):
    binding = {"parameter": pid, "logarithmic": log, "drag_scale": 1.0}
    return {"widget": {"kind": "handle", "color": color, "span": span}, "visible_when": visible, "id": cid,
            "name": name, "tooltip": f"{name}: drag to move it; double click resets", "bounds": bounds,
            "gesture": "position", "x": binding if axis == "x" else None, "y": binding if axis == "y" else None,
            "drawing": []}


def handles(prefix, x0, y0, w, h, visible=None):
    """Crossover lines dragged sideways and each band's two thresholds dragged up and down. A
    handle's bounds are its parameter's range on the axis, so the line sits where the value is."""
    controls = []
    for pid, name in (("lowCrossover", "Low crossover"), ("highCrossover", "High crossover")):
        low, high = CROSSOVER_RANGES[pid]
        left, right = freq_x_number(low, x0, w), freq_x_number(high, x0, w)
        controls.append(handle(f"{prefix}{pid}_handle", name, [left, y0, right - left, h], pid, "x", ACCENT, log=True,
                               visible=visible))
    xs = edges(x0, w)
    for i, (bid, name, color) in enumerate(BANDS):
        for which, caption, alpha in (("AboveThreshold", "above threshold", 255), ("BelowThreshold", "below threshold", 150)):
            controls.append(handle(f"{prefix}{bid}{which}_handle", f"{name} {caption}", [x0, y0, w, h], f"{bid}{which}",
                                   "y", tint(color, alpha), span=[xs[i], xs[i + 1]], visible=visible))
    return controls


def separator(cid, x, y, h):
    return {"widget": {"kind": "separator"}, "visible_when": None, "id": cid, "name": "Separator", "tooltip": "",
            "bounds": [x - 4.0, y, 8.0, h], "gesture": "drag", "x": None, "y": None}


def preset_button(cid, caption, index, bounds):
    return {"widget": {"kind": "button", "label": caption, "off_label": None, "indicator": False}, "visible_when": None,
            "id": cid, "name": f"{caption} preset", "tooltip": f"Apply the {caption} quick preset", "bounds": bounds,
            "gesture": {"preset": index}, "x": None, "y": None, "drawing": []}


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


def every_parameter():
    """Every parameter id, so the panel shows only its canvas and not the generated list under it."""
    return (GLOBALS + [f"{bid}{suffix}" for bid, _, _ in BANDS for suffix in BAND_PARAMETERS] + ["band"]
            + [f"{bid}Solo" for bid, _, _ in BANDS])


def meters(x, y, h):
    """The output's two peak meters, -60 to 0 dB, falling as the device's meter falls."""
    draws = []
    for i, index in enumerate((OUTPUT_LEFT, OUTPUT_RIGHT)):
        bx = x + i * 6.0
        level = maximum(0.0, add(1.0, mul(maximum(-60.0, minimum(0.0, readout(index))), 1.0 / 60.0)))
        draws.append(rect(bx, y, 4.0, h, DISPLAY, 1.0))
        draws.append(rect(bx, add(y + h, mul(level, -h)), 4.0, mul(level, h), ACCENT, 1.0))
    return draws


def compact():
    w, h = 820.0, 168.0
    sx, sy, sw, sh = 4.0, 4.0, 372.0, 138.0
    # The band view is a control, so it gives way to the three curves while the strip is open.
    controls = [
        display("band_view", "Band view", [sx, sy, sw, sh], band_view(0.0, 0.0, sw, sh), "amount", closed()),
        *handles("", sx, sy, sw, sh, closed()),
        number("lowCrossover", "Low Crossover", [sx + 4.0, sy + sh + 4.0, 72.0, 18.0], "frequency", 0, visible=closed()),
        number("highCrossover", "High Crossover", [sx + sw - 76.0, sy + sh + 4.0, 72.0, 18.0], "frequency", 0,
               visible=closed()),
    ]
    for i, (bid, name, color) in enumerate(BANDS):
        x = sx + i * 124.0
        controls.append(display(f"{bid}_curve_open", f"{name} curve", [x, sy, 116.0, 116.0], curve(bid, 116.0, color),
                                f"{bid}AboveSlope", opened()))
        controls.append(power(f"{bid}_power_open", bid, name, [x, sy + 122.0, 56.0, 22.0], opened()))
        controls.append(button(f"{bid}_solo_open", f"{bid}Solo", "Solo", [x + 60.0, sy + 122.0, 56.0, 22.0],
                               visible=opened()))
    controls.append(separator("sep_view", 381.0, 4.0, 160.0))
    for i, (bid, name, _) in enumerate(BANDS):
        controls.append(button(f"tab_{bid}", "band", name, [386.0 + i * 54.0, 4.0, 52.0, 18.0], {"set": i}))
    controls.append(button("peak_toggle", "peak", "Peak", [550.0, 4.0, 52.0, 18.0], off_label="RMS"))
    controls.append(button("knee_toggle", "softKnee", "Soft knee", [604.0, 4.0, 70.0, 18.0], indicator=True))
    knobs = [("AboveThreshold", "Above", False, 1, "decibels"), ("AboveSlope", "Above slope", False, 2, "number"),
             ("BelowThreshold", "Below", False, 1, "decibels"), ("BelowSlope", "Below slope", False, 2, "number"),
             ("Attack", "Attack", True, 1, "seconds"), ("Release", "Release", True, 1, "seconds"),
             ("Input", "Input", False, 1, "decibels"), ("Gain", "Gain", False, 1, "decibels")]
    for i, (bid, name, color) in enumerate(BANDS):
        shown = equal(par("band"), float(i))
        shown_closed = mul(shown, closed())
        controls.append(display(f"{bid}_curve", f"{name} curve", [386.0, 28.0, 80.0, 80.0], curve(bid, 80.0, color),
                                f"{bid}AboveSlope", shown_closed))
        controls.append(power(f"{bid}_power", bid, name, [386.0, 114.0, 38.0, 22.0], shown_closed))
        controls.append(button(f"{bid}_solo", f"{bid}Solo", "Solo", [428.0, 114.0, 38.0, 22.0], visible=shown_closed))
        for k, (suffix, caption, log, decimals, fmt) in enumerate(knobs):
            col, row = k % 4, k // 4
            controls.append(dial(f"{bid}{suffix}", caption, [472.0 + col * 60.0, 28.0 + row * 68.0, 58.0, 66.0], log,
                                 decimals, fmt, shown))
    controls.append(separator("sep_bands", 714.0, 4.0, 160.0))
    controls.append(preset_button("preset_ott", "OTT", 0, [720.0, 4.0, 80.0, 18.0]))
    controls.append(dial("amount", "Amount", [718.0, 28.0, 84.0, 66.0], False, 0, "percent"))
    controls.append(dial("time", "Time", [718.0, 96.0, 42.0, 66.0], True, 0, "percent"))
    controls.append(dial("output", "Output", [760.0, 96.0, 42.0, 66.0], False, 1, "decibels"))
    controls.append(separator("sep_meter", 805.0, 4.0, 160.0))
    return {"size": [w, h], "viewport_size": None, "background": PANEL, "assets": [], "layers": meters(808.0, 8.0, 152.0),
            "controls": controls, "hidden_parameters": every_parameter(), "presets": [ott()], "accent_on": True}


def expanded():
    """A wide, low strip over the device row: the band view across its width with its handles,
    each band's name with its gain change and its two thresholds over it, the crossovers under it."""
    w, h = 1200.0, 160.0
    sx, sy, sw, sh = 4.0, 24.0, 1192.0, 112.0
    controls = handles("x_", sx, sy, sw, sh)
    layers = band_view(sx, sy, sw, sh)
    for i, (bid, name, color) in enumerate(BANDS):
        x = 4.0 + i * 400.0
        layers.append(text(x, 6.0, name, tint(color, 255), 11.0))
        layers.append(value_text(x + 34.0, 7.0, readout(2 * i + 1), suffix=" dB", size=10.0))
        controls.append(number(f"{bid}AboveThreshold", f"{name} Above", [x + 100.0, 2.0, 70.0, 20.0], "decibels", 1,
                               False))
        controls.append(number(f"{bid}BelowThreshold", f"{name} Below", [x + 174.0, 2.0, 70.0, 20.0], "decibels", 1,
                               False))
    controls.append(number("lowCrossover", "Low Crossover", [sx, sy + sh + 2.0, 80.0, 20.0], "frequency", 0))
    controls.append(number("highCrossover", "High Crossover", [sx + sw - 80.0, sy + sh + 2.0, 80.0, 20.0], "frequency", 0))
    return {"size": [w, h], "viewport_size": None, "background": PANEL, "assets": [], "layers": layers,
            "controls": controls, "hidden_parameters": every_parameter(), "strip": True, "accent_on": True}


def main():
    panel = {"ui": compact(), "expanded": expanded()}
    out = Path("devices/multiband/panel.json")
    out.write_text(json.dumps(panel, indent=1) + "\n", encoding="utf-8")
    print(f"wrote {out}")


if __name__ == "__main__":
    main()
