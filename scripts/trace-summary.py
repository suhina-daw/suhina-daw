#!/usr/bin/env python3
# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Sums up a trace log (`suhina --trace <file>`, docs/debugging.md): what a person testing heard
and saw, and where to look first. With --follow it prints the lines worth acting on as they come.

Usage:
  python scripts/trace-summary.py <trace.log> [--since SECONDS] [--last N]
  python scripts/trace-summary.py <trace.log> --follow

  --since SECONDS  only the last SECONDS of the log (by its timestamps)
  --last N         how many of the slowest frames and of the last actions to list (default 10)
  --follow         from the end of the file on: each audio line where something went missing,
                   each frame of 100 ms or more, plug-in reloads, failures and panics

The summary has: the processes and the plug-ins each host runs; the audio (device underruns,
plug-in blocks late and the plug-in most often late, stretched frames late, the highest engine
load), each with when it happened; the slow frames by the part that took the time; the slow
engine updates; the plug-ins that loaded twice, failed or took long to let go; the ARA documents
and their notes; and the last actions.
"""

import re
import sys
import time
from collections import Counter, defaultdict

LINE = re.compile(r"^(\d{13}) \[(\d+)\] (.*)$")


def parse(path, since=None):
    entries = []
    with open(path, encoding="utf-8", errors="replace") as f:
        for raw in f:
            m = LINE.match(raw.rstrip("\n"))
            if m:
                entries.append((int(m.group(1)), int(m.group(2)), m.group(3)))
    if since is not None and entries:
        cut = entries[-1][0] - int(since * 1000)
        entries = [e for e in entries if e[0] >= cut]
    return entries


def at(ms, start):
    """Seconds since the log's start, as m:ss."""
    s = max(0, (ms - start) // 1000)
    return f"{s // 60}:{s % 60:02d}"


def summary(entries, last):
    if not entries:
        print("the log has no trace lines")
        return
    start, end = entries[0][0], entries[-1][0]
    print(f"{len(entries)} lines over {at(end, start)} (m:ss); times below are from the start\n")

    # Processes: the app writes `audio:` and `frames:`, hosts `host ...`.
    roles = {}
    plugins = defaultdict(set)
    instance_plugin = {}
    for ms, pid, text in entries:
        if text.startswith(("frames:", "action ", "slow frame", "audio:")):
            roles.setdefault(pid, "app")
        m = re.match(r'host gets Load \{ instance: (\d+), plugin: "([^"]*)"', text)
        if m:
            roles.setdefault(pid, "host")
            name = re.sub(r"\.(vst3|clap)\b.*$", "", m.group(2).replace("\\\\", "\\").split("\\")[-1])
            plugins[pid].add(name)
            instance_plugin[(pid, m.group(1))] = name
    print("Processes")
    for pid, role in roles.items():
        extra = f": {', '.join(sorted(plugins[pid]))}" if plugins[pid] else ""
        print(f"  [{pid}] {role}{extra}")

    # Audio: the app's once-a-second line when something went missing or the load was high.
    print("\nAudio")
    underruns, late_blocks, stretched, loads = [], [], [], []
    worst = Counter()
    for ms, pid, text in entries:
        if not text.startswith("audio: device underruns"):
            continue
        m = re.search(r"device underruns \d+ \(\+(\d+)\)", text)
        if m and int(m.group(1)):
            underruns.append((ms, int(m.group(1))))
        m = re.search(r"stretched frames late \d+ \(\+(\d+)\)", text)
        if m and int(m.group(1)):
            stretched.append((ms, int(m.group(1))))
        m = re.search(r"plug-in blocks late \d+ \(\+(\d+), most (.*?): (\d+)\)", text)
        if m and int(m.group(1)):
            late_blocks.append((ms, int(m.group(1))))
            worst[m.group(2)] += int(m.group(3))
        m = re.search(r"engine load ([\d.]+) %, (\w+)", text)
        if m:
            loads.append((float(m.group(1)), ms, m.group(2)))

    def when(events):
        return ", ".join(f"{at(ms, start)} +{n}" for ms, n in events[:12]) + (" …" if len(events) > 12 else "")

    print(f"  device underruns: {sum(n for _, n in underruns)}" + (f" ({when(underruns)})" if underruns else ""))
    print(f"  plug-in blocks late: {sum(n for _, n in late_blocks)}" + (f" ({when(late_blocks)})" if late_blocks else ""))
    for name, n in worst.most_common(5):
        print(f"    most often late: {name} ({n})")
    print(f"  stretched frames late: {sum(n for _, n in stretched)}" + (f" ({when(stretched)})" if stretched else ""))
    if loads:
        load, ms, state = max(loads)
        print(f"  highest engine load: {load:.1f} % at {at(ms, start)} ({state})")
    print("  (a few late blocks as playback starts, and stretched frames late right after a jump, are normal)")

    # Slow frames: the part that took the most of each.
    print("\nSlow frames (25 ms or more)")
    frames = []
    for ms, pid, text in entries:
        m = re.match(r"slow frame ([\d.]+) ms: (.*?);", text)
        if m:
            parts = [(float(v), k.strip()) for k, v in re.findall(r"([a-z -]+?) ([\d.]+)(?:,|$)", m.group(2))]
            main = max(parts)[1] if parts else "?"
            frames.append((float(m.group(1)), ms, main, text))
    by_part = Counter(main for _, _, main, _ in frames)
    print(f"  {len(frames)} frames; by the part that took the most: "
          + ", ".join(f"{part} {n}" for part, n in by_part.most_common()))
    for took, ms, main, text in sorted(frames, reverse=True)[:last]:
        print(f"  {at(ms, start)}  {took:7.1f} ms  {text[text.index(':') + 2:][:150]}")
    if any(main == "rest" for _, _, main, _ in frames):
        print("  ('rest' is time after the frame's last timed part, outside the parts it names)")

    updates = [float(m.group(1)) for _, _, t in entries if (m := re.match(r"slow engine update: ([\d.]+) ms", t))]
    if updates:
        print(f"\nSlow engine updates: {len(updates)}, longest {max(updates):.1f} ms")

    # Plug-ins: reloads, failures, slow teardowns, hangs.
    print("\nPlug-ins")
    rebinds = [(ms, t) for ms, _, t in entries if t.startswith("rack rebinds")]
    shared = [(ms, t) for ms, _, t in entries if "shared with another track" in t]
    failed = [(ms, t) for ms, _, t in entries if "could not start" in t or "Failed" in t]
    unloading = {}
    slow_unloads = []
    for ms, pid, text in entries:
        m = re.match(r"host unloads (\d+)", text)
        if m:
            unloading[(pid, m.group(1))] = ms
        m = re.match(r"host unloaded (\d+) in ([\d.]+)(ms|s)", text)
        if m:
            unloading.pop((pid, m.group(1)), None)
            took = float(m.group(2)) * (1 if m.group(3) == "ms" else 1000)
            if took >= 1000:
                slow_unloads.append((ms, pid, m.group(1), took))
    print(f"  loaded again with their clips (ARA rebinds): {len(rebinds)}"
          + (f", first at {at(rebinds[0][0], start)}" if rebinds else ""))
    print(f"  ARA documents shared by tracks: {len(shared)}")
    for ms, text in failed[:5]:
        print(f"  failed at {at(ms, start)}: {text[:160]}")
    for ms, pid, inst, took in slow_unloads[:5]:
        print(f"  slow to let go at {at(ms, start)}: {instance_plugin.get((pid, inst), inst)} {took:.0f} ms")
    for (pid, inst), ms in unloading.items():
        print(f"  never let go (a hang?): {instance_plugin.get((pid, inst), inst)}, asked at {at(ms, start)}")
    panics = [(ms, t) for ms, _, t in entries if "panicked" in t]
    for ms, text in panics[:5]:
        print(f"  PANIC at {at(ms, start)}: {text[:200]}")

    # ARA: notes per region, analysis.
    notes = {}
    for _, _, text in entries:
        m = re.match(r"ara notes (clip-[\d-]+): (\d+) notes", text)
        if m:
            notes[m.group(1)] = int(m.group(2))
    if notes:
        with_notes = sum(1 for n in notes.values() if n)
        print(f"\nARA: {len(notes)} regions, {with_notes} with notes")

    actions = [(ms, t[len("action "):]) for ms, _, t in entries if t.startswith("action ")]
    if actions:
        counts = Counter(a.split(" ", 1)[0] for _, a in actions)
        print(f"\nActions: {len(actions)}; " + ", ".join(f"{k} {n}" for k, n in counts.most_common(8)))
        for ms, a in actions[-last:]:
            print(f"  {at(ms, start)}  {a[:140]}")


def follow(path):
    """Prints the lines worth acting on as they are written."""
    with open(path, encoding="utf-8", errors="replace") as f:
        f.seek(0, 2)
        while True:
            raw = f.readline()
            if not raw:
                time.sleep(0.3)
                continue
            m = LINE.match(raw.rstrip("\n"))
            if not m:
                continue
            text = m.group(3)
            if text.startswith("audio: device underruns"):
                grew = re.findall(r"\(\+(\d+)", text)
                if any(int(n) for n in grew):
                    print(text[:260], flush=True)
            elif (s := re.match(r"slow frame ([\d.]+) ms", text)) and float(s.group(1)) >= 100:
                print(text[:240], flush=True)
            elif re.search(r"rack rebinds|could not start|panicked|shared with another track", text):
                print(text[:240], flush=True)


def main(argv):
    if len(argv) < 2 or argv[1] in ("-h", "--help"):
        print(__doc__)
        return 0 if len(argv) >= 2 else 2
    path = argv[1]
    if "--follow" in argv:
        follow(path)
        return 0
    since = float(argv[argv.index("--since") + 1]) if "--since" in argv else None
    last = int(argv[argv.index("--last") + 1]) if "--last" in argv else 10
    summary(parse(path, since), last)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
