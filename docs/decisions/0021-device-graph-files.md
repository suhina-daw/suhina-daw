---
description: A data-only versioned graph format and Rust SDK for community-authored Suhina effects
read_when: .sd, Suhina device SDK, device graph, community device, graph effect
type: decision
status: current
last_verified: 2026-10-15
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
- **Tasks:** #300, #406

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
   the graph JSON in the device state, independent of the source file. The engine compiles it while
   building the renderer and swaps graphs through the existing edit path.
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
