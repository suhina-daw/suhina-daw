---
description: Instrument's compact stock panel and expanded Play, Model and Sample editor, sharing native controls and UI-only page navigation
read_when: changing the Instrument panel, expanded instrument editor, or local SDK page navigation
type: spec
status: current
last_verified: 2026-11-20
covers:
  - devices/instrument/src/panel.rs
  - devices/instrument/src/editor.rs
  - devices/instrument/src/editor_bodies.rs
  - crates/ui/src/detail/device_canvas.rs
---

# Instrument editor

The editor presents the experimental [Instrument](instrument.md) implemented in #1590.
It does not claim a calibrated acoustic family, automatic fitting, a preset library, effects
or per-note expression that the current core does not implement. This is the presentation
slice #1597 of #1579, designed from the maintainer's requested interactions and Suhina's SDK.

## Compact panel

The lower device area presents Source, a local sample waveform/drop target, Body decay,
Brightness, Release, Hybrid sample blend and Gain. A visible Open editor button and the
standard device-header expand button open the same expanded editor. The current model is
identified as experimental struck modal revision 1, modal profile revision 2, struck and bar
bodies revision 3 or fitted model revision 4, separately from its sound controls. The revision label names the stored request, not calibration acceptance.
The panel fits the standard 168-point body height and uses less width than the initial
900-point panel. Narrow device areas scroll instead of shrinking controls below readable sizes.

## Expanded editor

The larger canvas uses the existing centred, scrollable SDK expanded-panel path. Its header
identifies Instrument and its experimental model, with Source always available. The pages are:

- **Play:** the shared Gain and Release, Hybrid sample blend, the amplitude envelope (Attack,
  Decay, Sustain) shared by the Sample layer and fitted models, key/velocity admission ranges,
  and a **Soft pedal** switch beside the page buttons, shown on every page as the Source menu
  is (una corda for fitted models, the same as controller 67 held down). Release is the damper for fitted models and the fade otherwise.
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
- **Sample:** local waveform/drop target, Root (shown as a note name, 60 is C3), Playback, source Start/End, loop Start/End,
  Attack, Decay and Sustain. A source imported here uses `device.loadSample`, including the same
  Source automation override and one-step undo as the compact panel.

All sound controls use the existing stable parameter IDs and `device.setParam`, including
gesture coalescing, automation overrides, reset and undo/redo. All sound parameters are
reachable; `model_revision` remains hidden persisted metadata. Both presentations read the
same project values rather than keeping local copies of sound controls.

## Navigation and host contract

The built-in SDK reserves `#page` as a bounded UI-only selector, initially zero and at most
15. A native built-in canvas may bind a button's `Set` or `Expand` gesture to it and read it
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
(`devices/instrument/panel/`, with its provenance). The compact panel's has soft relief waves and
the Suhina logo at the bottom right, its bottom-right label stopping short of the logo. The
expanded editor (880 × 560) has the same surface with its relief blurred to soft folds and no
logo of its own; the Suhina logo stands at its top center, over "Modal bodies · experimental"
in small spaced capitals, in place of the Instrument title. Its own style sheet makes its
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

Analysis/job controls are a coordinated later integration through the same host commands;
this layout does not display nonfunctional analysis buttons. Family, library, MPE and effects
pages remain explicit future work in #1579.
The shared sample UI currently prepares waveform sources and drop addresses for top-level
devices only; nested sample panels remain a separate host-lifecycle limitation.
