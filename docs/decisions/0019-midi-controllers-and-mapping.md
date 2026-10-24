---
description: How hardware MIDI controllers drive Suhina - MIDI learn onto any command that takes a value, controller profiles in the user's settings, mappings to one project's tracks and devices saved in that project, gestures that make one undo step, and surface programs as extensions
read_when: MIDI controller, MIDI learn, MIDI mapping, remote mapping, map a knob, CC, control change, fader, encoder, relative encoder, takeover, pick up, controller profile, control surface, mapping mode, controller feedback, LED, MIDI output
type: decision
status: draft
last_verified: 2026-10-15
---

# 0019: MIDI controllers and mapping

- **Status:** proposed
- **Date:** 2026-10-15
- **Task:** [#403](https://github.com/suhina-daw/suhina-daw/issues/403). Behaviour spec:
  [`docs/spec/midi-mapping.md`](../spec/midi-mapping.md).

## Context

ROADMAP › Later lists MIDI controllers and remote mapping, and
[ADR 0016](0016-parity-target.md) puts it in the third tier (item 11): a performance feature for
people who already work in Suhina. This record is the design, written before any code, so the
pieces it touches are not built in a way that has to be undone.

What exists, or is in review:

- **MIDI input** (ADR 0017, [pull request #291](https://github.com/suhina-daw/suhina-daw/pull/291)):
  `crates/midi` opens every input port with `midir` and reads note on and off, the sustain pedal
  (CC 64) and all-notes-off (CC 120, 123); everything else is dropped. Notes reach the engine over
  a live-note queue from the driver's callback thread and play on armed tracks. Each event carries
  its source (a hash of the port name and the channel), and each track chooses which source it
  hears.
- **Commands** ([ADR 0002](0002-commands.md)): every change is a command with an id and JSON
  parameters, run by the window, the CLI and MCP alike. An edit records one undo step per run.
- **Edits while playing** ([ADR 0005](0005-edits-while-playing.md)): an edit rebuilds the
  engine's graph, except a device parameter, which has a cheap path
  (`Player::set_device_param`) that changes one value from the next block.
- **The window's drags**: a volume drag in the Session View shows the moving value locally and
  runs `track.setVolume` once, when the mouse is released (`crates/ui/src/session/session_view.rs`).
- **Settings and keymap**: the window keeps `settings.txt` and `keymap.json` in its storage
  folder; the keymap maps keys to commands that take no parameters.
- **Extensions** ([ADR 0015](0015-extensions.md)): an optional component is a separate program
  with a manifest, started by Suhina and spoken to over a protocol, never a library.

A controller differs from the mouse in three ways that shape the design. It sends a stream: a
fader moved for a second is a hundred messages, and each being an edit would mean a hundred undo
steps and a hundred graph rebuilds. It has its own idea of where a control is: a fader at the
bottom that is mapped to a track at 0 dB jumps the volume when touched. And it may answer back:
lights on buttons, rings around encoders, motor faders, which need MIDI output Suhina does not
have.

## Decision (proposed)

1. **A mapping runs commands; it never changes the model itself.** A mapping joins one control
   on a controller (a source) to one command and the parameter it sets (a target). What a mapped
   control does is exactly what the same command does from the window, the CLI or MCP (rule 6):
   the same validation, the same undo, the same engine path. There is no second way to change a
   track's volume.

2. **Commands say whether, and how, they can be mapped.** The registry gains a description of a
   command's **value**: which parameter carries it, its kind (a **range** with a minimum, maximum
   and curve, a **toggle**, a **choice** of options, or a **trigger** that takes no value) and,
   for a range, its unit. `command.list` reports it. The other parameters (the track, the
   device, the parameter id, the clip slot) are fixed by the mapping. A device parameter's range
   and curve come from its `ParamInfo` ([ADR 0004](0004-built-in-devices.md)); a plug-in's
   parameters join when plug-in parameters become commands. Commands without a value description
   cannot be mapped, so a mapping can never run something that opens a file, scans plug-ins or
   changes settings.

3. **A continuous movement is one gesture and one undo step.** A mapped control that sends a
   stream of values opens a **gesture** on its first message: the values are applied live and the
   session keeps the state from before the gesture. When the control rests (no message for 400 ms,
   or a touch-sensitive control is released), the gesture ends and the history records one step,
   the same one a single run of the command would have recorded. The session gains this as one
   general mechanism (a run that is part of an open gesture replaces the gesture's live value
   instead of pushing a step), so the window's own drags can use it too. While a gesture is open,
   values reach the engine without a graph rebuild: device parameters over the existing cheap
   path, track volume, pan and sends over a new one of the same kind (a message the renderer
   applies at the next block, with smoothing so a jump is not a click). Triggers and toggles are
   single runs and need no gesture.

4. **Two places for mappings, by what they point at.**
   - A **controller profile** describes one controller model: which port names it matches, what
     its controls are (CC, note, pitch bend, program change; absolute or one of the relative
     encodings; 7 or 14 bits), and mappings whose targets do not name anything in a project:
     transport, tempo, metronome, undo and redo, the master track, and **relative targets** such
     as "the selected track's volume", "the selected device's third parameter" or "the volume of
     track 5 of the current bank of eight". Profiles live in the user's settings folder as
     `controllers/<id>.json`, one file per controller, and travel between projects. Suhina ships
     a profile only for a generic controller and writes none for a product it does not have
     permission to describe; users write or share their own.
   - A **project mapping** points at something in one project: a track by id, a device by its
     track and position, a clip slot, a scene. It is saved in the project (`.suhina`, a new
     `controller_mappings` list), because a track id means nothing in another project, and
     adding, changing or removing one is an edit with undo. It names its source by profile id
     and control (or, with no profile, by port name and message), so a project opened on another
     computer with the same controller keeps working, and one opened without it keeps its
     mappings unused rather than losing them.
   When both map the same control, the project mapping wins while that project is active.

5. **MIDI learn is the way in.** A mapping mode in the window (a button in the title bar,
   Ctrl+M, `view.mappingMode`) marks every control that can be mapped. Clicking one and moving a
   control on the controller makes the mapping; nothing else in the window reacts while the mode
   is on. The same is a command, so an agent or the CLI can map without the window
   (`mapping.add` with an explicit source, or `mapping.learn`, which waits for the next control
   moved). A list of the mappings, in the detail area while the mode is on, edits each one's
   range, inversion, takeover and relative speed.

6. **Takeover is chosen per mapping, with a default per profile.** For an absolute control whose
   position may differ from the value: **jump** (the value follows the control at once), **pick
   up** (the control does nothing until it passes the current value), or **scale** (the value
   moves towards the control's position in proportion, meeting it at either end). Pick up is the
   default. A relative encoder and a control with feedback that Suhina has just set need no
   takeover.

7. **Mapped messages are taken off the MIDI path, off the audio thread.** `crates/midi` keeps all
   channel messages instead of only notes and the pedal, and hands each to a **router** on the
   driver's callback thread. The router reads a table built from the active project's mappings
   and the profiles (replaced whole when they change, never edited in place) and sends a message
   either to the live-note queue (ADR 0017, unchanged) or, when a mapping or the learn mode claims
   it, to a control queue the session drains. A claimed note does not also play on an armed
   track. The session drains the control queue where it already polls the plug-ins
   (`Projects::poll_plugins`), and the router asks the window for a frame when it queues a
   message, so a mapped button is seen within a few milliseconds even when the window is idle.
   Each message keeps the driver's timestamp.

8. **Only what needs the sample clock goes to the engine directly, and nothing new does now.**
   Live notes and the pedal already do (ADR 0017). Launching a clip or a scene is quantised by
   the engine, so the few milliseconds of the control queue do not move it; a launch with
   quantisation off lands at the next block, as a mouse click does. Recording a mapped control's
   movement into automation, when automation exists, will place it by the message's timestamp
   (the same work as [#269](https://github.com/suhina-daw/suhina-daw/issues/269) for notes),
   not by moving mapping onto the audio thread.

9. **Feedback is MIDI output, and comes after input works.** `crates/midi` grows output ports.
   A profile says which controls take feedback and how (a CC value, a note velocity for a
   coloured pad). The session sends a mapped value back when it changes from any front end, so a
   motor fader follows an undo and a pad lights when a clip is launched from the window. Values a
   gesture is moving are not echoed to the control that moves them.

10. **Dedicated surfaces are surface programs, run as extensions.** A controller whose layout is
    more than a list of mappings (a grid of pads that shows the clip launcher, a screen, modes)
    is driven by a **surface program**: an extension ([ADR 0015](0015-extensions.md)), started
    and supervised by Suhina, with a manifest that names the ports it claims. Suhina hands it the
    raw messages of those ports (including system exclusive) and sends what it asks to send; the
    program changes Suhina only through commands and reads it only through reading commands and
    the state changes it subscribes to, as an agent over MCP does. Suhina embeds no scripting
    language. A surface program that crashes or hangs is restarted like a plug-in host, and its
    ports fall back to the profile's mappings meanwhile.

11. **Generic first.** The first version supports what a generic controller sends: CC (7 bit and
    14 bit pairs), notes, pitch bend and program change, on any channel; absolute, and relative
    in the three common encodings (two's complement, sign and magnitude, offset from 64). NRPN,
    feedback, surface programs and the computer keyboard as a controller come later, in that
    order; the keymap already maps keys to commands that take no parameters.

## Consequences

- One mechanism maps every control a user can see: a new command with a value description is
  mappable from the day it lands, with no work in the mapping code.
- The registry's parameter description grows (kind, range, curve, unit). The window's knobs and
  sliders can use the same description, which removes ranges that are now repeated in the UI
  (`VOLUME_RANGE`).
- Gestures change the history: a run can now replace the live value of an open step. Undo during
  an open gesture ends it first. The window's drags can move to gestures and show the value in
  the engine while dragging, which they do not today.
- The engine gains a block-rate path for track volume, pan and send levels, beside the one for
  device parameters; ADR 0005 is amended when it lands.
- The `.suhina` format gains `controller_mappings`; an older Suhina ignores the field. The
  spec's format section and [`docs/spec/suhina-format.md`](../spec/suhina-format.md) are updated
  with the code.
- `crates/midi` stops dropping controller messages and gains output ports; ADR 0017 is amended
  when the router lands.
- A controller profile is a file users write and share. It is data, read as hostile input
  (sizes capped, unknown fields skipped with a warning), never code.
- Surface programs inherit everything that is still open about extensions (protocol version,
  transport, trust; [ADR 0015](0015-extensions.md) › Not decided yet), and wait for it.

## Alternatives considered

- **A message per value, each an edit.** Simplest, and wrong: a fader move would flood the undo
  history and rebuild the graph a hundred times a second.
- **Mappings only in the project.** Each project would need its transport buttons mapped again.
  Rejected for targets that do not name anything in a project.
- **Mappings only in the settings.** A mapping to "track 7's filter" would point at a different
  track, or nothing, in the next project. Rejected for targets that name project content.
- **Resolving mappings on the audio thread.** It would let a mapped launch land on an exact
  frame, but the engine would have to know commands, ranges and takeover, and run the history.
  Launches are quantised anyway; the timestamp covers automation recording later.
- **An embedded scripting language for surfaces.** Fast to start with, but it puts other people's
  code in Suhina's process (against rule 5 and ADR 0015), and every language chosen would be a
  dependency and an interface to keep forever. A surface program can be written in any language
  that speaks the protocol.
- **Shipping profiles for commercial controllers written from their manuals.** Possible later,
  where a vendor publishes its MIDI implementation under terms that allow it; not decided here.

## Not decided here

- The exact file format of a profile (the spec proposes one) and whether profiles get a version
  field before the first release that reads them.
- How a surface program describes what it shows on a screen, and whether Suhina sends it ready
  text or the program asks for state.
- MIDI clock, Link and MPE, which are separate items in ADR 0016.
- Whether mapping a control to several targets at once (one knob, three parameters) is a
  mapping feature or waits for macros (ADR 0016 item 11).
