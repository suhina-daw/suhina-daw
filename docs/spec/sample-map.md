---
description: How the sample map behaves: samples laid out by how they sound, coloured by kind, as a cloud or a grid, previewed by pointing, dragged into the project; NKS preset previews in the same map
read_when: sample map, similarity, sound map, browse by sound, t-SNE, UMAP, point cloud, grid layout, similar samples, kick snare hat categories, audio features, MFCC, NKS, preset previews, soundmap commands
type: spec
status: draft
last_verified: 2026-10-18
covers:
  - crates/soundmap/**
  - crates/session/src/soundmap.rs
  - crates/ui/src/browser/sample_map.rs
  - crates/ui/src/browser/sample_map_crowd.rs
  - crates/ui/src/browser/sample_map_panel.rs
---

# Sample map

A view of the browser that places samples on a plane by how they sound: samples that sound
alike lie close together, kicks gather in one region and hats in another. Pointing at a sample
plays it, so a library is explored by moving the mouse instead of reading names. Instrument
presets that come with audio previews (NKS) take part in the same map.

- **Sources:** the maintainer's request (2026-10-12) with two screenshots of commercial drum
  plug-ins that have such a view; public papers and open-source projects listed under
  *Prior art*. The screenshots were used for structure and behaviour only (what the view shows
  and how it reacts); the look follows [`docs/design/brief.md`](../design/brief.md). Nothing
  was run, measured or taken from those products.
- **Builds on:** the browser ([`browser.md`](browser.md)), its file library
  ([ADR 0012](../decisions/0012-file-library.md)) and previewing
  ([ADR 0011](../decisions/0011-preview-and-added-audio.md)).
- **Architecture:** [ADR 0013](../decisions/0013-sample-map-analysis.md): the analysis crate,
  the features, the layout methods and the cache.
- **Status:** built, except NKS presets: the analysis, layout, grid, kinds and caches
  (`crates/soundmap`), the `soundmap.*` commands (`crates/session/src/soundmap.rs`), the map
  view (`crates/ui/src/browser/sample_map.rs`) and its place in the browser and the main area
  (`crates/ui/src/browser/sample_map_panel.rs`). NKS presets wait for the format to be checked on our
  own files ([`nks-format.md`](nks-format.md) › To observe). Statements marked **(to decide)**
  are open.

## What was observed

Two existing products show the idea in two forms. In our own words:

1. **A free cloud.** Every sample is a small dot; the dots form a cloud in which similar
   sounds are near each other. The dot's colour says what kind of sound it is (kick, snare and
   clap, hats and so on); colours blend where the kinds meet. The samples the instrument is
   using are drawn as larger dots in the colour of the slot that uses them. A search field sits
   above the cloud; zoom in, zoom out and fit buttons at a corner; a small overview of the cloud
   beside the slots.
2. **A grid.** The same idea with every dot snapped to a cell of a grid, one sample per cell,
   so no dot hides another and each is easy to hit. The name of each kind (kick, crash, closed
   hi-hat, tom, other) is written in large letters over its region. The map sits beside a file
   browser, drum pads and a step sequencer.

Both play a sample as soon as the pointer is over it.

## Terms

- **Map:** the plane with one **point** per item. An item is a sample file from the library or
  an NKS preset with a preview (see *NKS presets*).
- **Scope:** the items a map holds: those of the place selected in the browser (a folder, Pinned,
  a Recent group), or of every folder when no place is selected.
- **Features:** numbers computed from an item's audio that describe its sound
  ([ADR 0013](../decisions/0013-sample-map-analysis.md) lists them). Two items are **similar**
  when their features are close.
- **Layout:** the position of every point, computed from the features of the scope.
- **Kind:** what sort of sound an item is (kick, snare, hat…): set by hand, from its name, or
  recognised from its sound (*Kinds and colours*).

## Where the map is shown

- **In the browser.** The **Sample map** button at the end of the category row changes the
  results area into the map; a category button shows the list again. The sidebar, the category filters, search, tags and the preview strip stay as they
  are. The browser can be widened to half the window.
- **Large.** The **Large** button at the map's corner (and View › Sample map,
  **Ctrl+Shift+M**) shows the map in the main area in place of the Session or Arrangement view,
  at full size; **Close**, Ctrl+Shift+M, Escape (when Show similar is not on) or the view
  switches go back. The browser stays beside it and shows its list meanwhile. While something is
  dragged from the large map, the main view shows instead, so the drop can land on a track.
- **In a window of its own.** The **Window** button on the map's bar (and View › Sample map in a
  window, **Ctrl+Alt+M**) moves the map into a separate window, which can sit on another screen
  while the arrangement or the Session view keeps the main window. The browser shows its list
  meanwhile, as for the large map. Everything works there as in the browser: pointing previews,
  Show similar, tags, pins, double-click adds to the selected track, and a drag from the map to
  a track, a slot or a device chain in the main window drops there (the map's window has the
  pointer while the button is down; the main window shows the drop label and takes the drop).
  The keymap's keys work in that window too. **Dock**, Ctrl+Alt+M, closing the window, the
  browser's **Sample map** button or the large view put the map back. Whether the window is open, and where
  it is and how big (`map_window`, `map_window_rect` in `settings.txt`: left, top, width and
  height in points on the whole desktop, so a second screen counts), are kept for the next
  start; a place no screen could show is not used.

The map holds the samples of the browser's place, whichever category filter is chosen (the
filters choose what the list shows).

**Suhina:** analysis starts with the desktop app and the MCP server, as indexing does; tests
and the command line never analyse a user's folders unasked.

## Analysis

Before an item can be placed, its audio is analysed. This happens in the background:

- When a folder is added or its index changes (new, changed or removed files), its new and
  changed samples are queued; removed ones leave the cache.
- Analysis uses at most half of the computer's cores, never the audio thread, and the
  results are kept on disk, so a library is analysed once.
- A sample is analysed from its onset (the first moment within 40 dB of its loudest, looked for
  in its first 4 seconds), for 4 seconds; leading silence is skipped. Samples longer than the **map length limit**
  (Settings › Browser › Map: longest sample: 5, 10, 30 or 60 s, 10 by default;
  `soundmap.layout` takes 1–60) are not shown: the map is for one-shots and short loops. The
  length is kept with the analysis, so changing the limit needs no new analysis.
- The map's top bar says what it holds: "1 240 samples, analysing, 6 760 to go, 12 too long,
  3 could not be read". While the analysis goes on, the layout is computed again at most every
  2 seconds, on a thread of its own ("arranging…", then "arranging 42 %" as the layout's
  iterations go by), so the window never waits for it.
- While samples are left to analyse, a card in the middle of the map says **Analysing samples**,
  with a bar and "6 760 of 14 000"; once they are all analysed and a layout takes longer than a
  moment, it says **Arranging the map**, with a bar and the share of the layout's iterations done
  ("Getting the map ready" before the first, and for a layout already kept). The card is only drawn: the map
  under it, its buttons and the rest of the window keep working.
- **The top bar:** the controls in one row (**Cloud | Grid**, **+**, **−**, **Fit**, **Large**,
  **Window**, **Size**, **Volume**), wrapping onto more rows when the map is narrow, never on top
  of each other (a label stays with its field); under them, the status on a row of its own.
- **Volume** (−48 to +6 dB) is the preview level the browser's previews use too (Options ›
  Settings › Browser › Preview volume), kept; moving it also changes the sample playing now
  (`preview.setGain`), gliding there without a click.
- When the status does not fit its row, it is cut short with "…". Its tooltip has all of it and says why samples are missing, one part each:
  still to analyse; too long (the limit, and Options › Settings › Browser › Map: longest sample
  to change it); could not be read, by reason with the commonest first (the reason as the
  decoder gives it, such as "unsupported codec: …", "read error: …", "silent" or "larger than
  256 MiB", its count and the names of the first three files; at most 12 reasons, the rest
  counted under the last); and left out over the 200 000 a map holds.
- A file that cannot be decoded is left out and counted; `soundmap.info` gives the reason.

## Layout

- **Cloud** (the default): points where the layout puts them. Similar items are close; distances
  between far-apart regions mean little. Kinds are found before the layout and the layout is
  given them along with the features, so each kind forms regions of its own and an item of
  one kind does not sit inside another kind's region; items of no kind lie where their sound
  puts them.
- **Grid:** the same layout snapped to a grid of square cells, one item per cell, keeping
  neighbours together as far as possible. The grid is as close to the map's aspect ratio as the
  item count allows, and is made again when the map's shape changes; `soundmap.layout` takes the
  aspect ratio as `aspect`. Snapping a large map takes a while (over a second for 150 000 items),
  so it is made on a thread of its own, only while the grid is shown, and the old grid is drawn
  stretched meanwhile; resizing never waits for it.
- A **Cloud | Grid** switch at the top of the map chooses; the choice is kept.
- The same scope with the same items always opens with the same layout (layouts are kept),
  so a user can learn where things are. Adding or changing files lays the scope out again
  **(to decide:** whether new items are placed into the existing layout instead, until a
  Re-layout button is pressed**)**.
- A large scope takes a while to lay out the first time (about 5 seconds for 10 000 items, a
  minute or more for 200 000); meanwhile the map shows the **Arranging the map** card over the
  previous layout or an empty map. A scope with more than 200 000 items shows the first 200 000 and says so.

## Many items

A library of 200 000 samples must stay readable and smooth (60 frames a second) at every zoom.
Drawing every dot then only hides the dots under it, so the map changes how it draws with the
number of points in view:

- **Density** when more than 20 000 points are in view. The view is divided into cells of 3 by 3
  screen pixels. Each point is spread over the cells around it with a small Gaussian kernel (about
  6 pixels each way), and the raster is drawn with linear filtering, so a cluster reads as a soft
  cloud rather than blocks. Each cell is coloured by the mix of the kinds of the points near it
  (each kind's colour weighted by its count). Its opacity grows with the logarithm of the count,
  fading out towards a cluster's edge and full in the densest cell in view. Clusters then show as coloured
  regions, and where kinds meet, their colours blend. Dimmed points (search, tags, legend,
  Show similar) count only as a faint grey under the others. Points that need to be seen one
  by one are drawn on top as dots: the selected ones, the ones in use, the similar ones, and
  the one under the pointer or last previewed. Pointing, clicking, dragging and the tooltip work
  on the points as before.
- **Dots** when 20 000 points or fewer are in view. The dots grow as the points spread out: the
  radius follows the room each point has on screen, from 1.5 to 6 pixels. A dot that would cover
  its neighbours is drawn translucent, so overlapping dots read as a denser region rather
  than a solid blob.
- The grid layout draws the same way. A cell smaller than 3 pixels is part of the density;
  larger cells are drawn as before.
- **Resizing.** While the map's area changes size (the window, the map's own window, the
  large map), the last density raster is drawn scaled into place, so the points stay where they
  are drawn. It is worked out again once the area has kept one size for 150 ms. In the cloud
  the legend's counts and the clusters are kept, since the points' positions do not depend on
  the view's shape; the grid works them out again, as its cells follow it.

**Labels.** Every cluster of a kind gets a label, not one label for the whole kind. Kinds
that sit in several places are named in each of them.

- A **cluster** is a group of map cells in which the kind holds at least a third of the points,
  joined to its neighbours across sides and corners. The map is divided into a square of cells
  half the square root of the item count a side (4 to 48), so a small map's sparse clusters stay
  in one piece. The cluster's items are its kind's items in those cells.
- A cluster gets a label when it holds at least 8 items (32 for Other) and at least one in
  2 000 of the items in the map, so a map of 200 000 shows at most about a hundred
  candidates.
- The label sits at the cluster's item nearest to the cluster's centre. Its size grows with the
  cluster's size (the logarithm of its count), from the map's label size up to 1.7 times it.
  It is written in the light text colour with a dark shadow, since it sits on its own kind's
  colour.
- **Labels never overlap.** They are placed from the largest cluster down. A label that would
  overlap one already placed is moved up or down by its own height and a small gap. If neither
  place is free, it is left out for this zoom. Zooming in makes room, and smaller clusters appear. A label
  that would fall outside the map area is left out too.
- Labels fade out when zoomed in far enough to read point names, as before.

**Kind toggles.** The legend works as before (click: only that kind; Ctrl+click: several). In
the density view the dimmed kinds are drawn as faint grey, so the shown kinds stand out from
the rest of the library. The legend's counts and the clusters are computed when the points
change, not every frame.

## Kinds and colours

Every item has a kind. The kinds and the words in names that give them:

| Kind | Words (whole words in the file name or its two nearest folders, any case) |
|---|---|
| Kick | kick, kik, kck, bd, bassdrum, bass drum |
| Snare | snare, snr, sd |
| Clap | clap, clp, cp, snap |
| Closed hat | chh, closedhat, closed hat, hhc, hat closed, closed hihat, hihat closed, hh closed |
| Open hat | ohh, openhat, open hat, hho, hat open, open hihat, hihat open, hh open |
| Hat | hat, hihat, hh |
| Rim | rim, rimshot, rs, stick |
| Tom | tom, toms, floor tom |
| Crash | crash |
| Ride | ride |
| Cymbal | cymbal, cym, china, splash |
| Percussion | perc, percussion, shaker, shk, tamb, tambourine, conga, bongo, cowbell, clave, block, triangle |
| Bass | 808, bass, sub |
| Tonal | stab, chord, pluck, keys, synth, lead, pad |
| Vocal | vox, vocal, voice, chant |
| Effect | fx, sfx, riser, impact, sweep, noise |
| Other | anything else |

- Names are split into words at spaces, punctuation, changes of case and between letters and
  digits (`KickHard_02` → kick, hard, 02); a word with a trailing *s* also counts as the word
  without it (`Kicks`, `Hats`) when what is left has at least three letters and the word is not
  itself in the table. Two words in a row (`closed hat`, `bass drum`) count before one word
  alone. These words also count at the start of a longer word (`Kickdrum`, `Snareroll`,
  `Bassline`): kick, snare, clap, hihat, crash, cymbal, shaker, conga, bongo, cowbell, bass,
  synth, chord, pluck, vocal, voice, riser, sweep, impact.
- **The most specific kind in a name wins**, wherever it is in the name: a drum (kick, snare,
  clap, closed or open hat, rim, tom, crash, ride) over Hat and Cymbal, those over Percussion,
  then Vocal, Bass, Tonal and Effect (`808 Kick` is a kick, `Snare FX` a snare, `Vocal Chop FX`
  a vocal). Between two equally specific kinds, the first in the name wins.
- **The file name is read first, then its two nearest folders** below the library folder; a
  folder further up is usually a pack's or a collection's name, not a kind. In a folder's name,
  "bass" next to a word of a style of music (house, music, future, drum, dnb, uk, and, n,
  jungle, trap: `Bass House Vol 2`, `Drum and Bass`) names no kind.
- **Kinds from the sound.** Before the layout, the map learns what each kind sounds like from
  the scope's own items whose file name (or the user) gives their kind; no model is shipped. A
  kind with fewer than 4 such items learns from items whose folder names it as well. Each
  learned kind is a few centres of its examples in feature space. Then, for every item:
  - a kind **set by hand** wins over everything;
  - a kind from the **file name** stands, whatever the sound;
  - a kind from a **folder** stands, unless the item clearly sounds like another kind: that
    kind is at least 1.4 times nearer, and the item is within reach of it (below);
  - an item **without a kind** takes the kind it sounds like, when that kind is within reach and
    every kind of another family is at least 1.15 times further away; otherwise it is Other.

  Within reach means no further from the kind's centres than 2.5 times the distance most (nine
  in ten) of its own examples are. Closed hat, Open hat and Hat are one family, Crash, Ride and
  Cymbal another, Snare and Rim a third: telling relatives apart never overrules a folder or
  makes a guess unsure. A kind from the sound is drawn a little paler, and the item's tooltip
  says "guessed".
- **Set by hand.** Right-click a point, **Kind ▸**, and choose a kind; **Automatic** gives the
  item back to its name and sound. The kind shows at once, the scope is laid out again in the
  background so the item moves to its kind's region, and the tooltip says "set by hand". It is
  kept beside the feature caches (`kinds.txt`, by the file's path), survives analysing the file
  again and every new layout, and teaches the map that kind like a file name does.
- Each kind has a colour from Suhina's own palette (the design brief's track colours, extended),
  chosen so that neighbouring kinds (kick and bass, snare and clap, the hats) are told apart.
- **Labels.** Each cluster of a kind is named over its region, without overlapping another
  label: *Many items* › Labels. Labels fade out when zoomed in far enough to read point names.
- **Legend:** a row under the map with each kind, its colour and its count. Clicking a kind dims
  all others (Ctrl+click: several); clicking it again stops.

## Points

- A point is a dot (cloud) or a filled cell (grid) in its kind's colour.
- **Size** in the map's top bar (25 to 400 %, kept) makes the dots smaller or larger; their
  places do not change, so smaller dots stand further apart. In the density view it sets how far
  each point is spread: below 75 % not at all, so neighbouring points stand apart, and further
  the larger the size.
- **In use:** items used by a clip of the active project are drawn larger with a ring.
- **Selected:** items selected in the browser have a light outline; the map and the list share
  one selection.
- **Previewed:** the item last previewed, and the one under the pointer, have an accent ring.
- **Similar:** while Show similar is on, the similar items have an accent ring and the others
  are dimmed.
- **Dimmed:** items that do not match the search text, the tag filter or the legend are drawn
  faint but stay in place, so the map does not jump while typing.
- **Tooltip** over a point: the file name, the kind ("guessed" or "set by hand" when it is), and
  its folder.

## Pointing, clicking and moving

| Input | Does |
|---|---|
| Pointer over a point | Highlights it and **previews it** (Settings › Browser › Map preview on hover, on by default). Moving across points plays each in turn; a new one stops the one before. A point is played only after the pointer has rested on it for 40 ms, so a fast sweep does not play everything it crosses. |
| Pointer resting on a point | Its tooltip shows the name, the kind, the folder and, after 0.15 s, the file's waveform, read in the background ([waveforms](waveforms.md)). |
| Click a point | Selects it and previews it (also with preview on hover off) as the button goes down, as in the list. |
| Ctrl+click, Shift+drag on empty space | Adds to or removes from the selection; Shift+drag draws a rectangle and selects what is in it. |
| Drag a point | Drags the selection into the project, exactly as from the list (drop targets, labels, Ctrl for one track each): [`browser.md`](browser.md) › Dragging into the project. The drag carries the point the button went down on (with the selection when it is part of it), not the one under the pointer when the drag begins a few pixels later; while it goes on, points are not highlighted or previewed and the view does not move. |
| Double-click a point | Adds it to the selected track, as in the list. |
| Drag empty space, middle-drag | Moves the view. |
| Mouse wheel | Zooms around the pointer (Ctrl+wheel is not needed). |
| Double-click empty space, the Fit button | Fits all points in view. |
| Arrow keys | Moves the selection to the nearest point in that direction and previews it; with Show similar on, Up and Down step through the similar list instead. |
| Right-click a point | Menu: **Show similar**, Pin or Unpin, Tag ▸, Kind ▸ (*Kinds and colours* › Set by hand), Show in folder (switches to the list with the item selected). |

**Show similar** rings the 16 items nearest to the clicked one by features (not by position on
the map, which distorts distances), dims the rest and lists them in order in a strip at the map's
side, each with a play button; Escape or clicking empty space ends it. Arrow keys Up and Down then
step through the list, starting from the item it was shown for, and preview each; Left and Right
still move to the nearest point on the map.

The previews are the browser's own (`preview.play`): same volume, same output, stopped by the
same things.

## NKS presets

NKS is a format in which instrument presets (`.nksf`) and effect presets (`.nksfx`) are kept as
files that name the plug-in they belong to, carry its state and metadata (name, author, bank,
sound types), and often have a short audio **preview** recorded from the preset: in a hidden
`.previews` folder in the same folder, named after the whole preset file (`Bass.nksf` →
`.previews/Bass.nksf.ogg`). The format is described from public sources and our own files in
[`nks-format.md`](nks-format.md).

- **Listed in the browser:** `.nksf` and `.nksfx` files in an added folder are presets
  (category Presets), with the plug-in they need, the bank and the types from their metadata.
  Folders are found the way the browser finds any folder: the user adds them (the usual place
  for user and third-party content is `Documents\Native Instruments\User Content`). Databases
  and registry entries of other programs are not read.
- **Naming:** the browser and the docs say "Native Instruments preset files (.nksf)" where a
  name is needed; Suhina does not use the NKS logo or call itself NKS-compatible.
- **In the map:** a preset with a preview is an item of the map; its features come from the
  preview. Its kind comes from its sound type metadata when there is one, then from its name as
  above. Presets are drawn as rings rather than dots, so they are told from samples.
- **Previewing** plays the preview file, as a sample. A preset without a preview is listed but
  not in the map.
- **Dragging** a preset into the project adds the plug-in it names with the preset's state, as a
  plug-in from the browser: on a track's chain, or a new track in the empty area. When the plug-in
  is not installed, or is not a format Suhina hosts, the drop is refused with a message naming
  the plug-in.

How much of this works with a given library is open until the format is checked on real files;
see *To find out*.

## Commands

Everything the map does is a command (AGENTS.md rule 6):

| Command | Params | Does |
|---|---|---|
| `soundmap.status` | | analysis progress per folder: analysed, queued, failed, skipped as too long |
| `soundmap.analyse` | `folder?` | queues a folder (or all) for analysis again, ignoring the cache |
| `soundmap.layout` | `place?`, `text?`, `tags?`, `grid?`, `aspect?`, `maxSeconds?` | the scope's items with their position (0–1 in both axes), kind, whether the kind was guessed from the sound (`guessed`) or set by hand (`byHand`) and whether they match `text`/`tags`; leaves out samples longer than `maxSeconds` (1–60, default 10); with `grid`, each item's cell in a grid whose width over height is close to `aspect` (0.1–10, default 1); computes the layout when it is not cached |
| `soundmap.similar` | `item`, `count?`, `place?` | the `count` (default 16, at most 100) items most similar to `item` in its scope (`place`), nearest first, with their distance |
| `soundmap.info` | `item` | an item's kind and where it comes from (`kindFrom`: `hand`, `name`, `guess` or `none`; a guess is made among the items of the item's library folder), its features and when it was analysed |
| `soundmap.setKind` | `item`, `kind` | sets the item's kind by hand (a kind id, as `soundmap.layout` gives them), or `auto` to give it back to its name and sound |

Over MCP, `item`, `place` and `folder` paths must be inside the server's read roots
([`mcp.md`](../mcp.md)).

## Limits

| What | Limit | Beyond it |
|---|---|---|
| Sample length in the map | 10 s by default, 1–60 s | not analysed, not shown |
| Audio analysed per item | 4 s from the onset, the onset within the first 4 s | the rest is not read; a file silent for its first 4 s is not analysed |
| Kinds set by hand | 1 000 000 | refused |
| A file to analyse | 256 MiB | skipped with a reason |
| Items in one map | 200 000 | the rest left out; the map says so |
| Points drawn one by one | 20 000 in view | the density view (*Many items*) |
| Label candidates | clusters of 8 or more items and of 1 in 2 000 of the map | smaller clusters get no label |
| Grid part assigned exactly | 256 items | larger grids split first (ADR 0013) |
| `count` in `soundmap.similar` | 100 | cut |
| Feature cache per folder | 64 MiB | not read; the folder is analysed again |

## Prior art

Studied for the method; no code is taken from them unless its licence allows and ADR 0013 says
so.

- **AudioStellar** (GPL-3.0-or-later, C++): a folder of samples analysed (spectrum, MFCC),
  reduced (PCA, t-SNE or UMAP), clustered, shown as a coloured cloud played by pointing. The
  nearest open equivalent of this view.
- **FluCoMa** (BSD-3-Clause, C++): corpus maps from MFCC, spectral shape, loudness and pitch
  statistics, standardised, reduced with UMAP or MDS, a k-d tree for neighbours, and a grid
  object that snaps a 2D layout to a grid.
- **Freesound Explorer** (MIT, JavaScript): t-SNE maps of sounds by spectral descriptors with
  hover to play.
- **Infinite Drum Machine** (Apache-2.0, JavaScript) and its analysis notebooks (MIT): t-SNE of
  small spectrograms of one-shots.
- **RasterFairy** (BSD-3-Clause, Python): snapping a 2D cloud to a grid by recursive splitting.
- Papers: Pampalk, Hlavac and Herrera, *Hierarchical Organization and Visualization of Drum
  Sample Libraries* (DAFx 2004); Turquois et al., *Exploring the Benefits of 2D Visualizations
  for Drum Samples Retrieval* (CHIIR 2016); Shier et al., *Manifold Learning Methods for
  Visualization and Browsing of Drum Machine Samples* (JAES 2021); Richan and Rouat, *A
  proposal and evaluation of new timbre visualisation methods for audio sample browsers* (2020);
  Fried et al., *IsoMatch* (2015) and Barthel et al., *Improved Evaluation and Generation of
  Grid Layouts using Distance Preservation Quality and Linear Assignment Sorting* (2022) for
  grids.

Repositories without a licence (ml4a's audio t-SNE viewer, the code of Shier et al., IsoMatch)
gave ideas only, through their papers and descriptions; no code was copied, ported or
translated from them (CONTRIBUTING.md › Clean-room).

## To find out

- The NKS questions listed in [`nks-format.md`](nks-format.md) › To observe, above all whether a
  preset's state loads into the VST3 version of its plug-in.
- Whether the kinds learned from a library's own names are good enough on real libraries, or a
  small classifier trained on openly licensed one-shots (CC-BY or CC0 only) is needed. The
  generated test sounds (kicks, snares, claps, hats, tambourines, shakers, toms and basses, with
  leading silence, room tails and level changes) all get their kind; a tom with a long room
  tail 12 dB down sounds between a tom and a bass and is left Other.
- How the layout feels with real libraries of 1 000, 10 000 and 200 000 samples: perplexity,
  the 4-second window and the feature weights are set by trying them. With 200 000 items, also
  whether t-SNE separates the kinds well enough or a UMAP-style layout is needed
  ([ADR 0013](../decisions/0013-sample-map-analysis.md) › Alternatives).

## Not decided yet

- Loops: tempo-synced preview of loops in the map, and a layout for longer files.
- Placing new items into an existing layout instead of computing it again.
- Building a drum kit from a selection on the map.
