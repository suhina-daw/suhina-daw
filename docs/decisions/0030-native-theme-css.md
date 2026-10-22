---
description: Advanced native CSS theming, the style and layout engines to evaluate, and preservation of authored rules in JSON
read_when: modern CSS, Stylo, Taffy, Lightning CSS, Blitz, native CSS architecture, CSS JSON synchronization
type: decision
status: draft
last_verified: 2026-10-22
covers:
  - crates/ui/src/kit/native_css.rs
  - crates/ui/src/kit/native_css/paint.rs
  - crates/ui/src/kit/native_css/geometry.rs
  - crates/ui/src/kit/native_css/shadows.rs
  - crates/ui/src/kit/native_css/workspace.rs
  - crates/ui/src/kit/native_css/diagnostics.rs
  - crates/ui/src/kit/theme_css.rs
  - crates/ui/src/kit/style_class.rs
  - crates/ui/src/kit/skin.rs
  - apps/suhina/src/theme_files.rs
---

# 0030: Native theme CSS

## Goal

A theme may control every UI section through stable classes, including advanced visual
styles and layout. CSS and JSON are two editable representations of the same theme.
This is an explicit product requirement, not satisfied by a palette importer that
accepts a handful of declarations. The native UI and the audio engine remain separate.

## Research, 22 October 2026

Primary sources:

