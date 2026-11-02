# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Writes devices/fm-synth/panel.json: the device row and the expanded panel (docs/spec/fm-synth.md).

Colours are only the theme's palette and the --device-* tokens: a theme changes the panel's styles
and nothing else (ADR 0034), and this script fails when a drawing has any other colour. Algorithm
pictures come from the engine's table in devices/fm-synth/src/core.rs.
Run from the repository root: python scripts/fm_synth_panel.py
"""

import json
import re
from pathlib import Path

from multiband_panel import (ACCENT, BORDER, CONTROL, DISPLAY, HIGHLIGHT, INVISIBLE, PANEL, SPECTRUM_COLOR, TEXT, add,
                             button, circle, equal, graphic, handle, knob, line, maximum, minimum, mul, par, path, rect,
                             separator, sub, text, tint, value_text)

MODULATOR = [3, 0, 4, 255]
CARRIER = [3, 0, 3, 255]
OPS = range(1, 7)
ROW_H = 28.0
COMPACT = (1180.0, 168.0)
EXPANDED = (1180.0, 640.0)
PAGES = ["Env", "Wave", "Filter", "LFO", "Pitch", "Mod"]
EXPANDED_PAGES = ["Operator", "Filter", "LFO", "Pitch", "Matrix", "Algorithm", "Random"]
SECTIONS = [("envelopes", "Envelopes"), ("levels", "Levels"), ("waves", "Waves"), ("filter", "Filter"), ("lfo", "LFO"),
            ("pitchEnv", "Pitch envelope"), ("modulation", "Modulation"), ("stereo", "Stereo"), ("ratios", "Ratios"),
            ("algorithm", "Algorithm")]
MATRIX = [("velocity", "Velocity"), ("key", "Key"), ("pressure", "Pressure"), ("bend", "Pitch bend"), ("wheel", "Mod wheel"),
          ("foot", "Foot"), ("breath", "Breath"), ("lfo", "LFO"), ("random", "Random")]
ENV_FIELDS = ["attack", "decay", "release", "initial", "peak", "sustain", "end", "loop", "loopTime", "repeat", "timeByVel",
              "timeByKey", "attackSlope", "decaySlope", "releaseSlope"]
TIMES = ("attack", "decay", "release", "loopTime")
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
    f"random.{s}.{f}" for s, _ in SECTIONS for f in ("amount", "on", "lock")]


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
    table = source[source.index("const ALGORITHMS"):source.index("];", source.index("const ALGORITHMS"))]
    rows = re.findall(r"\[(0x[0-9a-f]{2}(?:, 0x[0-9a-f]{2}){5})\]", table)
    return [[int(v, 16) for v in row.split(", ")] for row in rows]


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


def picture(flags, w, h, size):
    """The algorithm as a drawing: wires, modulators outlined, operators heard filled."""
    edges, carriers = routing(flags)
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
            position[n] = ((i + 0.5) * w / len(row) - size / 2.0, y)
    draws = []
    for s, d in edges:
        a, b = position[s], position[d]
        draws.append(line([a[0] + size / 2.0, a[1] + size], [b[0] + size / 2.0, b[1]], tint(TEXT, 120), 1.0))
    for n, (x, y) in position.items():
        heard = n in carriers
        color = CARRIER if heard else MODULATOR
        draws.append(rect(x, y, size, size, color, 2.0))
        if not heard:
            draws.append(rect(x + 1.0, y + 1.0, size - 2.0, size - 2.0, DISPLAY, 1.0))
        if size >= 12.0:
            draws.append(text(x + size / 2.0, y + 1.0, str(n), PANEL if heard else color, max(size - 5.0, 8.0), center=True))
    return draws


def selected(n):
    return equal(par("panel.op"), float(n))


def both(a, b):
    if a is None:
        return b
    if b is None:
        return a
    return mul(a, b)


def choose(cid, pid, caption, value, bounds, visible=None):
    """A button that sets `pid` to `value`, on while it holds it."""
    return button(cid, pid, caption, bounds, gesture={"set": value}, visible=visible)


def field(pid, caption, bounds, value, suffix="", decimals=0, log=False, visible=None, tag="", cid=None, invert=False):
    """A compact drag field: name over value on a control face. Dragging up or down changes `pid`."""
    _, _, w, h = bounds
    drawing = [rect(0.0, 0.0, w, h, CONTROL, 3.0)]
    if h >= 24.0:
        drawing += [text(w / 2.0, 1.0, caption, tint(TEXT, 170), 9.0, center=True),
                    value_text(w / 2.0, 12.0, value, suffix=suffix, size=10.0, decimals=decimals, center=True)]
    else:
        drawing.append(value_text(w / 2.0, 1.0, value, suffix=suffix, size=10.0, decimals=decimals, center=True))
    return {"visible_when": visible, "id": cid or f"{pid}{tag}", "name": caption,
            "tooltip": f"{caption}: drag up or down; double click resets", "bounds": bounds, "gesture": "drag", "x": None,
            "y": {"parameter": pid, "logarithmic": log, "drag_scale": -1.0 if invert else 1.0}, "drawing": drawing}


def menu(pid, caption, bounds, visible=None, tag=""):
    return {"widget": {"kind": "menu"}, "visible_when": visible, "id": f"{pid}{tag}", "name": caption, "tooltip": caption,
            "bounds": bounds, "gesture": "menu", "x": {"parameter": pid, "logarithmic": False, "drag_scale": 1.0}, "y": None,
            "drawing": []}


def random_button(cid, caption, bounds, visible=None):
    return {"widget": {"kind": "button", "label": caption, "off_label": None, "indicator": False}, "visible_when": visible,
            "id": cid, "name": caption, "tooltip": "Give the sound new values, by the Random page's percentages",
            "bounds": bounds, "gesture": "randomize", "x": None, "y": None, "drawing": []}


def tabs(pid, names, x, y, w, h, tag, visible=None):
    return [choose(f"{tag}_{i}", pid, name, i, [x + i * w, y, w - 2.0, h], visible) for i, name in enumerate(names)]


def percent(pid):
    return mul(par(pid), 100.0)


def point(cid, name, bounds, pid, axis, log, visible):
    """A handle that shows where its value is, dragged along `axis`."""
    control = handle(cid, name, bounds, pid, axis, log=log, visible=visible)
    control["widget"]["color"] = tint(HIGHLIGHT, 170)
    return control


def env_curve(prefix, w, h, top, bottom, color):
    """A time envelope's shape: fixed stages with its levels; `top` and `bottom` are its range."""
    span = top - bottom

    def y(pid):
        return add(mul(sub(top, minimum(top, maximum(bottom, par(pid)))), (h - 8.0) / span), 4.0)
    points = [[0.0, y(f"{prefix}initial")], [w * 0.2, y(f"{prefix}peak")], [w * 0.45, y(f"{prefix}sustain")],
              [w * 0.75, y(f"{prefix}sustain")], [w, y(f"{prefix}end")]]
    return [rect(0.0, 0.0, w, h, DISPLAY, 3.0), path(points, color, 1.5)]


