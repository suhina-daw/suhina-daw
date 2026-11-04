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
# Each operator's colour, the same on its row, in the algorithm's picture and where something is sent to it.
OP_COLORS = {1: [3, 0, 2, 255], 2: [3, 0, 3, 255], 3: [3, 0, 4, 255], 4: HIGHLIGHT, 5: SPECTRUM_COLOR, 6: ACCENT}
CAPTION = tint(TEXT, 170)
QUIET = tint(TEXT, 110)
GRID = tint(TEXT, 30)
FILL = tint(TEXT, 28)
OPS = range(1, 7)
COMPACT = (1078.0, 168.0)
EXPANDED = (1400.0, 230.0)
PAGES = ["Env", "Wave", "Filter", "LFO", "Pitch", "Algo", "Mod", "Voice"]
# The index of the Drawn wave in devices/fm-synth/src/params.rs.
DRAWN_WAVE = 22
# The pages the display and the larger view share; Random has no tab, its button has the way there.
PAGE_NAMES = PAGES + ["Random"]
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
PANEL_PARAMS = ["panel.op", "panel.page", "panel.filterView", "random.amount", "random.seed"] + [
    f"random.{s}.{f}" for s, _ in SECTIONS for f in ("amount", "on")]
# Where an operator row's columns fall, as (x, width).
ROW_COMPACT = {"select": (2.0, 20.0), "mode": (24.0, 34.0), "freq": (60.0, 44.0), "coarse": (106.0, 32.0),
               "fine": (140.0, 34.0), "level": (176.0, 40.0), "on": (218.0, 32.0)}
# Where the stages of an envelope's picture fall, as shares of its width: attack and decay each get
# up to one stretch by their time, on a logarithmic scale; release starts at a fixed place. Every
# stage has at least a gap, so two points never lie on top of each other.
TIME_STRETCH = 0.24
RELEASE_AT = 0.72
RELEASE_STRETCH = 0.28
STAGE_GAP = 7.0
# Where a stage's curve is sampled, as shares of it.
CURVE_SAMPLES = (0.125, 0.25, 0.375, 0.5, 0.625, 0.75, 0.875)
# How far the pointer moves a stage's curve dot through its whole range, in pixels.
CURVE_TRAVEL = 100.0
ROW_GAP = 2.0
# The filter's picture: 20 Hz to 20 kHz across, as hearing spreads pitch, and decibels up and down.
FILTER_TOP_DB, FILTER_BOTTOM_DB = 30.0, -60.0
# The filter's Q at no resonance and at all of it (filter_q in devices/fm-synth/src/ext.rs).
FILTER_Q_LOW, FILTER_Q_HIGH = 0.5, 24.5
DB_PER_LOG = 10.0 / math.log(10.0)
# The second LFO's waves (params.rs) as the SDK's preview shapes (dsp::lfo::Shape::ALL).
LFO_SHAPES = [0, 4, 1, 3, 2, 5, 5]
NOISE_WAVE = 6
SYNC_RANGE = 2
LFO_RANGES = {0: (0.02, 30.0), 1: (8.0, 12_000.0)}
# The note lengths of its synced range, in beats (REPEAT_BEATS in devices/fm-synth/src/params.rs).
REPEAT_BEATS = [0.125, 0.25, 0.5, 0.75, 1.0, 1.5, 2.0, 4.0, 8.0, 16.0]
# How long its picture lasts on the free range, in seconds: more cycles fit the faster it goes.
LFO_WINDOW = 2.0


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


def classic_lfo_speeds():
    """The engine's table: the classic LFO's rate in hertz at each speed."""
    source = Path("devices/fm-synth/src/lfo.rs").read_text(encoding="utf-8")
    table = source[source.index("const SPEED_HZ"):source.index("\n];", source.index("const SPEED_HZ"))]
    speeds = [float(v) for v in re.findall(r"\b\d+\.\d+\b", table)]
    assert len(speeds) == 100, len(speeds)
    return speeds


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
        color = OP_COLORS[n]
        if ring:
            on = selected(n)
            draws.append(rect(sub(x, mul(on, 3.0)), sub(y, mul(on, 3.0)), mul(on, size + 6.0), mul(on, size + 6.0), HIGHLIGHT, 3.0))
        draws.append(rect(x, y, size, size, color, 2.0))
        if not heard_now:
            draws.append(rect(x + 1.0, y + 1.0, size - 2.0, size - 2.0, DISPLAY, 1.0))
        if size >= 12.0:
            draws.append(text(x + size / 2.0, y + 1.0, str(n), PANEL if heard_now else color, max(size - 5.0, 8.0), center=True))
        if ring:
            # An operator not in the sound is only a faint place in the picture.
            draws.append(rect(x, y, mul(is_off(f"op{n}.on"), size), size, tint(DISPLAY, 200), 2.0))
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
            "tooltip": caption, "bounds": bounds, "gesture": "drag",
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
    control = button(cid, "panel.page", caption, bounds, gesture={"expand": page}, visible=visible)
    control["name"] = f"{caption} {PAGE_NAMES[page]}"
    control["tooltip"] = f"Show {PAGE_NAMES[page]} in the larger view above"
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


def point(cid, name, bounds, x_pid, y_pid, visible, log_x=False, flip_x=False, offset=None, span=None, flip_y=False, relative=False, hollow=False):
    """A dot on a picture, dragged along the axes it has parameters for: to where the pointer is, or
    `relative`, by how far it moves."""
    return {"widget": {"kind": "handle", "color": HIGHLIGHT, "span": span, "offset": offset, "dot": True, "relative": relative, "hollow": hollow}, "visible_when": visible,
            "id": cid, "name": name, "tooltip": name, "bounds": bounds,
            "gesture": "position", "x": binding(x_pid, log_x, flip_x), "y": binding(y_pid, flip=flip_y), "drawing": []}


def stage(share, span, gap):
    """How wide a stage is in a picture: at least `gap`, at most `span`."""
    return add(gap, mul(share, span - gap))


def env_x(prefix, w, gap=STAGE_GAP):
    """The ends of a time envelope's attack and decay and of its release, in its picture."""
    attack = stage(par(f"{prefix}attack", True, True), w * TIME_STRETCH, gap)
    decay = add(attack, stage(par(f"{prefix}decay", True, True), w * TIME_STRETCH, gap))
    release = add(w * RELEASE_AT, stage(par(f"{prefix}release", True, True), w * RELEASE_STRETCH, gap))
    return attack, decay, release


def bent(p, slope):
    """How far along a stage its level is at `p`, as the engine's curve (bend in devices/fm-synth/src/time_env.rs)."""
    rising = maximum(slope, 0.0)
    falling = maximum(mul(slope, -1.0), 0.0)
    fast = sub(1.0, {"op": "exp", "value": mul(math.log(1.0 - p), add(1.0, mul(rising, 4.0)))})
    slow = {"op": "exp", "value": mul(math.log(p), add(1.0, mul(falling, 4.0)))}
    return add(add(fast, slow), -p)


def env_level(pid, h, top, bottom):
    """Where the level `pid` lies in a picture `h` high, for a range from `top` to `bottom`."""
    return add(mul(sub(top, minimum(top, maximum(bottom, par(pid)))), (h - 8.0) / (top - bottom)), 4.0)


def env_shape(prefix, w, h, top, bottom, color, gap=STAGE_GAP):
    """A time envelope's shape, moving with its times, levels and slopes; `top` and `bottom` are its range."""
    y = lambda pid: env_level(pid, h, top, bottom)
    attack, decay, release = env_x(prefix, w, gap)
    keyup = w * RELEASE_AT

    def curve(x0, y0, x1, y1, slope):
        return [[add(x0, mul(sub(x1, x0), p)), add(y0, mul(sub(y1, y0), bent(p, par(slope))))] for p in CURVE_SAMPLES]
    initial, peak, sustain, end = y(f"{prefix}initial"), y(f"{prefix}peak"), y(f"{prefix}sustain"), y(f"{prefix}end")
    points = [[0.0, initial]] + curve(0.0, initial, attack, peak, f"{prefix}attackSlope") + [[attack, peak]]
    points += curve(attack, peak, decay, sustain, f"{prefix}decaySlope") + [[decay, sustain], [keyup, sustain]]
    points += curve(keyup, sustain, release, end, f"{prefix}releaseSlope") + [[release, end], [w, end]]
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


