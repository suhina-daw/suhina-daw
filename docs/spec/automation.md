---
description: Automation envelopes - lanes of points over the arrangement for a track's volume, pan and mute and every built-in, .sd and plug-in parameter, curves and jumps, how the engine plays them, a manual change turning a lane off, and the automation.* commands
read_when: automation, envelope, automation lane, automation lanes view, show automation, automation.movePoints, automation.deletePoints, select points, breakpoint, automation point, curve, automation.addPoint, automation.movePoint, automation.deletePoint, automation.clear, automation.setEnabled, automation.list, device.setAutomation, automation off, re-enable automation, volume automation, pan automation, mute automation, device parameter automation, AutomationLane, AutomationPoint, MixLanes, DeviceLanes, Envelope
type: spec
status: current
last_verified: 2026-10-17
covers:
  - crates/model/src/automation.rs
  - crates/engine/src/automation.rs
  - crates/engine/src/automation_tests.rs
  - crates/engine/src/automation_render_tests.rs
  - crates/engine/src/plugin_params.rs
  - crates/engine/src/chain.rs
  - crates/engine/src/buses.rs
  - crates/session/src/automation.rs
  - crates/session/tests/automation.rs
  - crates/ui/src/arrangement/automation_lanes.rs
  - crates/ui/tests/automation_lanes.rs
  - crates/project-file/src/lib.rs
---

# Automation

A parameter can follow an **envelope** over the arrangement: a lane of points, each a beat and a
value, that the parameter moves along while the transport plays. This is Suhina's own design
(#395); it extends the plug-in lanes of #109 to every parameter. Lanes are set by command (CLI,
MCP) and drawn and edited in the Arrangement view (#420, [below](#in-the-arrangement-view)).

## What can be automated

| Parameter | Lane id | Values |
|---|---|---|
| A track's volume | `volume` (on the track's mixer) | a linear gain in the model, 0 to +6 dB; commands take and show decibels, as `track.setVolume` |
| A track's pan | `pan` | -1 (left) to 1 (right) |
| A track's mute | `mute` | 0 (sounding) or 1 (muted), a step |
| A built-in effect or instrument's parameter | its stable id (`devices.list`) | in the parameter's unit, kept in its range as `device.setParam` keeps it |
| A data-only `.sd` device's parameter | its stable id in the `.sd` graph | in its unit and range; discrete controls step |
| A plug-in's parameter | its id, a number (`device.pluginParams`) | in the plug-in's own range (#109) |

Send levels get lanes once sends play (#396). The master channel's mixer is not automated yet.

## Lanes and points

A lane belongs to what it automates: mixer lanes to the track's channel
(`Channel::automation`), device lanes to the device (`Device::automation`), so a lane moves, copies
and is deleted with its device. One lane per parameter.

- Points are kept in beat order. Before the first point the lane holds the first point's value;
  after the last, the last's.
- Between two points the value moves along the first point's **curve**, -1 to 1: 0 is a straight
  line; otherwise the share of the way covered at `t` (0 to 1) of the segment is `t` to the power
  4^-curve, so 1 rises fast and settles, -1 starts slowly and rises late. The same shape goes up
  and down.
- **Two points at one beat make a jump**: the value reaches the first, then leaves from the second.
  A new point at a beat that has points goes after them.
- On/off and choice parameters, and the mute, are **steps**: each point's value holds until the
  next point; curves do not apply. Whole-number parameters move along the line and the device
  rounds.
- A lane is **enabled** or not. One that is off does not play: the parameter keeps the value set on
  it (the channel's volume, the device's `params`).

## A manual change turns the lane off

Setting a parameter that follows an enabled lane by hand (`track.setVolume`, `track.setPan`,
`track.setMute`, `device.setParam`) turns its lane off in the same undo step, whether or not the
transport runs, and the answer says `"automation": "off"`. The value set by hand holds until
`automation.setEnabled {enabled: true}` turns the lane on again; the lane's points are kept. Undo
brings the lane back on with the old value.

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
  semitone, scale-degree and key values show as `+5 st`, `+2 sd` and `C3`. `.sd` processors receive the same
  changes through their parameter API; integer, toggle, button and choice controls step. A fresh
  device starts at the lane's value at beat 0. While stopped, a parameter keeps its last value.
- **Plug-in parameters**: as #109 built them (ADR 0006 › 3): a value at the block's start, at
  each point and every 64 frames while it moves, as parameter events at their frames; the curve
  and the enabled flag apply too.
- **Mixer lanes**: the track's gains are computed at every 64th frame of the arrangement and move
  in a straight line between, frame by frame, so a frame's gain depends on its time only (not on the
  block size) and a fader move or mute never clicks: a mute switches over 64 frames. A track whose
  mixer has a lane that plays runs through a device chain (one without devices when it has none),
  and its gains apply after its devices, as for any track with devices. A group track with mixer
  lanes becomes a bus (ADR 0004 › 10), so its lanes scale the sum of the tracks routed into it.
  The lanes are read earlier by the chain's latency compensation, so they line up with what is
  heard. While stopped, the gains are the lanes' values at the play start marker.
- **Mute and solo**: a mute lane decides the channel's mute while it plays (a muted channel with a
  mute lane sounds where the lane says 0). Solo is not automated.

