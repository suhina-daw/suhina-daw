---
description: How Suhina ships a surface program's script inside the app — where it unpacks, how the port scan runs it, and what happens to the user's copy
read_when: shipped script, bundled script, Suhina scripts folder, Scripts folder, Scripts/MIDI, unpack, starter script, controller script on first launch, APC40
type: spec
status: planned
last_verified: 2026-11-21
covers:
  - apps/suhina/src/pack_materialize.rs
  - crates/session/src/extensions.rs
  - crates/session/src/shipped_scripts.rs
---

# Shipped scripts

- **Sources:** Suhina's own design, from [ADR 0036](../decisions/0036-library-and-shipped-content.md)
  (the library and shipped content), [ADR 0015](../decisions/0015-extensions.md) (extensions) and
  [ADR 0019](../decisions/0019-midi-controllers-and-mapping.md) › 10 (surface programs).
- **Builds on:** the extensions store and its supervisor ([extensions](extensions.md),
  [surface programs](surface-programs.md)), the pack that rides inside the executable and unpacks
  into the library (ADR 0036 › 2 and 3), and the command registry ([ADR 0002](../decisions/0002-commands.md)).
- **Status:** planned ([#1676](https://github.com/suhina-daw/suhina-daw/issues/1676)). Built so
  far: the clean-room script in `packs/apc40-mkii/` (#1677), the unpack into the library's
  `Scripts/MIDI/` folder at every start — each file only when it is missing — and the store
  install, with its tests: the shipped content installs when the store does not hold it, a
  removal is remembered, and only a copy still carrying exactly the recorded content is
  updated. The script's end-to-end run through the supervisor follows.

## What ships

A **surface program's script** may ride inside the executable as Suhina's own content, like the
NAM pack (ADR 0036 › 3). The first is the Akai APC40 MkII's ([extensions](extensions.md) › Running
one; ADR 0019 › 10).

- The script's files live in the tree under `packs/` and are embedded with `include_bytes!`
  ([ADR 0036](../decisions/0036-library-and-shipped-content.md) › 3): its `extension.json`, the
  script itself, a `README.md` carrying its provenance, and its licence text.
- A shipped script is **written from public documentation alone** — the controller's public MIDI
  implementation chart and its public communication protocol — never from another product's code
  or content, and it is Suhina content: CC0-1.0, its provenance README and licence text riding
  beside the files (ADR 0036 › 1 and 6, [CONTRIBUTING.md](../../CONTRIBUTING.md) › Shipped
  content, AGENTS.md rule 14).
- A controller's **proprietary modes** are put right by a message its public protocol document
  describes. The APC40 MkII's script sends, when its ports open, the Application/Configuration
  identifier that puts the device in the mode the document names Ableton Live (0x41): every
  button momentary, the button LEDs the host's own. The document, and the behaviour it comes
  from, are cited in the script's README.

## Where it lands, and what runs

- At start, each shipped script unpacks into the library's `Scripts/MIDI/` folder
  (`Documents/Suhina/Scripts/MIDI/<id>/`), **each file only when it is missing**: a file the
  user carries is theirs, never overwritten, never moved (ADR 0036 › 3, the pack's own rule). A
  write failure is said in the start-up warnings, one line per script, and the next start
  tries again.
- What runs is the **store's copy**, as for any surface program: the app installs the script into
  the extensions store the way `extensions.add` does, and the store's port scan starts it for the
  device the manifest names ([extensions](extensions.md) › Running one). The folder in
  `Scripts/MIDI/` is the user's to read and edit; what runs is the store's copy, so editing the
  folder does not change the running program. To run an edited script, remove the extension and
  add the folder back (extensions › Adding, listing, removing).
- **A removal is remembered.** `extensions.remove` on a shipped script stops it and forgets it,
  and later starts do not install it again; adding it back from the folder is the way to want it
  again.
- **An update never overwrites the user's work.** Suhina keeps a record of the content it
  installed for each shipped script. When the app carries a newer version, the store's copy is
  replaced only while it still carries exactly the recorded content; a copy whose files the user
  has changed is left alone, and the `Scripts/MIDI/` folder copy is never overwritten either —
  deleting a file unpacks the shipped version again.

## Testing

- The unpack, like the pack's (`shipped_scripts_tests`): a fresh library gets every file, an
  existing file is not touched, a deleted file comes back, the manifest's pin holds against
  the store's own validation, and a failure is not fatal.
- The store install (`shipped_scripts_tests`): a fresh store gets every script whole and the
  store lists it as usable, a second start installs nothing, a removed script stays removed
  while adding the library's folder back is what brings it in, an edited copy — or the user's
  own extension under the id — is left alone, an untouched copy is replaced when the app
  carries a newer version, and a failure is not fatal.
- The shipped script: the end-to-end tests every surface program gets
  (`crates/lua-runner/tests/programs.rs`), run against the pack's files — it starts, the mode
  message goes out when the ports open, a press of each strip button sends its command, and the
  LEDs follow the project.
- The start: a fresh library gets the `Scripts/MIDI/` folder and the store entry, and a removed
  shipped script stays removed on the next start.

## Not yet

- Shipping more than the first script, and a newer version offered in the window.
- Unpacking on a machine that never runs the desktop app: the CLI and the MCP server read the
  store only, and the app is the one that unpacks.
