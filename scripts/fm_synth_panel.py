# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Writes devices/fm-synth/panel.json: the device row and the expanded panel (docs/spec/fm-synth.md).

Colours are only the theme's palette and the --device-* tokens: a theme changes the panel's styles
and nothing else (ADR 0034), and this script fails when a drawing has any other colour. Algorithm
pictures come from the engine's table in devices/fm-synth/src/core.rs.
Run from the repository root: python scripts/fm_synth_panel.py
"""

import json
import math
import re
from pathlib import Path

from multiband_panel import (ACCENT, BORDER, CONTROL, DISPLAY, HIGHLIGHT, INVISIBLE, PANEL, SPECTRUM_COLOR, TEXT, add, button,
                             circle, cos, equal, graphic, knob, line, maximum, minimum, mul, par, path, rect, separator, sin, sub,
                             text, tint, value_text)

MODULATOR = [3, 0, 4, 255]
CARRIER = [3, 0, 3, 255]
CAPTION = tint(TEXT, 170)
QUIET = tint(TEXT, 110)
GRID = tint(TEXT, 30)
FILL = tint(TEXT, 28)
OPS = range(1, 7)
COMPACT = (1180.0, 168.0)
EXPANDED = (1180.0, 640.0)
PAGES = ["Env", "Wave", "Filter", "LFO", "Pitch", "Mod", "Voice"]
# The index of the Drawn wave in devices/fm-synth/src/params.rs.
DRAWN_WAVE = 22
EXPANDED_PAGES = ["Operator", "Filter", "LFO", "Pitch", "Matrix", "Algorithm", "Random"]
SECTIONS = [("envelopes", "Envelopes"), ("levels", "Levels"), ("waves", "Waves"), ("filter", "Filter"), ("lfo", "LFO"),
            ("pitchEnv", "Pitch envelope"), ("modulation", "Modulation"), ("stereo", "Stereo"), ("ratios", "Ratios"),
            ("algorithm", "Algorithm")]
MATRIX = [("velocity", "Velocity"), ("key", "Key"), ("pressure", "Pressure"), ("bend", "Pitch bend"), ("wheel", "Mod wheel"),
          ("foot", "Foot"), ("breath", "Breath"), ("lfo", "LFO"), ("random", "Random")]
ENV_FIELDS = ["attack", "decay", "release", "initial", "peak", "sustain", "end", "loop", "loopTime", "repeat", "timeByVel",
              "timeByKey", "attackSlope", "decaySlope", "releaseSlope"]
OP_DX = ["rate1", "rate2", "rate3", "rate4", "level1", "level2", "level3", "level4", "breakPoint", "leftDepth", "rightDepth",
         "leftCurve", "rightCurve", "rateScaling", "ampModSens", "velocitySens", "level", "mode", "coarse", "fine", "detune",
         "on"]
OP_EXT = ["wave", "feedback", "phase", "retrigger", "freqByVel", "freqQuantize", "levelByKey", "envMode", "partials", "repeat",
          "normalize", "oddEven"]
DX_GLOBAL = ["pitchRate1", "pitchRate2", "pitchRate3", "pitchRate4", "pitchLevel1", "pitchLevel2", "pitchLevel3", "pitchLevel4",
             "algorithm", "feedback", "oscKeySync", "lfoSpeed", "lfoDelay", "lfoPitchDepth", "lfoAmpDepth", "lfoKeySync",
             "lfoWave", "pitchModSens", "transpose"]
PERFORMANCE = ["pitchBendRange", "pitchBendStep", "portamento", "glissando", "portamentoTime"]
ROUTES = [f"{s}{f}" for s in ("wheel", "foot", "breath", "pressure") for f in ("Range", "Pitch", "Amp", "EgBias")]
OUTPUT = ["volume", "pan", "panByKey", "panRandom", "spread", "voices", "tone", "time", "retrigger", "antialias", "interpolation"]
FILTER = ["filterOn", "filterType", "filterSlope", "filterCircuit", "filterFreq", "filterRes", "filterMorph", "filterDrive",
          "filterEnv", "filterByVel", "filterByKey", "shaper", "shaperDrive", "shaperMix"]
LFO = ["lfoOn", "lfo2Wave", "lfo2Range", "lfo2Rate", "lfo2Sync", "lfo2Retrigger", "lfo2ByKey", "lfo2ByVel", "lfo2Amount"] + [
    f"lfo2ToOp{n}" for n in OPS] + ["lfo2ToFilter", "lfo2DestB", "lfo2AmountB"]
PITCH = ["pitchEnvOn", "pitchEnvAmount"] + [f"pitchEnvToOp{n}" for n in OPS] + ["pitchEnvToLfo", "pitchEnvDestB", "pitchEnvAmountB"]
PANEL_PARAMS = ["panel.op", "panel.page", "panel.expandedPage", "random.amount", "random.seed"] + [
    f"random.{s}.{f}" for s, _ in SECTIONS for f in ("amount", "on")]
# Where an operator row's columns fall, as (x, width), on the device row and in the expanded panel.
ROW_COMPACT = {"select": (2.0, 20.0), "env": (24.0, 72.0), "mode": (99.0, 34.0), "freq": (135.0, 40.0), "coarse": (177.0, 32.0),
               "fine": (211.0, 32.0), "level": (245.0, 42.0), "on": (289.0, 31.0)}
ROW_EXPANDED = {"select": (4.0, 24.0), "env": (30.0, 80.0), "mode": (112.0, 36.0), "freq": (150.0, 46.0), "coarse": (198.0, 36.0),
                "fine": (236.0, 36.0), "level": (274.0, 42.0), "on": (318.0, 30.0)}
# Where the stages of an envelope's picture fall, as shares of its width: attack and decay each get
# up to one stretch by their time, on a logarithmic scale; release starts at a fixed place. Every
# stage has at least a gap, so two points never lie on top of each other.
TIME_STRETCH = 0.24
RELEASE_AT = 0.72
RELEASE_STRETCH = 0.28
STAGE_GAP = 7.0
ROW_GAP = 2.0
# The filter's picture: 20 Hz to 20 kHz across, as hearing spreads pitch, and decibels up and down.
FILTER_TOP_DB, FILTER_BOTTOM_DB = 30.0, -60.0
DB_PER_LOG = 10.0 / math.log(10.0)
# The second LFO's waves (params.rs) as the SDK's preview shapes (dsp::lfo::Shape::ALL).
LFO_SHAPES = [0, 4, 1, 3, 2, 5, 5]
NOISE_WAVE = 6
SYNC_RANGE = 2
LFO_RANGES = {0: (0.02, 30.0), 1: (8.0, 12_000.0)}


def every_parameter():
    """Every parameter id, so the panel shows only its canvases."""
    ids = [f"op{n}.{f}" for n in OPS for f in OP_DX] + DX_GLOBAL + PERFORMANCE + ROUTES + OUTPUT + FILTER
    ids += [f"filterEnv.{f}" for f in ENV_FIELDS] + LFO + [f"lfoEnv.{f}" for f in ENV_FIELDS] + PITCH
    ids += [f"pitchEnv.{f}" for f in ENV_FIELDS]
    ids += [f"mod.{s}.{f}" for s, _ in MATRIX for f in ("dest1", "amount1", "dest2", "amount2")]
    ids += [f"op{n}.{f}" for n in OPS for f in OP_EXT] + [f"op{n}.env.{f}" for n in OPS for f in ENV_FIELDS]
    ids += [f"op{n}.h{h}" for n in OPS for h in range(1, 65)] + PANEL_PARAMS
    return ids


def algorithms():
    """The engine's table: per algorithm, per operator from 6 to 1, its flags."""
    source = Path("devices/fm-synth/src/core.rs").read_text(encoding="utf-8")
    table = source[source.index("const ALGORITHMS"):source.index("\n];", source.index("const ALGORITHMS"))]
    rows = re.findall(r"\[(0x[0-9a-f]{2}(?:, 0x[0-9a-f]{2}){5})\]", table)
    assert len(rows) == 43, len(rows)
    return [[int(v, 16) for v in row.split(", ")] for row in rows]


TABLES = algorithms()


def routing(flags):
    """Who modulates whom, walked as the engine's render does: edges and the operators heard."""
    buses, edges, carriers = {1: [], 2: []}, [], []
    for i, f in enumerate(flags):
        number, in_bus, out_bus, adds = 6 - i, (f >> 4) & 3, f & 3, bool(f & 4)
        if in_bus:
            edges += [(s, number) for s in buses[in_bus]]
        if out_bus == 0:
            carriers.append(number)
        elif adds:
            buses[out_bus].append(number)
        else:
            buses[out_bus] = [number]
    return edges, carriers


def layout(flags, w, h, size):
    """Where an algorithm's operators lie in its picture: modulators above what they modulate."""
    edges, _ = routing(flags)
    depth = {n: 0 for n in OPS}
    for _ in OPS:
        for s, d in edges:
            depth[s] = max(depth[s], depth[d] + 1)
    layers = {}
    for n in OPS:
        layers.setdefault(depth[n], []).append(n)
    top = max(layers)
    position = {}
    for d, row in layers.items():
        y = (h - size) / 2.0 if top == 0 else h - size - (d * (h - size)) / top
        for i, n in enumerate(row):
            position[n] = (4.0 + (i + 0.5) * w / len(row) - size / 2.0, 4.0 + y)
    return position


def picture(flags, w, h, size, ring=False):
    """The algorithm as a drawing: wires, modulators outlined, operators heard filled, a ring around the selected one."""
    edges, carriers = routing(flags)
    position = layout(flags, w, h, size)
    draws = []
    for s, d in edges:
        a, b = position[s], position[d]
        draws.append(line([a[0] + size / 2.0, a[1] + size], [b[0] + size / 2.0, b[1]], tint(TEXT, 120), 1.0))
    for n, (x, y) in position.items():
        heard_now = n in carriers
        color = CARRIER if heard_now else MODULATOR
        if ring:
            on = selected(n)
            draws.append(rect(sub(x, mul(on, 3.0)), sub(y, mul(on, 3.0)), mul(on, size + 6.0), mul(on, size + 6.0), HIGHLIGHT, 3.0))
        draws.append(rect(x, y, size, size, color, 2.0))
        if not heard_now:
            draws.append(rect(x + 1.0, y + 1.0, size - 2.0, size - 2.0, DISPLAY, 1.0))
        if size >= 12.0:
            draws.append(text(x + size / 2.0, y + 1.0, str(n), PANEL if heard_now else color, max(size - 5.0, 8.0), center=True))
    return draws


def selected(n):
    return equal(par("panel.op"), float(n))


def both(a, b):
    if a is None:
        return b
    if b is None:
        return a
    return mul(a, b)


def balanced_sum(terms):
    """The sum of `terms` as a shallow tree, as an expression's depth is limited."""
    if not terms:
        return 0.0
    if len(terms) == 1:
        return terms[0]
    half = len(terms) // 2
    return add(balanced_sum(terms[:half]), balanced_sum(terms[half:]))


