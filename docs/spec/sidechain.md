---
description: Planned key inputs - a device whose detector reads another channel's signal (sidechain), the tap points it can read from, and the plug-in bus model (aux audio in and out) that carries the same idea across the sandbox boundary
read_when: sidechain, key input, key source, tap point, pre FX, post FX, post fader, routing, aux input, aux output, multi-out, multiple outputs, bus, audio port, region version, protocol version, VST2 bus, device input
type: spec
status: planned
last_verified: 2026-10-15
covers:
  - crates/model/src/lib.rs
  - crates/engine/src/chain.rs
  - crates/engine/src/buses.rs
  - crates/engine/src/plugin.rs
  - crates/device-api/src/lib.rs
  - crates/ipc/src/region.rs
  - crates/plugins/src/protocol.rs
  - crates/plugin-host/src/vst3.rs
  - crates/plugin-host/src/clap.rs
  - crates/session/src/devices.rs
  - devices/compressor/src/lib.rs
---

# Key inputs and plug-in buses

- **Sources:** Suhina's own design, from the routing model as it is built ([ADR 0001](../decisions/0001-daw-neutral-model.md),
  the device chain in [ADR 0004](../decisions/0004-built-in-devices.md) › 9 and 10, the plug-in boundary in
  [ADR 0006](../decisions/0006-plug-in-hosting.md)) and from the gap the parity measurement records
  ([`docs/research/parity-live-12.md`](../research/parity-live-12.md): chapter 17 lists *sidechain* and *multi-timbral*
  among what is missing). Nothing here is observed from another program; nothing is implemented yet.
- **Status:** planned. What this document had to choose, it says and why, so work can start without an answer;
  the choices the maintainer can still turn are in *Open questions*, and the split into issues is at the end.

## What it is

Two mechanisms that are the same primitive at both ends of the engine:

1. **A key input on a device.** A dynamics device (a compressor, a limiter, a gate, a vocoder) can read its
   detector signal from **another channel** instead of from its own input. That is what "sidechain" means in
   practice: the bass ducks under the kick, the pad follows the vocal, the mute-trigger track still keys the
   compressor while it is not heard.
2. **Plug-in buses.** The same idea across the sandbox boundary: a plug-in that declares an auxiliary audio
   input (a sidechain port, a key bus) is fed from a chosen channel, and a plug-in that declares auxiliary
   outputs (a multi-out instrument) publishes them so that parts can be processed on their own tracks.

Both are expressed with one model: a device names a **source** (a channel and a **tap point** in that channel's
signal) for each **port** it has. Nothing in the model knows whether the port ends in a built-in device or in a
plug-in bus, and nothing names a plug-in format: what a VST3 auxiliary input bus, a CLAP sidechain port and a
VST2 third input pair have in common is what the model stores.

## Words

- **Key** is the signal a detector reads: what a device listens to without processing it. In the interface the
  control is called **Sidechain**, the word people look for; documents and code say *key*.
- **Tap point** is where in the source channel's signal the key is taken: `PreFx`, `PostFx` or `PostFader`
  (below). A send has the same idea with two points (`Send::pre_fader`), and this document generalises it.
- **Port** is one audio connection of a device: a direction (in or out), an index, a channel count, an optional
  name and a role (`Main`, `Aux`, `Key`). A built-in effect has one input port and one output port; a VST3
  plug-in may declare more; a VST2 plug-in's extra pairs are ports without a name or a role.
- **Bus** in this document means a group track's summing channel ([ADR 0004](../decisions/0004-built-in-devices.md) › 10),
  not a plug-in bus. A *plug-in bus* is always written out in full.

## Behaviour

### A device's key

- A device with a key runs its **detector** over the key and its **audio** over its own input. The dry path,
  the gain reduction, the smoothing and the latency are the device's own business; only the detector's source
  changes.
- **No key is the state every device is in today**: the detector reads the device's own input, exactly as
  `devices/compressor` does now (its `sidechainHighPass` filter runs on the detector signal either way).
