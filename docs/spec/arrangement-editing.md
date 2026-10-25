---
description: How arrangement clips behave when moved (along a lane, to another track, over other clips), how audio clips fade in and out, how a group's tracks fold away, where playback starts and stops (the play start marker), how clips are split and consolidated, and named markers on the timeline
read_when: start marker, clip offset, loop start, moving clips, clip.move, overlapping clips, clip stacking, trimming, splitting, clip.split, split at playhead, cut a clip, consolidate, clip.consolidate, join clips, render a range, bounce, allow sample stacking, allow MIDI stacking, fades, fade-in, fade-out, clip.setFades, fade handles, fade shape, fade curve, curve handle, inShape, outShape, inCurve, outCurve, linear, equal_power, fast, slow, s_curve, FadeShape, FadeCurve, play position, play start marker, ruler click, transport.setPosition, stop returns, playhead, grid, grid lines, snapping, snap to grid, launcher.setQuantization, quantisation, Alt, off the grid, markers, locators, marker row, marker.add, marker.move, marker.rename, marker.delete, marker.jump, project.markers, fold, folded group, unfold, group triangle, group lane, volume, pan, mute, solo, mixer controls, knob, the wheel over a value, deselect, click on empty space, clip note preview, short lane
type: spec
status: draft
last_verified: 2026-10-25
covers:
  - crates/session/src/commands.rs
  - crates/ui/src/arrangement/timeline.rs
  - crates/ui/src/arrangement/arrangement_view.rs
  - crates/ui/src/arrangement/automation_lanes.rs
  - crates/ui/src/interaction/timeline_grid.rs
  - crates/model/src/lib.rs
  - crates/model/src/overlap.rs
  - crates/engine/src/audio.rs
  - crates/engine/src/fade_cost_tests.rs
  - crates/engine/src/launcher.rs
  - crates/engine/src/loop_tests.rs
  - apps/suhina-cli/src/play.rs
  - crates/ui/src/interaction/menus.rs
  - crates/ui/src/interaction/colour_picker.rs
  - crates/ui/src/interaction/drop.rs
  - crates/session/src/files.rs
  - crates/project-file/src/lib.rs
  - crates/ui/src/settings.rs
  - crates/ui/src/settings/keymap.rs
  - crates/session/src/editing.rs
  - crates/engine/src/track_render.rs
  - crates/ui/src/app.rs
  - crates/session/src/lib.rs
---

# Arrangement editing

How arrangement clips behave when they are moved. The familiar behaviour (observed in Live 12.4,
which this follows) is that a clip placed over others **replaces** what it covers. Some users come
from pattern-based DAWs where clips **stack** and all play; two settings allow that.

`arrangement.audioOverlaps {track?}` lists the pairs of audio clips that sound together on one
track: their spans share time, by their start and end. Clips that only touch (one ends within
1e-9 beats of where the next starts, as the sub-rows count it) do not overlap, and clips that play
notes are left out. Each pair has the track, the shared span (`start`, `end`, in beats), `kind`
(`partial`, or `contained` when one clip lies within the other) and both clips with their `id`
(what `clip.move` and the other clip commands take, ADR 0025), `track`, `index` and name; at most 10 000 pairs, `truncated` saying when
there were more. ARA's first version refuses such overlaps for the clips it edits
([ADR 0028](../decisions/0028-ara-hosting.md)); ordinary stacking is unaffected.

## Moving a clip

`clip.move {track, index, start, toTrack?, stack?}` moves one arrangement clip to `start` (beats,
at least 0), on its own track or on `toTrack`, which must take the clip's kind: a MIDI clip goes
to a track that plays notes, an audio clip to an audio track. A clip that moves to another track
plays through that track's instrument and effects. The move is one undo step.

In the Arrangement view a clip is moved by dragging it, along its lane or onto another; it lands
on the nearest line of [the grid](#the-grid), or where it is let go with Alt held (at the end of
the drag; the clip is drawn where it will land while it is dragged).

## The grid

The Arrangement view has one grid, drawn as vertical lines across the lanes and used by
everything that snaps. Its value is the box in the control bar next to the tempo and the time
signature (`none`, `8 bars` … `1 bar`, `1/2`, `1/2T` … `1/16T`, `1/32`), the same value that sets
when launched clips start ([clip launching](clip-launching.md) › Launch quantisation): changing it
runs `launcher.setQuantization`. Note values are quarter-note beats (`1/16` is a quarter of a beat,
a `T` triplet two thirds of that note), and bars are the time signature's bar. The value is kept by
each open project's session, so every project tab has its own; it is not saved in the project
file and is not an undo step (a new or opened project starts on `1 bar`).

**Lines follow the zoom.** A line is drawn at every step of the value, starting again at every bar
line, and bar lines are stronger than the lines between them. Lines are never drawn closer than
**8 points**: where they would be, the grid coarsens until they are at least that far apart. A
straight note value doubles (`1/16` to `1/8` to `1/4`) up to the beat; a triplet finer than a beat
goes straight to the beat, so lines stay on the bar; from the beat it goes to the bar, then 2, 4,
8 bars and so on. A value of a bar or more is never drawn finer than it is. So with `1/16`, zoomed
in, a beat has four lines; zoomed out, lines fall on beats, then on bars.