def one_of(pid, values):
    """1 while `pid` holds one of `values`."""
    return balanced_sum([equal(par(pid), float(v)) for v in values])


def is_off(pid):
    return equal(par(pid), 0.0)


def is_on(pid):
    return equal(par(pid), 1.0)


def heard(n):
    """1 while operator `n` is heard directly under the current algorithm."""
    return one_of("algorithm", [i + 1 for i, flags in enumerate(TABLES) if n in routing(flags)[1]])


def modulates(n):
    return sub(1.0, heard(n))


def percent(pid):
    return mul(par(pid), 100.0)


def ms(pid):
    return mul(par(pid), 1000.0)


def binding(pid, log=False, flip=False):
    return {"parameter": pid, "logarithmic": log, "drag_scale": -1.0 if flip else 1.0} if pid else None


def written(pid, x, y, value, suffix, decimals, size=11.0):
    """A value as text: a time in ms or s and a frequency in Hz or kHz come from the parameter itself."""
    formats = {" ms": "seconds", " Hz": "frequency"}
    if suffix in formats and isinstance(value, dict) and pid is not None:
        return text(x, y, "", TEXT, size, center=True, parameter=pid, fmt=formats[suffix])
    return value_text(x, y, value, suffix=suffix, size=size, decimals=decimals, center=True)


def field(pid, caption, bounds, value, suffix="", decimals=0, log=False, visible=None, tag="", cid=None, invert=False, fill="bar", plain=False):
    """A number field that is its own slider: dragging up or down changes `pid` and the fill shows where
    it is, from the left or, with `fill` "centre", from the middle. Tall enough, its name is over its
    value; wide enough, beside it. In a table (`plain`) the column has its name."""
    _, _, w, h = bounds
    drawing = [rect(0.0, 0.0, w, h, CONTROL, 3.0)]
    share = par(pid, True, log)
    if fill == "bar":
        drawing.append(rect(0.0, 0.0, mul(share, w), h, FILL, 3.0))
    elif fill == "centre":
        drawing.append(rect(minimum(w / 2.0, mul(share, w)), 0.0, mul(maximum(sub(share, 0.5), sub(0.5, share)), w), h, FILL))
    if plain or (h < 28.0 and w < 90.0):
        drawing.append(written(pid, w / 2.0, (h - 13.0) / 2.0, value, suffix, decimals))
    elif h >= 28.0:
        drawing += [text(w / 2.0, 3.0, caption, CAPTION, 10.0, center=True), written(pid, w / 2.0, h - 15.0, value, suffix, decimals)]
    else:
        drawing += [text(6.0, (h - 12.0) / 2.0, caption, CAPTION, 10.0), written(pid, w * 0.72, (h - 13.0) / 2.0, value, suffix, decimals)]
    return {"visible_when": visible, "id": cid or f"{pid}{tag}", "name": caption,
            "tooltip": f"{caption}: drag up or down; Shift for precision; double click resets", "bounds": bounds, "gesture": "drag",
            "x": None, "y": {"parameter": pid, "logarithmic": log, "drag_scale": -1.0 if invert else 1.0}, "drawing": drawing}


def menu(pid, caption, bounds, visible=None, tag=""):
    return {"widget": {"kind": "menu"}, "visible_when": visible, "id": f"{pid}{tag}", "name": caption, "tooltip": caption,
            "bounds": bounds, "gesture": "menu", "x": {"parameter": pid, "logarithmic": False, "drag_scale": 1.0}, "y": None,
            "drawing": []}


def named_menu(pid, caption, bounds, visible=None, tag=""):
    """A menu with its name over it, as a menu shows only its value."""
    x, y, w, h = bounds
    return [graphic(f"{pid}{tag}_name", caption, [x, y, w, 13.0], [text(w / 2.0, 0.0, caption, CAPTION, 10.0, center=True)], visible),
            menu(pid, caption, [x, y + 13.0, w, h - 13.0], visible, tag)]


def label_beside(cid, caption, x, y, w, h, visible=None):
    return graphic(cid, caption, [x, y, w, h], [text(0.0, (h - 12.0) / 2.0, caption, CAPTION, 10.0)], visible)


def toggle(pid, caption, bounds, visible=None, tag="", off_label=None):
    return button(f"{pid}{tag}", pid, caption, bounds, visible=visible, off_label=off_label)


def switch(pid, name, bounds, visible=None, tag=""):
    """A section's power: it reads On or Off, so its state is in its words as well as its fill."""
    control = button(f"{pid}{tag}", pid, "On", bounds, visible=visible, off_label="Off")
    control["name"] = f"{name} on"
    control["tooltip"] = f"Turn the {name.lower()} on or off"
    return control


def choose(cid, pid, caption, value, bounds, visible=None):
    """A button that sets `pid` to `value`, on while it holds it."""
    return button(cid, pid, caption, bounds, gesture={"set": value}, visible=visible)


def random_button(cid, caption, bounds, visible=None):
    return {"widget": {"kind": "button", "label": caption, "off_label": None, "indicator": False}, "visible_when": visible,
            "id": cid, "name": caption, "tooltip": "Give the sound new values, by the Random page's shares",
            "bounds": bounds, "gesture": "randomize", "x": None, "y": None, "drawing": []}


def more(cid, page, bounds, visible=None, caption="More…"):
    control = button(cid, "panel.expandedPage", caption, bounds, gesture={"expand": page}, visible=visible)
    control["name"] = f"{caption} {EXPANDED_PAGES[page]}"
    control["tooltip"] = f"Open the expanded panel's {EXPANDED_PAGES[page]} page"
    return control


def tabs(pid, names, x, y, w, h, tag, visible=None):
    return [choose(f"{tag}_{i}", pid, name, i, [x + i * w, y, w - 2.0, h], visible) for i, name in enumerate(names)]


def tab_marks(x, y, w, tag, pages):
    """A dot on the tab of a section that is on, so what plays shows from any page."""
    return [graphic(f"{tag}_mark_{page}", f"{name} is on", [x + (page + 1) * w - 12.0, y + 2.0, 8.0, 8.0], [circle([4.0, 4.0], 2.5, HIGHLIGHT)],
                    is_on(pid)) for page, name, pid in pages]


def section(cid, caption, x, y, w, visible=None):
    """A section's name with a line under it: groups come from a step of lightness, not a box."""
    return graphic(cid, caption, [x, y, w, 16.0], [text(0.0, 0.0, caption, tint(TEXT, 215), 11.0), line([0.0, 15.0], [w, 15.0], BORDER)], visible)


def dim(cid, bounds, when):
    """Quietens what lies under it while `when`: it still answers, but it does nothing right now."""
    _, _, w, h = bounds
    return graphic(cid, "Inactive", bounds, [rect(0.0, 0.0, w, h, tint(PANEL, 165))], when)


def point(cid, name, bounds, x_pid, y_pid, visible, log_x=False, flip_x=False, offset=None, span=None):
    """A dot on a picture, dragged along the axes it has parameters for."""
    return {"widget": {"kind": "handle", "color": HIGHLIGHT, "span": span, "offset": offset, "dot": True}, "visible_when": visible,
            "id": cid, "name": name, "tooltip": f"{name}: drag to move it; double click resets", "bounds": bounds,
            "gesture": "position", "x": binding(x_pid, log_x, flip_x), "y": binding(y_pid), "drawing": []}


def stage(share, span, gap):
    """How wide a stage is in a picture: at least `gap`, at most `span`."""
    return add(gap, mul(share, span - gap))


def env_x(prefix, w, gap=STAGE_GAP):
    """The ends of a time envelope's attack and decay and of its release, in its picture."""
    attack = stage(par(f"{prefix}attack", True, True), w * TIME_STRETCH, gap)
    decay = add(attack, stage(par(f"{prefix}decay", True, True), w * TIME_STRETCH, gap))
    release = add(w * RELEASE_AT, stage(par(f"{prefix}release", True, True), w * RELEASE_STRETCH, gap))
    return attack, decay, release


def env_shape(prefix, w, h, top, bottom, color, gap=STAGE_GAP):
    """A time envelope's shape, moving with its times and levels; `top` and `bottom` are its range."""
    span = top - bottom

    def y(pid):
        return add(mul(sub(top, minimum(top, maximum(bottom, par(pid)))), (h - 8.0) / span), 4.0)
    attack, decay, release = env_x(prefix, w, gap)
    points = [[0.0, y(f"{prefix}initial")], [attack, y(f"{prefix}peak")], [decay, y(f"{prefix}sustain")],
              [w * RELEASE_AT, y(f"{prefix}sustain")], [release, y(f"{prefix}end")], [w, y(f"{prefix}end")]]
    return path(points, color, 1.5)


def env_grid(w, h, top, bottom, levels):
    """Lines at `levels` of an envelope's range, and where the key goes up."""
    span = top - bottom
    draws = []
    for level in levels:
        y = 4.0 + (top - level) * (h - 8.0) / span
        draws.append(line([0.0, y], [w, y], GRID))
    draws.append(line([w * RELEASE_AT, 0.0], [w * RELEASE_AT, h], GRID))
    if h >= 90.0:
        draws.append(text(w * RELEASE_AT - 34.0, h - 13.0, "Key up", QUIET, 9.0))
    return draws


def env_picture(prefix, w, h, top, bottom, color, levels):
    return [rect(0.0, 0.0, w, h, DISPLAY, 3.0)] + env_grid(w, h, top, bottom, levels) + [env_shape(prefix, w, h, top, bottom, color)]


def env_points(prefix, tag, x, y, w, h, visible):
    """What is dragged on a time envelope's picture: each stage's end, in time and level at once."""
    attack, _, _ = env_x(prefix, w)
    zone = w * TIME_STRETCH - STAGE_GAP
    top, tall = y + 4.0, h - 8.0
    return [
        point(f"{prefix}initial_point{tag}", "Initial level", [x, top, w, tall], None, f"{prefix}initial", visible, span=[x - 6.0, x + 6.0]),
        point(f"{prefix}attack_point{tag}", "Attack and peak", [x + STAGE_GAP, top, zone, tall], f"{prefix}attack", f"{prefix}peak", visible, log_x=True),
        point(f"{prefix}decay_point{tag}", "Decay and sustain", [x + STAGE_GAP, top, zone, tall], f"{prefix}decay", f"{prefix}sustain", visible, log_x=True,
              offset=attack),
        point(f"{prefix}release_point{tag}", "Release and end level", [x + w * RELEASE_AT + STAGE_GAP, top, w * RELEASE_STRETCH - STAGE_GAP, tall],
              f"{prefix}release", f"{prefix}end", visible, log_x=True),
    ]


def rate_share(pid):
    """A rate as a stage's width: a lower rate is a longer stage."""
    return mul(sub(99.0, par(pid)), 1.0 / 99.0)