A corner of a lane that falls between two 64-frame steps is cut across that step (at most 1.3 ms
early or late in shape, not in time: a jump's own frame is exact for device and plug-in
parameters).

## Commands

Every edit is one undo step; a parameter is named by `track`, `device` (left out for the track's
mixer) and `param`. Values are in the unit of the parameter's set command.

| Command | What it does |
|---|---|
| `automation.addPoint {track, device?, param, beat, value, curve?}` | adds a point, making the lane (enabled) if there is none; answers the point's index in beat order |
| `automation.movePoint {track, device?, param, point, beat?, value?, curve?}` | changes a point; answers its new index |
| `automation.deletePoint {track, device?, param, point}` | removes a point; the lane goes with its last point |
| `automation.movePoints {track, device?, param, points, beats?, value?, curve?}` | moves several points of one lane together in one undo step: `beats` later (earlier below 0), `value` up or down in the set command's unit (dB for the volume), each value kept in the parameter's range (a silent volume point stays silent, a louder one stops at +6 dB), `curve` given to each; answers the points' new indices in the order given |
| `automation.deletePoints {track, device?, param, points}` | removes several points in one undo step; the lane goes with its last point |
| `automation.clear {track, device?, param?}` | removes a lane, or every lane of the mixer or the device |
| `automation.setEnabled {track, device?, param, enabled}` | turns a lane off or on again |
| `automation.list {track}` | the track's lanes: `mixer` and `devices` (index, name, lanes), each with `param`, `enabled` and `points` (`beat`, `value`, `curve`) |
| `device.setAutomation {track, device, param, points}` | replaces a device lane with straight segments (`points` a flat list of beat and value; `[]` removes it), for built-in devices, `.sd` devices and plug-ins |

Refused, changing nothing: an unknown track, device or parameter; a beat below 0 or past the
arrangement's limit; a value out of range for the mixer (a volume above +6 dB, a pan outside -1 to
1, a mute other than 0 or 1); a curve outside -1 to 1; a point index the lane does not have; a lane
of more than 65,536 points; a `movePoint` with nothing to change.

## In the Arrangement view

Each track header has an envelope button at the end of its first row ("Show automation of
Drums"); a click shows the track's automation lanes under it, and another hides them. Shown the
first time, they are the lanes that have points, or the volume's. Which lanes are shown is the
view's own state, like a lane's height: it is not saved and needs no command. Return tracks and
the main track have no lanes yet.

A lane is 40 px tall across the timeline, with a header under the track's:

- **The parameter**, a dropdown of the track's volume, pan and mute and each built-in or `.sd`
  device's parameters, grouped by device; a parameter with points is marked " •". Choosing one
  shows it in this lane. Plug-in parameters can be set by command but are not yet in this dropdown.
- **On/Off** (lanes with points): whether the lane plays. A manual change to the parameter turns
  it off (the envelope is drawn in `text.hint` and the header says "Off: set by hand"); a click
  turns it on again (`automation.setEnabled`), or off.
- **+** shows the next parameter not shown in a new lane; **×** hides this lane.

The envelope is drawn in the track's colour through every point, holding before the first and
after the last, along each segment's curve; steps for mute, on/off and choices. Values are placed
as the parameter's control places them: the volume as the mixer's volume field (0 dB at 85 % of
the height), a device's parameter as its knob turns. Editing (every edit is one undo step):

| Gesture | Command |
|---|---|
| Click the lane | `automation.addPoint` at the beat under the pointer (on the clip grid of the zoom; Alt: free) and the value at its height |
| Drag a point | `automation.movePoint`, its beat on the grid (Alt: free) and its value following the pointer; points dragged past others keep beat order |
| Click a point | selects it; Shift or Ctrl adds it to the selection or takes it out. Selecting a clip ends a selection of points |
| Drag on the lane away from the line | draws a box that selects the points in it |
| Drag a selected point | `automation.movePoints`: the whole selection moves by the same beats (none before beat 0) and the same change of value |
| Delete (or Backspace) | `automation.deletePoints` for the selection, before any selected clip |
| Double-click a point, or its menu's Delete point | `automation.deletePoint` (the menu deletes the whole selection when the point is part of one) |
| Drag the line between two points up or down | `automation.movePoint` with the first point's curve: up bends the line up, from -1 to 1 |

## In the project file

`automation` on a channel and on a device: `[{ param, points: [{ beat, value, curve? }],
enabled? }]`. `curve` is written only when it is not 0 and `enabled` only when it is false, so a
file without automation, or with straight lanes that play, keeps its shape
([suhina-format.md](suhina-format.md)). A curve outside -1 to 1 is refused, as is a lane of more
than 65,536 points; points are read in beat order.

## Not done yet

- Recording automation, drawing with a pencil, and plug-in parameter lanes in the view.
- The master channel's mixer, send levels (#396), the tempo (#402).
- Importing automation from other formats.
