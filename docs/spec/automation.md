---
description: Automation envelopes - lanes of points over the arrangement for a track's volume, pan and mute and every built-in, .sd and plug-in parameter, segments that move evenly along the parameter's control, the one-handle curve, the volume fader's law, jumps and their smoothing, how the engine plays them, a manual change turning a lane off, and the automation.* commands
read_when: automation, envelope, curve family, Bezier, volume fader, fader law, volume taper, volume smoothing, Taper, VolumeTaper, Curve, CurveDrag, curve split, curve drag, automation lane, automation lanes view, show automation, automation.movePoints, automation.deletePoints, select points, breakpoint, automation point, curve, automation.addPoint, automation.movePoint, automation.deletePoint, automation.clear, automation.setEnabled, automation.list, device.setAutomation, automation off, re-enable automation, volume automation, pan automation, mute automation, device parameter automation, AutomationLane, AutomationPoint, MixLanes, DeviceLanes, Envelope
type: spec
status: current
last_verified: 2026-11-11
covers:
  - crates/model/src/automation.rs
  - crates/engine/src/automation.rs
  - crates/engine/src/automation_tests.rs
  - crates/engine/src/automation_render_tests.rs
  - crates/engine/src/plugin_params.rs
  - crates/engine/src/chain.rs
  - crates/engine/src/buses.rs
  - crates/session/src/automation.rs
  - crates/session/src/automation_fit.rs
  - crates/session/tests/automation.rs
  - crates/session/tests/clip_automation.rs
  - crates/ui/src/arrangement/automation_lanes.rs
  - crates/ui/tests/automation_lanes.rs
  - crates/ui/src/kit/widgets.rs
  - crates/project-file/src/lib.rs
---

# Automation

