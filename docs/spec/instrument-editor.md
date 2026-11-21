---
description: Instrument's compact stock panel and expanded Play, Model, Sample and Analysis editor, sharing native controls, UI-only page navigation and dimming of controls the Source leaves without effect
read_when: changing the Instrument panel, expanded instrument editor, or local SDK page navigation
type: spec
status: current
last_verified: 2026-11-20
covers:
  - devices/instrument/src/panel.rs
  - devices/instrument/src/editor.rs
  - devices/instrument/src/editor_bodies.rs
  - devices/instrument/src/editor_pages.rs
  - crates/ui/src/detail/device_canvas.rs
---

# Instrument editor

The editor presents the experimental [Instrument](instrument.md) implemented in #1590.
It does not claim a calibrated acoustic family, automatic fitting, a preset library, effects
or per-note expression that the current core does not implement. This is the presentation
slice #1597 of #1579, designed from the maintainer's requested interactions and Suhina's SDK.

## Compact panel

The lower device area (620 × 168) presents, from top to bottom:

- Along the top, the Suhina logo and, for revisions 1 and 3, Body as a menu at the left; at the
  right **Source**, which moves to the next of Modal, Sample and Hybrid when it is clicked, and
  **Editor ›**, which opens the expanded editor on the page it showed last. The panel has one
  Open editor button for each editor page, each shown only while the editor's page (`#page`,
  shared by both canvases) is that page and expanding to it, so it reads as one button. The
  standard device-header expand button opens the same editor. A hairline runs under the row.
- At the left, the body's sound as `Strands` (the Model page's ribbons, smaller) with Modal, the
  local sample's waveform and drop target with Sample, and both with Hybrid, the ribbons fainter
  under the waveform; under the waveform the sample's root as a note name. With Modal the
  waveform and its drop target are not shown; a sample is loaded on the editor's Sample page or
  after switching Source.
- At the right, six dials with the editor's small machined faces: Hardness (Touch for Kalimba),
  Decay (Body decay), Brightness, Release, Blend and Gain.
- Under the dials, the model in small capitals: struck modal (revision 1), modal profile
  (revision 2), modal bodies (revision 3) or fitted model (revision 4), each marked experimental
  with its revision. The label names the stored request, not calibration acceptance.

Hardness is dimmed with Sample, with the Struck body (which has no mallet) and with revisions 2
and 4; Decay and Brightness with Sample; Blend outside Hybrid (see Dimmed controls below).
Narrow device areas scroll instead of shrinking controls below readable sizes.

## Expanded editor

The larger canvas uses the existing centred, scrollable SDK expanded-panel path. Its header
identifies Instrument and its experimental model, with Source always available. The pages are:

- **Play**, from top to bottom:
  - The amplitude envelope (Attack, Decay, Sustain, shared by the Sample layer and fitted
    models, and Release) drawn as the page's picture: a brass line over a light fill, each stage
    as wide as its value (Attack and Decay linear in seconds, as their dials are, Release on a
    logarithmic scale), Sustain the held part's height. Its three points are dragged directly:
    the peak sideways for Attack, the knee sideways for Decay and up and down for Sustain, the
    end sideways for Release. Under it, each stage's name and value.
  - **Key range**: a column for each of the 128 keys, the playable ones lit and the others
    faded, middle C (C3) named, and while the sample plays its root marked. The lowest and
    highest keys are dragged by the dots at the ends of the brass bar above the keys; their
    note names stand at the right.
  - **Velocity**: a wedge rising from soft to hard with the admitted part lit, its two ends
    dragged as lines; the lowest and highest velocity stand at the right as values that are
    dragged too.
  - A hairline, then one even row of dials as on the Model page: Gain, Blend (the Hybrid sample
    blend), Attack, Decay, Sustain and Release. Release is the damper for fitted models and the
    fade otherwise.
  - A **Soft pedal** switch beside the page buttons, shown on every page as the Source menu is
    (una corda for fitted models, the same as controller 67 held down). Its tooltip also gives
    the keyboard's facts: C3 is key 60, the pitch wheel bends ±2 semitones and the sustain
    pedal holds keys.
