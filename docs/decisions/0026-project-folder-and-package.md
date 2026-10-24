---
description: A project becomes a folder holding a document, a manifest of each asset's path and BLAKE3 hash, and only when collected its audio; the shareable package is a zip with its own extension; derived analysis stays out of the project; the .locr compatibility path is removed
read_when: project folder, manifest, asset hash, BLAKE3, collect, packaged project, zip, sharing a project, analysis sidecar, asd, hand-set kind, .locr, old extension, git and audio, git-lfs, project package
type: decision
status: draft
last_verified: 2026-10-15
covers:
  - crates/project-file/src/lib.rs
  - crates/session/src/lib.rs
  - crates/library/src/index.rs
---

# 0026: The project folder, the manifest and the package

- **Status:** proposed
- **Date:** 2026-10-15
- **Task:** the collaboration and project-version work
  ([ADR 0023](0023-collaboration-and-project-versions.md)) needs a project that can be shipped,
  versioned and merged as a unit, and ADR 0010 left "a packaged format that carries its audio, and
  'collect and save'" undecided. An audit of an authored `.als` project folder (below) showed where
  a project's weight actually is and what a per-asset analysis sidecar costs.

## Context

ADR 0010 made a Suhina project a single versioned JSON file that **refers** to its audio by a
relative path plus an absolute path, and deliberately copies nothing. That is right for one person
on one machine and it left three things open: a format that carries its audio, collecting, and
autosave. It also accepted that "large note-heavy projects are bigger than a binary format would
make them".

Two things now press on that.

**First, a project has to travel and to be compared.** Sharing one, handing it to another person,
keeping versions of it and merging two diverged copies all want a project to be a *thing with
parts*: the document, the assets it needs, and a record of which asset is which. Referring to audio
by path alone cannot express "the same audio file, at a different path, on someone else's machine",
which is exactly the question every one of those features asks.

**Second, measurement contradicted an assumption.** A project folder's weight is usually assumed to
be audio. Measured on an authored `.als` project folder (41 files, 39.2 MB), it is not:

| Part | Measured | Share |
|---|---|---|
| Documents, 10 × `.als` — 5 named versions in the folder root plus 5 timestamped copies in a `Backup/` folder | **15.0 MB** | **38 %** |
| ↳ of which the `Backup/` copies | 8.3 MB | 21 % |
| Audio, 15 files | 22.8 MB | 58 % |
| `.asd` analysis sidecars, 14 files | 0.8 MB | **2 % of the bytes** |
| Project icon | 0.48 MB | 1.2 % |

Three findings from that audit decide this ADR:

- **An `.als` is gzip-compressed XML: 1 619 538 B packed from 11 847 274 B, 7.3×.** Ten
  near-identical documents are stored ten times over, because gzip does not deduplicate across
  files. **The largest measured win available is the documents, and it is not audio.** A store that
  compresses *and* deltas between versions — which is what git does — would hold those ten for an
  estimated 2–3 MB. Compressed-but-not-diffable (the `.als` choice) and readable-but-uncompressed
  (ADR 0010's choice: plain JSON, "so people and agents can read and diff it") each give something
  up. What gives up neither is a readable document stored in something that compresses and deltas.
- **The `.asd` sidecars are exactly one per audio file (14 of 15) and sit next to the audio**, inside
  the project's own sample folders: **48 % of the files in the samples folder and 34 % of the
  project's files, for 2 % of its bytes.** The cost of a per-asset sidecar is file count and
  pollution, not space. One of them is 614 KB. One recorded take is a **0-byte file with no
  sidecar**, so a cache kept beside the audio can also drift out of step with it.
- **Content addressing saves nothing inside one project here:** all 15 audio files are unique. Its
  value is integrity, merge and recognition *across* projects and across version history, not local
  deduplication. This ADR does not claim otherwise.

The audit also found that the folder-as-project convention is **unenforced**: that one folder held
sets from two different songs (2024 and 2026), an almost-empty set, and a Windows `Desktop.ini`.

The observations above were taken from a file the project's own authors made, which AGENTS.md
rule 1 permits; they belong in [`docs/spec/als-format.md`](../spec/als-format.md) and
[`docs/spec/als-corpus.md`](../spec/als-corpus.md) before anything is built on them. The same spec
already lists this open item to observe:
[`docs/spec/als-format.md:299`](../spec/als-format.md) — "what **Collect All and Save** does".

## Decision (proposed)

1. **A project is a folder**: a document, a manifest, and — only if the user collected — a
   sub-folder holding the project's audio. **A single `.suhina` file stays valid and readable**, and
   a project that has neither assets of its own nor versions may go on being one file. The folder is
   what a project becomes when it has either.
2. **The manifest maps each asset to its path and its BLAKE3 hash**, and it is the one structure that
   serves referencing, collecting, relinking and merging. ADR 0010 §6 already stores a **relative
   path plus an absolute path** per region; **the hash is the field it gains.** No new referencing
   scheme is invented, and the existing rule stays: the relative path is tried first, the absolute
   path is the fallback, and saving elsewhere rewrites both.
3. **Reference by default; collecting is an explicit action.** Nothing is copied unless the user asks
   for it. This is the same philosophy the `.als` format's own "Collect All and Save" applies, and
   Suhina's own command for it is *collecting* (`project.collect`-style, AGENTS.md rule 6), never
   another product's name for its feature. Its behaviour is written down in a spec **from
   observation** before it is implemented, per the open item at
   [`docs/spec/als-format.md:299`](../spec/als-format.md).
4. **The package is a zip**, as ADR 0010 decision 6 already anticipated ("a self-contained package
   (a zip with the audio) is a separate format for sharing"). `zip` is already a workspace dependency
   — themes back themselves up and export templates with it — so the package adds no new dependency.
   **The package gets its own file extension**, because
   `suhina_project_file::is_project_file` must be able to tell a document from a package without
   opening either: a package opened as a document, or a document treated as a package, is exactly the
   kind of ambiguity that produces a confusing failure instead of a clear one. The exact string is
   left open below; that it exists is decided here.
5. **Derived analysis never lives in the project and never beside the user's samples.** ADR 0013 §8
   already keeps the sample-map feature cache in `<settings>/library/features-<hash>.bin`, **one file
   per library folder rather than one per sample**, for this reason: a sample folder can be on a
   network drive, read-only or slow (ADR 0012 says libraries are "sometimes on network drives", and
   its watcher falls back to polling there). That decision is kept and extended to every derived
   cache. Waveform peaks and feature vectors are recomputable, so their absence costs time, not
   information.
6. **A hand-set kind of sound is the one exception, and it is a different kind of data.** Issue
   [#325](https://github.com/suhina-daw/suhina-daw/issues/325) ("set a kind by hand") is a **decision
   by the user, not derived data**: recomputing will not reproduce it. ADR 0013's cache is
   **versioned** — "The feature list has a version; a cache entry of another version is [ignored]" —
   so storing a hand-set kind there means a features-v2 bump **destroys the user's corrections**.
   Therefore: **hand-set metadata lives in its own, unversioned store, keyed by content hash, not by
   path.** The same bytes are then recognised the same way wherever they are and however many copies
   exist. **A sidecar next to the file does not achieve that**: copying just the audio file — into a
   chat, into another project, onto another machine — leaves the sidecar behind, which is precisely
   how samples travel. Derived caches are versioned and may be dropped; a user's decision is not and
   may not be.
7. **Git tracks the document and the manifest, never the audio.** Committing unchanged audio does not
   duplicate it, but **every new take, bounce or render is a permanent blob**, and removing one means
   rewriting history. The manifest is the only thing a merge needs to touch, and because it is a
   hash-keyed set, **its merge is a union**. A `.gitignore` keeps the audio and renders out. This is
   the reason the manifest exists as its own structure rather than as fields scattered through the
   document: it is the part that is versioned, shipped and merged.
8. **The `.locr` compatibility path is removed, as a recorded decision.** `.locr` is the extension
   from the product's previous name, and the old name may not be written out anywhere
   (`scripts/rename_suhina.py --check`), so the code is a compatibility shim named after a name the
   project cannot spell. **Measured: about 39 occurrences across 12 files, of which the functional
   code is two constants (`OLD_EXTENSION`, `OLD_FORMAT`) and two branch points (`is_project_file`,
   and the format check in `from_json`); the rest are tests, doc comments and user-facing help text.**
   **No `.suhina` or `.locr` file exists anywhere in the tree**, so the path has no users.
   Removing it *before* the format version bump avoids one reader having to support three format
   names. This is a decision, not a silent deletion (AGENTS.md: docs are part of the change), and it
   is recorded again as ADR 0010 decision 11. The same change updates: the recent-files list, the CLI
   and app help text, the MCP tool description, the project dialog, `media::info`'s
   `FileKind::Project` test, and the tests in `crates/project-file/src/tests.rs`,
   `crates/session/tests/save.rs`, `crates/session/tests/relink.rs` and
   `crates/automation/src/server.rs`. **One thing must be checked first:** whether those lines carry
   a `rename_suhina: keep` marker. If they do, removing them also removes an exception from the
   rename check; if they do not, the removal cannot break it. Either way the answer is worth knowing
   before the change, not after.
9. **A folder holding more than one document is tolerated, not refused**, but it is *named*: the
   project dialog and the folder state say which document is the project's, and opening a folder whose
   document is ambiguous asks rather than guesses. Refusing outright would fight a convention users
   already have (the measured folder held sets from two songs); guessing would open the wrong song
   silently.
10. **The document format gains a version for ids.** [ADR 0010](0010-project-files.md) decision 3
    fixes the rules: one version, written in every file, that rises with every change to what a
    file holds; a reader refuses a newer version and reads every older one. **The next version
    carries explicitly what earlier versions could only imply: the object ids of
    [ADR 0025](0025-object-identity.md).** Version 2 already means "may hold a device group" and
    version 3 is the first written always, so ids take the next number free when they land. A file
    of an earlier version is still read, with those ids **derived in document order**; the new
    version writes them, so a later reader never has to derive them and an id survives the object
    being moved. This is the bump that ADR 0025's steps C and D wait for, and it is the only change
    to the document's layout this ADR makes. Everything that reads or writes the model gets its
    line here, in the round-trip test and in the version test's project (ADR 0010).

## Consequences

- A project can be shipped, received, versioned and merged as a unit, and a document that refers to
  audio which is absent lands in a state that is **already supported**: `project.missingFiles`,
  `project.relinkFiles` and the find-by-name panel exist
  ([`docs/spec/missing-files.md`](../spec/missing-files.md)).
- **A plain-JSON `.suhina` is larger on disk than a gzip `.als` for the same content until it is
  compressed.** This is the price ADR 0010's readability choice pays, and it is paid back by storing
  the document somewhere that compresses and deltas — the same decision that makes it diffable in the
  first place. The alternative — compressing the document in place — would spend the diffability
  ADR 0010 bought.
- Every new asset field needs its line in `suhina-project-file` and in the round-trip test, as every
  model field does (ADR 0010).
- The manifest is a second thing to keep in step with the document; a manifest that disagrees with
  the document is a state the reader must handle, not one it may assume away.
- Collecting must stay explicit, because a shared content-addressed store has no safe moment to
  delete from (see below).
- The `.locr` removal is a small, self-contained change with a documentation cost that is larger than
  its code cost, which is why it is recorded here rather than done quietly.
- Windows path length becomes a real constraint rather than a theoretical one: one collected stem in
  the measured project has a filename of about **120 characters** and a full path of about **170**, so
  a project under a deep user folder would exceed 260. The folder layout, and possibly the
  platform's long-path support, must account for it.

## Alternatives considered

- **Keep a single file and refer to everything by path (the status quo).** What breaks is specific: a
  path cannot say "this is the same audio" when the audio has moved, so collecting has nothing to
  check against, relinking is name-guessing (the current find-by-name flow), a merge cannot tell "the
  same asset" from "a different asset with the same name", and a package cannot verify what it
  received. Paths also cannot be deduplicated, so two projects using one sample store it twice.
  Rejected.
- **Always self-contained: collect automatically on save.** Simple, and it makes every project
  portable by construction. Rejected because it copies gigabytes without being asked, multiplies
  storage across every project that touches a shared sample, and makes saving slow and surprising.
  The measured alternative is what one project folder already does by hand: 22.8 MB of audio copied
  in, whether or not the user wanted that project to be portable.
- **A binary container instead of a folder.** A single opaque file is tidy and avoids path-length
  problems. Rejected: it defeats the reason ADR 0010 chose JSON, removes the ability to keep the
  document in git, and makes the audio invisible to every other tool on the machine — including the
  user's own file manager.
- **A per-asset sidecar beside the audio** (one analysis file per sample). Rejected on three measured
  and structural grounds: it costs **48 % of the files in samples folder and 34 % of a project's
  files for 2 % of its bytes**; it writes into folders that can be read-only, on a network or slow
  (ADR 0012); and it **does not follow the file** — copying the audio alone leaves it behind, so it
  cannot deliver the one thing a sidecar is for. The measured project also shows a cache drifting out
  of step with the audio (a 0-byte take with no sidecar). A sidecar remains a possible *export* —
  "write the analysis beside the files" as an explicit action, for handing a sample folder to someone
  who has no Suhina store — but never the default.
- **`git-lfs` or `git-annex` for the audio.** Both solve "large files in git" properly.
  [ADR 0023](0023-collaboration-and-project-versions.md) decides that the project runs no
  infrastructure and that a user runs any relay; `git-lfs` requires a server that implements its
  protocol, which is the thing ADR 0023 rules out, and `git-annex` is a second tool to install and
  explain. Rejected for now, with the honest note that a user who already runs an LFS server may use
  one — the point is that nothing Suhina ships may require it.
- **Put the audio in git anyway.** Workable for a small project and genuinely simple, and
  unchanged files are not duplicated across commits. Rejected because every new take, bounce or
  render is a permanent blob: a four-minute 48 kHz/24-bit stem is tens of megabytes, and ten takes are
  hundreds, with no way to reclaim them short of rewriting history. Renders are the worst case and the
  most routine.

## Not decided yet

- **The exact package extension string.** That it has one is decided above; the string is not
  chosen here. It interacts with the `.locr` removal (both are format-name decisions) and should be
  chosen once, deliberately, in the spec.
- **The folder's exact layout**, its name, and whether it carries a marker file saying "this is a
  Suhina project folder". A marker would make folder detection reliable and adds a file that must not
  get out of step.
- **Asset garbage collection in a shared content-addressed store.** Two candidates: reference
  counting, or a reachability scan over the projects that reference a hash. The consequence is decided
  already — **collecting stays explicit, because there is no safe moment to delete from
  automatically** — but the mechanism is not.
- **Whether a package can be read and written without a folder ever existing.** A user who only ever
  sends and receives packages may never see the folder at all, which is either a good simplification
  or a second code path; not decided.
- **Whether the store of hand-set kinds is per machine or travels with a project.** This ADR decides
  that it is keyed by content hash and unversioned; where that store lives relative to the settings
  folder is open.
- **Whether autosave writes into the project folder** (issue
  [#202](https://github.com/suhina-daw/suhina-daw/issues/202)). The measurement is a warning, not a
  decision: five timestamped whole-document copies were **21 % of the project**, so autosave must not
  be "keep a copy of the document".

## Sources

- This repository: [ADR 0010](0010-project-files.md) (the single file, §6 the path pair, decision 6
  the zip package, "Not decided yet" for collecting and autosave), [ADR 0012](0012-file-library.md)
  (network drives, the index on disk, the watcher's fallback), [ADR 0013](0013-sample-map-analysis.md)
  §8 (the cache in the settings folder, one file per folder, and its versioning),
  [ADR 0023](0023-collaboration-and-project-versions.md) (no infrastructure we run),
  [ADR 0027](0027-merge-and-versions.md) (what the manifest is merged for),
  [`docs/spec/als-format.md`](../spec/als-format.md) (the Collect All and Save item),
  [`docs/spec/missing-files.md`](../spec/missing-files.md) (absent assets are a supported state),
  [`AGENTS.md`](../../AGENTS.md) rules 1, 2, 6 and 8.
- Issue [#325](https://github.com/suhina-daw/suhina-daw/issues/325) (a kind of sound set by hand is
  part of the sample map's next version), issue
  [#202](https://github.com/suhina-daw/suhina-daw/issues/202) (backups and autosave).
- The measurements above were taken from an authored project folder — observation of a file made by
  the project's own authors, which AGENTS.md rule 1 permits. They must be written into
  [`docs/spec/als-format.md`](../spec/als-format.md) and
  [`docs/spec/als-corpus.md`](../spec/als-corpus.md) before any code depends on them; until then they
  are recorded here and are not a spec.