- [Stylo](https://github.com/servo/stylo): browser-grade CSS parsing, selector matching,
  cascading and computed styles. The downstream Servo implementation is MPL-2.0.
- [Taffy](https://github.com/DioxusLabs/taffy): CSS Block, Flexbox and Grid layout, designed
  for use inside other native UI libraries; MIT.
- [egui_taffy](https://github.com/PPakalns/egui_taffy): an egui adapter for Taffy. Its
  0.14.0 manifest names egui 0.36 and Taffy 0.9.2, matching the UI toolkit used here.
- [Lightning CSS](https://lightningcss.dev/): a modern CSS parser, transformer and
  serializer. Parsing a declaration is not an implementation of its runtime semantics,
  layout, animation or painting. It cannot by itself provide full CSS styling.
- [Blitz](https://github.com/DioxusLabs/blitz): integrates Stylo, Taffy and a text/paint
  stack. Its own README describes it as beta with bugs and missing features. Its
  [CSS support matrix](https://blitz.is/status/css) distinguishes implemented features,
  renderer-specific support and exclusions. It is not evidence of universal CSS support.
- [egui_css](https://docs.rs/crate/egui_css/0.1.1): targets egui 0.29 and explicitly lists
  missing layout, media queries, working advanced pseudo-classes and animations. It is
  not a suitable shortcut for this requirement.

These sources describe capabilities, not measured performance in Suhina. No third-party
source or artwork was copied into this change.

## Selected direction

The staged replacement in [ADR 0031](0031-style-sheets.md) now handles complete
sheets in the supported native paint subset. The integration described below
remains the compatibility path for other sheets, including layout, radial/conic gradients,
CSS layers and animation. Native box/text shadow painters no longer depend on
Stylo value types; this adapter converts those types at its boundary.

Use Stylo for CSS semantics and Taffy for layout, with egui retaining native controls
and command dispatch. This direction was selected for the broad customization target.
The initial adapter uses released `blitz-dom`, `blitz-html` and `blitz-traits`
0.3.0-beta.2 with default features disabled: Stylo 0.20 and Taffy 0.14. There is no
Blitz shell, networking provider, scripting engine or renderer. CSS enters through
the style API, never by interpolating it into HTML. Host-generated markup contains
only validated Suhina roles and toolbar module ids. Sequential style traversal avoids
Blitz's documented shared traversal-pool concurrency hazard.

This initial integration is experimental and still needs platform, conformance and
performance verification. The native-property CSS reader remains for the existing
palette/skin projection; the advanced source is separate and retained losslessly.

A host-maintained style tree mirrors stable Suhina objects and UI roles. Identity must
not depend on drawing order or a track's current index. Each node has a stable key,
classes, ancestry, interactive state and an optional container. Styling must not change
which project object a control edits. Layout produces rectangles used for both drawing
and hit testing, including clipping, scrolling and transformed content. Every existing
keyboard/accessibility/undo path remains supported.

The style engine owns the cascade and computed values; the layout engine owns sizes
and placement; the renderer owns gradients, shadows, transforms and compositing. The
native UI owns interactions and dispatches existing commands. Nothing in these systems
runs in the audio callback. Parse, resource decoding and expensive restyling are prepared
outside interactive frame work, with stale-result rejection and explicit budgets.

## Required capability groups

| Group | Target |
|---|---|
| Selectors and cascade | Class, id, descendant/child selectors, state pseudo-classes, specificity, inheritance, `!important`, `:is()`/`:not()`, nesting and cascade layers |
| Variables and values | Custom properties, scoped `var()` with fallbacks/cycle handling, CSS colours, lengths/percentages, `calc()`, `min()`, `max()`, `clamp()` |
| Visuals | Background images, SVG assets and control states, linear/radial gradients, borders/radii, opacity, shadows, font roles and text styling |
| Responsiveness | Viewport media conditions, reduced motion, light/dark preference, and container-size queries with real container geometry |
| Layout | Flex/Grid for host-managed panel/module containers, spacing, alignment, constraints, visibility and overflow |
| Motion | Transitions and keyframe animations with a host-supplied clock, reduced-motion behaviour and bounded repainting |
| Effects | 2D transforms with matching input coordinates; filters/compositing only when the active renderer actually implements them |
| Diagnostics | Source locations, unknown/unsupported property reporting, per-node computed-style inspection and a documented capability matrix |

This is a product target, not a claim that the current branch implements this table.
No property may be advertised as supported merely because a parser accepts it. A
renderer-specific feature needs a visible fallback and tests for both paths.

## Authored rules and JSON

Advanced CSS cannot be flattened into one palette: state rules, variables, queries and
animations would be lost. The durable JSON model must retain the authored rule structure,
expressions, source order and resource references. Computed values are a projection for a
particular viewport, state and time. They must not replace authored expressions when saved.

Changing a simple colour in CSS updates the corresponding JSON representation and the
editor. Changing it in the editor updates the appropriate authored declaration. Where
several conditional declarations can supply a value, the editor must identify the target
rule rather than silently destroying the other rules. Independently edited files merge
by stable authored rule/declaration identities; conflicting edits preserve both inputs and
produce a diagnostic. Round-trip tests cover variables, queries, states and animations,
not just literal colours.

## Current implementation and gate

The branch supplies SVG resources, configurable meters, toolbar rearrangement, portable
ZIP resources and a bounded native CSS subset with live CSS/JSON mirroring. It supplies
an initial class registry and tests for literal-colour synchronization and conflicts.
The initial Stylo adapter computes colour, typography and radii for active class
ancestries and caches bounded contexts until CSS, palette or viewport changes. The
Taffy adapter lays out the actual ordered toolbar module tree; native drawing and
hit targets use those same rectangles. Advanced authored CSS is stored verbatim in
`skin.css` and `advanced.css`, including comments, queries and source order; edits
mirror between those two representations. Conditional rules are not flattened into
unconditional palette values.

Native scopes now collect a bounded retained tree (4096 nodes, depth 16), with stable
named Session track roles (`data-object`) and state from connected egui responses.
Child/sibling matching uses this actual tree. A changed tree is styled as one snapshot;
results are applied on the next repaint, preserving native command ids and targets.
Identity remains stable for named track scopes when tracks move. Clip, slot and scene
roles expose native project identities too. Connected panels pass authored colour,
font size and radius to ordinary egui widgets without creating new UI identities;
unrelated CSS leaves native defaults intact. Some raw controls and complete
geometry/container-query integration are still missing.

Connected surfaces paint linear, radial and conic gradients, including repeating forms,
with bounded cached colour ramps/spatial textures and Stylo's colour interpolation.
Native surface dimensions resolve the gradient's centre, extent and radii. Token-level input budgets bound nesting and
source size. Source-line notices identify known unsupported effect/font declarations
without deleting authored source. Connected surfaces also paint bounded inset/outset
box shadows using cached distance-field textures; blur is an approximation and follows
native clipping. An optional workspace Flex/Grid adapter now provides actual browser,
main-view and detail rectangles with matching native input clips. Removing CSS layout
restores native splitters and sizes. Bars/floating-window layout, text shadows, transforms,
filters and comprehensive computed-style diagnostics
remain implementation work. Parsed declarations that the painter does not consume do
not imply support. Toolbar height remains the host's height. These limits are also
visible in the advanced CSS editor. Before enabling that engine,
verify package licences, platform builds, interaction/accessibility parity, malformed
inputs, large projects and both rendering backends. A full browser-CSS claim is out of
scope unless an explicit support matrix and conformance evidence establish it.

The retained document now persists across native state changes; pre-mutation snapshots
let Stylo run transitions rather than restarting them. The egui frame clock advances
keyframes/transitions for connected paint properties, with bounded repaint scheduling
and a final repaint after completion. The global animation setting and high contrast
force CSS motion off. Geometry animation and a real reduced-motion media feature remain
unconnected; structural scope changes rebuild the document and restart animations.