def env_points(prefix, tag, x, y, w, h, visible, top, bottom):
    """What is dragged on a time envelope's picture: each stage's end, in time and level at once, and in
    each stage's middle a dot that bends it, higher for a stage that rises quickly at first."""
    attack, decay, release = env_x(prefix, w)
    zone = w * TIME_STRETCH - STAGE_GAP
    upper, tall = y + 4.0, h - 8.0
    level = lambda pid: env_level(f"{prefix}{pid}", h, top, bottom)
    keyup = w * RELEASE_AT

    def bend_dot(name, slope, x0, x1, y0, y1, flip):
        """The dot in the middle of a stage, on its curve: dragged up or down by the pointer's
        travel, it bends the stage and stays on it."""
        pid = f"{prefix}{slope}"
        middle = add(x, mul(add(x0, x1), 0.5))
        start = level(y0)
        on_curve = add(start, mul(sub(level(y1), start), bent(0.5, par(pid))))
        share = sub(1.0, par(pid, True)) if flip else par(pid, True)
        # The handle stands at the bottom of its bounds less its share of them: the offset puts that on the curve.
        offset = add(add(on_curve, -CURVE_TRAVEL), mul(share, CURVE_TRAVEL))
        return point(f"{pid}_point{tag}", name, [x, y, w, CURVE_TRAVEL], None, pid, visible, offset=offset,
                     span=[add(middle, -6.0), add(middle, 6.0)], flip_y=flip, relative=True, hollow=True)
    return [
        point(f"{prefix}initial_point{tag}", "Initial level", [x, upper, w, tall], None, f"{prefix}initial", visible, span=[x - 6.0, x + 6.0]),
        point(f"{prefix}attack_point{tag}", "Attack and peak", [x + STAGE_GAP, upper, zone, tall], f"{prefix}attack", f"{prefix}peak", visible, log_x=True),
        point(f"{prefix}decay_point{tag}", "Decay and sustain", [x + STAGE_GAP, upper, zone, tall], f"{prefix}decay", f"{prefix}sustain", visible, log_x=True,
              offset=attack),
        point(f"{prefix}release_point{tag}", "Release and end level", [x + keyup + STAGE_GAP, upper, w * RELEASE_STRETCH - STAGE_GAP, tall],
              f"{prefix}release", f"{prefix}end", visible, log_x=True),
        bend_dot("Attack's curve", "attackSlope", 0.0, attack, "initial", "peak", False),
        bend_dot("Decay's curve", "decaySlope", attack, decay, "peak", "sustain", True),
        bend_dot("Release's curve", "releaseSlope", keyup, release, "sustain", "end", True),
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
            ] + env_points(f"op{n}.env.", tag, x, y, w, h, time, 0.0, -70.0) + dx_points(n, tag, x, y, w, h, rates)


def r_power(power):
    """(frequency / cutoff) to `power` at a plot's place on a 20 Hz to 20 kHz logarithmic axis."""
    return {"op": "exp", "value": add(add(power * math.log(20.0), mul(par("#x"), power * math.log(1000.0))),
                                      mul({"op": "log", "value": par("filterFreq")}, -power))}


def filter_damping():
    """1 / Q squared of one section, with the device's Q from resonance. Two sections have the square
    root of the Q each (crates/dsp/src/multimode.rs)."""
    log_q = add(math.log(FILTER_Q_LOW), mul(par("filterRes"), math.log(FILTER_Q_HIGH / FILTER_Q_LOW)))
    return {"op": "exp", "value": mul(log_q, add(par("filterSlope"), -2.0))}


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
    return {"kind": "plot", "from": 0.0, "to": w, "top": 0.0, "bottom": h, "samples": 240, "y": y, "color": color, "width": 1.5}


def filter_x(hz, w):
    return (math.log(hz) - math.log(20.0)) * w / math.log(1000.0)


def filter_picture(x, y, w, h, tag, visible=None):
    """The filter's response with its frequency and resonance on one dot, on a log scale of hertz."""
    per_db = h / (FILTER_TOP_DB - FILTER_BOTTOM_DB)
    zero = FILTER_TOP_DB * per_db
    cutoff = mul(add({"op": "log", "value": par("filterFreq")}, -math.log(20.0)), w / math.log(1000.0))
    ground = [rect(0.0, 0.0, w, h, DISPLAY, 3.0), line([0.0, zero], [w, zero], GRID), line([cutoff, 0.0], [cutoff, h], tint(HIGHLIGHT, 60))]
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
    # At the cutoff a low-pass or a high-pass stands at its Q, so the dot lies on the curve.
    high = (FILTER_TOP_DB - 20.0 * math.log10(FILTER_Q_HIGH)) * per_db
    low = (FILTER_TOP_DB - 20.0 * math.log10(FILTER_Q_LOW)) * per_db
    controls.append(point(f"filter_point{tag}", "Frequency and resonance", [x, y + high, w, low - high], "filterFreq", "filterRes", visible, log_x=True))
    return controls


def classic_lfo_hz():
    return balanced_sum([mul(equal(par("lfoSpeed"), float(i)), hz) for i, hz in enumerate(classic_lfo_speeds())])


def lfo_rate_hz(index):
    low, high = LFO_RANGES[index]
    return {"op": "exp", "value": add(math.log(low), mul(par("lfo2Rate"), math.log(high / low)))}


def lfo_motion(index):
    """On a range: how many cycles its picture holds, and how many pass a second, as fast as an eye can follow."""
    if index == 0:
        hz = lfo_rate_hz(0)
        return minimum(24.0, maximum(0.25, mul(hz, LFO_WINDOW))), minimum(3.0, hz)
    if index == 1:
        return add(1.0, mul(par("lfo2Rate"), 23.0)), 1.0
    per_beat = balanced_sum([mul(equal(par("lfo2Sync"), float(i)), 1.0 / beats) for i, beats in enumerate(REPEAT_BEATS)])
    return minimum(24.0, mul(per_beat, 4.0)), minimum(3.0, mul(per_beat, 2.0))


def lfo_envelope():
    """How much of the LFO its envelope lets through across the picture."""
    p = "lfoEnv."
    t = mul(par("#x"), LFO_WINDOW)
    over = lambda pid: {"op": "exp", "value": mul({"op": "log", "value": par(pid)}, -1.0)}
    up = minimum(1.0, maximum(0.0, mul(t, over(f"{p}attack"))))
    down = minimum(1.0, maximum(0.0, mul(sub(t, par(f"{p}attack")), over(f"{p}decay"))))
    initial, peak, sustain = par(f"{p}initial"), par(f"{p}peak"), par(f"{p}sustain")
    return add(add(initial, mul(sub(peak, initial), up)), mul(sub(sustain, peak), down))


def lfo_curve(w, h, amplitude, color, shown, speed):
    """Its wave across the picture, running while it is on."""
    shape = balanced_sum([mul(equal(par("lfo2Wave"), float(i)), float(s)) for i, s in enumerate(LFO_SHAPES) if s])
    noise = add(1.0, mul(equal(par("lfo2Wave"), float(NOISE_WAVE)), 7.0))
    cycles = add(mul(mul(par("#time"), is_on("lfoOn")), speed), mul(par("#x"), mul(shown, noise)))
    y = sub(h / 2.0, mul(amplitude, {"op": "wave", "shape": shape, "cycles": cycles}))
    return {"kind": "plot", "from": 0.0, "to": w, "top": 0.0, "bottom": h, "samples": 240, "y": y, "color": color, "width": 1.5}


def lfo_picture(x, y, w, h, tag, visible=None):
    """The second LFO: its wave quietly, and in colour as much of it as reaches the sound. More cycles
    fit as its rate rises, they run while it is on, and its amount and envelope shape the colour."""
    reach = h / 2.0 - 5.0
    grid = [line([0.0, h * k / 4.0], [w, h * k / 4.0], GRID) for k in (1, 3)] + [line([w * k / 8.0, 0.0], [w * k / 8.0, h], GRID) for k in range(1, 8)]
    ground = [rect(0.0, 0.0, w, h, DISPLAY, 3.0)] + grid + [line([0.0, h / 2.0], [w, h / 2.0], tint(TEXT, 60))]
    controls = [graphic(f"lfo_ground{tag}", "LFO wave", [x, y, w, h], ground, visible)]
    for index in range(3):
        shown, speed = lfo_motion(index)
        reached = mul(mul(par("lfo2Amount"), reach), lfo_envelope())
        controls.append(graphic(f"lfo_wave_{index}{tag}", "LFO wave", [x, y, w, h],
                                [lfo_curve(w, h, reach, QUIET, shown, speed), lfo_curve(w, h, reached, SPECTRUM_COLOR, shown, speed)],
                                both(visible, equal(par("lfo2Range"), float(index)))))
    return controls


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


def fixed_decade(n):
    """Which power of ten an operator's coarse chooses at a fixed frequency: its coarse modulo 4."""
    coarse = par(f"op{n}.coarse")
    turn = mul(coarse, math.pi / 2.0)
    return sub(sub(sub(1.5, cos(turn)), sin(turn)), mul(0.5, cos(mul(coarse, math.pi))))


def power_of_ten(exponent):
    return {"op": "exp", "value": mul(exponent, math.log(10.0))}