def dx_curve(n, w, h, color):
    """A DX7 envelope's shape: its four levels at even steps."""
    def y(field_name):
        return add(mul(sub(99.0, par(f"op{n}.{field_name}")), (h - 8.0) / 99.0), 4.0)
    points = [[0.0, y("level4")], [w * 0.2, y("level1")], [w * 0.45, y("level2")], [w * 0.75, y("level3")], [w, y("level4")]]
    return [rect(0.0, 0.0, w, h, DISPLAY, 3.0), path(points, color, 1.5)]


def op_row(n, x, y, h, tag, wide):
    """One operator's row: its number, envelope picture, ratio or Hz, coarse, fine, level, on."""
    scale = 1.5 if wide else 1.0
    controls = [
        {"widget": {"kind": "button", "label": str(n), "off_label": None, "indicator": False}, "visible_when": None,
         "id": f"select_op{n}{tag}", "name": f"Select operator {n}", "tooltip": f"Select operator {n}",
         "bounds": [x, y + 2.0, 22.0, h - 4.0], "gesture": {"set": n},
         "x": {"parameter": "panel.op", "logarithmic": False, "drag_scale": 1.0}, "y": None, "drawing": []},
        graphic(f"op{n}_time{tag}", f"Operator {n} envelope", [x + 26.0, y + 2.0, 66.0 * scale, h - 4.0],
                env_curve(f"op{n}.env.", 66.0 * scale, h - 4.0, 0.0, -70.0, TEXT), equal(par(f"op{n}.envMode"), 1.0)),
        graphic(f"op{n}_rates{tag}", f"Operator {n} envelope", [x + 26.0, y + 2.0, 66.0 * scale, h - 4.0],
                dx_curve(n, 66.0 * scale, h - 4.0, TEXT), equal(par(f"op{n}.envMode"), 0.0)),
    ]
    at = x + 26.0 + 66.0 * scale + 4.0
    controls.append(button(f"op{n}.mode{tag}", f"op{n}.mode", "Hz", [at, y + 2.0, 24.0, h - 4.0], off_label="Ratio"))
    at += 28.0
    for pid, caption, width in ((f"op{n}.coarse", "Coarse", 46.0), (f"op{n}.fine", "Fine", 42.0), (f"op{n}.level", "Level", 46.0)):
        controls.append(field(pid, caption, [at, y + 2.0, width, h - 4.0], par(pid), tag=tag))
        at += width + 3.0
    controls.append(button(f"op{n}.on{tag}", f"op{n}.on", "On", [at, y + 2.0, 26.0, h - 4.0], off_label="Off"))
    return controls


def algorithm_pictures(x, y, w, h, tag, size):
    tables = algorithms()
    return [graphic(f"algorithm_picture_{i + 1}{tag}", f"Algorithm {i + 1}", [x, y, w, h],
                    [rect(0.0, 0.0, w, h, DISPLAY, 3.0)] + picture(flags, w - 8.0, h - 8.0, size),
                    equal(par("algorithm"), float(i + 1))) for i, flags in enumerate(tables)]


def dial(pid, caption, x, y, value, suffix="", decimals=0, log=False, visible=None, tag="", big=False):
    return knob(pid, caption, [x, y, 50.0, 70.0], value, suffix, decimals, log, visible, tag, big)