- The key is **read, never summed**: it adds nothing to the reader's channel and never reaches the output except
  through what the device does with it.
- A switched-off device (`enabled: false`) has no key at all: it does not run.
- The key is **only read**: a device cannot write it, and a device that ignores a key (an effect without a
  detector) is unaffected by having one.

### Tap points

The key can be taken at three points of the source's signal: a channel's, or a device group's branch (below).
They are the same three points for built-in keys and for plug-in aux inputs.

| Tap | What it contains |
|---|---|
| `PreFx` | What plays on the source track, before its device chain and before its fader. Changes to the source's devices and its mix have no effect on the key. |
| `PostFx` (**default**) | The source's device chain output, before its fader: what the source's devices produce. This is what a keyed compressor almost always wants. |
| `PostFader` | The source's output as it leaves its channel: its fader, its mute and its pan applied. |

Consequences that follow from the table and are worth saying out loud:

- **A muted source still keys a `PreFx` or `PostFx` key.** A muted trigger track is a normal way to work: the
  track is not heard but its signal drives the detector. A `PostFader` key of a muted source is silence, and the
  device falls back to its own input, as it does without a key.
- **Solo does not silence a key.** While some channel is soloed, a key still reads its source: a solo elsewhere
  must not make a device stop working. (The source's own mute, at `PostFader`, is the exception above.)
- A source with devices that add latency (`PostFx`, `PostFader`) carries that latency in the key; a `PreFx` key
  carries none. The alignment rule below makes this invisible to the user.

### Latency

A key adds no latency to the device that reads it and does not change what that device reports: the reader is
heard exactly as it was.

The **key must line up with the source as the source is heard**. The engine already computes, for every chain,
how late its output arrives at its target and how much each path is delayed so that all paths line up
(`arrive`, `input`, `Chain::delay` in `crates/engine/src/buses.rs`). A key edge takes part in the same pass: the
key buffer gets its own delay line, sized so the source's contribution to the output and the key reach the reader
at the same block-relative time (at least zero). For the common case of two plain tracks this is the difference
of the two chains' own device latency.

Chosen: **the key is aligned in the first version**, not documented as an offset. An unaligned key is a defect
that golden renders would freeze in place, and the delay lines the alignment needs already exist (allocated with
the graph, capped by `MAX_LATENCY_SECONDS`, off the audio thread).

### Ordering and cycles

- A key edge is a **read dependency**: the source's signal at the tap point must exist before the reader runs.
  The order the engine builds chains in today (`sort_by_key(Reverse(buses_after(track).len()))`,
  `crates/engine/src/buses.rs`) covers summing only. It becomes a **topological sort over both kinds of edge**
  (sum and key) with the existing depth-first rule as its tie-break, so equal depths keep the project's order.
- No block of delay is ever used to break a dependency: a render must not depend on how the host divides a
  stream into blocks (`Device::process`, ADR 0004).
- **A cycle is refused when it is made.** `device.setKey` refuses a key edge that would close a loop through
  summing and key edges together, as `track.setOutput` refuses a routing loop today, and says which edge closes
  it.
- **A malformed project never hangs**: an engine that meets a cycle in a file follows the existing rule — the
  chain runs, the key buffer reads silence, and the device falls back to its own input — and the graph stays
  finite (`MAX_HOPS`, the bounded walks in the model's validation). `Project::validate` reports it as its own
  error, so the file is refused rather than played wrongly.
- A keyed source that is a **bus** works the same way: the topological order puts the bus before the reader, so a
  device on a group can be keyed by a track inside it and a track can be keyed by a group.

### A source needs a chain

Today a chain exists for a track whose channel holds devices Suhina can run, or as a pass-through chain for a
track that plays into a bus. A key source that has neither would have no buffer to read.

Chosen: **a channel that is a key source gets a pass-through chain**, exactly as a track that plays into a bus
gets one today. The rule lives in the same place (`crates/engine/src/buses.rs`) and costs one buffer per keyed
source, allocated with the graph.

### Device groups (racks)

A track's chain can hold a **device group** ([device-groups](device-groups.md)): several parallel branches, each
a serial chain of devices, summed into the group's output. A branch is a point in the signal flow, and a key can
read from it — the same three taps apply, relative to the branch:

| Tap of a branch | What it contains |
|---|---|
| `PreFx` | The branch's input, the signal the group hands every branch. |
| `PostFx` (**default**) | The branch's own devices' output, before its volume and pan. |
| `PostFader` | The branch's output after its volume, pan, mute and solo, before it is summed into the group's mix. |

- A **branch is a source** and a device **inside** a branch can be keyed: from another track, from another
  branch, or from the branch's own group (which is a cycle and refused).
- A key read from a branch carries that branch's own devices' latency, and the group's latency from there to its
  track; the alignment rule above uses both, unchanged.
- Order: a key edge orders the branch's chain before its reader, exactly as it orders chains. A device group's
  branches run before the group's mix, so a key into a branch's `PostFader` point is available only after the
  branch's own gains; anything earlier may be read by another branch in the same block. A cycle through branches
  and chains is refused when it is made and reads silence in a malformed project.
- A key into a device inside a branch is addressed by that device's **path** (`"2/1/0"`), the same path every
  `device.*` command takes once device groups land (device-groups › Commands).
- The device group's own features that are planned but not built yet (key and velocity ranges per branch, a
  parameter that selects branches, a branch's own presets) do not change any of this: the key names a *point*,
  and what selects or filters that point is the group's business.

## Model

One field on `Device` (ADR 0001 › core: a channel's device chain). Chosen: a list from the first version, even
though the first commands allow one entry, because a plug-in's auxiliary inputs are the same primitive and the
project file's version should move once.

```rust
pub struct Device {
    pub name: String,
    pub kind: DeviceKind,
    pub enabled: bool,
    pub state: Option<Vec<u8>>,
    pub params: Vec<DeviceParam>,
    /// The device's ports that read another signal (docs/spec/sidechain.md). Empty for almost every
    /// device. At most one entry in the first version.
    pub inputs: Vec<DeviceInput>,
    pub extensions: Extensions,
}

pub struct DeviceInput {
    /// Which port of the device reads it: `"key"` for a built-in detector, or the plug-in's own name
    /// for an auxiliary bus (`"Sidechain"`, `"Aux 1"`, `"2"` for a VST2 pair without a name).
    pub port: String,
    pub source: SignalPoint,
}

/// Where a key is read: a channel, or a branch of a device group inside a track's chain
/// (docs/spec/device-groups.md), each at one of the tap points.
pub enum SignalPoint {
    Channel {
        channel: ChannelId,
        tap: TapPoint,
    },
    Branch {
        track: TrackId,
        /// The device group, as a device path (`"2"`, or `"2/1/0"` for a group that is itself
        /// inside a group), the same path the `device.*` commands take.
        device: Vec<u32>,
        /// The branch within that group, from 0.
        branch: u32,
        tap: TapPoint,
    },
}

pub enum TapPoint {
    /// Before the point's devices.
    PreFx,
    /// After its devices, before its fader. The default.
    PostFx,
    /// After its fader, mute and pan.
    PostFader,
}
```

A `Channel` point names a **channel**, not a track: what a key reads is a signal, and a track without a channel
has none (`Track::channel: Option<ChannelId>`). A `Branch` point names the track and the group's path, because a
branch has no channel of its own.

`Project::validate` gains checks, each its own `ValidationError`:

- a device has at most one input in this version;
- a `Channel` point's channel exists, and a `Branch` point's track, device path and branch exist and the device
  at that path is a device group;
- no cycle through summing and key edges together;
- a plug-in device's `port` is empty when the plug-in is not installed or declares no such bus (the entry is
  kept, as a missing plug-in's state is, and does nothing until it is found);
- a built-in device's `port` is `"key"` or empty, and a built-in device that declares no key is refused at the
  command, not in the file (an unknown device id is a missing device already).

### Project file

`.suhina` ([`suhina-format.md`](suhina-format.md), ADR 0010) writes the field only when it is not empty, and the
file version is the one after the version device groups take (2 today, `crates/project-file/src/lib.rs`), so a
project without a key still opens in a Suhina from before this:

```json
{ "name": "Compressor", "kind": { "type": "builtin", "id": "compressor" },
  "inputs": [ { "port": "key", "source": { "channel": 5, "tap": "postFx" } } ],
  "params": [ { "id": "threshold", "value": -18.0 } ] }
```

A key read from a device group's branch carries the point instead of the channel:

```json
{ "port": "key", "source": { "branch": { "track": 3, "device": [2], "branch": 1, "tap": "postFx" } } }
```

The reader accepts both versions and refuses an unknown `tap` or an unknown point with a message that names it,
never silently.

## Commands

Every behaviour is a command (AGENTS.md rule 6, ADR 0002); each edit is one undo step and a playing engine hears
it at once (ADR 0005). `device` addresses the device as it does today (an index on the track; a path once device
groups land).

| Command | Kind | Params | What it does |
|---|---|---|---|
| `device.setKey` | edit | `track`; `device`; `source` (a track or channel id, `"none"`, or a branch: `device` (a device path) with `branch`); `tap` (text, optional, default `postFx`); `port` (text, optional, default `"key"`) | Makes the device read the point's signal. Refused when the point is inside the device's own chain (that is what no key means), when the point does not exist, when the port is not one the device has, when the device has a key already, and when the edge would close a loop (the message names the loop). One undo step. |
| `device.clearKey` | edit | `track`; `device`; `port` (text, optional, default `"key"`) | Removes the key. One undo step. |
| `track.devices` | read | `track` | Already lists each device; a device with a key gains `key: { source: <channel or branch>, sourceName, tap, port }` so the panel and an agent can show it without a second call. |

The command is the only way a key is made: the UI, the CLI and MCP all dispatch it. `device.add` never adds one,
and a device that is duplicated (`device.duplicate`) or moved keeps its key, since it is a field of the device.
Removing a channel (`track.delete`) removes every key that reads it, in the same edit as the other references it
already cleans up.

## Engine

- A chain that is **read** as a key gets a **key buffer** of `MAX_BLOCK` frames (interleaved stereo), made with
  the graph; a chain that reads keys holds the **reader's views** of those buffers (indices, resolved when the
  graph is built). Nothing is allocated on the audio thread.
- The source chain fills the key buffer at its tap point, in the pass that already has that signal in hand:
  `PreFx` right after the block's sources are summed (`Chain::begin`/`add`), `PostFx` after its devices have run
  and before its compensation delay and post gains, `PostFader` after those gains. `Chain::finish` is where the
  last two live today (`crates/engine/src/chain.rs`).
- The key buffer passes the **alignment delay** (above) before the reader sees it.
- A chain runs its devices as it does now; a slot with a key is handed `(buffer, key)` instead of `buffer`
  (`Slot::Device` and `Slot::Plugin` both).
- Order: chains are sorted topologically over sum and key edges (above). A key edge is not a summing target: it
  does not make its source a bus and does not change any gain.
- Offline renders run this code unchanged (AGENTS.md rule 4), so the golden renders are the oracle: a keyed
  compressor's gain reduction, the alignment, block-size independence (1, 333, 512, 4096 frames) and a cycle in a
  hand-written project each get a render test in `crates/engine/src/chain_tests.rs` and `buses_tests.rs`.

## device-api

`Device::process` gains the key. Chosen: one **IO value** rather than an extra parameter per future port, so
adding a plug-in bus or a second key later does not change the trait again:

```rust
/// The audio a device reads this block: its own input, and the key ports it declared, in the order of
/// its own `DeviceInfo` (docs/spec/sidechain.md).
pub struct Inputs<'a> {
    pub main: &'a [f32],
    /// One slice per key port; empty for a device without keys.
    pub keys: &'a [&'a [f32]],
}

fn process(&mut self, context: &ProcessContext, inputs: Inputs<'_>, buffer: &mut [f32]);
```

- The key is **read-only** (`&[f32]`): a device cannot leak it into its output by accident.
- A port the engine has no source for is handed **silence**, never left unset, so a device needs one code path.
- `DeviceInfo` gains the ports it declares (`key_ports: &'static [&'static str]`, empty for almost every device),
  so the model, the command and the panel agree on what a device can read.
- Every built-in device is updated mechanically; `devices/compressor` is the first that uses it,
  and `limiter`, `overdrive` and the planned `bus-compressor` (issue #385) follow.
- `crates/device-api/src/conformance.rs` gains the checks that a device with a declared key port reads it (a
  loud key must change its output) and that a device without one is bit-identical to today.

## Plug-in buses

The same primitive across the boundary. A plug-in's ports are **not** in the model as device inputs twice: a
plug-in's auxiliary input bus is one `DeviceInput` with the bus's own name as `port`, and its auxiliary outputs
are routing sources for other channels (its own feature, below).

### What a host reports

A plug-in declares its audio ports when it is loaded; the host reports them with the rest of what the session
knows about an instance (latency, instrument or effect, parameters, editor):

| Field | Meaning |
|---|---|
| `direction` | `in` or `out` |
| `index` | The bus's or port's own index |
| `channels` | 1, 2 or more |
| `name` | The plug-in's own name for it, when it has one |
| `role` | `main`, `aux` or `key` — `key` only where the format says so (a CLAP sidechain port, a VST3 `kSidechain` bus) |

- **VST3** (`crates/plugin-host/src/vst3.rs`): `getBusInfo` is not called at all today, only bus 0 is activated,
  and `process` fills the first input bus and reads back the first output bus. The work is: read every audio
  bus's info and arrangement, activate main **and** the auxiliary buses, copy each auxiliary input's frames from
  the link, publish each auxiliary output's frames to the link, and set `silenceFlags` per bus.
- **CLAP** (`crates/plugin-host/src/clap.rs`): today only the port *count* is read, one input and one output are
  chosen and the index, `IS_MAIN`, port type and name are ignored. The work is to enumerate ports, take the main
  one by `IS_MAIN`, treat other inputs as keys (sidechain ports by their type) and publish extra outputs.
- **VST2** (no hosting code exists yet; ADR 0006 › 8 and *Not decided yet*): `numInputs`/`numOutputs` and
  `setSpeakerArrangement` give channel counts and layouts only — never a name and never a typed sidechain, and
  there is no per-bus activation and no per-bus silence flag. So the port model keeps `name` and `role` optional,
  a VST2 pair is a port named after its index, and the user decides which one is the key. This is the reason the
  model stores ports generically instead of a "sidechain" field.

### The boundary

- **Shared memory** (`crates/ipc/src/region.rs`): one region already carries an interleaved buffer of up to
  `MAX_CHANNELS = 64` channels and a 256-byte header, and `Link::create(4, …)` is legal today. So a second stereo
  pair needs **no new region**: the header's unused bytes (16–64) take a small per-pair table (direction, pair
  index, live, bus index), `CHANNELS` stays the total, and the **region version rises by one**. Both sides refuse a
  region made by another version (`Region::open`), so an old host and a new engine fail loudly at start instead
  of playing silence. One region per pair is refused: it costs another doorbell and another round trip per block,
  which the deadline budget (ADR 0006 › 4) cannot pay.
- **The control protocol** (`crates/plugins/src/protocol.rs`): `Loaded` gains the port list, `Launch.channels`
  becomes the total the region carries, and the **protocol version rises by one**. A host that does not know the
  version is refused as it is today (the engine compares the hello answer).
- **The engine's side** (`crates/engine/src/plugin.rs`): `PluginProcessor::process`/`send`/`wait` take the extra
  pairs as pre-resolved views (`&[&[f32]]` in, `&mut [&mut [f32]]` out), never allocating on the audio thread;
  `PluginSlotInfo` grows the port counts; the chain hands the key span it already holds to the slot.
- **Offline runs the same path**: `crates/session/src/rack.rs` and its offline counterpart build the same
  buffers, so a render and playback carry the same audio (AGENTS.md rule 4).
- **The test plug-ins grow a bus**: `test:vst3` and `test:clap` gain an auxiliary input (and later an output), so
  CI proves the whole path — model, engine, region, protocol, host — without any proprietary plug-in.

### Auxiliary outputs

A plug-in's auxiliary outputs are the other direction of the same work and are **not** in the first version. When
they land, an output port is a **routing source** like a channel: `track.setInput`-shaped commands choose it, the
model stores it on the receiving channel (a `SignalPoint` that names a device and its port instead of a channel
and a tap), and the engine reads the port's buffer instead of a channel's. The port model above is what makes
that possible later without moving the model again.

## Data-only graph devices (`.sd`)

Community devices are **data**: a validated graph in the `.sd` format whose JSON a project embeds in
`Device::state` (`DeviceKind::Sd`). They are not a second kind of key — they are the same field, the same
command and the same panel row, and the rules above apply to them unchanged. Three things are needed for that to
be true, and they belong in the `.sd` work rather than after it:

1. **A graph declares its audio inputs.** The format and the SDK must carry the device's input ports: a graph
   with a key input says so (`inputs: [{ "port": "key", "name": "Sidechain" }]`, or the same information as a
   port name on the input node), and its validation allows exactly **one main audio input plus zero or more
   declared key inputs**, each with a different port name. A second unnamed `audio_input` stays an error.
   Today's rule ("exactly one audio input and output are required") and the input node's own documentation
   ("stereo input of the track") say the device has one input, so a key cannot be expressed at all.
2. **The graph runs its input nodes from their own buffers.** The runtime binds each declared input node to the
   buffer the engine hands it; a node with no source reads silence. Nothing else in the graph changes.
3. **A graph device reports what a device reports.** The prepared graph is wrapped in a type that implements
   `suhina_device_api::Device`: its `info()` from the graph's parameters and role, its ports from the graph's
   declarations, its `latency()` from the graph's own delay nodes, and its `process` taking `Inputs` like every
   other device.

The last point is a *change to how the engine runs a graph device today*: a graph device is a separate slot kind
that the engine drives through the SDK's own processing entry point, so it does not have the trait, the contract
tests, the latency reporting or the device registry that every built-in device has. Without the wrapper the key
input would have to be implemented twice — once for built-in devices and once for graphs — and a third time for
plug-ins; with it, a key is one path for all three, and the graph's delay nodes are latency-compensated like any
other device's.

## Import and export

A key that a project carries is written to `.suhina` as above. Reading keys out of another format is a
**separate observation job**, done the way every other part of the `.als` importer is done
([`als-corpus.md`](als-corpus.md)): author sets in our own copy of the program, one change per file, diff
neighbours, and write down what was seen in [`als-format.md`](als-format.md) before any code changes.

What the observation has to settle, because our model and the format may disagree about **which side of the
edge stores the key**:

- A key can be stored on the **producer** side: the source track's output routing names the destination track
  and, within it, the device input it feeds. The importer then has to invert the relation — read the source's
  routing, resolve the destination device and its port, and write `DeviceInput { port, source }` on the
  destination device, which is where our model keeps it.
- Or on the **consumer** side: the device itself names its source channel (and possibly a tap point). The
  importer then writes the same field directly.
- The observation also has to say what happens for the cases that matter: a key from a group track, from a
  return track, a key whose source was deleted (kept as a dangling reference, or dropped), a tap point named in
  the file (mapped onto `PreFx`/`PostFx`/`PostFader`, with our default where the file names none), a device
  inside a device group or rack whose own routing points exist, and whether a plug-in's auxiliary input bus is
  named at all.
- Unknown or unmappable pieces follow the importer's existing rule: keep what can be kept, and add a warning
  that names the device and the source, never a silent drop.

Chosen: a key whose **source** cannot be resolved (an external input, a track that was dropped, a plug-in bus we
do not host) is kept as an entry with no channel — the device still plays, the panel shows *missing*, and the
warning says so — while a key on a device **we do not map at all** (a device kept as a foreign placeholder) is
kept on the placeholder, so re-importing or mapping that device later does not lose the routing. Nothing is
guessed from the format's name or from another product's documentation: the entries are written from files we
authored.

A key whose source is a plug-in output (auxiliary outputs, above) is written the same way once that exists;
until then the file never contains one.

## The interface

In the device panel ([ADR 0009](../decisions/0009-ui-toolkit-and-look.md), [components](../design/components.md)),
a device that declares a key port shows a **Sidechain** row: a source chooser and a small tap chooser
(`Pre FX`, `Post FX`, `Post Fader`). The source chooser lists what can be read — the tracks that have a channel,
by name, plus *None* — and, under a track that holds a device group, the branches of that group at any depth
("Drums ▸ Parallel ▸ Post FX"), because a branch is a point a key can read like a channel is. A key whose source
is gone, whose branch no longer exists, or whose plug-in is missing shows *missing* in the row, keeps the
reference and does nothing. Everything the row does is `device.setKey` / `device.clearKey`; the panel changes
nothing itself.

## Parity

Chapter 17 (Routing and I/O) counts sidechain and multi-timbral as missing today, at 10 %; chapter 18 (Mixing)
does not mention a key at all. When the built-in key lands, `scripts/parity/score.py` says so and
`python scripts/parity/update_report.py` rewrites the marked parts of
[`docs/research/parity-live-12.md`](../research/parity-live-12.md); the plug-in buses and auxiliary outputs move
the same numbers in their own pull requests. ROADMAP's *What works now* gains a bullet when a user can hear a
keyed compressor, and the devices table says which devices read a key.

## Open questions

For the maintainer; each has the choice this document made, so work can start without an answer.

1. **The word.** *Key* in the model and *Sidechain* in the interface is chosen (the second is what people search
   for, the first is what the primitive is). Always *sidechain* everywhere would be simpler and slightly wrong for
   a VST2 auxiliary pair or a vocoder's carrier input.
2. **One key per device.** Chosen for the first version, since `Device::inputs` allows more later and one key
   covers the dynamics devices. A second key is a validation and command change only.
3. **Default tap `PostFx`.** Chosen because a key should follow the source's devices (that is what makes a
   processed trigger track useful) while a fader change on the source does not silently move the detector's
   threshold. `PreFx` as the default would ignore the source's devices.
4. **Alignment in the first version.** Chosen (above) rather than documenting an offset. The alternative saves one
   delay line per key and costs a permanent, subtle timing difference between what is heard and what keys it.
5. **A key of a muted source.** Chosen: `PreFx` and `PostFx` keys still work (the muted-trigger workflow), a
   `PostFader` key is silence. The alternative (mute kills every key) is simpler to explain and removes a way of
   working that people use.
6. **Latency of the key edge.** Chosen: a key adds no latency to the reader and none to the project. The
   alternative — treating the key as a path that must be compensated on its own — would make a device's reported
   latency depend on what keys it, which the model cannot express and automation could not follow.
7. **Auxiliary outputs in the same version as auxiliary inputs.** Chosen: inputs first (sidechain is the need),
   outputs next, because outputs change what a channel's *input* means and need their own commands, import rules
   and UI.
8. **Branch points (a device group's chain) in the first version.** Chosen: **yes in the model**, because it is
   the same primitive and the same file version (a branch is a point, and adding a second point kind later would
   move the file version again), and the **panel lists them** because that is how a keyed parallel chain is
   found. Building a key *from* a branch works as soon as the engine's read path does; a key *into* a branch
   waits for device groups (ADR 0016's tier 2), which is where a branch exists at all.

## Implementation plan

Proposed issues, in order; each is a pull request with its tests (AGENTS.md rule 7), its *How to check it* when a
user can notice it, and its documentation. The first five make a built-in key audible end to end and are the
first slice; the boundary work follows and is independent of it.

1. **Model and project file.** `DeviceInput`, `SignalPoint` (a channel or a device group's branch), `TapPoint`,
   `Device.inputs`; the validation rules above with a malformed-input test for each; the file version written only
   when a key is present, round-trip tests; `Project::validate` cycle and missing-point errors. ADR 0001 gets an
   amendment. *Nothing audible yet.*
2. **device-api: `Inputs`.** `process` takes `Inputs`, `DeviceInfo.key_ports`, every built-in device updated, the
   conformance checks for a declared and an undeclared key. ADR 0004 › *Not decided yet* loses the side-chain
   line and gets a decision. *Still nothing audible: no source exists yet.*
3. **Engine: the read path.** Pass-through chains for key sources, the key buffers, the three tap points at a
   channel and at a branch, the alignment delay, the topological sort over sum and key edges, cycles as silence;
   render tests (keyed gain reduction, alignment, block sizes 1/333/512/4096, a cycled project file, a keyed bus,
   a key from a branch once device groups run). ADR 0004 › 10 mentions the read edge; ADR 0006 is untouched.
4. **Compressor reads a key.** `devices/compressor` runs its detector over the key when it has one; contract,
   behaviour and golden tests; `docs/spec/` for the device if it does not have one. The same change later for
   `limiter`, `overdrive` and #385's `bus-compressor` (comment there so the work is not done twice: the device's
   own plan has only the side-chain high-pass).
5. **`device.setKey` / `device.clearKey`,** `track.devices` with the key, loop refusal, undo, malformed
   parameters; session command tests; `docs/mcp.md` and the components spec for the panel row.
6. **The interface:** the Sidechain row in the device panel (source dropdown, tap chooser), UI tests that pick a
   source and that a missing source shows *missing*.
7. **The boundary: one auxiliary input.** The next region version, with the pair table in its header, `Link::create(4, …)`,
   the next protocol version with the port list on `Loaded`, `PluginProcessor` views, `PluginSlotInfo` and `PluginReport`, the
   VST3 and CLAP hosts feeding the first auxiliary input, `test:vst3`/`test:clap` gaining one, an `ipc` test that
   refuses an old region. ADR 0006 › 3 and 7 amended (region and protocol versions, "silent buffers for any other
   bus" replaced), `docs/architecture.md` updated.
8. **Auxiliary outputs**, then a channel's input can name a plug-in port (`track.setInput` and the same panel
   row for inputs). ADR 0006 › 7 amended; the parity chapter 17 line on multi-timbral changes here.
9. **A key in a data-only graph device (`.sd`)**, in the device SDK's own pull request: the format and the SDK
   declare a graph's input ports, validation allows one main input plus declared key inputs, the runtime binds
   each input node to its buffer, and the prepared graph is wrapped in a `suhina_device_api::Device` so a graph
   device is a device like any other (its ports, its latency, its contract tests). The `examples/compressor.rs`
   of the SDK gains a keyed version, which is the end-to-end proof.
10. **Reading another program's keys from `.als`**, in three steps and in this order: author the sets
    ([`als-corpus.md`](als-corpus.md) › the sidechain series), diff them and write what was seen into
    [`als-format.md`](als-format.md), then read it in `crates/als` with the mapping rules above, a corpus test
    and a warning for every entry that could not be resolved.
11. **VST2 hosting** stays where it is (ADR 0006 › 8, *Not decided yet*): its own clean-room description of the
    ABI, its own crate, decided when we know which plug-ins people still use only as VST2. Nothing in this
    document changes when it happens — that is the point of the port model.
