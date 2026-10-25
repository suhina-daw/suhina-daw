---
description: A data-only versioned graph format and Rust SDK for community-authored Suhina effects
read_when: .sd, Suhina device SDK, device graph, community device, graph effect
type: decision
status: current
last_verified: 2026-10-24
covers:
  - crates/device-sdk/src/builtin.rs
  - crates/device-registry/src/lib.rs
  - crates/device-sdk/Cargo.toml
  - crates/device-sdk/src/lib.rs
  - crates/device-sdk/src/nodes.rs
  - crates/device-sdk/src/**
  - crates/device-sdk/src/graph.rs
  - crates/device-sdk/src/runtime.rs
  - crates/device-sdk/examples/gain.rs
---

# 0021: Data-only device graphs

- **Status:** accepted
- **Date:** 2026-10-15
- **Tasks:** #300, #406, #679 (device panels)

## Context

Issue #300 asks for a Rust SDK that can create a shareable `.sd` device file. The file describes
the device and signal flow as data, without embedding Rust source. Version 2 loads composable device graphs into tracks and draws their nodes and parameter
controls in the detail panel. Version 1 gain graphs remain readable.

Version 3 added optional [visual connection layouts](../spec/sd-device-format.md#visual-connection-layout):
node rectangles, wire bends and port anchors. Version 4 separates the file schema version from
the device revision and adds data-only parameter-id migrations. Versions 1 through 3 remain
readable. Version 5 adds an optional expanded panel ([amendment](#amendment-device-panels-for-built-in-devices-2026-10-19), 7);
a file is always written as version 5 ([Versions](../spec/sd-device-format.md#versions)), so old readers
reject a document they would misread instead of silently dropping metadata. Interactive graph editing remains deferred. Only supported, strictly validated
authoring fields are accepted.

## Decision

1. Every effect, instrument and MIDI effect made with the SDK is a versioned `.sd` graph with
   device metadata, role, parameters, processing nodes and graph connections. It contains no
   executable code. Suhina's built-in devices are not `.sd` graphs (item 3).
2. `suhina-device-sdk` is a Rust crate that builds, validates, reads and writes this format and
   prepares supported graphs for processing. Authors can create graphs with the Rust SDK or the
   visual editor; both preserve the same graph and editor data.
3. Heavy DSP is provided by shared native Rust blocks used by graphs. Built-in devices remain
   native implementations with a layout (the `.sd` format's UI part only, no graph); they are not
   rewritten as primitive graphs and not wrapped in one. Shared node implementations live under `crates/device-sdk/src/nodes/` and use
   `crates/dsp`; a heavy implementation may move to its own crate when size or independence
   warrants it.
   How their layouts are made, stored and shown is the
   [device panels amendment](#amendment-device-panels-for-built-in-devices-2026-10-19).
4. Version 2 supports sample-rate stereo signal graphs: branching, explicit input/output ports,
   arithmetic, channel operations, modulation, filters, envelope detection and delays. Delayed
   input edges break feedback cycles; undelayed cycles are rejected. MIDI-effect graphs are a
   single event chain with sample-offset-preserving MIDI nodes; instrument graphs receive MIDI
   events and render audio with MIDI value source nodes. Version 1 linear gain files stay readable.
   The [node reference](../spec/sdk-nodes.md) specifies every supported node.
5. Graph validation and compilation happen off the audio thread. Prepared processing uses fixed
   graph state and does not allocate. Validation caps file size, nodes, parameters, connections and total delay history.
   Delay history is budgeted at 384 kHz so accepted files fit every supported sample rate.
6. `device.addSd` loads a validated effect onto a track according to its role. The project embeds
   the graph JSON in the device state, independent of the source file (a built-in device's layout
   is not embedded: [device panels amendment](#amendment-device-panels-for-built-in-devices-2026-10-19)).
   The engine compiles it while building the renderer and swaps graphs through the existing edit
   path.
7. Parameter declarations generate slider, logarithmic slider, integer and toggle controls in the device detail panel. The panel also
   draws the graph's visible nodes as a read-only signal-flow view. SDK authors can hide
   implementation nodes with `visible: false`; hidden nodes still run. Editing node connections in
   the UI is deferred. Panels label the graph's role as `AudioEffect`, `MIDI Effect` or `Instrument`.
8. Device ids remain stable across revisions. `device.version` is separate from the file schema
   version; each revision bump declares a migration from the previous revision. `device.updateSd`
   explicitly replaces an embedded graph only when id and role match, then migrates parameter values
   as part of one undoable command. Projects do not silently adopt a newer file.

## Consequences

- Rust authors may use the additive `author::GraphBuilder` and standard `CustomControl`
  constructors instead of manipulating vectors directly. Builder handles are scoped to one
  append-only graph, connection errors are reported during authoring, and `finish` still uses
  the existing full validator. These helpers emit the same `.sd` data; they add neither an
  executable language nor an alternative runtime. See [checked authoring](../device-sdk.md#checked-rust-authoring).
  Numeric control setters and parameter-derived formatting are likewise authoring-only;
  shared local group origins flatten to the same absolute panel bounds without serializing
  a layout tree or changing draw/focus order.

- SDK authors compose effects, MIDI effects and monophonic instruments from supported primitives and distribute one self-contained
  `.sd` file, with generated parameter UI and a read-only node list.
- Node contracts are available in the SDK's JSON catalog and the read command `device.sdNodes`
  through MCP command dispatch. The same catalog generates the node reference table;
  tests reject stale documentation. [The authoring guide](../device-sdk.md) includes AI workflows.
- The graph runs in offline and real-time chains without loading community Rust code.
- Executable custom DSP, polyphonic instruments, spectral processing, oversampling and
  graph editing remain outside this data-only effect implementation. Delay memory and node
  counts are bounded, but graph admission is not a hardware CPU deadline guarantee.


Custom UI controls and configurable parameter smoothing are described in the
[format specification](../spec/sd-device-format.md). Generate a gallery with
`cargo run -p suhina-device-sdk --example ui_controls -- target/sdk-examples/ui-controls.sd`.

Parameter UI metadata also supports bounded cell size/position, RGB theme overrides, font size
and family, shape and label placement. The renderer scrolls the authored canvas, and the SDK
rejects non-finite or excessive geometry before loading. The format remains data-only.

An optional data-only [custom canvas](../spec/sdk-custom-ui.md) now packages embedded PNG/JPEG
images, vector drawing expressions and parameter input bindings. Authors program these with Rust
SDK types; the file contains no executable extension. The UI caches parsed scenes and bounded
image textures, performs decoding with allocation limits, and sends edits through `device.setParam`.

## Amendment: device panels for built-in devices (2026-10-19)

Item 3 said built-in devices are wrapped by `.sd` graphs. This amendment decides that a built-in
gets only a **layout**, the `.sd` format's UI part, and how layouts are made, stored and shown
(#679). It was first written in terms of generated `.sd` wrappers; that was narrowed on
Discord to the layout part, with no graph and no `native_device` node, before any wrapper was
used for a project, and the text below says so. The `native_device` node and the bundle of
generated `.sd` wrappers that the combined build (#726, from #600) had added are removed:
`.sd` graphs have no node that runs a built-in device.

1. **A generated parameter list by default, a layout optional.** Every built-in effect,
   instrument and MIDI effect gets a parameter list generated from its `DeviceInfo` and
   `ParamInfo` ([ADR 0004](0004-built-in-devices.md) › 2), drawn with the stock `.sd` controls:
   a slider (logarithmic on a logarithmic scale), a toggle, a whole-number slider or a menu of a
   choice's labels, in parameter order, in 260 px columns that fill the compact panel's height
   from left to right. A layout replaces it
   when the device has one. No built-in lacks a panel for want of a layout, and the generated list
   is what shows when a layout fails to load. A plug-in gets the same list from the parameters
   its host lists (hidden and read-only ones left out), under its editor button while its editor
   is not in the panel.
   *Amended 2026-10-25, see Amendment: a long parameter list continues to the right.*
2. **Parameters come from the native device.** A layout refers to parameters by their stable ids
   and takes name, range, scale, unit and default from `ParamInfo`; it never declares them again.
   It may only say how one is presented (its control and the `.sd` parameter `ui` fields), and a
   control its range does not allow is refused. A test fails when a layout refers to a parameter
   id its device does not have.
3. **A project stores the id, not the layout.** A built-in stays `{ "type": "builtin", "id" }`
   with its parameter values ([suhina-format.md](../spec/suhina-format.md)), and its panel is
   loaded by id from the running Suhina. Item 6 (embedding the graph) applies to `.sd` files
   only. An embedded copy would freeze the parameter list at the time the device was added and
   go wrong when the device later changes; embedding exists so a project works without its
   source file, and a built-in always ships with Suhina. The project format and the `.als`
   import mapping do not change, panel fixes reach old projects without `device.updateSd`, and
   the sound of an old project stays the same. The `.sd` wrappers of built-ins (a graph around
   one `native_device` node) existed only in test builds of #600 and are not read: a wrapper file
   is refused, and no conversion is kept for a project such a build saved.
4. **DSP stays native.** The layout adds nothing to the audio path.
5. **The compact panel has a fixed height and a free width.** Every device has a compact panel
   for the device chain, at most 168 px high: the detail area's default 200 px less the 20 px
   header and 6 px padding above and below. Its width is the author's, 260 px by default, and
   the generated list is always 260 px wide. The row layout already makes a device as wide as
   its canvas plus 12 px (220 px at least, [sdk-custom-ui.md](../spec/sdk-custom-ui.md)); the
   stacked column's default width is 272 px so a 260 px panel fits it. A smaller detail area or
   column scrolls the panel. A longer list continues in another 260 px column to the right and
   the panel widens; stock controls a canvas leaves shown sit beside it the same way. No zoom:
   plug-in editors are fixed-size too, and ADR 0006 › 10 already lays the panel out around them.
   *Amended 2026-10-25, see Amendment: a long parameter list continues to the right.*
6. **An expanded panel is optional.** A device may also have one larger fixed-size panel with
   more controls and larger displays (an equaliser's curve, a sampler's waveform), opened with a
   button in the device's header; a device without one has no button. It opens over the main
   area, which the design brief's "One window, no pop-ups" allows as it is, and closes with its
   close button, Escape, a second click on the header button, or when its device is no longer
   shown. A window of its own, with **Window** and **Dock** as the sample map has
   ([sample-map.md](../spec/sample-map.md)), may follow; the pull request that adds it adds that
   window to the brief's exceptions. Both panels change the same parameters through
   `device.setParam`, so automation, presets and undo do not depend on which is open.
7. **Format version 5.** The expanded panel is the optional field `device.expanded` in `.sd`
   file version 5, and `device.ui` is the compact panel. Older readers reject version 5, as they
   reject version 4 now, rather than drop the field on saving; every file is written as version 5,
   with or without an expanded panel ([Versions](../spec/sd-device-format.md#versions)). Built-in layouts never meet an older Suhina, since they are loaded
   by id from the Suhina that ships them.
8. **The browser stays by role.** Instruments and Effects list built-in devices,
   `.sd` files and plug-ins together by role, as [browser.md](../spec/browser.md) describes; a
   built-in keeps its key `device:<id>`, so pins and Recent still find it. `.sd` files of every
   role are listed, not only audio effects.
9. **Where layouts are kept.** A device crate gives its layout in Rust with the SDK's UI types
   (the Sampler's `layout()`), or as `devices/<name>/panel.json` (a `Layout` in JSON) listed in
   the registry. `suhina_device_registry::panel(id)` loads it once per run.
10. **Not now:** layouts by others for a built-in device (#617), pages of a fixed number of
    controls for devices with many parameters, and a mark for parameters that cannot be
    automated (#669), which show and edit like the others meanwhile.

## Amendment: a long parameter list continues to the right (2026-10-25)

A generated list longer than the compact panel's 168 px scrolled inside it, so a device with
many parameters (Utility, the FM synth, most plug-ins) showed a few rows at a time and its
controls slid away under the pointer. Items 1 and 5 of the amendment above change in place: the
list fills columns 260 px wide, each as tall as the compact panel (less a plug-in's rows and the
graph's node rows), from left to right, and the device's panel is as wide as its columns. Stock
controls a canvas leaves shown go in the same columns to the canvas's right instead of under it.
The chain already scrolls sideways, so a wide device needs nothing new; a detail area smaller
than the panel still scrolls it, and the expanded panel is unchanged. Cells an `.sd` file places
itself keep their position.
