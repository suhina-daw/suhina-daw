---
description: The library under Documents/Suhina - the factory pack built into the executable and unpacked beside the user's own files, library-relative paths in projects, device.savePreset, and what content may ship with Suhina
read_when: library, Documents Suhina, Suhina pack, factory pack, starter pack, Presets folder, Sounds folder, NAM models folder, Cabinets folder, library-relative path, savePreset, save as preset, shipped content, licence, CC0, GPL, Guitarix, Freesound, pelennor, unpack, pack files, packs folder
type: decision
status: accepted
built: partly
last_verified: 2026-11-08
---

# 0036: The library and the shipped content

- **Date:** 2026-11-07, unpack added 2026-11-08
- **Origin:** [issue 1279] and the maintainer's decision there of 2026-11-07; extends
  [ADR 0014](0014-stock-presets.md) (stock presets) and the two NAM devices' spec
  ([nam-amp-and-cab.md](../spec/nam-amp-and-cab.md)).

## Context

A fresh install's NAM folders are empty, so neither NAM device has anything to run, and there is
no place the user's own presets and sound files call home as a library. The stock presets added
for the two devices work, but they only set controls — no file comes with them ([issue 1183]).
The devices' loaders read folders the Settings page points at ([nam-amp-and-cab.md] › Folders)
and store absolute paths in projects.

## Decision

1. **What may ship with Suhina.** Content that its publisher has licensed compatibly with
   GPL-3.0-or-later ships as it is, with its author, source and licence recorded beside it. The
   publisher's licence is enough; the project does not chase each capture's original rightsholder
   beyond that record. An author asking for their file's removal is removed
   ([IP.md](../../IP.md)). Only two things block content: it was taken from a commercial
   product's library, or its licence or terms forbid redistribution (NC, ND, a service's terms
   that forbid passing files on, a download with no licence). This rule is also written in
   [CONTRIBUTING.md](../../CONTRIBUTING.md) and [AGENTS.md](../../AGENTS.md) in the same pull
   request, so it stops being stretched beyond what it says.

2. **The library lives in `Documents/Suhina/`:**

   ```text
   Documents/Suhina/
   ├── Projects/
   ├── Templates/
   ├── Presets/<device>/            ← presets the user saves, one folder per device
   └── Sounds/
       └── NAM/
           ├── Amp models/          ← .nam models
           │   ├── Suhina/          ← the factory pack
           │   └── <user's packs>/
           └── Cabinets/            ← cabinet IRs (.wav)
               ├── Suhina/          ← the factory pack
               └── <user's packs>/
   ```

   A preset is a device's settings, not a sound, so it has a folder of its own at the root, and
   it covers every device. Sounds holds every sound; NAM sits under it as the device family's
   own folder, other sounds under it later (`Sounds/Samples/` …). Each subfolder is a pack, as
   the device panels list them today; the factory pack is named **Suhina** and is listed first
   with the user's own packs beside it.

3. **A small starter pack rides inside the executable and unpacks beside the user's own
   files.** A few handfuls of files — our own trained models, our own synthetic cabinet IRs,
   and the kept files of the collected sets (CC0 from Freesound, Guitarix's
   `BestPlugins_Amps` and `BestPlugins_Bands` after the listening pass, a few of the pelennor
   collection's) — at an estimated 2–5 MB whole. The app start writes every pack file into the
   library's `Suhina` folders when it is not there: a file the user carries is theirs (never
   overwritten, never moved), a write failure is harmless (the pack still plays from the
   executable, and the next start can try again), and stock presets always find their files.
   The device panel lists the **Suhina** pack first: from the folder when the unpack has placed
   it there, from the executable while the folder does not hold it — so the pack never shows
   twice. Larger content later arrives through an in-app download window, which unpacks packs
   into these same folders — its own issue.

4. **A project stores a file's path relative to the library**, for example
   `Sounds/NAM/Cabinets/<pack>/<file>.wav`, so a project opens on another machine that has the
   pack installed. Opening searches the library there first, and an absolute path is the last
   resort, recorded when it is all there is. The old defaults `Documents/Suhina/NAM models` and
   `Documents/Suhina/IRs` *move into* the device folders at start — a pack carried whole, a
   name moved only where it is missing, an old folder left whole when a name would be written
   over, a moved-empty folder removed, and the move told in a start-up warning — so files
   already placed land in the library and the old default disappears by emptying out. The
   shipped file in a device's `builtin:<device id>/<file name>` state loads from the
   executable always — the pack rides the build, and the unpack's on-disk copy is the user's
   plain file. `device.setNamFile` stays the command that chooses a file, keys and paths both.

5. **`device.savePreset`** saves one device's values — and, with the preset's file it selects,
   one file reference — into the library: `Presets/<device>/<name>.<device id>.preset.suhina`,
   the format of [ADR 0014](0014-stock-presets.md) with its optional `file` field. The device
   panel has **Save as preset…** on it; a stock preset's `file` names a factory file, and a
   user preset's may name the user's own file. The amendment is written on
   [ADR 0014](0014-stock-presets.md) › Amendment: a preset may name a file. The browser lists
   the library's presets beside the stock ones.

6. **Content by the rule in 1**, and the format's proof first: NAM's own MIT-licensed example
   models go into the tests, so the A1 model format is proven to load through `nam-rs` before
   any shipping. Files' provenance is recorded in `README.md` files beside them (`packs/` for
   the pack the build carries, the library's pack folders for the rest): source, author or
   capturer, licence, what changed. A file's name may say which hardware a recording was made
   from, with the no-affiliation sentence AGENTS.md rule 9 asks for; a tone-match IR is a
   frequency response and copies no recording.

7. **Tests:** one test per device loads every shipped file through the devices' normal file
   loading and renders a phrase with it (the offline render is the oracle); for a model of our
   own, the phrase goes through its source device chain and through the shipped file in the same
   run, **aligned by the device's declared `latency()`** so an offline render's own lead rides
   out of the compare, and the pair keeps within a bound (the first own model, an Overdrive
   chain trained at 1000 epochs, sits −13.1 dB from its source at the compare's fitted lead —
   the Overdrive's own dry-path test knows a lead twice its declared `latency()`, so the pack's
   compare takes its alignment from the streams' real lead). A test keeps every pack row parse,
   fit and size inside the ADR's ceiling, and the unpack to write-when-missing only. The
   library-relative path resolution is round-tripped: a project saves to a library-relative
   name, opens on a machine without the file on disk and still renders from the pack.

## Consequences

- The executable grows by the starter pack's size; the estimated 2–5 MB is the pack's ceiling
  and the download window is the growth valve.
- A project is portable across machines that share the pack (any machine with Suhina, for the
  factory pack's files) and reads the files a user carried beside the packs.
- `[ADR 0014]`'s "user-saved presets in the settings folder" is replaced by the library's
  `Presets/<device>/` (the settings row for user presets is removed with this pull request); the
  NAM models and IRs settings rows keep pointing at other folders, and their defaults move to
  the library's `Sounds/NAM/` paths.

## Not decided yet

- The download window and the packs it offers (its own issue).
- Whether the browser lists the library's sound folders the Files place does today.

[issue 1279]: https://github.com/suhina-daw/suhina-daw/issues/1279
[issue 1183]: https://github.com/suhina-daw/suhina-daw/issues/1183
[ADR 0014]: 0014-stock-presets.md
[nam-amp-and-cab.md]: ../spec/nam-amp-and-cab.md
