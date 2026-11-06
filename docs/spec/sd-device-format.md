---
description: Version 6 of the data-only Suhina device graph format, SDK, track loading, and device revision migrations
read_when: .sd, Suhina device, device graph, device SDK, community effect, visual node, device UI
type: spec
status: current
last_verified: 2026-10-25
covers:
  - crates/device-sdk/src/builtin.rs
  - crates/device-sdk/Cargo.toml
  - crates/device-sdk/src/lib.rs
  - crates/device-sdk/src/editor.rs
  - crates/device-sdk/examples/wire_layout.rs
  - crates/device-sdk/src/graph.rs
  - crates/device-sdk/src/midi.rs
  - crates/device-sdk/src/runtime.rs
  - crates/device-sdk/src/native_node.rs
  - crates/device-sdk/src/modulation.rs
  - crates/device-sdk/examples/gain.rs
  - crates/model/src/lib.rs
  - crates/project-file/src/lib.rs
  - crates/session/src/devices.rs
  - crates/engine/src/chain.rs
  - crates/engine/src/chain_tests.rs
  - crates/session/src/devices.rs
  - crates/ui/src/detail/detail_view.rs
  - crates/media/src/info.rs
---

# Suhina device graph (`.sd`)

The Rust `author::GraphBuilder` convenience API writes this same format through `DeviceGraph`.
Its node and port handles exist only while authoring, are never serialized, and cannot bypass
validation. Parameter/UI constructors likewise configure the existing fields. Using these
helpers alone changes neither the file schema version nor the device revision. See the
[authoring guide](../device-sdk.md#checked-rust-authoring).

An `.sd` file describes a device as data. It contains no Rust source, executable code or embedded audio assets. Stable sample binding keys
refer to immutable resources prepared by the host. The Rust SDK builds and validates the graph, writes the JSON file and can prepare the
supported graph for audio processing.

Native stereo source and effect graph nodes implement a device-neutral `NativeNode` lifecycle: preparation and
resource binding happen off the audio thread; span preparation, rendering, note events and
reset are allocation-free. A separate factory maps graph metadata to native implementations.
The interpreter stores trait objects and never selects concrete instrument implementations.
Sampler resource access is an adapter over the generic native-node binding boundary.

A `native_effect` node names a trusted host kernel by `effect`. Its fixed ports are `in`,
`control1`, `control2`, `control3` and `control4`, each a stereo signal with a zero default.
`out` carries stereo; `left` and `right` select and duplicate one channel. Every input buffer
has the same interleaved stereo length as the output. Kernels must overwrite their output;
sources ignore inputs. `compile_with_native_at` resolves fresh, sample-rate-prepared kernels
off-thread; ordinary compilation rejects unresolved kernels. File data never loads code.
Legacy `native_device` wrappers are rejected, as on main.

## File and device versions

The top-level object contains `format: "suhina-device"`, `version`, a `device` object,
`nodes`, `connections` and optional `editor` metadata. The reader accepts file versions 1 through
6, and a writer always writes the newest (*Versions* below); a version 4 or older file with
`device.expanded`, or a version 5 or older file with a panel `style`, is rejected. Older readers
reject a version they do not know, preventing
silent loss of authoring metadata when saving. File version describes
the JSON schema; `device.version` describes revisions of a device's identity and parameter
contract. Files without `device.version` are revision 1. Device `id` remains stable across
revisions. The device also has a name, role and parameters (`id`, `name`, `min`, `max`, `default`,
`control`, `unit`). Controls include `slider` (default), `log_slider` (positive range), `integer`
(integer range/default) and `toggle` (range 0..1, integer default). Unit is display text, at most
128 bytes, default empty. Parameter commands and the processor apply the same clamping/rounding.

Parameter ids are saved identifiers. Keep an id when its meaning and unit stay the same. A device
revision above 1 includes a `migrations` entry from the immediately previous revision, even when
no ids changed. Each entry may rename parameter ids with `parameter_ids` (`from`, `to`); mappings
must preserve meaning and value units. Unchanged ids retain their clamped values, renamed ids
retain values through the mapping, new parameters receive their defaults, and removed parameters
are discarded. A migration from a still older revision must be listed directly. The format does
not define numeric unit conversions or arbitrary code migrations.

Each node has `id`, `kind`, kind-specific configuration and `visible` (default true).
Hidden nodes still process; the UI shows visible nodes in dependency order. It is a read-only
list of nodes, not an editor of the connections. Sequential arrows appear only for linear graphs.

Connections have `from`, `to`, `from_port` (default `out`) and `to_port` (default `in`).
Ports carry stereo sample-rate signals; source ports `left` and `right` broadcast one channel.
Constants and parameters broadcast control values. Each input accepts at most one connection;
branching is allowed, summing requires add/mix. `effect` graphs require one audio input and
output. `instrument` graphs have no audio input, require one audio output, and use timestamped
MIDI events as control inputs. `midi_effect` graphs use one `midi_input` and one `midi_output` in
a single chain; `midi_transpose` changes note-on/off keys and drops keys outside 0..127, preserving
each event's sample offset.
Note-off events retain the output key of their note-on even when transpose automation changes
while a note is held. A repeated note-on releases its previous mapped key first; a zero-velocity
note-on releases the held mapping. Processing needs room for up to two output events per input;
with room for one only, a repeated note-on sends the release and is dropped, so no note is left
sounding. A note-on discarded outside the MIDI key range has no matching output note-off, and a
second note-off for a released key is dropped. A note-off for a key the effect has not seen pressed
since it was made (the chain was rebuilt while the key was down) is transposed by the current
amount, so the note that started earlier still ends.
Exactly one role-appropriate input/output is required, and every node must contribute to the output.
Missing references, duplicate ids, invalid ports,
missing required inputs and cycles are rejected with node/port diagnostics. Only the `in` port
of sample_delay/delay breaks a cycle; its input is written after all current-frame evaluations.
Graphs execute node by node in chunks of at most 64 stereo frames, using
node buffers and parameter timelines allocated during compilation. Native sampler sources render
one chunk per call. Input, output, constants, parameters, gain, addition and multiplication
select their scalar kernel once per node chunk; other scalar nodes share the frame evaluator. Transport beats and parameter smoothing still advance for every frame;
timestamped MIDI splits processing before applying each event. Compilation finds strongly connected
components over all signal edges, including delayed inputs, and schedules their dependency graph.
Only feedback components execute frame by frame in the validated node order, committing their
delay inputs after the component has evaluated each frame. Feed-forward delay nodes read and
commit each frame within their node chunk. Other components retain block processing, so delays
do not force unrelated nodes through the frame interpreter. If only audio input/output sit outside
feedback components, the original frame schedule avoids unnecessary scratch copies. Feedback
timing is unchanged.
Processing a longer host buffer reuses the same bounded scratch storage.
Delay time is an ordinary dependency, so a cycle through it is rejected.

File size is capped at 2 MiB, graphs at 128 nodes, 640 connections and 256 parameters.
Shared delay history is capped at 2,000,000 stereo frames and validated at 384 kHz, before
allocating history. Accepted graphs fit all supported rates (8..384 kHz). Non-finite signals
become zero and finite results are limited to the f32 range after every node.

Compilation with `compile_at(sample_rate)` prepares all buffers and DSP state off the audio
thread. `process_context` receives the engine transport (tempo, beat, playing); `process`
uses stopped transport defaults. Oscillators and delays continue to advance while stopped.
Parameter updates and processing allocate nothing. `reset` clears history without changing
parameter values. Offline and real-time rendering run this same processor; graph swaps keep
state when the embedded graph is unchanged, including effect tails and oscillator phase.

See the complete [node reference](sdk-nodes.md) and the [authoring guide](../device-sdk.md),
including the machine-readable catalog for AI agents.

## Versions

The file version rises with every change to what a file holds, and every file is written as the
newest ([ADR 0010](../decisions/0010-project-files.md) › Amendment: format versions); a reader
refuses a newer file and reads every older one. `device.version`, the device's own revision, is
separate (above).

| Version | What it added |
|---|---|
| 1 | Input, gain and output nodes in a linear chain |
| 2 | Every node kind and port connections |
| 3 | `editor` metadata |
| 4 | `device.version` and `migrations`, apart from the file version |
| 5 | `device.expanded`, the expanded panel; since 2026-10-24 written by every file, not only one with an expanded panel |
| 6 | A panel's `style`, its own style sheet (theme tiers, `docs/spec/theme-skins.md`) |

## Visual connection layout

Optional `editor.graph` contains `nodes` and `wires` arrays (both default empty).
This is authoring data for a future editor, not an interactive editor or a second processing
graph. The existing read-only node list does not render this layout yet. Unknown fields in
editor objects are rejected so unsupported metadata cannot be silently discarded.

Node layout records have `node` (an existing node id), `position: [x,y]` and
`size: [width,height]`. Positions and wire points are logical pixels in graph coordinates
(-8192..8192); sizes are 24..4096. At most one layout per node, including hidden nodes, is
allowed. Layout records are optional for individual nodes; absent records mean automatic layout.

Wire records have `connection` (the existing `from`, `from_port`, `to`, `to_port` tuple),
`points` (default empty, at most 256 intermediate polyline bends), and optional
`from_anchor` / `to_anchor` offsets in logical pixels from the corresponding node's top-left.
An explicit anchor requires a layout for that endpoint, and must lie within its node rectangle
(edges included). Omitted anchors mean automatic port placement; they are not audio values.
The complete visual route is source anchor, intermediate points in array order, destination
anchor. No bend means a direct line. The endpoint tuple, not an array index, identifies a wire;
reordering connections does not change its meaning. One route per existing connection is allowed.
Different connections using the same port must use the same explicit anchor for that port.
Removing or reconnecting an edge requires updating/removing its route before saving.

There are at most 128 node layouts, 640 wire routes and 4096 total bend points. All geometry
must be finite. Invalid references, duplicate layouts/routes and inconsistent port anchors fail
validation before compilation. Editor data is preserved by SDK serialization and embedded
project state and is not read by DSP processing. It does not change the existing file-size cap.
Versions 1/2 must not contain non-null `editor` data.

```json
"editor": {"graph": {
  "nodes": [
    {"node":"in", "position":[0,0], "size":[120,80]},
    {"node":"gain", "position":[240,0], "size":[120,80]}
  ],
  "wires": [{
    "connection":{"from":"in","from_port":"out","to":"gain","to_port":"in"},
    "from_anchor":[120,40], "to_anchor":[0,40],
    "points":[[180,40],[180,60]]
  }]
}}
```

Generate a complete example with `cargo run -p suhina-device-sdk --example wire_layout --
target/sdk-examples/wire-layout.sd`, then validate it with the `validate` example.
Layers, component hierarchy and editable envelope documents remain separate planned additions.

## Example

```json
{
  "format": "suhina-device",
  "version": 5,
  "device": {
    "id": "org.example.gain",
    "version": 1,
    "name": "Example Gain",
    "role": "effect",
    "migrations": [],
    "parameters": [{ "id": "gain", "name": "Gain", "min": 0.0, "max": 2.0, "default": 1.0, "control": "slider" }]
  },
  "nodes": [
    { "id": "in", "kind": "audio_input", "visible": false },
    { "id": "gain", "kind": "gain", "parameter": "gain", "visible": false },
    { "id": "out", "kind": "audio_output", "visible": false }
  ],
  "connections": [{ "from": "in", "to": "gain" }, { "from": "gain", "to": "out" }]
}
```

The graph can be created with the SDK example and then inserted on a track:

```sh
cargo run -p suhina-device-sdk --example gain -- path/to/gain.sd
```

The browser currently lists `.sd` files under Effects as `AudioEffect`. Drop one onto a track or
run `device.addSd` through MCP. Audio effects work on audio or MIDI tracks; MIDI effects and
instruments require a MIDI track. Dropping an `.sd` into empty space or calling `track.add`
with `sd` creates an audio track for an audio effect and a MIDI track for an instrument or MIDI effect.
The complete validated graph is
stored in the project with the device, so the original `.sd` file is not needed after insertion.
Projects keep this embedded graph when a newer file becomes available. Run `device.updateSd`
explicitly with the newer file to update a device; its id and role must match, its device revision
must increase, and parameter migration is applied as one undoable command.
Parameter values are project state and can be changed with `device.setParam` or the generated
sliders in the detail panel. They can also be automated by stable parameter id with
`device.setAutomation`; their lanes appear in the Arrangement parameter chooser. The
[automation spec](automation.md) describes lane editing and playback. The `AudioEffect` panel
reserves space for each slider's value so controls stay inside narrow device cards.

All three roles execute. Instrument graphs expose `midi_key`, `midi_velocity` and `midi_gate`
signals to the existing audio nodes to generate sound. This initial instrument runtime is
monophonic for these three signals: the latest note controls them, and note-off releases the gate.
The `sampler` source instead owns polyphonic voices and broadcasts no MIDI value signals.
It receives timestamped events, exposes stereo audio, and requires off-thread PCM/control binding
through `PreparedEffect::sampler_mut`; sample assets are not embedded in the file. MIDI effects go
before the track instrument; their event buffers are prepared with the chain, so processing does
not allocate on the audio thread. On a track with no instrument the notes they send play as the
track's sine tones, as for built-in MIDI effects ([ADR 0004](../decisions/0004-built-in-devices.md) › 13).
The `.sd` file contains data only. FFT, sample assets, oversampled nonlinear processing and
freely arranged graph editing are not implemented; accepted graphs need none of these to load.

## Custom parameter controls and smoothing

Parameter `control` accepts `slider`, `log_slider`, `integer`, `toggle`, `knob`, `log_knob`,
`number`, `vertical_slider`, `button`, `choice` and `radio`. Button is a persistent on/off
control. Choice/radio use integer values 0..N-1 and `ui.options` (N nonempty labels, at most
128 labels of 128 bytes). Log controls require a positive minimum. Integer/toggle/button and
choice/radio values stay discrete. All edits use `device.setParam` and persist in the project.

Optional `ui` settings: `tooltip` (up to 1024 bytes), `group` (label prefix in the generated panel, and the heading the automation lanes' parameter menu gathers the parameter under; up to 128 bytes),
`step` (nonnegative finite value, default 0), `decimals` (0..8, default 3), and `options`.
Rows use the selected control's height and the panel scrolls when controls exceed its viewport.
Executable UI code and arbitrary pixel layouts are not part of the data format.

`smoothing_ms` defaults to 20 ms (including files without this field), accepts 0..1000 ms,
and ramps continuous parameter changes on the audio thread without allocation. Initial values
are applied immediately before the first frame. Reset settles to the target value. Discrete
controls switch immediately; use a `smooth` signal node when a toggle drives gain. This reduces
parameter step clicks; unstable feedback, discontinuous mode switches and underruns still need
an effect-specific solution. Delay-time ramps can change pitch as the read head moves.

Generate a control gallery with
`cargo run -p suhina-device-sdk --example ui_controls -- target/sdk-examples/ui-controls.sd`.
The SDK examples put labels above controls and values underneath, with enough cell height for
both rows. Run each example again after changing its presentation: `cargo build` alone does not
rewrite `.sd` files. Reinsert a device to update a project's embedded copy of its graph.
The Custom Panel, Orbit Filter and Wire Layout examples include explanations inside the panel.
`custom-panel.sd` appears as Velvet Volume to demonstrate an independently chosen `device.name`;
`orbit-filter.sd` appears as Orbit Filter and demonstrates custom UI. Their device ids are distinct.
Orbit Filter demonstrates a custom drawn XY pad, output knob, gradient and embedded texture;
it combines a low-pass filter, stereo balance and output gain. Wire Layout processes
only gain; its node positions and wire bends demonstrate saved graph-editor metadata, while the
current device panel shows its signal order as a node list.

Example parameter UI declaration:

```json
{"id":"mode","name":"Mode","min":0,"max":2,"default":1,"control":"choice",
 "ui":{"options":["Off","Clean","Warm"],"group":"Tone","tooltip":"Select the processing mode"}}
```

## Visual styling and placement

`ui.size` is an optional [width,height] in logical pixels for the entire parameter cell
(24..4096 wide, 24..4096 high). `ui.position` is optional [x,y] relative to the start of the
parameter area, after the node list (0..8192). Unpositioned cells stack after earlier cells.
Both axes scroll when the authored canvas exceeds the viewport. Explicit cells may overlap;
parameters later in the array draw later. Layout is stored as data in the embedded graph.

Optional RGB triplets `ui.accent`, `ui.background`, `ui.panel_color` and `ui.text_color` style controls, frames
and text. Omitted colours follow the Suhina theme. `ui.font_size` is 8..48 (default 12),
`ui.monospace` selects the font family, and `ui.corner_radius` is 0..128 (default 4).
`ui.shape` is rounded (default), square or pill; knobs use their circular body by default
and a square body for square. `ui.label_position` is left (default), top or hidden;
`ui.show_value` defaults to true. With a top label, the control fills the middle row and its
label and value are centered above and underneath the control; set `show_value` to false to omit
the value row. Knob size follows
the available control area.

For a device with a custom canvas, `device.ui.viewport_size[0]` sets the preferred available
panel width (24..8192 logical pixels); the host clips it to the detail area and scrolls its
contents. This width setting applies to the whole device panel, including its generated controls.
Without a custom canvas, a device in the horizontal chain expands to fit the declared parameter
cells, with a minimum width of 220 pixels. The UI Controls example uses a single horizontal row.

These fields are optional and old files retain automatic placement. Authors must leave enough
cell space for labels and controls. Untrusted geometry is checked before UI rendering;
there is no executable UI code, external font loading or arbitrary drawing script.

`background` colours widget bodies; `panel_color` colours the entire cell independently.

For a compact knob use `size: [40,40]`, `label_position: "hidden"`, `show_value: false`.

## Authored canvases and embedded images

Optional `device.ui` adds a canvas with backgrounds, embedded PNG/JPEG images, drawing expressions
and custom parameter controls. Canvas `size` defaults to `[640,240]`; optional `viewport_size`
(default null, automatic) independently sets the preferred scrollable body dimensions. See the [custom panel contract](sdk-custom-ui.md) for every field,
gesture and resource limit. Graphics are evaluated on the UI thread and are stored in the same
`.sd` as the audio graph. They do not change its DSP execution contract.

`device.ui` is the compact panel, drawn in the device chain. Optional `device.expanded`
(version 5) is a second, larger canvas with the same fields and limits, validated against the same
parameters; the device's header then has a button that opens it over the main area
([sdk-custom-ui.md](sdk-custom-ui.md) › Compact and expanded panels).

## Panel style sheets (version 6)

`device.ui.style` and `device.expanded.style` are optional CSS text (at most 64 KiB, no NUL) that
style that panel, and only that panel: presentation, never the graph, parameters or bindings. The
host applies style tiers in order (ADR 0031 › Device panels use the same style engine):

1. **Default:** Suhina's own appearance.
2. **Theme:** the application theme's style sheet, which can reach device panels through
   `.device-panel`, `.device-control` and the widget classes (`.knob`, `.button`, …).
3. **Device:** this sheet, bounded to its panel: its selectors match only elements inside the panel,
   so `.browser { … }` here changes nothing outside it.
4. **User:** `user.css` in the themes folder, over everything, and kept when the theme changes.

A higher tier wins over a lower one even against `!important`; within a tier importance,
`@layer`, specificity and source order apply. Each control is a `.device-control` element whose id
is the control's `id`, so `#cutoff { color: #ffd27f; accent-color: #ff8a00; }` styles the control
`cutoff`. Built-in widgets take `color` (text and ink), `background-color` (body), `border-color`
and `accent-color` (arc, switched-on fill) from it; what a sheet leaves out keeps the theme. A
sheet that does not parse is ignored and the panel keeps the theme.

```json
"ui": { "size": [260, 120], "style": "#cutoff { accent-color: #ff8a00; }\n.device-control:hover { color: #ffffff; }", "controls": [ … ] }
```

## Linked parameter edits

A parameter's `ui.link` may contain `enabled_by` (a declared 0..1 Toggle id) and `peer`
(a different parameter id with the same range, control kind and unit). Invalid references
are rejected. Links expand `device.setParam` on the session thread into clamped parameter
messages; they do not perform work on the audio thread. Expansion is one hop, not recursive.
Declare reciprocal links for bidirectional pairs. While the toggle is enabled, either edit
updates both stored values. When enabling the toggle, the earlier parameter in each reciprocal
pair is copied to the later one. The complete edit shares one undo snapshot and all messages
reach the attached player; undo, redo and project saving preserve the pair.

## Built-in devices are not `.sd` files

Registered built-in audio effects resolve automatically when a `.sd` graph is added and
when track or group chains are prepared. The registry adapter supplies the `in` audio
signal at the effect defaults; additional control connections are rejected by this
adapter unconditionally. `Node::bind` maps parameters and does not enable control1 through control4 connections. Custom Rust hosts can still supply their own input-aware kernels. Unknown ids,
non-effect devices and effects reporting nonzero latency are rejected before insertion;
latency compensation between graph paths is not implemented. This never loads plug-in code.


## Sample composition and embedded private graphs

The node reference defines prepared resources, event routes, polyphonic voices, granular playback,
and versioned components. Node `bindings` is an optional list of `{parameter,target}` objects:
the parent parameter id maps to a private/native parameter id. Empty lists are omitted. Unknown
parent/target ids and duplicate targets are rejected; default values are installed before rendering.
Old files without bindings behave unchanged. New node kinds and fields are rejected by older strict
readers instead of being silently dropped. Existing stock Sampler ids/revisions are unchanged.

`Component`, `VoiceGroup` and `GrainPool` store their complete child `.sd` text. Every child validates
independently, then expanded preparation caps nesting at four, nodes at 4096, voice capacity at 128,
grain capacity at 256 and aggregate delay storage at 2,000,000 frames. Binding targets are validated
against the prepared child. Sample PCM is not duplicated per instance. The resource key survives a
round-trip; the host supplies the corresponding PreparedSamples map after relocation. Project commands bind one source named `sample` through `device.addSd` or `track.add`;
`device.loadSample` replaces it while preserving graph state. The desktop has no resource-map
editor. Custom Rust hosts can supply several named bindings.
