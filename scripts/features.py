# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""What Suhina does today and what is planned, by area: written from the issues and docs/features.json.

    python scripts/features.py            # write docs/features.md and the two figures (reads GitHub)
    python scripts/features.py --check    # what CI runs: features.json is valid; reads and writes nothing

The areas are the ones Suhina aims at (ADR 0016), each with an `area: …` label on GitHub. What
is counted in an area:

- every issue with its label that is not a bug or an epic: done when it was closed as completed,
  in progress while an open pull request says it closes it, planned while it is open otherwise;
- the features in docs/features.json without an issue of their own: what was built before the
  board existed, or is planned and has no issue yet (a feature whose `ref` names an issue is
  counted through that issue only);
- open bugs, shown beside the area but not in its bar.

docs/features.md and the figures at the top of the README (docs/features-by-area.svg and its dark
version) are written on main after every merge (.github/workflows/docs-generated.yml), so pull
requests change only features.json and the issues, and never conflict on what is written here.
GITHUB_TOKEN is used when it is set; the repository is public, so it also works without one.
"""

import argparse
import json
import os
import re
import sys
import urllib.error
import urllib.request
from html import escape
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MAP = ROOT / "docs" / "features.json"
LIST = ROOT / "docs" / "features.md"
FIGURES = {"light": ROOT / "docs" / "features-by-area.svg", "dark": ROOT / "docs" / "features-by-area-dark.svg"}
REPO = "suhina-daw/suhina-daw"
ISSUES = f"https://github.com/{REPO}/issues"
# Statuses in features.json, and the kinds an item is counted as.
STATUSES = ("done", "partial", "planned")
KINDS = ("done", "working", "planned")
KIND_OF = {"done": "done", "partial": "working", "planned": "planned"}
LABELS = {"done": "Done", "working": "In progress", "planned": "Planned"}
WORDS = {"done": "done", "working": "in progress", "planned": "planned"}
LABEL = re.compile(r"^area: [a-z0-9-]+$")
CLOSES = re.compile(r"\b(?:close[sd]?|fix(?:e[sd])?|resolve[sd]?)\s+#(\d+)", re.I)
PAGES = 30

# One hue, darkest for done, checked as an ordinal ramp against each surface.
THEMES = {
    "light": {"surface": "#fcfcfb", "ink": "#0b0b0b", "muted": "#52514e",
              "done": "#184f95", "working": "#3987e5", "planned": "#86b6ef"},
    "dark": {"surface": "#1a1a19", "ink": "#f0efec", "muted": "#c3c2b7",
             "done": "#9ec5f4", "working": "#3987e5", "planned": "#184f95"},
}


class MapError(Exception):
    pass


def issue_ref(ref) -> int | None:
    if isinstance(ref, str) and ref.startswith("#") and ref[1:].isdigit():
        return int(ref[1:])
    return None


def load(path: Path = MAP) -> list:
    """The areas of the map, checked: names, labels, statuses, no repeats."""
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError) as error:
        raise MapError(f"{path.name}: {error}")
    areas = data.get("areas") if isinstance(data, dict) else None
    if not isinstance(areas, list) or not areas:
        raise MapError("features.json: `areas` must be a list of at least one area")
    names, labels = set(), set()
    for area in areas:
        name = area.get("name") if isinstance(area, dict) else None
        if not isinstance(name, str) or not name.strip():
            raise MapError("features.json: every area needs a name")
        label = area.get("label")
        if not isinstance(label, str) or not LABEL.match(label):
            raise MapError(f"features.json: area {name!r} needs a label like `area: launcher`")
        if name in names or label in labels:
            raise MapError(f"features.json: area {name!r} or its label appears twice")
        names.add(name)
        labels.add(label)
        features = area.get("features", [])
        if not isinstance(features, list):
            raise MapError(f"features.json: the features of {name!r} must be a list")
        seen = set()
        for feature in features:
            fname = feature.get("name") if isinstance(feature, dict) else None
            if not isinstance(fname, str) or not fname.strip():
                raise MapError(f"features.json: a feature of {name!r} has no name")
            if fname in seen:
                raise MapError(f"features.json: {fname!r} appears twice in {name!r}")
            seen.add(fname)
            if feature.get("status") not in STATUSES:
                raise MapError(f"features.json: {fname!r} has status {feature.get('status')!r}, "
                               f"not one of {', '.join(STATUSES)}")
            ref = feature.get("ref")
            if ref is not None and (not isinstance(ref, str) or not ref.strip()):
                raise MapError(f"features.json: {fname!r} has an empty ref")
    return areas


def get(url: str) -> list:
    """Every page of a GitHub REST list."""
    headers = {"Accept": "application/vnd.github+json", "User-Agent": "suhina-features"}
    token = os.environ.get("GITHUB_TOKEN") or os.environ.get("GH_TOKEN")
    if token:
        headers["Authorization"] = f"Bearer {token}"
    out = []
    for page in range(1, PAGES + 1):
        sep = "&" if "?" in url else "?"
        request = urllib.request.Request(f"{url}{sep}per_page=100&page={page}", headers=headers)
        try:
            with urllib.request.urlopen(request, timeout=30) as response:
                batch = json.loads(response.read().decode("utf-8"))
        except (urllib.error.URLError, ValueError, OSError) as error:
            raise MapError(f"GitHub: {error}")
        if not isinstance(batch, list):
            raise MapError("GitHub: an answer that is not a list")
        out += batch
        if len(batch) < 100:
            return out
    raise MapError(f"GitHub: more than {PAGES} pages")


def fetch() -> tuple:
    """The repository's issues (pull requests left out) and its open pull requests."""
    api = f"https://api.github.com/repos/{REPO}"
    issues = [i for i in get(f"{api}/issues?state=all") if "pull_request" not in i]
    pulls = get(f"{api}/pulls?state=open")
    return issues, pulls