def op_dials(n, x, y, tag, visible):
    """The Env and Wave pages' eight dials for operator `n`."""
    shown = both(visible, selected(n))
    p = lambda f: f"op{n}.{f}"
    e = lambda f: f"op{n}.env.{f}"
    env_page, wave_page = equal(par("panel.page"), 0.0), equal(par("panel.page"), 1.0)
    time = both(env_page, both(shown, equal(par(p("envMode")), 1.0)))
    rates = both(env_page, both(shown, equal(par(p("envMode")), 0.0)))
    wave = both(wave_page, shown)
    cells = lambda i: (x + (i % 4) * 50.0, y + (i // 4) * 80.0)
    controls = []
    for i, (pid, caption, value, suffix, decimals, log) in enumerate((
            (e("attack"), "Attack", mul(par(e("attack")), 1000.0), " ms", 0, True),
            (e("decay"), "Decay", mul(par(e("decay")), 1000.0), " ms", 0, True),
            (e("sustain"), "Sustain", par(e("sustain")), " dB", 1, False),
            (e("release"), "Release", mul(par(e("release")), 1000.0), " ms", 0, True),
            (e("peak"), "Peak", par(e("peak")), " dB", 1, False),
            (p("velocitySens"), "Vel", par(p("velocitySens")), "", 0, False),
            (e("timeByKey"), "Key", percent(e("timeByKey")), " %", 0, False),
            (p("detune"), "Detune", par(p("detune")), "", 0, False))):
        cx, cy = cells(i)
        controls.append(dial(pid, caption, cx, cy, value, suffix, decimals, log, time, f"{tag}_time"))
    for i in range(8):
        pid = p(f"rate{i + 1}") if i < 4 else p(f"level{i - 3}")
        caption = f"Rate {i + 1}" if i < 4 else f"Level {i - 3}"
        cx, cy = cells(i)
        controls.append(dial(pid, caption, cx, cy, par(pid), visible=rates, tag=f"{tag}_rates"))
    for i, (pid, caption, value, suffix) in enumerate((
            (p("feedback"), "Feedback", percent(p("feedback")), " %"),
            (p("phase"), "Phase", mul(par(p("phase")), 360.0), "°"),
            (p("freqByVel"), "Osc Vel", percent(p("freqByVel")), " %"),
            (p("levelByKey"), "Lvl Key", percent(p("levelByKey")), " %"),
            (p("velocitySens"), "Vel", par(p("velocitySens")), ""),
            (p("rateScaling"), "Rate Key", par(p("rateScaling")), ""),
            (p("ampModSens"), "Amp Mod", par(p("ampModSens")), ""),
            (p("detune"), "Detune", par(p("detune")), ""))):
        cx, cy = cells(i)
        controls.append(dial(pid, caption, cx, cy, value, suffix, visible=wave, tag=f"{tag}_wave"))
    return controls


def global_dials(x, y, tag):
    """The Filter, LFO, Pitch and Mod pages' dials."""
    pages = {
        2: [("filterFreq", "Freq", par("filterFreq"), " Hz", 0, True), ("filterRes", "Res", percent("filterRes"), " %", 0, False),
            ("filterMorph", "Morph", percent("filterMorph"), " %", 0, False), ("filterEnv", "Env", percent("filterEnv"), " %", 0, False),
            ("filterByVel", "Vel", percent("filterByVel"), " %", 0, False), ("filterByKey", "Key", percent("filterByKey"), " %", 0, False),
            ("filterDrive", "Drive", par("filterDrive"), " dB", 1, False), ("shaperMix", "Mix", percent("shaperMix"), " %", 0, False)],
        3: [("lfo2Rate", "Rate", percent("lfo2Rate"), " %", 0, False), ("lfo2Amount", "Amount", percent("lfo2Amount"), " %", 0, False),
            ("lfo2ByKey", "Rate Key", percent("lfo2ByKey"), " %", 0, False), ("lfo2ByVel", "Amt Vel", percent("lfo2ByVel"), " %", 0, False),
            ("lfoSpeed", "DX Speed", par("lfoSpeed"), "", 0, False), ("lfoDelay", "DX Delay", par("lfoDelay"), "", 0, False),
            ("lfoPitchDepth", "DX Pitch", par("lfoPitchDepth"), "", 0, False), ("lfoAmpDepth", "DX Amp", par("lfoAmpDepth"), "", 0, False)],
        4: [("pitchEnv.initial", "Initial", par("pitchEnv.initial"), " st", 1, False), ("pitchEnv.attack", "Attack", mul(par("pitchEnv.attack"), 1000.0), " ms", 0, True),
            ("pitchEnv.peak", "Peak", par("pitchEnv.peak"), " st", 1, False), ("pitchEnv.release", "Release", mul(par("pitchEnv.release"), 1000.0), " ms", 0, True),
            ("pitchEnvAmount", "Amount", percent("pitchEnvAmount"), " %", 0, False), ("pitchBendRange", "Bend", par("pitchBendRange"), " st", 0, False),
            ("portamentoTime", "Glide", par("portamentoTime"), "", 0, False), ("transpose", "Trans.", par("transpose"), " st", 0, False)],
        5: [("mod.velocity.amount1", "Velocity", percent("mod.velocity.amount1"), " %", 0, False), ("mod.wheel.amount1", "Wheel", percent("mod.wheel.amount1"), " %", 0, False),
            ("mod.foot.amount1", "Foot", percent("mod.foot.amount1"), " %", 0, False), ("mod.breath.amount1", "Breath", percent("mod.breath.amount1"), " %", 0, False),
            ("mod.pressure.amount1", "Pressure", percent("mod.pressure.amount1"), " %", 0, False), ("mod.key.amount1", "Key", percent("mod.key.amount1"), " %", 0, False),
            ("mod.random.amount1", "Random", percent("mod.random.amount1"), " %", 0, False), ("voices", "Voices", par("voices"), "", 0, False)],
    }
    controls = []
    for page, cells in pages.items():
        shown = equal(par("panel.page"), float(page))
        for i, (pid, caption, value, suffix, decimals, log) in enumerate(cells):
            controls.append(dial(pid, caption, x + (i % 4) * 50.0, y + (i // 4) * 80.0, value, suffix, decimals, log, shown, f"{tag}_p{page}"))
    return controls


def display(x, y, w, h, tag):
    """The display beside the dials, following the page and the selected operator."""
    controls = []
    page = lambda i: equal(par("panel.page"), float(i))
    for n in OPS:
        shown = both(selected(n), page(0))
        time = both(shown, equal(par(f"op{n}.envMode"), 1.0))
        rates = both(shown, equal(par(f"op{n}.envMode"), 0.0))
        heading = text(6.0, 4.0, f"Operator {n} envelope", tint(TEXT, 170), 10.0)
        controls.append(graphic(f"display_time_{n}{tag}", f"Operator {n} envelope", [x, y, w, h],
                                env_curve(f"op{n}.env.", w, h, 0.0, -70.0, ACCENT) + [heading], time))
        controls.append(graphic(f"display_rates_{n}{tag}", f"Operator {n} envelope", [x, y, w, h],
                                dx_curve(n, w, h, ACCENT) + [heading], rates))
        bar_w = (w - 12.0) / 16.0
        draws = [rect(0.0, 0.0, w, h, DISPLAY, 3.0), text(6.0, 4.0, f"Operator {n} partials", tint(TEXT, 170), 10.0)]
        for i in range(16):
            draws.append(rect(6.0 + i * bar_w, sub(h - 4.0, mul(par(f"op{n}.h{i + 1}"), h - 24.0)), bar_w - 2.0,
                              mul(par(f"op{n}.h{i + 1}"), h - 24.0), ACCENT))
        controls.append(graphic(f"display_wave_{n}{tag}", f"Operator {n} partials", [x, y, w, h], draws, both(selected(n), page(1))))
        controls.append(menu(f"op{n}.wave", "Wave", [x + w - 96.0, y + 2.0, 92.0, 18.0], both(selected(n), page(1)), tag))
    controls.append(graphic(f"display_filter{tag}", "Filter envelope", [x, y, w, h],
                            env_curve("filterEnv.", w, h, 1.0, 0.0, [3, 0, 4, 255]) + [text(6.0, 4.0, "Filter envelope", tint(TEXT, 170), 10.0)], page(2)))
    controls.append(menu("filterType", "Filter Type", [x + w - 96.0, y + 2.0, 92.0, 18.0], page(2), tag))
    controls.append(menu("filterCircuit", "Filter Circuit", [x + w - 96.0, y + 22.0, 92.0, 18.0], page(2), tag))
    controls.append(button(f"filterOn{tag}", "filterOn", "On", [x + w - 96.0, y + 42.0, 44.0, 18.0], visible=page(2), off_label="Off"))
    controls.append(graphic(f"display_lfo{tag}", "LFO envelope", [x, y, w, h],
                            env_curve("lfoEnv.", w, h, 1.0, 0.0, SPECTRUM_COLOR) + [text(6.0, 4.0, "LFO envelope", tint(TEXT, 170), 10.0)], page(3)))
    controls.append(menu("lfo2Wave", "LFO 2 Wave", [x + w - 96.0, y + 2.0, 92.0, 18.0], page(3), tag))
    controls.append(menu("lfo2Range", "LFO 2 Range", [x + w - 96.0, y + 22.0, 92.0, 18.0], page(3), tag))
    controls.append(button(f"lfoOn{tag}", "lfoOn", "On", [x + w - 96.0, y + 42.0, 44.0, 18.0], visible=page(3), off_label="Off"))
    controls.append(graphic(f"display_pitch{tag}", "Pitch envelope", [x, y, w, h],
                            env_curve("pitchEnv.", w, h, 48.0, -48.0, HIGHLIGHT) + [text(6.0, 4.0, "Pitch envelope", tint(TEXT, 170), 10.0)], page(4)))
    controls.append(button(f"pitchEnvOn{tag}", "pitchEnvOn", "On", [x + w - 96.0, y + 2.0, 44.0, 18.0], visible=page(4), off_label="Off"))
    draws = [rect(0.0, 0.0, w, h, DISPLAY, 3.0), text(6.0, 4.0, "Modulation: the expanded panel routes", tint(TEXT, 170), 10.0)]
    for i, (source, name) in enumerate(MATRIX):
        row = 22.0 + i * 13.0
        draws.append(text(6.0, row, name, tint(TEXT, 170), 9.0))
        draws.append(value_text(w - 30.0, row, percent(f"mod.{source}.amount1"), suffix=" %", size=9.0, decimals=0))
    controls.append(graphic(f"display_mod{tag}", "Modulation", [x, y, w, h], draws, page(5)))
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


def compact():
    w, h = COMPACT
    controls = []
    for n in OPS:
        controls += op_row(n, 2.0, (n - 1) * ROW_H, ROW_H, "", False)
    controls.append(separator("sep_ops", 324.0, 4.0, h - 8.0))
    controls.append(field("algorithm", "Algorithm", [330.0, 2.0, 112.0, 26.0], par("algorithm")))
    controls += algorithm_pictures(330.0, 30.0, 112.0, 106.0, "", 12.0)
    controls.append(field("feedback", "Feedback", [330.0, 138.0, 112.0, 26.0], par("feedback")))
    controls.append(separator("sep_alg", 448.0, 4.0, h - 8.0))
    controls += tabs("panel.page", PAGES, 454.0, 2.0, 50.0, 18.0, "page")
    controls += display(454.0, 22.0, 298.0, 142.0, "")
    for n in OPS:
        controls += op_dials(n, 758.0, 4.0, "", None)
    controls += global_dials(758.0, 4.0, "")
    controls.append(separator("sep_dials", 962.0, 4.0, h - 8.0))
    controls.append(dial("time", "Time", 966.0, 4.0, percent("time"), " %"))
    controls.append(dial("tone", "Tone", 1016.0, 4.0, percent("tone"), " %"))
    controls.append(dial("spread", "Spread", 966.0, 84.0, percent("spread"), " %"))
    controls.append(dial("volume", "Volume", 1016.0, 84.0, par("volume"), " dB", 1))
    controls.append(random_button("random", "Random", [1072.0, 4.0, 62.0, 22.0]))
    controls.append(choose("random_settings", "panel.expandedPage", "%", 6, [1072.0, 30.0, 62.0, 22.0]))
    controls.append(meter("meter", [1140.0, 4.0, 36.0, h - 8.0]))
    return {"size": [w, h], "viewport_size": None, "background": PANEL, "assets": [], "layers": [], "controls": controls,
            "hidden_parameters": every_parameter(), "accent_on": True}


def form(cells, x, y, visible, tag, columns=4, w=78.0, h=30.0):
    """Fields in a grid: (pid, caption, value, suffix, decimals, log), a menu as (pid, caption), a toggle as (pid, caption, True)."""
    controls = []
    for i, cell in enumerate(cells):
        cx, cy = x + (i % columns) * (w + 4.0), y + (i // columns) * (h + 4.0)
        if len(cell) == 2:
            controls.append(menu(cell[0], cell[1], [cx, cy, w, h], visible, tag))
        elif len(cell) == 3:
            controls.append(button(f"{cell[0]}{tag}", cell[0], cell[1], [cx, cy, w, h], visible=visible))
        else:
            pid, caption, value, suffix, decimals, log = cell
            controls.append(field(pid, caption, [cx, cy, w, h], value, suffix, decimals, log, visible, tag))
    return controls


def env_form(prefix, x, y, visible, tag, unit, slopes):
    ms = lambda f: (f"{prefix}{f}", f.capitalize(), mul(par(f"{prefix}{f}"), 1000.0), " ms", 0, True)
    level = lambda f: (f"{prefix}{f}", f.capitalize(), par(f"{prefix}{f}"), unit, 2 if unit == "" else 1, False)
    cells = [ms("attack"), ms("decay"), ms("release"), level("initial"), level("peak"), level("sustain"), (f"{prefix}loop", "Loop"),
             (f"{prefix}repeat", "Repeat"), ms("loopTime"), (f"{prefix}timeByVel", "Time by vel", percent(f"{prefix}timeByVel"), " %", 0, False),
             (f"{prefix}timeByKey", "Time by key", percent(f"{prefix}timeByKey"), " %", 0, False)]
    if slopes:
        cells += [level("end")] + [(f"{prefix}{s}Slope", f"{s.capitalize()} slope", percent(f"{prefix}{s}Slope"), " %", 0, False) for s in ("attack", "decay", "release")]
    return form(cells, x, y, visible, tag)


def expanded_operator(x, y, w):
    page = equal(par("panel.expandedPage"), 0.0)
    controls = []
    for n in OPS:
        shown = both(page, selected(n))
        p = lambda f: f"op{n}.{f}"
        e = f"op{n}.env."
        time = both(shown, equal(par(p("envMode")), 1.0))
        rates = both(shown, equal(par(p("envMode")), 0.0))
        tag = "_x"
        controls.append(graphic(f"x_time_{n}", f"Operator {n} envelope", [x, y + 24.0, w, 130.0], env_curve(e, w, 130.0, 0.0, -70.0, ACCENT), time))
        controls.append(graphic(f"x_rates_{n}", f"Operator {n} envelope", [x, y + 24.0, w, 130.0], dx_curve(n, w, 130.0, ACCENT), rates))
        controls.append(choose(f"x_mode_time_{n}", p("envMode"), "Time", 1, [x, y, 70.0, 20.0], shown))
        controls.append(choose(f"x_mode_rates_{n}", p("envMode"), "Rates and levels", 0, [x + 74.0, y, 110.0, 20.0], shown))
        # Handles over the curve: attack, decay and release sideways, each in its own stretch,
        # sustain up and down; each shows where its value is.
        controls.append(point(f"x_attack_{n}", "Attack", [x, y + 24.0, w * 0.2, 130.0], f"{e}attack", "x", True, time))
        controls.append(point(f"x_decay_{n}", "Decay", [x + w * 0.2, y + 24.0, w * 0.25, 130.0], f"{e}decay", "x", True, time))
        controls.append(point(f"x_release_{n}", "Release", [x + w * 0.75, y + 24.0, w * 0.25, 130.0], f"{e}release", "x", True, time))
        controls.append(point(f"x_sustain_{n}", "Sustain", [x + w * 0.45, y + 24.0, w * 0.3, 130.0], f"{e}sustain", "y", False, time))
        controls += env_form(e, x, y + 160.0, time, tag, " dB", False)
        controls += form([(p(f"rate{i}"), f"Rate {i}", par(p(f"rate{i}")), "", 0, False) for i in range(1, 5)]
                         + [(p(f"level{i}"), f"Level {i}", par(p(f"level{i}")), "", 0, False) for i in range(1, 5)]
                         + [(p("breakPoint"), "Break point", par(p("breakPoint")), "", 0, False), (p("leftDepth"), "Left depth", par(p("leftDepth")), "", 0, False),
                            (p("rightDepth"), "Right depth", par(p("rightDepth")), "", 0, False), (p("rateScaling"), "Rate scale", par(p("rateScaling")), "", 0, False),
                            (p("leftCurve"), "Left curve"), (p("rightCurve"), "Right curve"),
                            (p("ampModSens"), "Amp mod", par(p("ampModSens")), "", 0, False), (p("velocitySens"), "Velocity", par(p("velocitySens")), "", 0, False)],
                         x, y + 160.0, rates, tag)
        wave_y = y + 300.0
        controls += form([(p("wave"), "Wave"), (p("feedback"), "Feedback", percent(p("feedback")), " %", 0, False), (p("phase"), "Phase", mul(par(p("phase")), 360.0), "°", 0, False),
                          (p("retrigger"), "Retrigger", True), (p("freqByVel"), "Freq by vel", percent(p("freqByVel")), " %", 0, False), (p("freqQuantize"), "Quantize", True),
                          (p("levelByKey"), "Level by key", percent(p("levelByKey")), " %", 0, False), (p("detune"), "Detune", par(p("detune")), "", 0, False)],
                         x, wave_y, shown, tag)
        bars_y, bars_h = wave_y + 72.0, 130.0
        controls += [choose(f"x_partials_{n}_{i}", p("partials"), label, i, [x + i * 44.0, bars_y, 40.0, 18.0], shown) for i, label in enumerate(("16", "32", "64"))]
        controls.append(menu(p("repeat"), "Repeat", [x + 140.0, bars_y, 70.0, 18.0], shown, tag))
        controls.append(menu(p("oddEven"), "Partials used", [x + 214.0, bars_y, 70.0, 18.0], shown, tag))
        controls.append(button(f"{p('normalize')}{tag}", p("normalize"), "Normalize", [x + 288.0, bars_y, 70.0, 18.0], visible=shown))
        controls.append(graphic(f"x_bars_bg_{n}", "Partials", [x, bars_y + 22.0, w, bars_h], [rect(0.0, 0.0, w, bars_h, DISPLAY, 3.0)], shown))
        bar_w = (w - 8.0) / 64.0
        for i in range(64):
            pid = p(f"h{i + 1}")
            # One partial's bar: dragging up or down sets it.
            limit = 0.0 if i < 16 else (1.0 if i < 32 else 2.0)
            visible = both(shown, maximum(0.0, minimum(1.0, mul(sub(par(p("partials")), limit - 0.5), 2.0))))
            controls.append({"visible_when": visible, "id": f"{pid}{tag}", "name": f"Partial {i + 1}", "tooltip": f"Partial {i + 1}",
                             "bounds": [x + 4.0 + i * bar_w, bars_y + 26.0, bar_w, bars_h - 8.0], "gesture": "position", "x": None,
                             "y": {"parameter": pid, "logarithmic": False, "drag_scale": 1.0},
                             "drawing": [rect(0.0, sub(bars_h - 8.0, mul(par(pid), bars_h - 8.0)), bar_w - 1.0, mul(par(pid), bars_h - 8.0), ACCENT)]})
    return controls


def expanded_filter(x, y, w):
    shown = equal(par("panel.expandedPage"), 1.0)
    controls = [graphic("x_filter_env", "Filter envelope", [x, y, w, 130.0], env_curve("filterEnv.", w, 130.0, 1.0, 0.0, MODULATOR), shown)]
    controls += form([("filterOn", "On", True), ("filterType", "Type"), ("filterSlope", "Slope"), ("filterCircuit", "Circuit"),
                      ("filterFreq", "Freq", par("filterFreq"), " Hz", 0, True), ("filterRes", "Res", percent("filterRes"), " %", 0, False),
                      ("filterMorph", "Morph", percent("filterMorph"), " %", 0, False), ("filterDrive", "Drive", par("filterDrive"), " dB", 1, False),
                      ("filterEnv", "Env", percent("filterEnv"), " %", 0, False), ("filterByVel", "By vel", percent("filterByVel"), " %", 0, False),
                      ("filterByKey", "By key", percent("filterByKey"), " %", 0, False), ("shaper", "Shaper"),
                      ("shaperDrive", "Shaper drive", par("shaperDrive"), " dB", 1, False), ("shaperMix", "Dry/Wet", percent("shaperMix"), " %", 0, False)],
                     x, y + 136.0, shown, "_x")
    return controls + env_form("filterEnv.", x, y + 276.0, shown, "_x", "", True)


def expanded_lfo(x, y, w):
    shown = equal(par("panel.expandedPage"), 2.0)
    controls = [graphic("x_lfo_env", "LFO envelope", [x, y, w, 100.0], env_curve("lfoEnv.", w, 100.0, 1.0, 0.0, SPECTRUM_COLOR), shown)]
    controls += form([("lfoOn", "On", True), ("lfo2Wave", "Wave"), ("lfo2Range", "Range"), ("lfo2Sync", "Sync"),
                      ("lfo2Rate", "Rate", percent("lfo2Rate"), " %", 0, False), ("lfo2Amount", "Amount", percent("lfo2Amount"), " %", 0, False),
                      ("lfo2ByKey", "Rate by key", percent("lfo2ByKey"), " %", 0, False), ("lfo2ByVel", "Amt by vel", percent("lfo2ByVel"), " %", 0, False),
                      ("lfo2Retrigger", "Retrigger", True), ("lfo2ToFilter", "To filter", True), ("lfo2DestB", "Dest B"),
                      ("lfo2AmountB", "Amount B", percent("lfo2AmountB"), " %", 0, False)] + [(f"lfo2ToOp{n}", f"To op {n}", True) for n in OPS]
                     + [("lfoWave", "DX wave"), ("lfoSpeed", "DX speed", par("lfoSpeed"), "", 0, False), ("lfoDelay", "DX delay", par("lfoDelay"), "", 0, False),
                        ("lfoPitchDepth", "DX pitch", par("lfoPitchDepth"), "", 0, False), ("lfoAmpDepth", "DX amp", par("lfoAmpDepth"), "", 0, False),
                        ("pitchModSens", "DX pitch sens", par("pitchModSens"), "", 0, False), ("lfoKeySync", "DX key sync", True), ("oscKeySync", "Osc key sync", True)],
                     x, y + 104.0, shown, "_x", h=26.0)
    return controls + env_form("lfoEnv.", x, y + 330.0, shown, "_x", "", True)


def expanded_pitch(x, y, w):
    shown = equal(par("panel.expandedPage"), 3.0)
    controls = [graphic("x_pitch_env", "Pitch envelope", [x, y, w, 100.0], env_curve("pitchEnv.", w, 100.0, 48.0, -48.0, HIGHLIGHT), shown)]
    controls += form([("pitchEnvOn", "On", True), ("pitchEnvAmount", "Amount", percent("pitchEnvAmount"), " %", 0, False),
                      ("pitchEnvToLfo", "To LFO", True), ("pitchEnvDestB", "Dest B"),
                      ("pitchEnvAmountB", "Amount B", percent("pitchEnvAmountB"), " %", 0, False)] + [(f"pitchEnvToOp{n}", f"To op {n}", True) for n in OPS]
                     + [(f"pitchRate{i}", f"DX rate {i}", par(f"pitchRate{i}"), "", 0, False) for i in range(1, 5)]
                     + [(f"pitchLevel{i}", f"DX level {i}", par(f"pitchLevel{i}"), "", 0, False) for i in range(1, 5)],
                     x, y + 104.0, shown, "_x", h=26.0)
    return controls + env_form("pitchEnv.", x, y + 290.0, shown, "_x", " st", True)


def expanded_matrix(x, y):
    shown = equal(par("panel.expandedPage"), 4.0)
    controls = []
    for i, (source, name) in enumerate(MATRIX):
        row = y + i * 28.0
        cells = [(f"mod.{source}.dest1", f"{name} goes to"), (f"mod.{source}.amount1", f"{name} amount", percent(f"mod.{source}.amount1"), " %", 0, False),
                 (f"mod.{source}.dest2", f"{name} also to"), (f"mod.{source}.amount2", f"{name} amount 2", percent(f"mod.{source}.amount2"), " %", 0, False)]
        controls.append(graphic(f"x_matrix_{source}", name, [x, row, 90.0, 24.0], [text(0.0, 5.0, name, TEXT, 11.0)], shown))
        controls += form(cells, x + 94.0, row, shown, "_x", w=104.0, h=24.0)
    route_y = y + 9 * 28.0 + 8.0
    controls.append(graphic("x_routes_title", "Controllers", [x, route_y, 300.0, 16.0], [text(0.0, 0.0, "Controllers to the DX7 voice", tint(TEXT, 170), 10.0)], shown))
    cells = []
    for s, name in (("wheel", "Wheel"), ("foot", "Foot"), ("breath", "Breath"), ("pressure", "Pressure")):
        cells += [(f"{s}Range", f"{name} range", par(f"{s}Range"), "", 0, False), (f"{s}Pitch", "Pitch", True), (f"{s}Amp", "Amp", True), (f"{s}EgBias", "EG bias", True)]
    return controls + form(cells, x, route_y + 18.0, shown, "_x", h=24.0)


def expanded_algorithm(x, y):
    shown = equal(par("panel.expandedPage"), 5.0)
    controls = []
    size = (60.0, 58.0)
    for i, flags in enumerate(algorithms()):
        cx, cy = x + (i % 9) * (size[0] + 2.0), y + (i // 9) * (size[1] + 2.0)
        controls.append(graphic(f"x_picker_{i + 1}", f"Algorithm {i + 1}", [cx, cy, size[0], size[1] - 14.0],
                                [rect(0.0, 0.0, size[0], size[1] - 14.0, DISPLAY, 2.0)] + picture(flags, size[0] - 6.0, size[1] - 18.0, 6.0), shown))
        controls.append(choose(f"x_pick_{i + 1}", "algorithm", str(i + 1), i + 1, [cx, cy + size[1] - 14.0, size[0], 13.0], shown))
    return controls


def expanded_random(x, y):
    shown = equal(par("panel.expandedPage"), 6.0)
    controls = [random_button("x_random_now", "Random", [x, y, 90.0, 30.0], shown)]
    controls += form([("random.amount", "Amount", percent("random.amount"), " %", 0, False), ("random.seed", "Seed", par("random.seed"), "", 0, False)], x + 94.0, y, shown, "_x")
    for i, (sid, name) in enumerate(SECTIONS):
        row = y + 40.0 + i * 30.0
        controls.append(graphic(f"x_section_{sid}", name, [x, row, 130.0, 26.0], [text(0.0, 5.0, name, TEXT, 11.0)], shown))
        controls += form([(f"random.{sid}.amount", "Percent", percent(f"random.{sid}.amount"), " %", 0, False), (f"random.{sid}.on", "On", True), (f"random.{sid}.lock", "Lock", True)],
                         x + 134.0, row, shown, "_x", w=78.0, h=26.0)
    return controls


def expanded():
    w, h = EXPANDED
    controls = []
    for n in OPS:
        controls += op_row(n, 4.0, 40.0 + (n - 1) * 44.0, 44.0, "_x", True)
    controls.append(random_button("x_random", "Random", [4.0, 6.0, 70.0, 24.0]))
    controls.append(field("algorithm", "Algorithm", [80.0, 2.0, 100.0, 32.0], par("algorithm"), tag="_x"))
    controls.append(field("feedback", "Feedback", [184.0, 2.0, 100.0, 32.0], par("feedback"), tag="_x"))
    controls += algorithm_pictures(4.0, 310.0, 340.0, 200.0, "_x", 18.0)
    controls.append(separator("x_sep_left", 356.0, 4.0, h - 8.0))
    controls += tabs("panel.expandedPage", EXPANDED_PAGES, 362.0, 4.0, 72.0, 22.0, "x_page")
    x, y, mid = 362.0, 32.0, 520.0
    controls += expanded_operator(x, y, mid)
    controls += expanded_filter(x, y, mid)
    controls += expanded_lfo(x, y, mid)
    controls += expanded_pitch(x, y, mid)
    controls += expanded_matrix(x, y)
    controls += expanded_algorithm(x, y)
    controls += expanded_random(x, y)
    controls.append(separator("x_sep_right", 888.0, 4.0, h - 8.0))
    right = [("volume", "Volume", par("volume"), " dB", 1, False), ("time", "Time", percent("time"), " %", 0, False), ("tone", "Tone", percent("tone"), " %", 0, False),
             ("spread", "Spread", percent("spread"), " %", 0, False), ("pan", "Pan", percent("pan"), " %", 0, False), ("panByKey", "Pan by key", percent("panByKey"), " %", 0, False),
             ("panRandom", "Random pan", percent("panRandom"), " %", 0, False), ("voices", "Voices", par("voices"), "", 0, False), ("retrigger", "Retrigger", True),
             ("antialias", "Antialias", True), ("interpolation", "Interpolation", True), ("transpose", "Transpose", par("transpose"), " st", 0, False),
             ("pitchBendRange", "Bend range", par("pitchBendRange"), " st", 0, False), ("pitchBendStep", "Bend step", par("pitchBendStep"), " st", 0, False),
             ("portamento", "Glide", True), ("glissando", "Glissando", True), ("portamentoTime", "Glide time", par("portamentoTime"), "", 0, False)]
    controls += form(right, 892.0, 4.0, None, "_xr", columns=3, w=92.0, h=34.0)
    return {"size": [w, h], "viewport_size": None, "background": PANEL, "assets": [], "layers": [], "controls": controls,
            "hidden_parameters": every_parameter()}


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


def main():
    panel = {"ui": compact(), "expanded": expanded()}
    check_colours(panel)
    ids = [c["id"] for c in panel["ui"]["controls"]]
    assert len(ids) == len(set(ids)), [i for i in ids if ids.count(i) > 1][:5]
    ids = [c["id"] for c in panel["expanded"]["controls"]]
    assert len(ids) == len(set(ids)), [i for i in ids if ids.count(i) > 1][:5]
    out = Path("devices/fm-synth/panel.json")
    out.write_text(json.dumps(panel, separators=(",", ":")) + "\n", encoding="utf-8")
    print(f"wrote {out}: {out.stat().st_size} bytes, {len(panel['ui']['controls'])} and {len(panel['expanded']['controls'])} controls")


if __name__ == "__main__":
    main()