A parameter can follow an **envelope** over the arrangement: a lane of points, each a beat and a
value, that the parameter moves along while the transport plays. This is Suhina's own design
(#395); it extends the plug-in lanes of #109 to every parameter. How a segment moves, the shape
of its curve, the volume fader's law and the smoothing of a volume jump follow how Live 12.4.6
plays its lanes, measured from renders of automated parameters (`live-12/automation.md` and
`measurements/automation/` in the device-specs repository, #906). Lanes are set by command (CLI,
MCP) and drawn and edited in the Arrangement view (#420, [below](#in-the-arrangement-view)).

## What can be automated

| Parameter | Lane id | Values |
|---|---|---|
| A track's volume | `volume` (on the track's mixer) | a linear gain in the model, 0 to +6 dB; commands take and show decibels, as `track.setVolume` |
| A track's pan | `pan` | -1 (left) to 1 (right) |
| A track's mute | `mute` | 0 (sounding) or 1 (muted), a step |
| A built-in effect or instrument's parameter | its stable id (`device.list`) | in the parameter's unit, kept in its range as `device.setParam` keeps it |
| A data-only `.sd` device's parameter | its stable id in the `.sd` graph | in its unit and range, of the kind its control shows (a toggle, a choice, a whole number or a continuous value) |
| A plug-in's parameter | its id, a number (`device.pluginParams`) | in the plug-in's own range (#109) |

The main channel's volume, pan and mute have lanes too ([ADR 0004](../decisions/0004-built-in-devices.md)
› Amendment: a chain on the main channel). Send levels have none yet.

## Lanes and points

A lane belongs to what it automates: mixer lanes to the track's channel
(`Channel::automation`), device lanes to the device (`Device::automation`), so a lane moves, copies
and is deleted with its device. One lane per parameter.

- Points are kept in beat order. Before the first point the lane holds the first point's value;
  after the last, the last's.
- Between two points the value moves **evenly along the parameter's control**: its position,
  0 to 1 over the travel of the fader or knob that sets it, not its stored value
  (`automation::Taper`). The volume moves along the volume fader ([below](#the-volume-fader)), a
  built-in or `.sd` device's parameter along its knob (`ParamInfo::to_normalized`: linear,
  logarithmic, or Utility's gain law, [utility.md](utility.md#the-gain-knob); a `.sd` parameter is
  logarithmic on a log slider or knob), pan and plug-in parameters in their value. What
  a lane draws straight is what plays: half-way from 50 Hz to 500 Hz on a logarithmic knob is
  158 Hz.
- **A whole-number parameter** (`ParamKind::Integer`, and a `.sd` parameter on an integer control) moves along the line like any other, and
  takes the lane's value with its **fraction cut off toward zero** (`ParamInfo::played`): 36.98
  plays 36, 0.94 plays 0 and -2.97 plays -2. It is not rounded or floored: a lane from 40 to 41
  plays 40 until its last point, one from 41 to 40 plays 40 from its first step, and one from 3
  to -3 spends twice as long on 0 as on each other number. A point may hold a value between two
  numbers (a split on a slope leaves one; a file can hold one): it sits where it falls between
  them, unrounded (`ParamInfo::to_normalized`), and plays cut toward zero, so 40.5 plays 40.
  This follows how Live 12.4.6 plays integer lanes, read through its API with the playhead
  stepped across them (`measurements/automation/kinds/`, #914); whether a device's sound follows
  the value the same way was not heard. A value typed for a point (`automation.addPoint` with a
  `value`) is kept as `device.setParam` keeps it, a whole number. The lane draws the line the
  lane moves along; the header, the point's tooltip and the panel while it plays show the number
  played.
- A hosted plug-in's parameter (#1205) moves evenly over the plug-in's own range (a VST3 plug-in's
  normalized 0 to 1, a CLAP plug-in's plain values) and, for a stepped one, holds each point's
  value on its nearest step until the next point, as drawn: the engine learns which parameters
  take steps from the plug-in's own list of them, read once it runs, for playing and for an
  offline render alike (the project file does not keep it). The header, the point's tooltip,
  Edit value and the value beside the pointer while dragging show the plug-in's own text for the
  value ("-3.2 dB"), asked from the plug-in in the background (`get_param_texts`, [the plug-in
  protocol](plugin-protocol.md)), at most ten times a second; until it answers, for a plug-in that
  names no values (VST2) and for 30 seconds after a request it did not answer, they show the
  number. While a point is dragged the plug-in follows the line where the transport stands; when
  the drag ends, given up or not, it goes back to what the lane plays there.
- The movement follows the first point's **curve** (`automation::Curve`): a cubic Bezier from
  (0, 0) to (1, 1) in (share of the segment's time, share of the way in position) through two
  inner control points (x1, y1) and (x2, y2). Bending and dragging keep each anywhere from 0 to 1;
  a split can put them outside ([below](#splitting-a-segment)). With both on the diagonal the
  segment is straight (the default). The same curve has the same shape going up and
  down. Bending a straight segment gives the **mirrored family**, one handle `bend` from -1 to 1
  (`Curve::bend`): the second control point is the first's mirror, (1 - y1, 1 - x1), and the
  first runs along y1 = 1 - 3 x1 to (0, 1) at 1 (fast first, then settles) or along
  x1 = 1 - 3 y1 to (1, 0) at -1 (waits, then moves late); the two lines meet at (1/4, 1/4),
  straight. Splitting and dragging give pairs outside the family. This follows how Live 12.4.6
  stores and plays its lanes (#906, #913).
- **Limits.** Each coordinate is kept from -0.5 to 1.5 (`CONTROL_MIN`, `CONTROL_MAX`). With x1
  and x2 from 0 to 1 the time share never falls, so the curve is a function of time, and with y1
  and y2 there the value stays between the two points; a split's half is a piece of such a curve,
  so it keeps both, whatever its points. A curve inside the limits whose time share does fall,
  which only a hand-edited file or command can give, plays at the first moment its time share
  reaches each share of the segment's time, its value held between the two points.
- **Two points at one beat make a jump**: the value reaches the first, then leaves from the second.
  A new point at a beat that has points goes after them.
- On/off and choice parameters, and the mute, are **steps**: each point's value holds until the
  next point; curves do not apply. Whole-number parameters move along the line, as above.
- A lane is **enabled** or not. One that is off does not play: the parameter keeps the value set on
  it (the channel's volume, the device's `params`).

## Splitting a segment

A point placed on a bent segment keeps the sound exactly (`Curve::split`, `automation.addPoint`
without a value): at share x of the segment's time the curve's parameter t is where its time
share is x; the new point's value is the parameter's value at the curve's position there, and
each half gets the de Casteljau subdivision at t, scaled to its own box. The halves are no longer
mirrored. A half that does not move is straight.

The halves are **not clamped** to the unit square, as Live 12.4.6 stores them (split-overshoot in
`live-12/automation.md`): from a curve whose points run back in time or in value they lie partly
outside it. (1, 0, 0, 1) split at 0.75 of its time gives a first half with x1 = 1.1958, at 0.25 a
second half with x2 = -0.1958; (0, 1, 1, 0) gives y1 = 1.2931 at 0.75 and y2 = -0.2931 at 0.25.
Splitting a curve of the square, however often, keeps the first control point's coordinates from
0 to 4/3 and the second's from -1/3 to 1 (a cubic whose time share never falls has no others), so
the limits above leave room for rounding; a coordinate past them, which only a sliver of a cut or a
curve whose time runs back can give, is held at the limit. A point added at another point's beat takes that
point's curve, since it starts the segment.

## Dragging a curve

Dragging a segment's line reshapes its curve (`automation::CurveDrag`), as measured from Live
12.4.6's editor. Each control point has a **path**: from the corner (0, 1) to a point (p, p) on
the diagonal, then on to (1, 0); its place on the path F is 0 at (0, 1), 1 at (p, p) and 2 at
(1, 0). A drag moves F in whole steps of 1/64 and never changes p.

- **A drag first holds the curve in the unit square**: each coordinate of a control point outside
  it (from a split) is moved to 0 or 1 on its own, so x1 = 1.1958 becomes 1 and (0, 1.2931) the
  corner (0, 1). The drag then goes on as below from the held curve. A drag shorter than one step
  still holds it, and changes the sound by that alone (split-overshoot-drag, Live 12.4.6).
- A straight curve, one with both points on the diagonal and one with both in the corners (0, 1)
  or (1, 0) start on the family's paths, p = 1/4 and 3/4; a single point in a corner takes its
  family path (not measured). A single point on the diagonal keeps its own p.
- **Together** (both points on one side of the diagonal, or both on it): each step moves both F
  the same way, so the curve moves earlier or later. **Apart** (opposite sides, or one on it):
  the point with the larger F moves with the drag and the other against it, so an S bends further
  or flattens through a straight line into the reverse S. The mode is chosen when the drag
  starts and kept to its end; the next drag chooses again.
- A point that would pass the end of its path stops there while the other goes on; a step
  neither can take is not taken, so a drag can stop a step short of a corner.
- In the lane, 1.5 points of vertical mouse travel are one step, and the line follows the
  mouse: up bends it up, which moves a rising segment's curve earlier and a falling one's later.
  How far the mouse moves per step was not measured; this is Suhina's own choice.

## The volume fader

The volume lane, the mixer's volume field and fader, the Arrangement's volume field and the
volume knob all place a volume on one law (`automation::volume_db`, `volume_position`), measured
from Live 12.4.6's track volume (and the same on Operator's Volume):

- **From 0.4 of the travel to the top**, dB = 40 p - 34: even in decibels, 40 dB per travel,
  0 dB at 0.85, +6 dB at the top, -18 dB at 0.4.
- **Below 0.4**, dB = 40 p - 34 - 207 (0.4 - p)^2: it falls faster, -34 dB at 0.2, -49 dB at
  0.1, -67 dB just above the bottom (within 0.25 dB of the measured fader above 0.05).
- **The bottom** is silence; a volume below the law's lowest level sits there.

The mixer's volume field still steps by decibels when dragged; the law is where it draws the
level and where a lane moves.

## A manual change turns the lane off

Setting a parameter that follows an enabled lane by hand (`track.setVolume`, `track.setPan`,
`track.setMute`, `device.setParam`) turns its lane off in the same undo step, whether or not the
transport runs, and the answer says `"automation": "off"`. The value set by hand holds until
`automation.setEnabled {enabled: true}` turns the lane on again; the lane's points are kept. Undo
brings the lane back on with the old value.

A move in a hosted plug-in's own editor turns its lane off the same way, as one undo step, and
the engine stops playing the lane at once (#1205). A value the plug-in reports that the lane sent
it does not count: stopped, the lane's value at the playhead; playing, any value the lane went
through in the last two beats, as a report may come a little after the value it echoes.

## What the controls show

A control whose parameter follows an enabled lane (a track header's volume and pan, a mixer
strip's, the returns' and Main's, a device's knob) shows the lane's value at the playhead,
whether the transport runs, is paused or is stopped, and follows the playhead when it is moved
while stopped (#706, #811). A lane turned off shows the value set by hand.

While a point of a hosted plug-in's enabled lane is dragged, the plug-in itself takes the dragged
line's value where the transport stands, from the next audio block, so its own editor's knob moves
with the point (#1205); the project and the lane change only when the point is let go of.

While an enabled lane's points or curves are dragged in the Arrangement view, the track header's
volume or pan, Main's volume and a device's knob in the detail view show the value of the line as
it is dragged at the playhead, so a control moves while the drag changes the line where the
playhead is, not only once the point is let go of (#811). A drag that changes the line elsewhere
leaves the control as it is. While a time range is painted on a lane, the same controls show the
line's value at the range's moving edge, under the pointer, and once it is let go of the value at
its start, where the play start marker moves. The mixer strips of the Session view are not drawn beside the lanes
and follow once the change is in.

## How the engine plays them

The engine runs one render path (offline renders and playback alike), turns lanes into
arrangement frames when the graph is built (at the project's tempo), and allocates nothing on the
audio thread.

- **Device parameters** (built-in and `.sd`): while the transport runs, the parameter is set at every point
  that falls inside a block and at every 64th frame of the arrangement (1.3 ms at 48 kHz) where its
  value changed, and at a block's first frame when the transport has just started or moved. The
  device's chain splits its block there, so a jump lands on its frame and the device smooths the
  change as it smooths any parameter (ADR 0004, decision 5); a MIDI effect's span of events
  splits there too, the change coming before the events at its frame (ADR 0004 › 14), and its
  semitone, scale-degree and key values show as `+5 st`, `+2 sd` and `C3`. A `.sd` device is a
  `Device` like a built-in (#763 › 6) and takes the same changes: its toggle, button, choice and
  radio controls step, and an integer control plays as a whole-number parameter (above). A fresh
  device starts at the lane's value at beat 0. While stopped, a parameter keeps its last value.
- **Plug-in parameters**: as #109 built them (ADR 0006 › 3): a value at the block's start, at
  each point and every 64 frames while it moves, as parameter events at their frames; the curve
  and the enabled flag apply too. While stopped, a lane sends its value where the transport
  stands when the play start marker moves or the lane changes, so the plug-in's editor and its
  parameter list show what plays there (#1205). A plug-in that starts (the project opened, a
  restart) keeps the state it was given until then, so opening a project changes nothing in it
  and leaves no unsaved change.
- **Mixer lanes**: the track's gains are computed at every 16th frame of the arrangement and move
  in a straight line between, frame by frame, so a frame's gain depends on its time only (not on the
  block size) and a fader move or mute never clicks: a mute switches over 16 frames. A **jump of
  the volume** (two points at one beat) becomes an S-shaped ramp (smootherstep) along the fader,
  4.4 ms long and centred on the jump (`VOLUME_SMOOTHING`): it starts 2.2 ms early, which the
  engine can do because it reads the lane ahead. A straight or bent segment plays as drawn. This
  is what Live 12.4.6's track volume does with a jump (a fixed 4.4 ms at any sample rate, an S in
  the fader's position, centred); pan and mute jumps are not smoothed further. A track whose
  mixer has a lane that plays runs through a device chain (one without devices when it has none),
  and its gains apply after its devices, as for any track with devices. A group track with mixer
  lanes becomes a bus (ADR 0004 › 10), so its lanes scale the sum of the tracks routed into it.
  The lanes are read earlier by the chain's latency compensation, so they line up with what is
  heard. While stopped, the gains are the lanes' values at the play start marker.
- **Mute and solo**: a mute lane decides the channel's mute while it plays (a muted channel with a
  mute lane sounds where the lane says 0). Solo is not automated.

A corner of a lane that falls between two steps (64 frames for device and plug-in parameters,
16 for the mixer) is cut across that step (at most 1.3 ms early or late in shape, not in time: a
jump's own frame is exact for device and plug-in parameters). A built-in device smooths a jump
itself (Utility's gain glides along its knob, [utility.md](utility.md#smoothing)).

## Commands

Every edit is one undo step; a parameter is named by `track`, `device` (left out for the track's
mixer) and `param`. Values are in the unit of the parameter's set command.

| Command | What it does |
|---|---|
| `automation.addPoint {track, device?, param, beat, value?, curve?, shape?}` | adds a point, making the lane (enabled) if there is none; answers the point's index in beat order. Without `value` the point goes on the envelope and a bent segment is split so it plays as before ([above](#splitting-a-segment)); a lane without points needs a value. With `shape` (a name `automation.insertShape` takes), the segment from the point before to the new one takes that shape between their heights, a repeating one once a beat |
| `automation.movePoint {track, device?, param, point, beat?, value?, curve?}` | changes a point; answers its new index |
| `automation.deletePoint {track, device?, param, point}` | removes a point; the lane goes with its last point |
| `automation.movePoints {track, device?, param, points, beats?, value?, curve?}` | moves several points of one lane together in one undo step: `beats` later (earlier below 0), `value` up or down in the set command's unit (dB for the volume), each value kept in the parameter's range (a silent volume point moves from the lane's bottom, -70 dB, so points at the bottom move with the rest; a level moved down to -70 dB or below is silent, a louder one stops at +6 dB), `curve` given to each; answers the points' new indices in the order given |
| `automation.deletePoints {track, device?, param, points}` | removes several points in one undo step; the lane goes with its last point |
| `automation.clear {track, device?, param?}` | removes a lane, or every lane of the mixer or the device |
| `automation.setEnabled {track, device?, param, enabled}` | turns a lane off or on again |
| `automation.copy {track, device?, param, start, end}` | copies the envelope of a time range, with its values at the range's edges, for `automation.paste` (or, with `points` in place of the range, those points as they are, from the first of them: pasted, they go in among the lane's points and replace only those between the first and the last); each value is kept as where it sits on its lane (0 at the bottom, 1 at the top), as the lanes place it |
| `automation.paste {track, device?, param, start}` | pastes the copied range from `start` over what is there, making the lane if there is none; on another parameter each value goes to the same height of that lane (the bottom of a pan lane is full left, of a mute lane off); answers the range pasted |
| `automation.duplicate {track, device?, param, start, end}` | copies a range right after itself, over what is there; answers the copy's range, so the next duplicate goes on from it; the clipboard stays as it was |
| `automation.deleteRange {track, device?, param, start, end}` | removes the points inside a range |
| `automation.simplify {track, device?, param, start, end, tolerance?, gesture?}` | replaces the points inside a range with the fewest bent segments that stay within `tolerance` of the old line, curves included, everywhere (a share of the lane's height, 0 to 1, default 0.02): the old line is sampled (24 places a segment), cut at its jumps and at corners sharper than 45° (on a lane taken as four times wider than tall) that the line needs: a corner that one fit across follows within the tolerance, with no more segments than its two sides apart, is smoothed away, and each piece gets one cubic curve fitted by least squares along its end directions (each handle at most 0.45 of the segment's chord, so a bend spreads along it), split where it strays furthest until every part is close enough (Schneider's curve fitting); a part whose curve the lane cannot hold (one running back in time, or bending a segment with level ends) is split further. The range's first and last points stay, and the result never has more points than before (else the range is left). Answers how many points it removed |
| `automation.scaleRange {track, device?, param, start, end, factor, around?, gesture?}` | stretches the points inside a range up and down by `factor` (0 to 4) around `around`: `middle` (the default) is the middle of where they sit on the lane, kept inside the travel. With `points` in place of the range, only those points move, around the middle of the line over their span (the troughs of a wave picked alone deepen, its peaks stay). Calls with one `gesture` are one undo step; `bottom` is the bottom of the lane, so 0.5 halves each place on it. With `continue` and `replace` each call starts from the line as it was before the step it continues, so scaling up then back to 1 puts back points the lane's edge had cut |
| `automation.smooth {track, device?, param, start, end, amount, gesture?}` | rounds the corners and jumps inside a range, `amount` (0 to 1) taking up to 0.45 of the room on each side: a jump (two points at one beat) becomes an eased step that leaves its first value slowly, passes the middle at the jump's beat and settles on its second; a corner between two straight segments whose slope changes gets a point on each side on the old line, the line bending into the corner and out of it. Bent segments, stepped lanes and the range's edges are left as they are; answers how many it rounded |
| `gesture` on `automation.insertShape`, `simplify`, `smooth` and `scaleRange` | a call that joins the open gesture runs on the project as it was before the gesture began, so each call of a slider's drag replaces the one before rather than adding to it; with no gesture open (another edit came between, or the calls before changed nothing), it runs on the line as it is ([ADR 0002](../decisions/0002-commands.md) › 9) |
| `automation.insertShape {track, device?, param, start, end, shape, cycles?, depth?, center?, gesture?}` | fills a range with a shape over the parameter's whole travel: `sine`, `triangle`, `saw_up`, `saw_down` and `square` repeat `cycles` times (1 to 512, default 1); `ramp_up`, `ramp_down`, `curve_up`, `curve_down` and `swell` play once. A sine is four bent segments a cycle (its curves pass through a sine's value half-way along each quarter), so it needs few points. With `depth` (0 to 1, default 1) the shape swings over that much of the travel, around `center` (default where the lane's value is at the range's start), kept inside the travel. With `point` in place of `start` and `end`: the segment from the point before it to it (the first point's: to the next), between the two points' heights (the whole travel when they are level), a repeating shape once a beat unless `cycles` says |
| `automation.list {track}` | the track's lanes: `mixer` and `devices` (index, name, lanes), each with `param`, `enabled` and `points` (`beat`, `value`, `curve` as `[x1, y1, x2, y2]`) |
| `device.setAutomation {track, device, param, points}` | replaces a device lane with straight segments (`points` a flat list of beat and value; `[]` removes it), for built-in devices, `.sd` devices and plug-ins |

A `curve` is `[x1, y1, x2, y2]`, each from -0.5 to 1.5 (from 0 to 1 but for a split's halves,
which `automation.list` shows as they are), or one bend from -1 to 1 for the mirrored family.

An edit of a range replaces the points inside it and keeps the envelope outside it: points at the
range's edges hold the values the lane had there (left out where the new points already start or
end with them).

Refused, changing nothing: an unknown track, device or parameter; a range whose end is not after
its start; a copy or duplicate of a lane with no points, a paste with nothing copied, an unknown
shape; a beat below 0 or past the
arrangement's limit; a value out of range for the mixer (a volume above +6 dB, a pan outside -1 to
1, a mute other than 0 or 1); a curve that is not four numbers from -0.5 to 1.5 or a bend from -1 to 1; a point index the lane does not have; a lane
of more than 65,536 points; a `movePoint` with nothing to change.

## Automation under clips

The automation of a track under an arrangement clip goes with the clip (#916), as it sits
relative to the clip:

- **Moved** (`clip.move`, `clip.moveMany`): every lane of the track's mixer and devices with a
  point from the clip's start up to its end takes its envelope there along: from the clip's start
  to its end, with its values at the two edges. Where the clip was, the lane is a straight line
  between the values it had at those edges; where it lands, the envelope replaces what was there,
  the values outside kept (points at the edges hold them, so a different value outside is a jump
  at the clip's edge). A copy made by the move (`copy`) leaves the envelope where it was too.
- **Copied** (`clip.copy`, `clip.cut`, `range.copy`): each clip's envelope goes on the clipboard
  with it, and `clip.paste` lays it under the clip's copy, on the track pasted to.
- **Duplicated** (`clip.duplicate`, `range.duplicate`): each copy gets the envelope under the clip
  it copies.
- **A time selection** (`range.copy`, `range.cut`, `range.duplicate`, `range.delete`) carries its
  tracks' automation over its whole span, under clips and where no clip plays alike (#1205): every
  lane of the tracks' mixers and devices with a point inside it, from the span's start to its end
  with the values at its edges. A copy pastes back on its own tracks with `clip.paste` and no track,
  the span's start where the paste starts; a duplicate lays it right after the span; a delete clears
  it and keeps the values at its edges. A selection with automation and no clip is no error.

In the Arrangement view a time selection painted over tracks lights their take and automation
lanes under them too, and a selection box begun on a track over no clip picks the automation points
it covers on the lanes under the track (on the lane where it covers the most; a selection of points
is on one lane).

A lane with no point under the clip is a straight line there and stays as it is. On another
track the mixer's lanes (volume, pan, mute) go along; a device's lanes only on the track the
clip came from, whose devices they name. All of it is part of the clip's command, one undo step.

## In the Arrangement view

Each track header has an envelope button at the end of its first row ("Show automation of
Drums"); a click shows the track's automation lanes under it, and another hides them. Whether a
track's lanes are open is the view's own state. Return tracks have no lanes yet; Main's are under
its row ([arrangement editing](arrangement-editing.md), #687).

The lanes are the **top lane** and the **mounted lanes** under it (#1322):

- The top lane is always there while the lanes are open. It shows the parameter being worked
  on: one chosen in its dropdown, or one moved by hand (below). Before either, it shows the
  parameter Settings › Arrangement › Automation › **Top lane shows** names: **Volume** (the
  default), **Pan** or **None**, unless that parameter is mounted. Without a parameter it is an
  empty row reading "Choose a parameter, or move a control, to show it here", with a dropdown to
  choose one. The pin outline left of its dropdown ("Pin this parameter as a lane of its own below") mounts its
  parameter: the parameter gets a lane of its own right under the top lane, and the top lane is
  left empty; it does not go on to another parameter. Its **×** at the header's right end ("Clear the top lane") clears it,
  leaving the empty row: the top lane never goes. Which parameter it shows is the view's own
  state.
- A mounted lane keeps its parameter: its header shows the name, with no dropdown, and a change
  to that parameter is made in its own lane. A filled pin in the theme's
  `automation.pin` colour (the built-in themes give it their accent) stands left of the name; a click on it ("Unpin this lane", or "Unpin these N lanes" when it is
  one of several picked) unmounts it, with the other picked lanes (the lanes' points stay). The
  buttons have no box: their marks, drawn with lines so they are the same in every font, are dim
  until the pointer is on them.
- Which lanes are mounted, in which order, and their groups are part of the project
  (`automation.setLayout`, the project file's `lane_layouts`): each change below is one undo
  step and is saved. A track never laid out mounts the lanes that have points when its lanes are
  opened, until its layout is first changed.

A lane is 40 px tall across the timeline until it is given another height, with a header under
the track's. Dragging the line under a lane makes it taller or shorter (24 to 240 px) and moves
the lanes below; the envelope is drawn to the new height. Dragged past the bottom of the view,
the view scrolls with the line, so the whole lane stays in sight and the line stays under the
pointer to be dragged back up. A double-click on that line, or
**Reset height** in the menu a right-click on the lane's header opens, puts it back to 40 px.
Each lane keeps its height while the track's lanes are hidden, for as long as the project is
open (#818), and when it comes to show another parameter, chosen in its dropdown or brought in
by a control moved by hand (#1205).

Mounted lanes are put in an order of one's own and gathered in groups (#1322):

- A click on a mounted lane header's free space picks the lane (tinted in the accent colour);
  Ctrl+click (Cmd on macOS) adds or takes one away, Shift+click picks every mounted lane from the
  one picked last. A click on a group heading picks the group; Ctrl+click adds or takes one away.
  A click on a lane or a clip lets go of what is picked. Picking is the view's own state.
- Dragging a mounted header's free space moves the lane, or all picked lanes when it is one of
  them, in their order; a line across the lanes shows where they land, before the row under the
  pointer's middle or after the last, under the top lane at least. A lane dropped before one of a
  group's lanes joins the group, and the line then starts further in, under the group's name; one
  dropped on an open group's heading lands above the heading, outside the group (the group's own
  first lane too, which so leaves the group); one dropped anywhere else leaves its group. Over a
  folded group's heading the heading lights up, and the lanes dropped there go into the group, at
  its end. Dragging the top lane's header mounts its parameter where it lands.
- **Group lane** or **Group N lanes** in a mounted header's right-click menu gathers the picked
  lanes (or the lane alone) under a heading in the place of the first of them, named "Group 1"
  (the first number free on the track). The heading shows the group's name and how many lanes it
  holds; its arrow folds the lanes away to the heading's row and back; a double-click or
  **Rename** types a new name (Enter keeps it, Escape leaves the old one); dragging it moves the
  whole group. **Ungroup** (or **Ungroup N groups** with several picked) puts the lanes back where
  they are; **Delete group** (**Delete N groups**) unmounts the groups' lanes.
- Delete with lanes or group headings picked unmounts them; Ctrl+Z brings them back.
- With Settings › Arrangement › Automation › **Group plug-in lanes** on (the default), a lane
  mounted from a hosted plug-in goes into a group of that plug-in's lanes, named after the
  device: made the first time, used again for its next lanes; another device of the same name
  gets its own group with a number after the name.
- A time range painted on a lane and dragged up or down over other lanes of the track is on all of
  them: copy, cut, paste, duplicate and delete act on each in one step (`automation.copy`,
  `automation.duplicate` and `automation.deleteRange` with `lanes`; a paste of lanes copied together
  puts each back on its own lane of the track). A lane with no automation in the range is left out.
  It is drawn as one area over those lanes, with no gap where one lane meets the next. It is the
  one selection: painting it lets go of the time selection and the clips picked on the tracks, and
  a time selection or a clip picked after it lets go of it. Dragged out of the track's lanes, up
  onto its clips or onto another track, it goes on as a time selection over every track it
  crosses, lit over their clips and lanes as one area; a time selection painted on the clips takes
  in a track whose take or automation lanes it crosses, too.
  Ctrl+L, or **Loop selection** in the range's right-click menu, loops the range's span, as it
  does a time selection on the clips' lanes; Ctrl+L again over the same span turns looping off.

A control moved by hand brings its parameter into view when its track's lanes are shown
(#1205): the track header's volume, pan or mute (Main's too), a knob of a device on the track's
own chain in the detail view, or a parameter turned in a hosted plug-in's own editor. The top
lane shows it, ready to be drawn on, unless it is mounted: then it is edited in its own lane and
the top lane stays as it is. Either way it is the lane clicked last (Ctrl+V pastes onto it). A
plug-in editor's moves come in as they happen, also while Suhina is asking that plug-in something
(listing its parameters, a value's text), so the lane follows the knob without pauses. A value set on several tracks at once,
or a value a lane sent a plug-in and the plug-in reported back, brings nothing forward, and
closed lanes stay closed. The header:

- **The parameter**, on the top lane a dropdown of the track's volume, pan and mute and each
  built-in, `.sd` and plug-in device's parameters, grouped by device; a parameter with points is
  marked " •". Choosing one shows it in the top lane. A mounted lane shows its parameter's name
  instead, which does not change.
  The parameters with points come first, under a heading "Automated" that does not fold: a
  shortcut to them while their devices are closed, each named with its device ("Utility: Gain")
  and in the order of the list below, where it is listed as well. A search narrows it like the
  rest of the list, and the heading is left out when none of the track's parameters (or none
  that match) has points. A parameter chosen there opens the menu there again, scrolled to its
  copy under "Automated" (while it has points); one chosen in its device opens it on its own
  row. Its device and group are open either way.
  The track's volume, pan and mute come next, without a heading. Each device follows under a
  heading with an arrow, the device's name and how many parameters it has (" •" when one of
  them has points). Inside a device, parameters that name a group (the FM synth's operators,
  its LFO; an `.sd` parameter's `ui.group`) are gathered under a heading of the same kind with
  the group's name; a device's parameters without a group come first, then each group in the
  order its first parameter comes. A row under a heading leaves out the group's name at the
  start ("Rate 1" under "Op 1"). Devices and groups start closed, except the device and group
  holding the lane's own parameter, and a track's only device; a click on a heading opens or
  closes it, and a device closed and opened again keeps its groups as they were. The menu grows
  as headings open, up to the limits below, without moving.
  The menu opens under the dropdown, or over it when there is more room above, and stays on that
  side while it is open. It has a search field in focus and a list that scrolls when it is longer
  than 60 % of the window or than the room on the menu's side (the FM synth has over a hundred
  parameters), scrolled to the lane's own
  parameter (where it was chosen, above). Typing narrows the list to the parameters whose device, group and name contain
  every word typed between them ("op 3 coarse", or "lfo" for a whole group), under their
  device's and their group's headings, open; Enter takes the first of them as listed, an automated one first. A parameter another lane
  of the track shows is greyed out. Clicks in the menu (the search field, a heading, the heading of the lane's own parameter included, which the lane keeps showing) leave it open; choosing a parameter or a click outside closes it, and closing it clears the search.
  A hosted plug-in (CLAP, VST3, VST2) on the track is a device of this list like any other
  (#1205): its name is the heading, the groups it gives its parameters (a CLAP module path) are
  the groups under it, and the same search and Automated section take them in. Only the
  parameters the plug-in marks automatable, and does not hide or keep read-only, are listed, as it
  last listed them: the session asks a running plug-in on a track whose lanes are open again every
  couple of seconds in the background, so a long list (thousands of parameters) opens without a
  wait. A lane whose parameter the plug-in does not list (it is not running or not installed, or
  no longer has it) stays with its points as "Parameter 7 (not available)".
- **On/Off** (lanes with points): whether the lane plays. A manual change to the parameter turns
  it off (the envelope is drawn in `text.hint` and the header says "Off: set by hand"); a click
  turns it on again (`automation.setEnabled`), or off.
- The top lane's pin outline pins its parameter as a lane of its own and its **×** clears it; a
  mounted lane's filled pin unpins it (above).

The envelope is drawn in the track's colour through every point, holding before the first and
after the last, along each segment's curve; steps for mute, on/off and choices. Values are placed
as the parameter's control places them, and a segment is drawn evenly in that placement, as it
plays: the volume on the fader's law ([above](#the-volume-fader), 0 dB at 85 % of the height), a
device's parameter as its knob turns. A lane without points draws a faint line at
the value set on the mixer or the device. Editing (every edit is one undo step):

| Gesture | Command |
|---|---|
| Point at the line (within 8 % of the lane's height above or below it, at least 6 points), or a lane's faint line | the pointer becomes a hand; a dot on the envelope marks where a click puts the point (on the grid), and the envelope's value there shows beside the pointer |
| Click away from the line | no point is added: the play start marker moves there (`transport.setStart`, on the grid; Alt: free), as a click on a clip lane does, and the painted range and the selection of points end. With Settings › Arrangement › Automation › **Click anywhere adds** on (off by default), it is `automation.addPoint` at the beat under the pointer (on the nearest line the arrangement's grid draws at this zoom, [arrangement editing](arrangement-editing.md#the-grid); Alt, Option on macOS: free) and the value at its height ([the lane's ends](#the-lanes-ends)) |
| Double-click away from the line (the setting off) | `automation.addPoint` at the pointer, as above |
| Click on or near the line (within 6 points), or a lane's faint line | `automation.addPoint` without a value: on the envelope, a bent segment split exactly so it plays and draws as before ([Splitting a segment](#splitting-a-segment)); on the faint line, with the parameter's value |
| Drag a point | `automation.movePoint`, its beat on the grid (Alt, Option on macOS: free) and its value following the pointer, the lane's top and bottom taking the parameter's ends ([the lane's ends](#the-lanes-ends)); with Shift only along the axis the pointer moved further on, and up or down at a quarter of the speed for a finer value, without the ends. While the button is down the point's value and beat show beside the pointer ("50R at beat 25.25"), as its tooltip shows them afterwards; the parameter's own control follows the line as it is dragged at the playhead ([what the controls show](#what-the-controls-show)). Points dragged past others keep beat order, and a point dragged onto another's beat stays on the side it came from, so the jump runs as drawn while dragging |
| Right-click the line: **Add value** | `automation.addPoint` on the envelope at that beat, on the grid, as a click on the line does |
| Right-click a point: **Edit value** | a field beside the point with its value selected (in the set command's unit: dB or `-inf` for the volume; a pan as -1 to 1 or `25L`, `C`, `25R`; `%` and `ms` read for such parameters); Enter or a click elsewhere runs `automation.movePoint` with it, Escape leaves it |
| The lanes' menus: **Snap to grid** and **Grid** | Snap to grid off (on by default; the window's choice, not saved) places points and ranges where the pointer is, as Alt does; Grid sets the Arrangement's grid ([arrangement editing](arrangement-editing.md#the-grid)) |
| Click a point | selects it; Shift or Ctrl adds it to the selection or takes it out. Selecting a clip ends a selection of points |
| Press on the line and drag | the press runs `automation.addPoint` at once where the guide showed (on the grid, Alt: free), and the drag moves that point with the pointer (`automation.movePoint` when the button comes up), the envelope following it |
| Drag on the lane's upper half away from the line (the pointer is the text cursor there) | paints a time range over the lane's whole height, its ends on the grid (Alt, Option on macOS: free), and selects the points in it. While it is painted, the parameter's controls show the line's value at the range's moving edge, under the pointer. When it is let go of, the play start marker moves to the range's start (`transport.setStart`), as a time selection on the clips' lanes moves it, so the controls show the value there and Play starts there. A click on the lane or a point, a clip or a track header ends it |
| Drag on the lane's lower half away from the line | draws a box that selects the points in it, as the clips' lanes select with a box from below; no range |
| Ctrl+C, Ctrl+X, Ctrl+D, Delete with a range painted | `automation.copy`; `automation.copy` and `automation.deleteRange`; `automation.duplicate`, the range moving onto the copy so the next Ctrl+D goes on (Ctrl+Z takes the copy back and the range with it, one copy at a time, and Ctrl+Y takes it on again, while the range is still where the copy left it); `automation.deleteRange`. They act on the range before anything else that is selected |
| Ctrl+C, Ctrl+X with points selected and no range | `automation.copy` with `points`; Ctrl+X also `automation.deletePoints` |
| Ctrl+V with no range, on the lane clicked last | `automation.paste` from the play start marker; points copied as points go in among the lane's from there |
| Ctrl+V with a range painted | `automation.paste` from the range's start, on that lane: paint a range where the copy should go, then paste |
| Right-click inside a painted range: **Insert shape** | how many bars the range holds, then ten small pictures of shapes; a click runs `automation.insertShape` on the range. Under them, **Depth** (0 to 100 %, the shape's `depth`: how far it swings around its middle) and **Rate** (how long one cycle of a repeating shape lasts: 4 bars, 2 bars, 1 bar, 1/2, 1/4, 1/8, 1/16 or 1/32 of a whole note; 1/4 by default, a beat in 4/4) apply to the next shape, and a change of either puts the shape put in that range last in again with them, around the same place on the lane, while the slider is dragged (at most 60 times a second) and when it is let go of (when nothing else changed since, it replaces the shape before (one `gesture`), so the range's edges come from the line as it was, and the shape and its changes are one undo step). A Scale, Smooth or Simplify after it ends that: with no shape of the menu's in the range, a line under the two says they apply to the next shape inserted there. Under **Points in the range**, for whatever points the range holds, drawn or inserted: **Scale** (0 to 200 %) runs `automation.scaleRange` on the range while it is dragged and when it is let go of, each change replacing the one before (one `gesture`), so the percentage is always of the line as it was when the range was picked and going back to 100 % puts it back whole, cut peaks included; it stays where it was let go of while the range stays the same. **Around** under it chooses what it scales around: *Middle* (the default), half-way between the lowest and highest point, worked out from the line before scaling, or *Bottom*, the bottom of the lane, where 50 % halves every value. While the slider is hovered or dragged a dashed line on the lane marks it. **Smooth** (0 to 100 %) runs `automation.smooth` on the range the same way, each change replacing the one before (one `gesture`), so each amount rounds the line as it was; a line it does not change, a sine's, stays as it is. **Simplify** (0 to 100 %) runs `automation.simplify` on the range the same way, each change replacing the one before; its `tolerance` is 0.04 times the square of the amount, so 0 % changes nothing and 100 % lets the new line stray 4 % of the lane's height. A line under it counts the points in the range before and now (`206 → 67 points`), and while the menu is open the line from before Simplify is drawn faintly under the new one. Each of Scale, Smooth and Simplify starts again from 100 % or 0 % once another of them, or a shape, has changed the range. The lane's menus close on a command or a click outside them, not on Middle, Bottom or a slider. The sliders' values are the window's, not saved |
| Right-click the lane elsewhere: **Shape for new points** | the same pictures; the one clicked is lit and draws nothing: every point added from then on gives the segment from the point before it that shape (`automation.addPoint` with `shape`). A click on the lit one, or **Straight lines for new points**, goes back to straight lines. It is the window's choice, not saved |
| Right-click a point: **Shape to this point** | under Delete point, the same pictures: `automation.insertShape` with `point`, the segment from the point before to this one (the first point's: to the next) |
| Drag the round handle half-way along a sloping segment (one at least 28 points wide) | bends it as an Alt-drag of the line does; a double-click on it straightens it (curve 0) |
| Drag a selected point | `automation.movePoints`: the whole selection moves by the same beats (none before beat 0) and the same change of value, and stays selected where it went, so Delete or a second move acts on it |
| Delete (or Backspace) | `automation.deletePoints` for the selection, before any selected clip |
| Right-click the lane or a point with points of the lane selected | the menu also says how many are selected and has **Scale** (5 to 200 %): it runs `automation.scaleRange` with them while it is dragged and when it is let go of, one drag one undo step; it stays where it was let go of while the same points are selected, each change measured from where the scaling began |
| Left, Right, Up, Down with points selected | `automation.movePoints`: a grid step earlier or later (the Arrangement's grid; a beat with none), or a step of value up or down (a decibel for the volume, 0.02 of a pan, one for a whole number or a choice, a hundredth of the range otherwise) |
| Double-click a point, or its menu's Delete point | `automation.deletePoint` (the menu deletes the whole selection when the point is part of one) |
| Shift-drag the line between two points up or down | `automation.movePoints` of the two points by the same change of value, their beats kept (finer than the pointer) |
| Alt-drag the line between two points up or down (Option on macOS) | `automation.movePoint` with the first point's new curve, along the paths of [Dragging a curve](#dragging-a-curve): up bends the line up |

### The lane's ends

A point placed or dragged within 2 points of the top or bottom of the lane's plot (the lane less
its 4-point inset above and below) takes the parameter's maximum or minimum exactly: 50R or 50L
for a pan, +6 dB for the volume, a device parameter's end of range (#811). In a lane of the default
height one point is about 3 % of the range, so a point a pixel under the top would otherwise play
47R and be drawn on the same line as 50R. It applies to a click that adds a point away from the
line and to a point dragged without Shift, the dragged point deciding for the points moved with
it; the pointer may go past the lane's edge, so a point taken hold of off its centre gets there
too. The value shown while dragging is the one the command sets (a pan rounded to a thousandth). A Shift drag and Edit value reach the values next to the ends.

## In the project file

`automation` on a channel and on a device: `[{ param, points: [{ beat, value, curve? }],
enabled? }]`. Values are stored in the parameter's unit (a linear gain for the volume). `curve` is
`[x1, y1, x2, y2]` from version 4, written only when the segment is not straight; files before
version 4 wrote one bend from -1 to 1, which reads as the mirrored curve it played (#906), so they
sound as before. `enabled` is written only when it is false
([suhina-format.md](suhina-format.md)). A coordinate may lie outside 0 to 1, as a split leaves
it ([Splitting a segment](#splitting-a-segment)); one outside -0.5 to 1.5 or not a number (or a
bend outside -1 to 1) is refused, as is a lane of more than 65,536 points; points are read in beat
order.

## Not done yet

- Recording automation and drawing with a pencil.
- A VST3 plug-in's units are not read yet, so its parameters have no groups in the list; a VST2
  plug-in names only its current value, so its lanes show numbers.
- Send levels. The tempo changes over the song through the [tempo map](tempo-map.md), not a lane.
- Importing automation from other formats.