def tally(areas: list, issues: list, pulls: list) -> list:
    """Each area with its items (title, kind, issue number or None) and its open bugs."""
    closing = set()
    for pull in pulls:
        closing.update(int(n) for n in CLOSES.findall(pull.get("body") or ""))
    out = []
    for area in areas:
        items, bugs = [], []
        for feature in area.get("features", []):
            if issue_ref(feature.get("ref")) is None:
                items.append({"title": feature["name"], "kind": KIND_OF[feature["status"]],
                              "issue": None, "ref": feature.get("ref")})
        for issue in issues:
            names = {label.get("name") for label in issue.get("labels") or []}
            if area["label"] not in names or "epic" in names:
                continue
            number, open_ = issue["number"], issue.get("state") == "open"
            if "bug" in names:
                if open_:
                    bugs.append({"title": issue.get("title", ""), "issue": number})
                continue
            if open_:
                kind = "working" if number in closing else "planned"
            elif issue.get("state_reason") in (None, "completed"):
                kind = "done"
            else:
                continue
            items.append({"title": issue.get("title", ""), "kind": kind, "issue": number, "ref": None})
        out.append({"name": area["name"], "label": area["label"], "items": items, "bugs": bugs})
    return out


def counts(area: dict) -> dict:
    out = {k: 0 for k in KINDS}
    for item in area["items"]:
        out[item["kind"]] += 1
    return out


def breakdown(c: dict, bugs: int = 0) -> str:
    """"40 done · 3 in progress · 30 planned · 5 bugs", leaving out what is zero."""
    parts = [f"{c[k]} {WORDS[k]}" for k in KINDS if c[k]]
    if bugs:
        parts.append(f"{bugs} bug{'s' if bugs != 1 else ''}")
    return " · ".join(parts)


def anchor(name: str) -> str:
    return "".join(ch for ch in name.lower().replace(" ", "-") if ch.isalnum() or ch == "-")


def item_line(item: dict) -> str:
    if item["issue"] is not None:
        return f"- [#{item['issue']}]({ISSUES}/{item['issue']}) {item['title']}"
    ref = f" ({item['ref']})" if item.get("ref") else ""
    return f"- {item['title']}{ref}"


def markdown(tallied: list) -> str:
    total = {k: sum(counts(a)[k] for a in tallied) for k in KINDS}
    bugs = sum(len(a["bugs"]) for a in tallied)
    out = [
        "<!-- Generated by scripts/features.py from docs/features.json and the issues; edit those instead. -->",
        "",
        "# What Suhina does today",
        "",
        f"In the areas Suhina aims at ([ADR 0016](decisions/0016-scope.md)): {total['done']} done, "
        f"{total['working']} in progress and {total['planned']} planned, counted from the issues of each "
        f"area's `area:` label and the features without an issue in [features.json](features.json); "
        f"{bugs} open bugs besides. The order the planned ones are built in is in [ROADMAP.md](../ROADMAP.md).",
        "",
        "| Area | Done | In progress | Planned | Open bugs |",
        "|---|---|---|---|---|",
    ]
    for area in tallied:
        c = counts(area)
        out.append(f"| [{area['name']}](#{anchor(area['name'])}) | {c['done']} | {c['working']} | "
                   f"{c['planned']} | {len(area['bugs'])} |")
    for area in tallied:
        query = area["label"].replace(" ", "+").replace(":", "%3A")
        out += ["", f"## {area['name']}", "",
                f"Issues: [`{area['label']}`]({ISSUES}?q=is%3Aissue+label%3A%22{query}%22)", ""]
        for kind in ("working", "planned", "done"):
            items = [i for i in area["items"] if i["kind"] == kind]
            if items:
                out += [f"**{LABELS[kind]}**", ""] + [item_line(i) for i in items] + [""]
        if area["bugs"]:
            out += ["**Open bugs**", ""] + [item_line(b) for b in area["bugs"]] + [""]
        while out and out[-1] == "":
            out.pop()
    return "\n".join(out) + "\n"


