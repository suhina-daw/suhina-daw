---
description: Functional spec of the MIDI clip editor (piano roll): layout, toolbar size and icons, the six tools and the Mute tool, the MIDI tools column, grid and snapping, zoom, drawing, selecting, moving, resizing, velocity, quantise, scales, ghost notes, slicing, a playable key column, chords, shortcuts and keymap presets, and the note.* and preview.* commands
read_when: piano roll, clip editor, toolbar, toolbar size, toolbar icons, tool icons, draw tool, paint tool, erase tool, slice tool, mute tool, muted note, note.setMuted, MIDI tools column, transpose, humanise, humanize, fit to scale, invert, reverse, legato, stretch, randomise velocity, ramp velocity, B key, number keys choose a tool, right button and wheel, Shift and Draw, pulled to the top, maximise piano roll, piano roll grid value, grid menu, adaptive grid, fixed grid, snap to grid, snap off, triplet grid, narrow grid, widen grid, Ctrl+1, Ctrl+2, Ctrl+3, Ctrl+4, MIDI editor, note editing, draw notes, select notes, move notes, resize notes, velocity lane, velocity, grid, snapping, Alt off-grid, zoom, detail area height, scale highlighting, fold, ghost notes, quantise, quantize, inline quantise, clip column, selection info, slice, glue, chords, strum, playable keyboard, key column plays, preview.noteOn, preview.noteOff, note.add, note.delete, note.move, note.resize, note.setVelocity, note.split, note.list, note.quantize, Ctrl+U, note.copy, note.cut, note.paste, note.duplicate, copy paste notes, arrow keys, select all notes, keymap preset, Draw-first, Select-first, Mouse-first, mouse switches, duplicate key, workflow mode, workflow menu, Custom mode, double-click, double-click adds a note, double-click deletes a note, triple click, least height, short detail area, close the piano roll, hide editor, close button, collapse the detail area, notes in view, centred notes, ruler, loop row, loop region, loop handles, loop start, loop end, clip loop, start marker, end marker, clip length, clip offset, clip.setLoop, clip.setLength, clip.setStart, ruler drag zoom, ruler scroll, zoom to selection, zoom to clip, Z, Shift+Z, notes outside the loop, shaded notes, left edge, resize the start, startBy, Ctrl+drag copies, copy by dragging, Alt+drag velocity, velocity line, draw a line in the velocity lane, velocities, velocity lane height, lane top edge, audition, hear notes while editing, preview.noteOn keys, ghost menu, ghost notes of other clips, notes of other clips, playing together, write a bass line against the chords, faint notes behind, Scale menu, clip scale, clip.setScale, clip.scale, project.setDefaultScale, default scale, scale rows, key column mark, Fold, Fold to scale, folded rows
type: spec
status: draft
last_verified: 2026-11-02
covers:
  - crates/ui/src/detail/piano_roll.rs
  - crates/ui/src/detail/piano_roll_grid.rs
  - crates/ui/src/detail/piano_roll_ruler.rs
  - crates/ui/src/detail/piano_roll_gestures.rs
  - crates/ui/src/detail/piano_roll_ghosts.rs
  - crates/ui/src/interaction/timeline_grid.rs
  - crates/ui/src/detail/piano_roll_scale.rs
  - crates/ui/src/detail/midi_tools.rs
  - crates/ui/src/detail/detail_view.rs
  - crates/session/src/notes.rs
  - crates/session/src/scales.rs
  - crates/session/src/note_tools.rs
  - crates/session/src/preview.rs
  - crates/session/src/clip_shape.rs
---

# UI: clip editor (MIDI piano roll)

Functional spec of the editor that opens in the detail area for a MIDI clip: a piano roll with a
keyboard on the left, a note grid, and a velocity lane below. Only structure and behaviour; the look
comes from [`docs/design/brief.md`](../design/brief.md) and the measurements from
[`docs/design/components.md`](../design/components.md) › Clip editor.

**Sources.** Suhina's own design, written in our own words under the clean-room rules
(`CONTRIBUTING.md` › Clean-room); nothing here comes from another product's code, files or
artwork. Items marked **(to decide)** are open design questions to settle when they are built.

