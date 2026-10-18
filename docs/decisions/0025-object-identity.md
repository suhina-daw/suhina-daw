---
description: Why clips, arrangement lanes and scenes gain an id, what an id is, why ids need not be globally unique, and the four steps that make every command address objects by id instead of by position
read_when: object id, ClipId, clip identity, positional addressing, clip.move, duplicated clips, make unique, project file version, ParamType::Clip, merge, version diff
type: decision
status: current
last_verified: 2026-10-18
covers:
  - crates/model/src/lib.rs
  - crates/model/src/ids.rs
  - crates/session/src/commands.rs
  - crates/session/src/lib.rs
  - crates/project-file/src/lib.rs
  - crates/als/src/lib.rs
---

# 0025: Object identity in the model

- **Status:** accepted (2026-10-17, implemented in [#568](https://github.com/suhina-daw/suhina-daw/issues/568))
- **Date:** 2026-10-15

## Context

Three of the model's objects have an id and the rest do not. `TrackId(pub u32)` (`crates/model/src/lib.rs:12`),
`ChannelId(pub u32)` (`:16`) and `ContentId(pub u32)` (`:20`) name their objects; a `Clip` (`:285`),
an `ArrangementLane` (`:453`) and a `Scene` (`:467`) are identified by **where they sit in a `Vec`**.
`Clip`'s fields are `content`, `start`, `length`, `offset`, `loop_range`, `enabled`, `fade_in`,
`fade_out`, `fade_in_curve`, `fade_out_curve` and `extensions` — there is no field that says which
clip it is. `ArrangementLane` is a `name`, an optional `track` and `clips: Vec<Clip>`; a `Scene` is
a `name` and an optional tempo. Even the model's own error type reports positions:
`ValidationError::BadFades { lane: usize, clip: usize }` (`:512`, raised at `:596`).

Position is a fine identity for one program reading one file, and it stops being one as soon as two
copies of a project exist:

- **"Move the third clip" does not mean the same thing to two machines.** The command is
  `clip.move { track, index, start, to_track?, stack? }` (`crates/session/src/commands.rs:395–411`),
  and `index` is "clip index on the track, from `project.clips`". Two objects that changed
  independently both answer to "the third clip".
- **A readable diff is impossible.** Moving one clip makes **everything after it on the lane** look
  changed; splitting a clip makes **half the lane** look changed; copying a clip looks like several
  edits. A version comparison that reports noise instead of "these three clips moved" is not a
  version comparison.

And the identity that is already there is per-project and allocated as `max + 1`:
`TrackId` at `crates/session/src/commands.rs:926`, `ContentId` at `:1243`, and in
`crates/session/src/files.rs:88` and `:470` (which at least use `checked_add`). A blank project
starts from literal values — `ChannelId(100)`, `ChannelId(90)`, `ChannelId(91)` and
`TrackId(10 + offset)` (`crates/session/src/blank.rs:14–15`, `:49`) — so two machines do not merely
risk colliding, they **start collided**.

This is worth doing with no collaboration anywhere in sight, and this is the strongest argument for
it:

- **[#352](https://github.com/suhina-daw/suhina-daw/issues/352) ("Duplicated clips share their notes:
  editing one duplicate edits all", `enhancement, ui`) is the same missing identity.** A duplicate
  is a new placement of the same content and the two are indistinguishable afterwards. The model
  already concedes the shape of the fix ([ADR 0001](0001-daw-neutral-model.md) › Consequences):
  *"Shared content means editing a clip content can change several clips at once. Commands must offer
  'make unique' (copy-on-edit)"* — and "make unique" is only expressible if a placement can be
  named.
- **A musical diff needs it.** The version browser's whole point is naming what changed between two
  versions of a project and auditioning it. That is a feature valuable to one person, and it cannot
  be built on positions.

### Two facts that decide the cost

**The engine never sorts by id**, so the audio path does not notice this change. Its only sorts are
by start time (`crates/engine/src/lib.rs:796`), chain depth (`crates/engine/src/buses.rs:72`), note
time (`crates/engine/src/launcher.rs:618`), marker time (`crates/engine/src/audio.rs:95`) and an
internal graph sequence (`crates/engine/src/swap.rs:259`). **The golden renders therefore do not
break on an id change** — `crates/engine/tests/golden/`, `devices/fm-synth/tests/golden/` and
`crates/ui/tests/golden/` are untouched. That single fact is what makes this affordable rather than
a DSP regression hunt.

**The window is now.** Measured: **no `.suhina` or `.locr` file exists anywhere in the tree** — no
fixtures, no committed examples, no installed base. The format can be designed freely rather than
conservatively, and every week in which people make real saves makes the migration more expensive.
The same reasoning applies to the release of `.locr` (its own decision, not this one).

## Decision

1. **Clips, arrangement lanes and scenes gain an id: `ClipId`, `LaneId`, `SceneId`.** They join
   `TrackId`, `ChannelId` and `ContentId` as `u32` newtypes. A `Slot` (`crates/model/src/lib.rs:481`)
   does not gain one: it is a cell of a grid, addressed as (column, scene id), not an object that
   moves between parents.

2. **An id is a monotonic counter stored in the project, and a value is never reused.** The project
   keeps the next value to hand out (`Project::next_id`, written as `next_id` in the file); deleting
   an object does not return its id to the pool, and neither does an undo: a session keeps the
   counter where it got to when it steps back through its history, so an object added after an
   undo never gets the id of the one the undo took away. **Clips, lanes and scenes share the one
   counter**, so a value names at most one object of the three kinds; that is cheaper than a type
   tag and leaves one block of values to reserve (decision 3). The values run from 1 to
   `u32::MAX − 1`: 0 is "no id yet" (an object built in code, a file written before ids), and the
   largest value is never handed out, so the counter can always name the next one; a project that
   reaches it can still be opened and edited, only not given new objects. A
   deleted-then-recreated object must not inherit an identity, because an id that can be resurrected
   silently aliases two different objects — which is exactly the failure that makes a diff
   untrustworthy. This also keeps ids short, ordered and readable in a file and in `project.clips`,
   which a random value is not.

3. **Ids do not have to be globally unique, and this is what keeps the change cheap.** Two peers
   who each add a clip to diverged copies will allocate the same value. That is resolved by how a
   merge works, not by making the id bigger:

   > **A merge is always against a common ancestor.** Objects that existed in the ancestor are
   > matched **by the ancestor's ids**, which both sides inherited unchanged. Only objects created
   > since the ancestor are new, and those are **renumbered on import, with the destination
   > allocating their ids** — the same operation that already copies objects between projects
   > ([ADR 0007](0007-several-open-projects.md) › Not decided yet).

   Collisions are therefore confined to objects that have no ancestor in common, and those are
   precisely the objects that must be given fresh ids anyway.

   **Repeated merges with the same guest: id blocks.** Renumbering on import needs a mapping per
   guest per merge, or a clip the guest made in version 3 looks new again in version 5. The host
   avoids it by **reserving a block of its counter for the guest** when it invites one or exports a
   package (a million values, say): the host moves its own `next_id` past the block, and the guest
   starts its counter at the block's first value. The objects the guest makes then keep their ids
   when they come back, no mapping is needed, and the ids stay short and readable. Renumbering
   stays the fallback for two folders that meet with no common origin. One shared counter
   (decision 2) is what makes a single block enough. The block is a counter value and a length,
   nothing in the model changes; the invitation and the package that carry it are
   [ADR 0023](0023-collaboration-and-project-versions.md)'s and [ADR 0026](0026-project-folder-and-package.md)'s. **Peer-scoped or globally unique ids
   are not needed for asynchronous merge across a package or a repository**, and they are deferred
   until — and only if — simultaneous multi-writer editing of one project is ever wanted, which the
   host-authoritative session model deliberately avoids ([ADR 0023](0023-collaboration-and-project-versions.md)).

4. **Ordering: four steps, each shipping on its own** (AGENTS.md: "Keep the tree building at every
   step"):
   - **A.** The ids exist in the model, and **a v1 project file read derives them in document
     order**, so no file breaks and the current format still round-trips. No behaviour changes yet.
   - **B.** Every addressing moves to ids, the UI included: selection, context menus, drag/drop,
     copy/paste and the sub-row layout. Still no command or file change.
   - **C.** Command parameters take an id, and **`index` is still accepted during a deprecation
     window**, documented in each command's own description so it reaches the CLI help, the MCP tool
     list and the generated `docs/commands.md` without a second place to remember. **The window
     ends with the project format's next version** (ADR 0026's bump): the change that makes ids
     required in the file removes `index` (and `track` where it only served `index`) from the clip
     commands, so the two meanings are never both live for longer than one format version.
   - **D.** The file format gains a version and keeps reading v1. **The version bump itself is
     [ADR 0026](0026-project-folder-and-package.md)'s decision**, not this one.

   **As implemented** (one pull request, [#568](https://github.com/suhina-daw/suhina-daw/issues/568),
   steps A to C; of step D, the reading side, while the version bump waits for ADR 0026): ids live in `crates/model/src/ids.rs`; `Project::assign_ids` gives every object
   without one the next ids in document order (each arrangement lane and then its clips, then the
   scenes, then the slots' clips column by column), which is how a file without ids, the `.als`
   reader and a session opening any project get them, and how a command that adds an object
   without allocating its id gets one. A command that must report the new id allocates it itself
   (`Project::new_clip_id` and its neighbours). A copy (paste, duplicate, a duplicated scene, the
   second part of a split, the part of a covered clip after one placed over it) is a new clip with a
   new id; a moved, trimmed or edited clip keeps its id. For step D, the file stays **version 1**:
   `id` and `next_id` are optional fields that a reader without them ignores, and a reader with them
   derives what is missing ([`docs/spec/suhina-format.md`](../spec/suhina-format.md)). The
   commands resolve a `clip` id (or a name only one clip has) to the clip's place in the project as
   it is when the command runs, in one place before any command sees its parameters
   (`commands::resolve_clip_ids`), so every command that names a clip by `track` with `index` or
   `scene` takes `clip` too, the `note.*` commands and `transport.play` included.

5. **The parameter type is `ParamType::Clip`, and it is not new machinery.** `ParamType::Track`
   already exists (`crates/session/src/commands.rs:106`, `const TRACK`) and its JSON Schema is
   already `json!(["integer", "string"])` — "track id or name" (`:139`). The registry generates the
   agent-facing schema from that, so a `Clip` variant is the same shape applied to clips, and the
   `id or name` convention is already the house style.

6. **Addressed by id, reported by id.** The read commands stop reporting clips by position:
   `clip_json(p, name, track, index: usize, clip)` (`crates/session/src/commands.rs:812`) reports
   the id instead. A read API that names objects by index would keep the ambiguity in the agent
   interface after the commands were fixed.

## Alternatives considered

- **Leave positional addressing and match objects heuristically when merging or diffing.** This is
  what the current model forces, and it fails in a specific and predictable way: a heuristic has to
  guess that "the third clip on track 2" in one copy is "the third clip on track 2" in the other,
  and it is wrong whenever a clip was inserted, deleted or moved before it. **Every object after a
  single insertion is mis-matched**, and the merge is then confidently wrong rather than visibly
  failed. It also cannot distinguish "both sides edited the same clip" from "both sides edited
  different clips that happen to sit at the same index" — which is the difference between reporting
  a conflict and destroying one. Rejected.

- **Use a content hash as the object's identity.** Rejected for two independent reasons, either of
  which is enough. **Identity must survive an edit**: moving a clip, changing its gain or trimming
  it must leave the whole system still referring to the same clip, and a hash changes the moment the
  content does. And **two identical objects must stay distinguishable from one object referenced
  twice**: two clips with identical notes on two tracks hash the same, so a hash cannot tell
  "duplicated" from "shared" — which is exactly the distinction [#352](https://github.com/suhina-daw/suhina-daw/issues/352)
  needs and exactly the distinction the content pool already models deliberately. A hash **is** the
  right identity for content and for blobs, where immutability is the point; that belongs to
  [ADR 0026](0026-project-folder-and-package.md), and this ADR does not decide it.

- **Peer-scoped ids now, for example a `(peer, counter)` pair.** The real cost is not the bytes, it
  is that **a peer identity enters the model**: every id in the file, every id in every command
  parameter, every id in a read result, and every test fixture grows a component that only means
  something once two peers exist. It also ages badly: the peer a project was created on becomes
  permanent content of a file that is otherwise about music. Deferred, because decision 3 shows the
  common-ancestor rule removes the need for it.

- **UUIDs everywhere.** 128 bits per object in a file whose readability is its stated reason for
  existing ([ADR 0010](0010-project-files.md) › 1: *"people and agents can read and diff it"*), for
  a property decision 3 shows is not required. A `uuid` crate is already in the dependency graph
  transitively, so availability is not the objection; the objection is that it buys uniqueness that
  the merge model does not need and pays for it in every diff a human reads. Rejected for now, and
  it is the natural upgrade path if simultaneous multi-writer editing ever arrives.

- **Derive the id deterministically from position and content.** This is the tempting middle: no
  counter to store, stable-looking values. It fails on its own definition — an id must **not** change
  when the thing it names is edited or moved, and deriving it from position or content is precisely
  a promise that it does. Rejected.

## Consequences

- `model` changes first, and then **every reader and writer**: `als`, `project-file`, `session`,
  `engine`, `ui` and the tests of each. The measured blast radius is **~245 id-construction sites in
  34 files** (`TrackId(` 111, `ChannelId(` 82, `ContentId(` 52), plus ~130 `.clips` and ~203 `.lanes`
  references.
- A newtype means the compiler finds all of it, **but not the semantic change**: if a parameter
  stays an integer, an `index` and an `id` compile identically and mean different things. Step C's
  deprecation window and its tests are the mitigation, and this is the one place in the change where
  a green test suite is not evidence.
- **The agent-facing interface is a breaking change and is treated as one.** Positional addressing
  reaches **seven `clip.*` commands** — `clip.delete` (`:297`), `clip.setGain` (`:306`),
  `clip.setFades` (`:317`), `clip.copy` (`:339`), `clip.cut` (`:346`), `clip.duplicate` (`:366`),
  `clip.move` (`:399`) — and beyond clips, `track.move`'s `to` (`:269`), `device` (`:107`) and
  `scene` (`:108`). [ADR 0002](0002-commands.md): *"Command ids are part of the public interface for
  agents; renaming one is a breaking change."* A parameter that keeps its name but changes its
  meaning is worse than a rename, so the deprecation window is part of the change, not a courtesy.
- The UI's `(TrackId, usize)` disappears: `selected_clip() -> Option<(TrackId, usize)>`
  (`crates/ui/src/arrangement/arrangement_view.rs:201`), `clip_menu(ui, track, index, …)`
  (`crates/ui/src/interaction/menus.rs:95`), copy/cut/paste from `(track, index)`
  (`crates/ui/src/app.rs:1674`), and `ClipBox`'s `index`/`row`/`rows`
  (`crates/ui/src/arrangement/timeline.rs:66`, `:83–84`) with `sub_rows` (`:247`) computing the stacking layout
  from positions.
- `crates/project-file` gains a v1 reader that **derives ids in document order**, and the writer
  emits them; the round-trip test and every model field's line in the spec move with it
  ([ADR 0010](0010-project-files.md) › Consequences). `docs/spec/suhina-format.md` is updated in the
  same change.
- `ValidationError` reports ids rather than indices (`BadFades { lane, clip }` becomes ids), which
  changes what a failed validation prints to a user and to an agent.
- `docs/commands.md` is regenerated (`suhina-cli commands --markdown`, on `main`), and `keymap.json`
  binds commands, so a parameter change reaches the CLI, the MCP tool list, the documentation and
  every user's keymap.
- **This is a prerequisite for [ADR 0027](0027-merge-and-versions.md).** A version diff and a merge
  cannot be honest without it, and both are why the work is worth doing before any networking exists.
- `Position` and `i64` are unaffected; only project-local object identity changes.

## Not decided yet

- **Tracks, channels and contents still allocate `max + 1`.** Their ids are as old as the model and
  are left as they are here; moving them to the same never-reused counter is the next step when a
  merge needs it (issue 05 of the collaboration plan), and the literal ids of a blank project
  (`crates/session/src/blank.rs`) go with it.
- **Whether content becomes content-addressed.** A `ClipContent` is already shared through the pool,
  so a content hash is a natural identity for it — but that is a statement about assets, packages
  and blobs, and it belongs to [ADR 0026](0026-project-folder-and-package.md).
- **Whether an id needs a type tag**, so an id taken from one kind cannot be used for another.
  Today all of them are `u32` newtypes, and a `ClipId` cannot be passed where a `TrackId` is wanted
  — which may be enough, and the question is whether a wire format or a command parameter needs the
  same guarantee in a place the compiler cannot check.
- **Whether the derived-in-document-order rule is the v1 read path forever**, or only until a v2 file
  carries ids explicitly. Decided as far as it goes: a file without ids keeps opening; ADR 0026's
  version decides whether a newer file may leave them out.

Decided since: **an imported `.als` gets fresh ids.** Its own id space is separate and its object-id
counter is not ours to reinterpret; the importer gives every lane, clip and scene a new id in
document order and keeps nothing of the set's ids.

## Note on independent evidence

Using ids at all is not a Suhina invention. An authored `.als` project, unpacked, carries
`<NextPointeeId Value="49663" />` — an object-id counter — and a 60-character `Revision` hash in its
XML root. Another tool of this kind found it needed stable object identity too. This is observation
of a self-authored file, which AGENTS.md rule 1 permits; the format observations themselves belong
in [`docs/spec/als-format.md`](../spec/als-format.md) and [`docs/spec/als-corpus.md`](../spec/als-corpus.md).

**The one honest tension:** [ADR 0001](0001-daw-neutral-model.md) › Consequences requires that *"Each
addition is checked against at least two formats before it lands, so it does not bake in one DAW's
assumptions"* — and there are currently two formats in the tree (`als` and `project-file`), while
`crates/dawproject` does not exist (M7 is unstarted). So the second format that would check this
design is Suhina's own file, which is a weaker witness than DAWproject would be. The new field must
be filled by the `.als` reader as well, and the round-trip test is what keeps it honest in the
meantime.

## Sources

- This repository: [ADR 0001](0001-daw-neutral-model.md) (the model, shared content, the two-format
  rule), [ADR 0002](0002-commands.md) (the registry, parameters as JSON, command ids as a public
  interface), [ADR 0007](0007-several-open-projects.md) (copying between open projects, not decided),
  [ADR 0010](0010-project-files.md) (the file, its version, why JSON),
  [ADR 0023](0023-collaboration-and-project-versions.md),
  [ADR 0024](0024-musical-time-and-tempo.md),
  [ADR 0026](0026-project-folder-and-package.md),
  [ADR 0027](0027-merge-and-versions.md).
- Code as it is: `crates/model/src/lib.rs`, `crates/session/src/commands.rs`,
  `crates/session/src/files.rs`, `crates/session/src/blank.rs`, `crates/project-file/src/lib.rs`,
  `crates/ui/src/arrangement/arrangement_view.rs`, `crates/ui/src/interaction/menus.rs`, `crates/ui/src/app.rs`,
  `crates/ui/src/arrangement/timeline.rs`, `crates/engine/src/lib.rs`, `crates/engine/src/buses.rs`.
- [#352](https://github.com/suhina-daw/suhina-daw/issues/352) (duplicated clips share their notes).
