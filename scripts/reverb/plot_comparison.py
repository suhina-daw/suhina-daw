#!/usr/bin/env python3
# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Plot the separate diagnostics of compare_ir.py; no perceptual score is inferred."""

import argparse
import json
from pathlib import Path

import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
import numpy as np


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("report", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--previous-label", default="Prototype v1")
    parser.add_argument("--candidate-label", default="Prototype v2")
    args = parser.parse_args()
    report = json.loads(args.report.read_text(encoding="utf-8"))
    if args.output.exists():
        parser.error("plot already exists; choose a fresh output")
    responses = report["responses"]
    names = {"reference": "Reference", "previous": args.previous_label, "candidate": args.candidate_label}
    colors = {"reference": "#3d7b85", "previous": "#999999", "candidate": "#d05824"}
    fig, axes = plt.subplots(2, 2, figsize=(13, 8), layout="constrained")
    common_peak = max(max(v["structure"]["envelope_1ms_rms"]) * v["listening_gain"] for v in responses.values())
    target = responses["reference"]["analysis"]["broadband"]["t30"]["seconds"] or 2.
    for label in ("previous", "reference", "candidate"):
        if label not in responses: continue
        record = responses[label]
        data = record["structure"]
        kwargs = {"label": names[label], "color": colors[label], "linewidth": 1.6, "alpha": .85}
        envelope = np.asarray(data["envelope_1ms_rms"]) * record["listening_gain"]
        axes[0,0].plot(np.arange(len(envelope)), 20*np.log10(np.maximum(envelope/common_peak, 1e-6)), **kwargs)
        axes[0,1].plot([p["ms"]/1000 for p in data["remaining_energy"]], [p["db"] for p in data["remaining_energy"]], **kwargs)
        axes[1,0].plot([p["start_ms"]+10 for p in data["density"]], [p["value"] for p in data["density"]], **kwargs)
        shape = record["octave_shape_db"]
        frequencies = sorted(map(int, shape))
        axes[1,1].semilogx(frequencies, [shape[str(f)] for f in frequencies], marker="o", markersize=4, **kwargs)
    axes[0,0].set(title="Onset and early envelope (1 ms RMS bins)", xlabel="Time from input impulse (ms)", ylabel="dB relative to common envelope peak", xlim=(0,200), ylim=(-65,3))
    axes[0,1].set(title="Integrated energy decay", xlabel="Time from input impulse (s)", ylabel="Remaining energy (dB)", xlim=(0,max(.6,target*1.2)), ylim=(-65,2))
    axes[1,0].set(title="Echo-density diagnostic (20 ms windows)", xlabel="Time from input impulse (ms)", ylabel="Fraction outside 1 sigma / Gaussian value", xlim=(0,600), ylim=(0,1.4))
    axes[1,0].axhline(1, color="#cccccc", linestyle="--", linewidth=1)
    axes[1,1].set(title="Integrated octave energy relative to 1 kHz", xlabel="Octave-band center (Hz)", ylabel="Relative energy (dB)")
    for ax in axes.flat:
        ax.grid(alpha=.2)
        ax.legend(fontsize=9)
    fig.suptitle("Reverb research: same 48 kHz stimulus clock; separate structural diagnostics", fontsize=14)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    fig.savefig(args.output, dpi=150)
    plt.close(fig)
    print(f"Wrote {args.output}")


if __name__ == "__main__":
    main()
