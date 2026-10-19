# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Update the marked sections of docs/research/parity-live-12.md, in place.

The report is an ordinary document: its prose is edited in markdown, by hand or by an agent. The
sections between `<!-- parity:begin key -->` and `<!-- parity:end -->` are written from
`scripts/parity/score.py` and `scripts/parity/devices.json`, so the numbers cannot rot, and
everything outside the markers is never touched.

    python scripts/parity/update_report.py                 # rewrite the marked sections
    python scripts/parity/update_report.py --check-inputs  # what CI runs on a pull request
    python scripts/parity/update_report.py --check          # non-zero when it would change

The marked sections are written on main after every merge (.github/workflows/docs-generated.yml),
like docs/INDEX.md and docs/commands.md, so pull requests change only the inputs (`CHAPTERS` in
score.py, devices.json) and never conflict on the numbers. `--check-inputs` renders every section
and checks the markers without comparing them with what is committed, so a pull request passes
whether its marked sections are stale or were regenerated. The command count is read from
docs/commands.md, which the same workflow writes from the command registry just before.

The rule that keeps the two apart: **a number lives in a marked section, and prose does not quote a
number the markers already carry.** If a sentence needs one, the sentence goes inside a marker.
"""

import argparse
import difflib
import json
import re
import sys
from pathlib import Path

import score

REPO = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent
DOC = REPO / "docs" / "research" / "parity-live-12.md"
# A row of the commands table in docs/commands.md: `| `id` | kind | ...`.
COMMAND_ROW = re.compile(r"^\| `[^`]+` \|", re.M)

MARKED = re.compile(r"<!-- parity:begin ([a-z0-9-]+) -->.*?<!-- parity:end -->", re.S)


def tally(rows) -> dict:
    counts = {"equivalent": 0, "close": 0, "planned": 0, "none": 0}
    for row in rows:
        counts[row[3]] = counts.get(row[3], 0) + 1
    return counts


def block(body: str) -> str:
    """A marked section that owns whole lines."""
    return "\n" + body.strip("\n") + "\n"


def chapter_table() -> str:
    lines = ["| # | Chapter | Manual pages | Suhina covers | Status | What exists / what is missing |",
             "|---|---|---|---|---|---|"]
    for num, title, pages, cov, status, note in score.CHAPTERS:
        cover = "—" if cov is None else f"{cov*100:.0f} %"
        lines.append(f"| {num} | {title} | {pages} | {cover} | {status} | {' '.join(note.split())} |")
    return block("\n".join(lines))


def area_table() -> str:
    lines = ["| Area | Manual pages | Covered |", "|---|---|---|"]
    for name, nums in score.AREAS.items():
        cov, pages = score.weighted([r for r in score.CHAPTERS if r[0] in nums and r[3] is not None])
        lines.append(f"| {name} | {pages} | {cov*100:.0f} % |")
    return block("\n".join(lines))


def device_table(rows) -> str:
    lines = ["| Device | Manual | Kind | Suhina | Note |", "|---|---|---|---|---|"]
    for name, section, kind, status, note in rows:
        lines.append(f"| {name} | {section} | {kind} | {status} | {note} |")
    return block("\n".join(lines))


def command_count(path: Path) -> int:
    """The number of commands docs/commands.md lists, generated from the command registry."""
    try:
        text = path.read_text(encoding="utf-8")
    except OSError as error:
        raise SystemExit(f"{path}: cannot read the command list ({error})")
    count = len(COMMAND_ROW.findall(text))
    if count == 0:
        raise SystemExit(f"{path}: lists no commands")
    return count


def headline(devices) -> str:
    audio, midi, instruments = (tally(devices[key]) for key in
                                ("audio_effects", "midi_effects", "instruments"))
    scored = [r for r in score.CHAPTERS if r[3] is not None]
    all_cov, _ = score.weighted(scored)
    core = [r for r in scored if r[0] not in score.OUT_OF_SCOPE]
    core_cov, _ = score.weighted(core)
    scope = [r for r in core if r[0] != 41]
    scope_cov, _ = score.weighted(scope)
    metrics = score.section_metrics(REPO / "target" / "parity")
    live_total = sum(len(devices[key]) for key in ("audio_effects", "midi_effects", "instruments"))
    effects = sum(1 for d in devices["suhina_devices"] if d[1] == "effect")
    own_instruments = sum(1 for d in devices["suhina_devices"] if d[1] == "instrument")
    own_midi = sum(1 for d in devices["suhina_devices"] if d[1] == "MIDI effect")
    lines = [
        "| Measure | Value |",
        "|---|---|",
        f"| The manual it was measured from | {score.MANUAL_CHAPTERS} chapters, "
        f"{score.MANUAL_PDF_PAGES} PDF pages, {sum(r[2] for r in scored)} pages of feature text |",
        f"| Page-weighted coverage of everything the manual documents | **about {all_cov*100:.0f} %** |",
        f"| Same, without Push 1 and 2, Max for Live (+ its devices) and video | **about {core_cov*100:.0f} %** |",
        f"| Same, over the scope the roadmap aims at (also without accessibility) | **about {scope_cov*100:.0f} %** |",
        f"| Weighting each chapter by its numbered sections instead of its pages | "
        f"**{metrics['coverage']*100:.0f} %** (the two weightings agree) |",
        f"| Numbered manual sections | **{metrics['total']}**, of which "
        f"**{metrics['zero']} ({metrics['zero'] / metrics['total'] * 100:.0f} %)** sit in chapters "
        f"Suhina has not started at all |",
        f"| Built-in devices | **{len(devices['suhina_devices'])}** ({effects} effects, "
        f"{own_midi} MIDI effect, {own_instruments} instrument) against Live's {live_total} "
        f"({len(devices['audio_effects'])} audio effects, {len(devices['midi_effects'])} MIDI effects, "
        f"{len(devices['instruments'])} instruments) |",
        f"| Commands | **{score.COMMANDS}**, every one reachable from the UI, the CLI and MCP |",
        "| Still absent as whole features | MPE, Grooves, Tuning, Comping, "
        "Racks, Clip Envelopes, Push, Link, video, accessibility |",
    ]
    return block("\n".join(lines))


def device_counts(devices) -> str:
    audio, midi, instruments = (tally(devices[key]) for key in
                                ("audio_effects", "midi_effects", "instruments"))
    live_total = sum(len(devices[key]) for key in ("audio_effects", "midi_effects", "instruments"))
    have = (audio["equivalent"] + midi["equivalent"] + midi["close"] + instruments["equivalent"]
            + instruments["close"])
    planned = audio["planned"] + midi["planned"] + instruments["planned"]
    never = audio["none"] + midi["none"] + instruments["none"]
    lines = [
        "| | Live 12 | Suhina equivalent exists | Suhina plans one (ROADMAP) | No counterpart named |",
        "|---|---|---|---|---|",
        f"| Audio effects | {len(devices['audio_effects'])} | {audio['equivalent']} | "
        f"{audio['planned']} | {audio['none']} |",
        f"| MIDI effects | {len(devices['midi_effects'])} | {midi['equivalent'] + midi['close']} | "
        f"{midi['planned']} | {midi['none']} |",
        f"| Instruments | {len(devices['instruments'])} | "
        f"{instruments['equivalent'] + instruments['close']} | {instruments['planned']} | "
        f"{instruments['none']} |",
        f"| **All** | **{live_total}** | **{have}** | **{planned}** | **{never}** |",
    ]
    return block("\n".join(lines))


def device_none(devices) -> str:
    audio, midi, instruments = (tally(devices[key]) for key in
                                ("audio_effects", "midi_effects", "instruments"))
    live_total = sum(len(devices[key]) for key in ("audio_effects", "midi_effects", "instruments"))
    never = audio["none"] + midi["none"] + instruments["none"]
    return block(
        "Read the last two columns together: **even if every device the ROADMAP names is built, "
        f"{never} of Live's {live_total} devices ({never / live_total * 100:.0f} %)\n"
        f"still have no counterpart** - {midi['none']} of the {len(devices['midi_effects'])} MIDI "
        "effects, the amp and cabinet models, the "
        "modulated-delay\nfamily (chorus, ensemble, vibrato, phaser, flanger, doubler), the spectral "
        "pair, the analysers\n(Spectrum, Tuner), the gate, the grain and beat repeaters, the vocoder, "
        "and the synths the roadmap\ndoes not name (Meld, Electric, Impulse, External Instrument)."
    )


def suhina_devices(devices) -> str:
    lines = ["| Device | Kind | What it does |", "|---|---|---|"]
    for name, kind, what in devices["suhina_devices"]:
        lines.append(f"| {name} | {kind} | {what} |")
    return block("\n".join(lines))


GAPS = [
    ("Devices: effects and instruments", "Devices (ch 23-25, 29-31)",
     "35 more audio effects and 12 more instruments, or a decision to cover the common ones only"),
    ("Browser and file management depth", "Browser, files, project (ch 4, 5)",
     "collections, tag editor, project folders, collect files, unused files, packs, cloud"),
    ("Views and clip editing", "Views and editing (ch 6-10, 16)",
     "MIDI note editor, clip view panels, time selections"),
    ("MIDI tools, MPE, grooves, tuning", "Generative/transform (ch 11-15)",
     "transformation and generative tools, MPE editing, groove pool, tuning systems"),
    ("Automation and clip envelopes", "Automation, envelopes (ch 26, 27)",
     "drawing and recording automation lanes, send and tempo automation, clip envelopes"),
    ("Recording, bounce, comping", "Recording, bounce, comp (ch 19-22)",
     "overdub, punch in and out, take lanes, comping, per-track bounce"),
    ("Mixing, routing, I/O", "Mixing, routing, I/O (ch 17, 18, 38)",
     "crossfader, inputs and monitoring, resampling"),
    ("Outside a DAW's own scope", "Outside scope (ch 28, 32-37, 41)",
     "video, Max for Live, Push 1 and 2, Link, accessibility - ADR 0016 leaves most of this out"),
]


def gap_table() -> str:
    lines = ["| Gap | Manual pages | Today | What closing it means |", "|---|---|---|---|"]
    for gap, area, meaning in GAPS:
        cov, pages = score.weighted(
            [r for r in score.CHAPTERS if r[0] in score.AREAS[area] and r[3] is not None])
        chapters = area[area.index("(") + 1:-1] if "(" in area else ""
        lines.append(f"| {gap} | {pages} ({chapters}) | {cov*100:.0f} % | {meaning} |")
    return block("\n".join(lines))


def values(devices) -> dict:
    return {
        "title": f"# Parity with Ableton Live 12 (status check, {score.MEASURED_AT})",
        "headline-table": headline(devices),
        "chapter-table": chapter_table(),
        "area-table": area_table(),
        "device-counts": device_counts(devices),
        "device-none": device_none(devices),
        "device-audio": device_table(devices["audio_effects"]),
        "device-midi": device_table(devices["midi_effects"]),
        "device-instruments": device_table(devices["instruments"]),
        "suhina-devices": suhina_devices(devices),
        "gap-table": gap_table(),
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--doc", default=str(DOC))
    parser.add_argument("--commands", type=Path, default=None,
                        help="the generated command list to count (default docs/commands.md)")
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--check", action="store_true",
                      help="change nothing; fail when the marked sections are out of date")
    mode.add_argument("--check-inputs", action="store_true",
                      help="change nothing; fail only when the sections cannot be rendered or "
                           "the markers are wrong, whatever the marked sections hold now")
    args = parser.parse_args()

    path = Path(args.doc)
    try:
        devices = json.loads((HERE / "devices.json").read_text(encoding="utf-8"))
    except (OSError, ValueError) as error:
        raise SystemExit(f"devices.json: {error}")
    # The count the headline quotes comes from the generated command list, not from score.py.
    score.COMMANDS = command_count(args.commands or REPO / "docs" / "commands.md")
    rendered = values(devices)

    text = path.read_text(encoding="utf-8")
    seen = []

    def replace(match):
        key = match.group(1)
        if key not in rendered:
            raise SystemExit(f"{path}: marker {key!r} has no renderer in update_report.py")
        seen.append(key)
        return f"<!-- parity:begin {key} -->{rendered[key]}<!-- parity:end -->"

    updated = MARKED.sub(replace, text)

    duplicates = sorted({key for key in seen if seen.count(key) > 1})
    if duplicates:
        raise SystemExit(f"{path}: marker used more than once: {', '.join(duplicates)}")
    missing = sorted(set(rendered) - set(seen))
    if missing:
        raise SystemExit(f"{path}: no marker for {', '.join(missing)}")

    if args.check_inputs:
        state = "up to date" if updated == text else "not current, which main regenerates after the merge"
        print(f"{path.name}: the inputs render and the markers are fine; the marked sections are {state}")
        return 0

    if updated == text:
        print(f"{path.name}: the marked sections are up to date")
        return 0

    if args.check:
        diff = difflib.unified_diff(text.splitlines(), updated.splitlines(),
                                    fromfile="committed", tofile="rendered", lineterm="", n=1)
        sys.stderr.write("\n".join(diff) + "\n")
        sys.stderr.write(
            f"\n{path.name}: the marked sections are out of date. On main the docs workflow "
            f"rewrites them; a pull request leaves them to it.\n")
        return 1

    path.write_text(updated, encoding="utf-8", newline="\n")
    print(f"{path.name}: marked sections updated")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