- **Model:** what it shows depends on the model revision, and every control applies to the
  next strike.
  - Revisions 2 and 4 (an accepted profile or a fitted set) show a Struck body group: Body decay,
    Brightness, Structure and Strike color, a creative weighting of accepted modes, not a
    measured physical impact position. Profile adoption resets these four controls and enables
    all profile modes; the underlying stable parameter IDs remain the same. Revision 4 adds a
    Fitted model group: Source morph, Velocity to brightness, Damper noise, Pedal resonance
    and Model tune (semitones, for a model fitted at the wrong root or slightly off pitch; the
    Sample layer is tuned with Root).
  - Revisions 1 and 3 show the bodies' page, from top to bottom:
    - The bodies as text tabs, Struck, Marimba, Vibraphone, Xylophone, Glockenspiel and Kalimba
      ([bar bodies](instrument.md#bar-bodies-revision-3)), which set Body; the one playing is
      underlined in brass.
    - Across the whole width, behind the controls, `Strands`
      ([sdk-custom-ui.md](sdk-custom-ui.md) › Reusable functional widgets) shows the body's
      sound: its ribbons flow at rest, a note's modes (the Instrument's live values,
      [instrument.md](instrument.md) › Live values for the panel) lift them and raise a ribbon
      each, and a strike throws sparks out of the center. Width sets how far they spread, Mallet
      weight how wide they are, Hardness how fast they flow, and Resonator how strongly they show.
    - At its center the page's one large dial, Hardness (Touch for Kalimba, Brightness for
      Struck, which has no mallet): a machined ivory knob on an espresso rim inside a scale of
      ticks, with a brass arc and pointer, its name and value inside it, the sound fullest around
      it.
    - Under it, the body's name and a line on it at the left, and **Advanced ›** at the right,
      which opens the Advanced page; then a hairline.
    - One even row of dials: Decay, Brightness, Strike, Weight (Finger for Kalimba), Resonator
      (Box for Kalimba) and Width; Struck has Decay, Strike and Structure.
    - At the bottom right, Mallet model as two choices, Contact and Pulse, the chosen one
      underlined in brass; Kalimba shows Plucked instead, and Struck nothing.
  - **Advanced** (revisions 1 and 3): Tuning, Ring and Torsion as larger dials with a line each on
    what they do (a tine has no Torsion; Struck has Structure alone), then Mallet model and Modes
    per voice as choices. **‹ Model** goes back. The sound shows at about half strength behind it.
  - Revisions 2 and 4 show Modes per voice on the Model page.
- **Sample**, from top to bottom:
  - The local sample's waveform and drop target as the page's picture, across the page. Its
    markers are dragged on it: the source's Start and End, or the loop's with Loop playback.
    With no sample it invites a drop. Above it Start and End, under it Loop start and Loop end,
    as values that are dragged too, so all four stay reachable with any Playback.
  - The name line, **Sample**, and a line on dropping a file with the sample's root as a note
    name (60 is C3), and **Analysis ›** at the right, which opens the Analysis page; then a
    hairline.
  - One even row of dials: Root (as a note name), Attack, Decay, Sustain and Release.
  - At the bottom right, Playback as three choices, Gate, One shot and Loop, the chosen one
    underlined in brass, as the mallet model sits on the Model page.

  A source imported here uses `device.loadSample`, including the same Source automation
  override and one-step undo as the compact panel.
- **Analysis** (page 4, from **Analysis ›**): the host's local sample analysis
  (`SampleAnalysis`): the root it proposes, with its correction and explicit acceptance as one
  undoable change, and fitting a model from the sample. **‹ Sample** goes back. Nothing changes
  until a proposal is accepted.

## Dimmed controls

