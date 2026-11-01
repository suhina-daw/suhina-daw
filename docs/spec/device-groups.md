---
description: Device groups - several devices on a track as one unit, with parallel branches, per-branch volume, pan, mute and solo, a mix, macros, group presets, latency compensation, the model and the commands
read_when: device group, group effects, group devices, parallel processing, parallel chain, branch, macro, macro control, mix, dry/wet, group preset, nested devices, layering instruments
type: spec
status: current
last_verified: 2026-11-01
covers:
  - crates/model/src/lib.rs
  - crates/model/src/groups.rs
  - crates/project-file/src/lib.rs
  - crates/engine/src/chain.rs
  - crates/engine/src/group.rs
  - crates/session/src/devices.rs
  - crates/session/src/device_groups.rs
  - crates/ui/src/detail/detail_view.rs
---

# Device groups

- **Sources:** Suhina's own design, from the issue
  ([#147](https://github.com/suhina-daw/suhina-daw/issues/147)), the device chain as it is built
  ([ADR 0004](../decisions/0004-built-in-devices.md) › 9 and 10), plug-in hosting
  ([ADR 0006](../decisions/0006-plug-in-hosting.md)) and the model
  ([ADR 0001](../decisions/0001-daw-neutral-model.md)). Nothing here is observed from another
  program.
- **Status:** steps 1 to 6 of the *Implementation plan* are built (#147): the model and file,
  groups in the engine with latency compensation, the commands, plug-ins inside groups and the
  panel with Ctrl+G. Macros (7), group presets (8) and key ranges (9) are still planned; the model
  already keeps the eight macros and their mappings. The questions the maintainer decides are in
  *Open questions*, answered by the defaults this document chose. Where this document had to
  choose, it says so and why, so a different answer changes one place.

## What it is

A **device group** is a device on a track's chain that holds other devices. From the outside it
behaves like one device: it sits at one position in the chain, has a name, can be switched off,
moved, removed, saved and loaded as one, and reports one latency. Inside, it holds one or more
**branches**. Each branch is a serial chain of devices; the group sends the same input into every
branch and adds their outputs together.

- One branch: the devices run one after another, as on the track. This is plain grouping: fold
  four devices into one box, switch them off together, save them as one preset.
- Several branches: parallel processing. A dry branch and a heavily compressed one; a low branch
  and a high branch after a crossover device in each; three instruments layered on the same
  notes.

Each branch has its own **volume, pan, mute and solo**. The group has a **mix** (how much of the
branches' sum against the group's input, the dry signal), an **output gain** and eight
**macros**: knobs whose movement drives one or more parameters of devices inside the group.

### Words

- *Device group* (in the interface: **Group**) is the unit. The word *group* alone already names a
  group track (a track other tracks play into, ADR 0004 › 10), so documents and code say *device
  group*; the interface says *Group* only inside the device panel, where nothing else is meant.
- *Branch* is one parallel path inside a group. Not *chain*: the engine's `Chain` is a track's
  whole device chain, and a branch is one level inside it.
- *Macro* is a group's own knob that controls parameters inside it.

## Behaviour

### Signal flow

For each block, a device group with input `x` (the stereo signal at its position in the chain):

1. Every branch gets a copy of `x` and runs its devices over it in order, as a track's chain does.
   A branch with no devices passes `x` through, so an empty branch is a clean dry path.
2. Each branch's output is delayed by its *compensation* (see *Latency*), then scaled by the
   branch's volume and pan, then added into the group's sum. Pan uses the same equal-power law as
   a track's channel (`track_gains`), centred at unity.
3. The dry signal is `x` delayed by the group's latency.
4. The group's output is `dry × (1 − mix) + sum × mix`, then multiplied by the output gain.

`mix` runs from 0 (only the dry signal) to 1 (only the branches), default 1, so a new group
sounds exactly like its branches. The crossfade is linear in amplitude: dry and wet are usually
correlated in a group, where an equal-power law would make the middle louder. Mix and output gain
are smoothed over 20 ms, as the utility smooths its gains (ADR 0004 › 5).

A group that is switched off (`enabled: false`) passes `x` through unchanged and its devices keep
running at their state, as a switched-off device does today.

### Mute and solo

- A muted branch adds nothing to the sum, but its devices keep running with silence after them:
  the same rule as a muted track (ADR 0004 › 9), so a plug-in in it keeps its process and its
  state and sounds again at once when unmuted, and a reverb tail is cut rather than restarted.
- While any branch of a group is soloed, only the soloed branches add to the sum. Solo is local
  to its group: it does not touch other groups, other branches outside the group or the track
  solo. Mute wins over solo on a branch (on a track, the track's own solo outranks its mute).
- Muting every branch leaves only the dry part of the mix.

### Instruments in a group

A group is either an **effect group** or an **instrument group**; it is never both.

- An *effect group* holds only effects (nested effect groups included). It can sit anywhere an
  effect can.
- An *instrument group* has an instrument (or an instrument group) as the first device of every
  branch that is not empty, and effects after it. It counts as the track's instrument: it must be
  first in the chain, one per track, and only on a track that plays notes, exactly the rules
  `device.add` applies to an instrument today. Every branch gets the track's notes as events at
  their frames, so the instruments are layered. There is no dry signal in an instrument group, so
  `mix` is fixed at 1 and not shown.

`device.duplicateMany`, `device.paste` and the group commands check these rules on the whole chain
after the edit, and a refused edit changes nothing. A MIDI effect, built-in or `.sd`, leads the
chain and goes only on a track that plays notes.

Splitting notes between branches by key or velocity (a bass on the low keys, a pad on the high
ones) is not in the first version: every branch hears every note. It is its own issue
(*Implementation plan*, 9), because it needs per-branch ranges in the model and a way to edit them.

### Nesting

A branch can hold device groups. Depth is limited to **4** levels of groups, which covers a
parallel branch inside a grouped preset inside a layered instrument and keeps paths short. The
limits that already exist count every device at every depth: a track holds at most 64 devices
(`MAX_DEVICES`), groups included. A group holds at most **16 branches**.

### Latency

Each branch's latency is the sum of its enabled devices' latencies (built-in `latency()`, a
plug-in's reported latency, a nested group's own). The group's latency `L` is the largest of its
branches'. Every branch with latency `l` is delayed by `L − l` before it is summed, and the dry
signal by `L`, so all of them line up. The group reports `L` to the chain it sits in, so the
track's latency (and with it the compensation across tracks and buses, ADR 0004 › 10) includes it
and nothing else changes outside the group.

- Delay lines are made with the graph, off the audio thread, at most 2 s
  (`MAX_LATENCY_SECONDS`), like the track compensation. A latency above that is cut to 2 s and the
  device panel says the group is not fully compensated.
- A muted or soloed-out branch still counts: muting must not change the group's latency, or the
  whole track would jump in time.
- A switched-off device inside a branch does not count, as on a track today (the switch builds a
  new graph).
- A plug-in's latency is known once it has started, which builds a new graph (ADR 0006); a group
  holding it is recompensated in that graph like a track.

### Macros

A device group has **eight macros**. Each has a name (default *Macro 1* … *Macro 8*) and a
value from 0 to 1, default 0. A macro holds up to **32 mappings**; a mapping names a parameter of
a device inside the group (at any depth) and a range on that parameter's travel:

- `min` and `max` are positions on the target's 0..1 travel (`ParamInfo`'s conversion, the same
  travel a knob or automation lane uses, ADR 0004 › 3), so a logarithmic frequency stays
  logarithmic. `min > max` inverts the mapping.
- The target's value is `travel⁻¹(min + (max − min) × macro)`. A choice or integer parameter takes
  the nearest step.
- One parameter is mapped by at most one macro. While it is mapped, the parameter follows the
  macro: the device panel shows it as mapped and `device.setParam` on it is refused with a message
  that names the macro. This keeps the value of every parameter defined by one source.
- A macro in a nested group can be mapped by a macro of the outer group, as any other parameter,
  so a preset's controls stay usable after it is put inside another group.

Applying a macro:

- **Built-in devices:** the engine computes the target values when the macro changes (a command,
  or later its automation) and sets them on the devices through the same path as
  `device.setParam`; the device smooths the change itself (ADR 0004 › 5). The project stores the
  macro's value and the mapped parameters' resulting values, so a project opened without macro
  support still sounds the same.
- **Plug-ins:** the session sends the computed value as `set_param` (ADR 0006 › 3), which reaches
  the processor at the start of the next block. That is block-accurate. Automation reaches a plug-in as
  parameter events at their frames ([plug-in hosting](plugin-hosting.md) › Events in a block,
  #109), and a macro can take the same path once it is built. Mapping a plug-in parameter needs
  the plug-in running (its parameter list comes from `get_params`); a mapping to a plug-in that is
  missing is kept and does nothing.

### Plug-ins and built-in devices inside a group

Nothing about a device changes when it moves into a group.

- **A branch is a point in the signal flow**, so a device that reads its detector signal from elsewhere
  ([sidechain](sidechain.md)) can read **a branch** — at the branch's input, after its devices, or after its
  fader — and a device *inside* a branch can be keyed from a track, from another branch or from the group's own
  track. A key across branches is a read edge like any other: the engine orders it, compensates its latency and
  refuses a cycle that closes through it.

- **Built-in devices** run in the engine as they do now (ADR 0004): made, prepared and given their
  parameters when the graph is built, real-time safe on the audio thread.
  A device in a branch is built as one on a track is (`group::Builder` and `Chain::build` give the
  same results, #766): a Sampler gets its decoded sample and its manual slices, a parameter that
  follows an automation lane starts where the lane starts and follows it while playing (each
  branch keeps the lanes of its devices, planned for every block with the chain's), and a device
  that holds a file is known by a hash of its state, so a changed file is a new device.
- **Plug-ins** still run out of process (ADR 0006, AGENTS.md rule 5). A group is not a process
  boundary and never runs a plug-in in the engine. A crashed or hung plug-in in a branch does what
  it does on a track: an effect passes its input, an instrument is silent, the branch and the rest
  of the group go on, and `device.restart` brings it back.
- **Plug-ins running at the same time** (ADR 0006 › 4): the branches of a group are independent
  until they are summed, so their plug-ins can be sent their blocks together, each branch going
  on as its own plug-in answers, exactly as different tracks' chains do (#151). The first version
  may run branches one after another inside the chain and only later run them side by side; the
  sound is the same, only the deadline is used better.
- **A plug-in's key** today is its channel, its plug-in and how many of the same plug-in come
  before it on the channel (`plugin_devices`), which keeps a running plug-in when devices are
  added or moved around it. With groups, "before it" counts depth-first through the whole tree of
  the channel's devices, so moving a plug-in into or out of a group keeps its process and state,
  as long as the order of identical plug-ins does not change.
- **State** is saved per device as today: a plug-in inside a group keeps its `state` in its own
  `Device`, read on save and every 30 s (ADR 0006 › 5).

### The device panel

The group shows in a track's device panel ([ADR 0009](../decisions/0009-ui-toolkit-and-look.md),
[components](../design/components.md)) as one box with:

- a header: the name, the on/off switch, a fold button, **Mix** and **Gain** knobs;
- a row of the eight macro knobs (folded away when no macro is mapped);
- a branch list on the left: one row per branch with its name, a volume slider, a pan knob, **M**
  and **S** buttons, and a **+** to add a branch;
- the devices of the selected branch to the right of the list, laid out as on a track.

A click on a device's panel picks it, inside a group too: Ctrl (Cmd) adds one or takes it out,
Shift takes the run from the one picked last in the same chain or branch, and **Ctrl+A** picks
every device of the chain or branch of the one clicked last. Copy, cut, duplicate, Delete, the
on/off key and grouping act on the picks, by their paths through the groups.
Selecting devices and pressing **Ctrl+G** (Cmd+G on macOS) groups them (those of one chain or
branch, next to each other); **Ctrl+Shift+G**
ungroups a group of one branch. The context menu of a device has **Group**, and of a group
**Ungroup**, **Add branch**, **Save as preset…**. Dragging a device between branches or into and
out of a group moves it. While a parameter is being mapped, right-clicking it offers **Map to
macro ▸ Macro 1 … 8**. The visual design is settled with the UI issue, against the design brief.

## Model

`Device` gains one kind (ADR 0001 › core: a channel's device chain). Chosen: a kind rather than a
field on every device, because a group never has `state` and a plug-in never has branches, so
the type rules out the combinations that mean nothing.

```rust
pub enum DeviceKind {
    // … as now …
    /// Devices of Suhina's own that hold other devices (docs/spec/device-groups.md).
    Group(Box<DeviceGroup>),
}

pub struct DeviceGroup {
    /// At least one.
    pub branches: Vec<Branch>,
    /// Always eight; a macro without mappings is still there (its name and value are kept).
    pub macros: Vec<Macro>,
}

pub struct Branch {
    pub name: String,
    /// Linear gain; 1.0 is unity.
    pub volume: f64,
    /// −1.0 hard left … 1.0 hard right.
    pub pan: f64,
    pub mute: bool,
    pub solo: bool,
    pub devices: Vec<Device>,
    pub extensions: Extensions,
}

pub struct Macro {
    pub name: String,
    pub mappings: Vec<MacroMapping>,
}

pub struct MacroMapping {
    /// The target device, as a path relative to the group: branch, index, then branch, index
    /// again for each nested group ([2, 0] is branch 2's first device).
    pub device: Vec<u32>,
    /// The parameter's stable id: a built-in parameter id (ADR 0004), a plug-in parameter id as
    /// text, or `macro.N` of a nested group.
    pub param: String,
    /// Positions on the parameter's 0..1 travel.
    pub min: f64,
    pub max: f64,
}
```

The group's own values are ordinary parameters in `Device::params` (ADR 0001, amendment of
2026-10-11), so `device.setParam`, the project file and later automation (LOOP-39) handle them
with no new mechanism:

| Parameter id | Range | Default | Unit |
|---|---|---|---|
| `mix` | 0 … 1 | 1 | fraction |
| `gain` | −inf … +12 | 0 | dB |
| `macro.1` … `macro.8` | 0 … 1 | 0 | fraction |

`Project::validate` gains checks, each its own `ValidationError` so a malformed file is an error,
never a crash: a group has 1 to 16 branches and exactly 8 macros; depth at most 4; at most 64
devices on a channel counting every depth; volumes finite and not negative, pans in −1 … 1; every
mapping's path resolves to a device in the group and its `min`/`max` are in 0 … 1; no parameter
mapped twice; an instrument group's branches start with an instrument (or are empty) and nothing
else in a group is an instrument.

Format extensions (`Extensions`) sit on the group's `Device` and on each `Branch`, so an importer
that maps another format's grouped devices keeps what the core does not model.

### Project file

The `.suhina` file ([`suhina-format.md`](suhina-format.md), ADR 0010) writes a group as a device
with:

```json
{
  "name": "Parallel crush",
  "kind": {
    "type": "group",
    "branches": [
      { "name": "Dry", "volume": 1.0, "pan": 0.0, "mute": false, "solo": false, "devices": [] },
      { "name": "Crushed", "volume": 0.5, "pan": 0.0, "mute": false, "solo": false,
        "devices": [ { "name": "Compressor", "kind": { "type": "builtin", "id": "compressor" }, "params": [] } ] }
    ],
    "macros": [
      { "name": "Crush", "mappings": [ { "device": [1, 0], "param": "threshold", "min": 0.8, "max": 0.2 } ] }
    ]
  },
  "enabled": true,
  "params": [ { "id": "mix", "value": 1.0 }, { "id": "macro.1", "value": 0.4 } ]
}
```

A reader of version 1 does not know `"type": "group"` and would refuse the file with a confusing
message, so groups came with version **2**, written then only when the project held a group. Since
version 3 every file is written as the newest version
([suhina-format](suhina-format.md) › Versions); the reader accepts 1 to 3. A device-kind `type` the reader does not know is refused with "this project uses
a device a newer Suhina knows", never ignored, as dropping a group would drop the devices in it.

### Presets

A device group can be saved as a **group preset**, a `.json` file in the format of ADR 0014 with
its own `format` name, holding the group's device exactly as the project file writes it:

```json
{ "format": "suhina-device-group", "version": 1, "name": "Parallel crush",
  "author": "", "tags": ["drums", "parallel"], "device": { "name": "Parallel crush", "kind": { "type": "group", … } } }
```

- Plug-ins in it keep their state (base64), so a preset brings a plug-in back as it was set.
  Device presets are at most 64 KiB (ADR 0014); a group preset with plug-in state can be larger, so
  group presets are capped at **16 MiB**, checked before reading.
- Loading one adds the group like a device (`device.add {preset}`); a plug-in that is not
  installed comes in as a missing plug-in with its state kept (ADR 0006 › 8), and the command's
  result lists the missing ones.
- The user's group presets are saved under `presets/groups/` in the settings folder and listed
  by the browser ([browser](browser.md)). Stock group presets (ADR 0014) go in
  `presets/groups/` in the repository, CC0, and may hold built-in devices only, so no plug-in or
  its state ships with Suhina.

## Commands

Every behaviour above is a command (AGENTS.md rule 6, ADR 0002); each edit is one undo step and a
playing engine hears it at once (ADR 0005).

**Addressing devices inside groups.** Today `device` is a device's index on the track. It becomes a
*device path*: an integer still means a top-level device, so every existing call keeps working;
a text path `"2/1/0"` means the device at index 2 on the track (a group), its branch 1, device 0
there, and so on for each level. A position for adding or moving is a path the same way: `"2/1/3"`
is branch 1 of the group at 2, before its device 3. `track.devices` lists a group with its
`branches`, each with its devices (and their paths), and its macros. Every `device.*` command
(`setEnabled`, `setParam`, `remove`, `plugin`, `openEditor`, `closeEditor`, `restart`) takes a path;
`device.add` and `device.addPlugin` take a path `position`.

| Command | Kind | Params | What it does |
|---|---|---|---|
| `device.group` | edit | `track`; `device` (path): the first device; `count` (integer, default 1): how many consecutive devices at that level; `name` (text, optional) | Puts the devices into a new group with one branch, in the place of the first. Its latency, sound and plug-ins are unchanged. Refused when it would break the instrument rules or the depth limit. |
| `device.ungroup` | edit | `track`; `device` (path): a group | Puts the devices of a group's one branch in its place. A group with several branches is refused, since its sum cannot be written as a serial chain; its macro mappings go (undo brings them back). |
| `device.addGroup` | edit | `track`; `position` (path, optional, default last); `name` (text, optional) | Adds an empty effect group with one empty branch (a passthrough). |
| `device.move` | edit | `track`; `device` (path); `to` (path): the position to move it before | Moves a device within a track: along the chain, between branches, into or out of a group. Macro mappings follow the device; a mapping whose device leaves its group is removed. |
| `device.duplicate` | edit | `track`; `device` (path) | Copies a device (a group with everything in it) next to itself. Plug-in copies start with the state the original has now. |
| `device.duplicateMany` | edit | `track`; `devices` (paths) | Copies several devices, in their order, after the last of them. Refused, changing nothing, when the chain's rules would not hold for the result (a second instrument, a MIDI effect after the instrument or an effect, an instrument or a MIDI effect on a track that plays no notes, a MIDI effect in a group). |
| `device.paste` | edit | `track`; `position` (path, optional, default last) | Puts the devices `device.copy` took into a chain. Refused, changing nothing, on the same rules as `device.duplicateMany`. |
| `branch.add` | edit | `track`; `device` (path): a group; `position` (integer, optional, default last); `name` (text, optional) | Adds an empty branch. |
| `branch.remove` | edit | `track`; `device`; `branch` (integer) | Removes a branch with its devices and their mappings. The last branch cannot be removed. |
| `branch.duplicate` | edit | `track`; `device`; `branch` | Copies a branch next to itself. |
| `branch.rename` | edit | `track`; `device`; `branch`; `name` (text) | Renames a branch. |
| `branch.setVolume` | edit | `track`; `device`; `branch`; `db` (number) | As `track.setVolume`, in decibels. |
| `branch.setPan` | edit | `track`; `device`; `branch`; `pan` (number, −1 … 1) | As `track.setPan`. |
| `branch.setMute` | edit | `track`; `device`; `branch`; `mute` (bool) | Mutes a branch; its devices keep running. |
| `branch.setSolo` | edit | `track`; `device`; `branch`; `solo` (bool) | Solos a branch within its group. |
| `macro.rename` | edit | `track`; `device` (group); `macro` (integer 1 … 8); `name` (text) | Renames a macro. |
| `macro.map` | edit | `track`; `device` (group); `macro`; `target` (path relative to the group); `param` (text); `min`, `max` (numbers 0 … 1, default 0 and 1) | Maps a parameter to a macro. Refused when the parameter is mapped already, or the device is not inside the group. |
| `macro.unmap` | edit | `track`; `device` (group); `macro`; `target`; `param` | Removes one mapping; the parameter keeps its current value. |
| `macro.list` | read | `track`; `device` (group) | Each macro's name, value and mappings, with the targets' names and current values. |
| `device.savePreset` | read | `track`; `device` (path); `name` (text); `tags` (list of text, optional); `path` (text, optional: default the user's presets folder) | Writes the device (a group, or later a single device, ADR 0014) as a preset file. Plug-ins' state is read from them first, as `project.save` does. It changes nothing in the project, so it is a `read` command like `project.save` and not an undo step. |

Macro values, the mix and the gain are set with `device.setParam` on the group (`macro.1`, `mix`,
`gain`); a group preset loads with `device.add {preset}`. [`docs/mcp.md`](../mcp.md) says how an agent
uses device paths when these commands land; the command list itself is generated.

## Engine

- A chain's slots (`Slot`, `crates/engine/src/chain.rs`) gain `Slot::Group`: its branches, each a
  list of slots with its own buffer of `MAX_BLOCK` frames, the dry buffer, the delay lines and the
  smoothed gains, all made when the graph is built. Running a group copies the input into each
  branch buffer, runs the branches, delays, scales and sums them; nothing allocates, locks or
  waits on the audio thread (AGENTS.md rule 3).
- Events are split at their frames as now (ADR 0004 › 5 and 8): an instrument group hands each
  event to every branch's instrument.
- A graph swap (ADR 0005) hands running devices to the new graph when the same devices sit at the
  same place; the identity a chain compares (`Chain::ids`) becomes the device's path plus its id,
  so editing one branch keeps the others' devices, tails and notes running.
- Offline renders run the same code (AGENTS.md rule 4); golden renders test a group (see the
  plan).

## Open questions

For the maintainer; each has the choice this spec made, so work can start without an answer.

1. **The name.** *Device group* with *branches* is chosen to avoid the group track's *group* and
   the engine's *chain*. Another name for the unit (for example *stack*) would avoid *group* in
   the interface entirely.
2. **Eight macros.** Fixed at eight. A variable count (up to 16) costs an add and remove command
   and more parameter ids; eight fit one row of the panel.
3. **One macro per parameter.** Chosen to keep one owner per value. Several macros adding up on
   one parameter is more flexible and needs a rule for how they combine.
4. **Linear mix.** Chosen for correlated dry and wet. An equal-power option could be a toggle
   parameter later.
5. **Key and velocity ranges per branch** for instrument groups: left out of the first version
   (plan, 9). Is layering without them enough to ship?
6. **Choosing a branch by a parameter** (a selector that plays one branch at a time and crossfades
   between them) is left out; it would be one more group parameter and a per-branch range.
7. **Project file version 2 only when a group is present**, so older Suhina opens projects without
   groups. Answered 2026-10-24: every file is written as the newest version
   ([ADR 0010](../decisions/0010-project-files.md) › Amendment: format versions).
8. **Group presets up to 16 MiB** with plug-in state inside. Alternatively group presets hold
   built-in devices and plug-in references only, with the plug-in state left out.
9. **Return and master channels** (ADR 0004, *Not decided yet*): groups follow wherever chains
   go; nothing here needs return chains first.
10. **Side-chain inputs into a branch** are decided in [sidechain](sidechain.md) › *Device groups
    (racks)*: a branch is a key source at its own three tap points (before its devices, after them,
    after its fader), and a device inside a branch can be keyed, because a key names a *point* in the
    signal flow rather than a channel. Nothing in this document's model changes; the read path is the
    sidechain spec's work, and it lands before groups do.
11. **A MIDI effect group.** This document has effect groups and instrument groups. Device sets also
    have a group that holds MIDI effects only (a MIDI effect rack), and the device SDK's `midi_effect`
    role is the first thing that would need one. It is a third kind with its own placement rules (MIDI
    effects only, on a track that plays notes); nothing here forecloses it, and it is not planned until
    MIDI effects exist.
12. **Importing other formats' grouped devices** onto device groups: done for Live's Audio
    Effect Rack and Instrument Rack ([`als-format.md`](als-format.md) › Audio Effect Rack and
    Instrument Rack), observed in the user's own sets; macro mappings wait for a set that shows
    where Live stores them, and an Instrument Rack's zones for key ranges (9).

## Implementation plan

Proposed issues, in order; each is a pull request with its tests (AGENTS.md rule 7). The first
four make groups usable from the CLI and MCP; the UI comes after, and macros and presets build on
it.

1. **Model and project file.** `DeviceKind::Group`, `DeviceGroup`, `Branch`, `Macro`,
   `MacroMapping`; `Project::validate` with every check above and a malformed-input test for
   each; `.suhina` version 2 (written then only with a group), round-trip tests; the depth-first plug-in
   key in `plugin_devices`. ADR 0001 gets an amendment. *Nothing audible yet: the engine passes a
   group through.*
2. **Engine: groups in the chain.** `Slot::Group` with branches, volume, pan, mute, solo, mix and
   gain; built-in devices only; golden renders (two branches, one muted, one soloed, mix at 0, 0.5,
   1), passthrough exactness for a group of one empty branch, block-size independence and the
   real-time rules. ADR 0004 gets a decision.
3. **Engine: latency compensation inside groups.** Per-branch delays and the dry delay; renders
   with the overdrive (31 frames) in one branch against a dry branch showing no comb filtering; a
   nested group; the 2 s cap.
4. **Commands.** Device paths in every `device.*` command, `track.devices` with branches,
   `device.group`, `ungroup`, `addGroup`, `move`, `duplicate` and the `branch.*` commands, with
   undo tests and `docs/mcp.md`.
5. **Plug-ins in groups.** Plug-in slots in branches, the key surviving moves into and out of a
   group, crash and restart inside a branch, tests with `test:clap` and `test:vst3`; then running
   a group's branches side by side (ADR 0006 › 4 amended).
6. **UI: the group in the device panel.** Header, branch list, the selected branch's devices,
   Ctrl+G and Ctrl+Shift+G, dragging between branches, context menus; design measurements in
   `docs/design/`.
7. **Macros.** `macro.*` commands, mapping and applying for built-in devices, the refusal on
   mapped parameters, the macro row in the panel and **Map to macro**; then plug-in parameters
   through `set_param`.
8. **Group presets.** `device.savePreset`, loading with `device.add {preset}`, missing plug-ins
   reported, the browser listing them; ADR 0014 amended for the group format.
9. **Key and velocity ranges for branches** of instrument groups (open question 5).

ROADMAP gets a line under *Built-in effects and instruments* when item 2 lands, and the parity
coverage (`scripts/parity/score.py`) changes then, not with this spec.
