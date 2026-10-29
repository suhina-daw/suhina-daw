---
description: Functional spec of the main window and the Session view: regions, clip slots, scenes, launch keys and their mapping mode, I/O, mixer, browser
read_when: Session view, main window layout, clip slot behaviour, scene launch, launch keys, mapping mode, Map button, Ctrl+M, mixer section, browser structure
type: spec
status: draft
last_verified: 2026-10-28
covers:
  - crates/ui/src/session/session_view.rs
  - crates/ui/src/session/mixer_strip.rs
  - crates/ui/src/settings/view_settings.rs
  - crates/ui/tests/mixer_strip.rs
  - crates/ui/src/interaction/colour_picker.rs
  - crates/ui/tests/track_colour.rs
  - crates/ui/src/interaction/grid.rs
  - crates/ui/src/session/slot_launch.rs
  - crates/ui/src/session/launch_keys.rs
  - crates/ui/tests/launch_keys.rs
  - crates/ui/src/app.rs
  - crates/ui/src/kit/widgets.rs
  - crates/ui/src/detail/detail_view.rs
  - crates/ui/src/settings/keymap.rs
  - crates/ui/src/detail/follow_view.rs
  - crates/engine/src/cpu.rs
---

# UI: main window and Session view

Functional spec, written from using Live 12.4.6 (Suite, Windows) with a self-authored set. Only
structure and behaviour; colours, sizes and look come from [`docs/design/brief.md`](../design/brief.md).
Items marked **(to observe)** still need checking in Live.

## Window regions

