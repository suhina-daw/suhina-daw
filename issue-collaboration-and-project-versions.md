Written after a feasibility pass over peer-to-peer collaboration, project versioning and the
project file format. Short answer: **the live-jam half is a different product with different
physics, and the versioning half is cheap, valuable solo, and mostly a consequence of decisions
this project has already made.** Findings, measurements and the split below.

Planned nowhere in `ROADMAP.md` yet. Related: [#352](https://github.com/suhina-daw/suhina-daw/issues/352)
(duplicated clips share their notes — the same missing identity), [#325](https://github.com/suhina-daw/suhina-daw/issues/325)
(a kind of sound set by hand — the one piece of sample metadata that cannot be recomputed),
[#202](https://github.com/suhina-daw/suhina-daw/issues/202) (autosave), ADR 0001 (the model),
ADR 0007 (several open projects), ADR 0010 (the project file), ADR 0012 (the library index),
ADR 0013 (the feature cache), ADR 0015 (extensions), ADR 0006 (plug-in hosting).

## Goal

Two things that share one mechanism, and must not be built as one:

1. **A project becomes a thing that can be versioned, compared, merged and shared** — with or
   without a second person. `.suhina` is versioned JSON that "people and agents can read and diff"
   (ADR 0010), which is the enabling property no DAW's binary format has.
2. **A session can be shared with another machine** — asynchronously (a package, a folder, a
   repository) first, live second.

Every asset is content-addressed, so a merge is a union of hashes and cannot conflict, and
switching versions never re-reads or re-decodes audio that both versions reference.

## The boundary that decides the shape: three products, three physics

| | Latency budget | Verdict |
|---|---|---|
| **Asynchronous share and merge** (a package, a folder, a repo) | none | **The valuable 80 %.** Works today. No physics. |
| **A shared live session** (transport, launcher, presence) | 100–300 ms RTT | Feasible. Clip launching is quantised to bars (M4), so latency is musically invisible. |
| **Guest performer** (streamed audio, auto delay compensation) | 10–40 ms one way | Feasible in principle; **blocked on recording/input, which does not exist**. See below. |

Conflating the first two is what killed the previous generation of this idea in other products:
perpetual infrastructure for an intermittent feature, with the audio assets as the expensive part.
**This epic keeps them apart and ships the first one first.**

## What the architecture already gives us (verified, not hoped for)

This was not designed for collaboration and almost all of it is what you would ask for:

| Needed | Already there |
|---|---|
| One replication unit | `Session::execute(id, &params)` / `Projects::execute` — one funnel the UI, CLI and MCP all use (`crates/session/src/lib.rs`, `crates/ui/src/app.rs`); 92 commands |
| A small document | `model` is pure data with no audio; ADR 0002: "Projects are small (no audio data)" |
| Edits while playing | A new graph is swapped in at a block boundary (ADR 0005). A remote edit is architecturally the same thing as a local one |
| Per-caller undo | Undo is a snapshot of the project (`Session::undo: Vec<Project>`) — a command log is enough |
| "What is the other person playing" | `launcher.state` is already a read command |
| Per-caller command permissions | each command's `Policy::agents` and `refused(id)` already refuse commands to a caller with a reason (`crates/automation/src/server.rs`) |
| A local protocol with health, deadline and restart | `crates/plugins/src/protocol.rs` (JSON lines over a local socket, `interprocess` already a dependency) |
| A symbolic object reference as a command parameter | `ParamType::Track` already accepts `["integer", "string"]` — "id or name" (`crates/session/src/commands.rs`) |

The peer is therefore **a fourth front end over the same commands**, not a new abstraction.

## The prerequisite that is not networking: object identity

The model identifies clips, lanes and scenes **by position**, and tracks/channels/contents by a
project-local `u32` allocated as `max + 1`:

- `Clip`, `ArrangementLane` and `Scene` have **no id at all**; `clip.move` takes an `index`
- `TrackId(pub u32)` and friends collide between two machines by construction
- `blank_project` uses literal ids, so the collision is structural

Measured blast radius of fixing it: **~245 id construction sites in 34 files**, plus ~130 `.clips`
and ~203 `.lanes` references; **seven** `clip.*` commands take a positional clip `index`
(`clip.delete`, `clip.setGain`, `clip.setFades`, `clip.copy`, `clip.cut`, `clip.duplicate`,
`clip.move`), with `track.move { to }`, `device` and `scene` taking positions of their own; and the
UI threads clip identity as `(TrackId, usize)` through selection, context menus, drag/drop,
copy/paste and the sub-row layout.

Two findings make this cheaper than it sounds and more urgent than it looks:

- **The engine never sorts by id.** Its sorts are by start time, chain depth, event order and
  marker time, so the golden renders (`crates/engine/tests/golden/`, `devices/fm-synth/tests/golden/`,
  `crates/ui/tests/golden/`) do **not** break on an id change. The audio path is untouched.
- **A readable diff needs identity.** Without it, moving one clip makes everything after it on the
  lane look changed, and splitting a clip makes half the lane look changed. The version browser
  below is the payoff that justifies this on its own, with no collaborators at all.

Ordering that keeps the tree building at every step: (A) ids in the model, **derived in document
order** when reading a v1 file; (B) all addressing moved to ids, UI included; (C) command
parameters take an id and **accept `index` during a deprecation window**; (D) the file format gains
a version and keeps reading v1. Global/peer-scoped ids and change metadata come later, and only if
simultaneous editing of one project is ever wanted.

An authored `.als` set carries both an object-id counter and a 60-character revision in its root,
which is independent evidence that object identity is load-bearing for this kind of tool.

## Scope: four parts

### 1. The project is a folder: document, manifest, assets

- A project folder holds the document, a manifest and the audio. **The manifest maps each asset to
  its path and its BLAKE3 hash** — one structure that serves reference, "collect", relink and merge.
- **Reference by default, collect on demand.** ADR 0010 already carries a relative + absolute path
  pair per region; the hash is the one field it gains. Nothing is copied unless the user collects,
  which reuses the shape ADR 0010 left open ("a packaged format that carries its audio").
- **A package is a zip** (ADR 0010 decision 6 says so). `zip` is already a workspace dependency
  (themes). The package needs an extension of its own so `is_project_file` stays unambiguous.
- **Derived analysis stays out of the project and out of the user's sample folders.** ADR 0013
  already keeps the feature cache in `<settings>/library/`. **One exception, and it is a
  different kind of data:** a kind set by hand ([#325](https://github.com/suhina-daw/suhina-daw/issues/325))
  is a decision, not derived data, and recomputing will not reproduce it. It needs its own store,
  **unversioned** — the feature cache is versioned and a features-v2 bump would otherwise destroy
  the user's corrections.
- **Git tracks the document and the manifest, never the audio.** `.gitignore` keeps `samples/` and
  renders out. Committing unchanged audio does not duplicate it, but every new take or render is a
  permanent blob and removing it means rewriting history.

### 2. Merge that does not need git

The baseline is **a merge command over two project folders**, because that is what a user actually
does: A sends a package, B works, B sends it back, merge. No branches, no remotes, no hashes.

- Assets merge as a **set union of hashes** — conflict-free by construction, because
  content-addressed blobs are immutable.
- The document merge is described in an ADR: what a conflict is, what is last-writer-wins, and what
  is reported rather than guessed.
- **`suhina merge-driver`** makes real `git merge` call this instead of diffing JSON textually
  (`.gitattributes` → `*.suhina merge=suhina`). This is an established pattern for formats that are
  text but not line-oriented.
- A merged project whose assets are absent lands in a **supported state**: `project.missingFiles`,
  `project.relinkFiles` and the find-by-name panel already exist (`docs/spec/missing-files.md`).

### 3. The version browser and the musical diff

- **Not a git DAG.** Musicians do not want nodes, edges and hashes. They want a list of named
  versions — the workflow they already perform by hand with "Save As".
- **The version browser** lists versions with date, author and length, in one UI over both
  storages (zip rounds and git commits).
- **The musical diff is the genuinely new part:** which *tracks* changed between two versions, and
  the ability to **audition them side by side or one after the other**. Git can say which lines
  changed; only a DAW can play them.
- Switching versions must be cheap by construction: reuse the audio pool, call `set_audio` only when
  the set of referenced hashes changed, and keep plug-in host processes alive when the device chain
  is unchanged — a version switch is a graph swap (ADR 0005), not a restart.

### 4. A shared live session: one host, guests

- **One host holds the truth.** No consensus, no server, and no need for global ids or change
  metadata. The host is also the relay for audio.
- **We run no infrastructure.** The invitation carries the addresses and a key, so the first
  connection needs no third party at all; a self-hostable `suhina-relay` covers the rest and is
  shipped as a binary the *user* runs.
- Guests are callers with a permission set, extending the `Policy::agents` pattern. Path parameters
  are confined as MCP's read roots already are.
- **Not in this epic:** sub-20 ms ensemble playing between sites. That is a separate product with a
  separate budget, and it is blocked on recording anyway.

## Measured: what a real project folder weighs

Measured on an authored `.als` project folder (41 files, 39.2 MB) — the observations belong in
`docs/spec/als-format.md` and `docs/spec/als-corpus.md`:

| Part | Measured | Share |
|---|---|---|
| Documents, 10 × `.als` (5 named versions + 5 timestamped backups) | **15.0 MB** | **38 %** |
| ↳ of which `Backup/` (autosave copies of the whole document) | 8.3 MB | 21 % |
| Audio, 15 files | 22.8 MB | 58 % |
| `.asd` analysis sidecars, 14 files | 0.8 MB | 2 % |
| Project icon | 0.48 MB | 1.2 % |

- An `.als` is **gzip-compressed XML: 1.62 MB packed from 11.85 MB, 7.3×**. Ten near-identical
  documents are therefore stored ten times over, because gzip does not deduplicate across files.
  **This is the largest measured win available, and it is not audio.** Git (zlib plus delta between
  versions) would hold those ten for an estimated 2–3 MB. Compressed *and* diffable is what neither
  format has alone.
- **`.asd` files are exactly one per audio file (14 of 15) and sit next to the audio**, inside the
  project's own sample folders: **48 % of the files in `Samples/` and 34 % of the project's files,
  for 2 % of its bytes.** The cost is file count and pollution, not space. One of them is 614 KB.
  One recorded take is a 0-byte file with no sidecar, so cache and audio drift apart.
- **Content addressing saves nothing inside one project here**: all 15 audio files are unique. Its
  value is integrity, merge and recognition across projects, not local deduplication.
- **Readability costs size.** A plain-JSON `.suhina` is larger on disk than gzip `.als` for the same
  content until it is compressed — which git does. This is a real consequence of ADR 0010's choice,
  and it is paid back by the same decision that makes diffing possible.
- **Path length is close to the limit:** one collected stem's filename is ~120 characters and its
  full path ~170. A project under a deep user folder would exceed 260 on Windows.
- **The folder-as-project convention is unenforced:** this one folder holds sets from two different
  songs, an almost-empty set and a `Desktop.ini`.

## Not in this epic

- Realtime ensemble playing under 20 ms between sites (separate product, separate budget).
- A CRDT, and global or peer-scoped ids: unnecessary while one host holds the truth, and only needed
  for simultaneous editing of the same object.
- Building a git GUI, a branch graph, or a hosting service.
- Any collaboration feature built as an **extension** (ADR 0015). Collaboration is a network-effect
  feature whose value is zero if the other side does not have it, and ADR 0015 §2 says nothing
  Suhina ships may need an extension. **An extension is for what the project cannot do itself** —
  a component whose licence it cannot rely on, or whose crash must not take Suhina down (ADR 0015
  §1) — **not for what it would rather not ship.** Neither applies here: every dependency is
  GPL-3.0-compatible and the code is ours. Where a licence has stood in the way of a capability,
  this project has removed the obstacle rather than the capability — the VST2 interface
  decision being prepared beside this one does exactly that.
- Mobile, accounts, cloud storage, or anything that requires a service the project operates.
- Automatic download of anything.

## Acceptance criteria

- [ ] A project opens, saves and reopens as a folder with a document, a manifest and referenced
      audio, and **a v1 `.suhina` file still opens** (ADR 0010 decision 3).
- [ ] The manifest records a path and a BLAKE3 hash per asset; a file whose hash does not match is
      reported by name and nothing is written.
- [ ] Collecting copies only what the project references, skips assets already present by hash, and
      is one undo step.
- [ ] A package written and read again reproduces the project exactly (round-trip test), and its
      extension is distinguishable from the document's by `is_project_file`.
- [ ] A kind of sound set by hand survives a features-version bump, and is found again for the same
      bytes at a different path (content-addressed, not path-keyed).
- [ ] `clip` identity is stable: moving a clip, splitting it or copying it changes only what it
      should, and a duplicated clip can be edited without changing its sibling
      ([#352](https://github.com/suhina-daw/suhina-daw/issues/352)).
- [ ] `clip.move` and its siblings accept an id, and `index` still works, with the deprecation
      documented in the command's own description.
- [ ] A merge of two project folders with disjoint edits completes without a conflict; a merge of
      overlapping edits on one clip reports it rather than guessing, and the result is one undo step.
- [ ] `suhina merge-driver` makes `git merge` on two diverged documents produce a valid project
      rather than a conflicted JSON file, and refusing is a documented outcome, not a crash.
- [ ] The version browser lists versions from both a folder's packages and a git repository, and
      switching a version while playing does not re-decode unchanged audio and does not restart
      plug-ins whose chain did not change.
- [ ] The musical diff names the tracks that changed between two versions and can audition them.
- [ ] With a host and one guest on a LAN: the guest joins from an invitation, the transport and
      launcher are shared, a guest's command outside its permission set is refused with a reason,
      and disconnecting the guest leaves the host's project intact and playing.
- [ ] No new code is reachable from the audio callback: `Renderer::process` still never allocates,
      locks, blocks or does I/O (AGENTS.md rule 3).
- [ ] With no network, no git binary and no relay present, every other feature works and the missing
      capability is an ordinary state with a message (ADR 0015 §2).
- [ ] `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`
      and `python scripts/docs.py check` pass.
- [ ] Tests that show it (AGENTS.md rule 7): folder and package round trips; a v1 file read with ids
      derived in document order; hostile manifests and packages (paths that escape, absurd sizes,
      hash mismatch, a zip bomb); merge cases including concurrent edits to one clip; a guest
      command refused by permission and by path confinement; a degraded render with assets absent,
      as a golden file.
- [ ] Documents updated (`python scripts/docs.py impact`): new ADRs (musical time and tempo; object
      identity; the project folder and package; session sharing; network transport; the audio link),
      new specs (`docs/spec/session-sharing.md`, `docs/spec/project-package.md`,
      `docs/spec/project-versions.md`), the crate map in `AGENTS.md` and `docs/architecture.md`,
      `ROADMAP.md`, `SECURITY.md` (the control channel it already scopes), and a licence row per new
      dependency in `ATTRIBUTION.md`.

## Risks

1. **"P2P" quietly becomes "we run a server".** Hole punching succeeds on roughly 70 % (±7.1 %) of
   measured attempts, and about 21 % of measured peers had no public address at all (CGNAT). A relay
   is mandatory, so a **self-hostable `suhina-relay` belongs in the same milestone as the
   transport** — never a dependency on someone else's public relays.
2. **Shipping neither half.** The exact failure mode of the previous generation. The boundary above
   goes into the documentation, and the asynchronous half ships first.
3. **Musical time decided wrong before any sync exists.** If wall-clock samples reach the model, a
   multi-peer tempo map becomes a model-wide rewrite. Positions stay in beats and the tempo map is
   a list of changes keyed by beat that merges three-way; this landed with #402 and ADR 0024 is
   accepted.
4. **Schema evolution meeting replication.** Two peers on different versions is the hard case. The
   schema is versioned independently of any replication format, migrations are idempotent and
   commutative, and syncing across incompatible major versions is refused rather than merged lossily.
5. **A missing asset or plug-in experience worse than no collaboration.** Content-addressed blobs
   with an explicit per-asset state (available, fetching, unavailable), silence substituted
   deterministically in the offline path, and a golden file for exactly that degraded case.
6. **Path length and cloud-sync folders.** A git repository inside a cloud-synced folder is a known
   source of corruption, and project paths approach 260 characters on Windows. Both need a decision.

## Notes

- **Where the code goes** (`docs/architecture.md` › Where new code goes): a new layer-2/3 crate for
  the session protocol and the peer boundary, beside `automation`; the identity change in `model`
  first and then every reader and writer; the folder and manifest in `project-file`; the merge in
  `project-file` (it maps the model, as the readers do); the version browser and the musical diff in
  `ui` on top of commands. A `suhina-relay` binary beside `suhina-plugin-host`. Nothing below the UI
  depends on a network library.
- **Dependencies, all verified GPL-3.0-compatible (rule 9):** `blake3` (CC0-1.0 OR Apache-2.0 OR
  Apache-2.0 WITH LLVM-exception; `notify` already sets a CC0 precedent), `iroh` (MIT OR
  Apache-2.0), `quinn` (MIT OR Apache-2.0), `mdns-sd` (Apache-2.0 OR MIT), `ring` (ISC, with
  BoringSSL-derived Apache-2.0/ISC and an MIT/Apache-2.0 `once_cell` polyfill), and for reference
  `yrs` 0.28 and `loro` 1.16 (both MIT) if a CRDT is ever wanted. `diamond-types` is ISC but
  unmaintained since August 2022 and text-only: **rejected on maintenance, not on licence** — record
  the reason so it is not re-added on a licence argument. Automerge's Rust crate is MIT, but its
  **npm/JavaScript** packages carry different terms and must be checked separately before any WASM
  client. `zip` and `interprocess` are already dependencies.
- **Prior art worth reading before the ADRs:** the interval-based approach to latency, the
  per-peer jitter buffer and the queue-as-latency-knob of the established network-audio tools, and
  the published measurement that ensemble quality degrades smoothly with delay rather than falling
  off a cliff. SonoBus is GPL-3.0 and JackTrip is MIT, so both may be ported with notices kept and
  credited in `ATTRIBUTION.md` under rule 9 — the clean-room rule concerns proprietary products, and
  neither is one. What ports is the streaming structure and the jitter buffer; the alignment problem,
  which is the hard part, this project already solves (ADR 0005, ADR 0006).
- **Related:** ADR 0001 (neutral model), ADR 0002 (commands as the replication unit), ADR 0005
  (edits while playing), ADR 0006 (which already answers "plugin state is not portable" by hosting
  out of process), ADR 0007 (tabs, and copying between open projects, listed as not decided),
  ADR 0010 (the folder and package it left open), ADR 0011 (`set_audio`), ADR 0012 and 0013 (where
  caches live), ADR 0015 (why this is not an extension), AGENTS.md rules 1, 2, 3, 6, 7 and 9.
- **Decisions that belong in the ADRs, not here:** the representation of tempo over time; what an
  object id is and how it survives a copy; the project folder layout and the package's extension;
  what a merge conflict is and what is reported; the packet format and the invitation; whether git
  is optional and how its absence degrades; where per-asset state is shown.
- **Board:** `enhancement` + `epic`. **A new area label is needed** — the existing set (`engine`,
  `ui`, `formats`, `browser`, `audio-io`, `plug-ins`, `devices`, `presets`, `tests`) has no home for
  this; `collaboration` is proposed. Phase **M8 or later**, Priority **P2**, on the reasoning that a
  DAW is adopted for being a DAW and this is a retention and differentiation feature, not an
  acquisition one. Two of its parts are exceptions and should land earlier because they are cheap
  now and expensive later: **musical time** and **object identity**.
- **Sub-issues this should be split into:** (1) the ADR on musical time and tempo over the song (done:
  #402 landed the tempo map and ADR 0024 is accepted, so it has no issue of its own), with
  tests, independent of everything else; (2) object identity in the model — ids derived from v1
  files, all addressing moved to ids, `ParamType::Clip` with a deprecation window; (3) the project
  folder, the manifest and the package, with the format version bump; (4) the hand-set kind store,
  separate from the versioned feature cache, closing [#325](https://github.com/suhina-daw/suhina-daw/issues/325);
  (5) the merge over two folders, with the conflict rules; (6) `suhina merge-driver` and the git
  integration, including ignoring `.git` in the library scan and watch; (7) the version browser;
  (8) the musical diff; (9) the release of `.locr` compatibility, as its own documented decision (#565)
  (it is a fossil of a name the project may not write out, its functional code is about four lines,
  and no such file exists in the wild); (10) the session protocol and the peer permission model;
  (11) the transport: direct, hole-punched and relayed, with the invitation; (12) `suhina-relay`;
  (13) recording, input and monitoring — **the prerequisite of everything a guest performer needs,
  and independently valuable**; (14) the audio link and the delay quantisation that makes a remote
  performer land in time; (15) **the author identity** — a display name in the settings, the
  project's own metadata, and a separate key the transport authenticates with. It is last in this
  list only because it was found last: it is the smallest item here and (5), (7) and (10) all want
  what it provides, so it should land before any of them. A name is the human-readable half of the
  "who changed this" that a merge and a version browser need, and a name alone cannot support the
  claim, because a name is a string anybody can set.
- **A note on what this is worth:** version control for music is valuable to one person, which is
  exactly what the previous generation of collaborative tools was not. Every one of them required
  somebody else to be useful, and every one of them is gone; every one of them was also a service.
  This is not. That is the difference worth defending.

## Appendix: verified facts for the ADRs

- **A shared live session needs no CRDT.** With one host as the truth and a permission model, the
  command stream is the replication unit (ADR 0002). Global ids, change metadata and tombstones only
  become necessary for simultaneous editing of one object.
- **The engine can already line up a very late path.** Latency compensation is capped at **2
  seconds** (`MAX_LATENCY_SECONDS` in `crates/engine/src/lib.rs`) and is a per-path delay line built
  with the graph. A network path with a measured delay is the same shape as a plug-in's latency,
  four orders of magnitude larger. **Quantise the delay to a musical interval** so the graph is
  rebuilt only when the interval changes — this is what keeps a jittery link from thrashing the
  graph, and it is borrowed from the interval-based approach to latency in the network-audio tools.
- **A guest performer is a monitoring problem, not a recording one.** The guest monitors themselves
  locally at zero latency and the network carries only the monitor stream; the take is recorded on
  the guest's own machine, uncompressed, and transferred as a blob. **The network then affects only
  what is heard, never what is kept.**
- **Bandwidth:** uncompressed 48 kHz/24-bit stereo is about 2.3 Mbps per stream; a lossy four-person
  arrangement is on the order of 768 kbps out. One guest is cheap, several are not.
- **Two problems this does not solve and should not pretend to:** audio clocks on two machines drift
  by tens of ppm, so a stream needs rate adaptation; and floating-point summation order, FMA and
  plug-in nondeterminism mean **bytes can be guaranteed and a bit-identical render cannot**. Any
  cross-peer render-equality test is scoped to the built-in effects and instruments.
- **The document is the heavy part of a small project.** Measured above: 38 % of a 39 MB project
  folder. This is the opposite of the assumption that audio dominates, and it holds for exactly the
  clip-and-loop workflow this project is built around.
- **The `.locr` count, for the removal issue:** about 39 occurrences across 12 files, of which the
  functional code is two constants and two branch points; the rest are tests, doc comments and
  user-facing help text.