def op_frequency(n, bounds, tag, visible=None):
    """What an operator's coarse and fine come to: the ratio to the note, or its fixed frequency."""
    x, y, w, h = bounds
    coarse, fine = par(f"op{n}.coarse"), par(f"op{n}.fine")
    ratio = mul(maximum(0.5, coarse), add(1.0, mul(fine, 0.01)))
    decade = fixed_decade(n)
    hertz = power_of_ten(add(decade, mul(fine, 0.01)))
    # The decade is a whole number but for rounding: halfway decides, so one of the two shows.
    low, high = maximum(0.0, sub(1.5, decade)), maximum(0.0, sub(decade, 1.5))
    fixed = both(visible, is_on(f"op{n}.mode"))
    def readout(cid, value, suffix, decimals, shown, prefix=""):
        return graphic(f"op{n}_freq_{cid}{tag}", f"Operator {n} frequency", [x, y, w, h],
                       [value_text(w / 2.0, (h - 13.0) / 2.0, value, prefix=prefix, suffix=suffix, size=11.0, decimals=decimals, center=True)], shown)
    return [readout("ratio", ratio, "", 2, both(visible, is_off(f"op{n}.mode")), prefix="×"),
            readout("low", hertz, " Hz", 1, both(fixed, low)), readout("high", hertz, " Hz", 0, both(fixed, high))]


def op_tuning(n, coarse_bounds, fine_bounds, tag, visible=None):
    """Coarse and fine as what they do: a multiple of the note, or a range of hertz and a multiple of it."""
    coarse, fine = f"op{n}.coarse", f"op{n}.fine"
    ratio, fixed = both(visible, is_off(f"op{n}.mode")), both(visible, is_on(f"op{n}.mode"))
    return [field(coarse, "Coarse", coarse_bounds, par(coarse), visible=ratio, tag=tag, fill=None, plain=True),
            field(fine, "Fine", fine_bounds, par(fine), visible=ratio, tag=tag, fill=None, plain=True),
            field(coarse, "Range in Hz", coarse_bounds, power_of_ten(fixed_decade(n)), visible=fixed, tag=f"{tag}_hz", fill=None, plain=True),
            field(fine, "Times the range", fine_bounds, power_of_ten(mul(par(fine), 0.01)), "×", 2, visible=fixed, tag=f"{tag}_hz", fill=None, plain=True)]


def op_mark(cid, n, bounds, visible=None):
    """Operator `n`'s colour along the left of `bounds`."""
    return graphic(cid, f"Operator {n}'s colour", bounds, [rect(0.0, 2.0, 3.0, bounds[3] - 4.0, OP_COLORS[n], 1.0)], visible)


def op_toggle(n, bounds, tag):
    """An operator's power: a small light, as six bright buttons in a column would outshout the sound's shape."""
    _, _, w, h = bounds
    cx, cy = w / 2.0, h / 2.0
    drawing = [rect(0.0, 0.0, w, h, CONTROL, 3.0), circle([cx, cy], 5.0, BORDER), circle([cx, cy], mul(par(f"op{n}.on"), 4.0), ACCENT)]
    return {"visible_when": None, "id": f"op{n}.on{tag}", "name": f"Operator {n} on", "tooltip": f"Turn operator {n} on or off", "bounds": bounds,
            "gesture": "toggle", "x": binding(f"op{n}.on"), "y": None, "drawing": drawing}


def op_row(n, columns, y, h, tag):
    """One operator's row: its number, ratio or Hz with what it comes to, coarse, fine, level, on. The
    row is lighter while selected, and at a fixed frequency its tuning stands out. An operator not in
    the sound shows only its number and its light, and the first such row offers to add it."""
    cell = lambda name: [columns[name][0], y + 1.5, columns[name][1], h - 3.0]
    x, width = columns["select"][0], columns["on"][0] + columns["on"][1] - columns["select"][0]
    on = is_on(f"op{n}.on")
    first_off = is_off(f"op{n}.on") if n == 1 else both(is_off(f"op{n}.on"), is_on(f"op{n - 1}.on"))
    mode_x = columns["mode"][0]
    tuning_w = columns["fine"][0] + columns["fine"][1] - mode_x
    add_bounds = [mode_x, y + 1.5, columns["level"][0] + columns["level"][1] - mode_x, h - 3.0]
    add_operator = button(f"op{n}_add{tag}", f"op{n}.on", "+ Add operator", add_bounds, visible=first_off)
    add_operator["name"] = f"Add operator {n}"
    controls = [
        graphic(f"op{n}_band{tag}", f"Operator {n} selected", [x - 1.0, y, width + 2.0, h], [rect(0.0, 0.0, width + 2.0, h, tint(TEXT, 22), 3.0)], both(on, selected(n))),
        {"widget": {"kind": "button", "label": str(n), "off_label": None, "indicator": False}, "visible_when": None,
         "id": f"select_op{n}{tag}", "name": f"Select operator {n}", "tooltip": f"Select operator {n}", "bounds": cell("select"), "gesture": {"set": n},
         "x": {"parameter": "panel.op", "logarithmic": False, "drag_scale": 1.0}, "y": None, "drawing": []},
        graphic(f"op{n}_fixed{tag}", f"Operator {n} stays at one frequency", [mode_x - 1.0, y, tuning_w + 2.0, h], [rect(0.0, 0.0, tuning_w + 2.0, h, tint(HIGHLIGHT, 60), 3.0)],
                both(on, is_on(f"op{n}.mode"))),
        button(f"op{n}.mode{tag}", f"op{n}.mode", "Hz", cell("mode"), visible=on, off_label="Ratio"),
        op_mark(f"op{n}_mark{tag}", n, [cell("select")[0], y + 1.5, 8.0, h - 3.0]),
    ]
    controls += op_frequency(n, cell("freq"), tag, on)
    controls += op_tuning(n, cell("coarse"), cell("fine"), tag, on)
    controls.append(field(f"op{n}.level", "Level", cell("level"), par(f"op{n}.level"), tag=tag, plain=True, visible=on))
    controls.append(op_toggle(n, cell("on"), tag))
    controls.append(add_operator)
    return controls


def op_table(columns, y, row_h, tag):
    """The operators as a table: its columns named once, the rows only values."""
    names = [("mode", "Pitch"), ("coarse", "Coarse"), ("fine", "Fine"), ("level", "Level")]
    x0 = columns["select"][0]
    draws = [text(columns[c][0] - x0 + columns[c][1] / 2.0, 0.0, name, CAPTION, 10.0, center=True) for c, name in names]
    width = columns["on"][0] + columns["on"][1] - x0
    controls = [graphic(f"op_heads{tag}", "Operator columns", [x0, y, width, 14.0], draws)]
    for n in OPS:
        controls += op_row(n, columns, y + 15.0 + (n - 1) * row_h, row_h, tag)
    return controls