def svg(tallied: list, theme: str) -> str:
    """One bar per area, the full width split into the shares done, in progress and planned, with
    the counts and the open bugs beside it; the first row is every area together."""
    t = THEMES[theme]
    label_w, bar_w, count_w, pad = 200, 300, 260, 16
    row, bar_h, top = 28, 16, 64
    total = {k: sum(counts(a)[k] for a in tallied) for k in KINDS}
    rows = [("All areas", total, sum(len(a["bugs"]) for a in tallied), True)]
    rows += [(a["name"], counts(a), len(a["bugs"]), False) for a in tallied]
    width = pad + label_w + bar_w + count_w + pad
    height = top + row * len(rows) + 10 + pad
    font = "font-family=\"-apple-system,BlinkMacSystemFont,'Segoe UI',Helvetica,Arial,sans-serif\""
    out = [
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" '
        f'viewBox="0 0 {width} {height}" role="img" aria-label="Suhina by area: done, in progress and planned">',
        f'<rect width="{width}" height="{height}" rx="8" fill="{t["surface"]}"/>',
        f'<text x="{pad}" y="26" {font} font-size="15" font-weight="600" fill="{t["ink"]}">'
        'Suhina by area: done, in progress, planned</text>',
    ]
    x = pad
    for kind in KINDS:
        out.append(f'<rect x="{x}" y="38" width="12" height="12" rx="3" fill="{t[kind]}"/>')
        out.append(f'<text x="{x + 18}" y="48" {font} font-size="12" fill="{t["muted"]}">{LABELS[kind]}</text>')
        # About 6.5 px a character at 12 px, then a gap.
        x += 18 + round(6.5 * len(LABELS[kind])) + 18
    x0 = pad + label_w
    for i, (name, c, bugs, whole) in enumerate(rows):
        # The total stands apart from the areas below it.
        y = top + i * row + (0 if whole else 10)
        n = sum(c.values())
        weight = ' font-weight="600"' if whole else ""
        out.append(f'<text x="{x0 - 10}" y="{y + 12}" text-anchor="end" {font} '
                   f'font-size="12"{weight} fill="{t["ink"]}">{escape(name)}</text>')
        if n:
            x = x0
            out.append(f'<clipPath id="bar{i}"><rect x="{x0}" y="{y}" width="{bar_w}" height="{bar_h}" rx="4"/></clipPath>')
            out.append(f'<g clip-path="url(#bar{i})">')
            for kind in KINDS:
                if c[kind] == 0:
                    continue
                w = bar_w * c[kind] / n
                title = f"{name}: {c[kind]} of {n} {WORDS[kind]}"
                out.append(f'<rect x="{x:.1f}" y="{y}" width="{w:.1f}" height="{bar_h}" fill="{t[kind]}">'
                           f'<title>{escape(title)}</title></rect>')
                x += w
                if x < x0 + bar_w - 0.5:
                    # A 2 px gap of the surface between segments.
                    out.append(f'<rect x="{x - 1:.1f}" y="{y}" width="2" height="{bar_h}" fill="{t["surface"]}"/>')
            out.append("</g>")
        out.append(f'<text x="{x0 + bar_w + 10}" y="{y + 12}" {font} font-size="12"{weight} '
                   f'fill="{t["muted"]}">{escape(breakdown(c, bugs) or "nothing yet")}</text>')
    out.append("</svg>")
    return "\n".join(out) + "\n"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="check features.json; read and write nothing else")
    args = parser.parse_args()
    try:
        areas = load()
        if args.check:
            n = sum(len(a.get("features", [])) for a in areas)
            print(f"features.json: {len(areas)} areas, {n} features, fine")
            return 0
        tallied = tally(areas, *fetch())
    except MapError as error:
        print(f"::error file=docs/features.json::{error}")
        return 1
    LIST.write_text(markdown(tallied), encoding="utf-8", newline="\n")
    for theme, path in FIGURES.items():
        path.write_text(svg(tallied, theme), encoding="utf-8", newline="\n")
    print(f"wrote {LIST.relative_to(ROOT)} and the two figures")
    return 0


if __name__ == "__main__":
    sys.exit(main())
