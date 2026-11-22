---
description: Functional spec of the audio editor in the detail area: what a double-clicked audio clip shows (its whole file across the content's beats under a bar, beat and sixteenth ruler, the played part lit, the loop's edges), the waveform color, zoom and scroll, and editing warp markers (add, place on transients, drag, remove with a double-click or Delete, reset warp)
read_when: audio editor, audio clip editor, waveform view, big waveform, detail area waveform, double-click audio clip, warp markers, add warp marker, move warp marker, delete warp marker, warp markers at transients, reset warp, align snares, audio ruler, audio grid, waveform color
type: spec
status: draft
last_verified: 2026-11-23
covers:
  - crates/ui/src/detail/audio_editor.rs
  - crates/ui/src/app/detail_area.rs
  - crates/session/src/clip_audio.rs
  - crates/session/src/commands/clip.rs
  - crates/ui/src/detail/detail_view/clip_panel.rs
  - crates/engine/src/ahead.rs
---

# UI: audio editor

Functional spec of the view the detail area shows for a selected audio clip, in the place where a
MIDI clip shows its piano roll. Suhina's own design, written in our own words under the clean-room
rules (`CONTRIBUTING.md` › Clean-room).

**What is built.** The audio shown large under a ruler, its warp markers added (one at a time or
on every transient), moved and removed on it, and a dialog that resets the warp. A click on the
ruler moves playback there and one on the waveform sets where it starts, the playhead showing on
the marker lane while the transport runs (an arrangement clip).

## Where it opens

- A double-click on an audio clip (arrangement or launcher slot) opens the detail area on it, as
  it does a MIDI clip's piano roll ([clip editor spec](ui-clip-editor.md) › Where it opens). A
  click only selects, so a hidden editor stays hidden.
- The clip's column ([clip panel spec](clip-panel.md)) is at its left, the editor fills the rest,
  in a frame inset a few points from the area's right edge, so the file's end reads as the end of
  the editor.
- The **Clip / Devices** switch, the stacked devices and *Devices beside piano roll* act as they
  do for a MIDI clip: the editor is the clip's side of the switch.
- An audio clip on a track whose plug-in edits clips (ARA) keeps that plug-in's editor in this
  place ([ARA spec](ara-hosting.md)).
- The area is resized with its splitter like any clip editor, and pulled to the top with Escape
  twice. Its least height is the header, the ruler, the marker lane and about 90 points of
  waveform.

## What it shows

