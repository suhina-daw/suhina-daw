---
description: What each kind of document under docs/ holds and does not hold, the rules for amending an ADR, the status fields, and the checks that enforce them
read_when: writing or editing a document, ADR, amendment, spec, decision status, built field, superseding an ADR, where text goes, docs check, Built so far, version history
type: howto
status: current
last_verified: 2026-11-02
covers:
  - scripts/docs.py
  - scripts/test_docs_structure.py
  - scripts/test_docs_skills.py
---

# How documents are written

The documents under `docs/` are what agents and contributors read first, so each kind holds one
thing and a reader can tell *what is decided now* from *what was true once*.
[`AGENTS.md`](../AGENTS.md) › *Documentation is part of the change* says which document a change
updates; this says what goes into it and where. The clean-room rules for specs are in
[`spec/README.md`](spec/README.md). The files under `works-now/` are not documents but ROADMAP
bullets waiting to be folded in ([`works-now/README.md`](works-now/README.md)); `docs.py` skips them.

## What each kind holds

| Kind | Holds | Does not hold |
|---|---|---|
| **ADR** (`decisions/`) | Context, the decision as numbered items, consequences, alternatives, sources. Measurements only where they justify the decision. | How far the work has got, *Built:* notes, protocol or format version histories, file and line references, counts that change ("eight devices") |
| **Spec** (`spec/`) | What Suhina does now, in behaviour terms, with *Not yet* for what is planned. The one place to read current behaviour. | Why a design was chosen over another (the ADR's); how it got here (*Built (#N)*, "moved from …", "revision 7 changed …": the pull request's); pixel sizes ([`design/components.md`](design/components.md) holds them); measurements that justify nothing in it |
| **System** (`architecture.md`, `mcp.md`, …) | A map: which part does what, and where to read more | Behaviour details that a spec or a crate's `//!` documentation holds |
| **History** | Pull request descriptions and git | (not in `docs/`) |

New text goes into the section it is about, not at the end of the document or at the start of the
section that happens to be nearest, and it removes or rewrites the sentences it makes stale.
After `## Sources` come only further sources.

## ADRs

- **An accepted ADR changes only by** its status, a *Superseded by* or *Amended by* line, link
  and typo fixes, and amendments as below.
- **An amendment records a decision**, dated, with its issue or pull request, in at most about 20
  lines, as a section `## Amendment: <what> (yyyy-mm-dd)`. It changes the numbered decision it
  amends in place and says so in one line under it (*Amended 2026-10-18, see Amendment: …*), so
  the numbered list is always the current decision.
- **A new ADR supersedes instead** when an amendment would change what a decision means rather
  than extend it, or when an ADR already holds three amendments. The new ADR states the whole
  current decision; the old one gets `status: superseded` and *Superseded by*.
- **Progress lives in the spec and the issue.** A *Built so far* section moves to the spec the ADR
  points at (creating one if needed), and the ADR keeps one line: "What is built:
  *the spec* › Status", with a link to it.
- **Version histories** (the plug-in control protocol, the shared-memory region, the `.sd` file,
  the `.suhina` file, the plug-in catalog) live in one table each, in the spec of that format:
  version, what it added, the pull request. An ADR names only the version a decision needs.
- **No line numbers.** Name the symbol (`media::decode_project_audio`), not the line it is on.
- Decisions are numbered items `N. **The decision.** Why.` under `## Decision`, in order from 1.
  A reference names the decision: `ADR 0010 › 6`.
- ADR numbers are unique: a new ADR takes the next number after the highest in `decisions/`,
  after fetching `main`.

## Status

The front matter is the only status; an ADR has no `- **Status:**` line in its text.

| `type` | `status` | Meaning |
|---|---|---|
| `decision` | `proposed` | Under discussion; not yet the maintainer's decision |
| | `accepted` | Decided, and the numbered items are current |
| | `superseded` | Replaced; `superseded_by: NNNN` names the ADR that replaces it, and that ADR has `supersedes: [NNNN]` |
| | `rejected` | Considered and not chosen; kept for the reasons |
| `spec`, `system`, `howto` | `planned` | Nothing built. A spec that a pull request is building stays `planned` until it merges |
| | `draft` | Built, with behaviour still being settled |
| | `current` | Built and settled |
| | `superseded` | Replaced, with `superseded_by` as above |

A decision also has `built: no | partly | yes`, how far the work has got. The detail is in the spec
the ADR points at, not in the ADR.

`last_verified` is stamped on `main` after a merge (`python scripts/docs.py stamp`); a pull
request leaves the line alone.

## Checks

`python scripts/docs.py check` runs in CI. Besides the front matter, covered paths and links, it
checks the structure above:

| Code | Rule |
|---|---|
| `ADR_NUMBER` | No two files in `decisions/` share a four-digit prefix |
| `NUMBERING` | A numbered list whose items start with bold text runs 1, 2, 3 … with no repeat or gap |
| `DECISION_REF` | `ADR NNNN › N`, `ADR NNNN decision N`, `decisions N and M` and `§N` name an ADR that exists and a decision it has |
| `SUPERSEDED_REF` | A reference to a superseded ADR names the one that replaced it |
| `DECISION_STATUS`, `BUILT`, `SUPERSEDED_BY`, `SUPERSEDES` | The statuses above; `built` on every decision; the two supersession fields agree |
| `STATUS_LINE` | No `- **Status:**` line in an ADR |
| `LINE_REF` | A path or symbol is not followed by a line number |
| `SKILL_PATH`, `SKILL_SYMBOL` | A repository path or a `suhina_*::` symbol that a skill in `.claude/skills/` names in backticks exists, so its checklists cannot go stale |
| `SOURCES_LAST` | Nothing after `## Sources` but sources |

Each of these is an error: a pull request that breaks one fails CI.