def dx_x(n, w, gap=STAGE_GAP):
    """Where a rates-and-levels envelope's first three stages end in its picture."""
    first = stage(rate_share(f"op{n}.rate1"), w * TIME_STRETCH, gap)
    second = add(first, stage(rate_share(f"op{n}.rate2"), w * TIME_STRETCH, gap))
    return first, second, add(second, stage(rate_share(f"op{n}.rate3"), w * TIME_STRETCH, gap))


def dx_shape(n, w, h, color, gap=STAGE_GAP):
    """A rates-and-levels envelope's shape, moving with its rates and levels."""
    def y(field_name):
        return add(mul(sub(99.0, par(f"op{n}.{field_name}")), (h - 8.0) / 99.0), 4.0)
    first, second, third = dx_x(n, w, gap)
    end = add(w * RELEASE_AT, stage(rate_share(f"op{n}.rate4"), w * RELEASE_STRETCH, gap))
    points = [[0.0, y("level4")], [first, y("level1")], [second, y("level2")], [third, y("level3")],
              [w * RELEASE_AT, y("level3")], [end, y("level4")], [w, y("level4")]]
    return path(points, color, 1.5)


def dx_picture(n, w, h, color):
    return [rect(0.0, 0.0, w, h, DISPLAY, 3.0)] + env_grid(w, h, 99.0, 0.0, [50.0]) + [dx_shape(n, w, h, color)]


def dx_points(n, tag, x, y, w, h, visible):
    """A rates-and-levels envelope's stage ends: further right is a lower rate, higher is a higher level."""
    first, second, _ = dx_x(n, w)
    zone = w * TIME_STRETCH - STAGE_GAP
    top, tall = y + 4.0, h - 8.0
    area = [x + STAGE_GAP, top, zone, tall]
    return [
        point(f"op{n}.stage1_point{tag}", "Rate 1 and level 1", area, f"op{n}.rate1", f"op{n}.level1", visible, flip_x=True),
        point(f"op{n}.stage2_point{tag}", "Rate 2 and level 2", area, f"op{n}.rate2", f"op{n}.level2", visible, flip_x=True, offset=first),
        point(f"op{n}.stage3_point{tag}", "Rate 3 and level 3", area, f"op{n}.rate3", f"op{n}.level3", visible, flip_x=True, offset=second),
        point(f"op{n}.stage4_point{tag}", "Rate 4 and level 4", [x + w * RELEASE_AT + STAGE_GAP, top, w * RELEASE_STRETCH - STAGE_GAP, tall],
              f"op{n}.rate4", f"op{n}.level4", visible, flip_x=True),
    ]


def op_envelope(n, tag, x, y, w, h, shown):
    """The selected operator's envelope on a display, in whichever model it uses."""
    time = both(shown, is_on(f"op{n}.envMode"))
    rates = both(shown, is_off(f"op{n}.envMode"))
    return [graphic(f"display_time_{n}{tag}", f"Operator {n} envelope", [x, y, w, h],
                    env_picture(f"op{n}.env.", w, h, 0.0, -70.0, ACCENT, [-20.0, -40.0, -60.0]), time),
            graphic(f"display_rates_{n}{tag}", f"Operator {n} envelope", [x, y, w, h], dx_picture(n, w, h, ACCENT), rates)
            ] + env_points(f"op{n}.env.", tag, x, y, w, h, time) + dx_points(n, tag, x, y, w, h, rates)


def r_power(power):
    """(frequency / cutoff) to `power` at a plot's place on a 20 Hz to 20 kHz logarithmic axis."""
    return {"op": "exp", "value": add(add(power * math.log(20.0), mul(par("#x"), power * math.log(1000.0))),
                                      mul({"op": "log", "value": par("filterFreq")}, -power))}


def filter_damping():
    """1 / Q squared, with the device's Q from resonance (devices/fm-synth/src/ext.rs)."""
    q = add(0.5, mul(24.0, mul(par("filterRes"), par("filterRes"))))
    return {"op": "exp", "value": mul({"op": "log", "value": q}, -2.0)}


def filter_numerators():
    """Each response's squared numerator over the two-pole denominator, as an analog prototype, with
    when it shows. Morph passes through low-pass, band-pass, high-pass, notch and back to low-pass."""
    r2, r4, k2 = r_power(2.0), r_power(4.0), filter_damping()
    of_type = lambda i: equal(par("filterType"), float(i))
    responses = [(None, of_type(0)), (r4, of_type(1)), (mul(k2, r2), of_type(2)), (add(add(1.0, mul(r2, -2.0)), r4), of_type(3))]

    def stretch(i):
        """Morph's way through its `i`th stretch, 0 to 1, and whether it is there."""
        at = mul(par("filterMorph"), 4.0)
        t = add(at, -float(i))
        rest = sub(1.0, t)
        beyond = maximum(0.0, minimum(1.0, add(mul(t, 10000.0), 1.0)))
        inside = beyond if i == 3 else mul(beyond, maximum(0.0, minimum(1.0, mul(rest, 10000.0))))
        return t, rest, both(of_type(4), inside)
    t, rest, shown = stretch(0)
    responses.append((add(mul(rest, rest), mul(mul(k2, r2), mul(t, t))), shown))
    t, rest, shown = stretch(1)
    responses.append((add(mul(mul(k2, r2), mul(rest, rest)), mul(mul(t, t), r4)), shown))
    t, rest, shown = stretch(2)
    responses.append((add(add(mul(t, t), mul(mul(r2, t), -2.0)), r4), shown))
    t, rest, shown = stretch(3)
    responses.append((add(add(1.0, mul(mul(r2, rest), -2.0)), mul(mul(rest, rest), r4)), shown))
    return responses


def filter_curve(numerator, w, h, color):
    """A filter's response across its picture: what the ear hears of it, in decibels over octaves."""
    r2, r4, k2 = r_power(2.0), r_power(4.0), filter_damping()
    denominator = add(add(1.0, mul(r2, -2.0)), add(mul(r2, k2), r4))
    gone = {"op": "log", "value": denominator}
    if numerator is not None:
        gone = add(gone, mul({"op": "log", "value": add(numerator, 0.000001)}, -1.0))
    per_db = h / (FILTER_TOP_DB - FILTER_BOTTOM_DB)
    slope = mul(per_db * DB_PER_LOG, add(1.0, par("filterSlope")))
    y = add(FILTER_TOP_DB * per_db, mul(gone, slope))
    return {"kind": "plot", "from": 0.0, "to": w, "top": 0.0, "bottom": h, "samples": 72, "y": y, "color": color, "width": 1.5}


def filter_x(hz, w):
    return (math.log(hz) - math.log(20.0)) * w / math.log(1000.0)


def filter_picture(x, y, w, h, tag, visible=None):
    """The filter's response with its frequency and resonance on one dot, on a log scale of hertz."""
    zero = (FILTER_TOP_DB) * h / (FILTER_TOP_DB - FILTER_BOTTOM_DB)
    ground = [rect(0.0, 0.0, w, h, DISPLAY, 3.0), line([0.0, zero], [w, zero], GRID)]
    for hz, name in ((100.0, "100"), (1000.0, "1k"), (10000.0, "10k")):
        at = filter_x(hz, w)
        ground += [line([at, 0.0], [at, h], GRID), text(at + 3.0, h - 12.0, name, QUIET, 9.0)]
    controls = [graphic(f"filter_ground{tag}", "Filter response", [x, y, w, h], ground, visible)]
    for i, (numerator, kind) in enumerate(filter_numerators()):
        shown = both(visible, kind)
        controls.append(graphic(f"filter_curve_{i}{tag}", "Filter response", [x, y, w, h], [filter_curve(numerator, w, h, ACCENT)],
                                both(shown, is_on("filterOn"))))
        controls.append(graphic(f"filter_curve_off_{i}{tag}", "Filter response", [x, y, w, h], [filter_curve(numerator, w, h, QUIET)],
                                both(shown, is_off("filterOn"))))
    controls.append(graphic(f"filter_off{tag}", "The filter is off", [x, y, 90.0, 16.0], [text(6.0, 2.0, "The filter is off", QUIET, 10.0)],
                            both(visible, is_off("filterOn"))))
    controls.append(point(f"filter_point{tag}", "Frequency and resonance", [x, y, w, zero], "filterFreq", "filterRes", visible, log_x=True))
    return controls


def lfo_rate_hz(index):
    low, high = LFO_RANGES[index]
    return {"op": "exp", "value": add(math.log(low), mul(par("lfo2Rate"), math.log(high / low)))}


def lfo_curve(w, h, amplitude, color):
    shape = balanced_sum([mul(equal(par("lfo2Wave"), float(i)), float(s)) for i, s in enumerate(LFO_SHAPES) if s])
    cycles = mul(par("#x"), add(2.0, mul(equal(par("lfo2Wave"), float(NOISE_WAVE)), 14.0)))
    y = sub(h / 2.0, mul(amplitude, {"op": "wave", "shape": shape, "cycles": cycles}))
    return {"kind": "plot", "from": 0.0, "to": w, "top": 0.0, "bottom": h, "samples": 96, "y": y, "color": color, "width": 1.5}


def lfo_picture(x, y, w, h, tag, visible=None):
    """Two cycles of the second LFO: its wave quietly, and in the accent how much of it reaches the sound."""
    reach = h / 2.0 - 5.0
    draws = [rect(0.0, 0.0, w, h, DISPLAY, 3.0), line([0.0, h / 2.0], [w, h / 2.0], GRID), lfo_curve(w, h, reach, QUIET),
             lfo_curve(w, h, mul(par("lfo2Amount"), reach), SPECTRUM_COLOR)]
    return [graphic(f"lfo_picture{tag}", "LFO wave", [x, y, w, h], draws, visible)]


def lfo_rate_knob(x, y, w, h, visible, tag):
    """The rate in hertz on the free ranges, a note length on the synced one."""
    bounds = [x, y, w, h]
    return [knob("lfo2Rate", "Rate", bounds, lfo_rate_hz(0), " Hz", 2, visible=both(visible, equal(par("lfo2Range"), 0.0)), tag=f"{tag}_low"),
            knob("lfo2Rate", "Rate", bounds, lfo_rate_hz(1), " Hz", 0, visible=both(visible, equal(par("lfo2Range"), 1.0)), tag=f"{tag}_high")
            ] + named_menu("lfo2Sync", "Rate", [x + 2.0, y + 8.0, w - 4.0, 38.0], both(visible, equal(par("lfo2Range"), float(SYNC_RANGE))), tag)


