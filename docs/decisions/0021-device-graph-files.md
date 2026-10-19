---
description: A data-only versioned graph format and Rust SDK for community-authored Suhina effects
read_when: .sd, Suhina device SDK, device graph, community device, graph effect
type: decision
status: current
last_verified: 2026-10-19
covers:
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
readable; all writes use version 4 so old readers reject the new document instead of silently
dropping metadata. Interactive graph editing remains deferred. Only supported, strictly validated
authoring fields are accepted.

## Decision

1. Every Suhina effect and instrument is a versioned `.sd` graph with device metadata, role,
   parameters, processing nodes and graph connections. It contains no executable code.
2. `suhina-device-sdk` is a Rust crate that builds, validates, reads and writes this format and
   prepares supported graphs for processing. Authors can create graphs with the Rust SDK or the
   visual editor; both preserve the same graph and editor data.
3. Heavy DSP is provided by shared native Rust blocks used by graphs. Existing built-in devices
   remain native implementations wrapped by `.sd` graphs; they are not rewritten as primitive
   graphs. Shared node implementations live under `crates/device-sdk/src/nodes/` and use
   `crates/dsp`; a heavy implementation may move to its own crate when size or independence
   warrants it.
   How the wrappers are made, stored and shown is the
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
   the graph JSON in the device state, independent of the source file (a built-in device's own wrapper
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

- SDK authors compose effects, MIDI effects and monophonic instruments from supported primitives and distribute one self-contained
  `.sd` file, with generated parameter UI and a read-only node list.
- Node contracts are available in the SDK's JSON catalog and the read command `devices.sdNodes`
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

Stock devices ship as generated, validated `.sd` definitions with their parameter
contracts and SDK panels. The `native_device` node wraps a trusted registered
implementation; it cannot load executable code or participate in arbitrary primitive
graphs. The host verifies its role and parameter contract before preparation.
See [stock bundles](../spec/stock-device-bundle.md).

## Amendment: device panels for built-in devices (2026-10-19)

The stock bundle above currently supplies native wrappers and SDK panels. This amendment
decides the target parameter and panel model (#679); that follow-up replaces the wrappers.
Projects already store built-ins by id without an embedded wrapper.

1. **A generated wrapper by default, a hand-made one optional.** Every built-in effect and
   instrument gets a wrapper generated from its `DeviceInfo` and `ParamInfo`
   ([ADR 0004](0004-built-in-devices.md) › 2), which lays out its parameters with the stock
   `.sd` controls. A hand-made `.sd` panel replaces it when the device has one. No built-in is
   missing from the browser for want of a panel, and the generated panel is what shows when a
   hand-made one fails to load.
2. **Parameters come from the native device.** A wrapper refers to parameters by their stable
   ids and takes name, range, scale, unit and default from `ParamInfo`; it never declares them
   again. A test fails when a wrapper refers to a parameter id its device does not have.
3. **A project stores the id, not the wrapper.** A built-in stays `{ "type": "builtin", "id" }`
   with its parameter values ([suhina-format.md](../spec/suhina-format.md)), and its panel is
   loaded by id from the running Suhina. Item 6 (embedding the graph) applies to `.sd` files
   only. An embedded copy would freeze the parameter list at the time the device was added and
   go wrong when the device later changes; embedding exists so a project works without its
   source file, and a built-in always ships with Suhina. The project format and the `.als`
   import mapping do not change, panel fixes reach old projects without `device.updateSd`, and
   the sound of an old project stays the same. A copy of a built-in's wrapper that a project already
   carries is ignored on reading and left out on saving; its parameter values are kept by id.
4. **DSP stays native.** The wrapper adds nothing to the audio path.
5. **The compact panel has a fixed height and a free width.** Every device has a compact panel
   for the device chain, a canvas 168 px high: the detail area's default 200 px less the 20 px
   header and 6 px padding above and below. Its width is the author's, 260 px by default, and
   the generated wrapper is always 260 px wide. The row layout already makes a device as wide as
   its canvas plus 12 px (220 px at least, [sdk-custom-ui.md](../spec/sdk-custom-ui.md)); the
   stacked column's default width becomes 272 px so a 260 px panel fits it. A smaller detail
   area or column scrolls the panel, as now. No flexible or dynamic layout: plug-in editors are
   fixed-size too, and ADR 0006 › 10 already lays the panel out around them.
6. **An expanded panel is optional.** A device may also have one larger fixed-size panel with
   more controls and larger displays (an equaliser's curve, a sampler's waveform), opened with a
   button in the device's header; a device without one has no button. It opens over the main
   area first, which the design brief's "One window, no pop-ups" allows as it is. A window of
   its own, with **Window** and **Dock** as the sample map has
   ([sample-map.md](../spec/sample-map.md)), may follow; the pull request that adds it adds that
   window to the brief's exceptions. Both panels change the same parameters through
   `device.setParam`, so automation, presets and undo do not depend on which is open.
7. **Format version 5.** The expanded panel is a new optional field in `.sd` file version 5,
   and the current `ui` becomes the compact panel. Older readers reject version 5, as they reject
   version 4 now, rather than drop the field on saving. Built-in wrappers never meet an older
   Suhina, since they are loaded by id from the Suhina that ships them.
8. **The browser stays by role.** Instruments and Effects list built-in devices,
   `.sd` files and plug-ins together by role, as [browser.md](../spec/browser.md) describes; a
   built-in keeps its key `device:<id>`, so pins and Recent still find it. `.sd` files of every
   role are listed, not only audio effects.

Not decided here, and left to #679: how the generated wrapper arranges many parameters (a list,
a grid, pages), a zoom factor for panels, wrappers made by others for a built-in device's DSP,
the generated panel for a plug-in whose editor cannot open, and where Suhina's own wrappers are
kept in the repository.
