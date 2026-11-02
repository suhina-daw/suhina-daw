---
description: How the browser shows installed plug-ins: the Plug-ins category lists the instruments or the effects by name and format, the other categories keep them under their vendors; and how each format's own categories map to Suhina's groups
read_when: plug-in views, browser plug-ins, instruments, effects, vendor, sorting, columns, name column, narrow browser, VST3 sub-categories, CLAP features, favourites, tags
type: spec
status: current
last_verified: 2026-10-18
covers:
  - crates/ui/src/browser.rs
  - crates/ui/src/browser/browser_view.rs
  - crates/session/src/rack.rs
  - crates/session/src/library.rs
  - crates/library/src/store.rs
---

# Plug-ins in the browser

Every installed plug-in the scan found (`docs/spec` › the plug-in catalog, ADR 0006 › 9) is
listed in the browser and can be dragged onto a track from any view of it. The plug-ins appear
in the tree under the **Plug-ins** heading, in the **Plug-ins** category
(the **Show Plug-ins** filter), and in searches, pins and tag filters like any other item
([browser](browser.md) › Commands, `library.search`).

## Instruments and Effects

When the category is **Plug-ins** (and no place, search or tag narrows it), a switch above the
results chooses **Instruments** (the default) or **Effects**, and the results are one list of
those plug-ins, no headings: Suhina's own built-in instruments and effects are not in it (they
are under the **Instruments** and **Effects** categories). The list has two columns, **Name**
and **Format** (`vst3`, `clap`, and `vst2` where the host's `vst2` feature is on). The vendor,
version, file and the rest are the plug-in manager's ([plug-in manager](plugin-manager.md)).

Each column heading is a button: clicking it sorts by that column, clicking again reverses; the
name sorts in natural order, the format by its text, the name breaking ties. The choice of
Instruments or Effects is kept with the library (browser.json, `plugins_view`), so it is there
on the next start; a value an earlier version kept (`grouped`, `type`, `flat`) reads as
Instruments. The sorting choice is the window's own and is not kept.

**The name always shows.** The format column sits at the right edge and the name takes the
rest. When the browser is narrowed, from either edge and whichever side of the window it is on,
the name keeps at least 96 points: a column that would leave it less drops, the rightmost first,
before the name is cut. Rows say nothing else at their right end (no "plug-in" note), so a
column never shares its place with other text.

Rows behave the same in the list and in the tree: select, drag onto a track (one row, or
several with the modifiers), pin, tag, right-click menu.

In the **All**, **Instruments** and **Effects** categories the plug-ins stay in the tree under a
**Plug-ins** heading, grouped by vendor (unknown vendors last), each vendor's plug-ins by name;
in **Instruments** and **Effects** only that type. Plug-ins switched off in the plug-in manager
are in neither the list nor the tree.

## Suhina's groups, and how each format's categories map to them

The groups name a plug-in's kind for searches (`library.search` details) and for a later
grouping; the browser does not show them as headings now.

A plug-in's type is `instrument` or `effect` (`suhina_session::is_instrument`: any of its
features containing "instrument"). Under the type, the plug-in's own sub-categories name its
group — the first of these that matches, in the order here; none matches, **Other**:

| Suhina's group | VST3 sub-category (`Fx …`/`Instrument …`) | CLAP feature |
|---|---|---|
| Instrument › Synth | `Synth` | `synthesizer` |
| Instrument › Sampler | `Sampler` | `sampler` |
| Instrument › Drums | `Drum` | `drum-machine` |
| Effect › Reverb | `Reverb` | `reverb` |
| Effect › Delay | `Delay` | `delay` |
| Effect › EQ | `EQ` | `eq` |
| Effect › Filter | `Filter` | `filter` |
| Effect › Dynamics | `Dynamics`, `Compressor`, `Limiter`, `Gate` | `compressor`, `limiter`, `gate` |
| Effect › Distortion | `Distortion` | `distortion`, `saturation` |
| Effect › Modulation | `Modulator`, `Chorus`, `Flanger`, `Phaser`, `Tremolo`, `Vibrato` | `modulator` |
| Effect › Analyzer | `Analyzer` | `analyzer` |
| Effect › Utility | `Utility`, `Spatial` | `utility` |
| (else) Other | — | — |

VST3 sub-categories are the scan's `features` (the words between the `|` of
`moduleinfo.json`'s *Sub Categories*, or of `PClassInfo2`); CLAP's are its features as the scan
read them. A VST3 word matches as a whole word, case-insensitively (`Fx|Reverb` gives the
Reverb group; `Fx|Dynamics|Compressor` gives Dynamics). The names shown are Suhina's own words;
a format's category is used only to pick the group.

## Where the choice is kept

`library.setPluginsView { view: "instruments" | "effects" }` sets it; the reply says the view
chosen. It is saved in `browser.json` as `plugins_view`
([browser](browser.md) › Where it is kept), read with the same rules as the rest of the file: a
missing value, or one an earlier version kept, is `instruments` quietly; any other falls back to
`instruments` with a warning, never a failure.

## Favourites and tags

Pins and colour tags work on a plug-in's row as on a file: the row's key is
`plugin:<spec>` ([browser](browser.md) › Terms), which the pin list and the tag store keep like
any key; the **Pinned** place and the footer's tag filters list tagged and pinned plug-ins with
the rest. The tags are the user's own named tags with colours
(`library.setTag`, `library.tag`); a plug-in can carry any of them, and filtering by a tag
shows the plug-ins that carry it among the files.
