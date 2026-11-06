---
description: Merging two project folders without git and the version browser over both storages; assets merge as a union of content hashes, the document merges three-way against a common ancestor and refuses rather than guesses, and one merge driver makes git merge work
read_when: merge, three-way merge, conflict, common ancestor, version, version browser, musical diff, compare versions, merge driver, gitattributes, git merge, branch, undo, set_audio, version switch, CRDT
type: decision
status: proposed
built: no
last_verified: 2026-11-02
covers:
  - crates/project-file/src/lib.rs
  - crates/session/src/commands.rs
---

# 0027: Merge and versions

- **Date:** 2026-10-15
- **Task:** part of the collaboration and project-version work
  ([ADR 0023](0023-collaboration-and-project-versions.md)). Depends on
  [ADR 0025](0025-object-identity.md) (what an object is) and
  [ADR 0026](0026-project-folder-and-package.md) (the folder, the manifest, the package).

## Context

Two people working on one song, on their own machines, is the ordinary case this is for. The
workflow is ordinary too: A sends a package, B works, B sends it back, and the two are put together.
Nothing about that requires a server, an account or a network library, and it is the same workflow
whether the two copies travelled as packages, as folders, or through a git repository.

What makes it possible at all is that **the document is readable text**: ADR 0010 chose versioned
JSON precisely "so people and agents can read and diff it", and that choice stopped being a
convenience and became load-bearing here. What makes it *cheap* is
[ADR 0026](0026-project-folder-and-package.md): assets are content-addressed, so they are immutable
and two copies of the same audio are literally the same value.

Two problems follow, and they are different sizes:

- **Assets.** Two people who each added samples have two sets of samples. This turns out not to be a
  problem at all — see decision 2 — which is the single property that makes this design affordable.
- **The document.** Two people who each edited the same song have two documents and no way to
  combine them by text. A JSON document is text but it is not *line-oriented*: a textual merge
  conflicts on adjacent formatting, produces a file that parses but is musically wrong, or both.

A second, unrelated problem sits beside it. Users already version their work — the audited project
folder held **five named sets of one song plus five timestamped automatic copies** — and they do it
by "Save As" and by remembering which filename was which. The measured cost of that habit was 38 % of
the project's bytes. So the question is not whether to introduce versioning, but whether to keep
leaving it to filenames.

## Decision (proposed)

1. **Merging two project folders is the baseline, and it does not need git.** A merge takes two
   folders and a common ancestor and produces one folder, with no repository, no remote and no
   hashes in the user's face. This is the ordinary case and it is decided first. **Git is the
   optional layer on top**, for the user who wants history, branches and a remote — not the
   requirement. A user who never installs git gets the whole of decisions 1–5 and 7–9.
2. **Assets merge as a set union of content hashes, and therefore cannot conflict.**
   Content-addressed blobs are immutable, so two people adding different samples is a union; two
   people adding the *same* sample is the same hash, so it is not even a union of two things. There
   is no such thing as an asset conflict, and nothing in the merge has to resolve one. This is the
   property that makes the rest affordable and it should be relied on rather than worked around.
3. **The document merges three-way against a common ancestor.** Objects that existed in the ancestor
   are matched **by the ancestor's ids** ([ADR 0025](0025-object-identity.md)) — the ancestor is what
   makes the match unambiguous, which is why a merge is three-way and not two-way. Objects added
   since the ancestor are **imported, with the destination allocating their ids**; this is why
   globally unique or peer-scoped ids are not needed for this feature, and it is the reason a merge
   can be implemented before any of [ADR 0023](0023-collaboration-and-project-versions.md)'s live
   session work exists.
4. **A merge never guesses. Where two sides disagree about the same object, the merge refuses that
   object and reports it.** Concretely:
   - a field changed on one side only, or changed on both sides to the same value — taken, no
     conflict;
   - **a field changed on both sides to different values — the object is left as the local side has
     it and reported as unmerged**;
   - **deleted on one side and modified on the other — reported as unmerged**, not resolved;
   - added on one side — imported;
   - deleted on one side and untouched on the other — deleted.
   **The principle is that a merge may not silently discard a musical idea**, and the consequence is
   that a merge is *partial*: it merges everything it can, declines the objects it cannot, and
   reports them by name — better on a two-hundred-track song than refusing the whole thing because
   one clip was edited twice. The report is a state the project can be in and be saved in; it is not
   an error that aborts the command.
