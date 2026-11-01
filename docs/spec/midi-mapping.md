---
description: What a MIDI controller can do in Suhina - sources, targets, ranges, takeover, relative encoders, gestures and undo, the controller profile file, mappings saved in a project, the mapping mode and MIDI learn, the commands, and how malformed input is treated
read_when: MIDI learn, MIDI mapping, map a knob, mapping mode, controller profile, controllers folder, controller_mappings, takeover, pick up, relative encoder, 14-bit CC, mapping.add, mapping.learn, controllers.list, gesture, controller feedback
type: spec
status: planned
last_verified: 2026-10-28
---

# MIDI controllers and mapping

- **Sources:** Suhina's own design ([ADR 0019](../decisions/0019-midi-controllers-and-mapping.md)),
  and the MIDI 1.0 specification for what the messages are (control change, note on and off,
  pitch bend, program change, 14-bit controller pairs).
- **Status:** planned. Nothing here is implemented; the split into work is in the pull request
  that added this document ([#403](https://github.com/suhina-daw/suhina-daw/issues/403)). It
  builds on MIDI input (ADR 0017, [#291](https://github.com/suhina-daw/suhina-daw/pull/291)).

## Words

- A **controller** is a device with knobs, faders, buttons, pads or keys that sends MIDI, seen by
  Suhina as one or more input ports (and, for feedback, output ports).
- A **control** is one of its knobs, faders, buttons or pads, identified by the message it sends:
  its kind, its channel (1 to 16) and its number.
- A **target** is a command and the parameter it sets, with its other parameters fixed: "set the
  volume of track 3" is `track.setVolume` with `track: 3` and the value going to `db`.
- A **mapping** joins one control to one target, with the settings in *Shaping the value*.
- A **profile** describes one controller model and holds the mappings that work in any project.

## Sources

| Kind | Message | Value | Notes |
|---|---|---|---|
| `cc` | Control change, number 0 to 119 | 0 to 127 | CC 120 to 127 are channel mode messages and are never mapped. CC 64 is mapped only when no armed track uses it as the sustain pedal (the mapping wins when one exists, see *Conflicts*). |
| `cc14` | Control change pair: number *n* (0 to 31) is the high part, *n* + 32 the low part | 0 to 16383 | The value is updated on the low part. A high part without a low part within 20 ms is taken alone, as 7 bits. |
| `note` | Note on and note off, key 0 to 127 | velocity 1 to 127, or 0 for off | A note on of velocity 0 is a note off. |
| `bend` | Pitch bend | 0 to 16383, 8192 at rest | Per channel. |
| `program` | Program change | 0 to 127 | No value of its own: the number is the control, and the message is a trigger. |

A source is written `kind:channel:number`, for example `cc:1:21`, `cc14:1:7`, `note:10:36`,
`bend:2`, `program:1:5`. With several ports, a source also names its port: by the profile that
matched the port, or by the port's name.

**Relative encoders** send changes, not positions. A `cc` control may be marked relative with
one of three encodings:

| Encoding | Up | Down |
|---|---|---|
| `twos` (two's complement) | 1 to 63 is +1 to +63 | 127 to 64 is -1 to -64 |
| `sign` (sign and magnitude) | 1 to 63 is +1 to +63 | 65 to 127 is -1 to -63 |
| `offset` (offset from 64) | 65 to 127 is +1 to +63 | 63 to 0 is -1 to -64 |

A value that means no change (0 in `twos` and `sign`, 64 in `offset`) is ignored.

## Targets

A command can be a target when its registry entry describes its **value**
([ADR 0019](../decisions/0019-midi-controllers-and-mapping.md) decision 2). The first set:

| Target | Command | Value kind |
|---|---|---|
| Track volume | `track.setVolume` | range, dB, from silence to the command's maximum, with the fader's curve |
| Track pan | `track.setPan` | range, -1 to 1 |
| Track mute, solo | `track.setMute`, `track.setSolo` | toggle |
| Arm a track | `track.setArm` | toggle |
| Stop a track's clips | `track.stop` | trigger |
| A device parameter | `device.setParam` | the parameter's own kind, range and curve from `ParamInfo` |
| Device on and off | `device.setEnabled` | toggle |
| Launch a clip slot | `clip.launch` | trigger |
| Launch a scene | `scene.launch` | trigger |
| Stop all clips | `launcher.stopAll` | trigger |
| Play, stop | `transport.play`, `transport.stop` | trigger |
| Tempo | `transport.setTempo` | range, BPM, the command's range |
| Metronome | `transport.setMetronome` | toggle |
| Undo, redo | `edit.undo`, `edit.redo` | trigger |

Sends, plug-in parameters and launch quantisation join when they are commands with a value. A
target may be **relative to the focus** instead of naming a track or device:

- `selected` track or device: whatever is selected in the window when the message arrives;
- `bank` track *n*: the *n*-th track (1 to 8) of the current bank, a run of eight tracks that
  starts at the leftmost track shown in the Launcher view, moved by the triggers
  `mapping.bankNext` and `mapping.bankPrevious`;
- `selected` device parameter *n*: the *n*-th parameter of the selected device, in the order the
  device lists them.

A relative target with nothing to point at (no track selected, a bank slot past the last track,
a device with fewer parameters) does nothing and is not an error.

## Shaping the value

Each mapping has:

- **min** and **max**, in the target's units, inside the target's range (the whole range by
  default). The control's lowest value gives **min** and its highest gives **max**; **min** may
  be greater than **max**, which inverts the control.
- **curve**: the target's own curve (the default; for volume, the fader's dB curve) or linear.
- **takeover** for absolute controls on range targets: `jump`, `pickup` (the default) or `scale`;
  see below.
- **speed** for relative encoders: how many steps cross the whole range, 128 by default.
- For a **note** or a `cc` button on a toggle target, the **button mode**: `toggle` (each press
  flips it, the default) or `momentary` (on while held). A `cc` button counts a value of 64 or
  more as pressed. Velocity is ignored for toggles and triggers.
- For a **note** or `cc` on a range target, the velocity or value is the position.
- For a **choice** target, the control's range is split evenly between the options; a button
  steps to the next option and wraps.

### Takeover

With an absolute control, the control's position and the target's value differ whenever the value
was changed by something else (the mouse, an undo, another project). Positions are compared after
shaping, in the target's normalized range 0 to 1:

- `jump`: the value becomes the control's position at its first message.
- `pickup`: messages do nothing until the control reaches or crosses the current value (a message
  on the other side of it from the previous message, or within 1/127 of it); from then on the
  value follows the control. The window shows a picked-up control as waiting (see *Mapping mode*).
- `scale`: each message moves the value by the control's movement times
  `(1 - value) / (1 - previous)` when moving up and `value / previous` when moving down, so the
  value and the control meet at either end and then move together.

A control is **caught** once the value follows it. It stays caught until the value is changed
by something other than this control.

## Gestures and undo

A mapped control that sends values opens a **gesture** on its first message to a range target.
While the gesture is open:

- each value is applied at once: it is heard from the next audio block and shown in the window;
- the project is not rebuilt and no undo step is recorded per value.

The gesture **ends** when the control has sent nothing for 400 ms, when the project or the
active tab changes, or when an undo or redo is asked for (the gesture ends first, then the undo
undoes it). Its end records **one** undo step, the same as one run of the command with the last
value. A gesture whose last value equals the value before it records nothing.

A toggle or trigger runs its command once per press and records what that command records (a
transport command records nothing).

## Conflicts

- A control claimed by a mapping does not reach the live-note queue: a pad mapped to a clip
  launch does not also play a note on an armed track. Keys and the pedal that no mapping claims
  play as they do today (ADR 0017).
- When a project mapping and a profile mapping use the same control, the project mapping wins
  while its project is the active tab.
- One control has at most one mapping in each place. Mapping a control that is already mapped
  replaces that mapping (with undo, for a project mapping).
- One target may have several controls mapped to it.

## The profile file

A profile is a JSON file in the user's settings folder: `<settings>/controllers/<id>.json`, next
to `settings.txt` and `keymap.json`. `id` is the file name without `.json`, made of letters,
digits and `-`. `SUHINA_CONTROLLERS` overrides the folder for tests and development.

```json
{
  "name": "Eight knobs and eight pads",
  "ports": ["*Knob Box*"],
  "takeover": "pickup",
  "controls": [
    { "source": "cc:1:21", "relative": "twos", "label": "Knob 1" },
    { "source": "note:10:36", "label": "Pad 1" }
  ],
  "mappings": [
    { "source": "cc:1:21", "target": { "command": "track.setVolume", "track": { "bank": 1 } } },
    { "source": "cc:1:22", "target": { "command": "device.setParam", "track": "selected", "device": "selected", "param": { "index": 1 } }, "min": 0.2, "max": 0.8 },
    { "source": "note:10:36", "target": { "command": "transport.play" } },
    { "source": "note:10:37", "target": { "command": "mapping.bankNext" } }
  ]
}
```

- `ports` are patterns matched against input port names (`*` matches any text, case ignored).
  The first profile whose pattern matches a port is that port's profile. A port that matches none
  is used without a profile: only project mappings that name it reach it.
- `controls` is optional. It declares what the controller has: a relative encoding, a label shown
  in the mapping list, `"feedback": true` when the control takes its value back (later). A source
  used in `mappings` but not declared is absolute.
- Each mapping has `source`, `target` and the optional settings from *Shaping the value*
  (`min`, `max`, `curve`, `takeover`, `speed`, `button`). A target names its command and fixes
  the command's other parameters, by value or by focus (`"selected"`, `{ "bank": n }`,
  `{ "index": n }` for a device parameter).
- Profile mappings may only use targets that name nothing in a project: focus targets, the
  master track, transport and history. A profile mapping with a fixed track or device is refused
  with a warning.

**Reading a profile.** It is user-written input:

- a file over 1 MiB, or with more than 4096 mappings, is not read, with a notice;
- a mapping with an unknown command, a command that cannot be mapped, a source that does not
  parse, or a value out of range is skipped with a warning naming the file and the mapping's
  position; the rest of the file is used;
- unknown fields are ignored; a file that is not JSON is not read, with a notice;
- profiles are read when Suhina starts and again when one changes on disk; a bad edit never
  removes the mappings that were working (the previous version is kept until a valid one is read).

Suhina writes a profile only when the user saves one from the mapping mode, and then writes the
whole file again, keeping fields it does not know.

## Mappings in a project

A project keeps its own mappings in `.suhina` as a list, `controller_mappings`, left out when
empty:

```json
"controller_mappings": [
  { "profile": "knob-box", "source": "cc:1:23", "target": { "command": "device.setParam", "track": 4, "device": 1, "param": "cutoff" }, "takeover": "jump" },
  { "port": "USB MIDI 1", "source": "note:1:60", "target": { "command": "clip.launch", "track": 2, "scene": 0 } }
]
```

- A source names a `profile` (by id) or a `port` (by name), never both.
- Targets name tracks by id and devices by position, like the commands. When a track or device is
  deleted, its mappings stay in the list, unused and shown as broken in the mapping list, so an
  undo brings them back working; `mapping.removeBroken` deletes them.
- A Live Set does not bring mappings with it; an imported project starts with none.
- Reading follows the format's rules for malformed input: a mapping that does not parse is
  dropped with a warning, the rest of the project opens.

## Mapping mode

- **Turning it on**: the **Map** button at the right of the title bar, or Ctrl+M
  (`view.mappingMode`). The button stays lit while the mode is on; Escape or Ctrl+M again leaves.
- **What it shows**: every control in the window that has a mappable target (a fader, pan knob,
  mute, solo and arm button, a device's knobs, a clip slot, a scene's launch button, the play and
  stop buttons, the tempo) gets a tinted overlay; a control that is mapped shows the source in
  small text on the overlay (`CC 21`, `Note 36`). Clicking a control selects it and does not
  operate it.
- **Learning**: with a control selected, the next message from any controller that is not a note
  off or a value of zero on a button is taken as its source. A mapping is made in the project, or
  in the active port's profile when **Save to profile** is ticked in the mapping list and the
  target names nothing in the project. A relative encoder is recognized when it sends only values
  near 1, 127, 63 or 65 during learning; otherwise the control is absolute.
- **The mapping list**: while the mode is on, the detail area shows every mapping that reaches the
  active project: its source, target, min, max, takeover, speed and button mode, where it is kept
  (project or profile name), and whether it is caught. Each value is edited in place; Delete
  removes the selected mapping.
- **Outside the mode**: a control whose mapping waits to pick up shows a thin marker at the
  control's position until it is caught.

## Commands

| Command | Kind | What it does |
|---|---|---|
| `controllers.list` | controllers | The input ports, each with its profile (if any) and whether it is open |
| `controllers.profiles` | controllers | The profiles read, with their warnings |
| `controllers.saveProfile` | controllers | Writes a profile from the given JSON (checked as when read) |
| `mapping.list` | read | The mappings that reach the active project, project and profile, with their state |
| `mapping.add` | edit | Adds a project mapping: `source`, `target`, and the optional settings |
| `mapping.set` | edit | Changes a project mapping's settings |
| `mapping.remove` | edit | Removes a project mapping, by its index in `mapping.list` |
| `mapping.removeBroken` | edit | Removes the project mappings whose target no longer exists |
| `mapping.learn` | transport | Waits up to 10 s for the next control moved and returns its source; makes no mapping |
| `mapping.bankNext`, `mapping.bankPrevious` | transport | Move the bank of eight tracks |

`controllers.*` runs on `Projects`, like `audio.*`; agents over MCP may run all of them. Adding a
mapping over MCP is the way an agent maps without a window.

## Feedback (later)

When a profile marks a control `"feedback": true` and its controller has an output port with a
matching name, Suhina sends the target's value to it after every change from any source: a CC
value for a range, 127 or 0 for a toggle, and for a clip slot a note velocity that says whether
the slot is empty, holds a clip, is queued or plays. A value moved by a gesture is not sent back
to the control that moves it. The velocities a pad needs for its colours are the profile's to say
(`"states": { "empty": 0, "stopped": 1, "queued": 2, "playing": 3 }`).

## Surface programs (later)

A surface program is an extension ([`extensions.md`](extensions.md)) whose manifest says
`"provides": "surface"` and lists the port name patterns it claims. Suhina passes it every
message from those ports, system exclusive included, sends what it asks on their output ports,
and gives it the command protocol: it runs commands, reads state through reading commands and
subscribes to changes. It never reaches the engine or the model by another way. When it is not
running, its ports fall back to their profiles.

## Testing

Each part lands with tests, by what it is:

- parsing sources and profiles: round trips, every malformed case above, the size caps;
- shaping: min and max, inversion, curves, the three relative encodings, 14-bit pairs, button
  modes, choice targets;
- takeover: jump, pickup from above and below and by a jump past the value, scale meeting at
  both ends;
- gestures: a stream of 200 values is one undo step; an undo in the middle ends and undoes it;
  the engine hears the values during the gesture (offline render of a moving volume);
- the router: a mapped note does not reach an armed track; an unmapped one does; the callback
  never waits on the session (a full control queue drops the message and counts it);
- project mappings: `.suhina` round trip, a deleted track's mapping stays and comes back with
  undo.
