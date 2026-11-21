---
description: Instrument's compact stock panel and expanded editor of two layers (body and sample) with their keys page and a strip of shared controls, sharing native controls, UI-only page navigation and dimming of controls the Source leaves without effect
read_when: changing the Instrument panel, expanded instrument editor, or local SDK page navigation
type: spec
status: current
last_verified: 2026-11-21
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

## Layers

The Instrument plays two layers: the **body** (the modal model, [instrument.md](instrument.md))
and the **sample** (the local file). Source says which play: Modal the body alone, Sample the
sample alone, Hybrid both, mixed by Blend. Both canvases show Source as the two layers, each with
a lamp: a lit lamp is a layer that plays. Clicking a layer's lamp turns it off while both play
(the other then plays alone) and on while it is off (both then play); the one layer that plays
alone cannot be turned off, so its lamp is only drawn. Every change is `device.setParam` on
Source, one undo step.

## Compact panel

The lower device area (620 × 168) presents, from top to bottom:

- Along the top, the Suhina logo and, for revisions 1 and 3, Body as a menu at the left; at the
  right the two layers, **Body** and **Sample**, each a lamp and its name that turn it on or off,
  and **Editor ›**, which opens the expanded editor on the page it showed last. The panel has one
  Open editor button for each editor page, each shown only while the editor's page (`#page`,
  shared by both canvases) is that page and expanding to it, so it reads as one button. The
  standard device-header expand button opens the same editor. A hairline runs under the row.
- At the left, the body's sound as `Strands` (the body layer's ribbons, smaller) with the body
  layer on, the local sample's waveform and drop target with the sample layer on, and both with
  both, the ribbons fainter under the waveform; under the waveform the sample's root as a note
  name. With the sample layer off the waveform and its drop target are not shown; a sample is
  loaded on the editor's sample layer or after turning the layer on.
- At the right, five dials with the editor's small machined faces: Decay (Body decay),
  Brightness, Release, Blend and Gain.
- Under the dials, the model in small capitals: struck modal (revision 1), modal profile
  (revision 2), modal bodies (revision 3) or fitted model (revision 4), each marked
  experimental. The label names the stored request, not calibration acceptance.

Decay and Brightness are dimmed with the body layer off, Blend with one layer on (see Dimmed
controls below). Narrow device areas scroll instead of shrinking controls below readable sizes.

## Expanded editor

The expanded editor (880 × 500) opens as a window over the workspace
([sdk-custom-ui.md](sdk-custom-ui.md) › Expanded panels). On every page it shows:

- Along the top, the Suhina logo at the center over the model in small spaced capitals ("Modal
  bodies · experimental"), and under them the two layers: each one's lamp, which turns it on or
  off (see Layers), and its name, which shows it and is underlined in brass while its layer
  shows, the Advanced page and the keys over it included.
- At the top right, **Keys**: the playable range (lowest and highest key as note names) with a
  chevron, which opens the keys page over the layer shown and closes it again.
- Along the bottom, after a hairline, the strip of what acts on both layers: **Blend** as a
  slider from Body to Sample with its value at the left (only with both layers on; it says "one
  layer on" otherwise and is dimmed), then **Release** and **Gain** as small knobs with their
  name and value beside them. Release is the damper for fitted models and the fade otherwise.

The pages (`#page`) are:

- **Body layer** (page 0, where the editor starts). What it shows depends on the model
  revision, and every control applies to the next strike.
  - Revisions 1 and 3 show, from top to bottom:
    - The bodies as text tabs, Struck, Marimba, Vibraphone, Xylophone, Glockenspiel and Kalimba
      ([bar bodies](instrument.md#bar-bodies-revision-3)), which set Body; the one playing is
      underlined in brass. Each tab's tooltip is the body's line.
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
    - Under it one row: the body's name in spaced capitals with its line beside it, Mallet model
      as two choices, Contact and Pulse, each with a dot before it (brass and filled for the
      chosen one; Kalimba shows Plucked instead, and Struck nothing), and **Advanced ›** at the
      right, which opens the Advanced page; then a hairline.
    - One even row of dials: Decay, Brightness, Strike, Weight (Finger for Kalimba), Resonator
      (Box for Kalimba) and Width; Struck has Decay, Strike and Structure.
  - Revisions 2 and 4 (an accepted profile or a fitted set) show a Struck body group: Body decay,
    Brightness, Structure and Strike color, a creative weighting of accepted modes, not a
    measured physical impact position. Profile adoption resets these four controls and enables
    all profile modes; the underlying stable parameter IDs remain the same. Revision 4 adds a
    Fitted model group: Source morph, Velocity to brightness, Damper noise, Pedal resonance and
    Model tune (semitones, for a model fitted at the wrong root or slightly off pitch; the
    sample layer is tuned with Root). Modes per voice follows as a menu.
- **Advanced** (page 2, revisions 1 and 3): Tuning, Ring and Torsion as larger dials with a line
  each on what they do (a tine has no Torsion; Struck has Structure alone), then Mallet model and
  Modes per voice as choices. **‹ Body** goes back. The sound shows at about half strength
  behind it.
- **Sample layer** (page 1), from top to bottom:
  - At the left the loaded file's name in spaced capitals and its facts under it (sample rate,
    mono or stereo, length), or "No sample" (`SampleFile`); at the right Playback as four
    choices, Gate, One shot, Loop and Sustain ([sampler.md](sampler.md) › Sustain playback),
    each with a dot before it.
  - The local sample's waveform and drop target across the page, its times along its bottom. Its
    markers are dragged on it: the source's Start and End, or the loop's with Loop playback.
    With no sample it invites a drop. Under it Start at the left, the loop's two ends in brass
    between, and End at the right, as values that are dragged too, so the region and the loop
    stay reachable with any Playback. Then a hairline.
  - At the left **Envelope**: Attack, Decay and Sustain as small knobs with their name and value
    beside them, shared by the sample layer and fitted models.
  - Past a short upright hairline, **Root key** as a large note name that is dragged up and
    down, and the host's local sample analysis (`SampleAnalysis`) beside it: under
    **Suggested**, the root the analysis proposes between ‹ and › that correct it, with
    **Accept** (one undoable change) and its cents and frequency (or a loop's key) under it;
    **Again** analyzes once more; **Fit model** and **Fit folder…** fit a model from the sample
    or a folder of notes, and **Discard** drops a proposal. Nothing changes until a proposal is
    accepted. While a fitted sound waits for acceptance it covers the envelope and the root,
    drawn on the page's own surface and taking the pointer, so the controls under it wait until
    the result is settled:
    what was fitted in a few words (its partials, the root and how close it is to the
    recording, the measurements in the tooltip), **Play original**, **Play fitted** and
    **Discard**, and how the body layer uses it, in words that say so: **Use as model** (or
    **Replace model** when it has one), **Use each note** for a chord, **Add to model** and
    **Add as another instrument**. The card's bottom line says what the button pointed at
    does; until then, to listen first and then choose. The text
    takes the page's espresso ink and the buttons are set in spaced capitals, Accept underlined
    in brass.

  A source imported here uses `device.loadSample`, including the same Source automation
  override and one-step undo as the compact panel.
- **Keys** (page 3 over the body layer, page 4 over the sample layer, so **Done** and Keys go
  back to the layer it came from): on a ground of the surface over the layer, the title and
  **Done**, then:
  - **Key range**: a column for each of the 128 keys, the playable ones lit and the others
    faded, middle C (C3) named, and while the sample plays its root marked. The lowest and
    highest keys are dragged by the dots at the ends of the brass bar above the keys; their
    note names stand at the right.
  - **Velocity**: a wedge rising from soft to hard with the admitted part lit, its two ends
    dragged as lines; the lowest and highest velocity stand at the right as values that are
    dragged too.
  - After a hairline, the **Soft pedal** switch (una corda for fitted models, the same as
    controller 67 held down), **Legato glide** and **Glide time** (seconds; a fitted note played
    while another key is held glides from that key's pitch, [modal
    profiles](instrument-modal-profile.md) › Glide), and a line with the keyboard's facts: C3 is
    key 60 and the pitch wheel bends ±2 semitones; the Soft pedal's tooltip adds that the sustain
    pedal holds keys.

A layer's page while that layer is off shows a note over its picture, on a card: that the layer
is off, and **Turn it on**, which plays both layers. Its controls stay, faded, so values can be
prepared.

## Dimmed controls

A control that has no effect in the current Source is drawn faded, in both canvases: a veil of
the canvas's own background lies over it at about 55 % strength. The veil takes no input, so the
control still takes edits and a value can be prepared before Source changes; dimming changes no
parameter, automation or saved state. The sample layer's controls (the file, Playback, the loop,
the waveform, Start, End and Root) are dimmed with the sample layer off; the envelope (Attack,
Decay, Sustain) too, except for fitted models (revision 4), which it shapes; the body's Decay and
Brightness in the compact panel with the body layer off; Blend with one layer on. Gain, Release,
Soft pedal and the key and velocity ranges act on both layers and are never dimmed. The veil
draws no tooltip, so a dimmed control does not say why.

All sound controls use the existing stable parameter IDs and `device.setParam`, including
gesture coalescing, automation overrides, reset and undo/redo. All sound parameters are
reachable; `model_revision` remains hidden persisted metadata. Both presentations read the
same project values rather than keeping local copies of sound controls.

## Navigation and host contract

The built-in SDK reserves `#page` as a bounded UI-only selector, initially zero and at most
15; the editor uses 0 the body layer, 1 the sample layer, 2 Advanced, and 3 and 4 the keys over the body and over the sample layer. A native built-in canvas may bind a button's `Set` or `Expand` gesture to it and read it
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
Suhina typography. With every theme, both canvases draw the maintainer's warm ivory artwork
behind every control (`devices/instrument/panel/`, with its provenance): a surface with its
relief blurred to soft folds and no logo of its own, all of it behind the expanded editor
(880 × 500) and a strip of it, a little below its middle, behind the compact panel. The Suhina
logo stands at the editor's top center, over the model in small spaced capitals, in place of
the Instrument title, and at the compact panel's top left. Captions are set in small spaced
capitals, brass marks what is dragged and what is chosen. Its own style sheet makes its buttons
text: the layers' names, the body tabs and the choices, the chosen one underlined or marked in
brass. Its dials have machined faces of the editor's own (an ivory dome with a brushed
sheen on an espresso rim), with brass value arcs. The look is the same with every theme: the canvases' own style sheet
gives every control espresso ink and brass, and the drawings use colors of their own rather
than the theme's palette, so a dark theme's light text never lands on the ivory. A window
smaller than the editor shows it scaled down to fit.

Tests validate every binding and hidden parameter, local-page bounds and malformed layouts,
keyboard/click navigation without project actions, parameter command parity, sample target
handling, and representative compact/expanded pages in every built-in theme. An automated
render and a rebuilt app support inspection without Computer Use. Listening is a separate
quality gate for the sound engine and is not implied by UI tests.

Family, library, MPE and effects pages remain explicit future work in #1579.
The shared sample UI currently prepares waveform sources and drop addresses for top-level
devices only; nested sample panels remain a separate host-lifecycle limitation.