5. **The merge is a command with exactly one undo step**, and it is reachable from the UI, the CLI
   and MCP like every other command (AGENTS.md rule 6). One undo step is free: undo in this project
   is a snapshot of the project (ADR 0002), so a merge that turns out badly is one Ctrl+Z, not a
   repair.
6. **`suhina merge-driver` makes real `git merge` use this instead of diffing JSON textually.** The
   repository carries `*.suhina merge=suhina` in `.gitattributes` and the driver is configured to
   call Suhina; git then asks Suhina to combine the file rather than doing it itself. This is an
   established pattern for files that are text but not line-oriented — lockfiles, Xcode project
   files and Terraform state are all handled this way — and it is **not optional for the git path**:
   without it, `git merge` mangles the document. The driver is a thin entry point over decision 3
   and 4, not a second implementation.
7. **Versions are named, and what the user sees is a version browser, not a git graph.** Musicians do
   not want nodes, edges and hashes, and they already do this by hand: the audited folder's five
   named sets are a version list with a bad interface. The browser lists versions with **date, author
   and length**, and it works over **both** storage modes — packages in a folder, and commits in a
   repository — so what the user chooses is a **version**, never a storage mechanism. A user who
   never learns what git is still gets versions.
8. **The musical diff: what changed between two versions, in musical terms.** It names **which tracks
   changed** between two versions, and it can **audition them side by side or one after the other**.
   This is the genuinely new part, and the reason the version browser is worth building at all: git
   can say which lines changed, and only a DAW can *play* them. It is **impossible without object
   identity** ([ADR 0025](0025-object-identity.md)) — without ids, moving one clip makes everything
   after it on the lane look changed — and it is **valuable to a single user with no collaborators**,
   which is what makes this the reason to build any of it.
9. **Switching a version is cheap by construction, and the way it is cheap is part of the decision.**
   A version switch reuses the decoded audio pool, calls `set_audio` **only when the set of
   referenced hashes changed** (`crates/session/src/lib.rs` — the `audio_changed` flag; ADR 0011),
   and keeps plug-in host processes alive when the device chain is unchanged, because a version
   switch is a **graph swap** (ADR 0005) and not a restart. The honest limit: a plug-in's **state**
   lives in the document as base64 (ADR 0010 §5), so switching between two versions that differ in a
   plug-in's state pushes new state to the host — a `setState`, not a restart, but not free either.
   And the document's heaviest part is plug-in state, not audio, so **the cost of a diff is usually
   plug-in state**; that is worth knowing before promising that comparing versions is instant.

## Consequences

- The merge lives in `crates/project-file`, because that crate already maps the model to and from
  its stored form, and a merge is exactly that mapping applied to three inputs (ADR 0010 §2).
- The version browser and the musical diff are **UI over commands**, not new machinery: the commands
  list versions, diff them and switch between them, and the view draws them (AGENTS.md rule 6).
- This ADR depends on [ADR 0025](0025-object-identity.md) for identity and on
  [ADR 0026](0026-project-folder-and-package.md) for the manifest; neither decision 2 nor decision 8
  can be built before both exist.
- A merge whose assets are absent is a **supported state**, not a failure:
  `project.missingFiles`, `project.relinkFiles` and the find-by-name panel already handle it
  ([`docs/spec/missing-files.md`](../spec/missing-files.md)). It gets a golden file for exactly that
  degraded case, because "a merged project with 40 % of its audio missing" is a state a user will
  really be in and it must not be worse than no merge at all.
- **The schema is versioned independently of any replication or merge format**, migrations are
  idempotent and commutative, and a merge across incompatible major versions is **refused rather than
  performed lossily**. Two users on different Suhina versions is the hard case; refusing is a worse
  experience than merging and a far better one than merging wrong.
- A merge that reports unmerged objects means the **document can carry a reported conflict**. That is
  a new kind of project state, and saving it, reopening it and re-attempting the merge later all have
  to work.
- Because the merge is a command with one undo step, and undo is a snapshot, merging a large project
  costs a project snapshot in memory — cheap, since the model holds no audio (ADR 0002).

## Alternatives considered