A control that has no effect in the current Source is drawn faded, in both canvases: a veil of
the canvas's own background lies over it at about 55 % strength. The veil takes no input, so the
control still takes edits and a value can be prepared before Source changes; dimming changes no
parameter, automation or saved state. The sample layer's controls (Root, the waveform, Start,
End, the loop and Playback) are dimmed with Modal; the amplitude envelope (Attack, Decay,
Sustain, on the Play and Sample pages) with Modal, except for fitted models (revision 4), which
it shapes; the modal body's Decay, Brightness and the mallet with Sample; Blend outside Hybrid.
Gain, Release, Soft pedal and the key and velocity ranges act on both layers and are never
dimmed. The veil draws no tooltip, so a dimmed control does not say why.

All sound controls use the existing stable parameter IDs and `device.setParam`, including
gesture coalescing, automation overrides, reset and undo/redo. All sound parameters are
reachable; `model_revision` remains hidden persisted metadata. Both presentations read the
same project values rather than keeping local copies of sound controls.

## Navigation and host contract

The built-in SDK reserves `#page` as a bounded UI-only selector, initially zero and at most
15; the editor uses 0 Play, 1 Model, 2 Sample, 3 Advanced and 4 Analysis. A native built-in canvas may bind a button's `Set` or `Expand` gesture to it and read it
in expressions such as `visible_when`. Other gestures/bindings are rejected. It is not a
DeviceInfo parameter, is never sent to the engine, is never automated, and does not create
an undo entry or dirty the project. The host keeps it in temporary view memory keyed by
project, track and complete device address, shared by that device's compact and expanded canvases.
`Expand` selects the requested page and opens the existing editor. A repeated selection
keeps that page. Project reload starts with the host's ordinary fresh view state.
Enter and Space activate a focused page button. Focused SDK controls in an expanded editor
retain their keyboard focus across frames, so page keys and parameter arrows do not reach the
Launcher behind it. Escape closes the editor unless a text field is being edited. Closing it
or focusing another part of the application restores ordinary shortcut handling.

This host binding is initially available to built-in layouts only; `.sd` version 6's
parameter-binding contract is unchanged. A future standalone SDK editor can implement
the same local state without modifying the DSP or exported parameter list.

## Appearance and validation

Controls, labels, focus and selection use native SDK widgets and
Suhina typography. The page buttons sit at the bottom left of every page, as text. With every
theme, both canvases draw the maintainer's warm ivory artwork behind every control
(`devices/instrument/panel/`, with its provenance): a surface with its relief blurred to soft
folds and no logo of its own, all of it behind the expanded editor (880 × 560) and a strip of
it, a little below its middle, behind the compact panel. The Suhina logo stands at the
editor's top center, over "Modal bodies · experimental" in small spaced capitals, in place of
the Instrument title, and at the compact panel's top left. The Play and Sample pages share the
Model page's grammar: the page's picture across the top, its name line, a hairline at the same
height and one even row of dials under it, choices at the bottom right, captions in small
spaced capitals and brass for what is dragged. Its own style sheet makes its
buttons text: the page buttons, the body tabs and the choices, the chosen one underlined or
marked in brass. Its dials have machined faces of the editor's own (an ivory dome with a brushed
sheen on an espresso rim), with brass value arcs. The look is the same with every theme: the canvases' own style sheet
gives every control espresso ink and brass, and the drawings use colors of their own rather
than the theme's palette, so a dark theme's light text never lands on the ivory. Narrow windows
scroll the full editor.

Tests validate every binding and hidden parameter, local-page bounds and malformed layouts,
keyboard/click navigation without project actions, parameter command parity, sample target
handling, and representative compact/expanded pages in every built-in theme. An automated
render and a rebuilt app support inspection without Computer Use. Listening is a separate
quality gate for the sound engine and is not implied by UI tests.

Family, library, MPE and effects pages remain explicit future work in #1579.
The shared sample UI currently prepares waveform sources and drop addresses for top-level
devices only; nested sample panels remain a separate host-lifecycle limitation.
