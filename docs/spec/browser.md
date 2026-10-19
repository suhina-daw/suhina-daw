---
description: How the browser behaves: places, categories, search, the tree, previewing, dragging samples, MIDI files, devices, presets and plug-ins into the project, pins, recent items and colour tags
read_when: browser, library, samples, drag from the browser, drop files, preview, audition, search, folders, pinned, recent, colour tags, track.add, clip.addFiles, library commands, preview.play, MIDI file import, .mid, presets, .syx, .sd, new track, empty area
type: spec
status: draft
last_verified: 2026-10-18
covers:
  - crates/library/**
  - crates/media/src/midi.rs
  - crates/media/src/info.rs
  - crates/media/src/tempo.rs
  - crates/session/src/files.rs
  - crates/session/src/library.rs
  - crates/session/src/preview.rs
  - crates/engine/src/preview.rs
  - crates/ui/src/browser.rs
  - crates/ui/src/browser/browser_view.rs
  - crates/ui/src/interaction/drop.rs
  - crates/ui/src/session/session_view.rs
---

# Browser

The panel at the side of the main window where samples, MIDI files, presets, built-in
instruments and effects and installed plug-ins are found, previewed and dragged into the
project.

- **Sources:** Suhina's own design. The structure is in [`ui-session-view.md`](ui-session-view.md)
  › Window regions and the look in [`docs/design/brief.md`](../design/brief.md) (Browser row) and
  [`components.md`](../design/components.md) › Browser. The decisions below were made with the
  project owner on 2026-10-12; nothing here was observed in another program.
- **Architecture:** previewing is [ADR 0011](../decisions/0011-preview-and-added-audio.md), the
  file index [ADR 0012](../decisions/0012-file-library.md).

## Terms

- **Item:** anything the browser lists. A *file* (sample, MIDI file, preset, project, .sd AudioEffect), a *built-in
  device* (an instrument or an effect) or an installed *plug-in*. An item is named by a key: a
  file by its absolute path, a voice in a preset bank by `<path>#<n>` (1–32), a built-in effect or instrument
  by `device:<id>`, a plug-in by `plugin:<spec>` (the spec `plugins.list` gives).
- **Folder:** a folder the user added to the browser. Only added folders are listed; there are
  none at first.
- **Place:** what the sidebar offers: Pinned, the Recent groups and the folders.
- **Library:** the folders, the index of the files in them, the pins, the recent items and the
  colour tags. It belongs to Suhina, not to a project (like the plug-in library).

## Kinds of files

| Kind | Extensions | Category |
|---|---|---|
| Sample | `.wav` `.aif` `.aiff` `.flac` `.mp3` `.ogg` `.oga` | Samples |
| Stems file | `.stem.mp4` `.stem.m4a` (a master and its stems, [spec](stems-format.md)) | Samples |
| MIDI file | `.mid` `.midi` | MIDI |
| Preset | `.syx` (DX7 voice data: one voice, or a bank of 32; [spec](dx7-voice-format.md)) | Presets |
| Project | `.suhina`, `.als` | All only |
| Device effect | `.sd` (data-only Suhina graph) | All, Effects |

Other files are not listed. Extensions are matched without regard to case.

## Layout

From top to bottom (sizes in components.md › Browser):

1. **Search** with the shortcut hint (Ctrl+F).
2. **Category filters**, a wrapping row of buttons: All, Samples, MIDI, Instruments, Effects,
   Presets, Plug-ins, then **Sample map**, which shows the results as the sample map
   ([`sample-map.md`](sample-map.md)). One is selected; All at first. Choosing a category shows
   the list again; choosing the category already selected shows its whole tree (lets go of the
   place); a category lets go of a recent group's place of another kind, which it would empty.
3. **Sidebar** (left part) and **results** (right part).
4. **Preview strip** for the selected sample or MIDI file.
5. **Footer:** the tags (dots that filter) and the count ("312 items", "3 selected").

The panel is on the left of the main view, or on the right (Settings › Browser ›
Side). Its width is dragged at its inner edge (from 200 px to half the window, default 300) and kept. Ctrl+B and
View › Show browser show and hide it; hidden, it takes no space. Ctrl+F shows it if hidden and
puts the cursor in the search field.

**Keyboard focus:** only the search field (and the fields for a folder's path or a tag's name,
while they show) takes the keyboard. Clicking a row, a filter or a
place does not, so Space, Tab and the other shortcuts keep working (`app.rs::keys_for_shortcuts`).

## Sidebar

Three sections, each title in small hint text:

- **Pinned:** one entry. Selecting it lists the pinned items.
- **Recent:** one entry per group: Projects, Samples, MIDI, Instruments, Effects, Plug-ins,
  Presets. Each lists the last 50 of its kind, newest first. A project counts when it is
  opened or saved; anything else when it is added to a project (dropped, or added with Enter or
  a double-click). Previewing does not count.
- **Folders:** the added folders by name, each with a mark when it is not plain (`…` while
  scanning, `poll` when it is polled, `gone` when missing), then **+ Add folder**: a field where a path is typed (Enter adds it), with a **Browse…** button
  at its end that picks the folder in the system's folder dialog instead.
  A folder dragged from the system onto the window is added too (and the browser shows). A folder's right-click menu
  has Refresh, Pin and Remove from the browser.

At most one place is selected. Clicking the selected place again keeps it; a **×** at the
right of the selected place lets go of it (the whole tree shows again), as does clicking the
selected category. Choosing a recent group (Samples, MIDI, Instruments, Effects, Plug-ins,
Presets) also chooses its category, so the category never hides everything the place lists;
the Pinned place, the recent projects and the folders hold any kind and keep the category.

## Results

**No search text:** a tree. With no place selected its top level is:

- *Instruments* (built-in instruments), *Effects* (built-in effects), *MIDI Effects* (built-in
  MIDI effects, such as [the pitch effect](pitch.md)), *Plug-ins* (installed plug-ins in a folder
  per vendor, *Unknown vendor* last), then each added folder.

With a place selected the tree is that place: the folder's contents, the pinned items or the
recent group's items (a flat list).

**The category filter** narrows what the tree shows: Samples, MIDI and Presets show files of
that kind and the folders that hold any (by the index; a folder not indexed yet is shown);
Instruments and Effects show only those built-in effects and instruments (Effects with the
built-in MIDI effects), `.sd` AudioEffects, and plug-ins with that role
(a plug-in with the *instrument* feature counts as an instrument); Plug-ins shows only
plug-ins. All shows everything.

**Search text:** a flat list of matching items (the tree's place and category still narrow it),
at most 500, from the index of every folder, the built-in effects and instruments, and the plug-ins. An item
matches when every word of the search appears in its name or in its path below its folder,
without regard to case. Exact name matches come first, then names that start with the
first word, then names with a word that starts with it, then the rest; shorter names first,
then by name. Each row shows the folder it is in after the name.

**Rows:** a chevron for what opens, the type icon, the name, the colour tag dots after it and
on the right the duration of a sample (`0:04.2`, read when the row is first shown) or the kind
of a device ("instrument", "effect", the plug-in's format). A folder or a heading opens and
closes with a click on its chevron (every click toggles; the second click of a rapid pair is a
double and is absorbed, so a double-click on the chevron opens it once). A heading
(*Instruments*, *Effects*, *Plug-ins*, a vendor) also opens and closes with a click on its
name, by the same rule. A folder also opens with a double-click on the row itself; a click
elsewhere on it only selects it. A folder's contents are read from disk when it opens, sorted
folders first then by name (natural order: "Kick 2" before "Kick 10"). A preset bank opens
into its 32 voices.

**Selection:** a click selects a row, as soon as the button goes down (so auto-play starts at
once; a row that is part of a selection of several is selected alone when the button comes up,
so pressing it to drag them all keeps the selection); Ctrl+click adds or removes a row; Shift+click selects the
range from the last clicked row. A sample row shows a play button at its left while it is
selected or under the pointer; clicking it previews the sample (a second click stops).

**Double-click or Enter** (in the search field) on an item adds it to the selected track (see
*Adding without dragging*); on a project file it opens the project as a project dropped on the
window does (Settings › General › When opening a project).

**Right-click menu:** Pin or Unpin; Tag ▸ (each tag, ticked when set; a *New tag…* field that
makes a tag in the next default colour and sets it at once — clicks inside the menu, in the
field included, do not close it; a click outside does, and every button closes it by its
action); Show in folder (a file: selects its folder
as the place); Refresh (a folder: lists it again from disk).

## Search field keys

While the search field has the cursor: Up and Down move the selection in the results; Enter
adds the selected item(s) to the selected track; Escape clears the search, a second Escape
gives the keyboard back to the window.

## Preview

The preview strip shows the selected sample or MIDI file: a play button, the name, duration,
tempo (BPM) and key when known, and a waveform (a sample) or a strip of the notes (a MIDI file),
with the part played so far brighter and a playhead.

- **Auto-play** (Settings › Browser › Auto-play, on by default): selecting a sample
  or MIDI file previews it. Off, previewing takes the play button or a click on the waveform.
- **Clicking the waveform** previews from there. The waveform is drawn as
  [waveforms](waveforms.md) says: the whole file across the strip, both channels when there is
  room, the part played brighter.
- A preview plays once, from the start or the clicked point, at the file's own speed. It is
  not synced to the project's tempo or beat. It plays whether the project is playing or not,
  into the main output after the main channel, at the preview volume (Settings ›
  Browser › Preview volume, −6 dB by default).
- Selecting something else, Escape in the search field, the play button again, or dropping
  items from the browser into the project stops it. Dragging does not: a press held still
  counts as the start of a drag, and the preview it started goes on.
- **A MIDI file** previews through the selected track's instrument, a built-in one or a
  plug-in, when that track has one; otherwise through the FM synth with its first voice. All
  of the file's MIDI tracks sound together, at 120 BPM or the file's first tempo.
- A file that cannot be read shows "Cannot preview: <reason>" in the strip and nothing plays.
  The status bar says so too, and says why when nothing can preview at all: there is no audio
  output here, or the active project has no audio output.
- **With no project open** the start panel stands alone and the browser is hidden; it shows
  again once a project is open (#632). The engine can still preview with no project, through an
  output of its own opened at the first preview and closed before a project opens its output
  ([ADR 0011](../decisions/0011-preview-and-added-audio.md) › Previews with no project open).
  Adding to a track needs a project.

The browser is not the only place a preview starts: the piano roll's key column plays a note
through the track's instrument while a key is held, with the same voice and the same commands
([clip editor](ui-clip-editor.md) › Playing the keys, [ADR 0011](../decisions/0011-preview-and-added-audio.md)).

## Tempo and key of a file

Shown in the preview strip and used when a sample is dropped (see *Warping*). Taken from, in
order:

1. **File metadata:** a WAV file's tempo chunk (`acid`) and its root note, ID3 tags `TBPM` and
   `TKEY`, Vorbis and FLAC comments `BPM` / `TEMPO` and `KEY` / `INITIALKEY`.
2. **The file name:** a number from 40 to 300 written next to "bpm" (`kick_128bpm`,
   `BPM 90 loop`) for the tempo; a word such as `Am`, `C#m`, `Fmin`, `Ebmaj`, `G#` or
   `Bb minor` between separators (space, `_`, `-`, `.`, brackets) for the key. A single letter
   with no sharp, flat or mode is not taken as a key.
3. **Tempo detection** (tempo only, when 1 and 2 give none): the file's onsets are analysed
   (at most its first 30 seconds; files of 0.5 s to 2 minutes, and in the preview strip and
   `library.info` only samples up to 60 s). Only tempos from 60 to 200 BPM at which the file is
   exactly 1, 2, 4, 8 or 16 bars of 4/4 are tried, and one is taken only when its beat stands out
   clearly in the onsets (the nearest to 120 BPM among equals), so one-shots and long recordings
   are not given one. Detected tempos are shown with a
   `~` ("~124 BPM").

## Dragging into the project

Anything in the results can be dragged; with several rows selected, all of them move. While
dragging, the target is outlined and a short label beside the pointer says what will happen
("Add 3 clips to Bass", "New audio track", "Add Overdrive to Drums"). Releasing over anything
else does nothing. Escape cancels a drag.

### Files: samples and MIDI files

A sample becomes an audio clip; a MIDI file becomes one note clip per MIDI track in it that has
notes (in the file's order; a type 0 file gives one clip). The clip is named after the file (and
the MIDI track's name, when it has one: "groove – Hats").

**Several files** (and a MIDI file's several clips) go onto **one track**, one after another:
in the Arrangement each starts where the one before ends; in the Session they go down the
track's slots from the slot they were dropped on. With **Ctrl** held when releasing, they go
onto **one track each**, side by side: in the Arrangement all at the dropped beat, in the
Session all in the dropped scene, the first on the track it was dropped on and the others on
new tracks after it. Settings › General › Dropping files › Several files flips this (one track each
without Ctrl).

| Dropped on | Result |
|---|---|
| An Arrangement lane | Clips from the beat under the pointer, on the grid the control bar's value sets at that zoom ([arrangement editing](arrangement-editing.md) › The grid; Alt or `none`: no snap). The clips replace what they cover, unless the stacking settings allow it ([arrangement editing](arrangement-editing.md)). |
| A Session slot (empty or not) | Clips into that slot and the ones below it; a clip already there is replaced. Scenes are added at the bottom when there are more clips than slots. |
| The empty area | New tracks, see below. |

**A track takes the files it can play:** samples go to an audio track, MIDI files to a note
track. Files the dropped-on track cannot take go onto a new track of their kind inserted after
it (one track for all of them, or one each with Ctrl).

**The empty area** is the space right of the last track in the Session (above the scenes, or
in the scene row dropped on) and the space under the last lane in the Arrangement (at the
dropped beat). Dropping there makes new tracks at the end of the regular tracks (before the
returns): one audio track for the samples and one note track for the MIDI files, or with Ctrl
one track per clip. A new track made for one file is named after it; otherwise "Audio <n>" or
"MIDI <n>".

**The empty area shows two buttons**, **+ MIDI track** and **+ Audio track** (at its top in the
Session, at its left in the Arrangement), which add an empty track of that kind. A drop on
either works like a drop on the empty area.

**Warping.** A sample whose tempo is known (see *Tempo and key*) is warped: it plays at the
project's tempo, stretched with the stretch mode in Settings › General › Warp mode (Rhythmic by
default, the `rhythmic` stretch mode), and its length in beats comes from its own tempo. A
sample without a known tempo is not warped: it plays at its own speed, and its length in beats
is its duration at the project's tempo when it is dropped.

**Suhina: in a slot, every sample loops.** A sample (or a Stems file's part) dropped into a clip
slot loops over its whole length, whether its tempo is known or not, like a MIDI file's clip does;
on the Arrangement it plays once. A sample with no known tempo keeps its own speed, so its loop is
the whole file: launched, it plays to the end of the file and starts again from its beginning. The
rule is "always", not "only when longer than about a bar", because a slot is where loops live and
because a length in beats depends on the project's tempo: a threshold would loop the same file at
one tempo and play it once at another. A one-shot (a hit, a crash) is made by switching its loop
off (`clip.setLoop` with `loop: false`; [clip launching](clip-launching.md) › Loop switch).

A MIDI file's notes keep their positions in beats (its tempo does not change the project's).
A clip's length is the end of its last note rounded up to a whole bar (at least one bar).

**A Stems file** becomes a group named after it, holding a track for the master (muted, so
nothing sounds twice; unmute it to compare or to play the track whole) and one track per stem,
named and coloured from the file. Each track gets a clip of its part from the dropped beat or
in the dropped scene, all with the master's tempo, so the parts stay locked together. The file's
mastering compressor and limiter go on the group, which runs them over the sum of the stems
([ADR 0004](../decisions/0004-built-in-devices.md) › 10); switched off when the file has them
off. Dropped on a group, the new group goes inside it, after its tracks; dropped on a track,
after that track; elsewhere, after the regular tracks. Ctrl does not change this. In the
Arrangement view the group arrives folded, one lane until its triangle is clicked
([arrangement editing](arrangement-editing.md) › Folded groups). Previewing a Stems file plays its
master.

**Unreadable files** (missing, damaged, too long, no notes) are left out; the others are still
placed, and the status bar says which were skipped and why. When none can be placed nothing
changes.

**One undo step:** everything one drop does (new tracks, new scenes, clips) is undone together.

### Files from the system

Samples, Stems files and MIDI files dragged from the system's file manager onto the window
are a drag into the project like one from the browser, with the same targets, label, Ctrl and
results (`clip.addFiles`, one undo step; files that cannot be read are `skipped`). While they
hover the window, the target under the pointer is outlined and the label says what a drop does.

Where the pointer is during the drag depends on the system: some report no pointer position
while files are dragged over a window. Then nothing is outlined, and a drop goes where the
pointer was when it was dropped; when that is not known either, the files go where Enter puts
them (*Adding without dragging*). A drop outside the views (on the browser, the bars) says in
the status bar where files can go. Without an open project nothing is added.

Folders, projects (`.suhina`, `.als`) and themes (`.json`, `.zip`) dropped on the window are
not a drag into the project: a folder joins the browser (*Sidebar*), a project opens and a theme
is added. Other files are an error in the status bar.

### Built-in effects and instruments, presets and plug-ins

| Dropped on | Result |
|---|---|
| A track (its header, an Arrangement lane, a Session slot, the detail area showing its chain) | The device is added to that track's chain: an instrument first, an effect last. An instrument on an audio track, or a second instrument on a track, is refused with a message. |
| The empty area | A new track with the device: a note track for an instrument, an audio track for an effect, named after the device. |

A **preset** is its device with the preset's values: a DX7 voice is the FM synth playing that
voice (a bank adds its first voice; a voice row inside the bank adds that voice). Several
devices dropped together are added in the order they were selected.

### Adding without dragging

Enter in the search field or a double-click on an item adds it to the selected track:
a device to its chain; samples and MIDI files in the Arrangement at the play start marker, in
the Session from the first empty slot down. With no track selected, a new track is made as for
the empty area.

## Colour tags

Tags have a name and a colour. There are seven at first: Red, Orange, Yellow, Green, Blue,
Purple and Grey; a right-click on a tag's dot in the footer renames it (a field, Enter; clicks
inside that menu do not close it),
recolours it (the swatches of the track colour palette, [components](../design/components.md#colour-palette), or a colour button for any colour) or removes it; new ones are added from a row's Tag menu, up to 64
(Settings › Browser › Tag limit raises it, at most 1024). Any item can carry any
number of tags; a tag is a 6 px dot after the name. Hovering a dot — the footer's or an item
row's — says the tag's name: as a tooltip, and in the status bar (the footer's dot adds how
many items carry it). Clicking a tag's dot in the footer shows
only items with that tag (several: items with any of them); clicking it again stops.

The **tag editor** is all of that in one place: the footer's **Edit tags** button opens a
popup right above itself (its bottom at the footer's top, its right edge at the button's),
floating over the preview strip and the list's bottom. One row per tag: its
checkbox — with items selected, ticking it tags the selection and unticking takes the tag
off every selected item (ticked while every one of them carries it); with nothing selected,
it filters the results by the tag —, its colour button, its name (typed in
the field, Enter renames it), how many items carry it (hidden when the popup is narrow), and
**Remove** (the first click arms it —
the button asks "Remove Red?" with a **Keep** beside it; the second click removes the tag from
the list and from every item carrying it). The last row adds a tag: a name and Enter, with the
next default colour. With
the editor open the footer's dots make way for it — its rows are the tag list; a click
anywhere outside the popup (the list, the sidebar) closes it, as does the Edit tags button.
The
editor's open state is the window's own and is not kept between runs.

## Where it is kept

`browser.json` in Suhina's settings folder (beside `settings.txt`): the folders, the pins,
the recent items by group, the tags and which items carry them, and the tag limit. A missing
file is an empty library. A file that cannot be read, or parts of it (an unknown group, a tag
without a name, a colour that is not `#RRGGBB`), are skipped with a warning; lists longer
than their limits are cut. The file index is kept beside it (ADR 0012). Panel width, side,
whether it is shown, auto-play, preview volume, how several files are dropped and the warp mode
are in `settings.txt`.

## Commands

The browser's state and everything a drop does are commands (AGENTS.md rule 6), so agents and
the CLI can do the same.

**Library** (on the open projects, like `plugins.*`; none touches a project):

| Command | Params | Does |
|---|---|---|
| `library.folders` | | the folders with their state (`watching`, `polling`, `scanning`, `missing`, `idle`) and file counts |
| `library.addFolder` | `folder` | adds a folder and indexes it |
| `library.removeFolder` | `folder` | removes it and its part of the index |
| `library.refresh` | `folder?` | reads a folder (or every folder) again |
| `library.list` | `folder` | what is directly in a folder: sub-folders and files of the kinds above, with kind and size |
| `library.search` | `text?`, `category?`, `place?`, `tags?`, `limit?` | matching items as in *Results* (`limit` at most 500) |
| `library.info` | `item` | a file's kind, size, duration, sample rate, channels, tempo (and where it came from) and key; a Stems file's stems and mastering; a MIDI file's tracks with their notes; a preset's voices; a built-in device's or plug-in's role (and a plug-in's vendor, format and version); the item's tags |
| `library.pinned` | | the pinned items |
| `library.pin`, `library.unpin` | `item` | pins or unpins an item |
| `library.recent` | `group?` | the recent items of one group or all |
| `library.tags` | | the tags (name, colour) and the limit |
| `library.setTag` | `name?`, `color?`, `rename?`, `limit?` | adds a tag, or changes one's colour or name; `limit` sets the most tags there may be |
| `library.removeTag` | `name` | removes a tag from the list and every item |
| `library.tag` | `item`, `tag`, `on` | sets or clears a tag on an item |
| `library.setPluginsView` | `view` | which plug-ins the Plug-ins category lists: `instruments` (default) or `effects` ([plug-in browsing](plugin-browsing.md)) |
| `library.findFiles` | `names`, `folder?` | finds files by name for missing files: in the folders' index and the active project's folder, or by walking `folder` ([missing files](missing-files.md)) |

**Project** (on the active project, one undo step each):

| Command | Params | Does |
|---|---|---|
| `track.add` | `kind?` (`audio`, `midi`), `name?`, `after?`, `device?`, `sd?`, `preset?`, `voice?` | adds a track with its mixer channel, clip slots for every scene and an Arrangement lane, after track `after` or at the end of the regular tracks; with `device` (a built-in id or a plug-in from `plugins.list`), `sd` or `preset`, the device too, and `kind` follows from it; `.sd` and built-in instruments and MIDI effects create MIDI tracks |
| `clip.addFiles` | `paths`, `track?`, `start?`, `scene?`, `separate?`, `stack?`, `warp?` | places samples, MIDI files and Stems files as in *Files*: on `track` (or new tracks when it is missing; a Stems file's group goes into `track` when it is a group), from beat `start` in the Arrangement or from `scene` (1-based) in the Session (exactly one of the two); `separate` gives each clip a track; `warp` is the stretch mode for samples with a known tempo (`rhythmic` by default); returns the clips, the new tracks, the Stems groups (`groups`) and what was `skipped` |
| `device.add` | adds `preset?` and `voice?` (1–32) | the built-in effect or instrument with a preset's values |

**Preview** (Transport kind, on the active project's player): `preview.play {path, from?, track?, gain?}`
previews a sample from `from` seconds at `gain` dB (−6 by default), or a MIDI file through `track`'s instrument (the FM synth
when it has none); `preview.stop`. `launcher.state` reports `preview` (`playing`, `seconds`).

Over MCP, every path a command reads (`paths`, `path`, `preset`, `folder`, a file `item`) must
be inside the server's read root, and `library.addFolder` and `library.removeFolder` are refused:
the user chooses the browser's folders ([`mcp.md`](../mcp.md)).

## Limits

| What | Limit | Beyond it |
|---|---|---|
| Folders | 256 | refused |
| Files indexed per folder | 200 000 | the rest are left out; the folder says so |
| Entries listed in one folder (`library.list`) | 10 000 | the rest are not listed |
| Words in a search | 16 | the rest are ignored |
| Folder depth | 32 | deeper folders are left out |
| Symbolic links | not followed | |
| A file name | 1024 bytes | left out |
| Files in one drop | 256 | the rest are left out |
| A sample | 2 hours (as when a project loads) | skipped with a message |
| A MIDI file | 16 MiB, 64 MIDI tracks, 1 000 000 notes | skipped with a message |
| A preset file | 64 KiB | skipped with a message |
| Pins | 1 000 | the oldest go |
| Recent items | 50 per group | the oldest go |
| Tags | 64 by default, at most 1024 | refused |
| `browser.json` | 8 MiB | not read; an empty library, with a warning |

## Not decided yet

- Loading the shared JSON preset format for built-in and `.sd` devices, and saving user presets;
  the format and `.sd` compatibility rules are decided in [ADR 0014](../decisions/0014-stock-presets.md),
  but the loader and browser entries are not implemented. An `.sd` preset applies to an existing
  device with a matching id and role; it does not install or create the graph.
- Presets of plug-ins (CLAP preset discovery, VST3 presets).
- Previewing in time with the project, and hot-swapping a device or sample by double-click.
- Key detection by analysis.
- A system file dialog for adding a folder (today a path is typed or a folder dropped).