- **A CRDT.** The obvious answer for merging a document, and the one most projects reach for.
  Rejected **for now**, for two reasons that are decisions of this project rather than general
  truths: [ADR 0023](0023-collaboration-and-project-versions.md) puts one host in charge of a live
  session, so **convergence is not needed** — there is no concurrent writer to converge with; and a
  CRDT makes **every future model field merge-compatible forever**, a permanent tax on every change
  to the model, paid whether or not anyone ever collaborates. If simultaneous editing of one object
  by two people turns out to be wanted, this is the document to supersede. The candidates are
  `yrs` and `loro` (both MIT), and `diamond-types` is rejected on maintenance (unmaintained since
  August 2022), not on licence.
- **A single-writer token instead of a merge.** One person holds the pencil at a time, so no merge is
  ever needed. Attractive because it removes this whole ADR, and worth having for live sessions. Not
  sufficient on its own: **it does not survive a day**, because two people working at different times
  — the actual workflow, and the one that makes this feature useful to one person — still diverge,
  and a token does not combine divergent work.
- **Relying on textual `git merge`.** Free, and it works for line-oriented text. Rejected: a JSON
  document is not line-oriented, so a textual merge conflicts on adjacent keys, can produce a file
  that parses and is musically wrong, and has no concept of an object. It is also the reason a merge
  driver is a deliverable rather than a nicety.
- **No merge at all: only manual copying.** The user opens both projects and copies what they want
  across. Rejected because it is the state of the art this is replacing: it does not scale past a few
  clips, it does not record what was taken, and copying between open projects is itself still an open
  item (ADR 0007).
- **Last-writer-wins everywhere, always succeeding.** Simple, always produces a result, and never
  reports anything. Rejected because it **silently discards one person's work** and gives no sign that
  it did — the one outcome a merge must not have. The alternative to refusing is not "merge anyway";
  it is "merge a smaller set of things correctly and say so".

## Not decided yet

- **The exact conflict policy's shape.** This ADR decides the *principle* — never guess, refuse the
  object, report it, stay partial — and the spec decides the presentation: what a reported object
  looks like in the UI, how the user resolves one, and whether a resolution is its own command.
- **Whether a version is a tag, a commit or a package.** Decision 7 makes the browser work over both
  storage modes; what a "version" *is* underneath, and whether the two modes have the same notion of
  one, is open.
- **How a merge presents a change it cannot express** — a change that is not a field, an object or an
  asset, such as a reordering that has no stable identity in the ancestor.
- **Whether a merge can be resumed or repeated** after a partial result, and whether a reported
  conflict is re-attempted automatically the next time the same two folders are merged.
- **How much of this the CLI offers** versus the window, and whether `suhina-cli` gains a merge
  subcommand before the UI has one.
- **Where the merge driver's configuration is written.** A repository needs `git config` for the
  driver; whether Suhina writes it, offers it, or documents it is open, and writing into a user's git
  configuration unasked is not acceptable.

## Sources

- This repository: [ADR 0002](0002-commands.md) (commands, and undo as a snapshot),
  [ADR 0005](0005-edits-while-playing.md) (a graph swap carries a project's changes to a playing
  engine), [ADR 0007](0007-several-open-projects.md) (copying between open projects is listed as not
  decided), [ADR 0010](0010-project-files.md) (readable JSON, §5 base64 plug-in state),
  [ADR 0011](0011-preview-and-added-audio.md) (`set_audio`), [ADR 0025](0025-object-identity.md),
  [ADR 0026](0026-project-folder-and-package.md),
  [`docs/spec/missing-files.md`](../spec/missing-files.md),
  [`AGENTS.md`](../../AGENTS.md) rules 1, 2, 6 and 7.
- [ADR 0023](0023-collaboration-and-project-versions.md) for the live-session topology this
  deliberately does not need, and [ADR 0024](0024-musical-time-and-tempo.md) for the time
  representation a diff compares.
- The versioning habit that decision 7 formalises was measured, not assumed: an authored project
  folder held five named sets of one song (the oldest from 2024) alongside five timestamped automatic
  copies, together 38 % of the folder's bytes. Those observations belong in
  [`docs/spec/als-corpus.md`](../spec/als-corpus.md) (AGENTS.md rule 1).
