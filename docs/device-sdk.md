---
description: Author data-only effects, MIDI effects and instruments in Rust or with AI agents
read_when: make an effect, MIDI effect or instrument, SDK tutorial, .sd example, node discovery
type: howto
status: current
last_verified: 2026-11-01
covers:
  - crates/device-sdk/src/author.rs
  - crates/device-sdk/src/ui/widgets.rs
  - crates/device-sdk/examples/stereo_utility.rs
  - crates/device-sdk/examples/lfo_pan.rs
  - crates/device-sdk/examples/filter.rs
  - crates/device-sdk/examples/distortion.rs
  - crates/device-sdk/examples/delay.rs
  - crates/device-sdk/examples/compressor.rs
  - crates/device-sdk/examples/sampler_composition.rs
  - crates/device-sdk/examples/catalog.rs
  - crates/device-sdk/examples/validate.rs
  - crates/device-sdk/examples/ui_controls.rs
  - crates/device-sdk/examples/custom_panel.rs
  - crates/device-sdk/examples/wire_layout.rs
  - crates/device-sdk/examples/midi_transpose.rs
  - crates/device-sdk/examples/gated_instrument.rs
  - crates/session/src/commands.rs
  - crates/session/src/devices.rs
---

# Author a data-only SDK device

## Checked Rust authoring

`author::GraphBuilder` is an additive, off-thread alternative to editing the graph's vectors.
It owns append-only node storage. Node handles belong to exactly one builder; copying a handle
shares the same node, and passing a handle from another builder returns an error. Port names use
the existing node catalog. `connect` rejects missing ports and occupied inputs immediately.
`op` inserts a node with named inputs; `binary` wires `a` and `b`. A failed `op` or
`parameter_signal` leaves no partial nodes, connections or parameters behind.

```rust
use suhina_device_sdk::{author::GraphBuilder, DeviceRole, FilterMode, NodeKind, Parameter,
    ParameterControl};

fn example() -> Result<(), Box<dyn std::error::Error>> {
let mut graph = GraphBuilder::new("org.example.filter", "Filter", DeviceRole::Effect);
let input = graph.node("in", NodeKind::AudioInput)?;
let output = graph.node("out", NodeKind::AudioOutput)?;
let cutoff = graph.parameter_signal(
    Parameter::new("cutoff", "Cutoff", (20., 20000.), 1000.)
        .unit("Hz").control(ParameterControl::LogKnob),
)?;
let filter = graph.op("filter", NodeKind::Filter { mode: FilterMode::Lowpass }, &[
    ("in", input.out()), ("frequency", cutoff.out()),
])?;
graph.connect(filter.out(), output.input("in"))?;
let graph = graph.finish()?;
graph.write_file(std::path::Path::new("filter.sd"))?;
Ok(())
}
```

Declare UI-only parameters with `parameter`; it consumes no DSP node. `parameter_signal`
declares both the parameter and its source node, using the same explicit id. Parameter setters
`unit`, `control`, `smoothing_ms`, `options` and `linked(peer, enabled_by)` configure data before
insertion; they do not bypass validation or infer a range from choice labels.

For feedback, declare the summing node first, construct the delayed branch and then `connect`
the return to the free input. Do not invent a forward string reference. Normal Rust functions
can add reusable fragments with explicit scoped ids and return node handles. `finish` runs the
existing full graph validation, including roles, required ports, cycles and resource budgets.
It consumes the builder and returns an ordinary `DeviceGraph`; set panel, editor and revision
metadata on that graph and validate again when saving or compiling. Builder handles cannot be
used to edit the finished graph. Existing low-level authoring remains supported.

The filter, LFO pan and Delay examples use this API. Their ids, parameter contracts, panel data
and sound are regression-tested against pre-migration `.sd` fixtures. The Delay graph is more
explicit about its node references; the main reduction in repetition is in its panel construction.
No new file format, runtime or executable extension is introduced.