def algorithm_pictures(x, y, w, h, tag, size, visible=None):
    """The chosen algorithm's picture, whose operators select when clicked."""
    controls = []
    for i, flags in enumerate(TABLES):
        shown = both(visible, equal(par("algorithm"), float(i + 1)))
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
        3: [("lfo2ByKey", "By key", percent("lfo2ByKey"), " %", 0, False), ("lfo2ByVel", "By vel", percent("lfo2ByVel"), " %", 0, False), None, None,
            ("lfoEnv.attack", "Attack", ms("lfoEnv.attack"), " ms", 0, True), ("lfoEnv.decay", "Decay", ms("lfoEnv.decay"), " ms", 0, True),
            ("lfoEnv.sustain", "Sustain", par("lfoEnv.sustain"), "", 2, False), ("lfoEnv.release", "Release", ms("lfoEnv.release"), " ms", 0, True)],
        4: [("pitchEnv.initial", "Initial", par("pitchEnv.initial"), " st", 1, False), ("pitchEnv.attack", "Attack", ms("pitchEnv.attack"), " ms", 0, True),
            ("pitchEnv.peak", "Peak", par("pitchEnv.peak"), " st", 1, False), ("pitchEnv.decay", "Decay", ms("pitchEnv.decay"), " ms", 0, True),
            ("pitchEnv.sustain", "Sustain", par("pitchEnv.sustain"), " st", 1, False), ("pitchEnv.release", "Release", ms("pitchEnv.release"), " ms", 0, True),
            ("pitchEnv.end", "End", par("pitchEnv.end"), " st", 1, False), ("pitchEnvAmount", "Amount", percent("pitchEnvAmount"), " %", 0, False)],
        5: [("feedback", "Feedback", par("feedback"), "", 0, False)],
        8: [("random.amount", "Amount", percent("random.amount"), " %", 0, False), ("random.seed", "Seed", par("random.seed"), "", 0, False)],
        7: [("voices", "Voices", par("voices"), "", 0, False), ("portamentoTime", "Glide", par("portamentoTime"), "", 0, False),
            ("pitchBendRange", "Bend", par("pitchBendRange"), " st", 0, False), ("transpose", "Transp.", par("transpose"), " st", 0, False),
            ("pan", "Pan", percent("pan"), " %", 0, False), ("panByKey", "By key", percent("panByKey"), " %", 0, False),
            ("panRandom", "Random", percent("panRandom"), " %", 0, False), ("tone", "Tone", percent("tone"), " %", 0, False)],
    }
    controls = []
    for page, cells in pages.items():
        shown = equal(par("panel.page"), float(page))
        for i, cell in enumerate(cells):
            if cell is None:
                continue
            pid, caption, value, suffix, decimals, log = cell
            controls.append(dial(pid, caption, x + (i % 4) * 50.0, y + (i // 4) * 80.0, value, suffix, decimals, log, shown, f"{tag}_p{page}"))
    # The filter's dials follow its picture.
    filter_page = equal(par("panel.page"), 2.0)
    response = both(filter_page, equal(par("panel.filterView"), 0.0))
    shaped = both(filter_page, equal(par("panel.filterView"), 1.0))
    cell = lambda i: (x + (i % 4) * 50.0, y + (i // 4) * 80.0)
    for i, (pid, caption, value, suffix, decimals, log) in enumerate((
            ("filterDrive", "Drive", par("filterDrive"), " dB", 1, False), ("filterMorph", "Morph", percent("filterMorph"), " %", 0, False),
            ("filterEnv", "Env", percent("filterEnv"), " %", 0, False), ("filterByVel", "By vel", percent("filterByVel"), " %", 0, False),
            ("filterByKey", "By key", percent("filterByKey"), " %", 0, False), ("shaperDrive", "Sh. drive", par("shaperDrive"), " dB", 1, False),
            ("shaperMix", "Sh. mix", percent("shaperMix"), " %", 0, False))):
        controls.append(dial(pid, caption, *cell(i), value, suffix, decimals, log, response, f"{tag}_p2"))
    controls += named_menu("shaper", "Shape", [x + 152.0, y + 88.0, 48.0, 38.0], response, f"{tag}_p2")
    for i, (pid, caption, value, suffix, decimals, log) in enumerate((
            ("filterEnv.attack", "Attack", ms("filterEnv.attack"), " ms", 0, True), ("filterEnv.decay", "Decay", ms("filterEnv.decay"), " ms", 0, True),
            ("filterEnv.sustain", "Sustain", par("filterEnv.sustain"), "", 2, False), ("filterEnv.release", "Release", ms("filterEnv.release"), " ms", 0, True),
            ("filterEnv.initial", "Initial", par("filterEnv.initial"), "", 2, False), ("filterEnv.peak", "Peak", par("filterEnv.peak"), "", 2, False),
            ("filterEnv.end", "End", par("filterEnv.end"), "", 2, False), ("filterEnv", "Amount", percent("filterEnv"), " %", 0, False))):
        controls.append(dial(pid, caption, *cell(i), value, suffix, decimals, log, shaped, f"{tag}_p2e"))
    # What does nothing right now looks it.
    controls.append(dim(f"dim_morph{tag}", [x + 50.0, y, 50.0, 70.0], both(response, sub(1.0, equal(par("filterType"), 4.0)))))
    controls.append(dim(f"dim_drive{tag}", [x, y, 50.0, 70.0], both(response, equal(par("filterCircuit"), 0.0))))
    controls.append(dim(f"dim_shaper{tag}", [x + 50.0, y + 80.0, 100.0, 70.0], both(response, equal(par("shaper"), 0.0))))
    return controls


def header_title(cid, caption, x, y, visible):
    return graphic(cid, caption, [x, y, 100.0, 18.0], [text(6.0, 3.0, caption, CAPTION, 11.0)], visible)


def display(x, y, w, h, tag):
    """The one display: a line of what the page chooses, the page's picture under it."""
    controls = []
    page = lambda i: equal(par("panel.page"), float(i))
    head_y, body_y, body_h = y + 2.0, y + 22.0, h - 22.0
    controls.append(graphic(f"display_ground{tag}", "Display", [x, y, w, h], [rect(0.0, 0.0, w, h, DISPLAY, 3.0)], sub(1.0, page(6))))
    end = x + w - 42.0
    for n in OPS:
        shown = both(selected(n), page(0))
        time = both(shown, is_on(f"op{n}.envMode"))
        controls.append(header_title(f"env_title_{n}{tag}", f"Operator {n}", x, y + 1.0, shown))
        controls.append(choose(f"mode_time_{n}{tag}", f"op{n}.envMode", "ADSR", 1, [x + 76.0, head_y, 38.0, 18.0], shown))
        controls.append(choose(f"mode_rates_{n}{tag}", f"op{n}.envMode", "Rates", 0, [x + 116.0, head_y, 38.0, 18.0], shown))
        controls.append(label_beside(f"loop_name_{n}{tag}", "Loop", x + 158.0, head_y, 26.0, 18.0, time))
        controls.append(menu(f"op{n}.env.loop", "Loop", [x + 184.0, head_y, 60.0, 18.0], time, tag))
        controls += op_envelope(n, tag, x, body_y, w, body_h, shown)
        waved = both(selected(n), page(1))
        controls.append(header_title(f"wave_title_{n}{tag}", f"Operator {n}", x, y + 1.0, waved))
        controls.append(menu(f"op{n}.partials", "Partials", [x + 76.0, head_y, 48.0, 18.0], waved, tag))
        controls.append(menu(f"op{n}.wave", "Wave", [x + 128.0, head_y, 116.0, 18.0], waved, tag))
        controls += partial_bars(n, x + 6.0, body_y + 2.0, w - 12.0, body_h - 8.0, tag, waved)
        controls.append(bars_note(f"bars_note_{n}{tag}", x + 2.0, body_y, w - 4.0, body_h - 2.0, n, waved))
    # Filter: its response or its envelope in one picture.
    response = both(page(2), equal(par("panel.filterView"), 0.0))
    shaped = both(page(2), equal(par("panel.filterView"), 1.0))
    controls.append(header_title(f"filter_title{tag}", "Filter", x, y + 1.0, page(2)))
    controls.append(choose(f"filter_view_0{tag}", "panel.filterView", "Response", 0, [x + 56.0, head_y, 62.0, 18.0], page(2)))
    controls.append(choose(f"filter_view_1{tag}", "panel.filterView", "Envelope", 1, [x + 120.0, head_y, 62.0, 18.0], page(2)))
    controls += filter_picture(x, body_y, w, body_h, tag, response)
    controls.append(graphic(f"filter_env_picture{tag}", "Filter envelope", [x, body_y, w, body_h], env_picture("filterEnv.", w, body_h, 1.0, 0.0, MODULATOR, [0.5]), shaped))
    controls += env_points("filterEnv.", tag, x, body_y, w, body_h, shaped, 1.0, 0.0)
    # LFO
    sends = body_h - 22.0
    controls.append(header_title(f"lfo_title{tag}", "LFO", x, y + 1.0, page(3)))
    controls += lfo_picture(x, body_y, w, sends, tag, page(3))
    controls.append(label_beside(f"lfo_to{tag}", "To", x + 6.0, body_y + sends + 2.0, 20.0, 18.0, page(3)))
    for n in OPS:
        at = x + 26.0 + (n - 1) * 33.0
        controls.append(toggle(f"lfo2ToOp{n}", str(n), [at, body_y + sends + 2.0, 31.0, 18.0], page(3), tag))
        controls.append(op_mark(f"lfo2ToOp{n}_mark{tag}", n, [at, body_y + sends + 2.0, 8.0, 18.0], page(3)))
    controls.append(toggle("lfo2ToFilter", "Filter", [x + 226.0, body_y + sends + 2.0, 58.0, 18.0], page(3), tag))
    # Pitch
    controls.append(switch("pitchEnvOn", "Pitch envelope", [x + 2.0, head_y, 30.0, 18.0], page(4), tag))
    controls.append(header_title(f"pitch_title{tag}", "Pitch envelope", x + 32.0, y + 1.0, page(4)))
    controls.append(graphic(f"pitch_picture{tag}", "Pitch envelope", [x, body_y, w, body_h],
                            env_picture("pitchEnv.", w, body_h, 48.0, -48.0, HIGHLIGHT, [24.0, 0.0, -24.0]), page(4)))
    controls += env_points("pitchEnv.", tag, x, body_y, w, body_h, page(4), 48.0, -48.0)
    # Algorithm
    controls.append(header_title(f"algorithm_title{tag}", "Algorithm", x, y + 1.0, page(5)))
    controls.append(stepper("algorithm", "Algorithm", [x + 76.0, head_y, 60.0, 18.0], tag, page(5)))
    controls += algorithm_pictures(x, body_y, w, body_h, tag, 12.0, page(5))
    # Voice
    cells = [("retrigger", "Retrigger"), ("portamento", "Glide"), ("glissando", "Glissando"), ("antialias", "Antialias"), ("interpolation", "Interpolation")]
    controls.append(header_title(f"voice_title{tag}", "Voices and pitch", x, y + 1.0, page(7)))
    for i, (pid, caption) in enumerate(cells):
        controls.append(toggle(pid, caption, [x + 6.0 + (i % 3) * 92.0, body_y + 4.0 + (i // 3) * 34.0, 88.0, 30.0], page(7), tag))
    controls.append(field("pitchBendStep", "Bend step", [x + 190.0, body_y + 38.0, 88.0, 30.0], par("pitchBendStep"), " st", visible=page(7), tag=tag))
    # Random
    controls.append(header_title(f"random_title{tag}", "Random", x, y + 1.0, page(8)))
    controls.append(graphic(f"random_note{tag}", "What Random changes", [x, body_y, w, body_h],
                            [text(w / 2.0, body_h / 2.0 - 7.0, "What it changes is in the larger view", CAPTION, 10.0, center=True)], page(8)))
    # The rest of each page, in the same place: the larger view shows the same page.
    for i in (0, 1, 2, 3, 4, 5, 7, 8):
        controls.append(more(f"more_{i}{tag}", i, [end, head_y, 40.0, 18.0], page(i)))
    return controls


def mod_page(x, y, tag):
    """Every source's first route, edited on the device row; the second routes are in the larger view."""
    shown = equal(par("panel.page"), 6.0)
    controls = [graphic(f"mod_heads{tag}", "Modulation columns", [x, y, 488.0, 13.0],
                        [t for col in (0.0, 245.0) for t in (text(col + 2.0, 0.0, "Source", CAPTION, 10.0), text(col + 62.0, 0.0, "Goes to", CAPTION, 10.0),
                                                             text(col + 150.0, 0.0, "Amount", CAPTION, 10.0))], shown)]
    for i, (source, name) in enumerate(MATRIX):
        cx, cy = x + (i // 5) * 245.0, y + 14.0 + (i % 5) * 26.0
        controls.append(label_beside(f"mod_{source}_name{tag}", name, cx + 2.0, cy, 58.0, 24.0, shown))
        controls.append(menu(f"mod.{source}.dest1", f"{name} goes to", [cx + 60.0, cy + 1.0, 86.0, 22.0], shown, tag))
        controls.append(field(f"mod.{source}.amount1", f"{name} amount", [cx + 150.0, cy + 1.0, 88.0, 22.0], percent(f"mod.{source}.amount1"), " %",
                              visible=shown, tag=tag, fill="centre", cid=f"mod.{source}.amount1{tag}", plain=True))
    controls.append(more(f"more_6{tag}", 6, [x + 245.0 + 60.0, y + 14.0 + 4.0 * 26.0 + 1.0, 178.0, 22.0], shown, "Second routes…"))
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


def stepper(pid, caption, bounds, tag="", visible=None):
    """A number with arrows that step it, as its neighbours have nothing in common but their numbers."""
    return {"widget": {"kind": "stepper", "decimals": 0}, "visible_when": visible, "id": f"{pid}{tag}", "name": caption,
            "tooltip": f"{caption}: click the arrows, or drag up or down", "bounds": bounds, "gesture": "drag", "x": None,
            "y": binding(pid), "drawing": []}


def main_blocks(x, tag):
    """The LFO's and the filter's main controls, in view whatever the display shows."""
    y = 2.0
    controls = [switch("lfoOn", "LFO", [x, y, 30.0, 18.0], tag=tag), label_beside("lfo_main_name", "LFO", x + 36.0, y, 40.0, 18.0),
                menu("lfo2Wave", "LFO wave", [x, y + 20.0, 82.0, 18.0], tag=tag), menu("lfo2Range", "LFO rate in", [x, y + 40.0, 82.0, 18.0], tag=tag),
                toggle("lfo2Retrigger", "Retrigger", [x, y + 60.0, 82.0, 18.0], tag=tag)]
    controls += lfo_rate_knob(x + 84.0, y, 46.0, 70.0, None, tag)
    controls.append(dial("lfo2Amount", "Amount", x + 130.0, y, percent("lfo2Amount"), " %", tag=tag, w=46.0))
    y = 84.0
    controls += [switch("filterOn", "Filter", [x, y, 30.0, 18.0], tag=tag), label_beside("filter_main_name", "Filter", x + 36.0, y, 40.0, 18.0),
                 menu("filterType", "Filter type", [x, y + 20.0, 82.0, 18.0], tag=tag), menu("filterSlope", "Filter slope", [x, y + 40.0, 82.0, 18.0], tag=tag),
                 menu("filterCircuit", "Filter circuit", [x, y + 60.0, 82.0, 18.0], tag=tag)]
    controls.append(dial("filterFreq", "Freq", x + 84.0, y, par("filterFreq"), " Hz", 0, True, tag=tag, w=46.0))
    controls.append(dial("filterRes", "Res", x + 130.0, y, percent("filterRes"), " %", tag=tag, w=46.0))
    return controls


def compact():
    w, h = COMPACT
    controls = op_table(ROW_COMPACT, 1.0, 25.0, "")
    controls.append(separator("sep_ops", 255.0, 4.0, h - 8.0))
    controls += tabs("panel.page", PAGES, 260.0, 2.0, 36.0, 18.0, "page")
    controls += tab_marks(260.0, 2.0, 36.0, "page", [(2, "Filter", "filterOn"), (3, "LFO", "lfoOn"), (4, "Pitch", "pitchEnvOn")])
    controls += display(260.0, 22.0, 288.0, 142.0, "")
    controls += mod_page(260.0, 22.0, "")
    for n in OPS:
        controls += op_dials(n, 552.0, 4.0, "")
    controls += global_dials(552.0, 4.0, "")
    controls.append(algorithm_legend("algorithm_legend", 552.0, 84.0, equal(par("panel.page"), 5.0)))
    controls.append(separator("sep_dials", 755.0, 4.0, h - 8.0))
    controls += main_blocks(759.0, "_main")
    controls.append(separator("sep_main", 937.0, 4.0, h - 8.0))
    controls.append(dial("spread", "Spread", 941.0, 4.0, percent("spread"), " %"))
    controls.append(dial("volume", "Volume", 941.0, 84.0, par("volume"), " dB", 1))
    controls.append(random_button("random", "Random", [995.0, 4.0, 56.0, 24.0]))
    controls.append(field("random.amount", "Amount", [995.0, 32.0, 56.0, 32.0], percent("random.amount"), " %"))
    controls.append(more("random_settings", 8, [995.0, 68.0, 56.0, 20.0]))
    controls.append(meter("meter", [1053.0, 4.0, 24.0, h - 8.0]))
    return {"size": [w, h], "viewport_size": None, "background": PANEL, "assets": [], "layers": [], "controls": controls,
            "hidden_parameters": every_parameter(), "accent_on": True,
            "animate_when": both(equal(par("panel.page"), 3.0), is_on("lfoOn"))}


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


COLUMN_A, COLUMN_B, COLUMN_C = 4.0, 612.0, 1020.0
WIDE_A, WIDE_B, WIDE_C = 600.0, 400.0, 376.0
BODY = 222.0


def on_page(index):
    return equal(par("panel.page"), float(index))


def env_rest(prefix):
    """What the dials on the device row leave of a time envelope."""
    return [f_num(f"{prefix}end", "End", " dB", 1), f_menu(f"{prefix}loop", "Loop"), f_menu(f"{prefix}repeat", "Repeat"), f_ms(f"{prefix}loopTime", "Loop time"),
            f_both(f"{prefix}attackSlope", "Att. slope"), f_both(f"{prefix}decaySlope", "Dec. slope"), f_both(f"{prefix}releaseSlope", "Rel. slope")]


def env_fields(prefix, unit):
    """Every number of an envelope as a field, for the envelopes whose dials are elsewhere."""
    return [lambda b, v, t, c=c: [field(c[0], c[1], b, c[2], c[3], c[4], c[5], v, t)] for c in env_knobs(prefix, unit)] + [
        f_menu(f"{prefix}loop", "Loop"), f_menu(f"{prefix}repeat", "Repeat"), f_ms(f"{prefix}loopTime", "Loop time"), f_both(f"{prefix}timeByVel", "By vel"),
        f_pct(f"{prefix}timeByKey", "By key"), f_both(f"{prefix}attackSlope", "Att. slope"), f_both(f"{prefix}decaySlope", "Dec. slope"), f_both(f"{prefix}releaseSlope", "Rel. slope")]


def expanded_envelope(y, tag):
    """The selected operator's envelope large, with what the dials leave and its keyboard scaling."""
    controls = []
    for n in OPS:
        shown = both(on_page(0), selected(n))
        p = lambda f: f"op{n}.{f}"
        time = both(shown, is_on(p("envMode")))
        controls.append(graphic(f"x_env_heard_{n}", f"Operator {n}: heard", [COLUMN_A + 6.0, y + 4.0, 200.0, 14.0], [text(0.0, 0.0, f"Operator {n}: heard", CAPTION, 10.0)], both(shown, heard(n))))
        controls.append(graphic(f"x_env_mod_{n}", f"Operator {n}: shapes others", [COLUMN_A + 6.0, y + 4.0, 200.0, 14.0], [text(0.0, 0.0, f"Operator {n}: shapes others", CAPTION, 10.0)], both(shown, modulates(n))))
        controls = op_envelope(n, tag, COLUMN_A, y, WIDE_A, BODY, shown) + controls
        controls.append(section(f"x_env_title_{n}", "Envelope", COLUMN_B, y, WIDE_B, time))
        controls += grid(env_rest(f"op{n}.env."), COLUMN_B, y + 18.0, time, tag, columns=4, w=97.0)
        controls.append(section(f"x_key_{n}", "Keyboard", COLUMN_C, y, WIDE_C, shown))
        controls += grid([f_num(p("breakPoint"), "Break pt"), f_num(p("leftDepth"), "Left"), f_menu(p("leftCurve"), "Curve"), f_num(p("rightDepth"), "Right"), f_menu(p("rightCurve"), "Curve")],
                         COLUMN_C, y + 18.0, shown, tag, columns=5, w=72.0)
    return controls


def expanded_wave(y, tag):
    """The selected operator's partials large, with how they are drawn and how its wave starts."""
    controls = []
    x, w = 874.0, 522.0
    for n in OPS:
        shown = both(on_page(1), selected(n))
        p = lambda f: f"op{n}.{f}"
        controls.append(graphic(f"x_bars_bg_{n}", "Partials", [COLUMN_A, y, 860.0, BODY], [rect(0.0, 0.0, 860.0, BODY, DISPLAY, 3.0)], shown))
        controls += partial_bars(n, COLUMN_A + 4.0, y + 4.0, 852.0, BODY - 8.0, tag, shown)
        controls.append(bars_note(f"x_bars_note_{n}", COLUMN_A + 2.0, y + 2.0, 856.0, BODY - 4.0, n, shown))
        controls.append(section(f"x_bars_title_{n}", "Partials", x, y, w, shown))
        controls += grid([f_menu(p("repeat"), "Repeat"), f_menu(p("oddEven"), "Use"), f_toggle(p("normalize"), "Normalize")], x, y + 18.0, shown, tag, columns=3, w=170.0)
        controls.append(section(f"x_osc_{n}", "Oscillator", x, y + 60.0, w, shown))
        controls += grid([f_toggle(p("retrigger"), "Retrigger"), f_toggle(p("freqQuantize"), "Quantize")], x, y + 78.0, shown, tag, columns=2, w=170.0)
    return controls


def expanded_filter(y, tag):
    shown = on_page(2)
    controls = filter_picture(COLUMN_A, y, WIDE_A, BODY, tag, shown)
    controls.append(section("x_filter_env_title", "Filter envelope", COLUMN_B, y, WIDE_B, shown))
    controls.append(graphic("x_filter_env", "Filter envelope", [COLUMN_B, y + 18.0, WIDE_B, 86.0], env_picture("filterEnv.", WIDE_B, 86.0, 1.0, 0.0, MODULATOR, [0.5]), shown))
    controls += env_points("filterEnv.", tag, COLUMN_B, y + 18.0, WIDE_B, 86.0, shown, 1.0, 0.0)
    controls += grid(env_fields("filterEnv.", ""), COLUMN_B, y + 108.0, shown, tag, columns=8, w=46.0, h=34.0, gap=4.0)
    controls.append(section("x_shaper_title", "Shaper", COLUMN_C, y, WIDE_C, shown))
    return controls + grid([f_menu("shaper", "Shape"), f_num("shaperDrive", "Drive", " dB", 1), f_pct("shaperMix", "Dry/wet")], COLUMN_C, y + 18.0, shown, tag, columns=3, w=122.0)


def expanded_lfo(y, tag):
    shown = on_page(3)
    controls = lfo_picture(COLUMN_A, y, WIDE_A, BODY, tag, shown)
    controls.append(section("x_lfo_env_title", "LFO envelope", COLUMN_B, y, WIDE_B, shown))
    controls.append(graphic("x_lfo_env", "LFO envelope", [COLUMN_B, y + 18.0, WIDE_B, 86.0], env_picture("lfoEnv.", WIDE_B, 86.0, 1.0, 0.0, SPECTRUM_COLOR, [0.5]), shown))
    controls += env_points("lfoEnv.", tag, COLUMN_B, y + 18.0, WIDE_B, 86.0, shown, 1.0, 0.0)
    controls += grid(env_fields("lfoEnv.", ""), COLUMN_B, y + 108.0, shown, tag, columns=8, w=46.0, h=34.0, gap=4.0)
    x, w = COLUMN_C, WIDE_C
    controls.append(section("x_lfo_also", "Also to", x, y, w, shown))
    controls.append(toggle("lfo2Retrigger", "Retrigger", [x, y + 18.0, 90.0, 34.0], shown, tag))
    controls += grid([f_menu("lfo2DestB", "Destination"), f_both("lfo2AmountB", "Amount")], x + 94.0, y + 18.0, shown, tag, columns=2, w=139.0)
    controls.append(section("x_lfo_classic", "Classic LFO", x, y + 60.0, w, shown))
    return controls + grid([f_menu("lfoWave", "Wave"), lambda b, v, t: [field("lfoSpeed", "Rate", b, classic_lfo_hz(), "Hz", 2, visible=v, tag=t)], f_num("lfoDelay", "Delay"), f_num("lfoPitchDepth", "Pitch"),
                            f_num("lfoAmpDepth", "Level"), f_num("pitchModSens", "Pitch sens"), f_toggle("lfoKeySync", "Key sync"), f_toggle("oscKeySync", "Osc sync")],
                           x, y + 78.0, shown, tag, columns=4, w=91.0)


def expanded_pitch(y, tag):
    shown = on_page(4)
    controls = [graphic("x_pitch_env", "Pitch envelope", [COLUMN_A, y, WIDE_A, BODY], env_picture("pitchEnv.", WIDE_A, BODY, 48.0, -48.0, HIGHLIGHT, [24.0, 0.0, -24.0]), shown)]
    controls += env_points("pitchEnv.", tag, COLUMN_A, y, WIDE_A, BODY, shown, 48.0, -48.0)
    controls.append(section("x_pitch_more", "Repeats and keys", COLUMN_B, y, WIDE_B, shown))
    prefix = "pitchEnv."
    controls += grid([f_menu(f"{prefix}loop", "Loop"), f_menu(f"{prefix}repeat", "Repeat"), f_ms(f"{prefix}loopTime", "Loop time"), f_both(f"{prefix}timeByVel", "By vel"),
                      f_pct(f"{prefix}timeByKey", "By key"), f_both(f"{prefix}attackSlope", "Att. slope"), f_both(f"{prefix}decaySlope", "Dec. slope"), f_both(f"{prefix}releaseSlope", "Rel. slope")],
                     COLUMN_B, y + 18.0, shown, tag, columns=4, w=97.0)
    controls.append(section("x_pitch_to", "Goes to", COLUMN_B, y + 100.0, WIDE_B, shown))
    controls += grid([f_toggle(f"pitchEnvToOp{n}", f"Op {n}") for n in OPS] + [f_toggle("pitchEnvToLfo", "To LFO")], COLUMN_B, y + 118.0, shown, tag, columns=7, w=53.0, h=24.0)
    controls += [op_mark(f"pitchEnvToOp{n}_mark", n, [COLUMN_B + (n - 1) * 57.0, y + 118.0, 8.0, 24.0], shown) for n in OPS]
    controls += grid([f_menu("pitchEnvDestB", "Also to"), f_both("pitchEnvAmountB", "Amount")], COLUMN_B, y + 146.0, shown, tag, columns=2, w=198.0)
    controls.append(section("x_pitch_classic", "Classic pitch envelope", COLUMN_C, y, WIDE_C, shown))
    return controls + grid([f_num(f"pitchRate{i}", f"Rate {i}") for i in range(1, 5)] + [f_num(f"pitchLevel{i}", f"Level {i}") for i in range(1, 5)], COLUMN_C, y + 18.0, shown, tag, columns=4, w=91.0)


def table_heads(cid, heads, x, y, w, visible):
    draws = [text(at, 0.0, name, CAPTION, 10.0) for at, name in heads] + [line([0.0, 14.0], [w, 14.0], BORDER)]
    return graphic(cid, "Columns", [x, y, w, 15.0], draws, visible)


def expanded_algorithm(y, tag):
    """Every algorithm as a card that takes the click anywhere on it, and the chosen one large."""
    shown = on_page(5)
    x, per_row = COLUMN_A, 16
    step, cell_w, cell_h = 65.0, 62.0, 56.0
    four_at = y + 18.0 + 2 * 60.0 + 2.0
    controls = [section("x_algorithms_six", "Six operators", x, y, per_row * step - 3.0, shown), section("x_algorithms_four", "Four operators", x, four_at, per_row * step - 3.0, shown)]
    for i, flags in enumerate(TABLES):
        top = y + 18.0 if i < 32 else four_at + 18.0
        row = i if i < 32 else i - 32
        cx, cy = x + (row % per_row) * step, top + (row // per_row) * 60.0
        on = equal(par("algorithm"), float(i + 1))
        drawing = [rect(0.0, 0.0, cell_w, cell_h, DISPLAY, 3.0), rect(0.0, cell_h - 13.0, mul(on, cell_w), 13.0, ACCENT, 2.0)]
        drawing += picture(flags, cell_w - 8.0, cell_h - 22.0, 6.0)
        drawing.append(text(cell_w / 2.0, cell_h - 13.0, str(i + 1), TEXT, 10.0, center=True))
        controls.append({"visible_when": shown, "id": f"x_pick_{i + 1}", "name": f"Algorithm {i + 1}", "tooltip": f"Use algorithm {i + 1}", "bounds": [cx, cy, cell_w, cell_h],
                         "gesture": {"set": i + 1}, "x": binding("algorithm"), "y": None, "drawing": drawing})
    x, w = 1060.0, 336.0
    controls.append(section("x_algorithm_title", "Algorithm", x, y, w, shown))
    controls.append(stepper("algorithm", "Algorithm", [x + w - 52.0, y - 3.0, 52.0, 17.0], tag, shown))
    return controls + algorithm_pictures(x, y + 18.0, w, BODY - 18.0, tag, 18.0, shown)


def expanded_matrix(y, tag):
    shown = on_page(6)
    x, w = COLUMN_A, 640.0
    columns = (90.0, 262.0, 340.0, 562.0)
    controls = [table_heads("x_matrix_heads", [(0.0, "Source"), (columns[0], "Goes to"), (columns[1], "Amount"), (columns[2], "Also to"), (columns[3], "Amount")], x, y, w, shown)]
    for i, (source, name) in enumerate(MATRIX):
        row = y + 18.0 + i * 22.5
        controls.append(graphic(f"x_matrix_{source}", name, [x, row, 86.0, 21.0], [text(0.0, 4.0, name, TEXT, 11.0)], shown))
        controls.append(menu(f"mod.{source}.dest1", f"{name} goes to", [x + columns[0], row, 168.0, 21.0], shown, tag))
        controls.append(field(f"mod.{source}.amount1", f"{name} amount", [x + columns[1], row, 72.0, 21.0], percent(f"mod.{source}.amount1"), " %", visible=shown, tag=tag, fill="centre", plain=True))
        controls.append(menu(f"mod.{source}.dest2", f"{name} also to", [x + columns[2], row, 218.0, 21.0], shown, tag))
        controls.append(field(f"mod.{source}.amount2", f"{name} second amount", [x + columns[3], row, 78.0, 21.0], percent(f"mod.{source}.amount2"), " %", visible=shown, tag=tag, fill="centre", plain=True))
    x, w = 670.0, 430.0
    controls.append(section("x_routes_title", "Classic controllers", x, y, w, shown))
    controls.append(table_heads("x_routes_heads", [(0.0, "Controller"), (84.0, "Range"), (168.0, "Goes to")], x, y + 18.0, w, shown))
    for i, (s, name) in enumerate((("wheel", "Mod wheel"), ("foot", "Foot"), ("breath", "Breath"), ("pressure", "Pressure"))):
        row = y + 36.0 + i * 28.0
        controls.append(graphic(f"x_routes_{s}", name, [x, row, 80.0, 24.0], [text(0.0, 5.0, name, TEXT, 11.0)], shown))
        controls.append(field(f"{s}Range", f"{name} range", [x + 84.0, row, 80.0, 24.0], par(f"{s}Range"), visible=shown, tag=tag, plain=True))
        for j, (f, caption) in enumerate((("Pitch", "Pitch"), ("Amp", "Level"), ("EgBias", "Envelope"))):
            controls.append(toggle(f"{s}{f}", caption, [x + 168.0 + j * 84.0, row, 80.0, 24.0], shown, tag))
    return controls


def expanded_output(y, tag):
    """What belongs to the whole device, in columns across the strip."""
    shown = on_page(7)
    w, step = 330.0, 65.0
    x = 4.0
    controls = [section("x_output", "Output", x, y, w, shown)]
    controls += knob_row([("volume", "Volume", par("volume"), " dB", 1, False), ("tone", "Tone", percent("tone"), " %", 0, False),
                          ("time", "Time", percent("time"), " %", 0, False), ("pan", "Pan", percent("pan"), " %", 0, False)], x, y + 18.0, shown, tag, step)
    controls.append(section("x_quality", "Quality", x, y + 94.0, w, shown))
    controls += grid([f_toggle("antialias", "Antialias"), f_toggle("interpolation", "Interpolation")], x, y + 112.0, shown, tag, columns=2, w=136.0, h=26.0)
    x += 350.0
    controls.append(section("x_stereo", "Stereo", x, y, w, shown))
    controls += knob_row([("spread", "Spread", percent("spread"), " %", 0, False), ("panByKey", "Pan by key", percent("panByKey"), " %", 0, False),
                          ("panRandom", "Pan random", percent("panRandom"), " %", 0, False)], x, y + 18.0, shown, tag, step)
    x += 350.0
    controls.append(section("x_voices", "Voices", x, y, w, shown))
    controls += knob_row([("voices", "Voices", par("voices"), "", 0, False), ("portamentoTime", "Glide time", par("portamentoTime"), "", 0, False)], x, y + 18.0, shown, tag, step)
    controls += grid([f_toggle("retrigger", "Retrigger"), f_toggle("portamento", "Glide"), f_toggle("glissando", "Glissando")], x, y + 94.0, shown, tag, columns=3, w=90.0, h=26.0, gap=5.0)
    x += 350.0
    controls.append(section("x_pitch", "Pitch", x, y, w, shown))
    return controls + knob_row([("transpose", "Transpose", par("transpose"), " st", 0, False), ("pitchBendRange", "Bend range", par("pitchBendRange"), " st", 0, False),
                                ("pitchBendStep", "Bend step", par("pitchBendStep"), " st", 0, False)], x, y + 18.0, shown, tag, step)


def expanded_random(y, tag):
    shown = on_page(8)
    x, w = COLUMN_A, 700.0
    controls = [section("x_random_title", "Randomizer", x, y, w, shown), random_button("x_random_now", "Random", [x, y + 18.0, 120.0, 34.0], shown)]
    controls += grid([f_pct("random.amount", "Amount"), f_num("random.seed", "Seed", fill=None)], x + 124.0, y + 18.0, shown, f"{tag}_page", columns=2, w=120.0)
    controls.append(graphic("x_random_note", "How the randomizer moves values", [x, y + 56.0, w, 14.0],
                            [text(0.0, 0.0, "A value moves from where it is, by at most its section's share of its range.", CAPTION, 10.0)], shown))
    for column in range(2):
        cx = x + column * 350.0
        controls.append(table_heads(f"x_random_heads_{column}", [(0.0, "Section"), (120.0, "Share"), (224.0, "Random")], cx, y + 72.0, 330.0, shown))
    for i, (sid, name) in enumerate(SECTIONS):
        cx, row = x + (i // 5) * 350.0, y + 90.0 + (i % 5) * 26.0
        controls.append(graphic(f"x_section_{sid}", name, [cx, row, 116.0, 24.0], [text(0.0, 5.0, name, TEXT, 11.0)], shown))
        controls.append(field(f"random.{sid}.amount", f"{name} share", [cx + 120.0, row, 100.0, 24.0], percent(f"random.{sid}.amount"), " %", visible=shown, tag=tag, plain=True))
        controls.append(toggle(f"random.{sid}.on", "Changes", [cx + 224.0, row, 100.0, 24.0], shown, tag, off_label="Keeps"))
    return controls


def expanded():
    """A strip above the device row, scaled to its width. The row stays in view and chooses the page,
    so the strip shows that page's picture large and only what the row has no room for."""
    w, h = EXPANDED
    y, tag = 4.0, "_x"
    controls = expanded_envelope(y, tag) + expanded_wave(y, tag) + expanded_filter(y, tag) + expanded_lfo(y, tag) + expanded_pitch(y, tag)
    controls += expanded_algorithm(y, tag) + expanded_matrix(y, tag) + expanded_output(y, tag) + expanded_random(y, tag)
    return {"size": [w, h], "viewport_size": None, "background": PANEL, "assets": [], "layers": [], "controls": controls,
            "hidden_parameters": every_parameter(), "strip": True, "accent_on": True,
            "animate_when": both(on_page(3), is_on("lfoOn"))}


# What each parameter does, for its tooltip: a control says what it is for, not how a mouse moves it.
WHAT = {
    "attack": "how long the level takes to rise when a key goes down",
    "decay": "how long it then takes to fall to the sustain level",
    "sustain": "the level held while the key stays down",
    "release": "how long the level takes to fade after the key goes up",
    "initial": "the level when a key goes down",
    "peak": "the level the attack rises to",
    "end": "the level the release ends on",
    "loop": "whether the envelope repeats while a note plays",
    "loopTime": "how long the envelope waits before it repeats",
    "repeat": "how often it repeats",
    "timeByVel": "how much a harder key press shortens the times",
    "timeByKey": "how much higher notes shorten the times",
    "attackSlope": "the curve of the attack",
    "decaySlope": "the curve of the decay",
    "releaseSlope": "the curve of the release",
    "envMode": "which kind of envelope this operator uses",
    "rate1": "how fast the envelope moves to level 1",
    "rate2": "how fast it moves on to level 2",
    "rate3": "how fast it moves on to level 3, held while the key is down",
    "rate4": "how fast it moves to level 4 after the key goes up",
    "level1": "the level reached first",
    "level2": "the level reached second",
    "level3": "the level held while the key stays down",
    "level4": "the level where a note starts and ends",
    "breakPoint": "the key where keyboard scaling changes direction",
    "leftDepth": "how much notes below the break point change the level",
    "rightDepth": "how much notes above the break point change the level",
    "leftCurve": "the shape of the change below the break point",
    "rightCurve": "the shape of the change above the break point",
    "rateScaling": "how much higher notes speed the envelope up",
    "ampModSens": "how much the classic LFO moves this operator's level",
    "velocitySens": "how much a harder key press raises this operator",
    "level": "how loud this operator is; one that shapes others shapes them harder",
    "mode": "Ratio follows the note played; Hz stays at one frequency on every key",
    "coarse": "Ratio: the whole multiple of the note. Hz: the range, 1, 10, 100 or 1000 Hz",
    "fine": "Ratio: adds up to almost one more multiple. Hz: multiplies the range by up to almost 10",
    "detune": "a slight pitch offset, for beating against other operators",
    "on": "adds this operator to the sound or takes it out",
    "wave": "the wave this operator plays",
    "feedback": "how much the output modulates itself, from smooth towards noise",
    "phase": "where in its wave the operator starts a note",
    "retrigger": "starts the wave from its phase on every note",
    "freqByVel": "how much a harder key press changes the pitch",
    "freqQuantize": "keeps a pitch changed by velocity on whole ratios",
    "levelByKey": "how much higher notes change this operator's level",
    "partials": "how many partials you draw",
    "normalize": "keeps the level steady as you draw",
    "oddEven": "which partials are heard: all, odd or even",
    "algorithm": "how the operators connect, who is heard and who shapes whom",
    "oscKeySync": "starts every operator's wave at the same place on each note",
    "lfoSpeed": "how fast the classic LFO runs",
    "lfoDelay": "how long the classic LFO waits after a note starts",
    "lfoPitchDepth": "how much the classic LFO moves the pitch",
    "lfoAmpDepth": "how much the classic LFO moves the level",
    "lfoKeySync": "restarts the classic LFO on each note",
    "lfoWave": "the classic LFO's wave",
    "pitchModSens": "how much the classic LFO's pitch movement is heard",
    "transpose": "moves every note by semitones",
    "pitchBendRange": "how far the pitch wheel bends",
    "pitchBendStep": "bends in steps of this many semitones; 0 bends smoothly",
    "portamento": "glides from one note to the next",
    "glissando": "glides in semitone steps",
    "portamentoTime": "how long a glide takes",
    "volume": "the device's output level",
    "pan": "where the sound sits between left and right",
    "panByKey": "spreads low and high notes across the stereo field",
    "panRandom": "gives each note a random place in the stereo field",
    "spread": "plays two detuned notes panned apart",
    "voices": "how many notes can sound at once; at 1 notes play legato",
    "tone": "brightens or darkens the whole sound",
    "time": "scales the time of every time envelope",
    "antialias": "keeps bright waves from folding into harsh tones",
    "interpolation": "smooths the wave tables, at a small cost in CPU",
    "filterOn": "turns the filter on or off",
    "filterType": "which frequencies the filter lets through",
    "filterSlope": "how steeply the filter cuts",
    "filterCircuit": "the filter's character: clean or driven",
    "filterFreq": "where the filter cuts",
    "filterRes": "how much the filter rings at its frequency",
    "filterMorph": "moves between the filter's shapes, with the Morph type",
    "filterDrive": "how hard the driven circuits are pushed",
    "filterEnv": "how much the filter's envelope moves its frequency",
    "filterByVel": "how much a harder key press opens the filter",
    "filterByKey": "how much higher notes open the filter",
    "shaper": "the shape of the distortion after the filter",
    "shaperDrive": "how hard the shaper is pushed",
    "shaperMix": "how much of the shaped sound is heard",
    "lfoOn": "turns the LFO on or off",
    "lfo2Wave": "the LFO's wave",
    "lfo2Range": "the rate in hertz, in hertz up to audio rate, or locked to the song",
    "lfo2Rate": "how fast the LFO runs",
    "lfo2Sync": "the LFO's cycle as a note length, locked to the song",
    "lfo2Retrigger": "restarts the LFO on each note",
    "lfo2ByKey": "how much higher notes speed the LFO up",
    "lfo2ByVel": "how much a harder key press raises the LFO's amount",
    "lfo2Amount": "how much the LFO moves what it goes to",
    "lfo2ToFilter": "sends the LFO to the filter's frequency",
    "lfo2DestB": "one more thing the LFO moves",
    "lfo2AmountB": "how much it moves that",
    "pitchEnvOn": "turns the pitch envelope on or off",
    "pitchEnvAmount": "how far the pitch envelope bends the pitch",
    "pitchEnvToLfo": "sends the pitch envelope to the LFO's rate",
    "pitchEnvDestB": "one more thing the pitch envelope moves",
    "pitchEnvAmountB": "how much it moves that",
    "dest1": "what this source moves",
    "amount1": "how much it moves that, either way",
    "dest2": "a second thing this source moves",
    "amount2": "how much it moves that, either way",
    "Range": "how far this controller reaches",
    "Pitch": "sends this controller to the pitch",
    "Amp": "sends this controller to the level",
    "EgBias": "sends this controller to the envelopes' level",
    "panel.op": "which operator the panel shows",
    "random.amount": "how far Random moves every value",
    "random.seed": "which result Random gives; the same seed gives the same result",
    "share": "how far Random moves this section's values",
    "takes_part": "whether Random changes this section or keeps it",
}


def what_it_does(pid):
    """The sentence for parameter `pid`, whichever operator or envelope it belongs to."""
    name = re.sub(r"^(op\d\.(env\.)?|filterEnv\.|lfoEnv\.|pitchEnv\.)", "", pid)
    if name.startswith("mod."):
        name = name.split(".")[-1]
    if re.fullmatch(r"(wheel|foot|breath|pressure)(Range|Pitch|Amp|EgBias)", name):
        name = re.sub(r"^(wheel|foot|breath|pressure)", "", name)
    if re.fullmatch(r"random\.\w+\.amount", name) and name != "random.amount":
        name = "share"
    if re.fullmatch(r"random\.\w+\.on", name):
        name = "takes_part"
    if name.startswith("lfo2ToOp"):
        return f"sends the LFO to operator {name[-1]}"
    if name.startswith("pitchEnvToOp"):
        return f"sends the pitch envelope to operator {name[-1]}"
    if re.fullmatch(r"pitchRate\d", name):
        return "how fast the classic pitch envelope moves to this level"
    if re.fullmatch(r"pitchLevel\d", name):
        return "a level of the classic pitch envelope"
    return WHAT[name]


def explain(canvas):
    """Every control that changes a parameter says what it is for."""
    for control in canvas["controls"]:
        bound = control.get("y") or control.get("x")
        gesture = control["gesture"]
        if bound is None or isinstance(gesture, dict) or control.get("widget", {}).get("kind") == "handle":
            continue
        control["tooltip"] = f"{control['name']}: {what_it_does(bound['parameter'])}"


def check_reach(panel):
    """Every parameter has a control on one canvas or the other."""
    bound = set()
    for canvas in (panel["ui"], panel["expanded"]):
        for control in canvas["controls"]:
            bound |= {b["parameter"] for b in (control.get("x"), control.get("y")) if b}
    missing = [pid for pid in every_parameter() if pid not in bound]
    assert not missing, missing[:12]


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
    explain(panel["ui"])
    explain(panel["expanded"])
    check_reach(panel)
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
