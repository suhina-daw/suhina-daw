# The collaboration and project-versions package

This folder holds the work items for the epic in
[`issue-collaboration-and-project-versions.md`](../issue-collaboration-and-project-versions.md). The
enclosing directory also holds the five decisions the work depends on, in `docs/decisions/`. Nothing
here is on GitHub yet; this file is the order in which it goes there.

## What is in the package

| | Where | What it is |
|---|---|---|
| 1 | `issue-collaboration-and-project-versions.md` | The epic |
| 16 | `issues/*.md` | The work items the epic is split into |
| 5 | `docs/decisions/0023`–`0027` | The decisions, written as ADRs |

The epic and the issues are staging files. **The ADRs are the durable part** and are already in the
place the repository expects, with front matter, so `python scripts/docs.py check` sees them and
`docs/INDEX.md` will list them once it is regenerated on `main` after the merge.

## Read this before handing anything to an agent

An `epic` is a container: `CONTRIBUTING.md` says *"claim one of its issues, not the epic"*. An agent
given the epic cannot start, and an agent given a vague issue **will confidently invent the
decisions it was not given** — that, not incapability, is the failure mode. Two rules follow:

1. **Never hand over the epic.** Hand over one issue.
2. **An issue whose prerequisites are not decided yet must not be started.** Put its board item in
   `Do not start yet` and say what it waits for; `CONTRIBUTING.md` describes that status for exactly
   this. Issues 10, 11 and 14 below exist to say "not yet", not to be started.

## The one prerequisite that is not in this package

`docs/decisions/0023`–`0027` decide the shape, the time base, identity, the folder and the merge.
They do **not** decide the session protocol, the transport or the audio link — those are named under
"Not decided yet" and need their own ADRs. Issues 10, 11, 12, 13 and 14 are where that work will
happen, and three of them are blocked until a decision exists.

## Labels

The repository has no home for this work in its area labels (`engine`, `ui`, `formats`, `browser`,
`audio-io`, `plug-ins`, `devices`, `presets`, `tests`). **Create one first:**

```sh
gh label create collaboration --repo suhina-daw/suhina-daw \
  --description "Sharing a session, project versions, the project folder" --color 5319e7
```

Then the epic and the issues below use `collaboration` alongside whichever existing area label fits
(`formats` for the file format and the folder, `ui` for the browser and the diff, `engine` where the
audio path is touched, `tests` where the work is mostly tests).

## Order

The order is a dependency order, not a priority order. Property 1 can start today; everything else
waits for what is above it.

```text
 1 musical time ────────────────┐   (independent; can start today)
                                │
 2 object identity ─────────────┼──┐   (independent; can start today — and it
                                │  │    is what the version diff needs)
 3 project folder, manifest,    │  │
   package  ◄───────────────────┘  │
 4 hand-set kind store            │
 5 merge two folders ◄────────────┘
 6 merge driver and git ◄── 5
 7 version browser ◄── 3, 6
 8 musical diff ◄── 2, 7
 9 remove .locr  (being done in #565; no issue to create)
15 author identity  (small; issues 5, 7 and 10 want what it provides, so it is
                    worth landing before them)

10 session protocol and permissions  — waits for 2 and 3, and for its own ADR
11 network transport                 — waits for its own ADR; its first deliverable is a spike
12 suhina-relay                      — waits for 11
13 recording, input, monitoring      — independent of all of the above, and XL
14 guest performer audio link        — waits for 13, 2 and 11
16 project info and status           — waits for 7 and 15; UI work, so it belongs to the UI milestone
```

The two that are cheap now and expensive later are **1** and **2**. Every week in which real
projects are saved makes them more expensive, and today there is no installed base at all: no
`.suhina` file exists in the tree. **15** is the same kind of thing on a smaller scale: it is two
settings fields and a model field, and two later issues would otherwise have to invent it.

## Putting it on GitHub

`gh` takes a file as the body, and `--title` is required — `gh` does not read a title from the body.
Each of the 16 issue files starts with a `# ` heading that is meant to become the title; the epic
file follows the house style of `issue-146-stem-separation.md` and starts with prose, so its title
is written out below.

**The links in the issue files are absolute GitHub URLs on purpose.** A relative link works when a
file is read in the repository tree and breaks when the same text is rendered as an issue body, so
the issue files were converted before they were ready to be sent. The epic and the 16 carry absolute
links; **this file was left relative on purpose**, because it stays in the tree and is not an issue.

**The label has to exist first.** `gh issue create --label collaboration` fails outright when the
label is missing, so nothing below works until step 1 has run.

**The area labels are derived from each file, not guessed.** Every issue states its own
`**Type:** … **Area:** …` line, and the script reads the backticked labels out of it, so an issue's
labels cannot drift from what the issue says about itself.