UI authors can configure numeric `format`/`decimals` and button labels before insertion,
inherit parameter names, mapping and numeric formatting with `parameter_metadata`, and place
controls relative to a shared `ControlGroup` origin. These are Rust authoring conveniences;
the saved panel remains flat. See [control constructors and groups](spec/sdk-custom-ui.md#rust-control-constructors).

Native sample instruments can also use `sampler::Sampler` from the SDK and author
their panel with `ui::DeviceUi`; `devices/sampler` is the built-in example. This
route uses the native Device contract, not a serialized sample DSP node. The host
installs decoded source audio during graph preparation. `Widget::SampleWaveform`
provides the host waveform, region handles, slice editing and zoom/pan; `Widget::SampleRecording` places
input selection and recording actions independently for that instrument.

The deliverable is one `.sd` file. It can be added to a track and is embedded in the project.

Version 4 separates the `.sd` file schema version from `device.version`, the revision of the
device's stable id and parameter contract. A device revision bump declares a migration from the
previous revision; migrations can preserve renamed parameter ids. Projects retain their embedded
graph until `device.updateSd` explicitly updates it.

Version 4 also preserves visual connection authoring through `graph.editor`:
`editor::Editor` holds a `GraphLayout`, with `NodeLayout` positions/sizes and `WireLayout`
routes referencing existing `Connection` endpoints. Optional anchors are node-local pixel
offsets; bend points are graph coordinates. See the [format contract](spec/sd-device-format.md#visual-connection-layout).
This stores a future editor's layout; the current app still shows the read-only node list.
File versions 1 through 4 remain readable, and serialization writes version 5
([format contract](spec/sd-device-format.md#versions)). Older device files
without `device.version` load as revision 1.

```sh
cargo run -p suhina-device-sdk --example wire_layout -- target/sdk-examples/wire-layout.sd
cargo run -p suhina-device-sdk --example validate -- target/sdk-examples/wire-layout.sd
```
An author writes Rust that assembles a graph from supported nodes; Suhina runs the graph as data.
Use [the format specification](spec/sd-device-format.md) and [node reference](spec/sdk-nodes.md).
Audio effects, MIDI effects, monophonic MIDI-value instruments and polyphonic sampler and voice graphs execute. Parameters, nodes and device ids
are stable saved identifiers.

Generate the MIDI examples with:

```sh
cargo run -p suhina-device-sdk --example midi_transpose -- target/sdk-examples/midi-transpose.sd
cargo run -p suhina-device-sdk --example gated_instrument -- target/sdk-examples/gated-instrument.sd
```

The transpose graph runs before the instrument and preserves each event's frame offset. The
instrument example maps MIDI key to oscillator frequency, then uses gate and velocity to shape
the audio output. The instrument is monophonic: a new note takes over from the currently held note.

## Start with a working effect

Generate six independent examples:

```sh
cargo run -p suhina-device-sdk --example stereo_utility -- target/sdk-examples/stereo-utility.sd
cargo run -p suhina-device-sdk --example lfo_pan -- target/sdk-examples/lfo-pan.sd
cargo run -p suhina-device-sdk --example filter -- target/sdk-examples/filter.sd
cargo run -p suhina-device-sdk --example distortion -- target/sdk-examples/distortion.sd
cargo run -p suhina-device-sdk --example delay -- target/sdk-examples/delay.sd
cargo run -p suhina-device-sdk --example compressor -- target/sdk-examples/compressor.sd
cargo run -p suhina-device-sdk --example validate -- target/sdk-examples/delay.sd
```

Each example includes its own graph-building and control-layout helpers; no shared example module is required.

Read the individual files under `crates/device-sdk/examples/` for complete graphs: stereo width, LFO pan,
low-pass filter, distortion with wet/dry, filtered/modulated delay and a stereo-linked compressor.
The compressor is a hard-knee example, the distortion is not oversampled and oscillators are
not band-limited; these examples demonstrate composition rather than production mastering.
Feedback greater than unity can grow without bound; constrain feedback parameters and use
an explicit clamp/tanh if required by the effect. Signal sanitization does not guarantee stability.

The core building pattern is:

```rust
use suhina_device_sdk::{Connection, DeviceGraph, DeviceRole, FilterMode, Node, NodeKind};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut graph = DeviceGraph::new("org.example.filter", "Example Filter", DeviceRole::Effect);
    graph.nodes = vec![
        Node::audio_input("in"),
        Node::new("filter", NodeKind::Filter { mode: FilterMode::Lowpass }),
        Node::audio_output("out"),
    ];
    graph.connections = vec![Connection::new("in", "filter"), Connection::new("filter", "out")];
    // Unconnected frequency/Q inputs use the documented 1 kHz/Butterworth defaults.
    graph.write_file(std::path::Path::new("filter.sd"))?;
    Ok(())
}
```

For editable controls, declare `Parameter::new(id, name, (min, max), default)`, set `unit` and
`control` as needed, and add `NodeKind::Parameter { parameter: id.into() }`. Connect its output
to the processing input with `Connection::ports("cutoff", "out", "filter", "frequency")`.
Use a smooth node when a parameter transition should ramp. Changes are automatically exposed
through the device panel and `device.setParam`; no custom UI or command is needed per effect.
Set `node.visible = false` for implementation nodes to keep the device panel compact;
this never removes their processing. The examples expose only their main processing nodes.

## Workflow for AI agents

1. Read the format and node reference. Obtain the current catalog with the read command
   `device.sdNodes {}` via MCP `command_run`.
   Offline, `cargo run -p suhina-device-sdk --example catalog` prints the same node records as JSON.
   Each record gives its kind, Rust variant, ports, units, optional defaults, processing rules
   and a minimal JSON node-kind example. Waveform/filter choices and graph limits are returned
   by `device.sdNodes` alongside the records. Do not invent unsupported node kinds or ports.
2. Assemble an effect graph. Give every node and parameter a unique stable id. Declare a
   parameter for each user control. Prefer an existing example with similar signal flow.
   Every node must reach the output, including the audio input. For an unconnected optional
   input the catalog default applies; a required input must have exactly one connection.
3. Save with `write_file` or author equivalent version 5 JSON. Call `validate` or run the
   validation example before adding the effect. Errors identify missing ids/ports, undelayed
   cycles and memory requests; fix those directly. A minimal catalog example describes
   `NodeKind`; wrap it with `id` when placing it in the JSON nodes array.
4. Test a self-authored impulse, silence and an asymmetric stereo signal. Compare multiple
   block sizes and reset. Check the intended response and parameter limits, not merely that
   the graph compiles. For feedback ensure the decay/growth behaviour matches the design.
5. Insert with a browser drop or `device.addSd {track, path}`; use `track.devices` to inspect
   values and `device.setParam` to change them. Save/reopen the project to check persistence.

## Adding a supported node

Document its behaviour in the spec before implementing DSP. Add a serializable `NodeKind`,
its `NodeInfo` catalog entry and exhaustive `info` mapping, validation of configuration,
prepared runtime state and processing/reset behaviour. Each new state must obey the audio
thread rules. Reuse `suhina-dsp` where possible. Include units, defaults, accepted ranges,
formulas, invalid-input behaviour, state/reset and any intentional delay in its description.
Provide a minimal catalog example and a focused audible/numerical test. The shared catalog
conformance test checks serialization, hostile inputs, reset and block independence.
Regenerate the node-reference table with the catalog's `--markdown` option and update the
format/ADR when the schema or processing rules change. Do not edit the generated docs index.


Custom UI controls and configurable parameter smoothing are described in the
[format specification](spec/sd-device-format.md). Generate a gallery with
`cargo run -p suhina-device-sdk --example ui_controls -- target/sdk-examples/ui-controls.sd`.

## Style a control

Each parameter's `ui` can include explicit cell geometry, theme overrides and label placement.
Coordinates are relative to the start of the parameter area. Omitted positions stack vertically;
explicit positions support columns or any arrangement. See the format spec for validation limits.
For example:

```json
"ui": {
  "size": [240, 150], "position": [260, 0],
  "accent": [85, 190, 160], "background": [50, 61, 73], "panel_color": [28, 35, 44], "text_color": [230, 235, 240],
  "font_size": 14, "monospace": false, "corner_radius": 10, "shape": "rounded",
  "label_position": "top", "show_value": true, "tooltip": "Output gain"
}
```

Sizes describe the whole cell (label, widget and value), not just the widget. A top label needs
at least `2*font_size+20` pixels of cell height. Square knobs and rounded/pill widget shapes
use the existing control behaviour, including keyboard edits and default reset. Settings are
local to a parameter and do not modify the global theme. The `ui_controls` example generates
a single-column canvas with styled controls sized for a narrow device panel. Existing project devices embed their graph; import
the new file as a new device to use its changed layout. Agents can obtain defaults and enum
choices from `device.sdNodes`.

`background` colours widget bodies; `panel_color` colours the entire cell independently.

## Custom panels, images and controls

Build an original panel with an embedded texture, XY pad and custom rotary control:

```sh
cargo run -p suhina-device-sdk --example custom_panel -- target/sdk-examples/custom-panel.sd
```

The example `crates/device-sdk/examples/custom_panel.rs` is a complete effect and editable authoring
source. `graph.ui = Some(DeviceUi { .. })` selects canvas dimensions, RGBA background, drawing
layers, embedded assets, controls and `hidden_parameters`. Stock controls remain available for
parameters not hidden by the author. The canvas scrolls when larger than the device viewport.

### Backgrounds and artwork

Use `Draw::Rect` for panels, `Draw::Gradient` for colour transitions, and paths, circles, lines
and arcs for vector icons. Images reference an `ImageAsset` id. Load your own file at authoring time:

```rust
use suhina_device_sdk::ui::{DeviceUi, Draw, ImageAsset};
let mut canvas = DeviceUi::default();
canvas.assets.push(ImageAsset::from_file("photo", std::path::Path::new("my-photo.jpg"))?);
canvas.layers.push(Draw::image("photo", [0.0, 0.0, 640.0, 240.0]));
graph.ui = Some(canvas);
```

Only embedded bytes are saved; the source image is not needed on another computer. PNG supports
alpha for icons and overlays; JPEG supports photos. `Draw::Image` offers RGBA tint, normalized UV
crop, stretch/contain/cover and `tile_size` for repeating patterns. One image is limited to 512 KiB
and one million pixels; total images to 1 MiB and four million pixels, `.sd` to 2 MiB. Decode errors
are displayed in the panel. Resize/compress large source images before embedding them.

### Write a control

A `CustomControl` has a stable id, accessible name, tooltip, `[x,y,width,height]` bounds, a drawing
program, a gesture and optional X/Y `UiBinding`s. All coordinates inside a control are local to its
bounds. Layer order is drawing order. Filled paths are convex.

- `Position`: pointer position maps to X/Y values; use for XY pads or bespoke faders.
- `Drag`: relative movement edits X/Y; bind Y for a vertically dragged rotary control.
- `Toggle`: clicks alternate the X parameter between its limits.
- `Momentary`: the X parameter stays at maximum while pressed and returns to minimum on release.

`UiValue::normalized("cutoff", true)` returns logarithmic normalized cutoff. Combine values with
`+`, `*`, `.sin()` and `.cos()` to draw handles, arcs, crosshairs and parameter-driven indicators.
`UiValue::parameter(id, false)` reads raw units; `Draw::Text` can append a formatted current value.
Use the same logarithmic flag in the expression and input binding. Custom controls support focus,
arrow keys, Shift fine adjustment, accessible names/values and double-click reset. Changes use the
normal parameter command, smoothing and undo; a two-axis gesture has one undo entry.

### Information for agents

Read [the complete custom UI contract](spec/sdk-custom-ui.md), then the example source and public
Rust types in `crates/device-sdk/src/ui.rs`. `device.sdNodes` includes primitive/gesture names,
default canvas values and image budgets. Create artwork yourself or use user-supplied licensed assets.
Do not attach external paths or executable code to the device. Keep parameter ids stable, validate
with the `validate` example, and rebuild the `.sd` after editing the Rust authoring source.

### Device and viewport dimensions

Set `DeviceUi.size = [800.0, 400.0]` to change canvas width and height (default `[640.0, 240.0]`).
Set `DeviceUi.viewport_size = Some([320.0, 240.0])` to reserve a smaller scrollable body;
its default is `None` (automatic host sizing). Both accept 24..8192 logical pixels per axis.
The header and padding are added by the host; available panel space may clip the viewport.
See [the sizing contract](spec/sdk-custom-ui.md) for automatic sizes and both chain orientations.

## Delay example

The 944 by 168 Delay panel has Left and Right timing columns with a link icon between them.
Each column has a Beats/ms button, delay length and offset: Beats offers explicit note lengths (for example
1/8, 3/16, 1/4), while ms exposes a logarithmic millisecond field. The saved division
still counts sixteenth notes from 1 through 16. Offset is only available in Beats mode.
Offsets adjust synchronized times by up to 33.3 percent; they do not alter the millisecond value in Time mode. Link mirrors Sync/Time mode, division, offset and millisecond edits in both directions.
Enabling Link copies the left settings to the right. Linked edits, including continuous drags,
are one undo step and persist in the project.
Feedback and Mix are percentages. Freeze sits below Feedback; it stops feeding new input and sets feedback to unity.
Ping Pong sits below Mix; it feeds the left channel first and swaps channels on each return.
The filter has a logarithmic centre frequency, width in octaves, high/low-pass edges and bypass.
The graph is a smooth logarithmic band display controlled by the same frequency/width parameters,
with continuous two-pole high-pass and low-pass magnitude curves at narrow and wide settings.
Motion controls delay-time depth, filter-frequency depth, rate and morph towards sine.
The horizontal order is Time, Feedback, Tone, Motion, then Mix.
The panel fits a 200-pixel slot including the host header and padding. The former
`lfo_expanded` parameter is retained for compatibility but no longer hides controls.
Widget colours follow the active Suhina theme, including number text and accents.
Motion has one speed control: frequency in Hz, period in milliseconds, or a rhythm menu.
There are no duplicated Time or depth controls.
The default waveform is Sine. Wave opens a menu of Sine, Triangle, Ramp Up, Ramp Down, Square, S&H and Wander.
S&H holds a deterministic random value per cycle; Wander smooths that source with a 200 ms one-pole.
Mode opens Hz, Time, Synced, Triplet, Dotted and 16th steps. Hz edits frequency and Time edits milliseconds.
Synced edits a whole-note divisor (1/N), Triplet and Dotted scale that frequency by 3/2 and 2/3,
and 16th steps selects the number of sixteenth notes per cycle. Rhythm menus offer
explicit values such as 1/8, 1/8 T, 1/8 D and 3 x 1/16; the caption changes with the mode.
These modes follow transport tempo.
Menu choices use the standard parameter command, persistence and undo. Opening or dismissing
an overlay does not create an edit.
Repitch slews the read-head distance over 20 ms, Fade crossfades read heads over 20 ms, and Jump
changes distance immediately. Times are bounded to one sample..4 seconds, including tempo sync
and modulation. The processor is an original implementation, without a claim of sound equivalence.
Existing projects embed their old graph; add the newly generated delay.sd to use this panel.
The entire example, including its node-authored panel, is in `examples/delay.rs`.
After SDK UI nodes change, rebuild the desktop application as well as the example
(`cargo build -p suhina`); an older executable cannot read newly added widget kinds.

### Reusing functional panel elements

`ui::Widget` describes one functional UI node: `Button`, `Number`, `Knob`,
`Disclosure`, `Menu`, `WaveformMenu` or `FilterBand`. Assign it to a
`CustomControl.widget`, set that control's id, bounds and parameter bindings,
and leave `drawing` empty. The host supplies the artwork and existing gestures.
Delay uses widgets for every interactive element; its Motion section is a composition
of individual widgets, not a reusable section with fixed parameters.

For example, a choice selector in any device can use:

```json
{"id":"mode_menu","name":"Mode","bounds":[10,10,80,16],
 "gesture":"menu","x":{"parameter":"mode"},"widget":{"kind":"menu"}}
```

Declare `mode` as a Choice parameter with its own options. Buttons use Toggle or
Set gestures; Number and Knob use Drag with a Y binding; Disclosure uses Toggle;
menus use Menu with a Choice X binding; FilterBand uses Position with X frequency
and Y width bindings (logarithmic X, linear Y). WaveformMenu uses seven choices
in order: sine, triangle, rising ramp, falling ramp, square, sample-and-hold and wander. Number supports precision, number/frequency/choice formatting,
colour, alignment, border, prefix and suffix. The widget contains no device-specific
parameter names. Custom drawings remain available for artwork and specialised controls.

## Layouts for built-in devices

A built-in device (`devices/<name>`) is native Rust, not a `.sd` graph. Its panel uses this
SDK's UI part only: `suhina_device_sdk::builtin::Layout` holds a compact canvas (`ui`), an
optional expanded one and how each parameter is presented, by parameter id. Parameters come
from the device's `DeviceInfo`; a layout cannot declare one. A device crate gives its layout in
Rust (the Sampler's `layout()`) or as `devices/<name>/panel.json`, listed in
`suhina_device_registry::LAYOUT_FILES`; `suhina_device_registry::panel(id)` loads it by id, and
a test fails when it names a parameter its device lacks. A device without a layout gets the
generated list. See [sdk-custom-ui.md](spec/sdk-custom-ui.md) › Layouts for built-in devices.

Use `Widget::Panel { label }` for themed, non-interactive groups behind controls.
`UiGesture::Tab(value)` on a Button selects a positive value in a zero-based
parameter; pressing the active tab returns to zero. Sampler uses this to switch
between its normal controls, envelopes and LFO without changing audio parameters.

`Widget::Envelope` provides a theme-aware point editor for cumulative ADSR or bipolar Attack/Decay envelopes; the sampler demonstrates both, with separate numeric readouts.

## Compose a sample source

Run `cargo run -p suhina-device-sdk --example sample_source` from the repository
root. The example constructs a Sampler source, multiplies its stereo output by
a graph parameter, binds a generated one-second sine sample off the audio thread,
and renders timestamped note events. It prints 48000 frames and a nonzero peak.
See [Sampler](spec/sampler.md#composable-sample-source-first-stage) for the
source-binding contract. The built-in Sampler remains native code with an SDK panel.

## Trusted native graph effects

Use `NodeKind::NativeEffect { effect }` to wire a trusted host effect into a graph.
Connect audio to `in`; the four optional `control1` through `control4` inputs carry
additional stereo signals and default to zero. Prepare with `compile_with_native_at`
and a host resolver that returns a fresh `NativeNode` prepared for the supplied sample rate.
The kernel receives five read-only interleaved stereo input buffers and a separate output
buffer of equal length. It must overwrite output without allocating, locking or doing I/O.
This host API does not load third-party plug-ins, and ordinary `compile_at` rejects
unresolved effects. MIDI events and reset use the same lifecycle as native sources.

Registered built-in audio effects resolve automatically when a `.sd` graph is added and
when track or group chains are prepared. The registry adapter supplies the `in` audio
signal at the effect defaults; additional control connections are rejected by this
adapter; named `Node::bind(parent, target)` parameter bindings are supported. Custom Rust hosts can still supply their own input-aware kernels. Unknown ids,
non-effect devices and effects reporting nonzero latency are rejected before insertion;
latency compensation between graph paths is not implemented. This never loads plug-in code.


## Compose editable samplers

Run from the repository root:

```sh
cargo run -p suhina-device-sdk --example sampler_composition -- target/sampler-composition
```

The example writes five `.sd` files and five audible `.wav` renders: loops, reverse, slices,
granular and tempo. Each definition round-trips and runs with the same deterministic source bound
as `sample`. `sampler_template` and `granular_template` return editable DeviceGraph values with
standard parameter panels; their private voices use the same public node catalogue. To use real
samples, decode them off-thread in your host, make SampleAsset values, insert stable binding names
into PreparedSamples, and call `compile_with_samples_at`. For registered/custom native effects,
use `compile_with_samples_and_native_at` with a trusted resolver. A missing binding is an error;
a prepared graph retains its original resource after replacing the host map. Retire it off-thread.
The desktop `device.addSd` path does not yet provide a resource-map editor for these compositions.

`NodeInfo::output_type` and the JSON catalogue's `output_types` describe event/resource/signal
ports identically to validation. Named outputs include per-voice gate/trigger/release, envelope
completion, read positions and grain overflow. Components store their name, version, child graph,
parameters and optional panel as data, with independent state for every prepared instance.