**What snaps.** Everything placed in the view goes to the nearest line of the grid as it is drawn
at that zoom (never before the start): a clip moved by dragging ([moving a clip](#moving-a-clip)),
samples and MIDI files dragged from the browser onto a lane or the empty space below the tracks
([browser](browser.md) › Dragging into the project), fade handles ([fades](#fades)) and ruler
clicks ([play position](#play-position)), and automation points added or dragged in an
automation lane. With **Alt** held, each of them goes exactly where the pointer is instead (off the
grid). So in 3/4 with a `1/2` grid, whose lines are at beats 0, 2, 3, 5, 6 …, an automation point
goes to beat 3, not 4.

**One grid for every timeline.** The arrangement, its automation lanes and the piano roll
([clip editor](ui-clip-editor.md) › Grid and snapping) draw and snap with the same code,
`TimelineGrid` in `crates/ui/src/interaction/timeline_grid.rs` (#734). Each view keeps its own grid
value (the control bar's here, the piano roll's grid menu there) and its own least spacing between
lines; only the arithmetic is shared.

**`none`** launches clips at once, and in the Arrangement view nothing snaps (as if Alt were held);
lines show beats and bars by the zoom alone.

## Selecting clips

A click on a clip selects it alone; **Ctrl** (Cmd) adds a clip to the selection or takes it out,
and **Shift** takes the rectangle from the clip clicked last to this one: the lanes between them,
as the view draws them, by the time from the earlier start to the later end, every clip that
touches it (#694). Ctrl+A selects every clip
in the arrangement. Pressing a clip that is part of the selection keeps the selection, so a drag can
start there; pressing one that is not selects that clip alone. Dragging a clip of the selection
moves all of it by the same beats, at once (`clip.moveMany {clips, beats, toTrack?, tracksBy?,
toNewTrack?, stack?}`, one undo step, so no moved clip replaces another on the way). Dragged up
or down, a selection on one track goes to the track under the pointer, and a selection on several
tracks moves each clip by as many tracks from its own (`tracksBy`), refused where a clip would
land on a track that cannot hold it. A clip an ARA plug-in (Melodyne) edits, going to another
track, asks first: **Keep edits** takes its edits along and puts the plug-in on that track when it
has none, **Copy as new** starts it there without them (`araEdits`, ADR 0028). Dropped on the empty space under the tracks, outlined there
as **New track** while carried, the clips (moved or copied) go to a new track of their kind made
after the last (`toNewTrack`), in the same undo step.

A click on an empty spot of a lane, or on the empty space under the tracks, clears the clip
selection (a double-click there still makes a clip, › Making an empty clip). The track
selection stays, so the detail area shows the selected track's devices (or its hint); it does not
close or hide the detail area ([clip editor](ui-clip-editor.md) › Least height and closing).

A clip shows what it holds at every lane height, down to the lane's least 20 px: a MIDI clip draws
a line per note in the strip it has under its name, and from below 30 px over the name's row; an
audio clip its waveform the same way ([components](../design/components.md) › MIDI marks).

In the track headers the same keys work on tracks: a click selects one, Ctrl adds or takes one out,
Shift takes the run of headers between the last one clicked and this one. Ctrl+G groups those.
After a header click Delete removes the selected tracks, not a clip (a clip on the track stays
selected for the detail area until another is picked); a click on a clip or a lane gives Delete
back to the clips. In the Session view a header click does the same for its track (#690). The
header shows its track's colour strongly (60 % over the panel); a selected track's is brighter
and more saturated, with an outline in it. A track keeps the colour it shows when tracks are
added, removed or moved: one without a colour of its own gets its place's palette colour stored
then, in the same undo step, and a new track gets its place's colour stored as it is made.

## Clip colours

A clip is drawn in its colour: the name bar along its top in full colour, the body a lighter
shade of it, its notes or waveform in ink that reads on it (dark on a light colour). A clip with
no colour of its own (`Clip::color` is `None`) shows its track's, so a new clip takes the
track's colour and follows it. Moved to another track, a clip keeps the colour it showed
(`clip.move` and `clip.moveMany` take `color`). The track menu's *Assign track color to all
clips* (`clip.setColor {track, color: "none"}`) hands every clip of the track back to the
track's colour; *Rename all clips* (`track.renameClips`) names them after the track. The track
headers are tinted with their track's colour, deeper while selected.

A clip on a lane at least 36 px tall has a name bar along its top. By default (Settings ›
Arrangement › *Moving a clip*: **Name bar**, `clip_drag = name_bar`) a drag from the name bar
moves the clip (a hand cursor) and a drag from its body below paints a time selection (a text
cursor) over the tracks it crosses. **Mirror** (`clip_drag = halves`) splits the clip in two
instead: its upper half paints time and its lower half moves it. An empty spot of a lane is split
in two either way: its upper half paints time, its lower half draws a rectangle that picks whole
clips and paints no time. A clip on a lane under 36 px moves from its lower half.

A click on a clip's name bar picks the whole clip. A click in its body picks it too and sets the
play start where it was clicked, on the grid (Alt: free), as a click on an empty spot of a lane
does; a thin line (the insert marker) shows it in that lane while the play start is there, also
while the transport plays and the playhead moves on. A clip picked this way is not lit but
outlined, so the clip the detail area shows and Delete takes is plain to see; a clip picked whole
is lit. The grid's lines show through a clip's body, faintly in its own colour,
the bars a little stronger (Settings › Arrangement › *Grid in clips*, `clip_grid = on | off`,
on by default). With *Ctrl+drag selects time
over clips* on, a plain drag moves clips and Ctrl paints time.
Ctrl (Cmd) held as the drag begins copies the clips instead (Shift when Ctrl selects time):
they stay, and copies with contents of their own go where they are dropped (`clip.moveMany`
with `copy`), the cursor showing the copy sign; while dragging, the originals stay drawn and the
copies show where they will land. Dragging a selection draws every clip of it where it goes. A time selection is drawn in the accent over
the clips with a line at each edge, and as a bar along the ruler.

Stacked clips keep their sub-rows: a clip of a stack deleted or moved away leaves the others
where they were, an empty row between them staying until the rows above it empty too. A new clip
inside the span a clip had on the same track (a split's parts) takes that clip's row (#703).

**Waveform zoom.** A thin slider at the outer edge of the header column's top corner, by the
ruler, draws every audio clip's waveform in the Arrangement larger, ×1 … ×16: one zoom for all
tracks, the display only, never the sound or a clip's gain. Drag it up or down (thirty points
double it), scroll over it a step at a time (×1.25), double-click it for ×1; Ctrl+Alt+Up and
Ctrl+Alt+Down step it too. It is kept as a setting (Settings › Arrangement › *Waveform zoom*,
`waveform_zoom`, the factor).

The **Hide returns** button in the header column's top corner, by the ruler, takes the return
tracks' rows out of the view and **Show returns** brings them back; it is kept as a setting
(Settings › Arrangement › Return tracks, `arrangement_returns`). Main's row stays.
Return tracks, along the bottom, have a track's header: volume and pan, the on switch and solo,
the menu, automation lanes under them (shown with the header's button), a click to select them
(the detail area shows their devices) and a top edge that sets their height (tall enough for
volume and pan by default): the fixed rows sit along the bottom, so they grow upward, as the
pointer goes. The main track's row has a volume bar that a drag sets (`mixer.setMainVolume`, heard
while dragging, one undo step), and its top edge sets its height the same way.

## Clip edges

A clip's left and right edges are handles (below the fade handles of an audio clip). Dragging one
moves that edge on the grid (Alt: free, as for every other drag) and the clip is drawn at its new
length as it goes; it never gets shorter than a grid step (a 16th of a beat with Alt or no grid).
Let go, it is
`clip.trim {track, index, start?, end?}`:

- The left edge moves the content with it, so what plays stays where it was; it goes no earlier
  than the content's start.
- A clip that does not loop ends at most where its audio ends; a MIDI clip can be made as long as
  wanted, its content growing with it.
- A looping clip goes on as far as its edge is dragged, its loop repeating: its right edge into
  later passes, its left edge back into earlier ones (to the song's start). Its repeats are
  drawn as the clip itself, in its colour, one colour all along; where each pass starts it has a
  notch cut into its top and bottom edges and a dashed line between them, so the repeats read as
  blocks even in an empty clip (#700). A MIDI clip's notes are drawn in every pass.
- The right edge is one handle over the clip's full height; there is no separate loop handle
  at the corner (#700). The loop is turned on in the clip column (› Clip column in
  [ui-clip-editor.md](ui-clip-editor.md)) or with `clip.setLoop`, and a looping clip's edge then
  draws out its repeats.

With Ctrl and Alt held (Settings › Arrangement › *Ctrl+Alt-drag on an edge stretches audio*, on by
default) an
audio clip's edge stretches its audio to the new length, its pitch kept: `clip.stretch {track, index, start?, end?}` scales the content's warp markers (an unwarped
clip is warped first) and gives the clip a content of its own when another clip plays the same.

A time selection over a clip and empty time, consolidated (Ctrl+J), becomes one clip over the
whole span, its empty part silent.

## Time selection

A time selection is a span of beats over some tracks; the keys then act on what plays inside it,
not on whole clips. It is made by:

- a drag from an empty spot of a lane (the rectangle's tracks and beats);
- a drag on the ruler: that time over every track;
- over a clip, as Settings › Arrangement › *Moving a clip* and *Ctrl+drag selects time over
  clips* say: off (the default) a drag from the clip's body selects time and the clip moves by
  its name bar (with **Mirror**, from its upper and lower half); on, a drag with Ctrl held
  selects time anywhere and a plain drag moves the clip.

Its edges are on the grid (Alt: free). A click on a clip, or moving one, ends it, so clicked
clips act whole again (Ctrl+A, Shift and Ctrl clicks still pick whole clips).

With a time selection:

| Key | Command | What it does |
|---|---|---|
| Delete | `range.delete` | Removes what plays inside; clips crossing an edge keep their parts outside it, and the gap stays. |
| Ctrl+X | `range.cut` | Copies what plays inside, cut to it, and removes it. |
| Ctrl+C | `range.copy` | Copies what plays inside, cut to it. |
| Ctrl+V | `clip.paste {start}` | A copy from several tracks goes back to its own tracks, from the play start marker. |
| Ctrl+E | `range.split` | Splits the clips at both edges: the span becomes clips of its own. |
| Ctrl+D | `range.duplicate` | Puts a copy right after the span on the same tracks; the selection moves onto the copy. |
| Ctrl+J | `clip.consolidate` | Consolidates the span on each of its tracks; a span painted in one row of a stack, that row's clips only. |

Each takes `{tracks, start, end}` and is one undo step. Closing the gap (deleting the time
itself) is not built yet.

## Duplicating and deleting

`clip.duplicate {clips, stack?}` places a copy of each named clip right after it, replacing what
the copies cover unless stacked, as a moved clip does (the window stacks when the setting for the
clip's kind is on); `clip.delete {clips}` removes them. Both take a list of `{ track, index }` as
`project.clips` lists them, at least one, and make one undo step for the whole list
(`clip.delete` also takes one clip as `{track, index}`, or a slot's clip as `{track, scene}`). In the
Arrangement view a clip's right-click menu has both, and Mute, and they act on the whole
selection when the click fell inside it, else on the one clip. The keys Ctrl+D and Delete do the
same for the selection. Points selected on an automation lane are deleted by Delete before any
clip; selecting a clip ends a selection of points, and selecting points ends the clips' selection
([automation](automation.md#in-the-arrangement-view)).

## Automation lanes

Under a track, the lanes of its automation, shown and hidden with the envelope button on its
header; a track's row grows by their height, and the next track moves down. Their gestures are
in the [automation spec](automation.md#in-the-arrangement-view).

## Making an empty clip

`clip.create {track, scene? | start?, length?, stack?}` makes an empty clip, one bar long, named
after its kind (a "MIDI clip" or an "Audio clip"), on a track that holds clips: MIDI on a track
that plays notes, an "Audio clip" with no file yet on a track that plays audio and not notes; an
audio clip so far plays silence until a sample is dropped on its slot or lane. A group or a return
is refused. It goes either in clip slot `scene` (from 1, an existing scene; the slot must be
empty), where it loops over its length, or at beat `start` in the arrangement (0 up to the
engine's last beat), where it replaces what it covers unless stacked, as a pasted clip does.
`length` is in beats, one bar of the project's time signature by default (four beats in 4/4, three
in 3/4), more than 0. It is one undo step and returns the track and the scene, or the start and
the clip's index as `project.clips` counts it.

In the Arrangement view a double-click on an empty spot of a lane that holds clips makes one a bar
long, starting at the grid line before the pointer (with Alt, exactly at the pointer), and selects
it; it stacks when the stacking setting for the track's kind is on (MIDI stacking, or audio
stacking). A double-click on a clip is the clip's own. In the Session view a double-click on an
empty slot makes one there ([Session view](ui-session-view.md) › Clip slots).

Both double-clicks follow one setting, Settings › Arrangement › **Double-click makes a clip** (On by
default, saved as `double_click_clip`). Off, a double-click on an empty spot of a lane, or on an
empty slot, does nothing: no clip, and the selection stays as the first click left it.
`clip.create` sent any other way (the command line, MCP) is not affected.

## Copy, cut and paste

`clip.copy {clips}` keeps a copy of the clips named in `clips` and what they play; `clip.cut` also
deletes them; `clip.paste {track, start, stack?}` places the copies on a track that takes their
kind, replacing what they cover unless stacked. The earliest copied clip lands on `start` and the
others keep their distance from it, so a copied run pastes as a run. The pasted clips play their own
copy of the content, so editing one does not change the other. The window's keys paste after the
selected clip, or at the playhead on the selected track ([shortcuts](../shortcuts.md)), and stack
when the stacking setting for the clip's kind is on, like a moved clip. A clip's right-click menu
has Copy.

The clipboard is shared with the clip slots ([clip launching](clip-launching.md) › Clips between
the arrangement and the slots): `clip.copy {track, scene}` copies a slot's clip, which pastes
onto the arrangement like any other, and `clip.paste {track, scene}` puts the copied
arrangement clip into a slot; a slot holds one clip, so with several copied that paste is an
error. Copying with `clips` and `scene` together, or neither, is an error, and so is pasting with
`start` and `scene` together, or neither; `start` is at most 120000 beats.

**Dragging into a slot, and back.** A clip dragged in the Arrangement view and held over
**Session** in the control bar for half a second switches the view; let go over a slot and the
clip is copied into it, the arrangement left as it was. A slot's clip dragged and held over
**Arrangement** lands on the lane under the pointer, at the beat on the grid, as a copy that
replaces what it covers unless stacked. Both are `clip.copy` then `clip.paste`
([Session view](ui-session-view.md) › Between the views).

## Clips off

`clip.setEnabled {clips, enabled}` turns clips on or off: an off clip is drawn dimmed, plays
nothing and does not sound in a render, so it works as a clip mute. The 0 key turns the selection off,
or back on when nothing in it plays; a clip's menu says Mute or Unmute for the clip it fell on.
## Splitting

`clip.split {track, index, at}` cuts an arrangement clip in two at song position `at` (beats from
the beginning), which must be inside the clip (not on its edges). It is one undo step and returns
both parts' indices (`first`, `second`).

- **Both parts play the same content** (one content in the pool, as before the cut), so editing
  the notes or the audio of one changes the other, as for a duplicated clip.
- **The second part carries on from where the clip was at the cut:** its start marker
  (`offset`) is the content position the clip had reached there, following the loop (a clip
  whose start marker is at or past the loop end starts at the loop start) and, for a transposed
  audio clip that is not warped, the faster or slower speed it moves through its file at
  (`2^(transpose / 12)` content beats per beat). Loop, warp markers, warp mode, gain and
  transpose are the content's or the clip's, and stay as they are.
- **Fades:** the first part keeps the fade-in and the second the fade-out, each with its shape
  and curve; there is no fade at the cut (a zero-length fade, so only the engine's 2 ms click
  guard is there). A fade longer than its part shrinks to fit it, as when a clip is shortened.
- **Notes across the cut end there.** A note that starts before the cut belongs to the first
  part and is cut at its end; the second part does not start it again (notes are not picked up
  mid-way, as when playback starts inside a clip). Notes that start at or after the cut play in
  the second part.

**The renders before and after a split are the same,** sample for sample, for a MIDI clip with
no note across the cut. An audio clip differs only around the cut: within the click guard on
both sides (2 ms), and for a warped clip, whose second part starts its time-stretcher at the cut
without the audio before it, very slightly for its first moments, as when playback starts there
(Play position).

In the Arrangement view **Ctrl+E** splits the selected clip at the playhead (nothing happens,
with a hint in the status bar, when the playhead is not inside it), and a clip's right-click
menu has **Split here**, which cuts where the clip was right-clicked, on the grid clips snap to
at that zoom (Alt held while right-clicking: exactly there).

## Consolidating

`clip.consolidate {track, start, end}` turns the range `start … end` (beats) on one track into
one new clip there. It is one undo step and returns the clip's `index`, and for audio the `file`
written.

- **What is in the range:** the enabled clips on the track's lane that overlap it, or with
  `clips` (clip ids) only those: a time selection painted in one row of a stack consolidates that
  row's clips and leaves the other rows as they are. A clip reaching in from before or past the
  range counts with the part inside it; clips outside are unchanged. As with a split, a note that starts before the range is left out.
- **MIDI clips** are joined: the new clip's content holds every note the clips play inside the
  range, where it sounds (after start markers, loops and clip ends are applied), each cut where
  its clip ends or the range ends. Gaps between clips are silent.
- **Audio clips** are rendered: the track's clips play through the engine's renderer
  (`suhina_engine::render_track`, the same `process` as playback) with their own gain, fades,
  warping and transpose, but no devices, no track volume, pan or mute and no other track, at
  unity gain, straight through: the arrangement's loop region does not wrap it. Gaps are silent. The audio is written as a 32-bit float stereo WAV file to a
  folder **`Consolidated`** next to the project file, named after the track and the bar the range
  starts on (`3 Audio bar 5.wav`; ` 2`, ` 3` … are added when that file exists), at the highest
  sample rate of the files rendered. The new clip plays that file unwarped, at gain 0 dB, without
  fades. A project that has not been saved has no folder for it, so the command asks to save it
  first. Undo removes the clip but leaves the file on disk.
- **The new clip** starts at `start`, is `end − start` long, has its start marker at 0, no loop
  and no fades, and replaces what the range held (the clips under it are shortened, split or
  removed as for a clip placed there, whatever the stacking settings). Its content is named
  after the first clip's.
- **Refused:** a range that is empty, negative, past 120 000 beats or longer than an hour of
  audio from the earliest clip in it; a range with no clips; MIDI and audio clips together; a
  clip that plays on other tracks (a shared pattern).

The render of the project before and after a consolidation is the same, apart from the click
guard at the range's edges when audio is playing there, since the new clip starts and ends
there. An audio clip consolidated plays at the tempo it was rendered at.

In the Arrangement view **Ctrl+J** consolidates the selected clip (its own start to its end,
with any clips stacked over it), and a clip's right-click menu has **Consolidate**. Consolidating
a single clip makes its loop, start marker, fades and warping one plain clip.

## Fades

An audio clip on the arrangement has a **fade-in** and a **fade-out**, each a length in beats
(`Clip::fade_in`, `Clip::fade_out` in the model; 0 is no fade). Both are at least 0 and together
at most the clip's length; a project that breaks this does not validate
(`ValidationError::BadFades`), so a renderer refuses it instead of guessing.

Each fade also has a **shape** and a **curve** (`Clip::fade_in_curve`, `Clip::fade_out_curve`:
a `FadeCurve` of a `FadeShape` and a number). The default is linear with curve 0 at both ends,
so a project without them sounds as it did before shapes existed.

`clip.setFades {track, index, in?, out?, inShape?, outShape?, inCurve?, outCurve?}` sets any of
them, lengths in beats; one undo step. It refuses a negative length, fades together longer than
the clip, an unknown shape, a curve outside −1 … +1, a MIDI clip, and a call with none of them.
With `scene` instead of `index` it sets the fades of the clip in that track's launcher slot
([clip launching](clip-launching.md) › Clip fades).
`project.clips` lists each clip's `fade_in` and `fade_out` and their shapes and curves
(`fade_in_shape`, `fade_in_curve`, `fade_out_shape`, `fade_out_curve`).

**Shapes.** Over a fade, `t` runs from 0 at its silent end to 1 at full level: `t = beats /
fade_in` for the fade-in, and `t = (length − frame − beats) / fade_out` for the fade-out, where
`frame` is the length of one frame in beats. A fade-out is therefore the mirror image of a
fade-in with the same shape. The shape gives the gain `s(t)`:

| Shape | `s(t)` | At ½ | Character |
|---|---|---|---|
| `linear` | `t` | 0.5 (−6 dB) | Straight line in amplitude |
| `equal_power` | `sin(π t / 2)` | 0.707 (−3 dB) | A fade-out and a fade-in of the same length over each other keep the power constant |
| `fast` | `1 − (1 − t)²` | 0.75 | Rises quickly, then levels off |
| `slow` | `t²` | 0.25 (−12 dB) | Stays low, then rises |
| `s_curve` | `t² (3 − 2t)` | 0.5 | Flat at both ends, steepest in the middle |

**Curve.** The curve `c`, −1 … +1, bends the shape: `gain = s(t)^(4^(−c))`. At 0 the gain is the
shape exactly; +1 raises it to the power ¼ (a faster start, a logarithmic feel), −1 to the power 4
(a slower start, an exponential feel). Every shape and curve gives 0 at `t = 0` and 1 at `t = 1`,
and rises in between.

On the frame `beats` into a clip of `length` beats the clip's gain is

```text
fade_in_gain  = gain(fade_in_curve, min(1, beats / fade_in))                      (1 when fade_in is 0)
fade_out_gain = gain(fade_out_curve, min(1, (length - frame - beats) / fade_out)) (1 when fade_out is 0)
gain          = fade_in_gain × fade_out_gain × click guard
```

The fade-in counts the time before the frame and the fade-out the time after it, so the first and
the last frame are both silent. The click guard is the engine's 2 ms linear ramp at both ends of
every audio clip, counted the same way, and the fades multiply with it rather than replacing it. The engine works it out per frame from the clip's
start and end in frames (`audio::Fades`), so fades are sample-accurate and the same in offline
renders and real-time playback, unwarped (direct reader) or warped (time-stretched): the envelope
is applied after the stretcher. A fade follows the tempo, since it is in beats: through a tempo
change its length in time is what the [tempo map](tempo-map.md) makes of its beats. Fades belong to
arrangement clips; a launched clip plays with the click guard only.

When a clip is shortened or split because another clip is placed over it (below), its fades are
fitted to what is left: a split clip's first part keeps the fade-in and the second part the
fade-out, and fades longer together than the part shrink in proportion.

In the Arrangement view an audio clip shows its fades as a line over the waveform, from the
clip's bottom corner to where the fade ends at the top, following the fade's shape and curve,
and the waveform is scaled by the fades.
While the clip is hovered or selected, a small square **handle** sits at its top-left corner
(fade-in) and top-right corner (fade-out); with a fade, the handle sits where the fade ends. A
horizontal drag sets the fade with its end on [the grid](#the-grid) (Alt held: free), up to the other fade;
the clip's own edge and the other fade snap like grid lines, so a clip that does not start or
end on the grid can be faded over its whole length; the clip shows the fade while it is dragged, and letting go runs `clip.setFades`. A
double-click on a handle removes that fade. A right-click on a handle opens a menu of the shapes,
with the fade's shape checked; choosing one runs `clip.setFades`. A fade at least 21 px wide has a
second, round **curve handle** on its line half-way through the fade. Dragging it up or down
moves it with the pointer: the curve becomes the one whose line passes through the handle (up
for +, down for −), until it reaches −1 or +1, the line following live, and letting go runs
`clip.setFades`; a double-click sets the curve back to 0. The view draws the fades per pixel
with `t` in beats (`length − beats` for the fade-out), without the one-frame term above, which
is less than a pixel at any zoom. Measurements are in
[components](../design/components.md) › Arrangement view.

### What fades cost the audio thread

The engine works out each frame's gain inside a fade from its shape and curve: a curve other
than 0 is one `powf` per frame (its exponent is worked out once per clip), equal power one `sin`.
Past the fade nothing is computed. Measured 2026-10-13 on a 4-core Xeon at 2.8 GHz (a cloud
container), release build, `crates/engine/src/fade_cost_tests.rs`: 64 audio tracks of 16
back-to-back clips, each faded in over its first half and out over its second, so every frame of
every track is inside a fade. 512-frame blocks at 48 kHz, budget 10.67 ms; the fastest of three
runs. A second run gave the same figures within about 10 %.

| Fades | Mean block | Load | 99th percentile | Per frame, `Fades::gain` alone |
|---|---|---|---|---|
| none | 447 µs | 4.2 % | 1.27 ms | 8 ns |
| linear, curve 0 | 476 µs | 4.5 % | 1.42 ms | 9 ns |
| S-curve, curve 0 | 480 µs | 4.5 % | 1.51 ms | 11 ns |
| equal power, curve 0 | 588 µs | 5.5 % | 1.83 ms | 19 ns |
| linear, curve ±1 | 839–881 µs | 7.9–8.3 % | 2.46–3.04 ms | 31 ns |
| fast +0.5, slow −0.5, S-curve +1 | 931–952 µs | 8.7–8.9 % | 2.61–3.50 ms | 33–35 ns |
| equal power, curve ±1 | 1.13 ms | 10.6–10.7 % | 2.91–3.09 ms | 47 ns |

A curve adds about 22 ns per frame per clip inside a fade; at 48 kHz that is about 0.1 % of one
core for each clip that is fading at the time. The worst case above, 64 tracks all inside curved
equal-power fades at once, adds 6.5 points of load and stays well within the budget. A project
fades a few clips at a time, so the cost is not worth a cheaper curve, and the engine keeps the
exact `powf`. A lookup table was weighed and left out: near the silent end, `t^¼` (curve +1)
rises too steeply for a table of reasonable size to stay within 10⁻⁶ of the exact gain, which the
fade tests check. Measure again with
`cargo test --release -p suhina-engine fade_cost -- --ignored --nocapture --test-threads=1`.

## Start marker and loop

A clip plays from its start marker (`Clip::offset`, in content beats) and, with a loop, around
the loop from then on. Live keeps the start marker inside the loop: moving the loop moves the
marker with it ([als-format](als-format.md) › Markers). **Suhina:** a project whose start marker
is at or past the loop end starts at the loop start, in every reader alike (notes, audio read
directly or time-stretched, and launched clips).

## Play position

The transport starts from the **play start marker**, a position in beats (0 when a project
opens). While stopped, the playhead sits on the marker.

**Where other DAWs stand** (their public manuals, October 2026): every one starts playback from a
start position the user sets; they differ on the first Stop.

| | First Stop | Stop again, while stopped | Setting |
|---|---|---|---|
| Live | back to where playback started (the insert marker) | the marker to the start | none |
| Bitwig Studio | stays: the start position moves to the playhead | playhead and start position to the start | none |
| Logic Pro | as the Stop button's menu says: stay, back to the left locator, or back to the last start | to the project start | the Stop button's menu |
| Cubase | back to the start position, or stays ("Return to Start Position on Stop") | back to the start position | Preferences › Transport |
| REAPER | Space: back to the start; Ctrl+Space: stays (pause) | | two keys |

Suhina takes the common ground: Stop goes back to the marker by default, staying put is a
setting, and a second Stop goes to the beginning.

**Commands.**

| Command | Does |
|---|---|
| `transport.setPosition {beats}` | Moves the marker to `beats` (0 … 120 000; a negative, too large or non-numeric value is an error). Stopped, the next `transport.play` starts there. Playing, playback jumps there at once. |
| `transport.play {from?, track?, index?}` | `from: "marker"` (the default): starts the transport from the marker; nothing happens while it runs. Launching a clip or scene while stopped also starts the arrangement from the marker. `from: "clip"`: `track` and `index` (from `project.clips`) name an arrangement clip; the marker moves to that clip's start and the transport starts there, or, while it runs, jumps there as `transport.setPosition` does. A missing or unknown clip, a track without an arrangement lane, another `from`, or `track`/`index` without `from: "clip"` is an error and changes nothing. |
| `transport.stop {return?}` | Stops everything at once. `return: "marker"` (the default): the playhead goes back to the marker. `return: "here"`: the marker moves to where the playhead stopped, so the next start carries on from there. Either way, stopping while already stopped moves the marker to beat 0. |

`launcher.state` reports `start_beat` (the marker) next to `beat` and `position` (the playhead;
bars through the time-signature changes, [tempo map](tempo-map.md)).

**A new audio output.** When the output is opened again (`audio.setOutput`, a device that
reported an error, the chosen device back), the marker stays where it was and, while playing,
playback goes on from the playhead: the engine jumps there without moving the marker
(`LaunchCommand::Locate`), so a later Stop still returns to the marker.

**Launcher over arrangement.** A track whose column launched a clip plays it instead of its
arrangement clips; in the Arrangement view its lane dims, and the ruler shows a
**Back to arrangement** icon at its right end while any lane is dimmed — clicking it runs
`launcher.backToArrangement` ([clip launching](clip-launching.md) › 8).

**Starting inside the arrangement.** From the marker's frame on, the arrangement plays exactly as
it would at that frame of a render from the beginning, with two exceptions: a note that started
before the marker and still sounds there starts at the marker (it is chased), from its attack, as
an instrument cannot begin a sound part-way, and ends where it would have; and a time-stretched audio clip
starts its stretcher at that point, without the audio before it, so it can differ very slightly
for its first moments. Unwarped and re-pitched audio clips that span the marker play from where
they are, sample for sample. The metronome counts from the marker: on a beat it clicks at once,
between beats it waits for the next one. The launcher's beat is the playhead's, so quantised
launches land on the arrangement's bars.

**A jump while playing** ends the notes that sound (with their release) and the audio clips that
play, and plays on from the new beat as a start would: a note that sounds there is chased, whether
the jump moves the marker (a click on the ruler, `transport.setPosition`) or only the playhead
(`Locate`, when the audio output is opened again); a loop's wrap is not a jump and chases nothing. Launches and a scene tempo that were
waiting are queued again for the next boundary from the new beat. Time-stretched clips are
silent until the stretch worker has caught up with the new place (about one block in practice).

**In the Arrangement view** a click on the ruler runs `transport.setPosition`: the beat under the
pointer, on [the grid](#the-grid) (Alt held, or the grid `none`: free, to a thousandth of a beat).
The marker is a small triangle hanging from the ruler's lower edge; the playhead is a line through
the ruler and the lanes, at the marker while stopped (measurements in
[components](../design/components.md) › Arrangement view). The Stop button and the play/stop key
pass the setting **Stop** (Settings › General): **Marker** (default) or **Stay**, saved as
`stop = marker | here`.

**Playing from the selected clip.** A clip's start is often where one wants to listen from;
moving the marker there by hand first is a step too many. **Suhina:** the play/stop key and the
Play button pass the setting **Play from** (Settings › General): **Selected clip** (default) or
**Marker**, saved as `play_start = clip | marker` (the older `play_from` key, which every file got as `marker` while that was the default, is read and ignored). Whatever way clips are picked whole while stopped (a click on a name bar, a rectangle, Ctrl+A, a paste), the play start marker also moves to the first of them. A click in a clip's body picks it but sets the play start where it was clicked, and play starts there, not from the clip's start, until the clip is picked whole again.
A loop the clip starts before still catches playback when it reaches the loop's end. With **Selected clip** and a clip
selected in the Arrangement view, they run `transport.play {from: "clip"}` on that clip; with no
clip selected, or while the Session view is shown (where the selection is not seen), they play
from the marker. **Shift+Space** (`transport.playFromSelection` in the keymap) plays from the
selected clip whatever the setting; while playing, it jumps there.

Playing from a clip **moves the marker** to the clip's start rather than leaving it where it was.
The marker is where the playhead rests while stopped and where Stop returns, so moving it keeps
what is seen and what is heard the same: Stop goes back to the clip's start, the next Play
(with either setting) starts there again, and auditioning one clip over and over is Shift+Space
and Space in turn. Leaving the marker behind would make Stop jump to a place the user did not
start from.

**From the command line** (`suhina-cli play --launcher`): `stop` returns to the marker,
`stop here` stays, `pos <beats>` moves the marker, `play <track> <i>` plays from the start of
the track's arrangement clip `i` (0 first, as `project.clips` lists them). Agents pass `return` themselves; without it,
Stop returns to the marker.

## Markers

A **marker** is a named position on the arrangement's timeline (other DAWs call it a locator or
a cue): a beat from 0 on and a name. Playback jumps to it. A project keeps its markers in beat
order; two can share a beat, and the one added later comes after. A marker is named by its index
in that order, as `project.markers` lists it, so moving one can change its index (the command
returns the new one).

| Command | Does |
|---|---|
| `project.markers` | Lists the markers: `index`, `beat`, `name`. `project.info` counts them. |
| `marker.add {beat, name?}` | Adds a marker at `beat` (0 … 120 000). Without a name it is "Marker n", n the first number from the marker count up that no marker uses. Returns its index. |
| `marker.move {index, beat}` | Moves a marker to `beat`; it goes after any marker already there. Returns its new index. |
| `marker.rename {index, name}` | Renames it; the name is trimmed and may not be empty. |
| `marker.delete {index}` | Deletes it. |
| `marker.jump {index}` | A transport command, not an edit: runs as `transport.setPosition` at the marker's beat. Stopped, the play start marker moves there; playing, playback jumps there at once. Needs the audio engine, as every transport command does. |

The four edits are one undo step each; a bad index, beat or name is an error and changes nothing.
The project file keeps markers in `arrangement.markers` ([format](suhina-format.md)); a Live Set's
locators import as markers ([.als format](als-format.md) › Locators).

**In the Arrangement view** markers sit in a row of their own under the ruler: a line at the
marker's beat with its name in a tag beside it, cut short where the next marker starts. A click on
a marker runs `marker.jump` and, when a marker comes after it, first makes the loop region from
that marker to the next one and turns looping on (`transport.setLoop {start, end, enabled: true}`),
so a section loops with one click; the last marker only jumps. Dragging it moves it (`marker.move` when let go) on the same grid a
ruler click uses (Alt held: free). A right-click on a marker opens its menu: a field with its name
and **Rename** (or Enter), **Jump to marker**, **Delete marker**. A right-click on the ruler or on
an empty part of the marker row offers **Add marker here**, on that grid. The key **M** (the action
`marker.addAtPlayhead`, in the keymap) adds a marker where the playhead is while playing, and at the
play start marker while stopped.

## Loop region

The **loop region** is a span of the arrangement's timeline, a start and an end in beats, that
playback repeats while **looping** is on. The project keeps the region while looping is off, so
turning it on again repeats the same span. It is separate from a clip's own loop
([Start marker and loop](#start-marker-and-loop)), which repeats inside one clip.

**Playing.** While looping is on and the playhead reaches the loop end, playback goes on from the
loop start on the very next frame: the frame after the loop end's last one is the loop start's
first, whatever the audio block size. The wrap is a jump while playing ([Play
position](#play-position)): notes that sound are released, notes that start at the loop start or
later play again, and audio clips play on from where they are at the loop start, sample for
sample. The play start marker does not move, so Stop goes back to it as usual. A playhead that is
already at or past the loop end (the play start marker set there, or a jump there) plays on
without wrapping; one before the loop start plays into the region and then repeats it. A loop
region shorter than one sample frame does not repeat. The region is in beats: its ends are turned
into frames through the [tempo map](tempo-map.md) on every pass, so a tempo change inside the loop
keeps the loop's length in beats.

Offline, the same path loops (`render_launches`, the oracle for playback). A render of the whole
arrangement (`suhina-cli render`, the MCP render tools) plays it once from start to end and does
not repeat the region.

**Commands.**

| Command | Does |
|---|---|
| `transport.setLoop {start, end, enabled?}` | Sets the region: `start` from 0, `end` after it, at most 120 000. Without `enabled` looping stays as it was; `enabled: true` or `false` turns it on or off as well. Returns `{start, end, enabled}`. |
| `transport.setLoopEnabled {enabled?}` | Turns looping on or off; without `enabled` it toggles. Turned on when the project has no region, it first makes one over the first 4 bars (in the project's time signature). |

Both are edits, one undo step each; a bad value is an error and changes nothing. `project.info`
reports `loop` (`{start, end, enabled}`, or null without a region). The project file keeps it
under `transport.loop` ([format](suhina-format.md)); a Live Set's arrangement loop imports
([.als format](als-format.md#arrangement-loop)).

**In the Arrangement view** the loop brace sits in a row of its own between the ruler and the
marker row: a bar from the loop start to the loop end, bright while looping is on and dim while
it is off. While looping is on, a line in the accent colour runs down every lane at the loop start
and at the loop end, so the region reads in the lanes too. Dragging along an empty part of the row draws a new region on the grid a ruler click
uses (Alt held: free) and turns looping on. Dragging within 4 px of an end of the brace moves that
end; dragging its middle moves the whole region, keeping its length. A click on the brace, the key
**Ctrl+L** (`transport.setLoopEnabled`, in the keymap) and a right-click on the row (**Turn loop
on** or **Turn loop off**) turn looping on or off. With a time selection or clips selected, Ctrl+L
loops their span; pressed again while the loop is on over that very span, it turns looping off.

**Limits.** A time-stretched audio clip in the region is silent for a moment after each wrap in
real-time playback, until the stretch worker has caught up with the loop start, as after any jump
while playing; offline it plays without a gap.

## Tracks

| Command | Does |
|---|---|
| `track.group {tracks, name?}` | A new group track takes the place of the first track; the tracks (with what is inside them) follow it, play into it, and it plays where they played. They must all be in the same group, or in none. Grouping tracks already in a group nests the new group inside it. |
| `track.ungroup {track}` | The group goes; its tracks stay where they were and play where the group played. |
| `track.setGroup {track, group, index?}` | Moves a track, a group with everything inside it, into the group track `group` (an id or a name), or out of every group with `"none"`, and its channel plays into that group (the main output for `"none"`). It lands at `index` among the tracks directly in that group (or at the top level), from 0; at the end without `index`, or with one past the end. Refused for a return track, a target that is not a group, a group into itself or a group inside it, and a move that would nest deeper than the cap. |
| `track.move {track, to}` | Moves a track, a group with everything inside it, to position `to` in the track order. |
| `track.rename`, `track.setColor`, `track.delete` | Name (not empty), colour (`#rrggbb` or `none`), and deleting a track with its clips and slots; deleting a group ungroups it. |
| `track.add {kind?, name?, after?, device?, preset?, voice?, sd?}` | A new audio or MIDI track with its channel, slots and lane, after `after` or after the last regular track. `sd` adds a data-only `.sd` effect to a new audio track; `device.addSd {track, path, position?}` inserts one on an existing track ([browser](browser.md) › Commands, [ADR 0021](../decisions/0021-device-graph-files.md)). |

Clips placed from the browser or dropped from the system's file manager (`clip.addFiles`) replace what they cover in the Arrangement like a
moved clip; they stack when either stacking setting is on (one `stack` flag for the whole drop,
not per kind) ([browser](browser.md) › Files).

In the Arrangement view a header click selects a track (Ctrl adds, Shift takes a run), Ctrl+G
groups the selection, dragging a header moves the track above the lane under the pointer (below it
from its lower half), and a right-click opens the track's menu: name, colour, group, ungroup,
**Move into group** (a submenu of every other group it can go into), **Move out of group** (one
level up, right after the group it leaves), **Audio input** or **MIDI input** on a track that
plays them (the source's channels and the monitoring; the commands the strip's block and the
Session view's menu share), delete. A group's header starts with a triangle that
folds its tracks away (Folded groups). The Session view's column headers have the same menu.

A header's left edge is a 6 px band of the track's colour, the header's full height (dimmed while
the track is off); return rows have one too. A click on the band opens the colour palette under
it, the same swatches as the menu's; a swatch sets the colour (`track.setColor`, one undo step).
A press on the band that drags still resizes the header column where the band meets its inner
edge ([Session view](ui-session-view.md) › Track colour).

A **double-click on a header** (on its name or its background, not on one of its controls) also
shows the track's devices in the detail area, opening the detail area if it was hidden; a clip
selected on that track stays selected, with the detail area's switch on **Devices**, and a clip
selected on another track is no longer selected ([clip editor](ui-clip-editor.md) › Where it
opens). It is window state, not a command.

### Groups inside groups

Groups nest: a group track can hold tracks and other groups, at most 16 levels deep
(`MAX_GROUP_DEPTH` in the model, which also refuses a group that is inside itself). A track's
sound goes through its own channel, its group's, that group's group's and so on to the main
output; each group's volume, mute and devices apply on the way ([ADR 0004](../decisions/0004-built-in-devices.md)
› 10), and solo follows the nesting: a soloed track is heard through every group above it, a
soloed group plays everything inside it at any depth.

A header shows its nesting by stepping in 16 px per level; every header keeps the fold triangle's
room, so a group's tracks start clearly to the right of their group, and each level of the indent
is filled with that group's colour, so a group and its tracks read as one block. A group's menu
has **Assign track color to all tracks and clips**, which gives every track inside it, at any depth,
the colour the group shows, and turns the clips of the group and of those tracks back to their
track's colour (`track.colorGroup` with `clips`, one undo step). A looping clip's notches are a
tenth of a pass wide, from a point and a half to four points, so a zoomed-out clip keeps its body. A group that holds tracks has
the fold triangle at the start of its header (Folded groups): the lanes of every track inside it, at any
depth, are hidden while it is folded, and the Session view hides their columns; both views fold
the same groups, and folding is part of the view, not an edit, so it does not undo. A dragged
header joins the group of the lane it is let go over: above that lane, or below it from its lower
half, and below the header of an open group is into that group, first. Within the same group the
drop is a `track.move`; into another group, or out to the top level, it is a `track.setGroup`.
After a folded group is after everything inside it.

While the piano roll is pulled to the top with **Edited track** (Settings › Clip editor › Piano roll),
the view shows only its ruler, loop brace and marker rows and the edited track's lane, whole and
lit (the view is as tall as those rows and the lane, so the lane's notes show as they do
elsewhere), with no return tracks and no main track, and is not scrolled; its scroll position is back when the view is whole again
([clip editor](ui-clip-editor.md) › Pulled to the top).

## Volume, pan, mute and solo

Every track's mixer strip is reached by a `track.*` command that takes `tracks`, a list of one or
more tracks by id or name:

| Command | Does |
|---|---|
| `track.setVolume {tracks, db, continue?}` | Sets each to `db` (at most +6; `"-inf"` is silence). |
| `track.adjustVolume {tracks, db}` | Moves each by the same number of dB (positive is louder), keeping what is between them, within the same range. |
| `track.setPan {tracks, pan, continue?}`, `track.adjustPan {tracks, pan}` | The same two, for pan; −1 is full left, +1 full right, and a move stops at either end. |
| `track.setMute {tracks, mute}` | Turns each off or back on. |
| `track.setSolo {tracks, solo, exclusive?}` | Solos each; `exclusive` also ends every other solo. |

A set or adjust that changes a parameter with an automation lane turns that lane off for each
track it changes, and the answer says `"automation": "off"` ([automation](automation.md)).

**A track's header** in the Arrangement view: a lane tall enough for two rows has, on the first,
the track-on toggle (its number), solo, cue and arm, then the volume bar and the pan field, the
level controls together; on the second, the automation lanes button under the toggle, the name,
and the send knobs at its end, as many as leave the name its room. A shorter lane has one row:
the buttons, the name and the automation button at its end. Taller still, the routing and the
input lines follow below.

In the Arrangement view a header's volume and pan fields and its track-on and solo buttons act on
the whole selection while the track they belong to is part of one of two or more tracks: dragging
one field moves every selected track's by the same amount (the others follow it as it is dragged,
and the commands land when it is let go), one click on a track-on or a solo button turns them all
off or on, and the whole list is one undo step. With one track selected, or with a click outside
the selection, they act on that track alone, and it is heard while it is dragged: each frame sets
the value with `continue`, so the drag is one undo step.

**Alt and the wheel over a field** change its value instead of scrolling the view: one tick of a
wheel is one step (1 dB, or 0.02 of pan), Shift as well makes it ten times finer, and the view
stays where it is. The wheel alone scrolls the view over a field too, so scrolling down the track
headers never moves a fader. The same holds for a clip's gain field, a send's knob, the main
volume and the Session mixer's values; a strip's device list scrolls with the wheel itself.

**Mixer controls** (Settings › Session and mixer) chooses how a track's volume and pan are
drawn: **Field** (default), the bar and the centre bar this view has always had, or **Knob**, a
small knob with its value beside it, as the Session mixer's pan knob. Either way the control is
dragged and rolled over the same.

## Over other clips

The moved clip covers `[start, start + length)` on its lane. Every other clip on that lane:

| Where it is | Replace (default) | Stack |
|---|---|---|
| Outside the moved clip | unchanged | unchanged |
| Entirely under it | removed | unchanged |
| Starting before it, ending inside | shortened to end where the moved clip starts | unchanged |
| Starting inside, ending after | starts where the moved clip ends; its content keeps playing from where it was at that point | unchanged |
| Starting before and ending after | split in two around the moved clip, the second part continuing the content from where it was | unchanged |

A shortened or split clip keeps fades that fit it (see Fades).

"Where the content was" follows the clip's loop: a looping clip continues inside its loop, so the
second part sounds as the original did at that moment.

Replace and stack are chosen per move (`stack: true` stacks). The app passes the user's settings:
**Allow sample stacking** for audio clips and **Allow MIDI stacking** for MIDI clips, both off by
default.

## Showing stacked clips

Where clips on a lane overlap, the lane shows them side by side in sub-rows (each clip in the
first sub-row free at its start), so every clip stays visible and can be grabbed. The rows are
only where clips overlap: the clips that overlap one another, directly or through others, share
the lane in as many rows as they use, and a clip that overlaps none fills the lane (#697). A thin
line between the rows runs along the whole lane, under the clips, so each row reads as one in the
empty stretches too.

## Folded groups

A group track's header starts with a small triangle; clicking it **folds** the group. The lanes of
the tracks inside it go away, and the group's own lane shows what is in them: a thin row for each
track inside it that has clips, every clip a block in its colour where it plays, so the folded lane
tells how much is in the group and where. Clicking the triangle again opens the group. A group
holding no tracks shows no triangle.

A group arrives folded when a Stems file makes one: dropping a `.stem.mp4` on a lane, a slot or
the empty area puts its stems and their muted master in a new group, and that group opens folded,
so the whole file reads as one lane until its triangle is clicked
([stems](stems-format.md) › Mastering).

Folding is view state, not part of the project: nothing in a saved project keeps it, so a project
that is saved and opened shows every group open, and no command folds or opens one. A folded
group's header stays its own — name, colour, volume, pan, mute, solo and output are the group's.
The blocks on a folded lane are a view of the clips, not the clips: to select, move or edit one,
open the group. The tracks inside a folded group cannot be reached with the mouse, since none of
them is drawn.

## Open

- Placing clips by recording follows the same rules (clips from the browser, duplicating and
  pasting already do).
- Moving (rather than copying) a clip between a slot and the arrangement.
- Whether a stacked lane should grow taller automatically.
- Dragging along the ruler to scrub (a drag there selects time now).
- Closing or opening a gap with a time selection (Delete Time, Insert Time).
- A consolidated audio clip warped to the project, so it follows a later tempo change.

## MIDI input monitoring

A note-playing track has one small control immediately after its record-arm button that reads
the track's monitoring mode, Auto, In or Off (the chip shows In as **On**), before its name or volume controls. A click opens the three choices, with a tooltip
explaining the current state; it is not a second record-arm button or headphone cue. The same
choices appear in the MIDI input menu, beside port and channel selection; a header too narrow
for the chip to end before the automation button leaves it out, and that menu is the way (#876). Auto (the default)
hears the input while armed; In hears it independently of arming; Off never hears the live
input. Recording still depends on arm and record, including when monitoring is Off; In alone
never records. Monitoring does not suppress clip playback. The selected mode is project data,
set through `track.setMidiInput` with `monitor: auto | in | off`, and supports undo and redo.
Changing mode releases notes that are no longer monitored, without retriggering held keys.
Old projects without this field retain Auto. Where the chip shows, compact and expanded headers reserve its space before names and level controls; where it is left out, the name starts after the arm button.

The popup is titled Hear your MIDI keyboard and offers three single-line choices: Automatic,
Always on and Off. A fixed-height footer explains the hovered or keyboard-focused choice,
or the current mode when none is hovered. Automatic says when the input is heard, Always on
says recording need not be enabled, and Off says live input is silent but can still be
recorded. Tooltips explain the selected mode and the separate record-ready
button, without requiring knowledge of "armed" or "play-through". The file/API value `in`
remains unchanged.