def partial_bars(n, x, y, w, h, tag, visible):
    """One operator's partials as bars, as many as it edits: a press and a drag across them draws,
    and drawing plays the Drawn wave."""
    p = f"op{n}."
    tooltip = "Drag across the bars to draw the wave; drawing switches the operator to the Drawn wave"
    controls = []
    for choice, count in enumerate((16, 32, 64)):
        shown = both(visible, equal(par(f"{p}partials"), float(choice)))
        step = w / count
        width = max(step, 8.0)
        for i in range(count):
            pid = f"{p}h{i + 1}"
            at = min(x + i * step, x + w - width)
            value = mul(par(pid), h)
            controls.append({"visible_when": shown, "id": f"{pid}_{count}{tag}", "name": f"Partial {i + 1}", "tooltip": tooltip,
                             "bounds": [at, y, width, h], "gesture": {"draw": DRAWN_WAVE},
                             "x": {"parameter": f"{p}wave", "logarithmic": False, "drag_scale": 1.0},
                             "y": {"parameter": pid, "logarithmic": False, "drag_scale": 1.0},
                             "drawing": [rect(0.0, sub(h, value), max(step - 1.0, 1.0), value, ACCENT)]})
    return controls


def bars_note(cid, x, y, w, h, n, visible):
    """While another wave plays, the bars are quiet and say what they do."""
    return graphic(cid, "Partials unused", [x, y, w, h], [rect(0.0, 0.0, w, h, tint(DISPLAY, 175)),
                                                          text(w / 2.0, h / 2.0 - 7.0, "Draw here to play your own wave", CAPTION, 10.0, center=True)],
                   both(visible, sub(1.0, equal(par(f"op{n}.wave"), float(DRAWN_WAVE)))))


def selector(cid, n, bounds, visible=None, drawing=None, name=None):
    """A place that selects operator `n` when it is clicked."""
    return {"visible_when": visible, "id": cid, "name": name or f"Select operator {n}", "tooltip": f"Select operator {n}", "bounds": bounds,
            "gesture": {"set": n}, "x": {"parameter": "panel.op", "logarithmic": False, "drag_scale": 1.0}, "y": None,
            "drawing": drawing or []}


def op_frequency(n, bounds, tag):
    """What an operator's coarse and fine come to: the ratio to the note, or its fixed frequency."""
    x, y, w, h = bounds
    coarse, fine = par(f"op{n}.coarse"), par(f"op{n}.fine")
    ratio = mul(maximum(0.5, coarse), add(1.0, mul(fine, 0.01)))
    turn = mul(coarse, math.pi / 2.0)
    decade = sub(sub(sub(1.5, cos(turn)), sin(turn)), mul(0.5, cos(mul(coarse, math.pi))))
    hertz = {"op": "exp", "value": mul(add(decade, mul(fine, 0.01)), math.log(10.0))}
    low = sub(1.0, maximum(0.0, minimum(1.0, sub(decade, 1.0))))
    fixed = is_on(f"op{n}.mode")
    def readout(cid, value, suffix, decimals, visible, prefix=""):
        return graphic(f"op{n}_freq_{cid}{tag}", f"Operator {n} frequency", [x, y, w, h],
                       [value_text(w / 2.0, (h - 13.0) / 2.0, value, prefix=prefix, suffix=suffix, size=11.0, decimals=decimals, center=True)], visible)
    return [readout("ratio", ratio, "", 2, is_off(f"op{n}.mode"), prefix="×"),
            readout("low", hertz, " Hz", 1, both(fixed, low)), readout("high", hertz, " Hz", 0, both(fixed, sub(1.0, low)))]


def op_toggle(n, bounds, tag):
    """An operator's power: a small light, as six bright buttons in a column would outshout the sound's shape."""
    _, _, w, h = bounds
    cx, cy = w / 2.0, h / 2.0
    drawing = [rect(0.0, 0.0, w, h, CONTROL, 3.0), circle([cx, cy], 5.0, BORDER), circle([cx, cy], mul(par(f"op{n}.on"), 4.0), ACCENT)]
    return {"visible_when": None, "id": f"op{n}.on{tag}", "name": f"Operator {n} on", "tooltip": f"Turn operator {n} on or off", "bounds": bounds,
            "gesture": "toggle", "x": binding(f"op{n}.on"), "y": None, "drawing": drawing}


def op_row(n, columns, y, h, tag, picture_h):
    """One operator's row: its number, envelope picture, ratio or Hz with what it comes to, coarse, fine, level, on.
    The row is lighter while selected, its envelope is coloured as what it does, and it dims while off."""
    cell = lambda name: [columns[name][0], y + 1.5, columns[name][1], h - 3.0]
    x, width = columns["select"][0], columns["on"][0] + columns["on"][1] - columns["select"][0]
    env_x0, env_w = columns["env"]
    gap = ROW_GAP
    time, rates = is_on(f"op{n}.envMode"), is_off(f"op{n}.envMode")
    controls = [
        graphic(f"op{n}_band{tag}", f"Operator {n} selected", [x - 1.0, y, width + 2.0, h], [rect(0.0, 0.0, width + 2.0, h, tint(TEXT, 22), 3.0)], selected(n)),
        {"widget": {"kind": "button", "label": str(n), "off_label": None, "indicator": False}, "visible_when": None,
         "id": f"select_op{n}{tag}", "name": f"Select operator {n}", "tooltip": f"Select operator {n}", "bounds": cell("select"), "gesture": {"set": n},
         "x": {"parameter": "panel.op", "logarithmic": False, "drag_scale": 1.0}, "y": None, "drawing": []},
    ]
    bounds = cell("env")
    w, ph = bounds[2], bounds[3]
    ground = rect(0.0, 0.0, w, ph, DISPLAY, 3.0)
    for name, model, shape in (("time", time, lambda color: env_shape(f"op{n}.env.", w, ph, 0.0, -70.0, color, gap)),
                               ("rates", rates, lambda color: dx_shape(n, w, ph, color, gap))):
        controls.append(selector(f"op{n}_{name}_heard{tag}", n, bounds, both(model, heard(n)), [ground, shape(CARRIER)], f"Select operator {n}, heard"))
        controls.append(selector(f"op{n}_{name}_mod{tag}", n, bounds, both(model, modulates(n)), [ground, shape(MODULATOR)], f"Select operator {n}, modulating"))
    controls.append(button(f"op{n}.mode{tag}", f"op{n}.mode", "Hz", cell("mode"), off_label="Ratio"))
    controls += op_frequency(n, cell("freq"), tag)
    controls.append(field(f"op{n}.coarse", "Coarse", cell("coarse"), par(f"op{n}.coarse"), tag=tag, fill=None, plain=True))
    controls.append(field(f"op{n}.fine", "Fine", cell("fine"), par(f"op{n}.fine"), tag=tag, fill=None, plain=True))
    controls.append(field(f"op{n}.level", "Level", cell("level"), par(f"op{n}.level"), tag=tag, plain=True))
    controls.append(op_toggle(n, cell("on"), tag))
    controls.append(dim(f"op{n}_off{tag}", [env_x0, y + 1.5, columns["level"][0] + columns["level"][1] - env_x0, h - 3.0], is_off(f"op{n}.on")))
    return controls


def op_table(columns, y, row_h, tag):
    """The operators as a table: its columns named once, the rows only values."""
    names = [("env", "Envelope"), ("freq", "Freq"), ("coarse", "Coarse"), ("fine", "Fine"), ("level", "Level")]
    x0 = columns["select"][0]
    draws = [text(columns[c][0] - x0 + columns[c][1] / 2.0, 0.0, name, CAPTION, 10.0, center=True) for c, name in names]
    width = columns["on"][0] + columns["on"][1] - x0
    controls = [graphic(f"op_heads{tag}", "Operator columns", [x0, y, width, 14.0], draws)]
    for n in OPS:
        controls += op_row(n, columns, y + 15.0 + (n - 1) * row_h, row_h, tag, row_h - 3.0)
    return controls


def algorithm_pictures(x, y, w, h, tag, size):
    """The chosen algorithm's picture, whose operators select when clicked."""
    controls = []
    for i, flags in enumerate(TABLES):
        shown = equal(par("algorithm"), float(i + 1))
        controls.append(graphic(f"algorithm_picture_{i + 1}{tag}", f"Algorithm {i + 1}", [x, y, w, h],
                                [rect(0.0, 0.0, w, h, DISPLAY, 3.0)] + picture(flags, w - 8.0, h - 8.0, size, ring=True), shown))
        for n, (ox, oy) in layout(flags, w - 8.0, h - 8.0, size).items():
            controls.append(selector(f"pick_op{n}_alg{i + 1}{tag}", n, [x + ox - 2.0, y + oy - 2.0, size + 4.0, size + 4.0], shown))
    return controls


def algorithm_legend(cid, x, y, visible=None):
    """What a picture's squares mean."""
    draws = [rect(0.0, 2.0, 9.0, 9.0, CARRIER, 2.0), text(13.0, 0.0, "heard", CAPTION, 10.0), rect(52.0, 2.0, 9.0, 9.0, MODULATOR, 2.0),
             rect(53.0, 3.0, 7.0, 7.0, DISPLAY, 1.0), text(65.0, 0.0, "shapes what it points to", CAPTION, 10.0)]
    return graphic(cid, "Filled is heard, outlined shapes what it points to", [x, y, 200.0, 14.0], draws, visible)


def dial(pid, caption, x, y, value, suffix="", decimals=0, log=False, visible=None, tag="", big=False, w=50.0):
    return knob(pid, caption, [x, y, w, 70.0], value, suffix, decimals, log, visible, tag, big,
                texts=lambda cx, at: [written(pid, cx, at, value, suffix, decimals)])


