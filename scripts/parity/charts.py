# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""The two figures of the parity report.

    python scripts/parity/charts.py [--out docs/research]

Only these output files are meant to be committed; the measurement itself (the manual text and
the heading map) stays in target/parity/.
"""

import argparse
import textwrap
from pathlib import Path

import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
from matplotlib.patches import Patch

import score

REPO = Path(__file__).resolve().parents[2]
COLOURS = {"green": "#3f9b5f", "light": "#8ec99f", "amber": "#e0a33a", "grey": "#c9ccd1"}

# Shorter labels for the chart than the tables use.
LABELS = {
    "Views and editing (ch 6-10, 16)": "Views and clip editing (ch 6-10, 16)",
    "Browser, files, project (ch 4, 5)": "Browser, files, projects (ch 4, 5)",
    "Devices (ch 23-25, 29-31)": "Devices: effects and instruments (ch 23-25, 29-31)",
    "Mixing, routing, I/O (ch 17, 18, 38)": "Mixer, routing, I/O, CPU (ch 17, 18, 38)",
    "Automation, envelopes (ch 26, 27)": "Automation and envelopes (ch 26, 27)",
    "Generative/transform (ch 11-15)": "MIDI tools, MPE, grooves, tuning (ch 11-15)",
    "Recording, bounce, comp (ch 19-22)": "Recording, bounce, comping (ch 19-22)",
    "Outside scope (ch 28, 32-37, 41)": "Outside a DAW's own scope (ch 28, 32-37, 41)",
    "Setup, shortcuts (ch 2, 3, 42)": "Setup, concepts, shortcuts (ch 2, 3, 42)",
}


def setup():
    plt.rcParams.update({
        "font.family": "DejaVu Sans",
        "font.size": 10,
        "axes.edgecolor": "#888888",
        "axes.labelcolor": "#222222",
        "text.color": "#222222",
        "xtick.color": "#444444",
        "ytick.color": "#222222",
        "figure.facecolor": "white",
        "axes.facecolor": "white",
    })


def stamp(fig):
    """Say on the figure which measurement it draws, so a stale one is visible at a glance."""
    fig.text(0.995, 0.005, f"measured {score.MEASURED_AT} at {score.MEASURED_COMMIT}",
             ha="right", va="bottom", fontsize=8, color="#777777")


def fig_areas(path: Path):
    rows = []
    for name, nums in score.AREAS.items():
        sel = [r for r in score.CHAPTERS if r[0] in nums and r[3] is not None]
        cov, pages = score.weighted(sel)
        rows.append((cov, pages, LABELS.get(name, name)))
    rows.sort()

    # Drawn at a size that stays legible when the README shows it at ~700 px wide and the report
    # at ~830: the label and value text must not fall below the body text around it.
    fig, ax = plt.subplots(figsize=(7.6, 4.1))
    ys = list(range(len(rows)))
    colours = [COLOURS["green"] if c >= 0.6 else COLOURS["light"] if c >= 0.35
               else COLOURS["amber"] if c >= 0.1 else COLOURS["grey"] for c, _, _ in rows]
    ax.barh(ys, [c * 100 for c, _, _ in rows], color=colours, height=0.62)
    for y, (cov, _, _) in zip(ys, rows):
        ax.text(cov * 100 + 1.2, y, f"{cov*100:.0f}%",
                va="center", fontsize=9.5, color="#333333")
    ax.set_yticks(ys)
    ax.set_yticklabels(["\n".join(textwrap.wrap(n, 36)) for _, _, n in rows], fontsize=9.5)
    ax.set_xlim(0, 60)
    ax.set_xlabel("share of the manual's functionality Suhina covers (page-weighted)", fontsize=10)
    ax.set_title("Parity with Ableton Live 12 by area", fontsize=12.5, loc="left", pad=10)
    ax.grid(axis="x", color="#e6e6e6", linewidth=0.8)
    ax.set_axisbelow(True)
    for side in ("top", "right"):
        ax.spines[side].set_visible(False)
    stamp(fig)
    fig.tight_layout()
    fig.savefig(path, dpi=170, metadata={"Software": None})
    plt.close(fig)
    print(f"wrote {path}")


def fig_overview(path: Path):
    scored = [r for r in score.CHAPTERS if r[3] is not None]
    buckets = [
        ("at parity or close (>=60%)", COLOURS["green"], lambda c: c >= 0.6),
        ("partial (30-60%)", COLOURS["light"], lambda c: 0.3 <= c < 0.6),
        ("thin (1-30%)", COLOURS["amber"], lambda c: 0.01 <= c < 0.3),
        ("absent (<1%)", COLOURS["grey"], lambda c: c < 0.01),
    ]
    fig, (ax1, ax2) = plt.subplots(1, 2, figsize=(10.2, 3.5),
                                   gridspec_kw={"width_ratios": [1.25, 1]})

    left = 0.0
    total_pages = sum(r[2] for r in scored)
    for name, colour, test in buckets:
        pages = sum(r[2] for r in scored if test(r[3]))
        ax1.barh([0], [pages], left=left, color=colour, height=0.5)
        if pages:
            ax1.text(left + pages / 2, 0, f"{pages}p\n{pages / total_pages * 100:.0f}%",
                     ha="center", va="center", fontsize=8.5,
                     color="white" if colour in (COLOURS["green"], COLOURS["amber"]) else "#333333")
        left += pages
    ax1.set_xlim(0, left)
    ax1.set_ylim(-0.6, 0.9)
    ax1.set_yticks([])
    ax1.set_xlabel(f"pages of the {total_pages}-page manual body")
    ax1.set_title("What the manual's pages are made of", fontsize=11, loc="left", pad=10)
    ax1.legend(handles=[Patch(facecolor=c, label=n) for n, c, _ in buckets],
               loc="upper center", bbox_to_anchor=(0.5, -0.42), ncol=2, frameon=False, fontsize=8)
    for side in ("top", "right", "left"):
        ax1.spines[side].set_visible(False)

    counts = device_counts()
    categories = ["audio\neffects", "MIDI\neffects", "instruments"]
    live = [counts["audio_effects"], counts["midi_effects"], counts["instruments"]]
    suhina = [counts["audio_effects_equivalent"], counts["midi_effects_equivalent"], counts["instruments_equivalent"]]
    xs = range(3)
    ax2.bar([i - 0.19 for i in xs], live, width=0.36, color=COLOURS["grey"], label="Live 12")
    ax2.bar([i + 0.19 for i in xs], suhina, width=0.36, color=COLOURS["green"], label="Suhina")
    for i, (a, b) in enumerate(zip(live, suhina)):
        ax2.text(i - 0.19, a + 0.6, str(a), ha="center", fontsize=8.5)
        ax2.text(i + 0.19, b + 0.6, str(b), ha="center", fontsize=8.5, color=COLOURS["green"])
    ax2.set_xticks(list(xs))
    ax2.set_xticklabels(categories)
    ax2.set_ylim(0, max(live) + 8)
    ax2.set_ylabel("devices documented")
    ax2.set_title("Device catalogue", fontsize=11, loc="left", pad=10)
    ax2.grid(axis="y", color="#e6e6e6", linewidth=0.8)
    ax2.set_axisbelow(True)
    for side in ("top", "right"):
        ax2.spines[side].set_visible(False)
    ax2.legend(frameon=False, fontsize=8.5)

    stamp(fig)
    fig.tight_layout()
    fig.savefig(path, dpi=170, metadata={"Software": None})
    plt.close(fig)
    print(f"wrote {path}")


def device_counts() -> dict:
    """Live's device counts and how many equivalents Suhina has, for the second panel."""
    import json
    path = Path(__file__).resolve().parent / "devices.json"
    devices = json.loads(path.read_text(encoding="utf-8"))
    audio, midi, instruments = (devices[key] for key in
                                ("audio_effects", "midi_effects", "instruments"))
    return {
        "audio_effects": len(audio),
        "midi_effects": len(midi),
        "instruments": len(instruments),
        "audio_effects_equivalent": sum(1 for r in audio if r[3] == "equivalent"),
        "midi_effects_equivalent": sum(1 for r in midi if r[3] in ("equivalent", "close")),
        "instruments_equivalent": sum(1 for r in instruments if r[3] in ("equivalent", "close")),
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", default=str(REPO / "docs" / "research"))
    args = parser.parse_args()

    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    setup()
    fig_areas(out / "parity-live-12-by-area.png")
    fig_overview(out / "parity-live-12-overview.png")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