The horizontal axis is the clip's **content beats**: where the warp markers are, so a marker sits
in the same place whichever clip plays the file. It runs from content beat 0 (or the start of what
the clip plays, or the file's start, whichever is first) to the file's end (or the end of what the
clip plays): the ruler stays put and the file moves along it, so dragging the first warp marker
later moves the start of the audio with it and leaves silence before it. The file is drawn once, from its first sample to
its last, at the zoom below. Each pixel column reads the stretch of the file the warp markers put
there, as [waveforms](waveforms.md) draws any waveform (peaks with RMS, two lanes for a stereo file
when tall enough, samples close up, the dB scale setting).

### The header and the tabs

A bar along the top (20 points), over the ruler:

- At its left, the file's name, its sample rate and its channels (`clip.wav  44.1 kHz · 2 Ch`),
  cut at the tabs when long.
- In its middle, two tabs drawn as the Clip / Devices switch is (`editor-tab`, `[checked]` for
  the one on show): **Sample**, the warp markers, the time selection and the play start (the rest
  of this spec), and **Envelopes**, the clip's gain envelope.
- At its right, the editor's tools, from the right: the grid's value now (`1/32`, `1/1024`,
  `1 Bar`; *Editor grid* to a screen reader), a click on which opens the grid's entries as the
  right-click menu holds them; **Snap**, lit while marks snap to the grid; **Reset…**, the Reset
  warp dialog; **Fit**, which fits the file to the area; and **Transients** (*Add warp markers at
  transients*), which pins a warp marker on every note start of the file, where its sound is
  (`clip.addWarpMarkersAtTransients`, one undo step). They take the tabs' look. In a header too
  narrow for them all, the ones that would reach the tabs are left out, Transients first; the
  right-click menu and the keys still do what they do.

**Envelopes** is the clip's *Edit gain envelope*, the same switch as the button on the clip in the
Arrangement (and Alt+E): opening one opens the other, closing one closes the other, and with the
clip shown in both the envelope is edited in both at once, its points and the edit under way
shared (their widgets are told apart by where they are drawn). It draws the clip's gain envelope
over the waveform and edits it there as on the clip in the Arrangement ([clip panel spec](clip-panel.md) › Gain envelope): a click on its line adds a
point, a drag moves one, a double-click puts it at 0 dB, Alt and a click (or Delete over it)
removes it, Shift and a drag of the line moves all of it; each is one `clip.setEnvelope`. The
points are content beats, so the editor's ruler, zoom and grid are the envelope's. On this tab the
waveform is the envelope's: a drag there edits it instead of painting the time selection, and a
click there does not move the play start; the marker lane keeps its warp markers.

### The ruler

A strip along the top (28 points) that counts where you are in the audio, so a sound can be put
on a beat:

- **Bars** are numbered from 1 (content beat 0 is the start of bar 1), every few bars when they
  would crowd; **beats** are numbered `bar.beat` once beats are 60 points apart, and
  **sixteenths** `bar.beat.sixteenth` once sixteenths are 60 points apart: one level at a time,
  with room around its numbers.
- **The grid** has lines at the finest power-of-two note value from a 4096th note to a half
  note, then one bar, two, four and so on, whose lines are at least the grid width's spacing
  apart (half as far again as the piano roll's: 18 points for Narrow, so a fitted file is on 32nds): it gets finer as the view zooms in, down to a sample or two of the
  wave, much finer than the piano roll's. Its value now (`1/32`, `1/1024`, `1 Bar`) is written in
  the ruler's right corner.
  A bar's line is the strongest, a beat's weaker, the rest faint; they run down the waveform as
  well as along the ruler, across the axis (before the file's start too), under the waveform in an ink that reads on the
  panel; nothing of the grid is drawn over the waveform, where a drag paints the time selection.
- **The grid's value** is the editor's own, as the piano roll's is its own: Adaptive at the
  Narrow width (the lines follow the zoom) until another is picked, in the header (› The header and the tabs: the grid's value, Snap) or
  with the piano roll's keys while the editor has the keyboard (it, or the clip's panel, was
  pressed last): **Ctrl+1** finer, **Ctrl+2** coarser, **Ctrl+3** triplets, **Ctrl+4** snapping on
  or off. The status bar says the grid picked. A fixed value too fine for the zoom is drawn and
  snapped to coarser, so lines are never closer than 10 points. With snapping off (or Alt held)
  marks go where the pointer is. The Arrangement's grid is left alone.
- **Under the pointer**, a thin line down the waveform and, on the ruler, where it is as
  `bar.beat.sixteenth`, both following the pointer exactly: nothing jumps from one transient or
  grid line to the next as it moves, and over the waveform nothing on the marker lane answers it.

The numbers and the strip take the theme's `label` and `detail-fill`; the lines are drawn from the panel's color.

### The loop row

A strip under the ruler (10 points) with the clip's loop as a bar, an arrow pointing in at each
end, in the clip's color; with the loop off the bar is gray over the part the clip plays. The
loop's edges also run as lines down the marker lane and the waveform.

| Gesture | Effect |
|---|---|
| A click on the bar | Selects it: the time selection becomes the loop and the bar is lit (`piano-time-selection`'s accent), the pointer a four-way arrow over it to drag it. It never switches the loop: the clip panel's **Loop** button does. A click beside the bar does nothing |
| A drag of the bar's end (the pointer a two-way arrow within 10 points of it, outside the bar or in, in at most a third of the bar's width) | Moves that end, on the grid (Alt: free), at least a grid step from the other (`clip.setLoop` with `start` and `end`) |
| A drag of the bar (the pointer the four-way move arrow between its ends) | Moves the whole loop, its length kept, on the grid (Alt: free) |

Under the bar, in the row's lower half, the clip's **start marker**: a triangle pointing right
where playback of the clip begins. A drag moves it on the grid (Alt: free), from 0 to just before
the loop's end, or the end of what the clip plays (`clip.setStart`). The part before it (and
before the loop's start, when that is earlier) and after the loop's end is dimmed.

The bar, the start marker and the dimmed part are drawn where a drag has them until the button is
let go. The start marker goes with the loop: at the loop's start (as Ctrl+L leaves it) it follows
the loop's start; inside the loop it moves with a drag of the whole bar by as much; before the
loop (a lead-in) it stays. When it moves, that is a second command (`clip.setStart`) and a second
undo step.

### The marker lane

A strip under the ruler (14 points) that holds:

- **Warp markers**: a flag (a small block pointing down) for each marker of a warped clip, in the
  clip's color, with its line down the waveform; the marker the pointer is on, the ones picked and
  the ones being dragged are drawn in the title ink, their line wider, and the pointer is a two-way
  arrow over a marker and while one is dragged. With two or more picked, a band in the title ink runs along the lane from the first of them to the last, a bar along its top joining their flags. The flag takes hold of the marker; the line does
  only with *Drag warp markers by their line* on (off by default), so a drag beside a line paints
  the time selection.
- **Transients**: a small gray mark where each note of the file starts that has no marker on it:
  a suggested place for a marker. Candidates are found from the sound's spectrum (SuperFlux spectral
  flux: a rise of each frequency's level over the loudest of it and its neighbours 10 ms before),
  and a candidate is a note when the pitches sounding change across it (the powers of semitone
  bands, octaves told apart, 45 ms either side) or its level rises at least 11 dB (a drum, a
  chord struck again). So a note that starts with no jump in level, as in a legato arpeggio,
  counts, while the wobble of a held chord, as a piano's, or a swell does not. Each is moved onto
  the attack the level finds within 25 ms of it, and they are at least 100 ms apart. Zoomed out so far that
  those on show would be closer than 12 points on average, each is a thin line on the lane
  instead of a mark, and a double-click on the lane goes on the grid: a marker put down there
  could not be told from the next. With the
  pointer on the lane, the one a double-click there would put a marker on is lit, with the
  outline of a marker's flag over it and a faint line down the waveform. They are found once per file.
- **Play start and playhead**: where playback starts next (an arrangement clip; a click on the
  waveform moves it) and, while the transport runs, the playhead, each a small mark on the lane
  with its line down the waveform, in the clip's content through its start, rate and loop, as the
  piano roll shows them.

### The rest

- **The part the clip plays** is lit: from its start (the offset) to its loop's end, or to where
  its length reaches. What it does not play is dimmed.
- **Loop**: a line in the clip's color at each edge of the loop.
- **Warp markers** (› The marker lane): a clip that is not warped has none, and says *Warp is
  off*: turn Warp on in the clip panel.
- **The time selection**: tinted over the waveform, with its edges and a bar along the ruler's
  lower edge (› Selecting time).
- The waveform is drawn through the clip's gain envelope (a point being dragged included), as the
  Arrangement draws the clip, so it shows what is heard; the clip's gain and fades are not
  applied.
- A file whose waveform is not there yet says *The audio is not loaded*; one with no length says
  so too. Warp markers that stand still in the file or go back (a project from elsewhere; the
  engine plays them) are drawn as they play, and the file's place on the beats is read through
  those of them that rise, so such a clip is still shown and edited.

### Waveform color

The waveform follows **Settings › Clip editor › Audio editor › Audio editor waveform color**, its
own setting: the Arrangement's *Waveform and note color* chooses an ink for drawing over a clip's
colored body, which this view has none of.

| Setting | The editor's waveform |
|---|---|
| Track color (the default) | The clip's color: its own when it has one, else its track's, as the Arrangement colors the clip |
| Theme | The theme's `clip-waveform` `color` where a theme gives one; else a neutral ink that reads on the editor's panel |

## Warp markers

Markers can be edited while the clip is warped (Warp on, two markers or more). Each edit is one
command and one undo step, and the clip is heard with it at once.

| Gesture | Effect | Command |
|---|---|---|
| Double-click on the waveform | Pins a marker on the grid line nearest the double-click (Alt: exactly there), where the sound is, so nothing moves yet | `clip.addWarpMarker` |
| Double-click on the marker lane | The same, onto the transient suggested nearest the pointer when one is within 8 points (lit while the pointer is on the lane), else on the grid line nearest it (Alt: exactly where the double-click is, past any transient) | `clip.addWarpMarker` |
| Drag a marker by its flag on the marker lane (or by its line down the waveform, with Settings › Clip editor › Audio editor › *Drag warp markers by their line* on) | Moves the marker, on the grid (Alt: free), and the sound pinned to it with it, so what plays between it and its neighbours stretches or squeezes. The waveform is drawn moving while it is dragged and the ruler says the beat it is at | `clip.moveWarpMarker` |
| Double-click a marker | Takes it away; the audio between its neighbours then runs evenly. A warped clip keeps two | `clip.removeWarpMarker` |
| Click a marker | Picks it alone (lit while picked); with **Ctrl** or **Shift** held, adds it to the markers picked, or takes it out when it is one. Painting a time selection picks the markers inside it; a click on the waveform away from the markers picks none | |
| Drag a picked marker while others are picked | Moves them all by as much, each with its sound; none passes a marker that is not picked. One undo step | `clip.moveWarpMarkers` |
| **Delete** | Takes away the markers picked (one undo step), or the one under the pointer when none is; nothing when that would leave fewer than the two a warped clip keeps | `clip.removeWarpMarker`, `clip.removeWarpMarkers` |
| **Transients** in the header | Pins a marker on every note start of the file (the transients the marker lane suggests), each where its sound plays, so nothing moves yet; a transient near a marker keeps that marker. One undo step | `clip.addWarpMarkersAtTransients` |

A marker never passes its neighbours (it stops a 64th of a beat short of them). A marker dragged
past the editor's edge goes on following the pointer, and moves when the button is let go there.
A marker let go is drawn where it was let go until the project has the move, so the sound never
flashes back to where the drag began.

**The keyboard.** While the editor has the keyboard (it, or the clip's panel, was pressed last),
the edit keys act on its markers or on nothing, never on the clip behind it, as in the piano roll:
Delete removes a marker as above, the loop key (**Ctrl+L**, the piano roll's) loops the clip
(› Selecting time), and Duplicate and Mute do nothing. Copy, Cut and Paste do nothing while the
editor has a time selection of its own; without one they go on to the Arrangement, so a click in
the editor to set the play start and then Ctrl+V pastes a time selection copied from the tracks
there, each part on its own track, and Ctrl+C or Ctrl+X acts on the Arrangement's time selection. A press in
another view, or the Clip / Devices switch on Devices, gives the keyboard back to that view, also
while the editor shows no waveform (its audio not loaded). While the Reset warp dialog is open the
keys do nothing, as behind the window's other dialogs (› Reset warp).

**The end of the audio.** Moving a marker moves where the audio ends in beats (dragging the last
one later stretches the whole file longer). The clip keeps its start and length, and the end its
right edge can be drawn out to in the Arrangement (`clip.trim`) follows the audio's end: a clip
whose audio was stretched past it is drawn out over the rest with a drag, without setting its length
first. A reset follows it the same way, and so does Warp turned on or off. Where the audio ends is
read as the engine plays the clip: by its warp markers when warped, else at the project's tempo,
wherever the clip lies on the song's tempo map (unwarped audio plays at its file's speed, and only
its start and end follow the map). The audio of a clip that other clips play too is copied for
it first, so theirs keeps its end.

**Note starts found once.** The editor finds the note starts of the file on a thread of its own and
gives them to the commands that place markers on them (`clip.addWarpMarkersAtTransients`,
`clip.quantizeWarpMarkers`) as `transients`, seconds in the file, so a long file is not analysed
again while the window waits. Transients, Ctrl+U or the menu's items, used while the note starts
are still being found, wait for them, the status bar saying *Finding where the notes start…*, and
are sent once they are there, in the project they were made in (one switched away from drops
them); the window never analyses the file on its own thread. One that waits names its clip by id,
not by its slot, so it edits that clip wherever it is then, and never another clip moved into the
slot meanwhile. A reset of the clip's warp (› Reset warp) drops those still waiting for that clip, so
they do not put markers back after it; and while a dialog holds the window's actions, they wait on
and are sent once it closes. Left out, the command analyses the
file itself (the CLI and agents).
A list given is checked: at most 100 000 numbers, each 0 or more; it is sorted, each second kept
once, and seconds past the end of the audio are left out. `clip.resetWarp` takes the same list and
checks it, but does not need it: its `detect` reads the tempo from the beat of the sound's level,
not from where notes start.

To put a snare
on a beat: zoom in until the grid has lines near it, double-click on the snare's start to pin it,
and drag the marker to the beat.

## Several clips

With several audio clips picked in the Arrangement (a multitrack, say), the editor shows the one
picked last; the header's name is then a list, **N samples selected**, of every picked clip, and
picking one shows it (`view.focusClip`, as the panel's *Editing* list). A warp marker edit made on
the clip shown (adding, moving, removing, several picked markers moved or removed together, from a
drag, a double-click, Delete or the right-click menu) is made at the same beats on every picked
clip as long as it and warped, one undo step for all (each clip's own command in one gesture): a
clip without a marker on the beat moved gets one there first, pinned where its sound is, so its
sound moves with the others'; a removal is made only where the clip has that marker and keeps two.
A move goes to every such clip or to none: when it would take one of their markers onto or past its
neighbour (closer than a 64th of a beat), or a marker one of them needs first would lie before its
file's start, it is made on the clip shown alone, so no clip is left with a marker added for a move
it then refused.
So a band recorded together is put in time with the metronome by warping its drums. Clips of
another length are left alone. Quantize and markers at transients act on the clip shown, from its
own sound.

## Selecting time and playing from a point

| Gesture | Effect |
|---|---|
| A click on the waveform or the marker lane (not on a marker) | An arrangement clip: the play start marker moves there, on the grid (Alt: free), as a click in a clip's body in the Arrangement does: the next start begins there, and while playing, playback goes on where it is (`transport.setStart`, #657). At once, as there: a double-click's first click moves it too. The time selection goes |
| A drag on the waveform or the marker lane (not on a marker) | Paints the time selection, on the grid (Alt: free) |
| Shift and a click | Carries the time selection's nearer end to the point |
| **Ctrl+A** | Select all, the piano roll's key: the time selection becomes the whole file, every warp marker picked with it |
| **Ctrl+U** | Quantise, the piano roll's key: each transient in the time selection (none: the part of the file the clip plays, from its start marker to its end or loop end) gets a warp marker where it plays, then every marker there moves towards the nearest line of the quantise grid by the quantise amount, its sound with it (`clip.quantizeWarpMarkers`, one undo step). Two that would land on one line: a new one is left out, one the clip had stays, and every marker that would move onto or past one that stays (outside the selection, or kept) goes back to its own beat, while the moves that still fit are made. The grid (the piano roll's quantise grids, 1/16 to start) and the amount (100 % to start; 75, 50 and 25 %) are set in the right-click menu's *Quantize settings* |
| **Ctrl+L** | Loops the clip over the time selection and starts it there, so the clip plays that stretch round and round over its whole length (`clip.setLoop` with `start` and `end`, then `clip.setStart` at the loop's start: two undo steps); a selection that begins before the clip's first beat (a warp marker can put the file's start there) loops from beat 0. Pressed again over the same loop, turns it off. With no time selection it turns the clip's loop on or off (`clip.setLoop`); a selection too short to be a loop (under a thousandth of a beat, or wholly before beat 0) leaves the loop as it is, and *Loop selection* in the right-click menu is grayed for it |

### Reset warp

The **Reset…** button in the header (*Reset warp…* to a screen reader) opens a dialog that says how many markers the clip has and
asks:

- **Keep warp markers**: closes it and changes nothing (so do Escape and its Close button);
- **Reset to clip tempo**: replaces the markers with two, one at each end of the audio, at the
  tempo the clip has (the project's for a clip that was not warped, which then counts as a tempo set
  by hand), so the audio runs evenly again
  (`clip.resetWarp`);
- **Find tempo again**: the same, at the tempo found in the file's metadata, then its name, then
  its sound (`clip.resetWarp` with `detect`); with none found it changes nothing and says so.

Either reset turns warp on, keeps the clip's start, length and loop where they are, and is one undo
step.

## Right-click menu

A right click on the waveform, the marker lane or a marker opens a menu at the pointer, compact
and in the same order as the piano roll's: each item with its key at its right end, grayed where it
has nothing to act on (rather than left out, so the items stay where the hand expects them). Each
runs a command (one undo step) or changes the view; the point clicked is where the items that need
one act (on the grid, Alt held when the menu opened: free).

| Item | Does |
|---|---|
| **Delete** | The picked markers, or the marker the menu was opened on, as the Delete key (`clip.removeWarpMarker`, `clip.removeWarpMarkers`); grayed when that would leave fewer than two |
| **Quantize** | As Ctrl+U (`clip.quantizeWarpMarkers`); grayed for a clip that is not warped |
| **Quantize settings: 1/16, 100 %** | A submenu of the quantise grids and the amounts (100, 75, 50, 25 %), the name saying the ones chosen |
| **Add warp marker here** | A marker at the point clicked, where its sound is (`clip.addWarpMarker`); on the marker lane, on the transient suggested near it, as a double-click there |
| **Add warp markers at transients** | A marker on every note start of the file, as the header's Transients (`clip.addWarpMarkersAtTransients`) |
| **Reset warp…** | The Reset warp dialog (› Reset warp) |
| **Loop selection** | With a time selection: loops the clip over it and starts it there, as Ctrl+L |
| **Set clip start here** | The clip's start marker to the point clicked (`clip.setStart`) |
| **Set loop start here**, **Set loop end here** | A looping clip: that end of its loop to the point clicked, kept a grid step from the other, the start never before beat 0 (a loop shorter than a step from beat 0: its start at 0); grayed where the point is not before the end (after the start) by half that (`clip.setLoop`) |
| **Split clip here** | An arrangement clip: cut in two where the point clicked first plays (`clip.split`): in a looping clip, a beat of the loop before the start marker where the loop comes round to it |
| **Zoom to fit** | The file fitted to the area again, as Fit |
| The grid's entries | Snap, the adaptive widths, the fixed values and triplets, as the header's grid list and the piano roll's keys (Ctrl+1 to Ctrl+4) |

Cut, copy, paste and duplicate of the audio in a time selection, warp from a point (re-warping
the audio after it), a clip's first beat for audio, normalizing, cropping the file and turning
audio into MIDI need commands of their own and are not in the menu yet.

## Zoom and scroll

| Gesture | Effect |
|---|---|
| Opening a clip | The file is fitted to the area, with 8 points left at each end |
| Ctrl and the wheel (or a pinch) | Zoom, the beat under the pointer staying under it |
| The wheel | Scroll along the file |
| A drag with the middle button held | Drags the view along the file, as in the piano roll and the other views |
| **Fit** in the header, or a double-click on the ruler | Fit the file again |
| A click on the ruler | Playback jumps there, as a click on the Arrangement's ruler does: to where that point first plays in the arrangement (in a looping clip, a beat of the loop before the start marker where the loop comes round to it; one the clip never plays, its start or end), on the grid (Alt: free), while playing at the next launch boundary (`transport.jump`); an arrangement clip only. At once, with no wait for a second click: a double-click's first click jumps too |
| A drag on the ruler | Up zooms in and down zooms out, the beat where the drag began staying put; sideways scrolls. The pointer is a magnifier over the ruler and hidden while the button is held, as on the piano roll's ruler |

Zoom stays between 0.05 and 20 000 points per beat, scroll between the file's two ends. Another
clip starts fitted. The zoom and scroll are the view's and are not saved with the project.

## Later

- A marker dragged near a transient snapping to it, and Alt-drag to move a marker's sound without
  its neighbours stretching.
- Vertical zoom of the waveform.