Session and Arrangement remain separate views. Optional theme-owned Session density
sets track/return/scene widths and header/slot heights through `skin.session` and
canonical native CSS; omitted values preserve the default sizes below. Painting,
scrolling and clip/file drop targets share these dimensions. The existing mixer-height
divider remains independent. See [theme skins](theme-skins.md#session-density-and-meter-materials).
The existing knob/field preference applies to the Session pan face as well as compact
volume: a knob uses the same native pan input area and command. A narrow themed pan
area shows the dial alone; a wider one keeps the value beside it.
Canonical `.pan { --suhina-appearance: knob; }` or `field` can override that preference
as portable theme data in either view. `native` restores the global preference.

From top to bottom:

1. **Menu bar:** File, Edit, Create, View, Playback, Navigate, Options, Help. (Suhina has File,
   Edit, View, Projects, Options and Help so far; components.md lists what is built.)
2. **Control bar**, left to right:
   - external sync and tempo: sync toggle, tap tempo, tempo field, nudge, time signature
   - groove amount, metronome, global launch quantisation (in Suhina also the Arrangement
     view's grid: [arrangement editing](arrangement-editing.md) › The grid). **Suhina:** the
     metronome's button, here and in the main strip, is held and dragged for its level: a
     slider opens above it, or below where there is no room above, and the pointer's height on
     it is the level (−60 to +6 dB), so the whole range is one drag
   - key and scale
   - transport: arrangement position, play, stop, record, overdub, automation arm, loop region
     start, punch, loop, loop length. **Suhina:** play starts from the play start marker, or
     from the selected Arrangement clip as Settings › Arrangement › Time and play › Play from says, and stop
     returns to it or stays, as Settings › Arrangement › Time and play › Stop says
     ([arrangement editing](arrangement-editing.md) › Play position)
   - **Suhina:** record, right of stop, records the armed tracks into the arrangement, and a
     Count-in menu right of the launch quantisation; an armed track's empty slots show a record
     button ([recording](recording.md) › In the window)
    - draw mode, computer-keyboard MIDI, MIDI and audio activity, sample rate, CPU load
      (**Suhina:** the CPU load is built, left of the view switch; `launcher.state`
      reports it as `cpu`; the rest is not). Left of the CPU load, the program's RAM
      (its resident memory, in MB or GB), shown always; **Suhina:** the number adds
      the resident memory every plug-in host process reports to the engine once
      a second, so the readout is what Suhina and its hosts use together
    - view switches (Session / Arrangement). **Suhina:** the last control at the right end
      of the bar, with the CPU load and the RAM left of it. In a window too narrow for all
      of them the RAM, then the CPU load, is left out rather than overlap the position
      readout; the order stays
3. **Main area**, left to right:
   - **Browser** (collapsible, resizable width). Observed in Live: a search field; a sidebar of
     collections (user colour tags), library categories and places (user folders); a results
     tree; tags and an item count below it. **Suhina's structure** (see the design brief):
     search with a shortcut; category filters as buttons (all, samples, MIDI, instruments,
     effects, presets, plug-ins); a sidebar of places only (Pinned, Recent, Folders, add folder);
     a file tree with expandable folders, type icons and length or duration; colour tags as dots
     on rows; a preview strip with play, waveform, playhead, duration, BPM and key; tags and the
     selection count at the bottom. **Suhina:** how it behaves (places, search, preview,
     dragging into the project) is in [`browser.md`](browser.md).
   - **Groove pool** under the browser (collapsible).
   - **Session view** (or Arrangement view; Tab switches).
4. **Detail area width** (collapsible), across the full width: the selected track's device chain, or
   the selected clip's editor. **Suhina:** Settings › Devices › **Detail area width** puts it
   **Beside the browser** (the default: it starts right of the browser, which runs down to the
   status bar) or **Full width** (it spans the whole window, under the browser too, which ends
   above it); with the device chain stacked in a column beside the view the layout does not
   apply. A splitter above it sets its height; under the
   Arrangement view, while the piano roll shows a clip, the splitter pulls it all the way up
   ([clip editor](ui-clip-editor.md) › Pulled to the top).
5. **Info view** in the bottom left corner: help text for whatever is under the pointer.
6. **Status bar:** messages, the selected track's name and a mini overview.

Suhina keeps one 20 px status bar for messages and counts and has no separate info view; where
this list and `docs/design/components.md` differ, components.md describes Suhina.

## Session view

A theme can opt slot columns into colour/state-keyed `track.body` SVG/PNG artwork
using canonical `.track-column { --suhina-track-artwork: on; }`. The image stays
behind cells; empty slots retain the texture, with native stop marks and hover
feedback. Launch, selection, mapping and drop geometry are unchanged. Omission or
`off` retains native slot fills. See [theme skins](theme-skins.md).

A grid. **Columns are tracks** in project order: regular tracks, then a gap, then return tracks,
then the main track at the far right. **Rows are scenes.** The main track's column holds the scene
launch buttons, numbered from 1; an extra empty area below the last scene accepts drops to
create a new scene or track.

### Track header

**Suhina:** the whole header is the track colour, with the name in a text colour that reads on
it, so tracks are told apart at a glance; the channel strip under the column carries the same
colour (Mixer › the channel strip), and the Arrangement headers have it as a band down their edge
(Track colour below). Clicking selects the
track; dragging moves it (after the header of an open group is into that group, and over another
group's column it joins that group, [arrangement editing](arrangement-editing.md) › Groups inside
groups); right-clicking opens its menu (rename, colour, group, ungroup, move into group, move out
of group, **Insert audio track** and **Insert MIDI track** after it, **Insert return track**,
delete). The track's channel strip and a return's column, which has no slots, open the same menu
on a right-click and select the track on a click. A track inside groups has its name stepped in 4 px per level; a group's name
follows a fold arrow, and a folded group hides the columns of the tracks inside it.
Double-clicking selects the track and shows its devices in the detail area, opening the detail
area if it was hidden ([clip editor](ui-clip-editor.md) › Where it opens); renaming stays in the
menu.
Dropping a device or file from the browser on the empty area right of the last track creates a
new track; the area also has **+ MIDI track**, **+ Audio track** and **+ Return track** buttons
(the view scrolls far enough past the last track for them to keep their width however many
tracks there are), and a right-click there offers the three inserts ([browser](browser.md) ›
Files). A file dropped on a slot or below the stop row becomes a clip in that slot (scenes are
added below when needed); a device dropped on a column joins the track's chain. Samples and MIDI
files dragged from the system's file manager land the same way ([browser](browser.md) › Files
from the system).

### Clip slots

Each cell is a slot for one clip.

| Slot state | Shows | Click |
|---|---|---|
| Empty, with stop button | stop control | stops the track at the next quantisation point |
| Empty, without stop button | nothing; **Suhina:** a double-click creates an empty clip of the track's kind (`clip.create`) | nothing (**Suhina:** selects it) |
| Clip, stopped | a play triangle in the track colour, clip name, launch control | on the launch control: launches the clip (**Suhina:** and selects it); on the name: selects it |
| Clip, queued | launch control in a pending (blinking) state until the quantisation point | as stopped |
| Clip, playing | a stop square in the track colour, progress through the clip | on the launch control: **Suhina:** a trigger clip (one that listens for no release) stops at the next quantisation point; a gate, toggle or repeat clip relaunches, its release keeps its own meaning; on the name: selects it |
| Clip, recording | recording state | |

**Launching and selecting** (public manual, Session View › clips and launch settings). A clip's
launch control is at the left edge of its slot; the rest of the slot, its name, selects the clip
without launching it. A selected clip shows in the detail area. Launching a clip with its launch
control also selects it, unless the user turns that off (a launch setting, on by default); with it
off, launching leaves the selection and the detail area as they were. With a clip selected, Enter
launches it and the arrow keys move the selection to the slot above, below, or in the next track.
Shift-click and Ctrl-click add clips to the selection **(not built)**.

**Suhina:** the launch control is the slot's mark and the padding around it (the first 18 px);
the name selects. Selecting a clip selects its track too, and selecting a track by its header ends
the clip's selection. The selected slot is drawn like a selected arrangement clip. The setting is
Settings › Session and mixer › **Select on launch** (On by default, saved as
`select_on_launch`). Enter is the action `selection.launch`; the arrows are `selection.up`,
`selection.down`, `selection.left` and `selection.right` (all rebindable,
[shortcuts](../shortcuts.md)); left and right move to the next track with clip slots (groups and
return tracks are skipped) and keep the scene, up and down stay in the track. They act in the
Session view only, and only while a clip slot is selected. Multiple selection is not built.

**Suhina:** a clip's launch control launches when the mouse button goes down on it, not when the
click ends, and going up is the release a clip in gate or repeat mode listens for
([clip launching](clip-launching.md#launch-settings)). Right-clicking a clip opens the slot's menu
with its launch settings under it: **Launch mode** (Trigger, Gate, Toggle, Repeat), **Launch
quantisation** (Global, None, 8 bars … 1/32) and **Legato**, each an undoable edit.

Double-clicking an empty slot (with or without a stop button) on a track that holds clips makes an
empty clip of the track's kind there — MIDI on a notes track, an "Audio clip" with no file yet on
one that plays audio (**Suhina:** while Settings › Arrangement › **Double-click makes a clip** is On,
the default; Off, the double-click does nothing, and the Arrangement's lanes follow the same
setting), one bar long and looping (`clip.create`, one undo step; [arrangement editing](arrangement-editing.md)
› Making an empty clip), and selects it, so the detail area shows it; a first click on a slot with
a stop button still stops the track. Groups and returns do nothing on a double-click. Editing the
notes of a MIDI clip waits for the piano roll (#200); the empty audio clip plays silence until a
sample lands on its slot. Double-clicking a clip opens it. Dropping a file or clip on a slot puts
the clip there; on an empty audio track the track takes the clip's name **(observed in the corpus)**.

Below the last scene each track has a **stop all clips** button for that track, and the main
column has **stop all clips** for the whole set.

**Suhina:** a click on a track's slot also selects it (a group's slots only launch; with
**Select on launch** off, a click on an occupied slot's launch control launches without selecting),
and selecting a slot, empty or not, selects its track too, so the detail area shows that track and
auto-arm (Settings › MIDI) arms it (#834). A right-click selects it and opens its menu: **Copy** copies its clip (`clip.copy {track,
scene}`), **Paste** puts the copied clip in it (`clip.paste {track, scene}`), greyed out while
nothing has been copied, and **Delete** takes its clip off, MIDI or audio (`clip.delete {track,
scene}`, one undo step), greyed out on an empty slot (#842). Ctrl+C and Ctrl+V do the same on the selected slot
([shortcuts](../shortcuts.md)); Ctrl+X and Ctrl+D act on arrangement clips only and say so in the
Session view. **Suhina:** the Delete key (or Backspace) takes a selected slot's clip off its
slot (`clip.delete` with the slot's track and scene); one undo puts it back, and the detail area
falls back to the track's devices.

### Between the views

The window shows one view at a time, so a clip goes from the Arrangement view to a slot, or from
a slot to the arrangement, in two ways. **Suhina** decisions, our own design:

- **Copy and paste.** Copy an arrangement clip (Ctrl+C, or Copy in its right-click menu), switch
  with Tab, select a slot and paste (Ctrl+V, or Paste in the slot's menu). The other way, copy a
  slot's clip, switch, select a clip or a track and paste: it lands as an arrangement paste does
  ([arrangement editing](arrangement-editing.md) › Copy, cut and paste).
- **Dragging.** While a clip is dragged, holding it over the other view's name in the control bar
  for half a second switches the view, and the drag goes on there: let go over a slot and the clip
  is copied into it; over a track's lane and it is copied there, at the beat under the pointer on
  the grid (Alt for free), stacking when the setting for its kind says so. **Suhina:** Tab does the
  same while a clip is dragged — or a press is held on it, before the pointer has moved — the view
  switches at once, and the drag goes on there as a carry (#1063). A press held on a slot's clip
  selects the slot at once, as the click will. A slot's
  clip dragged to another slot in the Session view **moves** — the target's slot takes it and
  the source's empties; the two slots **swap** when the target has a clip, so nothing is lost
  (`clip.moveSlot`, one undo step). Ctrl (Cmd on macOS) held on the drop copies instead. A label
  beside the pointer says where it will go ("Move Fill to Drums, scene 5", "Swap Fill and Beat A
  in Drums, scene 5", "Copy …" with Ctrl); Escape, or letting go anywhere else (a
  header, the empty space, the clip's own slot), does nothing.
- **Always a copy.** A clip that crosses views is copied, never moved: the arrangement and the
  launcher are separate places, and the original stays where it was. The drop runs `clip.copy`
  from where the clip came from, then `clip.paste`, so the dropped clip is also the copied clip
  for Ctrl+V. The paste is one undo step.
- **What travels.** The clip's content (its own copy), gain, warping, loop, offset, length and
  fades. In a slot its start is 0; on the arrangement it starts where it was dropped.

### Scene launch (main track column)

One launch control per scene with its number or name. Launching a scene launches every clip in
its row and stops tracks whose slot in that row is empty with a stop button. Scenes can carry a
tempo and time signature that apply when launched. **Suhina:** while a row plays, its button
stops the row: every track whose slot in that row has a clip stops at the next quantisation
point (`scene.stop`); a track whose slot in the row is empty keeps playing.

**Suhina:** the launch button shows a play glyph at its left — the stop square while the row
plays — then the scene's name, its tempo and time signature when it has them (`Drop 128 3/4`),
with the number on the right. Right-clicking it opens the scene menu:
fields for the name, the tempo (empty for none) and the time signature (`3/4`, empty for none),
each applied with Enter, then **Insert scene below**, **Duplicate scene** and **Delete scene**
(greyed out for the only scene). Every entry is a `scene.*` command, undone with Ctrl+Z. The
control bar shows the time signature playing, and its position counts bars in it; a launched
scene can change it.

### Launch keys

**Suhina:** a computer key can launch a slot, a scene or a track's stop button
([clip launching](clip-launching.md#launching-from-keys-and-notes)).

- **Mapping mode.** **Map keys** at the right end of the title bar, after the view names, or
  Ctrl+M (`view.mappingMode`) turns it on and shows the Session view; **Map keys** is lit while
  it is on.
  Every slot (empty ones too), scene launch button and track stop button gets a tint, and a
  control with keys mapped to it shows them in small text at its right edge (`Q`, `Note 36`).
  A click picks a control (an outline) and launches nothing; clicking it again drops it.
- **Mapping a key.** With a control picked, the next key pressed without Ctrl, Alt, Shift or Cmd
  is mapped to it (`launcher.addMapping`), one undo step; a key mapped elsewhere moves here.
  Delete or Backspace removes the picked control's keys (`launcher.removeMapping`). Escape, Ctrl+M
  or **Map keys** again leaves the mode; so does showing the Arrangement view. With nothing picked,
  keys do what they always do.
- **Playing.** Outside the mapping mode a mapped key going down is the press (`clip.launch`,
  `scene.launch`, `track.stop`) and going up the release (`clip.release`, for a clip in gate or
  repeat mode). Holding a key down does not press it again.
- **Keys that are also shortcuts.** A mapped key wins over a shortcut on the same key while the
  Session view is shown, no text field has the keyboard and no dialog or Settings is open;
  anywhere else the shortcut works as before. A key pressed with Ctrl, Alt, Shift or Cmd is
  always a shortcut, so Ctrl+Z still undoes when Z is mapped.
- **Releases are never lost.** A key held when the window loses focus, or when one of the
  conditions above ends (a text field takes the keyboard, the Arrangement view is shown), is
  released then, so a gate clip does not play on.

### I/O section (below the grid, per track, toggleable)

- Audio tracks: the audio input's block on the track's strip, above the on/solo/cue/arm row:
  **Audio From** (the caption) with the source's dropdown — "No input", a channel, or a stereo
  pair of the audio input device that Settings › Audio chose, the same list the track menu
  shows — and the **Monitor** buttons **In / Auto / Off** with the one the project has lit.
  They show from the mixer height where the track's name and output rows appear in full
  ([components](../design/components.md) › Height steps). A pick is the same command the menu
  runs (`track.setAudioInput`, one undo step); clicking the monitor button that is already on
  runs nothing.
- MIDI tracks: "MIDI From" (source, channel), monitoring (In / Auto / Off), "MIDI To" or "Audio
  To" once an instrument is on the track — planned with the I/O section.
- Return tracks: "Audio To".
- Main: cue out and main out.

### Sends (toggleable)

One knob per return track (A, B, …). Return tracks show pre/post toggles on the main column.

**Built:** sends form a horizontal row below the track on/solo/cue/arm buttons. Two
labelled fields fit side by side; further destinations continue to the right. Scrolling over
this row moves it horizontally without scrolling the Session grid or changing a send level.
The pre/post choice is in each field's right-click menu; a dot marks a pre-fader send.
Behaviour and commands: [sends and returns](sends-and-returns.md).

### Mixer (toggleable, resizable height)

Session strips and their name surfaces use the theme’s Track liquid coating, including returns;
Main uses the same coating over its neutral surface. Vertical faders have a dark unfilled rail,
a quiet numbered dB scale (0, −6, −12, −24 and −∞) and a 20 × 26 px shaped handle, alongside wider stereo meters (14 px).
The fader stays on the left on ordinary tracks; returns and Main centre the fader, with its meter to the right.
Pan spans the strip width. Session columns retain their original widths; their sends form a
single horizontal row below the toggle buttons, with two fields visible and independent horizontal scrolling to further destinations. Sends show their return letter and dB value. Pan and sends share their drawing with Arrangement.
The handle has a solid theme-tinted face, upper and lower bevels, a small shadow and a recessed centre marker. Its centre indicates the gain; the rail stays dark and unfilled.
The same fader and meter layout is used on tracks, returns and Main.

Per track, top to bottom: pan, volume with numeric field and meter ([level
meters](level-meters.md)), track on/off (shows the
track number), solo, cue (headphones: the track is also heard on the cue output, headphones,
`track.setCue`; [spec](cue-output.md)), arm (MIDI and audio tracks). Pan and volume are dragged
up or down and double-clicked to reset; the change is heard while dragging, and one drag is one
undo step (`track.setPan`, `track.setVolume` with `continue`). The Arrangement view's track
headers work the same. **Solo:** while any track is soloed, only the
soloed tracks are heard, with the groups they play into, the tracks inside a soloed group, and
the return tracks (so effects stay audible); mute still wins. A click solos one track and ends
the other solos; with a modifier (Ctrl) it adds to them (`track.setSolo`, `exclusive`). **Arm:**
an armed track's instrument plays what the MIDI controllers play; a click arms one track and
disarms the others, Ctrl adds (`track.setArm`, `exclusive`; [ADR 0017](../decisions/0017-inputs-and-latency.md)).
A track that plays audio is armed to hear its audio input (`track.setAudioInput`, monitoring Auto). MIDI tracks without an instrument show no
volume, pan or sends, only the MIDI meter. The height of this section is a saved view setting
**(observed: `ViewStateSessionMixerVolumeSectionHeight`)**.

**Suhina: the channel strip.** Our own design. Each track, group and return column ends in a
vertical channel strip under its slots, and the strip shows more the taller the mixer is:

- **Resizing.** The line between the slots and the mixer is dragged up or down (the pointer shows
  a vertical resize); a double-click on it gives the default height back. The least height keeps
  the toggles and a compact volume; the mixer grows up to just under the stop buttons under the
  last scene (the slots stay in view), and always at least to its default height, so a project
  with many scenes still gets the whole strip while its slots scroll. The height is kept in
  Settings (`session_mixer_height`) and is the same for every project.
- **Always shown**, at every height: the track on button (showing the track number, or the
  return's letter), solo, cue, arm (on tracks that play notes or audio), and a volume control. At
  the least height the volume is a compact value across the strip, dragged up or down.
- **As it grows**, it adds in this order: a vertical **volume fader** with the track's level meter
  beside it and its dB value over it (the fader replaces the compact value); the full-width **pan** field
  with its value; the **send** fields; the track's **name** at the top; under it the track's
  **output**; at the tallest heights a scrollable list of the track's **devices and
  plug-ins**; and on a track that plays audio the **input's block** above the on/solo/cue/arm
  row (Audio From and the monitor's In / Auto / Off buttons, [I/O](#io-section-below-the-grid-per-track-toggleable)).
  Shrinking drops them in the reverse
  order. Height the strip has beyond what its parts need goes to the fader, and once the device
  list shows, to the list after the fader is tall enough. Every strip reserves the same 26 px footer below the toggles when sends are shown, keeping faders and toggles aligned across tracks, returns and Main.
- **The fader** is dragged up or down (Shift for finer), and the wheel over it moves it a dB at
  a time; every frame of a drag is heard, and the drag is one undo step (`track.setVolume` with
  `tracks` and `continue`). A double-click sets it back to 0 dB, as the Arrangement headers do.
- **The output** reads "→ Main" or "→" and the group the track plays into. A click opens a menu
  of Main and the project's group tracks (a group does not list itself); picking one routes the
  track there (`track.setOutput`, one undo step). A route that would make a loop is refused with a
  message and changes nothing. The Arrangement view's track headers have the same dropdown.
  Choosing an input and monitoring here is planned with the I/O section.
- **The device list.** One row per device in chain order, its name dimmed while it is off; the
  wheel scrolls it when it holds more than fits. A click selects the track and shows its devices
  in the detail area, opening the detail area if it was hidden; the clicked row stays lit.
- **A double-click on the strip** itself (its background or its name, not a fader, knob,
  button or device row) does the same for the whole track: it selects the track and shows its
  devices in the detail area, opening the detail area if it was hidden. A single click there does
  nothing, as before.
- The main column shows the main output's meter where a track's meter stands.
- **The colour band** down the strip's left edge, at every mixer height, is the track colour;
  its name row is filled with it too. A click on the band opens the colour palette (Track
  colour).

### Track colour

**Suhina:** a track's colour shows over an area large enough to tell tracks apart: the Session
column's header and the strip's name row are filled with it, the strip has a band down its left edge,
and an Arrangement header a band down its left edge, the header's full height. A muted track's
colour is dimmed. Text drawn on the colour is the light or the dark text colour of the theme,
whichever stands out more from it.

The colour is picked from one palette, the theme's eight track colours, wherever a colour is
offered: a click on a track's colour band (the strip's in the Session view, the header's in the
Arrangement) opens it as a popup, the track's right-click menu has it as before, and a colour
tag's right-click menu in the browser has it before the free colour choice. Picking a swatch runs
`track.setColor` with that colour, one undo step, and closes the popup; picking the colour the
track already has runs nothing. A colour of any value (from an imported project, or set with
`track.setColor` from the CLI or MCP) shows as it is; the palette marks none of its swatches then.

## Detail area: a selected clip

A selected clip in a slot shows its properties as an arrangement clip does
([components](../design/components.md) › Clip properties), with **Scene** (its number) in place
of Start: track, scene, length, loop, then gain, fade-in and fade-out for an audio clip
(`clip.setFades {track, scene, …}` sets them, [clip launching](clip-launching.md) › Clip fades)
or the note count for a MIDI clip; the track's device chain follows. A slot that is emptied, or a
track that is deleted, shows the track's devices or nothing. Editing the properties there is
planned.

## Detail area: follow action

**Suhina** (own design): a selected slot clip shows its follow action
([clip launching](clip-launching.md) › Follow actions) first in the detail area, in a column at
the left of a row or at the top of the stacked column, with the rest of the detail beside or
under it. Launching a clip on a track selects it; clicking a track's header selects the track
instead and hides the section; the Arrangement view shows only its own selection. Top to bottom:

| Control | What it does |
|---|---|
| **Follow action** title with an **On / Off** switch | turns the follow action on or off (`enabled`); off, the rest reads as inactive but can still be changed |
| **Time** | the follow time in bars.beats (bars.beats.sixteenths when it has a part of a beat), dragged up or down in sixteenths (Shift for finer), a double-click sets one bar; dimmed while linked |
| **Linked** switch | on: the follow time is the clip's length (`linked`) |
| **Action A** dropdown | nothing, stop, again, previous, next, first, last, any, other, or **Jump to scene N** for each scene |
| **Chance A** | 0 to 100, dragged; its share shows as a fill; a double-click sets 100 |
| **Action B**, **Chance B** | as A; a double-click sets chance B to 0 |

Every change runs `clip.setFollowAction` with only what changed, one undo step each. Typing a
time or a chance is planned, as for every value field.

The pan and the volume are dragged up or down, and the wheel over either changes it a step at a
time (1 dB, or 0.02 of pan; Shift is finer) instead of scrolling the view. With Settings ›
Session and mixer › **Mixer controls** on Knob, the volume is drawn as a knob beside the pan
knob, and its value stays beside them
([arrangement editing](arrangement-editing.md) › Volume, pan, mute and solo).

## Detail area: device chain

Devices left to right in chain order. Each device has a header (on/off switch, name, fold,
options) and its controls. Instruments come first on MIDI tracks; effects follow. An empty
area at the end accepts dropped devices. Plug-ins show their name and an option to open their
own window. Suhina shows where the plug-in stands (loading, running, missing, crashed) and a
button that opens or closes its editor, or restarts it after a crash
([components](../design/components.md) › Device panel). With the setting Plug-ins › Editors ›
Device view, the editor opens inside the panel under that line: in a row the device grows as wide as the
editor and the panel as tall as the tallest open editor; stacked, the device grows as tall as
its editor, which is cut off at the column's right edge; what does not fit is cut off at the
panel's edge, and the editor goes with its track (shown only while the track is).

## Shortcuts (to observe and list)

Tab (Session/Arrangement), Ctrl+D (duplicate), Ctrl+T / Ctrl+Shift+T / Ctrl+Alt+T (insert audio /
MIDI / return track), Ctrl+I (insert scene), Ctrl+Shift+I (capture and insert scene), Space
(play/stop), Enter (launch selected), arrow keys (move selection), 0 (deactivate) **(to observe:
full list)**.

**Suhina:** its keys and how to change them are in [shortcuts](../shortcuts.md); Ctrl+T and
Ctrl+Shift+T add an audio or a MIDI track below the selected track, in its group (after the last
regular track when none is selected).

## Open

- Clip editor (piano roll, sample editor): separate spec.
- Arrangement view: separate spec.
- Hot-swapping a device or sample from the browser ([browser](browser.md) › Not decided yet).

## SDK effect panels

Suhina SDK effects display their declared stock parameter controls and optional authored canvas.
Canvas backgrounds, embedded images and custom parameter controls are described by the
[custom panel contract](sdk-custom-ui.md). The horizontal device card uses the declared canvas
width by default; `viewport_size` overrides the preferred body width and height in both chain
orientations. Constrained views scroll the contents. Custom controls use the same parameter commands,
undo history and smoothing as stock controls.

The column header shows the track type before its name: a miniature keyboard for MIDI, a waveform for audio, and both for mixed-content tracks. This remains visible when the mixer is collapsed. Groups retain their fold arrow and returns do not receive a MIDI/audio marker. The marker is read-only; clicking or dragging the header still selects or moves the track.