```sh
cd <the repository root>
python - <<'PY'
import json, pathlib, re, subprocess, sys

REPO = "suhina-daw/suhina-daw"
EPIC = pathlib.Path("issue-collaboration-and-project-versions.md")
EPIC_TITLE = "Collaboration and project versions (epic)"
EPIC_LABELS = ["enhancement", "epic", "collaboration"]


def gh(*args):
    out = subprocess.run(["gh", *args, "--repo", REPO], capture_output=True, text=True)
    if out.returncode:
        sys.exit(f"gh {' '.join(args)} failed:\n{out.stderr.strip()}")
    return out.stdout.strip()


def labels_of(text):
    """The backticked labels on the `**Area:**` line, plus the type."""
    line = next((l for l in text.splitlines() if l.startswith("**Type:**")), "")
    area = line.split("**Area:**", 1)[-1] if "**Area:**" in line else ""
    found = re.findall(r"`([a-z-]+)`", area)
    return ["enhancement", *dict.fromkeys(found)]


def create(path, title, labels):
    url = gh("issue", "create", "--title", title,
             "--label", ",".join(labels), "--body-file", str(path))
    return int(url.rstrip("/").rsplit("/", 1)[-1])


# 1. the label, once
if not any(l.split("\t")[0] == "collaboration"
           for l in gh("label", "list", "--limit", "200").splitlines()):
    gh("label", "create", "collaboration",
       "--description", "Sharing a session, project versions, the project folder",
       "--color", "5319e7")
    print("created the collaboration label")

# 2. the epic, so its number exists before the children reference it
epic = create(EPIC, EPIC_TITLE, EPIC_LABELS)
print(f"epic                      -> #{epic}")

# 3. the work items, in file order, and the map the cross-references need
mapping = {}
for f in sorted(pathlib.Path("issues").glob("[0-9]*.md")):
    text = f.read_text(encoding="utf-8")
    title = text.splitlines()[0].removeprefix("# ").strip()
    n = create(f, title, labels_of(text))
    mapping[f.name[:2]] = n
    print(f"{f.name:<42} -> #{n}")

pathlib.Path("issues/issue-numbers.txt").write_text(
    json.dumps(mapping, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print("\nwrote issues/issue-numbers.txt — the map for step 4")
PY
```

Then, by hand, three things the script deliberately does not do:

1. **Replace the cross-references.** Every issue body says `issue 07`, `issue 11` and so on, and
   those are **file numbers, not GitHub numbers**. `issues/issue-numbers.txt` is the map
   (`{"01": 410, "02": 411, …}`); editing each "issue NN" to `#<real>` is a five-minute pass through
   the 16 and the epic's "Sub-issues" list. **Until that pass is done the references are wrong**, and
   nothing the script can do makes them right, because the numbers do not exist until the issues do.
2. **Board status.** A new item lands in the project's default. Set `Do not start yet` on issues 10,
   11 and 14, and set Status, Priority and Phase from each file's own header line.
3. **Sub-issue linking.** If the board supports sub-issues, link the 16 to the epic afterwards, so
   the epic shows its children.


The ADRs go in as part of the same pull request as the first work item that needs them, because
`CONTRIBUTING.md` puts architecture in `docs/decisions/` and the documentation is part of the
change (AGENTS.md › Before you finish a task).

## Two follow-ups this package deliberately leaves open

1. **The observations must reach the specs.** AGENTS.md rule 1 requires what was learned from an
   authored `.als` project folder — the measured sizes, the analysis sidecars, what collecting does
   — to be written into `docs/spec/als-format.md` and `docs/spec/als-corpus.md` **before code
   depends on it**. `docs/spec/als-format.md` already lists "what Collect All and Save does" as an
   open item. Today those numbers live in ADR 0026 and in the epic, which is not a spec.
2. **New behaviour needs a spec before code**: `docs/spec/session-sharing.md`,
   `docs/spec/project-package.md`, `docs/spec/project-versions.md`. ADR 0027 also creates a new
   savable state — a document carrying a reported, unmerged conflict — which no spec describes.

## A note on the working tree this was written in

Prepared in a tree that moved underneath it. **The numbering 0023–0027 was forced by two hard
collisions, not chosen for taste**: the ADRs were first written as 0017–0021, and while this package
was being prepared, commits landed that took **0018** (`0018-own-windows-audio-driver.md`) and
**0019** (`0019-midi-controllers-and-mapping.md`), and a further **0022**
(`0022-vst2-interface-description.md`) exists in the tree as work in progress. Two ADRs sharing a
number is not cosmetic — every reference by number becomes ambiguous — so the five were moved to
**0023–0027**, past everything in use, and every reference was rewritten with them. **`0017`, `0020`
and `0021` are left unused**; that is untidy but harmless, and taking any of them would risk the
same collision again. If you renumber, renumber all five together and rewrite the references:
they are cited by number in the epic, in all 16 issues and in each other.

Every line-number citation in the ADRs and the epic was checked against `origin/main`, and the
files cited are unchanged there. Merge or rebase before opening a pull request.
