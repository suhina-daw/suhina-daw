---
description: Measurements, states and tokens of every UI component, from the minimal v3 mockups
read_when: widget sizes, marker row, clip slot states, value field, slider, knob, toggle, meter, track header levels, device panel, piano roll layout, audition button, velocity lane top edge, ghost menu, ghost notes look, Scale menu, scale rows, key column mark, folded rows
type: spec
status: draft
last_verified: 2026-10-25
covers:
  - crates/ui/src/kit/theme.rs
  - crates/ui/src/session/session_view.rs
  - crates/ui/src/session/mixer_strip.rs
  - crates/ui/src/interaction/colour_picker.rs
  - crates/ui/tests/track_colour.rs
  - crates/ui/src/settings/view_settings.rs
  - crates/ui/tests/open_recent.rs
  - crates/ui/src/kit/widgets.rs
  - crates/ui/src/arrangement/arrangement_view.rs
  - crates/ui/src/arrangement/timeline.rs
  - crates/ui/src/arrangement/automation_lanes.rs
  - crates/ui/src/kit/motion.rs
  - crates/ui/src/kit/scroll.rs
  - crates/ui/src/settings/settings_view.rs
  - crates/ui/src/settings/themes.rs
  - crates/ui/src/kit/meter.rs
  - crates/ui/src/kit/waveform.rs
  - crates/ui/src/detail/detail_view.rs
  - crates/ui/src/detail/follow_view.rs
  - crates/ui/src/app/project_dialog.rs
  - crates/ui/src/app/start_panel.rs
  - crates/ui/src/interaction/pick.rs
  - crates/ui/src/browser/browser_view.rs
  - crates/ui/src/app/loading_dialog.rs
  - crates/ui/src/app/missing_files.rs
  - crates/ui/src/app/splash.rs
  - crates/ui/src/kit/logo.rs
  - assets/brand/**
---

# Components

Measurements, states and tokens of every UI component, taken from the minimal v3 mockups
([Session](reference/session-minimal-v3.png), [Arrangement](reference/arrangement-minimal-v3.png)).
Token names are defined in [`brief.md`](brief.md) › Tokens. Sizes are logical pixels at 100 %
scale. Rows marked **(proposed)** were not drawn in the mockups yet; use them until a design
replaces them.

## Global rules

- Radius 0 everywhere. No shadows, gradients or cards.
- Regions are separated by 1 px gaps that show `border` beneath them.
- **One frame after an edit.** The window draws a view from the state at the start of the frame, and
  the commands of a click, a drag that ended and a drop run after it: when a command changed the
  project, the window asks for one more frame, so what the user did is on screen at once instead of
  waiting for another click or pointer move.
- Parts marked **(planned)** are designed but not built yet; everything else is what the app
  draws today.
- Type, by role:
  - 11 px `Encode Sans Semi Expanded` for UI text (names, labels of controls, menu items);
  - 10 px for small values and labels, and every number, time and position in `IBM Plex Mono`
    (bar numbers in the ruler too);
  - 9 px only for tags beside a name: a device's kind, a clip's gain, key labels in the clip
    editor;
  - 13 px only for a page title (Settings), the drop overlay and the name on the splash.
- Text colours: `text.primary` for values and names, `text.secondary` for labels,
  `text.hint` for inactive and hints. The one pure white (`#FFFFFF`) is the name of a playing
  or recording clip.
- Fonts live in `crates/ui/fonts/<font>/` with their licence and a row in `ATTRIBUTION.md`.

### Interaction states

| State | Change |
|---|---|
| Hover | Background one step lighter: `window.background` → `panel.background`, `panel.background` → `#2A2926`, `control.background` → `#34332F` |
| Pressed / dragging | `control.activeBackground` |
| Selected | A track header: `control.background` in the Arrangement view, `control.activeBackground` in the Session view; a clip: `control.activeBackground` with a 1 px `text.primary` inset |
| Keyboard focus | (planned) 1 px inset outline in `accent` |
| Disabled | Text `text.hint`, no fills |
| Track off (muted) | Its name in `text.hint`; its slots, clips, marks and colour strip at 55 % opacity |

### Motion

Animations are short and only show a change of state; with animations off (Settings ›
Appearance › Animations, or `--no-animations` for one run) every value jumps straight to its end.

| What | How |
|---|---|
| Hover | Fill and text fade to the hover colour in 80 ms |
| States (playing scene, transport running, track on) | Fade in 120 ms |
| Queued clip | The accent outline breathes between 45 % and 100 % every 0.8 s |
| Values changed from elsewhere (undo, another client) | The fill eases to the new value in 120 ms; a drag follows the pointer directly |
| View switch | The underline slides to the chosen view in 160 ms |

## Window

From top to bottom: menu bar, tab row (when shown), control bar, the open prompt or the missing files bar (when shown, [missing files](../spec/missing-files.md)),
the view with the detail area, status bar.

| Region | Size |
|---|---|
| Menu bar | 20 px, `panel.background`, items 14 px apart in `text.secondary`: File (New or open…, Open…, Open recent › the recent projects newest first, up to 10, by name with the path as the tooltip, each opened as Open… opens a file, greyed out while there are none, a file that is gone saying so in the status bar; Save, Save as…, Save as template…, Quit), Edit, View, Projects, Options, Help (About Suhina, [spec](../spec/about.md)); Create, Playback and Navigate (planned) |
| Tab row | 22 px, `window.background`, shown while two or more projects are open (ADR 0007): one tab per project, name (with " •" while it has unsaved changes, as the window title has) and a 12 px × to close; the active tab `panel.background` with `text.primary`, others `text.secondary`; Ctrl+Tab and Ctrl+Shift+Tab move between them |
| Control bar | 28 px, `panel.background`, a 1 px `border` under it. Left: the browser's show/hide button (20 × 20, `control.background`, a 14 × 11 panel outline whose left part is filled in `text.primary` while the browser shows; "Hide browser" / "Show browser", the same as Ctrl+B), 4 px on, then 6 px apart: tempo (56 × 20 value field, `control.background`, 11 px mono right-aligned, dragged up or down), time signature (40 px, 11 px mono `text.secondary`, "4 / 4"), the grid and launch quantisation (64 px dropdown, 11 px mono, its list tall enough for every value; the tooltip says it also sets the Arrangement grid). Centre: play and stop (28 × 20 each, 1 px apart), the position (64 × 20 on `window.background`, 11 px mono, "bar.beat.sixteenth", bars through the time-signature changes). Right: the CPU load (56 × 20 on `window.background`, only while an engine plays: "CPU" in 9 px `text.hint` left and the smoothed percentage in 10 px mono `text.secondary` right, over a 2 px-tall bar of the load, `meter.normal` fill, `meter.hot` from 80 %, in a `control.background` track; the audio thread's share of wall time inside the engine's render call, per mille smoothed over 0.2 s windows, published as an atomic), then the view switch at the right edge, 6 px from it. Left of the CPU load, 6 px apart, the RAM (76 × 20 on `window.background`, always shown: "RAM" in 9 px `text.hint` left, the program's resident memory in 10 px mono `text.secondary` right, "512 MB", "2.3 GB"). Right, left to right: RAM, CPU load, view switch; a block that would come within 6 px of the position is left out, the RAM first. (planned): metronome, record, loop |
| Transport button | 28 × 20, `control.background`; play shows `control.activeBackground` while playing, its glyph in `accent`; stop is an 8 × 8 square in `text.primary`; record (planned) a 1.5 px red ring |
| View switch | Text only, 11 px, right-aligned, 12 px apart: "Session", "Arrangement"; the current one `text.primary` with a 1 px underline, the other `text.secondary`. While a clip is dragged over the other view's name, a 1 px `accent` outline 3 px around it; after 0.5 s the view switches |
| Open prompt | When a project is dropped while another is open and the setting says Ask: a 32 px row on `panel.background` above the view: "Open x:" In a new tab, In place of y, Cancel, Remember (which sets the setting) |
| Drop overlay | While a project file is dragged over the window: `window.background` at 45 % over the view and "Drop to open" in 13 px `text.primary`, fading in and out with the animations |
| Browser | 300 px wide by default, dragged from 200 px to half the window by a 4 px handle at its inner edge (`border`, `border.strong` under the pointer); left of the view, or right (Settings › Browser); Ctrl+B and View › Show browser hide and show it. See Browser below |
| Detail splitter | 6 px, `control.background`, a 32 × 2 grip in `text.hint` in the middle; the cursor shows which way it moves |
| Detail area | Under the view (and under the browser too with Settings › Detail area › Devices › **Detail area layout** on Full width, the browser then ending above it), 200 px by default, resizable from 80 px (120 px while it shows the piano roll) to all but 160 px of the view; dragged below half that it collapses to the splitter along the bottom, which is dragged up to open it again ([clip editor](../spec/ui-clip-editor.md) › Least height and closing), and at least as tall as the tallest open embedded plug-in editor; Shift+Tab hides and shows it. Shows the selected track's device chain; `AudioEffect` panels show visible signal-flow nodes and fitted parameter sliders; a selected clip (on the arrangement, or in a slot) shows its properties and, beside them, the chain of the track it is on: in a row (220 px per device at least, 272 px for a generated parameter list), or stacked in a column beside the view (272 px by default, resizable), as Settings › Detail area sets. With nothing selected: "Select a track or a clip to see it here" in `text.hint` ("Select a track or a clip" in the stacked column) |
| Status bar | 20 px, `panel.background`: the last message on the left (10 px, `text.secondary`, errors in the recording red), "N tracks · M scenes" on the right (10 px, `text.hint`), followed by " · backed up 14:05" once the active project has been backed up in this run. There is no separate info view |

## Browser

Behaviour in the [browser spec](../spec/browser.md). The panel is `panel.background`; its parts
from top to bottom:

| Part | Spec |
|---|---|
| Search | 26 px, `window.background`, 11 px search icon (a ring and a handle in `text.hint`), placeholder "Search", shortcut hint right in 9 px mono `text.hint` |
| Category filters | Wrapping row of buttons, 16 px high, 7 px side padding, 2 px gaps, 10 px text; selected `text.value` with `window.background` text, others `control.background` (hover `control.hoverBackground`) with `text.secondary` |
| Sidebar | 104 px, a 1 px `border` on its right; section titles (Pinned, Recent, Folders) 10 px `text.hint` with 8 px top padding; items 17 px, 10 px `text.secondary` (`text.primary` when selected); selected item `control.activeBackground`, hover `panel.hoverBackground`; a folder's state at its right in 9 px mono `text.hint` ("…" scanning, "poll", "gone"); "+ Add folder" in `text.hint` opens a path field in its place |
| Tree row | 17 px; indent 12 px per level; 10 px lead (a chevron in `text.hint` for what opens, a play triangle for a selected or hovered sample or MIDI file), 12 px type icon, 5 px, name 10 px `text.value` (`text.primary` selected), tag dots, meta right in 9 px mono `text.hint` (length, "plug-in", "MIDI effect", the folder a search found it in); selected row `control.activeBackground`, hover `panel.hoverBackground`; the list's scroll bar at the right edge is the one every scroll area has (as in a device's panel: thin until the pointer is near, dragged, clicked in its track), from `kit::scroll` |
| Type icons | Our own strokes, 1.1 px, 12 px: folder and heading `text.secondary`; MIDI note `track.2` (ochre); audio wave `track.3` (steel blue); instrument and preset `track.4`; effect `track.1`; MIDI effect the effect's box in `track.2`; project page `text.secondary` |
| Colour tag | 6 px dot after the name, 2 px apart, in the tag's colour; a tag's right-click menu in the footer offers the colour palette (Session view › Colour palette), then any colour |
| Preview strip | 54 px, `window.background`, a 1 px `border` on top: 22 px play button (`control.background`, its glyph `accent` while playing), file name 10 px, length, BPM ("~" when detected) and key right in 9 px mono `text.secondary`; under them the waveform ([waveforms](../spec/waveforms.md); a MIDI file: 2 px note bars), played part `text.secondary`, rest `text.hint`, playhead 1 px `accent` |
| List and Map | 22 px row above the results: two buttons like the category filters, List and Map; Map shows the [sample map](../spec/sample-map.md) in the results area |
| Sample map | A 24 px top bar (Cloud and Grid, the status in 10 px `text.secondary`, then +, −, Fit, Large and Window at the right; in its own window Dock instead of Large and Window, the window `window.background` all over), the map on `window.background`, a 20 px legend under it (a 7 px dot and "Kick 150" in 10 px, per kind, in the kind's colour; dimmed kinds at 30 %). Points: 2–6 px dots by zoom (a 1.2 px ring for a preset), or square cells 1 px apart in the grid; a guessed kind at 70 % and dimmed points at 18 %; rings: in use 1.5 px in the kind's colour, selected 1 px `text.primary`, previewed or under the pointer 1.5 px `accent`, similar 1 px `accent`. Kind names 17.6 px in the kind's colour with a 1 px `window.background` shadow, fading out from 4× to 6× zoom. Show similar adds a 180 px `panel.background` list at the right. Kind colours are our own muted hues, the theme's `kind.*` tokens (kick `#D06E5E`, snare `#73AAC0`, clap `#5EC0B0`, closed hat `#C6AC5E`, open hat `#D08F6E`, …; [brief](brief.md) › Tokens) |
| Footer | 20 px, `panel.background`, a 1 px `border` on top: the tags as 6 px dots (a filter ring in `text.primary` when on), the count right in 9 px mono `text.hint` ("12 items", "3 selected") |
| Drag label | Beside the pointer while dragging from the browser: 10 px text on `control.activeBackground` with a 1 px `accent` outline when it can land (`border.strong` when not); the target is outlined 1.5 px `accent` (a 2 px `accent` mark at the beat on an Arrangement lane) |

## Session view

From left to right: the track columns (groups and their tracks in project order), empty space
(`window.background`) if any is left, the return columns pinned against the scene column, and the scene
column pinned to the right edge. The track columns scroll sideways when they do not fit; the
returns and the scene column stay. From top to bottom: header row, scene rows, the stop row, empty
space, the mixer at the bottom.

| Part | Spec |
|---|---|
| Track column | 116 px; return column 84 px; scene ("Main") column 96 px |
| Column separator | 1 px `border.strong` through header, grid and mixer |
| Row separator | 1 px `border` between scene rows, continued through returns and scenes |
| Track header | 20 px, filled with the track colour (55 % of it over `control.background` when off); name 11 px in the text colour that reads on it (Colour palette); a selected track's header has a 1.5 px inset outline in that same colour. A click selects the track, a drag moves it, a right-click opens its menu; the strip's colour band opens the palette |
| Main header | 20 px, `control.background`, "Main" in `text.primary`, no colour strip |
| Scene button | 20 px, `control.background` (hover `#34332F`), a 4.5 px play glyph in `text.secondary` (`text.primary` hovered) 6 px from its left, name after it in 11 px, number right in 10 px mono `text.hint`; a playing scene `control.playingBackground`, its glyph the stop square, its click `scene.stop` |
| Stop row | 20 px under the last scene, `panel.background`: per track a 7 × 7 square outline in `text.secondary` (`accent` while the stop is queued), centred; in the scene column the square and "Stop all" |
| Empty space | Right of the last track: "+ MIDI track", "+ Audio track" and "+ Return track" buttons (a right-click anywhere there offers Insert audio track, Insert MIDI track, Insert return track) (20 px, `control.background`, 10 px `text.secondary`, 2 px apart, 6 px from its top left) and, below them, "Drop files or devices to add a track" in 10 px `text.hint`. Drops from the browser there make new tracks; the Arrangement has the same under its last lane |

Track colours: the track's own colour if it has one, else `track.1`–`track.4` in track order.

### Colour palette

One palette for every colour choice: the theme's eight track colours `track.1`–`track.8`, as
14 px swatches 3 px apart, the track's current colour with a 1.5 px `text.primary` inset outline,
a hovered one with 1 px. It is offered in the track's right-click menu (both views), in a popup
under a track's colour area, and in a colour tag's right-click menu (before the free colour
button there). A swatch is a button named "Colour #rrggbb"; picking one runs `track.setColor`
(one undo step; the colour the track already has runs nothing) and closes the menu or popup.

The colour areas, each a button named "<track> colour" that opens the popup on a click:

| Where | Area |
|---|---|
| Arrangement track header, return row | A 6 px band down the header's left edge, the header's full height |
| Session channel strip | A 5 px band down the strip's left edge, in its padding, the strip's full height at every mixer height |

Text on a track colour (the Session header, the strip's name row) is `text.primary` or
`window.background`, whichever has the higher contrast ratio with the fill; on the default
colours it is `window.background`.

### Clip slot

20 px high, 6 px side padding, 6 px gap between mark and name. On a track's clip, the first 18 px (the mark and its padding) launch it and the rest, its name, selects it; a group's slot launches from anywhere.

| State | Background | Mark | Extra |
|---|---|---|---|
| Empty without a stop button | `window.background` | none | |
| Empty with a stop button | `window.background` | 3 × 3 `#4A4743` | |
| Stopped | `control.background`, text `#C9C4BB` | a 8 × 10 px play triangle in the track colour (its tip right), the click's shape | |
| Queued | `control.background` | 10 × 10 track colour at 50 % with 1 px `accent` outline | 1 px inset `accent` outline on the cell |
| Playing | `control.playingBackground`, text `#FFFFFF` | an 8 × 8 px square in the track colour, the click's shape | 2 px `accent` progress line along the bottom |
| Selected | `control.activeBackground` (playing keeps `control.playingBackground`) | as its state | 1 px inset `text.primary` outline (a queued clip keeps its `accent` outline) |
| Recording | (planned) `#3A2B2A`, text `#FFFFFF` | 6 px red dot | 2 px red progress line |

The slot clicked last (on a track, not a group) is selected: a 1 px `text.primary` inset over
whatever state it shows. A right-click opens its menu (Copy, Paste), and a clip in it can be
dragged ([Session view](../spec/ui-session-view.md) › Between the views).

### Mixer strip (Session)

A channel strip under every track and return column, `panel.background`, 6 px padding; the
scene column's part holds the main meter only (the main track's strip is planned). The mixer's
height is the strip's: 166 px by default, from 46 px up to just under the stop row (at least to
166 px whatever the scene count). Its top edge, the 1 px `border.strong` line under the slots,
takes the pointer 3 px either side (a vertical resize cursor, labelled "Mixer height"); a drag
moves it, a double-click gives 166 px back.

**Height steps.** Each part appears once the mixer is at least as tall as its step, and goes
again below it. Top to bottom, the strip at its tallest: name, device list, sends row, pan row,
volume readout, fader with the meter, toggles. Between the name and the device list: the output.
On an audio track, the input's block (Audio From and the monitor's In / Auto / Off) sits above
the toggles from 236 px ([spec](../spec/ui-session-view.md) › I/O section). While any strip shows
that block, every strip keeps its room (empty where a track has no input), so the sends, pan,
fader and toggles stay in line across the mixer.

| Mixer height | Adds | Size |
|---|---|---|
| 46 px (least) | Toggles; the compact volume above them | toggles 18 px; volume 12 px, 4 px above the toggles |
| 96 px | The fader, the meter beside it and the dB readout over it, in place of the compact volume | readout 12 px, 2 px gap, fader at least 48 px, 4 px above the toggles |
| 122 px | Pan row | 22 px and 4 px under it |
| 148 px | Sends row | 22 px and 4 px under it (the pre dot sits in the gap) |
| 166 px (default) | Name row, at the top | 16 px and 2 px under it |
| 192 px | Output row, under the name | 16 px and 4 px under it |
| 236 px | On an audio track, the input's block above the toggles: the "Audio From" caption and the source's dropdown, then the monitor's In / Auto / Off buttons | caption 12 px, dropdown 16 px, buttons 18 px, 3 px between the parts; the fader loses their height first |
| 260 px | Device list, under the output | at least four 16 px rows and 4 px under it; the fader takes the height up to 140 px, the list the rest. The list is as tall on every strip, the returns and Main included: it is worked out as if the strip had a sends row, and a strip without sends gives that row to its fader. Main lists the main channel's devices. A divider under the lists (6 px hit area, a 2 px `accent` line hovered or dragged) sets the fader's height for every strip at once: dragged down, the lists grow and the controls get lower, down to a 48 px fader; up, until one device row is left. A double-click makes the split automatic again. Settings keeps it (`session_strip_fader`, 0 is automatic) |

| Part | Spec |
|---|---|
| Compact volume | 12 px across the strip, as the Arrangement's volume field: `control.background` (`control.hoverBackground` hovered), filled up to the level in `control.playingBackground` on the volume fader's law (0 dB at 85 %), the value right in 10 px mono `text.primary`. With Mixer controls on Knob, a 12 px knob at the left and the value right of it. Dragged up or down, the wheel, double-click for 0 dB |
| Fader | A 22 px column at the strip's left: a 4 px `control.background` track centred in it, filled from the bottom up to the level in `control.playingBackground` (the volume fader's law, 0 dB at 85 % of the travel, as the volume field), a 1 px `text.hint` tick 5 px either side of the track at 0 dB, and an 18 × 7 px cap in `text.value` (`text.primary` hovered or dragged) centred on the level, a 1 px `window.background` line across its middle. The column, readout included, takes the drag, the wheel and the double-click |
| Readout | Over the fader, 12 px across the strip: the volume in 10 px mono `text.value` (`text.primary` hovered), "-inf" below −70 dB, starting at the fader column's left edge |
| Meter | Two 4 px bars, as below, standing 6 px right of the fader column, from the top of the fader to its bottom; at the least height, where there is no fader, it is not drawn |
| Pan row | The pan knob (22 px) at the left, its value right of it, 6 px apart, 10 px mono `text.secondary` ("C", "15L"). While a send is dragged, its letter and level stand there instead ("A -12.0") |
| Sends row | One send knob per return, 4 px apart from the left, as many as fit (four in a track column) |
| Colour band | 5 px down the left edge, in the strip's padding, the strip's full height, the track colour (dimmed as the header while the track is off); a click opens the palette (Colour palette) |
| Name | 16 px, filled with the track colour as the column's header: the track's name in 11 px in the text colour that reads on it, 4 px in, cut off at the strip's edge |
| Output | 16 px across the strip: "→" (14 px, 10 px `text.hint`) and the output dropdown the rest of the row ("Main" or a group track, as the Arrangement header's; `track.setOutput`). Its menu lists Main and every group track but the track itself |
| Device list | 16 px rows, one per device in chain order: its name in 10 px `text.secondary` 4 px in (`text.hint` while the device is off), "plug-in" right in 9 px `text.hint` for a plug-in; `control.hoverBackground` hovered, `control.activeBackground` for the picked devices, the same ones the detail area's chain lights. The wheel scrolls it. "No devices" in 10 px `text.hint` when the chain is empty. A click picks the device, selects the track and opens the detail area on its devices; Ctrl (Cmd) adds a device to the picks or takes it out, Shift picks the run from the last one; a double-click opens a plug-in's editor. Delete (`device.removeMany`), Ctrl+C, Ctrl+X and Ctrl+V (`device.copy`, `device.paste`, after the last pick) and Ctrl+D (`device.duplicateMany`: copies right after the last pick, which become the picks; the clipboard stays; the picks must be in one chain or branch) act on the picks, in the strip and in the chain alike, and a panel's X on one of several picks removes them all. A row or a chain's panel dragged onto another track's column, header, strip or lane moves the device there (`device.moveToTrack`), with Ctrl held copies it; a label beside the pointer says which |
| Send knob | A knob with the return's letter in its centre (8 px mono `text.hint`), pointer `text.value` (`text.primary` hovered); −70 dB … +6 dB over the travel. A 1.5 px `text.value` dot under it when the send is before the fader |
| Knob | 22 px; circle r 9, `control.background` fill, 1 px `text.hint` stroke; pointer 1.5 px from centre to r 8; travel −135° … +135°; pan pointer `text.primary`, sends `#C9C4BB`. While a knob, a device dial or a value field is dragged the pointer hides and the mouse's raw motion moves the value, so a long drag never stops at the screen's edge; on Wayland and macOS the pointer also stays where the drag began (Windows and X11 cannot lock it, so it moves on hidden). It shows again when the button is released or the window loses focus |
| Toggles | 18 px at the bottom, at every height: track on (the track number, or the return's letter, in 10 px mono) and, 2 px apart, solo ("S" in 10 px) and cue ("C" in 10 px), a quarter each; on a track that plays notes or audio, solo, cue and arm take a fifth each and track on the rest |
| Background | `panel.background` behind the parts; a double-click on it or on the name (not on a control or a device row) selects the track and shows its devices in the detail area, opening it if it was hidden |
| Track on | On: `#C9C4BB` with `window.background` text. Off: `control.background` with `text.hint` |
| Solo | On: `accent` with `window.background` text; off: `control.background`, `text.secondary`. A click solos the track alone; Ctrl (Cmd) adds it to the tracks soloed already |
| Cue | Coloured as track on: on, `#C9C4BB` with `window.background` text; off, `control.background` with `text.hint`. A click turns the track's cue on or off (`track.setCue`): it is also heard on the cue output ([spec](../spec/cue-output.md)) |
| Arm | Off: a 1.2 px `text.secondary` ring on `control.background` (hover `control.hoverBackground`). On: `recording.background`, the ring filled in `recording`. A click arms the track alone (`track.setArm`, exclusive); Ctrl (Cmd) adds it to the tracks armed already. Tracks that play notes or audio have one: an armed note track plays the MIDI controllers, an armed audio track hears its audio input |
| Meter bars | Two 4 px bars (left, right), 2 px apart, on `window.background`; the main strip's at the same place as a track's. The scale is the volume's, −70 … +6 dB, linear in dB; the peak at 45 % opacity, the RMS level over it in full, `meter.normal`, the top 12 % `meter.hot` (above −3 dB); the held peak (1.5 s) a 1 px line; a 3 px clip light on top, 1 px above the bars, `recording` while lit; a click on the meter turns the light off (`mixer.resetClip`; [spec](../spec/level-meters.md)) |

## Arrangement view

The timeline on one side, the track header column on the other (right by default). From top to
bottom: the ruler, the loop brace row, the marker row, the track lanes (scrolling), and fixed rows for the return tracks and the main
track along the bottom edge.

| Part | Spec |
|---|---|
| Bar ruler | 20 px, `panel.background`; bar numbers in 10 px mono `text.secondary`, 3 px right of a 1 px `border`, labelled every 1, 2, 4 … bars so labels are at least 40 px apart; bars and beat lines follow the time-signature changes. A tempo or time-signature change is a 1 px `accent` line with what changes ("140 bpm", "→ 140 bpm" for a ramp, "7/8") in 9 px mono `accent`, 30 px right of the line ([tempo map](../spec/tempo-map.md)). A click sets the play start marker (`transport.setPosition`) to the beat under the pointer, on the grid of that zoom, Alt for free ([spec](../spec/arrangement-editing.md) › Play position) |
| Back to arrangement | In the bar ruler, 8 px from its right edge above the tracks: a 14 px square button with a return-arrow glyph (a curve over the top into a head pointing down), 1.1 px lines in `text.primary` on `control.background` (`control.hoverBackground` hovered), the name as its tooltip and accessible label; shown while any lane's track plays launched clips, clicked it runs `launcher.backToArrangement`. The dimmed lane: a wash of `window.background` at 45 % over lane and header |
| Play start marker | A 9 × 6 px triangle in `text.primary`, pointing down, its tip on the ruler's lower edge at the marker's beat |
| Marker row | 16 px under the ruler, `panel.background`, a 1 px `border` between them. Each marker: a 1 px `text.secondary` line at its beat and a tag to its right (`control.background`, `control.hoverBackground` hovered, `control.activeBackground` dragged) with its name in 10 px `text.value`, 3 px in; the tag is as wide as the name, 9–120 px, and ends 1 px before the next marker. Click jumps (`marker.jump`), drag moves (`marker.move`, on the ruler's grid, Alt for free), right-click opens its menu (name field and Rename, Jump to marker, Delete marker); a right-click on the ruler or an empty part of the row offers Add marker here ([spec](../spec/arrangement-editing.md#markers)) |
| Loop brace | A 14 px row between the ruler and the marker row, `panel.background`, a 1 px `border` above and below. The brace: a 6 px bar, centred, from the loop start to the loop end; looping on: `control.activeBackground` with a 1 px `text.secondary` inset, off: `control.background` with a 1 px `text.hint` inset. Drag on the row draws a region (looping on), within 4 px of an end resizes it (the cursor shows resizing), the middle moves it (the cursor grabs), on the ruler's grid, Alt for free; a click on it or Ctrl+L turns looping on or off ([spec](../spec/arrangement-editing.md#loop-region)) |
| Zoom and moving | 8 px per beat by default, 1–4000 px; Ctrl and the wheel zoom around the pointer, Alt and the wheel make every track lane taller or shorter (within 20–400 px) keeping the lane under the pointer in place, the wheel scrolls; holding the middle button down over the timeline moves it freely both ways (the cursor grabs), also over a clip, which stays where it is: clips, fades, fields and headers are dragged with the left button only |
| Grid lines | In the lanes and the fixed rows (not the empty space below the lanes), at every step of the control bar's grid value, coarser where lines would be closer than 8 px ([spec](../spec/arrangement-editing.md) › The grid): a 1 px `border` at each bar line, a 1 px `panel.background` line between |
| Lanes | Default height 48 px, the lower edge of a header resizes it (20–400 px); separated by 1 px `border.strong`; empty area `window.background`. Overlapping (stacked) clips share the lane in equal sub-rows |
| Clip | Fills its lane (or sub-row) from top to bottom, 1 px short of its end; `control.background` body; 2 px top strip in the track colour; name 10 px `#C9C4BB`, 4 px from the left and 3 px from the top (it stays in view while the clip's start is scrolled off); selected clip `control.activeBackground` with 1 px `text.primary` inset; a drag moves it along its lane or to another; a right-click opens its menu (Duplicate, Delete, then Split here, greyed out on the clip's edges, and Consolidate) |
| MIDI marks | Under the name: one 2 px line per note in the track colour, its length the note's; the clip's lowest note at the bottom, its highest at the top. From 17 px under the clip's top to 3 px above its bottom on a clip 30 px tall or more; below that from 4 px under the top, over the name's row, so a lane at its least 20 px still shows its notes. Pulled to the top with **Edited track**, the view is as tall as its ruler, loop brace, marker row and the whole lane, so the marks show there too |
| Audio marks | The waveform ([waveforms](../spec/waveforms.md)) in the track colour: per pixel column the stretch of the file it covers (the engine's beat-to-file mapping: offset, loop, warp markers), a filled shape at 55 % with smoothed edges and the RMS band in the full colour, left and right lanes when the clip is at least 40 px tall, the samples with dots when zoomed in past one per pixel, scaled by the clip gain and its fades (both as they are dragged, before the edit lands); a centred line until the file is decoded. Clips at least 80 × 30 px show their gain 12 px from the top right (44 × 12 px, 9 px mono, "-3.0 dB"), dragged up or down (`clip.setGain`, −70 … +24 dB, double-click resets) |
| Fades | On an audio clip: a 1 px `text.secondary` line from the bottom-left corner to where the fade-in ends at the top (2 px down, under the colour strip), and from where the fade-out starts to the bottom-right corner, following the fade's shape and curve (sampled every 2 px) |
| Fade handles | 7 × 7 px squares at the clip's top corners, inside the clip, shown while it is hovered or selected (and while dragged) on clips at least 21 × 14 px; with a fade, at the fade's end along the top edge. `text.secondary`, `text.primary` hovered, 1 px `window.background` outline; an 11 × 11 px area takes the pointer, drawn and hit above the gain field. A horizontal drag sets the fade (`clip.setFades`): the fade's end snaps to the grid, Alt for free; double-click removes it; right-click opens a menu of the shapes with the current one checked |
| Fade curve handle | A 7 px circle on the fade's line half-way through the fade, on fades at least 21 px wide, drawn and hit (11 × 11 px) like the fade handles. A vertical drag moves it with the pointer and bends the curve so the line passes through it (`clip.setFades` `inCurve`/`outCurve`): up towards +1, down towards −1, stopping at either; double-click sets it to 0. The line is drawn with at most 256 points |
| Playhead | 1 px `accent` through the ruler and all lanes while playing; while stopped, at the play start marker once it is past beat 0 |
| Header column | 232 px by default, dragged wider or narrower by its inner edge (160–520 px); on the right of the timeline, or the left (Settings, or right-click above the headers); `panel.background` (`control.background` when selected), the 6 px colour band down its left edge (Colour palette), 5 px top/bottom and 6 px side padding after the band (8 px more per group level), 6 px between groups. A click selects the track (Ctrl adds), a drag moves it, a right-click opens its menu (name field, colour swatches, group, ungroup, delete) |
| Group fold triangle | 12 px at the start of a group's first row, before the track number, on a group that holds tracks: a 7 × 4 px triangle, `text.hint` (`text.primary` under the pointer), pointing right while the group is folded and down while it is open. A click folds the group's tracks away or opens it ([spec](../spec/arrangement-editing.md) › Folded groups); a folded group's row then draws the clips of the track it stands in for |
| Automation lanes | Under a track while its envelope button (an 18 × 15 px button at the end of the header's first row: `control.background`, `control.activeBackground` while lanes are shown, a small envelope line in `text.secondary`, `text.primary` when shown) has them open: 40 px each, a 1 px `border` under each. The envelope is a 1.5 px line in the track colour (`text.hint` while a manual change has it off), 4 px from the lane's top and bottom; points are 3.5 px circles (4.5 px hovered) in the same colour, `text.primary` when selected, with a 1 px `window.background` outline; an 11 × 11 px area takes the pointer. A selection box is `accent` at 15 % with a 1 px `accent` line. The lane's header (`panel.background`, 5 px top and 6 px side padding): a dropdown of track mixer, built-in device and .sd device parameters (its menu, 240 px wide, under the dropdown or over it when there is more room above, never moving while open: a search field, then the list in a scroll area at most 60 % of the window tall and never past the window, 8 px from its edge, the mixer's rows first, then a heading per device and, indented under an open one, a heading per group: a heading is a row as tall as a button, the browser's chevron in `text.hint` 6 px from its left, the group's name 14 px from its left in the text colour, its parameter count (" •" with points) in `text.hint` 4 px from its right, `widgets.hovered` fill on hover; an open heading's rows are indented), the 30 × 15 px On/Off toggle (as the track-on toggle; off has a 1 px `accent` inset), 16 × 15 px **+** and **×** buttons (`control.background`, 11 px `text.secondary`), and under them a 10 px line: the number of points in `text.hint`, "Off: set by hand" in `text.secondary`, or "Click the lane to add a point". Gestures: [automation spec](../spec/automation.md#in-the-arrangement-view) |
| Return and main rows | 20 px each along the bottom, `panel.background` headers: a return's colour band as a track's (the main row has none, its name lined up with the others), the name in 11 px and the volume as a plain 10 px mono value on the right (dragged up or down; the main track's is shown only); between them the meter lying down, 48 px long, 6 px left of the volume, its clip light at the right end |
| Header meter | The track's meter (Session view › Meter) standing at the header's right edge, 6 px in, its rows 6 px clear of it; from the top of row 1 to the bottom of row 2 once row 2 shows, row 1's height before |

### Track header levels

The header reveals groups as the track grows. Thresholds are the heights at which a group fits;
a lane between 20 and 26 px shows the first row clipped.

| Track height | Shows |
|---|---|
| ≥ 26 px | Row 1, 15 px: track on (30 px, the track number in 10 px mono), solo (18 px, "S"), cue (18 px, "C"), arm (18 px, on a track that plays notes or audio), 2 px apart, the name in 11 px 6 px after them (on a group, a fold arrow first, 7 px, pointing down open and right folded); the colour is the band down the header's edge. The row steps in 8 px per group the track is inside |
| ≥ 48 px | Row 2, 6 px under row 1: the volume field (the rest of the row) and the pan field (46 px), 4 px apart |
| ≥ 70 px | Routing, 4 px under row 2: "→" and the output dropdown ("Main" or a group track, `track.setOutput`); audio input and monitor In / Auto / Off (planned, with recording) |
| ≥ 90 px | On a track that plays audio, 4 px under routing: "Audio" (28 px, 10 px `text.hint`) and the audio input dropdown ("No input", or "In 3" or "In 1/2" and " · Auto", " · Monitor in" or " · Monitor off"; `track.setAudioInput`); its menu, also under "Audio input: …" in the track's menu in both views, offers No input, each channel of the input device and each neighbouring pair (the first eight while no input device is open, saying so), then Monitor auto, in or off. On a track that plays notes, 4 px under routing: "MIDI" (28 px, 10 px `text.hint`) and the MIDI input dropdown, the rest of the row ("All ports", a port, or "No input", then " · Ch N" for one channel; `track.setMidiInput`). The track's menu, in both views, has the same choices under "MIDI input: …": every port, each port there is (one chosen but not connected says so), none; every channel, or Ch 1 to 16 in a grid of four |
| ≥ 114 px | (planned) Send fields, one per return, labelled "Send A", "Send B", once the engine plays sends; routing then moves below them |

### Value field (slider)

A number field that is its own slider: drag vertically to change (Shift for fine steps),
double-click to reset (typing a value is planned). A track's volume and pan are heard while
dragging, and a drag is one undo step.

| Part | Volume | Pan | Send |
|---|---|---|---|
| Size | 16 px high, full width | 16 × 46 | 16 px, half width each |
| Background | `control.background` | `#232220` | `control.background` |
| Value shown as | Fill from the left, `#4A4843`, full height | 2 px bar along the bottom from the centre, `#C9C4BB` | Fill `#3E3C38` |
| Marker | 1 px tick in `#C9C4BB`, only the top and bottom 2 px | 1 px centre tick `#55524C`, top and bottom 2 px | none |
| Text | Value right-aligned, 10 px mono | Centred | Label left (sans), value right (mono) |
| Mapping | the volume fader's law: 0 dB at 85 % of the travel, 40 dB per travel down to -18 dB at 40 %, faster below ([automation](../spec/automation.md#the-volume-fader)) | 50L … C … 50R | as volume |

### Dropdown and segmented control

| Part | Spec |
|---|---|
| Dropdown | 15 px high, `control.background`, 4 px padding, 10 px text `#C9C4BB`, 6 × 4 chevron `text.hint` right |
| Segmented | 15 px, equal parts 1 px apart (64 px each in Settings); selected `#C9C4BB` with `window.background` text, others `control.background` with `text.secondary`, 10 px text |

## Settings

The Export page's MCP write permissions use the standard On/Off row for the project's folder,
showing that folder in the note. Explicit allowed folders occupy separate rows with fitted
path text and a Remove button on the right. Add folder… opens the system folder picker.
The list scrolls with the page, and its note says that each grant includes subfolders.

Options › Settings (Ctrl+,) opens a dialog over the window until it is closed (Close, Escape or
Ctrl+, again): the view stays behind it, dimmed with `window.background` at 60 % and taking no
clicks, wheel or pinch (they reach only the dialog, as with every dialog over the window); embedded plug-in editors hide while it shows. The dialog is at most 772 × 720 px (6 px
less than the window on each side when that is smaller), `window.background` with a 1 px
`border.strong` border. Inside, a centred block at most 724 px wide: the title "Settings" in 13 px
with a 52 px Close button on the right; under it the list of pages (140 px), 24 px of space, and
the page (560 px). A change applies at once and is saved; one that needs a restart (the
renderer) says so in its note.

The list of pages: 20 px rows, 11 px names 6 px in, `text.secondary`; the page shown is filled
`control.background` with `text.primary`, a hovered one `#2A2926`. Over it an 18 px search field,
"Search settings" as its hint, 8 px above the list. Which setting is on which page, under which
heading, and how the search filters them is the [Settings spec](../spec/ui-settings.md):

- General
- Audio
- MIDI
- Plug-ins
- Arrangement
- Session and mixer
- Clip editor
- Transport
- Detail area
- Browser
- Appearance
- Shortcuts

A page starts with its first section's title (10 px `text.hint` over a 1 px `border`), level with
the top of the search field; 8 px above each next section. While searching, a title names the page
too ("Browser › Previews"). The page scrolls when it is taller than the dialog: a scroll bar in
the 12 px right of it, the wheel over it, and a control given the focus scrolled into view. Each
setting is a 34 px row with its name (11 px `text.primary`), a note under it
(10 px `text.secondary`) and its control on the right: a segmented control of 64 px per part, or a 260 px dropdown
(Settings › Audio). A read-only setting (Audio › Now: Output, Latency) shows its value as the note.
The page is drawn under the title only; the General page, taller than a small window, scrolls with
the wheel over it, as far as its last row and no further.

| Page › Section | Setting | Values |
|---|---|---|
| General › Projects | When opening a project | Ask (default), New tab, Replace |
| General › Projects | At start | Panel (default), New, Template, Last: what opens when Suhina starts without a project given (ADR 0010); Panel is the [start panel](#start-panel) |
| General › Projects | Projects folder, Templates folder | A 300 px field each, ending in a 60 px **Browse…** button (the system's folder dialog); empty uses `Documents/Suhina/Projects` and `…/Templates`, shown as the hint |
| General › Projects | Start template | Shown with At start › Template: a 300 px field, a template's file name in the templates folder or a path, ending in a 60 px **Browse…** button (the system's file dialog) |
| General › Backups | Backups | A 260 px dropdown: Before critical operations, Every minute, Every 5 minutes (default), 10, 15, 30 or 60 minutes, Every 4 hours, Never ([spec](../spec/backups.md)) |
| General › Backups | Clean up backups | On (default): the newest 20 backups of each project are kept; Off |
| General › Backups | Last backup | The active project's newest backup, its time and file, as the row's note; "None written for this project yet" |
| General › Dropping files | Several files | One track (default), One each; Ctrl while dropping does the other ([browser spec](../spec/browser.md) › Files) |
| General › Dropping files | Warp mode | Rhythmic (default), Tonal, Texture, Smooth, High quality, Re-pitch: how a dropped sample with a known tempo is stretched |
| General › Updates | Check for updates | On (default): once per start, asks GitHub whether a newer build is out ([spec](../spec/updates.md)); Off |
| General › Updates | Channel | Nightly (default); Test builds as well while developer mode is on |
| General › Updates | Developer mode | Off (default); On lists test builds of pull requests as a channel |
| Audio › Output | Driver | Default (the host's own), then each host (WASAPI, ASIO, …): a 260 px dropdown |
| Audio › Output | Device | Default (the host's default device), then the host's devices; a saved device that is gone says so and the default plays |
| Audio › Output | Sample rate | The device's own (default), then the rates the device offers |
| Audio › Output | Buffer size | Default (1024 frames), then the device's sizes with their length in ms |
| Audio › Output | Engine block | Where the device's buffer is not Suhina's to set — WASAPI shared mode, or a driver whose declared sizes the engine cannot render in — the row says *Engine block*, and on WASAPI its note names the period Windows gives the device in milliseconds (WASAPI reports that period in frames at each sample rate, lowest first); ASIO says *Buffer size*, with the driver's declared range in the note; WASAPI exclusive mode says *Buffer size* too, and asks the device for the period |
| Audio › Output | Exclusive mode | On, Off (default): WASAPI only; the note says when the device does not allow it |
| Audio › Output | Input | Off (default), Default (the host's default input), then the host's input devices with their channel count ("Line In (2 in)"); a saved device that is gone says so. Tracks with an audio input hear it |
| Audio › Cue | Cue output | Same as main (1/2) (default), then Outputs 3/4, 5/6, … as far as the device has outputs: a 260 px dropdown; a saved pair the device lacks is listed as "(not there)" and the note says the cue plays with the main output ([spec](../spec/cue-output.md)) |
| Audio › Cue | Cue volume | −12, −6, 0 (default), +6 dB |
| Audio › Now | Output, Latency, Key to sound, Input now | Read only: the device in use (", until X is back" when it plays in place of the chosen device X, which it goes back to once connected again), its measured latency and underruns, the longest a key on a MIDI controller takes to be heard (saying whether a track is armed, which keeps one block ahead), and the input device open with its channels and underruns; "List devices again" (120 px) on the Output row. Changes on this page run `audio.setOutput`, or `output.setCuePair` and `output.setCueVolume` for the cue |
| MIDI › MIDI inputs | Each MIDI input port | On (default), Off: the note says whether it plays the armed tracks or why it could not be opened; "None" when no controller is connected. Listed again every 2 s while the page shows; runs `midi.setInput` |
| Plug-ins › Editors | Editors | Window (default), Device view: where a plug-in's editor opens; the note says when the device view is not available on this system |
| Arrangement › Clips | Allow sample stacking, Allow MIDI stacking | On, Off (default): moved clips stack instead of replacing ([spec](../spec/arrangement-editing.md)) |
| Arrangement › Clips | Double-click makes a clip | On (default): a double-click on an empty slot (Session view) or an empty spot of a lane (Arrangement view) makes an empty clip of the track's kind — MIDI on a notes track, audio with no file yet on an audio track — (`clip.create`); Off: it does nothing ([spec](../spec/ui-session-view.md) › Clip slots) |
| Arrangement › Tracks | Track headers | Left, Right (default) |
| Session and mixer › Launching | Select on launch | On (default), Off: launching a clip in the Session view selects it, so the detail area shows it ([spec](../spec/ui-session-view.md)) |
| Session and mixer › Mixer | Mixer controls | Field (default): the volume bar and the pan centre bar; Knob: a knob with its value beside it, in the Arrangement headers and the Session mixer ([spec](../spec/arrangement-editing.md) › Volume, pan, mute and solo) |
| Clip editor › Piano roll | Pulled to the top | Edited track (default), Whole window ([clip editor spec](../spec/ui-clip-editor.md)) |
| Clip editor › Piano roll | Piano roll grid | Its own (default), Control bar ([clip editor spec](../spec/ui-clip-editor.md)) |
| Clip editor › Piano roll | Toolbar size | Normal, Large (default), Larger; the toolbar's box, gaps and icon strokes scale with it ([clip editor spec](../spec/ui-clip-editor.md) › Layout) |
| Clip editor › Workflow | Click on empty grid, Right click on a note, Shift+right drag, Alt+right button, Duplicate key, Double-click on empty grid, Double-click on a note | The workflow settings, each set on its own; changing one makes the workflow **Custom**; kept in `keymap.json` ([shortcuts](../shortcuts.md) › Presets) |
| Transport › Play and stop | Play from | Marker (default): Play and Space start from the play start marker; Selected clip: from the start of the clip selected in the Arrangement view, the marker moving there ([spec](../spec/arrangement-editing.md) › Play position) |
| Transport › Play and stop | Stop | Marker (default): back to the play start marker; Stay: the marker moves to where it stopped ([spec](../spec/arrangement-editing.md) › Play position) |
| Detail area › Devices | Device chain | Row (default), Stack |
| Detail area › Devices | Detail area layout | Beside (default): the detail area starts right of the browser, which runs down to the status bar; Full width: it spans the whole window, under the browser too. Not used while the device chain is stacked ([spec](../spec/ui-session-view.md) › Window regions) |
| Browser › Browser | Browser side | Left (default), Right |
| Browser › Previews | Auto-play | On (default), Off: selecting a sample or MIDI file previews it |
| Browser › Previews | Preview volume | −18, −12, −6 (default), 0 dB |
| Browser › Sample map | Map layout | Cloud (default), Grid ([sample map](../spec/sample-map.md)) |
| Browser › Sample map | Map preview on hover | On (default), Off |
| Browser › Sample map | Map: longest sample | 5, 10 (default), 30, 60 s |
| Browser › Tags | Tag limit | 64 (default), 128, 256, 1024 colour tags (`library.setTag`'s `limit`) |
| Appearance › Window | Zoom | Three parts: −, the value ("100 %", a click goes back to 100 %), +; 50–200 % in 10 % steps, the size of the whole window ([spec](../spec/window-zoom.md)) |
| Appearance › Window | Animations | On (default), Off |
| Appearance › Waveforms | Waveform scale | Linear (default), dB ([waveforms](../spec/waveforms.md)) |
| Appearance › Graphics | Renderer | GPU (wgpu, default), OpenGL (glow) |

**Appearance › Theme** (a page of its own, listed stepped in under Appearance): the themes as 20 px rows in a list that scrolls after eight, the user's
own first and then those that come with Suhina, the name on the left and the file name or
`built-in` on the right (10 px `text.hint`); a click uses one, the one in use filled like the page
list and scrolled into view when the page opens. Under them a 34 px row: the name of the theme in
use in a field, kept on Enter or leaving it (a built-in theme shows its name and "changing a
colour makes a copy"), and the folder of the theme files as the note. Then a row of buttons
(15 px, as wide as their text): Duplicate, Reload, Open folder, Back up, Export template. Then
"Colours" over a hairline and every colour token in a list that scrolls: an egui colour button
with a 1 px `border.strong` outline, the value and the token name (10 px mono), and what it is
used for, at least 160 px tall. Changing a colour of a built-in theme first makes a copy of it
("<name> (custom)").
See [brief](brief.md) › Themes.

**Shortcuts › Keys** lists every action under its group (File, Transport, View, Edit, Tracks, Commands): its
label (300 px, 10 px), its keys (130 px, 10 px mono `#C9C4BB`, "Press keys" in `accent` while
waiting), Change and Clear; Reset all at the top; a key bound twice in red above the list
([shortcuts](../shortcuts.md)).

**Plug-ins › Library**, under the Editors row: "Folders" (11 px), then one 18 px row per
folder: a 16 px checkbox (scans look in it), its path in 10 px `text.secondary` (`text.hint`
when off; the standard ones with "(standard)"), its format on the right in `text.hint` ("CLAP
and VST3", "VST3", "CLAP"), and for an added one a 64 px Remove button; a 20 px field to add
one, a four-part format switch (Both, VST3, CLAP, VST2 in `text.hint`, not choosable until
VST2 is hosted) and a 64 px Add button; a 34 px Scan row (the count found, switched off, failed
and built for another processor as its note, Scan and Rescan all on the right, run in the
background); a 34 px Plug-in manager row with **Manage plug-ins** on the right. Read and changed
through the `plugin.*` commands.

**Plug-in manager**, in the Settings dialog's place: the title (11 px) and the counts (10 px
`text.secondary`), Scan, Rescan all and Close on the right; a 260 px search field; the column
headings (20 px, 10 px `text.hint`, the sorted one `text.primary` with ▲/▼): On 28, Name and
File sharing what is left (42 % and the rest), Vendor 130, Type 72, Format 52, Version 70,
Status 96, and 84 for **Scan again**; 20 px rows, hover `panel.hover`, a plug-in's name in
`text.value` (`text.secondary` when switched off), a failure's status and reason in the
recording red ([plug-in manager](../spec/plugin-manager.md)).

The app keeps them in `settings.txt` (`key = value` lines) in its app-data folder, the theme in
use as `theme = <id>`; unknown lines are skipped with a warning in the status bar.

## Project dialog and save prompt

File › New or open (Ctrl+N) and Open (Ctrl+O, the path field takes the keys) show a dialog over
the window, as the plug-in loading dialog: the window dimmed with `window.background` at 60 %,
a 620 × 380 px panel, `panel.background` with a 1 px `border.strong` border, a 20 px
`control.background` title "New or open a project" with Close on the right. Escape closes it;
shortcuts wait while it shows.

| Part | Spec |
|---|---|
| New (left half) | "New" heading (10 px `text.hint` over a `border` hairline); a 20 px row "New blank project" with "4 tracks, 2 returns, 8 scenes" on the right; "From a template" and the templates folder's `.suhina` and `.als` files by name, 20 px rows that scroll, "Suhina project" or "Live Set" on the right; "No templates yet: File › Save as template" when there are none; the folder at the bottom in 10 px `text.hint` |
| Open (right half) | A path field (hint "Path to a .suhina or .als file"; Enter opens; quotes around a pasted path are dropped), a 64 px **Browse…** button (the system's file dialog, offering `.suhina` and `.als`; the dialog stays behind it, and a project chosen opens) and a 56 px Open button; "Recent": the projects opened and saved lately, newest first (12 kept), the name and its folder, or "Nothing opened yet" |
| Rows | Hover `panel.hoverBackground`; name 11 px `text.primary`, note 10 px `text.hint` |

A new project opens in a new tab. Save (Ctrl+S) writes the project's own `.suhina` file; the first
time, Save as (Ctrl+Shift+S) and Save as template ask in a 420 px prompt: Name and Folder fields
(the projects folder, the project's own folder, or the templates folder; Folder ends in a 64 px
**Browse…** button, the system's folder dialog), the file that will be
written under them (in the recording red when it is there already and will be replaced), Cancel
and Save (Enter). A name with `/`, `\` or `:` cannot be saved.

Closing a project, opening another in its place or quitting while a project has unsaved changes
first shows a 420 px "Unsaved changes" prompt in the same style: "Save the changes to *name*
before closing it?" (or "opening another project in its place", "quitting") in 11 px
`text.primary`, and Cancel, Don't save and Save (Enter) at the bottom right; Escape cancels.
Quitting asks about each project with changes in turn; Save on a project without a file shows
the save prompt first, and cancelling it cancels the close.

On start, a backup a crash left that is newer than its project file shows a 520 px "Backup found"
dialog in the same style ([spec](../spec/backups.md) › The offer on start): "Suhina closed before
*name* was saved." and "A backup from *date time* is newer than the project file." in 11 px
`text.primary`, and Ignore, Show the folder and Open the backup (Enter) at the bottom right, 120 ×
18 px each; Escape and Close ignore it. Several backups are offered one after another.

**Browse…** buttons ask the desktop app to show the system's own dialog (`pick.rs`; the app uses
`rfd`: the native dialogs on Windows and macOS, the desktop portal on Linux) and put what is
chosen in the field; cancelling leaves it as it was. The dialog opens in the field's folder
(the projects or templates folder) when it is there. Every path can still be typed.

## Start panel

While no project is open, the start panel (`start_panel.rs`) fills the area below the menu and
control bar on `window.background`; the browser is hidden while it shows (Ctrl+B's setting keeps
its value for when a project opens), and Ctrl+N (File › New or open…) does nothing there, since the
panel offers the same: at start when Settings › General › At start is Panel (the default), and
after the last project is closed. Its content is at most 820 px wide, centred, 24 px from the
top. Files dropped anywhere on it open as on the rest of the window.

| Part | Spec |
|---|---|
| Actions | Three 132 × 24 px buttons along the top, 6 px apart: **New project** (a blank one), **From a template** (shows the Templates tab), **Open…** (the system's file dialog; a project chosen opens) |
| Drop hint | One line under the actions, 11 px `text.secondary`: "Drop a project (.suhina) or an .als file here to open it"; the status bar says the same at start |
| List | A `panel.background` card with a 1 px `border`: the tabs **Recent** and **Templates** (11 px, the chosen one `text.primary` over a 2 px `accent` line, the other `text.hint`) and a search field up to 200 px wide on the right (hint "Search") that filters both lists by name, ignoring case |
| Recent | The projects opened and saved lately, newest first, as 20 px rows: the name, and on the right when it was last saved in words ("Just now", "5 minutes ago", "Yesterday", "3 weeks ago", "Last year"), or its folder when that cannot be read; the whole path as the tooltip. A click opens it as File › Open would; right-click › **Remove from list** takes it off `recent.txt`. "No projects yet: start a new one, or drop a project here" when there are none |
| Templates | The templates folder's projects as in the project dialog; a click makes a new project from one; the folder in 10 px `text.hint` at the bottom |
| Audio | A 240 px card on the right: "Audio" heading, Output, Sample rate and Buffer as the settings choose them ("Default" when unset), and an **Audio settings…** button that opens Settings › Audio. Below 560 px of width it goes under the list |
| Footer | "Suhina" and the version on the left, 10 px `text.hint` |

## Device panel

| Part | Spec |
|---|---|
| Panel | `panel.background`, panels 1 px apart, end of chain is empty `window.background` |
| Header | 20 px, `control.background`, 6 px padding: on switch, name, and what it is on the right in 9 px `text.hint` (Suhina, CLAP, VST3, VST2, or "Not supported (<format>)" for a device of another DAW). A right-click anywhere on the panel opens its menu: **Remove device** (`device.remove`); the panel's hover says so. A click on the panel selects the device: a 1 px `selection` outline (yellow; one at a time), and the Delete key takes the selected one off the track |
| On switch | 9 × 9 square, 1 px border; on: filled `text.primary`; off: outline `text.hint`, name `text.hint`. A click bypasses the device or enables it again (`device.setEnabled`): a bypassed **effect** passes the sound through untouched, and a bypassed **instrument**'s notes play the track's plain tones (as on a track without an instrument); either way its settings are kept. The switch's hover and label say which way a click goes: "Bypass <name>" / "Enable <name>" |
| Parameters | The compact panel, at most 168 px high under the header ([sdk-custom-ui.md](../spec/sdk-custom-ui.md) › Compact and expanded panels): a layout's canvas, or the generated list, 260 px wide, one 24 px row a parameter in its order (its name on the left, the stock `.sd` control, the value with its unit on the right), scrolling past 168 px. Built-in effects, instruments and MIDI effects, `.sd` devices and running plug-ins all have one; a change is `device.setParam` |
| Expand | On a device with an expanded panel, a 12 px button left of what it is, two corners pointing out in `text.hint` (`text.primary` hovered or open): "Expand <name>". It opens the expanded panel over the main area: a 26 px title row in `control.background` with the name in 11 px and a 24 × 20 px close button ("Close expanded panel"), the canvas centred under it on `panel.background`, scrolling when the area is smaller. Escape, the close button or a second click closes it |
| Plug-in | A 16 px row with where it stands, 10 px: the plug-in's name and vendor (and its latency in frames) in `text.secondary` while it runs; "Loading …", "Not running" (the project is not playing) or "Not installed; kept with its settings" in `text.hint`; "Crashed: …" or "Could not start: …" in `recording`. Under it a 96 × 18 px button, `control.background` (hover `control.hoverBackground`), 1 px `border.strong` border, 10 px `text.primary`: "Open editor" or "Close editor" while it runs, "Restart" after a crash; none otherwise. It runs `device.openEditor`, `device.closeEditor` or `device.restart`. With Settings › Plug-ins › Editors › Device view, an open editor sits under the button, 6 px in: in a row the device is as wide as the editor plus 12 px (at least 220 px) and the detail area at least as tall as the tallest open editor; stacked, the device is as tall as the editor needs; what does not fit is cut off at the panel's edge |
| No parameters | Another DAW's device: "Kept so the chain keeps its shape"; a built-in effect or instrument without parameters: "No parameters" (10 px `text.hint`) |
| Embedded editor | A plug-in's editor shown in its panel: the panel takes the detail area's whole height and the editor is fitted to the room under the panel's header when the plug-in allows resizing (a fixed-size editor keeps its size). The area opens tall enough for the editor once, as it opens; after that its height is the user's, set with the splitter, and is not held to the chain's panels (#689) |
| Stacked | In a column 272 px wide by default, each device is as tall as its compact panel (20 px header, 6 px padding, then its canvas or its list up to 168 px; a plug-in as tall as its state row and button, then its list or an open embedded editor), 1 px apart |
| Menu | **Group** (Ctrl+G, `device.group`), **Duplicate device** (`device.duplicate`), **Remove device**; on a device group also **Ungroup** (Ctrl+Shift+G, only with one branch) and **Add branch** ([device groups](../spec/device-groups.md)) |

### Device group panel

A device group ([spec](../spec/device-groups.md) › The device panel) is one panel in the chain, 260 px wide in a row (the column's width stacked), followed by the devices of its shown branch as the chain's own panels; nested groups do the same.

| Part | Spec |
|---|---|
| Header | 20 px as a device's: on switch, name, then **Mix** (62 px) and **Gain** (66 px) value fields 2 px in from the header's edges, 2 px apart, then the remove ×. A value field is `control.background` (hover `control.hoverBackground`), its label 10 px `text.hint` on the left, its value 10 px `text.value` on the right; dragged sideways it changes (Mix 0.5 % a point, Gain 0.1 dB a point), a double-click resets it (100 %, 0 dB); the values go out while dragging, as `device.setParam` continuous edits |
| Branch row | 18 px tall, 20 px apart, 6 px in: the name (a click shows that branch's devices after the panel; the shown branch on `control.activeBackground`, a muted one's name `text.hint`), its volume (44 px, "-6.0 dB") and pan (32 px, "C", "25L", "40R") value fields, then **M** and **S** (16 px each, 2 px apart; M lit as a track's toggle, S on `accent`). Volume and pan are drafts while dragged and set once when let go (`branch.setVolume`, `branch.setPan`), one undo step a drag; a double-click resets them to 0 dB and the centre |
| Add branch | A last row "+ Branch", `window.background` (hover `control.hoverBackground`), 10 px `text.hint`: `branch.add` |
| Inside a group | A device after a group's panel has a 2 px `accent` line along its top, one more 3 px lower for each group further in |
| Picks | Ctrl+G groups the devices picked on the track (next to each other) in the place of the first; Ctrl+Shift+G takes the picked group of one branch apart. Devices inside a group are not picked yet |

## Plug-in loading dialog

Shown over the window while the active project's plug-ins start (`crates/ui/src/app/loading_dialog.rs`),
until none is loading or it is sent to the background; a project opened next shows it again.

| Part | Spec |
|---|---|
| Behind | The window dimmed with `window.background` at 60 %; clicks there do nothing |
| Panel | 360 px wide, centred, `panel.background`, 1 px `border.strong` border; 20 px `control.background` title "Loading plug-ins · <project>" in 11 px `text.primary` |
| Progress | A 4 px bar, `control.background`, filled with `accent` for the share ready; under it 16 px rows in 10 px: "N of M ready, K not installed" `text.secondary`, "Loading <name>" `text.hint` (six, then "and N more"), "Could not start <name>" `recording` |
| Button | 150 × 18 px, bottom right, as the device panel's: "Continue in the background"; Escape does the same. The project plays meanwhile |

## Splash

Shown while Suhina starts (`crates/ui/src/app/splash.rs`), drawn inside the main window over its
first frames rather than in a window of its own: a second window would need its own GL or wgpu
context, the slowest part of starting, and one window that shrinks and grows would fight the
main window's remembered size and position (and Wayland lets a program neither place nor
re-decorate its window). The window opens; the splash is drawn for at least two whole frames and
0.2 s, so it is on screen; then the desktop app starts the browser's library, the sample map and
the project asked for, and the splash fades out over the window that is ready. Frames alone are
not enough: on macOS the first frames are drawn before the window has come on screen, so a
splash that went after them was never seen.

| Part | Spec |
|---|---|
| Behind | The whole window `window.background`; clicks there do nothing until it has gone |
| Panel | 300 × 200 px (smaller in a smaller window), centred, `panel.background`, 1 px `border.strong` border |
| Logo | The Suhina logo (`assets/brand/suhina-logo.svg`) 200 px wide (24 px clear of the panel's sides in a narrower one), its centre 12 px above the middle; the name in `text.primary`, the wave in its own `#FFD97D`. The app fills the file's paths itself (`crates/ui/src/kit/logo.rs`, tiny-skia) at the screen's pixel density; if that fails, "Suhina" in 13 px `text.primary` instead |
| Version | "Version x.y.z" (the app's `CARGO_PKG_VERSION`), 10 px mono `text.secondary`, under the logo |
| Status | "Starting", 10 px `text.hint`, centred at the bottom |
| Going | Fades out in 120 ms once the start-up work has run, it has been shown for 1 s, and 0.3 s have passed since the first frame after that work (where the window comes on screen only once the work is done, it is still seen there); at once with animations off |

## Clip properties

The detail area for a selected clip, on the arrangement or in a clip slot: a 196 px `panel.background` column with the clip's name
(20 px title, 11 px) and 16 px rows of label (10 px `text.secondary`) and value (10 px mono
`#C9C4BB`): Track, Start (bar.beat; for a slot's clip, Scene and its number), Length (bars.beats), Loop, then Gain, Fade in and Fade out (beats,
"-" for none) for an audio clip, or Notes for a MIDI clip; a value keeps 52 px clear of its label, so a long track name is shortened
rather than drawn over it. The track's device chain follows to the right (in a row) or below (stacked:
the column is then as tall as its rows). For a MIDI clip the piano roll (below) takes the place of the
device chain. Editing the properties is planned.

For a clip with an editor (a MIDI clip) the title row ends in the **Clip / Devices** switch, 6 px
from the column's right edge: a segmented control 15 px high, two 44 px parts 1 px apart, "Clip"
and "Devices" in 10 px; the part on is `text.value` (`#C9C4BB`) with `window.background` text, the
other `control.background` (`control.hoverBackground` hovered) with `text.secondary`, and the change
eases over 120 ms. The clip's name keeps 6 px clear of it and is shortened with an ellipsis. On
**Devices** the column stays and the track's device chain takes the piano roll's place
([clip editor](../spec/ui-clip-editor.md) › Where it opens).

## Follow action

**(proposed)** The detail area for a selected slot clip starts with its follow action
([Session view spec](../spec/ui-session-view.md) › Detail area: follow action): a 196 px
`panel.background` column in a row, or 140 px at the top of the stacked column.

| Part | Spec |
|---|---|
| Title | 20 px, "Follow action" 11 px `text.primary`, 6 px padding; the on switch at its right |
| On switch | 32 × 15, "On" / "Off" 10 px; on: `accent` with `window.background` text, off: `control.background` with `text.secondary` |
| Rows | From 6 px under the title, 18 px apart: a 10 px `text.secondary` label (`text.hint` while off) in 64 px, the 15 px control in the rest |
| Time, chances | Value fields: `control.background`, value right-aligned 10 px mono `#C9C4BB`; a chance's share fills from the left in `control.playingBackground`; a linked time, or a chance of 0, in `text.hint` |
| Linked | 32 × 15 toggle as Track on: `#C9C4BB` with `window.background` text on, `control.background` with `text.hint` off |
| Actions | Dropdowns (above); the menu lists the actions, then "Jump to scene N" |

## Clip editor (MIDI)

Built so far (`crates/ui/src/detail/piano_roll.rs`, the ruler in `crates/ui/src/detail/piano_roll_ruler.rs`): the 24 px ruler (the 8 px clip loop row over 16 px of bar numbers and lines, with the start and end markers), the 36 px
key column, the note rows, notes and the velocity lane (72 px, or 30 % of the detail area's height
when that is less, with 127 and 1 in its scale column). Rows are drawn as white-key rows
(`panel.background`) and black-key rows (`window.background`) until the clip has a scale; with
one, the rows of the keys in the scale are `control.background`, its root's `control.hoverBackground`,
and the others `window.background`, and each key in the scale has a 2 px mark at the right edge of
the key column, 1 px in from the column's line, in `text.secondary` (the root's `text.value`); the
grid's lines are `border`, bar lines `border.strong`; the grid outside the part of the content the
clip plays (before its start marker or loop, past its loop's end or end marker) is darkened, and
the notes there are drawn at 0.45 of their opacity.
A rectangle being dragged is a 1 px `text.secondary` outline. The toolbar is built too
([#442](https://github.com/suhina-daw/suhina-daw/issues/442), icons
[#443](https://github.com/suhina-daw/suhina-daw/issues/443) and
[#460](https://github.com/suhina-daw/suhina-daw/issues/460)): 22 px at **Toolbar size** Normal on
`panel.background` with a
1 px `border` under it, 28 px at Large (the default) and 33 px at Larger, with the 14 px icon box,
the 3 px gap and the 1.1 px strokes scaled by the same factor (so at Large the box is 17.5 px and
the stroke 1.4 px), and never taller than the room the editor has; from where the grid starts
(36 px in), the tools Select, Draw, Paint,
Erase, Slice and Mute as icons drawn in code (1.1 px strokes at Normal: an arrow, a pencil, a
brush, a slanted eraser block with a lighter rubbing end, a blade, and a speaker with a cross),
3 px apart, the chosen one `control.activeBackground` with a `text.primary`
shape, the others `control.background` with a `text.secondary` shape; each has a tooltip and an
accessible label of its name and key ("Draw (2)"). The pointer over the grid takes the tool's
shape: an arrow, a crosshair, an eraser (the eraser icon, not a no-entry sign), a text cursor, an
arrow, an arrow. After a 1 px `border.strong` divider and 5 px,
the note clipboard's Cut, Copy, Paste and Duplicate icons, greyed out (`text.hint`, no highlight)
when there is nothing to act on; then the quantise icon (notes snapping onto a line), which shows
the MIDI tools column with the quantise block open, and the **MIDI tools** icon (three horizontal
sliders, `control.activeBackground` while the column shows), which shows and hides that column,
and the **Audition** icon (headphones: a 1.1 px band over two filled 3 × 5 px cups,
`control.activeBackground` with a `text.primary` shape while on, off at start; accessible label
"Audition"), which makes drawn and moved notes sound once.
4 px after it the 96 px **workflow menu**: the mode's name in 10 px `text.value`, its menu
the four modes as selectable rows, a divider, and one row per workflow setting ("Double-click on a
note: Deletes it") opening a submenu of its choices, the chosen one highlighted; it is left out when
the toolbar is too narrow for it and the grid menu.
After it the 64 px **ghost menu**: "Ghosts" in 10 px `text.value` (`text.hint` while it
shows none), its accessible label and tooltip naming what it shows ("Ghost notes Playing
together"); its menu the rows None, Playing together and This track, the chosen one highlighted, a
divider, and **Clip…** opening a submenu of every other MIDI clip ("Walk on Bass at bar 17"),
scrolling past 320 px. It is left out, before the workflow menu, when the toolbar is too narrow for
it and the grid menu. **Ghost notes** are plain rectangles in their own track's colour at 0.25,
under the clip's notes, with no name, edge or velocity stem
([clip editor spec](../spec/ui-clip-editor.md) › Ghost notes).
After it the **Scale menu** (at most 56 px): "Scale" in 10 px `text.value` (`text.hint` while the
clip has no scale), its accessible label and tooltip naming the scale ("Scale D dorian", "Scale
off"); its menu the row Off, a divider, **Root: D** and **Mode: Dorian** opening submenus of the
twelve roots and the modes, a divider, and the toggles **Fold** and **Fold to scale** (greyed out
without a scale), the chosen ones highlighted. The workflow, ghost and Scale menus each take the
width their button needs, 4 px after the one before; the Scale menu is left out when the grid
value would not fit after it, and the "Grid" caption is left out first when it needs the room.
Folded, the rows are drawn the same way with no gap for the keys left out, and every key-column
row is named in 9 px mono `text.secondary` when it is at least 8 px tall
([clip editor spec](../spec/ui-clip-editor.md) › Scale and key).
Each toolbar menu's button ends in a small filled triangle, 6 px wide, drawn as a shape 7 px from
its right edge in the button's text colour.
At the right end
"Grid" in 10 px `text.hint` and a 76 px grid menu button (its value in 10 px mono `text.value`). Its menu lists, in 10 px text and separated by dividers: **Snap to grid**; the heading
"Adaptive grid" in `text.hint` and the five widths; the heading "Fixed grid" and the nine values;
**Narrow grid** and **Widen grid**; **Triplet grid**. Each entry's key is right-aligned in 10 px
mono `text.hint`; the chosen entries are highlighted as selected, and an entry the control bar
cannot hold is greyed out while the piano roll follows it. Then the
**Hide editor** button: a box as tall as the tool buttons with a 1.1 px × in `text.secondary`
(`text.primary` on `control.hoverBackground` hovered), 3 px from the right edge. The piano roll's
least height is 120 px of detail area (the toolbar, the 24 px ruler and five 12 px key rows; the
velocity lane is left out when it would leave fewer than five rows or be under 12 px). The lane's
top edge (the 1 px `border.strong` line over it) is grabbed within 3 px either side, the pointer a
vertical resize cursor: dragged, the lane is 24 to 480 px tall, never leaving fewer than five key
rows. A velocity line being drawn in the lane is 1 px `accent`; an Alt+drag on a note shows its
velocity over it, "Velocity 112" in 9 px mono `text.primary` on a 76 × 14 px `control.background`
box with 2 px corners. Notes a draw or
paint drag is making are the track colour at 0.7 with a 1 px `text.primary` inset; notes an erase
drag has passed over fade to 0.12; the cut line of a slice drag is 1 px `accent`, and a note it
would split is marked with a 1 px `accent` line over it. A key with the pointer over it, or one
that is playing, is `control.hoverBackground`. A **muted** note is the track colour at 0.25 with a
1 px dashed `text.hint` outline under its 1 px `text.primary` inset when selected, and its velocity
stem is `text.hint`; it keeps its name and position.
Pulled to the top with **Edited track**, the Arrangement view's
one lane is `panel.hoverBackground` with a 2 px `accent` edge on its left, its header
`control.background` as a selected one. The quantise block is built at the top of the MIDI tools
column (112 px: a 20 px title "Quantise", then one row each for the 64 px Grid menu, the 150 px
Amount slider with its value behind it, the "Move ends too" box and the two 18 px buttons
**Quantise notes** and **Close**; nothing is placed beside a field whose value would fall off the
column's edge); the clip's own properties stay in the properties column beside it, and the block is
no pop-up.
The rest of this table is planned.

| Part | Spec |
|---|---|
| Columns | Clip properties 196 px · MIDI tools 112 px · editor (the tools column is not drawn with the detail area stacked) |
| Clip properties | 20 px title with 2 px clip-colour strip; 6 px padding and group gap; each pair = labels row (10 px `text.secondary`) and values row (15 px mono fields); Loop as a 15 px toggle; with notes selected, a Notes row "N of M" and, under it, From, To, Pitch and Velocity rows in the same pair shape, all 10 px/15 px |
| MIDI tools | 112 px on `panel.background` with a 1 px `border` on its right; 10 px head ("Selected notes" / "Whole clip") with a 10 px `text.hint` count under it; the quantise block (above); Transpose as a 64 px value field with −/+ steppers; then a two-column grid of 18 px buttons (×2, ÷2, Fit scale, Invert, Humanise, Reverse, Legato, and the velocity Randomise) plus a Ramp row of two 48 px fields (From, To) |
| Toolbar | 22 px at Toolbar size Normal, 28 px at Large, 33 px at Larger (built: the six tool icons, the clipboard's four, the quantise icon, the MIDI tools icon and the grid value); later also tabs (current has a 1 px underline), divider, Fold, Scale (on = `#C9C4BB`), scale name |
| Ruler | 24 px: the 8 px clip loop row, then 16 px of bar numbers (10 px mono `text.value`) and `border.strong` bar lines (built); later beats `text.hint` and beat lines `#34332F`. The start marker is a 1 px `text.primary` line down the bar-number row with a 6 × 7 px flag at its top pointing right; the end marker (a clip that does not loop) the same flag pointing left. Within 4 px of a marker a press drags it (the cursor shows resizing); anywhere else a drag zooms time (up out, down in) and scrolls it (sideways) (built) |
| Clip loop | 8 px row on `window.background` with a 1 px `border` under it; a 4 px bar, centred, from the loop start to the loop end: looping, `control.activeBackground` with a 1 px `text.secondary` inset and 2 × 8 px handles `text.primary`; not looping, where turning the loop on would put it, `control.background` with a `text.hint` inset and handles. Within 4 px of a handle a press drags it, the middle moves the loop (the cursor grabs), on the piano roll's grid, Alt for free; a click on the bar turns the loop on or off (built) |
| Key column | 36 px; white keys `control.background`, black keys `window.background`; the key under the pointer or playing `control.hoverBackground`; 2 px scale mark `#6E6A63`; C labels with octave (C3 = MIDI 60) |
| Note rows | Scale rows `#23221F`, others `#1A1917`; 1 px row lines `#2A2926`, under C `border.strong` |
| Note | Track colour; opacity 0.4 + 0.6 × velocity / 127; name in 9 px when ≥ 2 beats wide; 1 px gap to neighbours; selected = 1 px `text.primary` inset |
| Velocity lane | 72 px default (resizable); 36 px scale column (127, 64, 1); one 1 px stem per note with a 5 × 2 head, chord notes 4 px apart; selected stem `text.primary` |
| Velocity tools | 18 px: Randomize, Ramp, Deviation as small value fields |

A note-playing Arrangement header has a neutral Auto / On / Off chip after record arm.
It is 24 px wide on a compact row and 32 px on an expanded row. The popup explains
what input monitoring means, offers Auto, Always on and Off, and shows a contextual
footer for the current or hovered choice. MIDI monitoring is independent of record
arm and headphone cue. Only record arm uses red; the MIDI input menu has the same
choices. Header layout reserves this chip's width before names and level controls.
