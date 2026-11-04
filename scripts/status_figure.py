# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Draw the README's status figure: how much of each area of Suhina works today.

    python scripts/status_figure.py --status STATUS_JSON [--bugs BUGS_JSON]
    python scripts/status_figure.py --status STATUS_JSON --check

STATUS_JSON is the maintainers' feature count (status.json of the private repository
suhina-daw/suhina-parity: per area, how many features are done, started and to do).
BUGS_JSON, when given, holds the open bugs per `area:` label ({"launcher": 1, ...}).
.github/workflows/docs-generated.yml fetches both on main and runs this; a pull request
leaves the outputs alone.

Writes docs/status.svg and docs/status-dark.svg, one box per area sized by its number of
features and filled by the share done, and the block between the status markers in README.md.
Every label is measured against its box before it is drawn: a name that does not fit is
wrapped or shortened, and a line of numbers that does not fit is replaced by a shorter one or
left out, so nothing spills over a box's edge.
"""

import argparse
import json
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
README = REPO / "README.md"
LIGHT = REPO / "docs" / "status.svg"
DARK = REPO / "docs" / "status-dark.svg"
START = "<!-- status:start (written by scripts/status_figure.py on main; edit that, not this) -->"
END = "<!-- status:end -->"

# The area: labels, with the name the figure gives each one. Short, so they fit small boxes.
NAMES = {
    "plug-ins": "Plug-ins",
    "effects": "Built-in effects",
    "look": "Look and settings",
    "devices": "Devices",
    "arrangement": "Arrangement",
    "projects": "Projects and files",
    "instruments": "Instruments",
    "midi-editing": "MIDI editing",
    "mixing": "Mixing and routing",
    "browser": "Browser",
    "clips": "Clip editor",
    "launcher": "Launcher",
    "recording": "Recording",
    "audio": "Audio and warping",
    "audio-system": "CPU and audio",
    "automation": "Automation",
    "control": "Sync and control",
    "export": "Export",
    "audio-to-midi": "Audio to MIDI",
    "agents": "Agents",
}

WIDTH, HEIGHT = 680.0, 480.0
TOP = 34.0  # room for the legend above the boxes
GAP = 1.5
PAD = 8.0
MIN_SHARE = 0.045  # the smallest box gets this share of the area, so its name and number fit
FONT = "-apple-system, BlinkMacSystemFont, 'Segoe UI', Helvetica, Arial, sans-serif"

THEMES = {
    "light": {"bg": "#ffffff", "box": "#F1EFE8", "fill": "#1D9E75", "wash": 0.16,
              "started": "#EF9F27", "ink": "#2C2C2A", "sub": "#5F5E5A", "bug": "#E24B4A",
              "track": "#B4B2A9"},
    "dark": {"bg": "#0d1117", "box": "#2C2C2A", "fill": "#1D9E75", "wash": 0.25,
             "started": "#EF9F27", "ink": "#F1EFE8", "sub": "#B4B2A9", "bug": "#E24B4A",
             "track": "#5F5E5A"},
}

# Advance widths of a Helvetica-metric sans in thousandths of an em (Arial and Liberation Sans
# share them). The figure names a system font, which differs a little from these between
# systems, so every width is taken SAFETY times larger than measured.
_W = dict(zip(
    " !\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`abcdefghijklmnopqrstuvwxyz{|}~",
    [278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278,
     556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 278, 278, 584, 584, 584, 556, 1015,
     667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, 722, 778, 667, 778, 722,
     667, 611, 722, 667, 944, 667, 667, 611, 278, 278, 278, 469, 556, 333,
     556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, 556, 556, 556, 556, 333,
     500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584],
))
_W.update({"·": 278, "…": 1000})
BOLD = 1.08
SAFETY = 1.08


def measure(text: str, size: float, bold: bool = False) -> float:
    units = sum(_W.get(ch, 600) for ch in text)
    return units / 1000.0 * size * (BOLD if bold else 1.0) * SAFETY


def esc(text: str) -> str:
    return text.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")


# --- the data -----------------------------------------------------------------------------------

def load_areas(status: dict, bugs: dict) -> list:
    """One dict per area with features: name, done, started, todo, total, bugs."""
    areas = []
    counts = status.get("areas")
    if not isinstance(counts, dict):
        raise ValueError("status: no 'areas' object")
    for key, value in counts.items():
        if key not in NAMES:
            raise ValueError(f"status: unknown area {key!r}")
        try:
            done, started, todo = (int(value[k]) for k in ("done", "started", "todo"))
        except (KeyError, TypeError, ValueError) as error:
            raise ValueError(f"status: area {key!r} needs whole numbers done, started, todo") from error
        if min(done, started, todo) < 0:
            raise ValueError(f"status: area {key!r} has a negative count")
        total = done + started + todo
        if total == 0:
            continue
        open_bugs = bugs.get(key, 0)
        if not isinstance(open_bugs, int) or open_bugs < 0:
            raise ValueError(f"bugs: area {key!r} needs a whole number")
        areas.append({"key": key, "name": NAMES[key], "done": done, "started": started,
                      "todo": todo, "total": total, "bugs": open_bugs})
    if not areas:
        raise ValueError("status: no area has any features")
    return areas


# --- the layout ---------------------------------------------------------------------------------

def squarify(items: list, x: float, y: float, w: float, h: float) -> list:
    """Lay items (each with a 'weight') out in rows of boxes as close to square as they go."""
    out = []
    rest = list(items)
    total = sum(i["weight"] for i in rest)
    while rest:
        short = min(w, h)
        scale = w * h / total
        row, best = [], float("inf")
        for n in range(1, len(rest) + 1):
            cand = rest[:n]
            area = sum(i["weight"] for i in cand) * scale
            big = max(i["weight"] for i in cand) * scale
            small = min(i["weight"] for i in cand) * scale
            worst = max(short * short * big / (area * area), area * area / (short * short * small))
            if worst <= best:
                best, row = worst, cand
            else:
                break
        area = sum(i["weight"] for i in row) * scale
        thick = area / short
        offset = 0.0
        for item in row:
            length = item["weight"] * scale / thick
            if w >= h:
                out.append({**item, "x": x, "y": y + offset, "w": thick, "h": length})
            else:
                out.append({**item, "x": x + offset, "y": y, "w": length, "h": thick})
            offset += length
        if w >= h:
            x, w = x + thick, w - thick
        else:
            y, h = y + thick, h - thick
        total -= sum(i["weight"] for i in row)
        rest = rest[len(row):]
    return out


def layout(areas: list) -> list:
    total = sum(a["total"] for a in areas)
    floor = total * MIN_SHARE
    items = [{**a, "weight": max(a["total"], floor)} for a in areas]
    items.sort(key=lambda a: (-a["weight"], a["name"]))
    return squarify(items, 0.0, TOP, WIDTH, HEIGHT - TOP)


def wrap(text: str, width: float, size: float):
    """The words of text in lines that each fit width, or None when a single word does not."""
    lines, line = [], ""
    for word in text.split():
        cand = f"{line} {word}".strip()
        if measure(cand, size, True) <= width or not line:
            line = cand
        else:
            lines.append(line)
            line = word
    lines.append(line)
    if all(measure(s, size, True) <= width for s in lines):
        return lines
    return None


def name_lines(name: str, width: float):
    for size in (13, 12, 11):
        lines = wrap(name, width, size)
        if lines and len(lines) <= 2:
            return lines, size
    cut = name
    while len(cut) > 1 and measure(cut + "…", 11, True) > width:
        cut = cut[:-1]
    return [cut + "…"], 11


# --- the drawing --------------------------------------------------------------------------------

def text(x, y, size, fill, body, bold=False, anchor=None):
    weight = ' font-weight="600"' if bold else ""
    end = f' text-anchor="{anchor}"' if anchor else ""
    return (f'<text x="{x:.1f}" y="{y:.1f}" font-size="{size}"{weight}{end} '
            f'fill="{fill}">{esc(body)}</text>')


def box(r: dict, c: dict, index: int) -> list:
    x, y, w, h = r["x"] + GAP, r["y"] + GAP, r["w"] - 2 * GAP, r["h"] - 2 * GAP
    done_w = w * r["done"] / r["total"]
    started_w = w * r["started"] / r["total"]
    pct = round(100 * r["done"] / r["total"])
    title = (f'{r["name"]}: {r["done"]} done, {r["started"]} in progress, {r["todo"]} planned'
             + (f', {r["bugs"]} open bugs' if r["bugs"] else ""))
    out = [f'<g><title>{esc(title)}</title>',
           f'<clipPath id="b{index}"><rect x="{x:.1f}" y="{y:.1f}" width="{w:.1f}" '
           f'height="{h:.1f}" rx="6"/></clipPath>',
           f'<g clip-path="url(#b{index})">',
           f'<rect x="{x:.1f}" y="{y:.1f}" width="{w:.1f}" height="{h:.1f}" fill="{c["box"]}"/>',
           f'<rect x="{x:.1f}" y="{y:.1f}" width="{done_w:.1f}" height="{h:.1f}" '
           f'fill="{c["fill"]}" fill-opacity="{c["wash"]}"/>',
           f'<rect x="{x:.1f}" y="{y + h - 6:.1f}" width="{done_w:.1f}" height="6" fill="{c["fill"]}"/>',
           f'<rect x="{x + done_w:.1f}" y="{y + h - 6:.1f}" width="{started_w:.1f}" height="6" '
           f'fill="{c["started"]}"/>',
           '</g>']
    inner = w - 2 * PAD
    lines, size = name_lines(r["name"], inner)
    ty = y + PAD
    for n, line in enumerate(lines):
        out.append(text(x + PAD, ty + size + n * (size + 2), size, c["ink"], line, bold=True))
    ty += len(lines) * (size + 2) + size * 0.25
    # The open bugs sit in the bottom corner, above the bar, so names get the box's whole width.
    bottom = y + h - PAD - 4
    if r["bugs"]:
        label = str(r["bugs"])
        bw = max(22.0, measure(label, 11, True) + 10)
        bx, by = x + w - PAD - bw, y + h - 6 - 4 - 16
        out.append(f'<rect x="{bx:.1f}" y="{by:.1f}" width="{bw:.1f}" height="16" rx="4" '
                   f'fill="{c["bug"]}"/>')
        out.append(text(bx + bw / 2, by + 12, 11, "#ffffff", label, bold=True, anchor="middle"))
        bottom = by - 2
    for big in (28, 22, 16, 13):
        if measure(f"{pct}%", big, True) <= inner and ty + big + 4 <= bottom:
            out.append(text(x + PAD, ty + big, big, c["ink"], f"{pct}%", bold=True))
            ty += big + 4
            break
    else:
        # No room under the name: the share goes at the end of the name's last line, or nowhere.
        last = lines[-1]
        line_y = y + PAD + size + (len(lines) - 1) * (size + 2)
        if measure(last, size, True) + 6 + measure(f"{pct}%", size, True) <= inner:
            out.append(text(x + w - PAD, line_y, size, c["ink"], f"{pct}%", bold=True,
                            anchor="end"))
    started = f'{r["started"]} in progress · ' if r["started"] else ""
    for line in (f'{r["done"]} done · {started}{r["todo"]} planned',
                 f'{r["done"]} of {r["total"]} done', f'{r["done"]}/{r["total"]}'):
        if measure(line, 11) <= inner and ty + 13 <= bottom:
            out.append(text(x + PAD, ty + 11, 11, c["sub"], line))
            break
    out.append("</g>")
    return out


def legend(c: dict) -> list:
    out, x = [], 2.0
    for label, color in (("Done", c["fill"]), ("In progress", c["started"]),
                         ("Planned", c["track"]), ("Open bugs", c["bug"])):
        out.append(f'<rect x="{x:.1f}" y="11" width="10" height="10" rx="2" fill="{color}"/>')
        out.append(text(x + 15, 20, 12, c["sub"], label))
        x += 15 + measure(label, 12) + 16
    return out


def svg(areas: list, theme: str) -> str:
    c = THEMES[theme]
    done = sum(a["done"] for a in areas)
    total = sum(a["total"] for a in areas)
    head = [f'<svg xmlns="http://www.w3.org/2000/svg" width="{WIDTH:.0f}" height="{HEIGHT:.0f}" '
            f'viewBox="0 0 {WIDTH:.0f} {HEIGHT:.0f}" font-family="{FONT}" role="img" '
            f'aria-label="Suhina by area: {done} of {total} features done">',
            f'<rect width="{WIDTH:.0f}" height="{HEIGHT:.0f}" fill="{c["bg"]}"/>']
    summary = f"{round(100 * done / total)}% · {done} of {total} done"
    head.append(text(WIDTH - 2, 20, 12, c["ink"], summary, bold=True, anchor="end"))
    body = []
    for i, r in enumerate(layout(areas)):
        body += box(r, c, i)
    return "\n".join(head + legend(c) + body + ["</svg>"]) + "\n"


def block(areas: list) -> str:
    done = sum(a["done"] for a in areas)
    started = sum(a["started"] for a in areas)
    total = sum(a["total"] for a in areas)
    return "\n".join([
        START,
        "<picture>",
        '  <source media="(prefers-color-scheme: dark)" srcset="docs/status-dark.svg">',
        f'  <img width="680" alt="Suhina by area: {done} of {total} features done, '
        f'{started} in progress" src="docs/status.svg">',
        "</picture>",
        "",
        f"> How much of each area of Suhina works today: {done} of {total} features done and "
        f"{started} in progress. A box's size is its number of features, its fill the share "
        "done; the red number is the area's open bugs. Redrawn on `main` every day.",
        END,
    ])


def replace_block(readme: str, new: str):
    start = readme.find(START.split(" (")[0])
    end = readme.find(END)
    if start < 0 or end < start:
        return None
    return readme[:start] + new + readme[end + len(END):]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--status", required=True, help="status.json of the feature count")
    parser.add_argument("--bugs", help="open bugs per area, as JSON")
    parser.add_argument("--check", action="store_true", help="fail when the outputs are stale")
    args = parser.parse_args()

    try:
        status = json.loads(Path(args.status).read_text(encoding="utf-8"))
        bugs = json.loads(Path(args.bugs).read_text(encoding="utf-8")) if args.bugs else {}
        if not isinstance(bugs, dict):
            raise ValueError("bugs: expected an object of area: count")
        areas = load_areas(status, bugs)
    except (OSError, ValueError) as error:
        print(error)
        return 1

    readme = README.read_text(encoding="utf-8")
    new_readme = replace_block(readme, block(areas))
    if new_readme is None:
        print("README.md has no status markers")
        return 1
    outputs = {LIGHT: svg(areas, "light"), DARK: svg(areas, "dark"), README: new_readme}
    if args.check:
        stale = [p.name for p, body in outputs.items()
                 if not p.exists() or p.read_text(encoding="utf-8") != body]
        if stale:
            print("not up to date: " + ", ".join(stale))
            return 1
        return 0
    for path, body in outputs.items():
        path.write_text(body, encoding="utf-8", newline="\n")
    print(f"wrote {LIGHT.name}, {DARK.name} and the README block")
    return 0


if __name__ == "__main__":
    sys.exit(main())