def op_dials(n, x, y, tag):
    """The Env and Wave pages' eight dials for operator `n`."""
    shown = selected(n)
    p = lambda f: f"op{n}.{f}"
    e = lambda f: f"op{n}.env.{f}"
    env_page, wave_page = equal(par("panel.page"), 0.0), equal(par("panel.page"), 1.0)
    time = both(env_page, both(shown, is_on(p("envMode"))))
    rates = both(env_page, both(shown, is_off(p("envMode"))))
    wave = both(wave_page, shown)
    cells = lambda i: (x + (i % 4) * 50.0, y + (i // 4) * 80.0)
    controls = []
    for i, (pid, caption, value, suffix, decimals, log) in enumerate((
            (e("attack"), "Attack", ms(e("attack")), " ms", 0, True),
            (e("decay"), "Decay", ms(e("decay")), " ms", 0, True),
            (e("sustain"), "Sustain", par(e("sustain")), " dB", 1, False),
            (e("release"), "Release", ms(e("release")), " ms", 0, True),
            (e("initial"), "Initial", par(e("initial")), " dB", 1, False),
            (e("peak"), "Peak", par(e("peak")), " dB", 1, False),
            (e("timeByVel"), "By vel", percent(e("timeByVel")), " %", 0, False),
            (e("timeByKey"), "By key", percent(e("timeByKey")), " %", 0, False))):
        cx, cy = cells(i)
        controls.append(dial(pid, caption, cx, cy, value, suffix, decimals, log, time, f"{tag}_time"))
    for i in range(8):
        pid = p(f"rate{i + 1}") if i < 4 else p(f"level{i - 3}")
        caption = f"Rate {i + 1}" if i < 4 else f"Level {i - 3}"
        cx, cy = cells(i)
        controls.append(dial(pid, caption, cx, cy, par(pid), visible=rates, tag=f"{tag}_rates"))
    for i, (pid, caption, value, suffix) in enumerate((
            (p("feedback"), "Feedback", percent(p("feedback")), " %"),
            (p("detune"), "Detune", par(p("detune")), ""),
            (p("freqByVel"), "Pitch vel", percent(p("freqByVel")), " %"),
            (p("phase"), "Phase", mul(par(p("phase")), 360.0), "°"),
            (p("velocitySens"), "Velocity", par(p("velocitySens")), ""),
            (p("levelByKey"), "Key level", percent(p("levelByKey")), " %"),
            (p("rateScaling"), "Key rate", par(p("rateScaling")), ""),
            (p("ampModSens"), "LFO level", par(p("ampModSens")), ""))):
        cx, cy = cells(i)
        controls.append(dial(pid, caption, cx, cy, value, suffix, visible=wave, tag=f"{tag}_wave"))
    return controls


def global_dials(x, y, tag):
    """The Filter, LFO, Pitch and Voice pages' dials."""
    pages = {
        2: [("filterFreq", "Freq", par("filterFreq"), " Hz", 0, True), ("filterRes", "Res", percent("filterRes"), " %", 0, False),
            ("filterDrive", "Drive", par("filterDrive"), " dB", 1, False), ("filterMorph", "Morph", percent("filterMorph"), " %", 0, False),
            ("filterEnv", "Env", percent("filterEnv"), " %", 0, False), ("filterByVel", "By vel", percent("filterByVel"), " %", 0, False),
            ("filterByKey", "By key", percent("filterByKey"), " %", 0, False), ("shaperMix", "Shaper", percent("shaperMix"), " %", 0, False)],
        3: [None, ("lfo2Amount", "Amount", percent("lfo2Amount"), " %", 0, False),
            ("lfo2ByKey", "By key", percent("lfo2ByKey"), " %", 0, False), ("lfo2ByVel", "By vel", percent("lfo2ByVel"), " %", 0, False),
            ("lfoEnv.attack", "Attack", ms("lfoEnv.attack"), " ms", 0, True), ("lfoEnv.decay", "Decay", ms("lfoEnv.decay"), " ms", 0, True),
            ("lfoEnv.sustain", "Sustain", par("lfoEnv.sustain"), "", 2, False), ("lfoEnv.release", "Release", ms("lfoEnv.release"), " ms", 0, True)],
        4: [("pitchEnv.initial", "Initial", par("pitchEnv.initial"), " st", 1, False), ("pitchEnv.attack", "Attack", ms("pitchEnv.attack"), " ms", 0, True),
            ("pitchEnv.peak", "Peak", par("pitchEnv.peak"), " st", 1, False), ("pitchEnv.decay", "Decay", ms("pitchEnv.decay"), " ms", 0, True),
            ("pitchEnv.sustain", "Sustain", par("pitchEnv.sustain"), " st", 1, False), ("pitchEnv.release", "Release", ms("pitchEnv.release"), " ms", 0, True),
            ("pitchEnv.end", "End", par("pitchEnv.end"), " st", 1, False), ("pitchEnvAmount", "Amount", percent("pitchEnvAmount"), " %", 0, False)],
        6: [("voices", "Voices", par("voices"), "", 0, False), ("portamentoTime", "Glide", par("portamentoTime"), "", 0, False),
            ("pitchBendRange", "Bend", par("pitchBendRange"), " st", 0, False), ("transpose", "Transp.", par("transpose"), " st", 0, False),
            ("pan", "Pan", percent("pan"), " %", 0, False), ("panByKey", "By key", percent("panByKey"), " %", 0, False),
            ("panRandom", "Random", percent("panRandom"), " %", 0, False), None],
    }
    controls = []
    for page, cells in pages.items():
        shown = equal(par("panel.page"), float(page))
        for i, cell in enumerate(cells):
            if cell is None:
                continue
            pid, caption, value, suffix, decimals, log = cell
            controls.append(dial(pid, caption, x + (i % 4) * 50.0, y + (i // 4) * 80.0, value, suffix, decimals, log, shown, f"{tag}_p{page}"))
    lfo = equal(par("panel.page"), 3.0)
    controls += lfo_rate_knob(x, y, 50.0, 70.0, lfo, f"{tag}_p3")
    # What does nothing right now looks it.
    filter_page = equal(par("panel.page"), 2.0)
    controls.append(dim(f"dim_morph{tag}", [x + 150.0, y, 50.0, 70.0], both(filter_page, sub(1.0, equal(par("filterType"), 4.0)))))
    controls.append(dim(f"dim_drive{tag}", [x + 100.0, y, 50.0, 70.0], both(filter_page, equal(par("filterCircuit"), 0.0))))
    controls.append(dim(f"dim_shaper{tag}", [x + 150.0, y + 80.0, 50.0, 70.0], both(filter_page, equal(par("shaper"), 0.0))))
    return controls


def header_title(cid, caption, x, y, visible):
    return graphic(cid, caption, [x, y, 100.0, 18.0], [text(6.0, 3.0, caption, CAPTION, 11.0)], visible)


def display(x, y, w, h, tag):
    """The display beside the dials: a line of what the page chooses, the page's picture under it."""
    controls = []
    page = lambda i: equal(par("panel.page"), float(i))
    head_y, body_y, body_h = y + 2.0, y + 22.0, h - 22.0
    controls.append(graphic(f"display_ground{tag}", "Display", [x, y, w, h], [rect(0.0, 0.0, w, h, DISPLAY, 3.0)], sub(1.0, page(5))))
    end = x + w - 42.0
    for n in OPS:
        shown = both(selected(n), page(0))
        time = both(shown, is_on(f"op{n}.envMode"))
        controls.append(header_title(f"env_title_{n}{tag}", f"Operator {n}", x, y + 1.0, shown))
        controls.append(choose(f"mode_time_{n}{tag}", f"op{n}.envMode", "ADSR", 1, [x + 76.0, head_y, 38.0, 18.0], shown))
        controls.append(choose(f"mode_rates_{n}{tag}", f"op{n}.envMode", "Rates", 0, [x + 116.0, head_y, 38.0, 18.0], shown))
        controls.append(label_beside(f"loop_name_{n}{tag}", "Loop", x + 160.0, head_y, 28.0, 18.0, time))
        controls.append(menu(f"op{n}.env.loop", "Loop", [x + 186.0, head_y, 66.0, 18.0], time, tag))
        controls += op_envelope(n, tag, x, body_y, w, body_h, shown)
        waved = both(selected(n), page(1))
        controls.append(header_title(f"wave_title_{n}{tag}", f"Operator {n}", x, y + 1.0, waved))
        controls.append(menu(f"op{n}.partials", "Partials", [x + 76.0, head_y, 50.0, 18.0], waved, tag))
        controls.append(menu(f"op{n}.wave", "Wave", [x + 130.0, head_y, 122.0, 18.0], waved, tag))
        controls += partial_bars(n, x + 6.0, body_y + 2.0, w - 12.0, body_h - 8.0, tag, waved)
        controls.append(bars_note(f"bars_note_{n}{tag}", x + 2.0, body_y, w - 4.0, body_h - 2.0, n, waved))
    # Filter
    controls.append(switch("filterOn", "Filter", [x + 2.0, head_y, 30.0, 18.0], page(2), tag))
    controls.append(menu("filterType", "Filter type", [x + 36.0, head_y, 84.0, 18.0], page(2), tag))
    controls.append(menu("filterSlope", "Filter slope", [x + 124.0, head_y, 56.0, 18.0], page(2), tag))
    controls.append(menu("filterCircuit", "Filter circuit", [x + 184.0, head_y, 68.0, 18.0], page(2), tag))
    controls += filter_picture(x, body_y, w, body_h, tag, page(2))
    # LFO
    sends = body_h - 22.0
    controls.append(switch("lfoOn", "LFO", [x + 2.0, head_y, 30.0, 18.0], page(3), tag))
    controls.append(menu("lfo2Wave", "LFO wave", [x + 36.0, head_y, 110.0, 18.0], page(3), tag))
    controls.append(menu("lfo2Range", "LFO range", [x + 150.0, head_y, 70.0, 18.0], page(3), tag))
    controls += lfo_picture(x, body_y, w, sends, tag, page(3))
    controls.append(label_beside(f"lfo_to{tag}", "To", x + 6.0, body_y + sends + 2.0, 20.0, 18.0, page(3)))
    for n in OPS:
        controls.append(toggle(f"lfo2ToOp{n}", str(n), [x + 28.0 + (n - 1) * 34.0, body_y + sends + 2.0, 32.0, 18.0], page(3), tag))
    controls.append(toggle("lfo2ToFilter", "Filter", [x + 232.0, body_y + sends + 2.0, 62.0, 18.0], page(3), tag))
    # Pitch
    controls.append(switch("pitchEnvOn", "Pitch envelope", [x + 2.0, head_y, 30.0, 18.0], page(4), tag))
    controls.append(header_title(f"pitch_title{tag}", "Pitch envelope", x + 32.0, y + 1.0, page(4)))
    controls.append(graphic(f"pitch_picture{tag}", "Pitch envelope", [x, body_y, w, body_h],
                            env_picture("pitchEnv.", w, body_h, 48.0, -48.0, HIGHLIGHT, [24.0, 0.0, -24.0]), page(4)))
    controls += env_points("pitchEnv.", tag, x, body_y, w, body_h, page(4))
    # Voice
    cells = [("retrigger", "Retrigger"), ("portamento", "Glide"), ("glissando", "Glissando"), ("antialias", "Antialias"), ("interpolation", "Interpolation")]
    controls.append(header_title(f"voice_title{tag}", "Voices and pitch", x, y + 1.0, page(6)))
    for i, (pid, caption) in enumerate(cells):
        controls.append(toggle(pid, caption, [x + 6.0 + (i % 3) * 96.0, body_y + 4.0 + (i // 3) * 34.0, 92.0, 30.0], page(6), tag))
    controls.append(field("pitchBendStep", "Bend step", [x + 198.0, body_y + 38.0, 92.0, 30.0], par("pitchBendStep"), " st", visible=page(6), tag=tag))
    # A way to the rest of each page, in the same place.
    for i, target in ((0, 0), (1, 0), (2, 1), (3, 2), (4, 3), (6, 0)):
        controls.append(more(f"more_{i}{tag}", target, [end, head_y, 40.0, 18.0], page(i)))
    return controls


def mod_page(x, y, tag):
    """Every source's first route, edited on the device row; the second routes are in the expanded panel."""
    shown = equal(par("panel.page"), 5.0)
    controls = [graphic(f"mod_heads{tag}", "Modulation columns", [x, y, 498.0, 13.0],
                        [t for col in (0.0, 250.0) for t in (text(col + 2.0, 0.0, "Source", CAPTION, 10.0), text(col + 62.0, 0.0, "Goes to", CAPTION, 10.0),
                                                             text(col + 150.0, 0.0, "Amount", CAPTION, 10.0))], shown)]
    for i, (source, name) in enumerate(MATRIX):
        cx, cy = x + (i // 5) * 250.0, y + 14.0 + (i % 5) * 26.0
        controls.append(label_beside(f"mod_{source}_name{tag}", name, cx + 2.0, cy, 58.0, 24.0, shown))
        controls.append(menu(f"mod.{source}.dest1", f"{name} goes to", [cx + 60.0, cy + 1.0, 86.0, 22.0], shown, tag))
        controls.append(field(f"mod.{source}.amount1", f"{name} amount", [cx + 150.0, cy + 1.0, 88.0, 22.0], percent(f"mod.{source}.amount1"), " %",
                              visible=shown, tag=tag, fill="centre", cid=f"mod.{source}.amount1{tag}", plain=True))
    controls.append(more(f"more_5{tag}", 4, [x + 250.0 + 60.0, y + 14.0 + 4.0 * 26.0 + 1.0, 178.0, 22.0], shown, "Second routes…"))
    return controls


def meter(cid, bounds):
    """The output's peak from the device's readouts 0 and 1, in decibels."""
    _, _, w, h = bounds
    draws = []
    for i, index in enumerate((0, 1)):
        level = minimum(1.0, maximum(0.0, add(mul({"op": "readout", "index": index}, 1.0 / 70.0), 1.0)))
        bx = w / 2.0 - 6.0 + i * 7.0
        draws.append(rect(bx, 0.0, 5.0, h, CONTROL, 1.0))
        draws.append(rect(bx, sub(h, mul(level, h)), 5.0, mul(level, h), ACCENT if i == 0 else TEXT, 1.0))
    return graphic(cid, "Output level", bounds, draws)


def stepper(pid, caption, bounds, tag=""):
    """A number with arrows that step it, as its neighbours have nothing in common but their numbers."""
    return {"widget": {"kind": "stepper", "decimals": 0}, "visible_when": None, "id": f"{pid}{tag}", "name": caption,
            "tooltip": f"{caption}: click the arrows, or drag up or down", "bounds": bounds, "gesture": "drag", "x": None,
            "y": binding(pid), "drawing": []}


def compact():
    w, h = COMPACT
    controls = op_table(ROW_COMPACT, 1.0, 25.0, "")
    controls.append(separator("sep_ops", 325.0, 4.0, h - 8.0))
    controls.append(label_beside("algorithm_name", "Algorithm", 332.0, 2.0, 52.0, 20.0))
    controls.append(stepper("algorithm", "Algorithm", [384.0, 2.0, 36.0, 20.0]))
    controls.append(more("more_algorithm", 5, [422.0, 2.0, 24.0, 20.0], caption="All"))
    controls += algorithm_pictures(330.0, 24.0, 116.0, 114.0, "", 12.0)
    controls.append(field("feedback", "Feedback", [330.0, 140.0, 116.0, 24.0], par("feedback")))
    controls.append(separator("sep_alg", 451.0, 4.0, h - 8.0))
    controls += tabs("panel.page", PAGES, 456.0, 2.0, 42.0, 18.0, "page")
    controls += tab_marks(456.0, 2.0, 42.0, "page", [(2, "Filter", "filterOn"), (3, "LFO", "lfoOn"), (4, "Pitch", "pitchEnvOn")])
    controls += display(456.0, 22.0, 294.0, 142.0, "")
    controls += mod_page(456.0, 22.0, "")
    for n in OPS:
        controls += op_dials(n, 756.0, 4.0, "")
    controls += global_dials(756.0, 4.0, "")
    controls.append(separator("sep_dials", 960.0, 4.0, h - 8.0))
    controls.append(dial("time", "Time", 964.0, 4.0, percent("time"), " %"))
    controls.append(dial("tone", "Tone", 1014.0, 4.0, percent("tone"), " %"))
    controls.append(dial("spread", "Spread", 964.0, 84.0, percent("spread"), " %"))
    controls.append(dial("volume", "Volume", 1014.0, 84.0, par("volume"), " dB", 1))
    controls.append(random_button("random", "Random", [1070.0, 4.0, 66.0, 24.0]))
    controls.append(field("random.amount", "Amount", [1070.0, 32.0, 66.0, 32.0], percent("random.amount"), " %"))
    controls.append(more("random_settings", 6, [1070.0, 68.0, 66.0, 20.0]))
    controls.append(meter("meter", [1140.0, 4.0, 36.0, h - 8.0]))
    return {"size": [w, h], "viewport_size": None, "background": PANEL, "assets": [], "layers": [], "controls": controls,
            "hidden_parameters": every_parameter(), "accent_on": True}


# A cell of a grid: it is given its bounds, whether it shows and the canvas's tag.
def f_num(pid, caption, suffix="", decimals=0, fill="bar", log=False):
    return lambda bounds, visible, tag: [field(pid, caption, bounds, par(pid), suffix, decimals, log, visible, tag, fill=fill)]


def f_pct(pid, caption, fill="bar"):
    return lambda bounds, visible, tag: [field(pid, caption, bounds, percent(pid), " %", 0, False, visible, tag, fill=fill)]


def f_both(pid, caption):
    return f_pct(pid, caption, "centre")


def f_ms(pid, caption):
    return lambda bounds, visible, tag: [field(pid, caption, bounds, ms(pid), " ms", 0, True, visible, tag)]


def f_menu(pid, caption):
    return lambda bounds, visible, tag: named_menu(pid, caption, bounds, visible, tag)


def f_toggle(pid, caption, off_label=None):
    return lambda bounds, visible, tag: [toggle(pid, caption, bounds, visible, tag, off_label)]


def grid(cells, x, y, visible, tag, columns=8, w=61.0, h=34.0, gap=4.0):
    controls = []
    for i, cell in enumerate(cells):
        if cell is not None:
            controls += cell([x + (i % columns) * (w + gap), y + (i // columns) * (h + gap), w, h], visible, tag)
    return controls


def knob_row(cells, x, y, visible, tag, step=65.0):
    return [dial(pid, caption, x + i * step, y, value, suffix, decimals, log, visible, tag, w=step)
            for i, (pid, caption, value, suffix, decimals, log) in enumerate(cells)]


def env_knobs(prefix, unit):
    decimals = 2 if unit == "" else 1
    time = lambda f, caption: (f"{prefix}{f}", caption, ms(f"{prefix}{f}"), " ms", 0, True)
    level = lambda f, caption: (f"{prefix}{f}", caption, par(f"{prefix}{f}"), unit, decimals, False)
    return [time("attack", "Attack"), time("decay", "Decay"), level("sustain", "Sustain"), time("release", "Release"),
            level("initial", "Initial"), level("peak", "Peak"), level("end", "End")]


def env_cells(prefix):
    """An envelope's less used half: how it repeats, follows the keys, and bends."""
    return [f_menu(f"{prefix}loop", "Loop"), f_menu(f"{prefix}repeat", "Repeat"), f_ms(f"{prefix}loopTime", "Loop time"),
            f_both(f"{prefix}timeByVel", "By vel"), f_pct(f"{prefix}timeByKey", "By key"), f_both(f"{prefix}attackSlope", "Att. slope"),
            f_both(f"{prefix}decaySlope", "Dec. slope"), f_both(f"{prefix}releaseSlope", "Rel. slope")]


def expanded_operator(x, y, w):
    page = equal(par("panel.expandedPage"), 0.0)
    controls = []
    tag = "_x"
    for n in OPS:
        shown = both(page, selected(n))
        p = lambda f: f"op{n}.{f}"
        e = f"op{n}.env."
        time = both(shown, is_on(p("envMode")))
        rates = both(shown, is_off(p("envMode")))
        controls.append(section(f"x_title_heard_{n}", f"Operator {n}: heard", x, y, w, both(shown, heard(n))))
        controls.append(section(f"x_title_mod_{n}", f"Operator {n}: shapes others", x, y, w, both(shown, modulates(n))))
        controls.append(choose(f"x_mode_time_{n}", p("envMode"), "ADSR", 1, [x + w - 174.0, y - 3.0, 60.0, 17.0], shown))
        controls.append(choose(f"x_mode_rates_{n}", p("envMode"), "Rates and levels", 0, [x + w - 112.0, y - 3.0, 112.0, 17.0], shown))
        controls += op_envelope(n, tag, x, y + 18.0, w, 120.0, shown)
        controls += knob_row(env_knobs(e, " dB"), x, y + 142.0, time, tag)
        controls += grid(env_cells(e), x, y + 214.0, time, tag)
        controls += knob_row([(p(f"rate{i}"), f"Rate {i}", par(p(f"rate{i}")), "", 0, False) for i in range(1, 5)]
                             + [(p(f"level{i}"), f"Level {i}", par(p(f"level{i}")), "", 0, False) for i in range(1, 5)], x, y + 142.0, rates, f"{tag}_rates")
        at = y + 254.0
        controls.append(section(f"x_osc_{n}", "Oscillator", x, at, w, shown))
        controls += named_menu(p("wave"), "Wave", [x, at + 18.0, 126.0, 34.0], shown, tag)
        controls += grid([f_pct(p("feedback"), "Feedback"), f_num(p("detune"), "Detune", fill="centre"),
                          lambda b, v, t, pid=p("phase"): [field(pid, "Phase", b, mul(par(pid), 360.0), "°", visible=v, tag=t)], f_toggle(p("retrigger"), "Retrigger"),
                          f_both(p("freqByVel"), "Pitch vel"), f_toggle(p("freqQuantize"), "Quantize")], x + 130.0, at + 18.0, shown, tag, columns=6)
        at += 58.0
        controls.append(section(f"x_key_{n}", "Keyboard and velocity", x, at, w, shown))
        controls += grid([f_num(p("breakPoint"), "Break pt"), f_num(p("leftDepth"), "Left"), f_menu(p("leftCurve"), "Curve"), f_num(p("rightDepth"), "Right"),
                          f_menu(p("rightCurve"), "Curve"), f_num(p("rateScaling"), "Key rate"), f_both(p("levelByKey"), "Key level"), f_num(p("velocitySens"), "Velocity"),
                          f_num(p("ampModSens"), "LFO level")], x, at + 18.0, shown, tag, columns=9, w=54.0, gap=3.75)
        at += 58.0
        controls.append(section(f"x_bars_title_{n}", "Partials", x, at, w, shown))
        controls += [choose(f"x_partials_{n}_{i}", p("partials"), label, i, [x + 60.0 + i * 32.0, at - 2.0, 30.0, 16.0], shown) for i, label in enumerate(("16", "32", "64"))]
        controls.append(label_beside(f"x_repeat_name_{n}", "Repeat", x + 164.0, at - 2.0, 38.0, 16.0, shown))
        controls.append(menu(p("repeat"), "Repeat", [x + 202.0, at - 2.0, 70.0, 16.0], shown, tag))
        controls.append(label_beside(f"x_used_name_{n}", "Use", x + 280.0, at - 2.0, 24.0, 16.0, shown))
        controls.append(menu(p("oddEven"), "Partials used", [x + 304.0, at - 2.0, 70.0, 16.0], shown, tag))
        controls.append(toggle(p("normalize"), "Normalize", [x + w - 70.0, at - 2.0, 70.0, 16.0], shown, tag))
        bars_y, bars_h = at + 20.0, y + 606.0 - (at + 20.0)
        controls.append(graphic(f"x_bars_bg_{n}", "Partials", [x, bars_y, w, bars_h], [rect(0.0, 0.0, w, bars_h, DISPLAY, 3.0)], shown))
        controls += partial_bars(n, x + 4.0, bars_y + 4.0, w - 8.0, bars_h - 8.0, tag, shown)
        controls.append(bars_note(f"x_bars_note_{n}", x + 2.0, bars_y + 2.0, w - 4.0, bars_h - 4.0, n, shown))
    return controls


def expanded_filter(x, y, w):
    shown = equal(par("panel.expandedPage"), 1.0)
    tag = "_x"
    controls = [section("x_filter_title", "Filter", x, y, w, shown), switch("filterOn", "Filter", [x + w - 40.0, y - 3.0, 40.0, 17.0], shown, tag)]
    controls += filter_picture(x, y + 18.0, w, 124.0, tag, shown)
    controls += grid([f_menu("filterType", "Type"), f_menu("filterSlope", "Slope"), f_menu("filterCircuit", "Circuit")], x, y + 148.0, shown, tag, columns=3, w=170.0)
    cells = [("filterFreq", "Freq", par("filterFreq"), " Hz", 0, True), ("filterRes", "Res", percent("filterRes"), " %", 0, False),
             ("filterDrive", "Drive", par("filterDrive"), " dB", 1, False), ("filterMorph", "Morph", percent("filterMorph"), " %", 0, False),
             ("filterEnv", "Env", percent("filterEnv"), " %", 0, False), ("filterByVel", "By vel", percent("filterByVel"), " %", 0, False),
             ("filterByKey", "By key", percent("filterByKey"), " %", 0, False)]
    controls += knob_row(cells, x, y + 186.0, shown, tag)
    controls.append(dim("x_dim_morph", [x + 195.0, y + 186.0, 65.0, 70.0], both(shown, sub(1.0, equal(par("filterType"), 4.0)))))
    controls.append(dim("x_dim_drive", [x + 130.0, y + 186.0, 65.0, 70.0], both(shown, equal(par("filterCircuit"), 0.0))))
    controls.append(section("x_shaper_title", "Shaper", x, y + 260.0, w, shown))
    controls += grid([f_menu("shaper", "Shape"), f_num("shaperDrive", "Drive", " dB", 1), f_pct("shaperMix", "Dry/wet")], x, y + 278.0, shown, tag, columns=3, w=170.0)
    controls.append(section("x_filter_env_title", "Filter envelope", x, y + 318.0, w, shown))
    controls.append(graphic("x_filter_env", "Filter envelope", [x, y + 336.0, w, 100.0], env_picture("filterEnv.", w, 100.0, 1.0, 0.0, MODULATOR, [0.5]), shown))
    controls += env_points("filterEnv.", tag, x, y + 336.0, w, 100.0, shown)
    controls += knob_row(env_knobs("filterEnv.", ""), x, y + 440.0, shown, tag)
    return controls + grid(env_cells("filterEnv."), x, y + 512.0, shown, tag)


def expanded_lfo(x, y, w):
    shown = equal(par("panel.expandedPage"), 2.0)
    tag = "_x"
    controls = [section("x_lfo_title", "LFO", x, y, w, shown), switch("lfoOn", "LFO", [x + w - 40.0, y - 3.0, 40.0, 17.0], shown, tag)]
    controls += lfo_picture(x, y + 18.0, w, 76.0, tag, shown)
    controls += grid([f_menu("lfo2Wave", "Wave"), f_menu("lfo2Range", "Range")], x, y + 100.0, shown, tag, columns=2, w=196.0)
    controls.append(toggle("lfo2Retrigger", "Retrigger", [x + 400.0, y + 113.0, 120.0, 21.0], shown, tag))
    controls += lfo_rate_knob(x, y + 138.0, 65.0, 70.0, shown, tag)
    controls += knob_row([("lfo2Amount", "Amount", percent("lfo2Amount"), " %", 0, False), ("lfo2ByKey", "Rate by key", percent("lfo2ByKey"), " %", 0, False),
                          ("lfo2ByVel", "Amt by vel", percent("lfo2ByVel"), " %", 0, False)], x + 65.0, y + 138.0, shown, tag)
    controls.append(section("x_lfo_to", "Goes to", x + 260.0, y + 138.0, w - 260.0, shown))
    for n in OPS:
        controls.append(toggle(f"lfo2ToOp{n}", f"Op {n}", [x + 260.0 + ((n - 1) % 3) * 88.0, y + 158.0 + ((n - 1) // 3) * 24.0, 84.0, 21.0], shown, tag))
    controls.append(section("x_lfo_also", "Also to", x, y + 212.0, w, shown))
    controls.append(toggle("lfo2ToFilter", "Filter", [x, y + 230.0, 100.0, 34.0], shown, tag))
    controls += grid([f_menu("lfo2DestB", "Destination"), f_both("lfo2AmountB", "Amount")], x + 104.0, y + 230.0, shown, tag, columns=2, w=120.0)
    controls.append(section("x_lfo_env_title", "LFO envelope", x, y + 270.0, w, shown))
    controls.append(graphic("x_lfo_env", "LFO envelope", [x, y + 288.0, w, 80.0], env_picture("lfoEnv.", w, 80.0, 1.0, 0.0, SPECTRUM_COLOR, [0.5]), shown))
    controls += env_points("lfoEnv.", tag, x, y + 288.0, w, 80.0, shown)
    knobs = [lambda b, v, t, c=c: [field(c[0], c[1], b, c[2], c[3], c[4], c[5], v, t)] for c in env_knobs("lfoEnv.", "")]
    controls += grid(knobs + env_cells("lfoEnv.")[:1], x, y + 372.0, shown, tag)
    controls += grid(env_cells("lfoEnv.")[1:], x, y + 410.0, shown, tag)
    controls.append(section("x_lfo_classic", "Classic LFO", x, y + 450.0, w, shown))
    return controls + grid([f_menu("lfoWave", "Wave"), f_num("lfoSpeed", "Speed"), f_num("lfoDelay", "Delay"), f_num("lfoPitchDepth", "Pitch"),
                            f_num("lfoAmpDepth", "Level"), f_num("pitchModSens", "Pitch sens"), f_toggle("lfoKeySync", "Key sync"), f_toggle("oscKeySync", "Osc sync")],
                           x, y + 468.0, shown, tag)


def expanded_pitch(x, y, w):
    shown = equal(par("panel.expandedPage"), 3.0)
    tag = "_x"
    controls = [section("x_pitch_title", "Pitch envelope", x, y, w, shown), switch("pitchEnvOn", "Pitch envelope", [x + w - 40.0, y - 3.0, 40.0, 17.0], shown, tag)]
    controls.append(graphic("x_pitch_env", "Pitch envelope", [x, y + 18.0, w, 130.0], env_picture("pitchEnv.", w, 130.0, 48.0, -48.0, HIGHLIGHT, [24.0, 0.0, -24.0]), shown))
    controls += env_points("pitchEnv.", tag, x, y + 18.0, w, 130.0, shown)
    controls += knob_row(env_knobs("pitchEnv.", " st") + [("pitchEnvAmount", "Amount", percent("pitchEnvAmount"), " %", 0, False)], x, y + 152.0, shown, tag)
    controls += grid(env_cells("pitchEnv."), x, y + 224.0, shown, tag)
    controls.append(section("x_pitch_to", "Goes to", x, y + 264.0, w, shown))
    controls += grid([f_toggle(f"pitchEnvToOp{n}", f"Op {n}") for n in OPS] + [f_toggle("pitchEnvToLfo", "To LFO")], x, y + 282.0, shown, tag, columns=7, w=70.0, h=24.0)
    controls += grid([f_menu("pitchEnvDestB", "Also to"), f_both("pitchEnvAmountB", "Amount")], x, y + 310.0, shown, tag, columns=2, w=170.0)
    controls.append(section("x_pitch_classic", "Classic pitch envelope", x, y + 350.0, w, shown))
    return controls + grid([f_num(f"pitchRate{i}", f"Rate {i}") for i in range(1, 5)] + [f_num(f"pitchLevel{i}", f"Level {i}") for i in range(1, 5)], x, y + 368.0, shown, tag)


def table_heads(cid, heads, x, y, w, visible):
    draws = [text(at, 0.0, name, CAPTION, 10.0) for at, name in heads] + [line([0.0, 14.0], [w, 14.0], BORDER)]
    return graphic(cid, "Columns", [x, y, w, 15.0], draws, visible)


def expanded_matrix(x, y, w):
    shown = equal(par("panel.expandedPage"), 4.0)
    tag = "_x"
    columns = (84.0, 238.0, 304.0, 458.0)
    controls = [table_heads("x_matrix_heads", [(0.0, "Source"), (columns[0], "Goes to"), (columns[1], "Amount"), (columns[2], "Also to"), (columns[3], "Amount")], x, y, w, shown)]
    for i, (source, name) in enumerate(MATRIX):
        row = y + 18.0 + i * 28.0
        controls.append(graphic(f"x_matrix_{source}", name, [x, row, 80.0, 24.0], [text(0.0, 5.0, name, TEXT, 11.0)], shown))
        controls.append(menu(f"mod.{source}.dest1", f"{name} goes to", [x + columns[0], row, 150.0, 24.0], shown, tag))
        controls.append(field(f"mod.{source}.amount1", f"{name} amount", [x + columns[1], row, 62.0, 24.0], percent(f"mod.{source}.amount1"), " %", visible=shown, tag=tag, fill="centre", plain=True))
        controls.append(menu(f"mod.{source}.dest2", f"{name} also to", [x + columns[2], row, 150.0, 24.0], shown, tag))
        controls.append(field(f"mod.{source}.amount2", f"{name} second amount", [x + columns[3], row, 62.0, 24.0], percent(f"mod.{source}.amount2"), " %", visible=shown, tag=tag, fill="centre", plain=True))
    top = y + 18.0 + 9 * 28.0 + 8.0
    controls.append(section("x_routes_title", "Classic controllers", x, top, w, shown))
    controls.append(table_heads("x_routes_heads", [(0.0, "Controller"), (84.0, "Range"), (168.0, "Goes to")], x, top + 18.0, w, shown))
    for i, (s, name) in enumerate((("wheel", "Mod wheel"), ("foot", "Foot"), ("breath", "Breath"), ("pressure", "Pressure"))):
        row = top + 36.0 + i * 28.0
        controls.append(graphic(f"x_routes_{s}", name, [x, row, 80.0, 24.0], [text(0.0, 5.0, name, TEXT, 11.0)], shown))
        controls.append(field(f"{s}Range", f"{name} range", [x + 84.0, row, 80.0, 24.0], par(f"{s}Range"), visible=shown, tag=tag, plain=True))
        for j, (f, caption) in enumerate((("Pitch", "Pitch"), ("Amp", "Level"), ("EgBias", "Envelope"))):
            controls.append(toggle(f"{s}{f}", caption, [x + 168.0 + j * 84.0, row, 80.0, 24.0], shown, tag))
    return controls


def expanded_algorithm(x, y, w):
    """Every algorithm as a card that takes the click anywhere on it."""
    shown = equal(par("panel.expandedPage"), 5.0)
    step, cell_w, cell_h = 65.0, 62.0, 56.0
    controls = [section("x_algorithms_six", "Six operators", x, y, w, shown), section("x_algorithms_four", "Four operators", x, y + 18.0 + 4 * 60.0 + 6.0, w, shown)]
    for i, flags in enumerate(TABLES):
        top = y + 18.0 if i < 32 else y + 18.0 + 4 * 60.0 + 24.0
        row = i if i < 32 else i - 32
        cx, cy = x + (row % 8) * step, top + (row // 8) * 60.0
        on = equal(par("algorithm"), float(i + 1))
        drawing = [rect(0.0, 0.0, cell_w, cell_h, DISPLAY, 3.0), rect(0.0, cell_h - 13.0, mul(on, cell_w), 13.0, ACCENT, 2.0)]
        drawing += picture(flags, cell_w - 8.0, cell_h - 22.0, 6.0)
        drawing.append(text(cell_w / 2.0, cell_h - 13.0, str(i + 1), TEXT, 10.0, center=True))
        controls.append({"visible_when": shown, "id": f"x_pick_{i + 1}", "name": f"Algorithm {i + 1}", "tooltip": f"Use algorithm {i + 1}", "bounds": [cx, cy, cell_w, cell_h],
                         "gesture": {"set": i + 1}, "x": binding("algorithm"), "y": None, "drawing": drawing})
    controls.append(algorithm_legend("x_algorithms_legend", x, y + 18.0 + 4 * 60.0 + 24.0 + 2 * 60.0 + 2.0, shown))
    return controls


def expanded_random(x, y, w):
    shown = equal(par("panel.expandedPage"), 6.0)
    tag = "_x"
    controls = [section("x_random_title", "Randomizer", x, y, w, shown), random_button("x_random_now", "Random", [x, y + 18.0, 120.0, 34.0], shown)]
    controls += grid([f_pct("random.amount", "Amount"), f_num("random.seed", "Seed", fill=None)], x + 124.0, y + 18.0, shown, f"{tag}_page", columns=2, w=120.0)
    controls.append(graphic("x_random_note", "How the randomizer moves values", [x, y + 56.0, w, 14.0],
                            [text(0.0, 0.0, "A value moves from where it is, by at most its section's share of its range.", CAPTION, 10.0)], shown))
    controls.append(table_heads("x_random_heads", [(0.0, "Section"), (134.0, "Share"), (238.0, "Random")], x, y + 76.0, w, shown))
    for i, (sid, name) in enumerate(SECTIONS):
        row = y + 94.0 + i * 28.0
        controls.append(graphic(f"x_section_{sid}", name, [x, row, 130.0, 24.0], [text(0.0, 5.0, name, TEXT, 11.0)], shown))
        controls.append(field(f"random.{sid}.amount", f"{name} share", [x + 134.0, row, 100.0, 24.0], percent(f"random.{sid}.amount"), " %", visible=shown, tag=tag, plain=True))
        controls.append(toggle(f"random.{sid}.on", "Changes", [x + 238.0, row, 100.0, 24.0], shown, tag, off_label="Keeps"))
    return controls


def expanded_right(x, y, w):
    tag = "_xr"
    step = w / 4.0
    controls = [section("x_output", "Output", x, y, w)]
    controls += knob_row([("volume", "Volume", par("volume"), " dB", 1, False), ("tone", "Tone", percent("tone"), " %", 0, False),
                          ("time", "Time", percent("time"), " %", 0, False), ("pan", "Pan", percent("pan"), " %", 0, False)], x, y + 18.0, None, tag, step)
    at = y + 94.0
    controls.append(section("x_stereo", "Stereo", x, at, w))
    controls += knob_row([("spread", "Spread", percent("spread"), " %", 0, False), ("panByKey", "Pan by key", percent("panByKey"), " %", 0, False),
                          ("panRandom", "Pan random", percent("panRandom"), " %", 0, False)], x, at + 18.0, None, tag, step)
    at += 94.0
    controls.append(section("x_voices", "Voices", x, at, w))
    controls += knob_row([("voices", "Voices", par("voices"), "", 0, False), ("portamentoTime", "Glide time", par("portamentoTime"), "", 0, False)], x, at + 18.0, None, tag, step)
    controls += grid([f_toggle("retrigger", "Retrigger"), f_toggle("portamento", "Glide"), f_toggle("glissando", "Glissando")], x, at + 92.0, None, tag, columns=3, w=90.0, h=26.0, gap=5.0)
    at += 126.0
    controls.append(section("x_pitch", "Pitch", x, at, w))
    controls += knob_row([("transpose", "Transpose", par("transpose"), " st", 0, False), ("pitchBendRange", "Bend range", par("pitchBendRange"), " st", 0, False),
                          ("pitchBendStep", "Bend step", par("pitchBendStep"), " st", 0, False)], x, at + 18.0, None, tag, step)
    at += 94.0
    controls.append(section("x_quality", "Quality", x, at, w))
    controls += grid([f_toggle("antialias", "Antialias"), f_toggle("interpolation", "Interpolation")], x, at + 18.0, None, tag, columns=2, w=136.0, h=26.0)
    return controls


def expanded():
    w, h = EXPANDED
    controls = op_table(ROW_EXPANDED, 2.0, 36.0, "_x")
    at = 2.0 + 15.0 + 6 * 36.0 + 6.0
    controls.append(section("x_algorithm_title", "Algorithm", 4.0, at, 344.0))
    controls.append(stepper("algorithm", "Algorithm", [296.0, at - 3.0, 52.0, 17.0], "_x"))
    controls.append(field("feedback", "Feedback", [4.0, at + 20.0, 170.0, 24.0], par("feedback"), tag="_x"))
    controls.append(more("x_more_algorithm", 5, [178.0, at + 20.0, 170.0, 24.0], caption="All algorithms"))
    controls += algorithm_pictures(4.0, at + 48.0, 344.0, 240.0, "_x", 18.0)
    controls.append(algorithm_legend("x_legend", 4.0, at + 292.0))
    controls.append(section("x_random_side", "Random", 4.0, at + 312.0, 344.0))
    controls.append(random_button("x_random", "Random", [4.0, at + 330.0, 110.0, 34.0]))
    controls.append(field("random.amount", "Amount", [118.0, at + 330.0, 110.0, 34.0], percent("random.amount"), " %", tag="_x"))
    controls.append(more("x_more_random", 6, [232.0, at + 330.0, 116.0, 34.0]))
    controls.append(separator("x_sep_left", 356.0, 4.0, h - 8.0))
    x, y, mid = 362.0, 34.0, 520.0
    controls += tabs("panel.expandedPage", EXPANDED_PAGES, x, 2.0, mid / 7.0, 24.0, "x_page")
    controls += tab_marks(x, 2.0, mid / 7.0, "x_page", [(1, "Filter", "filterOn"), (2, "LFO", "lfoOn"), (3, "Pitch", "pitchEnvOn")])
    controls += expanded_operator(x, y, mid)
    controls += expanded_filter(x, y, mid)
    controls += expanded_lfo(x, y, mid)
    controls += expanded_pitch(x, y, mid)
    controls += expanded_matrix(x, y, mid)
    controls += expanded_algorithm(x, y, mid)
    controls += expanded_random(x, y, mid)
    controls.append(separator("x_sep_right", 888.0, 4.0, h - 8.0))
    controls += expanded_right(896.0, 2.0, 280.0)
    return {"size": [w, h], "viewport_size": None, "background": PANEL, "assets": [], "layers": [], "controls": controls,
            "hidden_parameters": every_parameter(), "accent_on": True}


def check_colours(value):
    """Every colour in a drawing is the theme's: palette or token, or nothing at all."""
    if isinstance(value, dict):
        for key, item in value.items():
            if key in ("color", "background", "fill") and isinstance(item, list):
                assert item[0] == 3 or item == INVISIBLE, (key, item)
            check_colours(item)
    elif isinstance(value, list):
        for item in value:
            check_colours(item)


def depth_of(expression):
    """How deep an expression goes, as the SDK limits it to 16."""
    if not isinstance(expression, dict) or "op" not in expression:
        return 0
    return 1 + max([depth_of(v) for k, v in expression.items() if k != "op"] + [0])


def check_depth(value):
    if isinstance(value, dict):
        if "op" in value:
            assert depth_of(value) <= 17, (depth_of(value), str(value)[:200])
        else:
            for item in value.values():
                check_depth(item)
    elif isinstance(value, list):
        for item in value:
            check_depth(item)


def check_fits(canvas):
    """Every control lies inside its canvas, as the SDK refuses one that does not."""
    w, h = canvas["size"]
    for c in canvas["controls"]:
        x, y, cw, ch = c["bounds"]
        assert x >= 0.0 and y >= 0.0 and x + cw <= w and y + ch <= h, (c["id"], c["bounds"])
        assert cw >= 8.0 and ch >= 8.0, (c["id"], c["bounds"])


def main():
    panel = {"ui": compact(), "expanded": expanded()}
    check_colours(panel)
    check_depth(panel)
    for canvas in (panel["ui"], panel["expanded"]):
        check_fits(canvas)
        ids = [c["id"] for c in canvas["controls"]]
        assert len(ids) == len(set(ids)), sorted({i for i in ids if ids.count(i) > 1})[:8]
    out = Path("devices/fm-synth/panel.json")
    out.write_text(json.dumps(panel, separators=(",", ":")) + "\n", encoding="utf-8")
    print(f"wrote {out}: {out.stat().st_size} bytes, {len(panel['ui']['controls'])} and {len(panel['expanded']['controls'])} controls")


if __name__ == "__main__":
    main()
