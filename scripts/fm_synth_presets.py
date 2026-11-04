# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Writes devices' stock presets for the FM Synth: presets/fm-synth/*.fm-synth.preset.suhina (docs/decisions/0014-stock-presets.md).

A preset names every parameter its device declares, so this reads the device's parameters, with
their defaults, from the `device.list` command's answer, saved as a file:
python scripts/fm_synth_presets.py device-list.json
"""

import json
import sys
from pathlib import Path

SAW_64, NOISE_WHITE, DRAWN = 10.0, 21.0, 22.0
# How strong each partial of the piano's strings is: falling with height, with the seventh weak as a hammer leaves it.
STRING = [1.0, 0.72, 0.5, 0.34, 0.26, 0.18, 0.05, 0.11, 0.08, 0.06, 0.045, 0.035, 0.025, 0.02, 0.015, 0.012]


def op(n, coarse, level, attack, decay, sustain, release, detune=0, fine=0, velocity=0, wave=0.0, by_key=0.0):
    """Operator `n` with an envelope in time."""
    values = {f"op{n}.envMode": 1.0, f"op{n}.on": 1.0, f"op{n}.mode": 0.0, f"op{n}.wave": wave, f"op{n}.coarse": float(coarse), f"op{n}.fine": float(fine),
              f"op{n}.level": float(level), f"op{n}.detune": float(detune), f"op{n}.velocitySens": float(velocity), f"op{n}.env.initial": -70.0, f"op{n}.env.peak": 0.0,
              f"op{n}.env.end": -70.0, f"op{n}.env.attack": attack, f"op{n}.env.decay": decay, f"op{n}.env.sustain": sustain, f"op{n}.env.release": release,
              f"op{n}.env.timeByKey": by_key}
    if wave == DRAWN:
        values[f"op{n}.partials"] = 0.0
        values[f"op{n}.normalize"] = 1.0
        values.update({f"op{n}.h{i + 1}": amount for i, amount in enumerate(STRING)})
    return values


def filter_env(attack, decay, sustain, release):
    return {"filterEnv.attack": attack, "filterEnv.decay": decay, "filterEnv.sustain": sustain, "filterEnv.release": release, "filterEnv.peak": 1.0, "filterEnv.initial": 0.0}


PRESETS = {
    "warm-fm-bass": ("Warm FM Bass", ["bass", "fm", "punchy"], {
        "algorithm": 5.0, "feedback": 5.0, "volume": 0.0, "voices": 8.0, "transpose": -24.0,
        "filterOn": 1.0, "filterType": 0.0, "filterSlope": 1.0, "filterCircuit": 1.0, "filterFreq": 1700.0, "filterRes": 0.22, "filterDrive": 6.0, "filterEnv": 0.55,
        **filter_env(0.001, 0.2, 0.0, 0.1),
        **op(1, 1, 96, 0.001, 0.5, -9.0, 0.09), **op(2, 1, 86, 0.001, 0.22, -18.0, 0.08),
        **op(3, 1, 82, 0.001, 0.35, -14.0, 0.09, detune=2), **op(4, 2, 84, 0.001, 0.14, -24.0, 0.08),
        **op(5, 0, 88, 0.001, 0.8, -4.0, 0.1), **op(6, 1, 72, 0.001, 0.06, -50.0, 0.05),
    }),
    "drawbar-organ": ("Drawbar Organ", ["organ", "keys"], {
        "algorithm": 32.0, "feedback": 0.0, "volume": -3.0, "voices": 16.0, "lfoSpeed": 38.0, "lfoPitchDepth": 6.0, "pitchModSens": 3.0, "lfoWave": 0.0, "lfoDelay": 20.0, "spread": 0.12,
        **op(1, 1, 96, 0.004, 0.1, 0.0, 0.04), **op(2, 0, 90, 0.004, 0.1, 0.0, 0.04), **op(3, 2, 86, 0.004, 0.1, 0.0, 0.04),
        **op(4, 3, 78, 0.004, 0.1, 0.0, 0.04), **op(5, 4, 74, 0.004, 0.1, 0.0, 0.04), **op(6, 8, 72, 0.001, 0.025, -70.0, 0.01),
    }),
    "struck-fm-keys": ("Struck FM Keys", ["keys", "fm", "bright"], {
        "algorithm": 32.0, "feedback": 0.0, "volume": -13.0, "voices": 32.0, "spread": 0.1, "panByKey": 0.3,
        "filterOn": 1.0, "filterType": 0.0, "filterSlope": 1.0, "filterCircuit": 0.0, "filterFreq": 1800.0, "filterRes": 0.0, "filterEnv": 0.45, "filterByVel": 0.5, "filterByKey": 0.6,
        **filter_env(0.001, 1.4, 0.15, 0.2),
        **op(1, 1, 96, 0.0015, 14.0, -70.0, 0.22, velocity=3, wave=DRAWN, by_key=0.35), **op(2, 1, 84, 0.0015, 9.0, -70.0, 0.22, detune=1, velocity=3, wave=DRAWN, by_key=0.35),
        **op(3, 1, 80, 0.0015, 2.6, -70.0, 0.22, detune=-1, velocity=3, wave=DRAWN, by_key=0.35), **op(4, 2, 70, 0.0015, 1.6, -70.0, 0.22, detune=2, velocity=3, by_key=0.35),
        **op(5, 3, 62, 0.0015, 0.9, -70.0, 0.22, detune=3, velocity=3, by_key=0.35), **op(6, 1, 64, 0.0015, 0.03, -70.0, 0.02, velocity=6, wave=NOISE_WHITE),
    }),
    "wide-trance-pad": ("Wide Trance Pad", ["pad", "trance", "wide"], {
        "algorithm": 32.0, "feedback": 0.0, "volume": 0.0, "voices": 32.0, "spread": 0.85,
        "filterOn": 1.0, "filterType": 0.0, "filterSlope": 1.0, "filterCircuit": 0.0, "filterFreq": 5000.0, "filterRes": 0.15, "filterEnv": 0.2,
        **filter_env(0.8, 3.0, 0.5, 1.2),
        **op(1, 1, 88, 0.35, 1.0, -2.0, 1.2, detune=-7, wave=SAW_64), **op(2, 1, 88, 0.35, 1.0, -2.0, 1.2, detune=7, wave=SAW_64),
        **op(3, 1, 84, 0.35, 1.0, -2.0, 1.2, detune=-4, fine=1, wave=SAW_64), **op(4, 1, 84, 0.35, 1.0, -2.0, 1.2, detune=3, wave=SAW_64),
        **op(5, 2, 78, 0.35, 1.0, -2.0, 1.2, detune=-5, wave=SAW_64), **op(6, 0, 80, 0.35, 1.0, -2.0, 1.2, wave=SAW_64),
    }),
    "slow-warm-pad": ("Slow Warm Pad", ["pad", "warm", "fm"], {
        "algorithm": 5.0, "feedback": 0.0, "volume": -4.0, "voices": 24.0, "spread": 0.75, "panRandom": 0.25, "transpose": -12.0,
        "filterOn": 1.0, "filterType": 0.0, "filterSlope": 0.0, "filterCircuit": 1.0, "filterFreq": 2400.0, "filterRes": 0.12, "filterDrive": 2.0, "filterEnv": 0.25,
        **filter_env(2.5, 4.0, 0.35, 2.0),
        **op(1, 1, 90, 0.004, 2.0, -3.0, 2.4, detune=-2), **op(2, 1, 62, 2.6, 3.0, -8.0, 2.2), **op(3, 1, 88, 1.8, 2.0, -3.0, 2.6, detune=3),
        **op(4, 2, 58, 3.2, 3.0, -6.0, 2.2), **op(5, 2, 72, 2.2, 2.0, -6.0, 2.8, detune=1), **op(6, 3, 48, 3.8, 3.0, -8.0, 2.4),
    }),
}


def main():
    devices = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
    device = next(d for d in devices if d["id"] == "fm-synth")
    folder = Path("presets/fm-synth")
    folder.mkdir(parents=True, exist_ok=True)
    for slug, (name, tags, values) in PRESETS.items():
        declared = [p["id"] for p in device["params"]]
        unknown = [pid for pid in values if pid not in declared]
        assert not unknown, (slug, unknown[:8])
        params = {p["id"]: values.get(p["id"], p["default"]) for p in device["params"]}
        preset = {"format": "suhina-preset", "version": 1, "device": "fm-synth", "role": "instrument", "name": name, "author": "suhina", "tags": tags, "params": params}
        out = folder / f"{name}.fm-synth.preset.suhina"
        out.write_text(json.dumps(preset, indent=0) + "\n", encoding="utf-8")
        assert out.stat().st_size < 64 * 1024, (slug, out.stat().st_size)
        print(f"wrote {out}: {out.stat().st_size} bytes, {len(params)} parameters")


if __name__ == "__main__":
    main()