**What is built.** Each section says which parts the first version builds (**Built**) and which
come later (**Later**, with the issue that tracks it). The first version is
[issue #200](https://github.com/suhina-daw/suhina-daw/issues/200)'s first pull request: a piano roll
for a selected arrangement clip, drawing, selecting, moving, resizing and deleting notes, a velocity
lane, the grid and zoom, and the `note.*` commands.

## Where it opens

- **Built:** selecting a MIDI clip in the Arrangement view shows the clip's properties column on the
  left of the detail area and the piano roll in the rest of it, in place of the track's devices.
  Selecting a track, or an audio clip, shows what it showed before (devices; an audio clip's
  properties). With the detail area stacked on the right, the piano roll sits under the properties.
- **Built:** a clip selected in a Launcher slot opens it too (the Clip / Devices switch below).
- **Later ([#343](https://github.com/suhina-daw/suhina-daw/issues/343)):** the piano roll edits
  that slot's clip with the same commands (`scene` instead of `index`). A double-click on a clip
  opens the detail area if it is closed.
- **Built ([#505](https://github.com/suhina-daw/suhina-daw/issues/505)):** a **Clip / Devices**
  switch, so a MIDI clip's track devices stay one click away. While the detail area shows a clip
  that has an editor (a MIDI clip, in the Arrangement view or in a slot), the clip column's title
  row ends in a two-part switch, **Clip** and **Devices** ([components](../design/components.md)
  › Clip properties). **Devices** puts the track's device chain where the piano roll was: the clip
  column stays on the left (on top when stacked) with the switch in it, and the chain fills the
  rest. The MIDI clip retains its resizable detail area in both tabs. A device row alone has
  two states: open at 200 logical pixels or closed. Dragging the splitter down past half that(
  )height closes it, dragging it up opens it whole; no height in between cuts its panels.
  Dropping an instrument, effect or plug-in there adds it to
  that track as it always does. **Clip** brings the piano roll back. **Ctrl+Shift+D** (the keymap
  action `view.devices`) and **View › Show devices** / **Show clip** do the same; they open the
  detail area first when it is hidden. An audio clip has no switch: its track's devices already
  show beside its properties, and the key leaves it as it is, unless its track has a plug-in that
  edits clips (below).
- **Built ([#594](https://github.com/suhina-daw/suhina-daw/issues/594)):** an **audio clip on a
  track whose plug-in edits clips** (ARA, such as a pitch editor; [ADR
  0028](../decisions/0033-ara-hosting.md)) has that plug-in's editor where a MIDI clip has its piano
  roll: the clip column on the left, the editor in the rest, embedded in the window whatever
  Settings › Plug-ins says for other editors (Windows, X11 on Linux; macOS opens it in a window of
  its own), asked to take the room there as the piano roll does (`fit_editor`; a plug-in that
  lets its editor be resized takes it within its limits, one of a fixed size is cut off at the
  edges), and the detail area does not grow for it. **Float** above it takes it out to a window of its own; the area then says so, with
  **Dock**, which brings it back, and **Close editor**. Its keys are its own while it has the
  keyboard: a click in it gives it the keyboard, so Ctrl+Z there undoes the plug-in's edit, not
  Suhina's last step. The plug-in is told which clip is selected, so its editor shows that
  clip, and selecting another clip on the track shows that one. The editor opens on its own the
  first time a clip is shown; closed, the area offers **Open editor**, and it stays closed for that
  clip until the clip is selected again. The clip column has the **Clip / Devices** switch, which
  works as for a MIDI clip (Devices shows the chain, the plug-in's editor among it). The plug-in belongs to the clip ([ARA hosting](ara-hosting.md) › The plug-in belongs to the clip): the clip's right-click menu has **Edit with** and the plug-in's name for a clip that does
  not play through it yet, **Open in** for one that does, and **Stop editing with** to take it off
  (`clip.setAra`); **Alt+M** (`view.araEditor`) is Edit with or Open in for the selected clip: it
  selects **Clip**, opens the detail area and the editor. They work on any audio clip in the
  Arrangement while such a plug-in is installed (Melodyne first): on a track without it, it
  becomes the track's clip host, first in its chain and not in its device lists, and its editor
  opens in the clip's place once it runs with the clip; the status bar says so meanwhile. A
  double-click on a clip that plays through it opens the editor; other clips on the track open
  nothing there.
  Moving or trimming the clip changes it in the editor at once, the editor staying open; when the
  plug-in has to be loaded again (it was added to a track with no audio clips yet), an editor
  that was open opens again.

  The switch is window state, like Shift+Tab: not a command, not an undo step, not saved. It
  lasts while the same clip stays selected; **selecting another clip** (in the Arrangement view or
  in a slot) shows its editor again, and in the Arrangement view so does a click on the clip
  already selected, so a single click on a clip keeps meaning "edit this clip". While the devices show, the piano roll's keys and Ctrl+U do nothing
  (there are no notes on screen), the splitter's least height is the devices' 80 px (the height dragged freely is for a clip's view and the devices reached from it; a track's devices, reached by clicking the track, are open at a compact panel's height or closed, never between: a drag on the splitter past half that height closes or opens the area, and a double-click closes it; a plug-in's editor embedded in the chain gets its full height, taking the view's least room when it needs to), and nothing
  is pulled to the top.
- **Built ([#505](https://github.com/suhina-daw/suhina-daw/issues/505)):** a **double-click on a
  track** shows its devices: on an Arrangement track header ([arrangement
  editing](arrangement-editing.md) › Tracks), on a Launcher column's name, or on the
  Launcher channel strip's own background ([Launcher view](ui-session-view.md) › Mixer). The track
  is selected (the first click of the two already does that), the detail area opens if it was
  hidden (Shift+Tab, View › Hide detail area, or the splitter), and it shows that track's device
  chain. A device's panel answers a click with its selection (a 1 px `selection` outline in the
  theme's yellow, one device at a time) and a right-click with its menu: **Remove device** takes it off
  the track, settings and all (`device.remove`), one undo putting it back; the Delete key
  removes the selected device while the chain shows. The panel's top-left switch **bypasses**
  the device or enables it again (`device.setEnabled`): a bypassed **effect** passes the sound
  through untouched, and a bypassed **instrument**'s notes play the track's plain tones (as on a
  track without an instrument); either way its settings are kept and its name dims. On an Arrangement
  header or a channel strip, a clip selected on that track stays
  selected and the switch goes to **Devices**, and a clip selected on another track is no longer
  selected; a click on a Launcher column's name clears the slot selection already, so there the
  devices show on their own. A double-click on a
  control in the header or the strip (a button, a fader, a knob, a value) is that control's own,
  as before.

## Coordinates

- **Time** is in beats from the start of the clip's content (its first beat is 0), the same beats
  the model stores (`Note::time`, `Note::duration`). The ruler shows bars and beats of the content
  (bar 1 at beat 0), in the project's time signature.
- **Pitch** is the MIDI key, 0 to 127. Key names use the convention of naming MIDI 60 **C3**, so
  the lowest key is C-2 and the highest G8. Every C is labelled.
- **Velocity** is shown and edited as 1 to 127 (MIDI). The model keeps 0.0 to 1.0; a command's
  velocity v is stored as v / 127, and shown as the stored value × 127, rounded. 0 is not a
  velocity (it means a note-off in MIDI).
- A clip may share its content with other clips (a duplicated clip, a Live Set's clips that
  point at one content, ADR 0001). The first note edit that changes such
  a clip's notes gives that clip its own copy of the content, with the edit, in the same command
  and so the same undo step; the other clips keep the content as it was
  ([#352](https://github.com/suhina-daw/suhina-daw/issues/352)). The copy keeps the content's
  name. An edit that changes nothing, or fails, copies nothing. A shared pattern (a content
  with lanes for named tracks) stays one content: editing it through one placement changes every
  placement, as it is meant to. **Later:** a linked copy the user asks for (a clip that stays
  shared on purpose), shown on the clip.

## Layout

From left to right: the clip properties column (exists), the MIDI tools column (built,
[#460](https://github.com/suhina-daw/suhina-daw/issues/460)), the editor. The editor, top to
bottom:

1. **Toolbar** (built, [#442](https://github.com/suhina-daw/suhina-daw/issues/442), icons
   [#443](https://github.com/suhina-daw/suhina-daw/issues/443) and
   [#460](https://github.com/suhina-daw/suhina-daw/issues/460)): the tools, then the note
   clipboard, then the MIDI tools column's toggle and the **Audition** button, then the
   **workflow menu** (› Workflow modes), the **ghost menu** (› Ghost notes) and the **Scale
   menu** (› Scale and key, [#347](https://github.com/suhina-daw/suhina-daw/issues/347)), and at
   the right end the piano roll's grid
   value (› Grid and snapping), and after it, at the very end, the **×** that hides the editor
   (› Least height and closing). Each of the three menus takes the width its button needs, so the
   next starts right after it. A menu is shown only while there is room for it with the grid value
   still after it, and is left out otherwise; the grid value's **Grid** caption is left out first
   when the Scale menu needs the room (at 1280 px with the browser closed and the MIDI tools column
   open, all three menus and the grid value show, without the caption). **Later:** tabs for Notes,
   Envelopes and Expression.

   The tools are **Select**, **Draw**, **Paint**, **Erase**, **Slice** and **Mute** (› Drawing
   notes, › Muting), each an
   icon button: the shape is drawn in code with our own strokes ([`crates/ui/src/browser/browser_view.rs`](../../crates/ui/src/browser/browser_view.rs)
   › `icon` does the same for the browser), on a 14 px box, 1.1 px wide, in `text.secondary`; the
   chosen tool's is `text.primary` on `control.activeBackground`. A click chooses the tool; the
   number keys **1** to **6** do too, after a click in the piano roll (› Shortcuts). The pointer
   over a button shows its name and its current key, such as "Draw (2)", and the button has that
   name as its accessible label, so a reader and a test find it by name.

   The six tool shapes are drawn as an outline (the pointer is a filled arrow with a stem, the
   pencil and the brush a tip and a body, the eraser a slanted block with a lighter rubbing end,
   the blade a handle over a line, the mute tool a speaker with a cross beside it), so they are
   told apart at 14 px and at the larger toolbar sizes below. The pointer over the grid takes the
   tool's own shape where the window can show one: a crosshair for Draw and Paint, an **eraser**
   for Erase (the shape of the eraser button, not the no-entry sign it used before), a text cursor
   for Slice, an arrow for Select and Mute.

   **Piano roll toolbar size** is a setting, Settings › Clip editor › Piano roll › **Piano roll toolbar size**: **Normal**
   (100 %), **Large** (125 %, the default) and **Larger** (150 %). It scales the toolbar's height,
   every button's box and the icon strokes inside it together, so the icons stay 1.1 px at Normal
   and grow with the box; the rest of the editor (grid, notes, ruler) does not change with it. The
   toolbar's height is its tallest control either way, and a size too large for the toolbar's
   room is shown at the largest that fits, never clipped.

   After a divider, **Cut**, **Copy**, **Paste** and **Duplicate** icon buttons run what
   Ctrl+X, Ctrl+C, Ctrl+V and the duplicate key run (› Copy, cut, paste and duplicate) on the
   selected notes; they are greyed out (a `text.hint` shape, no highlight, no action) when there
   is nothing to act on: no note selected for Cut, Copy and Duplicate, nothing copied for Paste.
   The **MIDI tools** icon (three horizontal sliders) shows and hides the MIDI tools column
   (› Other note tools); it is highlighted while the column shows, and a click on it is window
   state only, not a command. The **Audition** icon (headphones, built
   [#351](https://github.com/suhina-daw/suhina-daw/issues/351)) after it turns the audition on
   and off (› Playing the keys); it is off when Suhina starts, highlighted while on, and a click
   on it is window state only, not a command. The workflow menu and the ghost menu are left out,
   the ghost menu first, when the toolbar is too narrow for them and the grid menu. The grid menu
   stays at the right end of the toolbar (› Grid and
   snapping); the quantise
   controls are in the clip's properties column (› Quantise).
2. **Ruler** (built, the loop row and the markers [#353](https://github.com/suhina-daw/suhina-daw/issues/353)):
   36 px tall. On top a 14 px **loop row**: a bar from the clip's loop start to its loop end with
   a **handle** at each end (named "Loop start" and "Loop end" for readers and tests), bright while
   the clip loops; while it does not, the bar is drawn dim where turning the loop on would put it
   (from 0 over the clip's length, as `clip.setLoop` with `loop: true` makes it). Under it the bar
   numbers and bar lines, and the clip's **start marker** (its offset: where in the content it
   starts playing, "Clip start marker") as a line with a flag pointing right, and, for a clip that
   does not loop, its **end marker** (the offset plus the length, "Clip end marker") with a flag
   pointing left. A looping clip shows no end marker: its loop's end is where its content ends,
   and its length on the arrangement is the arrangement's.
   - Dragging a handle moves that end of the loop, dragging the bar between the handles moves the
     loop and keeps its length, dragging a marker moves it; each lands on the piano roll's grid
     (› Grid and snapping: the nearest line at this zoom, or anywhere with snapping off), and
     anywhere with **Alt** held. The ruler shows the new place while it is dragged, and letting
     go runs one command, one undo step: `clip.setLoop {start, end}` for the loop (which also
     turns a loop that was off on), `clip.setStart {offset}` for the start marker,
     `clip.setLength {length}` for the end marker. A drag that changes nothing runs nothing.
   - The drag keeps every value the commands take, so a drag never fails: the loop never shorter
     than a grid step (a hundredth of a beat when it does not snap) and its ends never cross; the
     start marker before the loop's end, or before the end marker; the clip never shorter than a
     step and, on the arrangement, never longer than the room up to the next clip on its lane.
   - Moving the start marker of a clip that does not loop keeps its end marker where it is, so the
     clip gets longer or shorter by as much (an arrangement clip keeps its place on the
     arrangement); a looping clip keeps its length.
   - A click on the loop bar turns the loop on or off (`clip.setLoop {loop}`).
   - Within 4 px of a handle or a marker the pointer shows resizing, over the bar it grabs.
   - Dragging anywhere else on the ruler zooms and scrolls time (› Zoom and scroll).
3. **Key column and note grid** (built): one row per key, black-key rows darker than white-key
   rows, a stronger line under every C; bar lines stronger than grid lines. The editor spans the
   part of the content the clip plays: to the loop's end when the clip loops, else to its offset
   plus its length; the grid past that is shaded, and so is the grid before the part the clip
   plays (before its start marker, or before its loop's start when that is earlier). Notes in a
   shaded part are drawn at a little under half their opacity, so a note outside the loop reads
   as not playing; they stay notes like any other (selected, moved and listed as before). Notes are drawn in the track colour, their opacity rising
   with velocity; a selected note has a light outline; every note shows its name, cut at the block's edge when it
   does not fit.
   **Playhead** (built): while the transport runs, a 1 px line in the accent colour over the
   ruler, the grid and the velocity lane shows where playback is in the clip's content, mapped
   through the clip's start, offset and loop (a looping clip shows it going round its loop), in
   step with the Arrangement view's playhead. Outside the clip, or while stopped, there is none.
4. **Velocity lane** (built): under the grid, sharing its time axis, 72 px tall (less in a short
   detail area: at most 30 % of the room under the ruler, and never so tall that fewer than five
   key rows are left above it; when that leaves it under 12 px it is not drawn at all, and the
   key rows take its room). One stem per note at the
   note's start, its height the velocity; a selected stem is brighter. **Built
   ([#351](https://github.com/suhina-daw/suhina-daw/issues/351)):** the lane's **top edge** can be
   dragged, the pointer within 3 px of it showing a vertical resize cursor (its accessible label is
   "Velocity lane top edge"): up makes the lane taller, down shorter, from 24 px to 480 px and never
   so tall that fewer than five key rows are left above it, so the least height above still holds.
   The height is window state, kept until Suhina closes (not saved, not a command, not an undo
   step); until it is dragged the lane takes the default height above. **Later:** chord notes on
   the same start drawn side by side, a few pixels apart, so each can be grabbed; the lane's tools
   (randomise, ramp, deviation) ([#351](https://github.com/suhina-daw/suhina-daw/issues/351)).

Everything scales with the detail area: the detail area's splitter (exists) makes the editor taller
or shorter, and the key rows and the velocity lane keep their own heights while the visible range
of keys grows and shrinks.

### Least height and closing

**Built:** the smallest open piano roll is 120 px of detail area (in a row under the view): the
toolbar, the ruler and at least five key rows at the default 12 px row height, with the clip
column beside them showing its title (the clip's name) and its Track, Start, Length, Loop and
Notes rows whole. The splitter, dragged down, stops at that height. Above it the view keeps its
own least height (160 px), as before; with the detail area showing a track's devices instead the
detail area's least height stays 80 px.

The editor closes three ways, and each does the same as **View › Hide detail area** (Shift+Tab):
the detail area is hidden and the view takes the whole height, its height is kept for when it
comes back. It is window state, not a command, and not an undo step.

- **The splitter dragged down** past half the least height (60 px from the bottom with the
  editor, 40 px with devices): the detail area collapses to the 6 px splitter along the bottom
  of the window. While the same drag goes back up past that point, it opens again under the
  pointer. Once it is let go, the splitter stays at the bottom, and dragging it up opens the
  detail area again at the height the drag reaches (at least the least height).
- **The ×** at the right end of the piano roll's toolbar (its top row), named **Hide editor**,
  with its key in the tooltip.
- **View › Hide editor** (Ctrl+Shift+E, the keymap action `view.editor`), shown while a clip's
  editor is open; while it is hidden and a clip is selected the entry reads **Show editor** and
  opens it. Escape does not close the editor: in the piano roll it already clears the note
  selection (`pianoRoll.deselect`) and closes the quantise block, so the editor has a key of its
  own instead, one no other default uses.

After the editor is closed one of these ways, **selecting a clip** (a click on a clip in the
Arrangement view, another clip, or a slot's clip in the Launcher view) opens it again at the
height it had. A detail area hidden with Shift+Tab stays hidden when a clip is selected, as
before; Shift+Tab, View › Show detail area and the splitter open it either way. A click on an
empty spot of the Arrangement view clears the clip selection (› [Arrangement
editing](arrangement-editing.md) › Selecting clips); the detail area stays open and shows the
track's devices, or the hint.

### Pulled to the top

**Built ([#442](https://github.com/suhina-daw/suhina-daw/issues/442)):** while the piano roll shows a
clip under the Arrangement view, or an ARA plug-in's editor shows an audio clip in its place, its
splitter can be dragged all the way up. Past the view's least
height (160 px) the piano roll snaps to the top; dragged down again past that height, the view comes
back at the height the drag reaches. A double-click on the splitter closes the detail area, as a
double-click on a split view's divider does on macOS; a double-click on the closed splitter opens it
again at its default size.
What stays above it is a setting, Settings › Clip editor › Piano roll › **Pulled to the top**:

- **Edited track** (default): the Arrangement view keeps its ruler and the lane of the track whose
  clip is edited, lit (a lighter lane, an accent edge, its header shown as selected); no other
  track, no return tracks and no main track. The view's scroll position is kept for when it comes
  back. Its header keeps the track's number in the whole view. A click on an empty spot of its
  lane moves the play position and keeps the clip selected, so the editor stays up.
- **Whole window**: the piano roll fills the whole area under the control bar.

It stays pulled up while MIDI clips (or, with an ARA plug-in, audio clips) are edited; selecting
something else (another kind of clip, a track, nothing), hiding the detail area (Shift+Tab) or the Launcher view shows the view as before. With the
device chain as a column beside the view (Settings › Devices › Device panels) there is nothing to
pull up.

## Grid and snapping

- **Built ([#442](https://github.com/suhina-daw/suhina-daw/issues/442), [#346](https://github.com/suhina-daw/suhina-daw/issues/346)):**
  the piano roll has its own grid value; it lives in the window, not the project, and is a
  snapping 1/16 until another is picked. The control bar's grid value (the launch quantisation,
  `launcher.setQuantization`) applies to the Arrangement view only. The grid value draws the
  vertical lines and is the step that notes snap to. The lines and snapping are the
  Arrangement view's ([arrangement editing](arrangement-editing.md) › The grid), from the same code
  ([#734](https://github.com/suhina-daw/suhina-daw/issues/734)): lines start again at every bar
  line, and when they would be closer than 6 points the grid coarsens (a straight value doubles
  up to the beat, a triplet goes to the beat, then the bar, then bars double), so lines always
  land on bar lines, in 5/4 or 3/8 and with triplets too. Snapping uses the same lines, so what
  snaps is what is drawn. A move by dragging or by the arrow keys goes by whole steps, so a note
  off the grid stays as far off it.
- **Built ([#346](https://github.com/suhina-daw/suhina-daw/issues/346)): the grid menu.** The
  button at the right end of the toolbar, after the word **Grid**, shows the value (`1/16`,
  `1/8T`, `2 bars`, `Adaptive`, `Adaptive T`, with ` off` after it while snapping is off; its
  accessible label is `Grid value` and that text). It opens one menu that holds everything about
  the grid, in this order, each entry with its key on the right (the key the keymap has bound to
  it now, also in the entry's accessible label, such as `Narrow grid (Ctrl+1)`) and the chosen
  ones ticked:
  1. **Snap to grid** (Ctrl+4): on or off. Off, the lines are still drawn and nothing snaps:
     drawing, moving, resizing and slicing place notes freely, as with Alt held, and a new note
     takes the length of the note last drawn or clicked, else a 1/16.
  2. **Adaptive grid**: **Adaptive widest**, **wide**, **medium**, **narrow**, **narrowest**. The
     lines follow the zoom: the step is the finest note value of 1/32, 1/16, 1/8, 1/4 and 1/2
     (the ones shorter than a bar), then 1 bar, 2 bars, 4 bars and on doubling, whose lines are at
     least 96, 48, 24, 12 or 6 points apart (widest to narrowest). Zooming in makes it finer,
     zooming out coarser; it is never finer than 1/32.
  3. **Fixed grid**: **8 bars**, **4 bars**, **2 bars**, **1 bar**, **1/2**, **1/4**, **1/8**,
     **1/16**, **1/32**. A note value is that part of a 4/4 bar (1/4 is a beat); a bar is the time
     signature's bar.
  4. **Narrow grid** (Ctrl+1) and **Widen grid** (Ctrl+2): one fixed value finer or coarser, or
     one adaptive width narrower or wider; the finest and the coarsest stay where they are.
  5. **Triplet grid** (Ctrl+3): on or off. On, every note value is two thirds as long (1/8T is a
     third of a beat), adaptive ones too; bar values stay as they are.

  The keys work after a click in the piano roll, like its other keys (› Shortcuts). The grid value
  and its steps are the `piano_roll_grid` module of the `ui` crate (`crates/ui/src/detail/piano_roll_grid.rs`);
  the lines and snapping are the shared `TimelineGrid` (`crates/ui/src/interaction/timeline_grid.rs`).
- **Built:** Settings › Clip editor › Piano roll › **Piano roll grid source** set to **Control bar** makes the
  piano roll use the control bar's value instead, and its menu and keys then set the control
  bar's (`launcher.setQuantization`). The control bar has no adaptive grid and no 1/32 triplet,
  so an entry whose result it cannot hold is greyed out and its key does nothing: the adaptive
  widths always, and Triplet grid or Narrow grid where they would reach a 1/32 triplet. Snap to
  grid off is the control bar's `none` (the button shows `1/16 off`); picking a fixed value snaps
  again.
- **Choice left open:** the issue proposed the adaptive grid as the default; the default stays a
  snapping fixed 1/16, as before the menu, until that is decided.
- **Built:** starts and ends of drawn, moved and resized notes snap to the grid. Holding **Alt**
  while drawing or dragging places them freely (no snapping). A move keeps a note's distance to the
  grid: the drag snaps the **offset** to whole grid steps, so a note that was off the grid stays
  just as far off (it keeps a played-in groove).
- **Later:** the same grid menu in the Arrangement view, for its own grid.
- **Later ([#346](https://github.com/suhina-daw/suhina-daw/issues/346)):** "magnetic" snapping to other notes' starts and ends within a few pixels.

## Zoom and scroll

- **Built:** the mouse wheel scrolls up and down through keys; **Shift**+wheel scrolls in time;
  **Ctrl**+wheel zooms time around the pointer, the beat under it staying put even past the clip's end (#910); **Ctrl+Shift**+wheel or **Alt**+wheel (Option on
  macOS, as in the Arrangement view) zooms key height around the pointer. Dragging with the middle mouse button pans both ways. A newly shown clip opens scrolled
  so its notes are in view (the middle of its lowest and highest note; C3 for an empty clip) and
  zoomed so the whole clip fits the width.
- **Built:** the notes stay in view at any height. Until the key rows are scrolled by hand (the
  wheel, a middle-button pan, zooming the key height), every change in the editor's height (the
  splitter, the window, pulling it to the top, closing and opening it) centres the key rows on the
  notes again, so at the least height the five rows show the middle of the clip's notes, not the
  rows around C3 it opened at. Once the key rows have been scrolled by hand they stay where they
  were put until another clip is shown.
- **Built:** while the **right** mouse button is held down over the grid, each wheel step chooses
  the next tool, or with the wheel turned the other way the previous one, in the order Select →
  Draw → Paint → Erase → Slice → Mute, stopping at the first and the last tool (no wrapping round)
  (› Drawing notes for the tools). The wheel never scrolls or zooms while the right button is
  held, nor afterwards with what is left of the wheel's smoothed movement: scrolling comes back
  once the button is up and the wheel has stopped.
  The toolbar's highlight moves to the new tool at once and the pointer takes the new tool's
  cursor. Letting the right button go opens no menu, adds no note and changes no selection.
- **Built:** key rows are 6 to 32 px tall (default 12 px); time is 4 to 400 px per beat.
- **Built ([#353](https://github.com/suhina-daw/suhina-daw/issues/353)):** dragging up and down
  on the ruler (away from its handles and markers) zooms time around the pointer, down zooming in
  and up zooming out, and dragging sideways scrolls time with the pointer; the beat under the
  pointer stays under it. **Z** zooms and scrolls time so the selected notes fill the grid's width
  (the whole clip when no note is selected), **Shift+Z** so the whole clip does (from beat 0 to
  the end of the part it plays), both within the zoom range and leaving the key rows as they are.
  Both keys work after a click in the piano roll (`pianoRoll.zoomSelection`, `pianoRoll.zoomClip`)
  and change the view only, not the project.
- **Built ([#347](https://github.com/suhina-daw/suhina-daw/issues/347)):** **Fold** and **Fold
  to scale**, in the Scale menu (› Scale and key), show fewer key rows. Fold shows only the keys
  that have notes in the clip (every key while it has none); Fold to scale shows the keys of the
  clip's scale and any key that has a note, so no note is ever hidden (greyed out while the clip
  has no scale). The two are one choice: picking one turns the other off, picking the ticked one
  again shows every key. The rows sit one under the other with no gap for the keys left out, and
  every row of the key column is named while folded, since the keys beside each other are no
  longer neighbours. Everything that turns a height into a key goes through the rows shown: a
  click or double-click lands on the row's key, painting and the key column play and select that
  key, and a note dragged up or down steps through the shown rows (the grabbed note lands on the
  key of the row under the pointer, and the other selected notes move by as many semitones, which
  may put one on a key that was not shown: its row appears, as Fold shows every key with notes).
  The arrow keys still move notes by a semitone or an octave. Ghost notes on a key that is not
  shown are not drawn. Picking a fold centres the notes again. It is window state, kept until
  Suhina closes and the same for every clip: not a command, not saved, not an undo step.
- **Built:** a drag up or down in the scale strip left of the keys changes the row height, the row
  under the pointer staying where it is (› Key column and scale strip).

## Drawing notes

- **Built:** a **double-click** on an empty spot of the grid adds a note there: its start snapped
  down to the grid line before the pointer (Alt: where the pointer is), its key the row under the
  pointer, its length one grid step (a quarter beat with `none`), velocity 100. The new note is
  selected. A double-click on a note deletes it. Both are workflow settings, and what a
  double-click does with every tool and in every part of the editor is › Double-click.
- **Built:** the toolbar's tools ([#442](https://github.com/suhina-daw/suhina-daw/issues/442),
  Slice [#443](https://github.com/suhina-daw/suhina-daw/issues/443)) decide
  what a press in the grid does (the velocity lane works the same with every tool):
  - **Select** (default): as above and in the sections below.
  - **Draw**: a click on an empty spot adds a note there; pressing and dragging sideways sets its
    length, its end on the grid (Alt: free), at least one grid step. A click on a note selects it,
    and dragging a note moves or resizes it as with Select. A double-click does nothing more.
  - **Draw with Shift held** draws notes **back to back**: the drag stays in the row it started in,
    and the notes it draws follow one another from the press position, each as long as the note
    last drawn or clicked (one grid step until then), the first starting where the press was
    (snapped; Alt: where the pointer is). The drag's end decides when to stop: notes are laid down
    while their end is before it, the last one as long as the others. **Draw with Shift and Paint
    differ in two ways:** Shift+Draw uses the last note's length while Paint uses the length of
    the note the stroke started with, and Shift+Draw keeps the row the drag started in while Paint
    follows the pointer into other rows. Both are one `note.add`, one undo step, and both select
    the notes they added.
  - **Paint**: pressing and dragging paints notes one note length apart, each in the row under the
    pointer at that moment, from where the press was in both directions; a slot already painted
    in the stroke, or holding a note of the same key at the same start, is skipped. The stroke is
    one `note.add`, one undo step, and the painted notes are selected. A click adds one note.
  - **Erase**: a click on a note deletes it; dragging deletes every note the pointer passes over
    (they fade while the drag lasts), one `note.delete`, one undo step.
  - **Slice**: pressing and dragging draws a cut line across the grid; when it is let go, every
    note the line crosses is split where the line crosses it, the cut on the grid (Alt: where the
    line is). A cut that lands on a note's start or end leaves that note alone; a vertical drag
    that crosses no note does nothing. The whole stroke is one `note.split`, one undo step, and
    the notes the cut made are selected. A click (no drag) does nothing.
  - **Mute** (built,
    [#460](https://github.com/suhina-daw/suhina-daw/issues/460)): a click on a note mutes it, or
    unmutes it when it is already muted; pressing and dragging sets every note the pointer passes
    over to the state the first note the stroke touched takes, so a stroke that starts on a muted
    note unmutes the run. The stroke is one `note.setMuted`, one undo step (› Muting). A click or
    drag on an empty spot does nothing. The tool works on the note under the
    pointer whatever the selection is, and never changes the selection, so a run can be muted
    without losing what was selected.
  - **B** (`pianoRoll.draw`, after a click in the piano roll) switches between Select and Draw. The
    number keys **1** to **6** (`pianoRoll.tool1` … `pianoRoll.tool6`) choose Select, Draw, Paint,
    Erase, Slice and Mute, read only while the piano roll has the keyboard, so the number keys keep
    their meaning elsewhere in the window.
  - A new note takes the length and velocity of the note last drawn or clicked (one grid step and
    100 until then), so repeated drawing keeps one feel. The cursor shows the tool (› Layout): a
    crosshair to draw or paint, an eraser to erase, a text cursor to slice, an arrow for Select and
    Mute.
- **Later ([#351](https://github.com/suhina-daw/suhina-daw/issues/351)):** dragging up or down while
  drawing sets the length.

## Double-click

**Built ([#490](https://github.com/suhina-daw/suhina-daw/issues/490)).** A double-click is two
presses of the left button in the piano roll, the second within 0.3 s of the first and within 6
points of it. The piano roll counts them itself, from its own clicks: a click anywhere else just
before (selecting the clip, a button) never turns the pair into something else, and a third quick
click starts a new pair rather than being read as a triple click. The first click of the pair
always does what a single click does; the table says what the second one adds.

| Where | Tool | The second click |
|---|---|---|
| Empty spot of the grid | **Select** | adds a note there (› Drawing notes) when **Double-click on empty grid** is **Adds a note** (every preset); with **Does nothing** it clears the selection, as a click does. When **Click on empty grid** is **Draws a note** (Mouse-first) the first click has already added the note, so the second one only selects it and never deletes it |
| Empty spot of the grid | **Draw**, **Paint** | selects the note the first click added: one note, one undo step |
| Empty spot of the grid | **Erase**, **Slice**, **Mute** | nothing |
| A note, or its right edge | **Select** | deletes the note when **Double-click on a note** is **Deletes it** (every preset), else selects it alone, as a click does. A note the first click of the pair added is never deleted |
| A note, or its right edge | **Draw**, **Paint**, **Slice** | selects it, as a click does |
| A note, or its right edge | **Erase** | nothing: the first click deleted the note, and the second does not delete the note under it, so a double-click deletes one note |
| A note, or its right edge | **Mute** | nothing: the first click muted (or unmuted) the note, and the second does not flip it back, so a double-click mutes a note in one undo step |
| Velocity lane | any | selects the stem's note, as a click does; nothing else |
| Key column | any | plays the key and selects its notes again, as a click does; nothing is added |

A double-click is never a drag: a pair whose second press moves further than egui's click distance
is a drag and does what the tool's drag does. Every edit a double-click makes is one command and
one undo step, so Ctrl+Z (Cmd+Z on macOS) takes back exactly what it did.

## Playing the keys

- **Built ([#443](https://github.com/suhina-daw/suhina-daw/issues/443),
  [#1117](https://github.com/suhina-daw/suhina-daw/issues/1117)):** pressing a key in the key
  column plays that note through the track's instrument for as long as the button is held: the note
  goes on at the press and off at the release. Dragging up and down plays the key under the pointer,
  one at a time: the key the pointer was on goes off and the new one goes on. The key that is held
  is drawn in the accent colour. Nothing is added to the project or to the undo history; a press
  that plays no note (no audio engine attached) changes nothing else either.
- The sound is a **transport**-kind command, `preview.noteOn` and `preview.noteOff` (› Commands):
  it goes to the engine the window is attached to (ADR 0011), the same path the browser's preview
  takes, and it never changes the project. The UI plays nothing when there is no engine.
- A press or a drag in the key column also selects: the notes of every key from the press to the
  pointer, the range growing and shrinking with the drag and skipping no key, however fast it is
  (in a folded view, the keys it shows). **Shift** adds another range to the selection, so two ranges
  with a gap between them can be selected; **Ctrl** selects without sounding anything. These change
  only the window's selection (› Selecting). The range shows on the keys and across the note rows,
  with a dashed edge, while the drag lasts; once let go the notes alone show what is selected, ready
  for another range or Shift+click.
- **Audition (built, [#351](https://github.com/suhina-daw/suhina-daw/issues/351),
  [#939](https://github.com/suhina-daw/suhina-daw/issues/939),
  [#1116](https://github.com/suhina-daw/suhina-daw/issues/1116)):** with the toolbar's **Audition**
  button on, a note sounds through the track's instrument at the press that takes it or is about to
  draw it, not when the button is let go, and for as long as the button is held. A drag sounds the
  notes at each new pitch they reach, and moving them in time only sounds nothing new. A note added
  to the selection (a click, Shift+click) sounds at once, so notes sound in the order they were
  picked. Notes a rectangle selects sound together when it is let go, with no delay between them,
  and a drag on the keys sounds each key as it reaches it. A key sounds once, however many of its
  notes were selected. Notes
  moved by the arrow keys and those a stroke drew sound where they are. Each sounding is one
  `preview.noteOn` with its keys (at most 16, each once) at the velocity a new note takes, and a
  `preview.noteOff` 0.3 s after the last reason to sound. The window counts the time; nothing on the
  audio thread waits. A new audition lets the last one go first; a key pressed in the key column
  replaces it; hiding the editor or showing no clip lets it go at once. Off (the default), editing
  makes no sound. Like the key column it changes neither the project nor the undo history.

## Key column and scale strip

- **Built ([#1118](https://github.com/suhina-daw/suhina-daw/issues/1118),
  [#1119](https://github.com/suhina-daw/suhina-daw/issues/1119),
  [#1120](https://github.com/suhina-daw/suhina-daw/issues/1120)):** from the left: the scale strip, the
  key column and the note grid.
- The **key column** looks like a keyboard. Black keys are two thirds as wide as the column and lie
  on the white keys; a white key runs unbroken past the tip of a black neighbour, up to the middle of
  its row, and white keys meet in a line only between E and F and between B and C. A C stands out
  over its whole shape. A row belongs to its own key along its whole width, so the tip of a black
  key's row plays and selects that black key. A folded view draws plain rows, since the rows beside
  each other are not neighbouring keys.
- **Key names:** only C (the default), the white keys or all keys. A folded view names every row;
  rows lower than 8 points name none.
- **Key style:** Bone (light, the default), Stone (mid) and Night (dark). A theme's sheet sets
  anything about the keys over the chosen style (`docs/spec/theme-skins.md`); when it sets their
  colours, the menu shows **Theme** in place of a choice.
- The **scale strip** always names the octaves. The name of the row under the pointer, in the grid
  or on the keys, shows at that row, the row itself a little lighter in the grid. Over the strip the
  pointer is a magnifier and no row is named, and a drag changes the row height (› Zoom and scroll).
  The ruler shows the same magnifier where a drag on it zooms.
- The **View** menu in the corner over the keys holds the key style, the key names and **Allow
  stacked notes**, and keeps them in the settings, where the same choices are on the Clip editor
  page.

## Selecting

- **Built:** a click on a note selects it alone; **Shift**+click (or Ctrl+click) adds it to the
  selection or takes it out. A click on an empty spot of the grid clears the selection. Dragging
  from an empty spot draws a rectangle; when it is let go, the notes it touches are selected
  (with Shift, added to the selection).
- **Built:** the selection lives in the window only; it is not part of the project and is not an
  undo step. It is cleared when another clip is shown.
- **Built ([#344](https://github.com/suhina-daw/suhina-daw/issues/344)):** while the piano roll has
  the keyboard (after a click in it), **Ctrl+A** selects every note of the clip and **Escape** clears
  the selection. Elsewhere Escape keeps its other uses.
- **Later:** a click on the ruler selects a time range, and every note starting in it; a lasso.
  A press or a drag on the key column, which selects the notes of a key or a range of keys, is
  built (› Playing the keys).

## Moving

- **Built:** dragging a selected note (or any note, which selects it first unless it is already in
  the selection) moves the whole selection: sideways in time, snapped by whole grid steps (Alt:
  freely), up and down by semitones. The notes are shown where they go while dragging; the move is
  one command, one undo step, when the mouse is let go. A move that would put a note before the
  content's start or outside keys 0 to 127 stops at that limit, for the whole selection, so the
  selection keeps its shape.
- **Built ([#344](https://github.com/suhina-daw/suhina-daw/issues/344)):** while the piano roll has
  the keyboard, **Left** and **Right** move the selection one grid step earlier or later (the grid
  as drawn; a beat with `none`), **Up** and **Down** a semitone, **Shift+Up** and **Shift+Down** an
  octave. Each press is one `note.move`, one undo step. A step in time that would cross the
  content's start stops at it; a pitch move that does not fit for every selected note does nothing,
  so no note changes its name.
- **Built ([#351](https://github.com/suhina-daw/suhina-daw/issues/351)):** with **Ctrl** (Cmd on
  macOS) held when a note drag starts, the drag copies the selection instead of moving it (a note
  not yet selected is added to the selection first, as Ctrl+click does): the pointer shows a copy
  cursor, the notes are shown where the copies go, snapped as a move is, and letting go leaves the
  notes where they were and adds the copies there with one `note.duplicate` (`beats` and
  `semitones`), one undo step. The copies keep everything of their notes (length, velocity, mute)
  and end up selected. Dropped where they started, nothing is copied. Ctrl only counts at the start;
  pressing it later changes nothing.
- **Built:** **Alt** pressed once a move has started frees it from the grid for as long as it is
  held (it is read every frame). Alt held when the drag starts on a note changes its velocity
  instead (› Velocity).
- **Later:** whether moving notes past the clip's end extends the clip is **(to decide)**.

## Resizing

- **Built:** the pointer over the last few pixels of a note shows a resize cursor; dragging there
  changes the length of every selected note by the same amount, the dragged note's end snapping to
  the grid (Alt: freely). Dragged onto or before its own start, the dragged note's end goes to the
  first grid line after its start; no selected note gets shorter than a hundredth of a beat.
- **Built ([#344](https://github.com/suhina-daw/suhina-daw/issues/344),
  [#1115](https://github.com/suhina-daw/suhina-daw/issues/1115)):** **Shift+Right** takes each selected
  note's end to the next grid line, or to the start of the next note of its key when that comes first;
  a note already touching the next one stays, and with **Allow stacked notes** on the next note stops
  nothing. **Shift+Left** takes each end to the previous grid line, and no note gets shorter than one
  step. One `note.resize` with `lengths`, one undo step; with snapping off, a whole step either way.
- **Built ([#1113](https://github.com/suhina-daw/suhina-daw/issues/1113)):** with **Allow stacked
  notes** off (the default), a note drawn, moved, resized, pasted, duplicated, quantised or made
  legato replaces the notes of its key under it: a note it covers whole is removed, one that starts
  before it ends where it starts, and one that runs on past it starts where it ends. On, notes of a
  key may overlap. The commands take this as `replace`, so the CLI and MCP choose too; left out,
  notes may overlap.
- **Built ([#351](https://github.com/suhina-daw/suhina-daw/issues/351)):** the pointer over the
  first few pixels of a note shows a resize cursor too; dragging there moves the start of every
  selected note by the same amount and keeps each end. The dragged note's start snaps to the grid
  (Alt: freely); dragged onto or past its end it goes to the last grid line before the end. No
  start goes before beat 0 and no note gets shorter than a hundredth of a beat, for the whole
  selection. Letting go is one `note.resize` with `startBy`, one undo step. Each edge zone is
  5 px, at most a third of the note's width, so a small note keeps a middle that moves it.
- **Later:** a modifier while resizing stretches the selection in time around its start (scale the
  positions as well as the lengths).

## Deleting

- **Built:** **Delete** or **Backspace** while the piano roll has the keyboard (after a click in
  it) deletes the selected notes, one undo step. Without a note selected, those keys keep doing
  what they do elsewhere (delete the selected clip). A double-click on a note deletes it.
- **Later ([#351](https://github.com/suhina-daw/suhina-daw/issues/351)):** a drag with the right
  button deletes notes. A **right-click** on a note deletes it in the Draw and Paint tools when
  the **Draw-first** keymap preset is chosen (› Keymap presets), and in every tool when the
  **Mouse-first** preset's **Right click deletes a note** switch is on; otherwise it does nothing.

## Muting

- **Built ([#460](https://github.com/suhina-daw/suhina-daw/issues/460)):** a muted note stays in
  the clip and does not play: the engine emits no voice for it, exactly as if the note were not
  there, through the arrangement, the launcher and an offline render. It keeps its time, length,
  key and velocity.
- The **Mute** tool (key **6**) mutes and unmutes the note under the pointer (› Drawing notes); a
  click toggles one note, and a drag sets every note it passes over to the state the first note the
  stroke touched takes, so a run is muted in one go without each note flipping on its own. The drag
  is one stroke, so it is one `note.setMuted`, one undo step: Ctrl+Z (Cmd+Z on macOS) puts the whole
  stroke back at once.
- A muted note is drawn as the note it is, with a faded body, a dashed edge and no velocity stem
  in the lane standing out (its stem is drawn faint), so it is clearly still there and clearly not
  playing. The note's name and its position on the grid are unchanged.
- Mute is part of the project ([ADR 0001](../decisions/0001-daw-neutral-model.md) › Amendment:
  muted notes): saving writes `"muted": true` on those notes only, so a project without muted
  notes is written as before, and reading a file from before this reads its notes as unmuted. A
  muted note stays muted after the project is saved and opened again, and a Live Set's note marked
  `IsEnabled="false"` is read as a muted note instead of being dropped
  ([.als format](als-format.md) › Notes).
- Copying, cutting, pasting and duplicating a note carries its mute with it; `note.setMuted` is the
  only command that changes it. The MIDI tools that change notes (transpose, reverse and the rest,
  › Other note tools) keep each note's mute, and act on muted notes as they do on the others, so a
  part can be muted, moved and unmuted without the mute being lost.
- **Later ([#351](https://github.com/suhina-daw/suhina-daw/issues/351)):** per-note probability and
  velocity range; muting a whole clip or a time range of the grid.

## Copy, cut, paste and duplicate

- **Built ([#344](https://github.com/suhina-daw/suhina-daw/issues/344)):** while the piano roll has
  the keyboard, **Ctrl+C** copies the selected notes (`note.copy`), **Ctrl+X** cuts them
  (`note.cut`: copies and deletes, one undo step), **Ctrl+V** pastes the copied notes
  (`note.paste`) and the duplicate key duplicates the selected notes (`note.duplicate`). The
  duplicate key is **Ctrl+B** with the **Draw-first** keymap preset and **Ctrl+D** with
  **Select-first**, the default (› Keymap presets); it can be changed like any other key.
  With no note
  selected (or, for Ctrl+V, no notes copied) the keys act on the selected clip, as elsewhere.
- The keys are Ctrl on Windows and Linux and Cmd on macOS. The window reports them (and the other
  system keys for the same thing: Shift+Delete, Ctrl+Insert and Shift+Insert on Windows, and the
  Cut, Copy and Paste keys some keyboards have) as cut, copy and paste rather than as keys; the
  keymap reads them as Ctrl+X, Ctrl+C and Ctrl+V. The window reports a paste only while the
  system clipboard holds text, so copying or cutting notes or a clip in Suhina also puts a short
  line ("Suhina: notes copied") on the system clipboard.
- The toolbar's Cut, Copy, Paste and Duplicate icon buttons (› Layout) run these same commands on
  the selected notes, greyed out when there are none; Ctrl+X and Ctrl+C keep working whichever
  preset is chosen.
- Copied notes keep their spacing, their keys, lengths and velocities. Paste puts the first of them
  right after the end of the selected notes; with none selected, at the playhead when it is inside
  the clip (a looping clip maps it into its loop); else right after where they were copied from.
  Duplicate puts the copy right after the selection's end (the selection spans from its earliest
  start to its latest end). Pasted and duplicated notes are selected; the notes they land on stay.
- The note clipboard belongs to the open project's session, apart from the clip clipboard; notes
  copied in one MIDI clip paste into any other. It is not saved with the project.

## Velocity

- **Built:** in the velocity lane, a note's bar is grabbed anywhere over the note's width (and a
  few pixels before its start, where its stem is drawn); dragging it sets the note's velocity to
  the height of the pointer in the lane (the top is 127, the bottom 1). With several notes
  selected and the dragged one among them, the others change by the same amount, each kept within
  1 to 127. The lane shows the values while dragging; letting go is one `note.setVelocity`, one
  undo step.
- **Built ([#351](https://github.com/suhina-daw/suhina-daw/issues/351)):** **Alt+drag** up or down
  on a note (Option on macOS; Alt held when the drag starts, on the note's body, with the Select or
  Draw tool) changes its velocity, one step per point, up louder; with several notes selected and
  the dragged one among them, every selected note changes by the same amount, the dragged one kept
  within 1 to 127. While dragging, the note's new velocity is shown over it ("Velocity 112"), the
  notes and their stems follow, and the note does not move; letting go is one
  `note.setVelocity` with `by`, one undo step.
- **Built ([#351](https://github.com/suhina-daw/suhina-daw/issues/351)):** a drag in the velocity
  lane that starts where no bar can be grabbed (or any drag in the lane with Alt held, for notes
  that leave no gap) draws a **line**; every stem whose note starts between the line's two ends in
  time takes the line's height at that start, whether selected or not. The stems follow while
  dragging; letting go is one `note.setVelocity` with `velocities` (one value per note), one undo
  step. A line that spans no stem does nothing.
- **Later ([#351](https://github.com/suhina-daw/suhina-daw/issues/351)):** release velocity and a
  note's probability and velocity range (a per-note chance to play and a random variation), shown
  as a second lane; randomise, ramp and deviation tools.

## Quantise

- **Built ([#345](https://github.com/suhina-daw/suhina-daw/issues/345), in the clip column
  [#443](https://github.com/suhina-daw/suhina-daw/issues/443), moved to the MIDI tools column
  [#460](https://github.com/suhina-daw/suhina-daw/issues/460)):** Ctrl+U quantises the
  selected notes, or every note of the clip when none is selected, with the last settings, at
  once. The quantise controls are the **Quantise** block at the top of the MIDI tools column
  (› Other note tools), on the left of the note grid: **Grid** (1/32 to 8 bars,
  triplets too; 1/16 at first), **Amount** (0 to 100 %, 100 at first) and **Move ends too** (off
  at first), and the buttons **Quantise notes** (quantises now) and **Close**. The clip's own
  properties (Track, Start, Length, Loop, Notes) stay in the properties column beside it. Nothing
  floats over the note grid.
- Escape and **Close** close the block, and the properties move back up. A press outside it does
  not close it (the block is part of the column, not a pop-up). The block's settings are kept for
  Ctrl+U until Suhina closes.
- Each start moves towards the nearest grid line by the amount: 100 % lands on the line, 50 %
  goes half way, 0 % changes nothing. A note already on a line keeps its exact start. A tie
  between two lines goes to the later one. Without ends a note keeps its length; with ends its
  end moves towards the nearest line the same way, and when that line is on or before the
  quantised start, to the first line after it, so a note never becomes zero long.
- The block's grid is its own and does not follow the piano roll's grid value (nor the control
  bar's), so snapping while drawing and quantising can differ. Bars are as long as the project's time signature
  makes them.
- The edit is `note.quantize` (see Commands), one undo step, so it also runs from the CLI and MCP.

### Clip column

- **Built ([#443](https://github.com/suhina-daw/suhina-daw/issues/443)):** the clip properties
  column is the same column in every layout: Track, Start, Length, Loop and (for a MIDI clip)
  Notes, or the audio clip's Gain and fades. With the detail area stacked (a column at the right of
  the window) it is a block at the top and the piano roll sits under it, as before.
- **Several clips picked** in the Arrangement: the column's title says how many ("2 selected")
  and its rows are the last picked clip's; its Loop switch turns every picked clip's loop on or
  off with it. With several MIDI clips picked (#1054) the piano roll edits one of them, the one
  with the focus (the last picked); the others' notes are drawn faint where they play, as ghost
  notes are, and the view runs on over them. The column lists the picked clips, the one being
  edited marked **Editing**; a click on another gives it the focus. Not yet: a strip of the
  clips over the piano roll and a click on a faint note to switch.
- **Built ([#699](https://github.com/suhina-daw/suhina-daw/issues/699)):** the column edits the
  clip, each change one undo step:
  - **Loop** is a switch (`clip.setLoop`). An audio clip that loops has **Loop start** and **Loop
    end** rows too, dragged sideways by quarter beats, a quarter beat apart at least; a MIDI clip's
    loop is dragged on the piano roll's ruler instead, so its column keeps room for the selection.
  - An audio clip has a **Start marker**, where in its audio it starts playing (`clip.setStart`),
    and, when it does not loop, an **End marker**, where it stops (`clip.setLength` from the start
    marker); both are dragged by quarter beats in content beats, a quarter beat apart at least.
    A looping clip's end is its length, set on its edge.
  - An audio clip has **Gain** (dragged by tenths of a dB, a double-click puts it at 0 dB;
    `clip.setGain`, an arrangement clip's or a slot's), **Warp** (a switch), **Mode** (a click steps through
    Re-pitch, Rhythmic, Tonal, Texture, Smooth and High quality) and **Transpose** (dragged by
    semitones, -48 to +48, a double-click puts it at 0), all through `clip.setAudio`. Warp turned
    on for audio without warp markers gives it two, at its start and its end, so it plays as it did
    and then follows the tempo. Those markers fit the tempo where the clip plays, so a clip whose
    audio another clip plays where the tempo differs gets its own copy of it first, and the other
    clip plays as it did ([#874](https://github.com/suhina-daw/suhina-daw/issues/874)). Warp off
    plays the file at its own speed whatever the tempo (a transpose repitches it): the clip then
    spans what its seconds take at the song's tempo, and changing the tempo makes it span more or
    fewer beats at once (a 4-beat file at 124 BPM is 1.935 s, 2.903 beats at 90 BPM; warped to
    the song it is 4 beats). Whenever warp, a repitching transpose or the tempo changes what a
    clip's beats mean in its file, every clip that plays it keeps its start and keeps playing the
    same part of the file: its offset, length and loop are worked out again, so the waveform, the
    end and the Length row agree with what is heard, and undo puts both back (#830).
  - A drag shows its value as it goes and sends one command when it is let go.
- **Built ([#460](https://github.com/suhina-daw/suhina-daw/issues/460)):** the **Notes** row reads
  **"N of M"** while notes are selected, where N is how many are selected and M how many the clip
  has, and just **M** with nothing selected. Under it, while a selection exists, come the rows that
  describe it, in this order and only when they say something:
  - **From** and **To**: the selection's span in the clip's beats, as the ruler writes them (bar
    and beat, such as `2.1` and `2.3`), from the earliest start to the latest end. They describe
    the notes as they are on the grid, so a drag shows the values it would land on only when it is
    let go.
  - **Pitch**: the range of the selection's keys, by name, such as `C3` for one key or `C3–G3` for
    several (the lowest and the highest, in that order).
  - **Velocity**: the range of the selection's velocities as 1 to 127, such as `64` or `64–127`.
  - A selection of muted notes says how many are muted in the Notes row, such as `2 of 4, 1 muted`,
    so a run muted with the Mute tool is visible in the column as well as on the grid.
- **Built:** the column never wraps a value onto more lines, at any height or width. A label or a
  value too long for its row is shortened with an ellipsis; rows that do not fit under the
  column's bottom are left out, from the last one up, rather than drawn cut in half, and the title
  goes last. The same holds for the MIDI tools column beside it: its buttons and fields shorten
  their text instead of wrapping it, and the column is left out when its head does not fit.
- When the quantise block is open it takes the column's top: its title **Quantise**, then Grid,
  Amount, the checkbox and the two buttons, and the clip's own properties are drawn under it. The
  space the block needs is given to the column for that frame; closing the block gives it back.
- The selection rows are window state, like the selection itself: not part of the project, not an
  undo step, and gone when the selection is cleared or another clip is shown (› Selecting).
- **Built ([#460](https://github.com/suhina-daw/suhina-daw/issues/460)):** the MIDI tools are their
  own column beside this one (› Other note tools), drawn between the properties and the editor; a
  later detail-area block takes its place in this column's list of blocks rather than changing its
  shape.
- **Later:** swing; quantise while recording (record quantisation) belongs to recording, which has
  its own issue.

## Scale and key

**Built ([#347](https://github.com/suhina-daw/suhina-daw/issues/347)).** A MIDI clip can have a
scale: a root (C to B) and a mode: major, minor, harmonic minor, melodic minor, the church modes
(dorian, phrygian, lydian, mixolydian, locrian), major and minor pentatonic, and blues. The scale
belongs to the clip's content ([ADR 0001](../decisions/0001-daw-neutral-model.md) › Amendment:
clip scales) and is saved with the project; a clip has none until one is set.

- **The Scale menu** in the toolbar, after the ghost menu (› Layout), reads **Scale**, dimmed while
  the clip has none; its accessible label and its tooltip name the scale ("Scale D dorian", or
  "Scale off"). Its entries: **Off**; a **Root** submenu ("Root: D") with the twelve roots; a
  **Mode** submenu ("Mode: Dorian") with the modes; then **Fold** and **Fold to scale** (› Zoom and
  scroll). The chosen ones are ticked. Off, a root and a mode each run `clip.setScale`, one undo
  step (Ctrl+Z takes it back): a root keeps the mode the clip has (major when it has none), a mode
  keeps the root (C when it has none).
- **The rows of the keys in the scale are lighter** than the others (the root's lighter still),
  in place of the white-key and black-key rows a clip without a scale shows; each key in the scale
  has a 2 px mark at the right edge of the key column. While the clip has a
  scale, each row of the key column in view is named for readers and tests: "Key E3 in scale", or
  "Key F3" for a key outside it.
- **A shared content.** Setting the scale through a clip whose content other clips also play gives
  that clip its own copy of the content first, as a note edit does (› Coordinates): the other
  clips keep theirs, and one undo takes back both. A shared pattern stays one content.
- **The project's default scale** (`project.setDefaultScale`, also in `project.info`) is the
  scale a new MIDI clip takes: one `clip.create` makes, a recorded take and a MIDI file dropped
  on a track. Clips that exist keep theirs; a new audio clip has none. There is no control for it
  in the window yet; agents and the CLI set it.
- **Fit to scale** (`note.fitToScale`, › Other note tools) fits to the clip's own scale when the
  call names neither a root nor a scale; the MIDI tools column always names the root and mode in
  its own fields.
- **Later:** drawing that snaps to the nearest key of the scale (a toggle); the MIDI tools
  column's Fit to scale fields following the clip's scale; a control for the default scale;
  reading a Live Set clip's scale where the file has one.

## Ghost notes

**Built ([#348](https://github.com/suhina-daw/suhina-daw/issues/348)).** The notes of other clips
are drawn faintly behind the edited clip's own notes and cannot be edited, so a part can be
written against another (a bass line against the chords).

- **The ghost menu** in the toolbar, after the workflow menu (› Layout), reads **Ghosts**, dimmed
  while nothing is shown; its accessible label and its tooltip name what is shown ("Ghost notes
  Playing together"). Its entries, the chosen one ticked:
  - **None**: no ghost notes.
  - **Playing together** (the default): the clips that play at the same time as the edited clip.
    For an arrangement clip, every other arrangement clip, on any track, that plays during the
    edited clip's first pass (below); a clip that is turned off is left out. For a Launcher slot's
    clip, the clips in the other slots of the same scene: launching the scene starts them
    together, so each is laid out from the same launch (a looping slot clip goes on for ever, one
    that does not loop stops after its length).
  - **This track**: the other clips of the edited clip's track (the other clips on its lane for an
    arrangement clip, the other slots of its column for a slot's clip), each laid out as if it
    started together with the edited clip.
  - **Clip…**, a submenu listing every other MIDI clip of the project by its name, track and place
    ("Walk on Bass at bar 17", "Walk on Bass in scene 2"; at most 500): one picked clip, on any
    track, laid out as if it started together with the edited clip. Picking the edited clip itself
    is not offered.

  The choice is window state, kept until Suhina closes and across the clips shown: not a command,
  not saved with the project and not an undo step. A picked arrangement clip is named by its
  track and its id, so it stays picked when other clips on the lane move; a slot's clip by its
  slot. When that clip or slot holds nothing any more, nothing is shown.
- **The time mapping.** Each other clip is laid out in time through its own start, offset and loop,
  as the engine plays it: from its start it plays its content from its offset; a looping clip
  plays on to its loop's end and then round the loop for the rest of its length (an offset at or
  past the loop's end starts where it falls inside the loop). Only the part of each note that
  sounds while the edited clip plays is drawn: a note held across the edited clip's start, a
  loop's end or the other clip's end is cut there. The edited clip's own time is its **first
  pass**: from its start until its content first reaches its loop's end, or its whole length when
  it does not loop. A moment of that time maps to the content beat the edited clip plays then (its
  offset plus the beats since its start), so a looping edited clip shows what plays with its
  first time round, not every pass on top of each other. Muted notes are not drawn.
- **Drawing.** Ghost notes are drawn under the clip's own notes and over the grid's rows, as
  plain rectangles in their own track's colour at 25 % opacity: no name, no edge, no velocity
  stem. They are never hit-tested: a click, a double-click or a drag on a ghost note does what it
  does on the empty grid (a click with the Select tool clears the selection, a double-click adds a
  note there), and a ghost is never selected, moved or listed by `note.list`. For readers and
  tests each ghost in view is labelled "Ghost note E1 at beat 0 from Bass" (key, content beat,
  track), a label that only describes and senses nothing.
- **Limits.** At most 20 000 ghost notes are mapped for one clip and at most 512 of those in view
  are labelled, so a large project never draws or maps without end.
- **Later:** ghost notes for an audio clip's editor; a colour or opacity setting.

## Slicing and gluing

- **Built ([#443](https://github.com/suhina-daw/suhina-daw/issues/443)):** the **Slice** tool (›
  Drawing notes) splits the notes a drawn cut line crosses, and `note.split` (see Commands) does
  the same for named notes at one beat, so the CLI and MCP can split too. One stroke is one
  command, one undo step.
- **Later ([#349](https://github.com/suhina-daw/suhina-daw/issues/349)):** a command that chops the
  selection into notes of one grid step each; glue (join) touching notes of the same key into one
  note (Ctrl+G); legato (each note lasts until the next one starts); the command `note.join`.

## Chords

- **Later ([#350](https://github.com/suhina-daw/suhina-daw/issues/350)):** a chord tool: draw one note and a chosen chord (major, minor, seventh and so on, or
  one from the clip's scale) is added on top of it. Strum and arpeggiate a selected chord: spread
  the starts of its notes over a time set by dragging (strum), or play them one after another on
  the grid (arpeggiate). Inversions move the lowest or highest note an octave.

## Other note tools (MIDI tools column)

- **Built ([#460](https://github.com/suhina-daw/suhina-daw/issues/460), the column itself
  [#349](https://github.com/suhina-daw/suhina-daw/issues/349)):** the editor has a **MIDI tools
  column** between the clip properties column and the note grid, shown while a MIDI clip is open
  and hidden by a click on the **MIDI tools** icon in the toolbar (› Layout). Showing or hiding it
  is window state: it is not a command, not saved with the project and not an undo step, and
  hiding it never changes the notes or the selection. The column is narrower than the properties
  column and scrolls on its own when the detail area is short.
- Its head says what the tools act on: **Selected notes** while notes are selected, otherwise
  **Whole clip**, with the count under it (`4 notes` or `12 of 12 notes`). The tools with a
  setting of their own (Transpose's semitones, Humanise's **Time** and **Velocity**, the scale's
  root and mode, the velocity tools) keep what was last typed until Suhina closes, as the quantise
  block's settings do.
- Each tool is a button that runs one command over the notes the head names, one undo step, so the
  same edit runs from the CLI and MCP (› Commands):

  | Tool | Does | Command |
  |---|---|---|
  | **Quantise** | Moves the starts (with **Move ends too**, the ends) towards the grid by **Amount** | `note.quantize` |
  | **Transpose** | Moves the notes by the semitones in its field, −24 to +24 | `note.move` with `semitones` |
  | **Humanise** | Moves each start by up to **Time** beats and each velocity by up to **Velocity** steps, a fresh draw each time, so pressing it twice gives two different results | `note.humanize` |
  | **Fit to scale** | Moves each note to the nearest key of the chosen root and mode | `note.fitToScale` |
  | **Invert** | Mirrors the pitches around the middle of the selection's range | `note.invert` |
  | **Reverse** | Turns the selection round in time, inside its own span | `note.reverse` |
  | **Legato** | Makes each note last until the next one starts, at most | `note.legato` |
  | **×2**, **÷2** | Stretches or squeezes the selection in time, its starts and its lengths | `note.stretch` |
  | **Velocity: Randomise** | Changes every velocity by a random amount up to **Amount** | `note.randomizeVelocity` |
  | **Velocity: Ramp** | Draws the velocities from **From** to **To** across the notes in time | `note.rampVelocity` |

- The quantise controls live here, at the top of the column with the rest: the **Quantise** block
  of the clip column (Grid, Amount, **Move ends too**, the **Quantise notes** button and **Close**)
  moved into this column, and Ctrl+U keeps running the same command with the same settings while
  the column is hidden. With the detail area stacked (a column at the right of the window) the
  MIDI tools column has no room and is not drawn, whatever the toolbar icon says; Ctrl+U and the
  keymap's tools still work there.
- **Invert** mirrors each note's key around the middle of the range the tool acts on: the lowest
  and highest key named (the selection's, or the clip's), so `C3` and `C5` swap and `C4` stays.
  **Reverse** puts the last note first inside the span from the earliest start to the latest end,
  each note keeping its length, so a rising line becomes a falling one at the same place in the
  clip. **×2** and **÷2** stretch relative to the earliest start, keeping the first note where it
  is. **Legato** moves each note's end to the start of the next note it acts on, on any key
  ([#1112](https://github.com/suhina-daw/suhina-daw/issues/1112)), never shorter than a hundredth of a
  beat. Notes that start together end together, at the next later start, and the last ones keep
  their length.
- **Later ([#350](https://github.com/suhina-daw/suhina-daw/issues/350)):** the chord tool, strum
  and arpeggiate, inversions; a length field (`note.resize`) and a legato mode that also moves
  starts.

## Shortcuts

Built (the first version, and keyboard editing from [#344](https://github.com/suhina-daw/suhina-daw/issues/344)):

| Keys | Does |
|---|---|
| Double-click on an empty spot | Adds a note (Select tool; › Double-click) |
| Double-click on a note | Deletes it (Select tool; › Double-click) |
| Click, Shift+click, Ctrl+click | Selects a note, adds or takes it out of the selection |
| Drag from an empty spot | Selects the notes in the rectangle |
| Drag a note | Moves the selection (time snapped by grid steps, pitch by semitones) |
| Drag a note's right edge | Changes the selection's length |
| Drag a note's left edge | Moves the selection's starts, keeping their ends |
| Ctrl+drag a note (Cmd on macOS) | Copies the selection to where it is let go |
| Alt+drag a note up, down | Changes the selection's velocity (Alt held at the start) |
| Alt while drawing, or pressed during a drag | No snapping |
| Ctrl+1, Ctrl+2 | Makes the grid finer, coarser (Narrow grid, Widen grid; after a click in the piano roll) |
| Ctrl+3 | Turns the triplet grid on or off (after a click in the piano roll) |
| Ctrl+4 | Turns snapping to the grid on or off (after a click in the piano roll) |
| Drag a bar in the velocity lane | Sets its note's velocity to the pointer's height (the selection by the same amount) |
| Drag from an empty spot of the velocity lane, or Alt+drag in it | Draws a line: each stem it spans takes its height |
| Drag the velocity lane's top edge | Makes the lane taller, shorter |
| Delete, Backspace | Deletes the selected notes (after a click in the piano roll) |
| Ctrl+U | Quantises the selected notes, or every note, with the last settings |
| Ctrl+A, Escape | Selects every note, clears the selection |
| Left, Right | Moves the selection one grid step earlier, later |
| Up, Down; Shift+Up, Shift+Down | Moves the selection a semitone; an octave |
| Shift+Left, Shift+Right | Takes the selection's ends to the previous, the next grid line, or to the next note of the key (› Resizing) |
| Ctrl+C, Ctrl+X, Ctrl+V | Copies, cuts, pastes notes |
| Ctrl+D, or Ctrl+B with Draw-first | Duplicates the selected notes (› Keymap presets) |
| Ctrl+Z, Ctrl+Shift+Z | Undo, redo (as everywhere) |
| Wheel, Shift+wheel | Scroll keys, scroll time |
| Ctrl+wheel; Ctrl+Shift+wheel or Alt+wheel | Zoom time; zoom key height |
| Right button held + wheel | Chooses the next or previous tool, stopping at the first and last |
| Shift+drag with Draw | Draws notes back to back, the last note's length each |
| 1, 2, 3, 4, 5, 6 | Chooses Select, Draw, Paint, Erase, Slice, Mute (after a click in the piano roll) |
| B | Switches between the Select and Draw tools (after a click in the piano roll) |
| Click, drag in the key column | Plays the key, through the track's instrument |
| Ctrl+click, Ctrl+drag in the key column | Selects the notes of that pitch, of those pitches |
| Middle-button drag | Pan |
| Drag a loop handle, the loop bar, a marker on the ruler | Moves the loop's end, the loop, the clip's start or end (on the grid; Alt free; › Layout › Ruler) |
| Drag the ruler up, down; sideways | Zooms time out, in; scrolls time |
| Z, Shift+Z | Zooms to the selected notes (the whole clip when none is selected); to the whole clip (after a click in the piano roll) |
| Ctrl+Shift+E | Hides the editor (the detail area), or shows it again (`view.editor`, › Least height and closing) |

The keys after a click in the piano roll are keymap actions ([`docs/shortcuts.md`](../shortcuts.md)):
the `selection.*` ones the window has anyway, and `pianoRoll.selectAll`, `.deselect`, `.earlier`,
`.later`, `.up`, `.down`, `.octaveUp`, `.octaveDown`, `.shorter`, `.longer`, `.draw`,
`.tool1` … `.tool6`, `.gridNarrow`, `.gridWiden`, `.gridTriplet`, `.gridSnap`, `.zoomSelection` and
`.zoomClip`, which are read only
while the piano roll has the keyboard, so they can be changed like the rest. `pianoRoll.quantize`
(Ctrl+U) means nothing elsewhere, so it works whenever the piano roll shows a clip.

Later: Ctrl+G (glue), added to the keymap when built.

## Workflow modes

**Built ([#490](https://github.com/suhina-daw/suhina-daw/issues/490)).** The way the piano roll
works is a **mode**: one of the three presets below, or **Custom**. The **workflow menu** in the
piano roll's toolbar, after the MIDI tools icon, shows the mode's name (its accessible label is
"Workflow" and the name, such as "Workflow Select-first") and can be changed at any time, while
editing; Settings › Keys › **Preset** offers the same four modes. The menu lists the four
modes, ticked as chosen, and under a divider every **workflow setting** with its choices:

| Setting | Choices | Select-first | Draw-first | Mouse-first |
|---|---|---|---|---|
| **Click on empty grid** (Select tool) | Clears the selection, Draws a note | Clears | Clears | Draws |
| **Right click on a note** | Does nothing, Deletes in Draw and Paint, Deletes in every tool | Nothing | Draw and Paint | Every tool |
| **Shift+right drag** | Does nothing, Splits notes | Nothing | Nothing | Splits |
| **Alt+right button** | Does nothing, Plays from the point | Nothing | Nothing | Plays |
| **Duplicate key** | Ctrl+D, Ctrl+B | Ctrl+D | Ctrl+B | Ctrl+D |
| **Double-click on empty grid** (Select tool) | Adds a note, Does nothing | Adds | Adds | Adds |
| **Double-click on a note** (Select tool) | Deletes it, Does nothing | Deletes | Deletes | Deletes |

- Choosing a preset fills in every setting with its column. Changing one setting, in the menu or
  in Settings › Keys › Mouse and key behaviour (one row per setting), makes the mode **Custom**, with the
  other settings kept as they were. Choosing **Custom** itself changes no setting, so a preset's
  column can be the start of a mix. A Custom mix that happens to equal a preset's column stays
  Custom until the preset is chosen.
- **Duplicate key** binds the key: Ctrl+B moves the browser's key to Ctrl+Shift+B, Ctrl+D gives
  the browser Ctrl+B back. The key can still be changed in the Shortcuts list; the setting then
  reads Ctrl+B only while the duplicate action is bound to Ctrl+B.
- The mode and every setting are kept in `keymap.json` with the keys: `"preset"` names the mode
  (`"Custom"` included) and each setting has its own entry, `"workflowEmptyClick"` (`"selects"`,
  `"draws"`), `"workflowRightClick"` (`"nothing"`, `"drawAndPaint"`, `"everyTool"`),
  `"workflowShiftRight"` and `"workflowAltRight"` (`"nothing"`, `"on"`), `"workflowDuplicate"`
  (`"Ctrl+D"`, `"Ctrl+B"`), `"workflowDoubleClickEmpty"` (`"adds"`, `"nothing"`) and
  `"workflowDoubleClickNote"` (`"deletes"`, `"nothing"`). The four `"mouse*"` entries of earlier
  files are still written and read (› Keymap presets below). A setting the file leaves out is the
  preset's; a value that is not one of its choices is ignored with a warning in the status bar. A
  file whose settings are not the ones its preset fills in (an earlier file with a mouse switch
  turned off by hand) opens as **Custom** with those settings.
- Changing the mode or a setting is window state, saved with the keymap: it is not a command, not
  part of the project and not an undo step, and it never changes the notes or the selection.

## Keymap presets

**Built ([#443](https://github.com/suhina-daw/suhina-daw/issues/443), Mouse-first
[#460](https://github.com/suhina-daw/suhina-daw/issues/460)).** A preset is a workflow mode
(› Workflow modes, which gives each preset's settings); this section is about the keys and the
file.

- Choosing a preset sets the defaults of the keys it names (the duplicate key); every other key
  keeps what it has, and single keys can still be changed afterwards (› Settings in
  [`docs/shortcuts.md`](../shortcuts.md)). Choosing a preset twice sets them again, so a key that
  was changed by hand comes back to the preset's.
- The choice is saved in `keymap.json` under `"preset"`, and read back when the file is read. A
  file without it is **Select-first**; **Custom** is read and written too. A `"preset"` naming
  something else is ignored: the keymap is Select-first and the window says so in the status bar,
  so a hand-edited file never leaves Suhina with no keys.
- Files from before the workflow settings kept four switches, `"mouseLeftDraws"`,
  `"mouseRightDeletes"`, `"mouseShiftRightSplits"` and `"mouseAltRightPlays"` (`true` or `false`).
  They are still written and read, as *Click on empty grid*, *Right click on a note* (every tool),
  *Shift+right drag* and *Alt+right button*; a file without them takes Select-first's, and a
  value that is not a boolean is ignored with a warning.
- The preset names describe the habit they suit; they are not the name of anything else.

## Commands

Every edit is a command (AGENTS.md rule 6), so the piano roll, the CLI and MCP make the same
edits. A command names a clip by `clip`, its id from `project.clips` or `project.launcher`, which
stays the same when the clip or the clips around it move
([ADR 0025](../decisions/0025-object-identity.md)); the piano roll names the clip it shows that
way. The older form, `track` with `index` (an arrangement clip's place on its track) or `scene`
(the clip in the track's slot of that scene, from 1), still works and is deprecated, except that
a slot's clip may still be named by its `scene`. The clip must be a MIDI clip: an audio clip, an
empty slot or a clip that does not exist is an error. Notes are named by their **index** in the clip's notes, as `note.list` returns them; edits
keep the order (`note.add` appends, `note.delete` removes and the later notes move up), so the
indices of notes an edit did not delete stay valid. Each command is one undo step and changes
nothing when it fails.

| Command | Params | Does |
|---|---|---|
| `note.list` | `clip` (or `track` with `index` or `scene`) | The clip's notes: index, time, length, key, velocity (1 to 127) |
| `note.add` | clip, `notes`: a list of `{time, length, key, velocity?}`, `replace?` | Adds notes (velocity 100 when left out); returns their indices |
| `note.delete` | clip, `notes`: a list of indices | Deletes them |
| `note.move` | clip, `notes`, `beats?`, `semitones?`, `replace?` | Moves them in time and in pitch. With `replace: true` here and on the commands marked so, no two notes of a key overlap afterwards: a note covered whole is removed, one covered in part is cut back, and the result lists the removed notes as `replaced` |
| `note.resize` | clip, `notes`, `length?`, `by?`, `startBy?` or `lengths?`, `replace?` | Sets their length, changes it by `by` beats, moves each start by `startBy` beats and keeps each end (the left edge), or sets one length per entry of `notes`; one of the four |
| `note.setVelocity` | clip, `notes`, `velocity?`, `by?` or `velocities?` | Sets their velocity, changes it by `by` (each kept within 1 to 127), or gives each note its own: `velocities` is one value, 1 to 127, per entry of `notes`, in the same order (a repeated index takes its last); one of the three |
| `note.quantize` | clip, `notes?`, `grid`, `amount?`, `ends?`, `replace?` | Moves the starts (with `ends: true` the ends too) of the notes, or of every note when `notes` is left out, towards the nearest line of `grid` (as `launcher.setQuantization` takes it, such as `"1/16"` or `"1/8T"`; not `none`) by `amount` percent, 0 to 100 (default 100); returns the indices and how many moved |
| `note.setMuted` | clip, `notes`, `muted?` | Mutes them (`muted: true`) or unmutes them (`false`); with `muted` left out each note is flipped. Returns the indices and their state |
| `note.humanize` | clip, `notes?`, `time?`, `velocity?`, `seed?` | Moves each note's start by up to `time` beats (0 to 1, default 0.02) and its velocity by up to `velocity` MIDI steps (0 to 127, default 8), each a fresh random draw; `seed` makes the same call repeatable; returns the indices |
| `note.fitToScale` | clip, `notes?`, `root?`, `scale?` | Moves each note to the nearest key of `scale` (`major`, `minor`, `harmonicMinor`, `melodicMinor`, `dorian`, `phrygian`, `lydian`, `mixolydian`, `locrian`, `majorPentatonic`, `minorPentatonic`, `blues`; default `major`) over `root` (a key name such as `"C"`, `"F#"`, or 0 to 11; default `"C"`); with neither `root` nor `scale`, the clip's own scale when it has one; a tie goes to the lower key; returns the indices and how many moved |
| `note.invert` | clip, `notes?` | Mirrors the keys around the middle of the range the notes cover (the lowest key plus the highest, halved, rounded down); returns the indices |
| `note.reverse` | clip, `notes?` | Turns the notes round in time inside their own span (the earliest start to the latest end): each note's start becomes `span end − its end`, its length unchanged; returns the indices |
| `note.legato` | clip, `notes?`, `replace?` | Makes each note last until the next of the notes it acts on starts, on any key, or leaves it when nothing follows; notes that start together end together; never shorter than a hundredth of a beat; returns the indices and how many changed |
| `note.stretch` | clip, `notes?`, `factor` | Stretches the notes in time about the earliest of their starts (`0.01` to `100`): starts and lengths are multiplied, the first start unchanged. `2` is the **×2** button, `0.5` the **÷2** one; returns the indices |
| `note.randomizeVelocity` | clip, `notes?`, `amount?`, `seed?` | Moves each velocity by up to `amount` MIDI steps (0 to 127, default 16), each kept within 1 to 127; `seed` makes the same call repeatable; returns the indices |
| `note.rampVelocity` | clip, `notes?`, `from`, `to` | Sets the velocities from `from` to `to` (1 to 127) across the notes in time order, evenly spaced; one note takes `to`; returns the indices |
| `note.copy` | clip, `notes` | Copies them to the note clipboard; changes nothing and is no undo step |
| `note.cut` | clip, `notes` | Copies them and deletes them |
| `note.paste` | clip, `time?`, `replace?` | Adds the copied notes, the first at `time` (default: right after the copied notes); returns their indices |
| `note.duplicate` | clip, `notes`, `beats?`, `semitones?`, `replace?` | Adds a copy right after the notes' end; with `beats` or `semitones`, that far from the notes instead (a copy made by dragging; the copies keep length, velocity and mute). Returns the copies' indices |
| `note.split` | clip, `notes`, `at` | Splits each named note that spans the beat `at` into two at `at`; a note that starts or ends at `at` is left alone; returns the indices the splits made |
| `clip.scale` | clip | The clip's scale, `{ "root": "D", "mode": "dorian" }` or `null`, and the project's `default_scale` the same way; changes nothing |
| `clip.setScale` | clip, `root?`, `mode?`, `off?` | Sets the clip's scale: `root` (a key name such as `"C"`, `"F#"` or `"Bb"`, or 0 to 11) and `mode` (one of the names `note.fitToScale` takes as `scale`), the one left out kept from the scale the clip has (C, major when it has none); `off: true` clears it, and is an error with `root` or `mode`; one of them is needed. A content other clips play is copied for this clip first (› Scale and key). Returns the scale and whether it changed; the same scale again changes nothing and is no undo step |
| `project.setDefaultScale` | `root?`, `mode?`, `off?` | The same for the scale new MIDI clips take; clips that exist keep theirs |
| `clip.setLoop` | clip, `loop?`, `start?`, `end?` | With `loop` alone: `true` turns the loop on (keeping the loop the clip has, else from 0 over its length), `false` off. With `start` and `end` (content beats, end after start), sets the loop and turns it on; while the clip loops, one of them alone moves that end. `loop: false` with `start` or `end` is an error. Any clip, MIDI or audio |
| `clip.setLength` | clip, `length` | How long the clip plays, in beats (above 0, at most 120 000). An arrangement clip keeps its start and may not run into the next clip on its lane (the nearest that starts after it); fades are shortened to fit. Any clip |
| `clip.setStart` | clip, `offset` | The start marker, in content beats (0 to 120 000). A clip that does not loop keeps its end marker, so its length changes by as much (and an arrangement clip may not run into the next clip); a looping clip keeps its length, and the marker must be before the loop's end. Any clip |
| `preview.noteOn` | `track`, `key` or `keys`, `velocity?` | Plays `key`, or the keys of `keys` together (1 to 128, each 0 to 127, counted once), through the track's instrument until `preview.noteOff` (transport kind: the project and the undo history are untouched) |
| `preview.noteOff` | `track`, `key?` | Releases the preview note of `key` on that track, or every held preview note when `key` is left out |

Limits, so a hostile input cannot break the project or exhaust memory: time from 0 to 120 000 beats
(the engine's furthest start), length above 0 and at most 120 000 beats, keys 0 to 127, velocity 1
to 127 for a set value, at most 10 000 notes named or added in one command and 100 000 notes in a
clip. A move or resize that would take any note out of these limits fails as a whole; the piano
roll limits its drags beforehand so a drag never fails. A list of indices that repeats one counts
it once. `startBy` and a duplicate's `beats` are within ±120 000, `semitones` within ±127, and a
start that would land before 0, a length at or under 0, a key outside 0 to 127, or a `velocities`
list not as long as `notes` (or with a value outside 1 to 127) fails the whole command. A split names at most 10 000 notes and adds at most one note per named note, so it can
never grow a clip past the 100 000 notes it holds; `at` is a beat from 0 to 120 000. The three
`clip.*` commands take a clip the same way (`clip`, or `track` with `index` or `scene`), MIDI or audio, and
refuse a value that is not a finite number in its range, an end not after its start, and a clip
that does not exist; each is one undo step and changes nothing when it fails.

## Open

- **A looping clip shows no end marker** in the ruler, as built: its loop's end is where the
  content ends, and how long it plays is set on the arrangement (or with `clip.setLength`).
  Showing a second end marker for the arrangement length is the maintainer's call
  ([#353](https://github.com/suhina-daw/suhina-daw/issues/353)).
- **A longer clip never runs into the next one**, as built: `clip.setLength` and `clip.setStart`
  refuse a length that reaches past the next clip on the lane, and the ruler stops its drags
  there. Replacing or shortening the next clip, as a moved clip does (› Over other clips in
  [arrangement editing](arrangement-editing.md)), is the alternative; the maintainer decides
  ([#353](https://github.com/suhina-daw/suhina-daw/issues/353)).
- **Dragging the ruler down zooms in**, up zooms out, as built; the other direction is one sign
  ([#353](https://github.com/suhina-daw/suhina-daw/issues/353)).

- **Alt+wheel over a note:** today it zooms the key rows. Proposed: over a note it changes that
  note's velocity (every selected note when the note under the pointer is selected) and over the
  empty grid it keeps zooming the rows, as the Arrangement view's clip gain does. Not built: the
  maintainer decides ([#443](https://github.com/suhina-daw/suhina-daw/issues/443)).
- **The toolbar size default: Large (125 %), as built.** Normal (100 %) was considered, and the
  maintainer decides ([#460](https://github.com/suhina-daw/suhina-daw/issues/460)); changing it is
  one constant and the default in Settings, and an existing `settings.txt` keeps the value it
  stored.
- **Mute is the sixth tool**, after Slice, with the number key **6**. Another order (Mute before
  Slice) changes the labels of `pianoRoll.tool5` and `pianoRoll.tool6` and the icons; the
  maintainer decides ([#460](https://github.com/suhina-daw/suhina-daw/issues/460)).
- **Double-click on a note in Select deletes it** in every preset, as built before; making
  **Does nothing** the default for Mouse-first (whose right button deletes) is the maintainer's
  call ([#490](https://github.com/suhina-daw/suhina-daw/issues/490)).
- **The preset names** (**Select-first**, **Draw-first** and **Mouse-first**) describe the habit;
  the maintainer may
  rename them ([#443](https://github.com/suhina-daw/suhina-daw/issues/443),
  [#460](https://github.com/suhina-daw/suhina-daw/issues/460)).
- Envelopes and Expression tabs (per-note pitch bend, pressure, timbre; clip automation): a
  separate spec.
- Audio clip editor (sample view, warp markers): a separate spec.
- MIDI recording into a clip, overdub and step input: belongs to recording.
