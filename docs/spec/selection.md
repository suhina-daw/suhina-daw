---
description: The selection an open project carries - the selected track and the selected device of its chain - as commands and a reading, and the window that shows it
read_when: selected track, selected device, selection, track.select, device.select, project.selection, controller focus, mapping selected target, what the knobs control
type: spec
status: current
last_verified: 2026-11-21
covers:
  - crates/session/src/selection.rs
  - crates/session/src/commands/selection.rs
  - crates/session/src/commands/device.rs
  - crates/session/src/devices.rs
  - crates/session/src/commands.rs
  - crates/device-registry/src/lib.rs
  - crates/ui/src/app.rs
  - crates/ui/src/app/actions.rs
---

# The selection

- **Sources:** Suhina's own design, from the command registry ([ADR 0002](../decisions/0002-commands.md))
  and the controllers and mapping design ([ADR 0019](../decisions/0019-midi-controllers-and-mapping.md)),
  whose `selected` track/device targets are "whatever is selected in the window" ([MIDI
  controllers and mapping](midi-mapping.md) › Targets).
- **Status:** current ([#1679](https://github.com/suhina-daw/suhina-daw/issues/1679)). The
  session's own state, the commands, the `project.selection` reading and the window are built: a
  click selects a track and picks a device, a command from the CLI, MCP or a surface program
  moves the highlight and the chain, and both read and set the same selection. The window
  already selects: both views share one track selection (#208) and a device is picked in the
  chain; that selection is now the session's, so commands, controllers, mappings and agents
  point at the same one.

## What the selection is

- **One per open project**: the **selected track**, and one **selected device** in that track's
  chain. One state: a click in the window and a command set the same selection, so they cannot
  disagree.
- **Not an edit.** The selection records no undo step; undo and redo never move it, and it is not
  saved with the project. Its commands are project-targeted with the registry's `Change` effect:
  they change something outside the undo history — what the session points at — as the effect's
  own words say. The main channel is among the selectable tracks, addressed as the device
  commands address it (`"main"`).
- **An open project begins with nothing selected**; the window's own habit (selecting as a
  project opens) is the selection being set, not a second state.
- **The window's multi-picks are its own.** Several tracks or devices picked with Ctrl (Cmd) or
  Shift for the commands that take many stay window state; the selection is the one the
  controllers, mappings and agents point at — the launcher's selected track and the device
  picked in the chain.
- **A device inside a group can be selected** by its path, with the same addressing every other
  device command uses (`"2/1/0"` — [`track.devices`](midi-mapping.md) lists the paths).

## The commands

| Command | Parameters | What it does |
|---|---|---|
| `track.select` | `track` | Selects that track (id or name). A track that is not the selected one selects its first device (none when its chain is empty); the already-selected track changes nothing. |
| `track.selectNext`, `track.selectPrevious` | — | The next or previous track in the project's order; with nothing selected, the first (next) or last (previous) track. At either end nothing happens, and it is not an error — the mapping spec's rule for a relative target with nothing to point at. |
| `device.select` | `device`, optional `track` | Selects a device on the selected track — or, with `track` named, that track's device, selecting the track too. Naming a device or track that does not exist is refused with its reason, like every other device command, and so is having no track selected and none named. |
| `device.selectNext`, `device.selectPrevious` | — | The next or previous device of the branch the selected device sits in — its track's top-level chain for a device not inside a group. With no track or device selected, an empty chain, or an end reached, nothing happens and it is not an error. |

None of them records an undo step; all of them work from the window, the CLI and MCP, and an
agent attached to the app may run them (the selection is project state, not settings).

## A removal clears it

- When the selected track or the selected device is removed, nothing is selected: the window's
  highlight is gone and `project.selection` answers with nothing. Undoing the removal does not
  bring the selection back — the selection is not an edit.

## The reading

`project.selection` answers what is selected, for a window, a script or an agent to follow:

- `track` — the selected track's id and name, or nothing.
- `device` — the selected device's entry as [`track.devices`](midi-mapping.md) lists it: its
  path, kind, id and name, and its parameters. A built-in's parameters carry `name`, `group`,
  `min`, `max`, `unit` and `kind` beside `id` and `value` (`continuous`, `integer`, `toggle` or
  `choice`) — the registry held them, and the `.sd` devices' branch of the reading already
  emitted its own `control` — so a controller's knob can take the continuous dials and leave the
  switches to the panel, scale onto the unit and draw the position. A built-in also carries
  `dials`: the parameter ids its knobs can turn — the page the panel shows, its dials left to
  right and top to bottom, then the dials that stay on every page in the panel's section order,
  then the other pages' dials in the panel's order — so a controller's knobs follow the device's
  layout, not its parameter order, and Bank pages through them. A compact panel with no native
  dials keeps its continuous parameters in parameter order. A dial also carries `normalized`:
  the position its panel's dial shows, 0 to 1, beside `value`, and `scale` (`linear`,
  `logarithmic` or `gain`), so a controller knows a log dial wants a fine step. A plug-in's
  parameters are its stored values, and `device.pluginParams` is the way to its full list.
- With nothing selected, both are nothing and the parameters are empty.

### A controller's dials

A controller's knob takes a dial from `dials` and sends `device.setParam` with `normalized`
(0 to 1) — the position its panel's dial shows — instead of `value`. The parameter's own scale
(linear, logarithmic or a gain law) turns it into the unit, and the reading carries the same
`normalized` for the ring, so a logarithmic time or frequency moves smoothly instead of jumping
between the ends of a linear value range. A logarithmic dial sends `delta`, a 1/512 share of its
travel per encoder step, and the session adds it to the dial's current normalized position: a
1/127 position would jump a tenth of a log time, while the fine step is a fortieth. Linear dials
send their normalized position. A controller maps the page the panel shows, and a page
or operator change in the window moves its dials with it because the reading carries the page.

The FM Synth is the first such panel: its page 0, operator 1 envelope dials (attack, decay,
sustain, release, initial, peak, by vel and by key) come first, then the always-visible LFO rate
and amount, filter frequency and resonance, Spread, Volume and Random amount dials; banks page
through the rest. The LFO's Rate dial computes its hertz from the stored position, and a
controller's ring uses the same normalized position either way
([FM Synth](fm-synth.md#panel), #1718).

## The window follows

- A command from the CLI or MCP moves the window: the track highlight and the device chain's
  shown device follow at the next frame.
- A command that moves the selection while a frame is drawn — a surface program's device arrow,
  a CLI or MCP call — is the session's: the frame's end does not publish the window's older
  pick, so the next frame shows the command's selection instead of springing back to it.
- A click that selects a track or picks a device sets the same selection — the window runs the
  commands, as it runs every action.
- Each open project keeps its own selection, so switching tabs switches it.

## Testing

- The commands and the reading: select, step, read back; the selected track's first device;
  the already-selected track changing nothing; the ends, the empty chain and no selected track
  doing nothing and not erroring; a named bad device or track refused with its reason; the
  built-in parameters carrying `name`, `min`, `max`, `unit`, `kind`, `scale` and `normalized`,
  with the dials in panel order, a page change moving them, and a fine `delta` adding a small
  share of a log dial's travel.
- A removal clearing the selection, and undo not restoring it.
- The window: a command-set selection showing (the highlight and the chain), and a click
  changing the reading.

## Not yet

- The arrangement view's own clip and multi-track selection as part of the focus; it keeps its
  own for now.
- Stepping into a device group's branches with the next/previous commands (a device inside one
  is selected by path).
- Saving the selection with the project, or naming it in `project.info`.
