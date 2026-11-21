---
description: Instrument's compact stock panel and expanded Play, Model and Sample editor, sharing native controls and UI-only page navigation
read_when: changing the Instrument panel, expanded instrument editor, or local SDK page navigation
type: spec
status: current
last_verified: 2026-11-20
covers:
  - devices/instrument/src/panel.rs
  - devices/instrument/src/editor.rs
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
identified as experimental struck modal revision 1 or modal profile revision 2, separately from
its sound controls. The revision label names the stored request, not calibration acceptance.
The panel fits the standard 168-point body height and uses less width than the initial
900-point panel. Narrow device areas scroll instead of shrinking controls below readable sizes.

## Expanded editor

The larger canvas uses the existing centred, scrollable SDK expanded-panel path. Its header
identifies Instrument and its experimental model, with Source always available. The pages are:

- **Play:** the shared Gain and Release, Hybrid sample blend, the amplitude envelope (Attack,
  Decay, Sustain) shared by the Sample layer and fitted models, key/velocity admission ranges,
  and a **Soft pedal** switch beside the page buttons, shown on every page as the Source menu
  is (una corda for fitted models, the same as controller 67 held down). Release is the damper for fitted models and the fade otherwise.
- **Model:** Body decay, Brightness, Structure, Strike position and Modes per voice. These
  describe the actual struck-modal model, and apply to the next strike. Revision 2 labels the
  excitation control Strike colour: a creative weighting of accepted modes, not a measured
  physical impact position. Profile adoption resets the four model controls and enables all
  profile modes; the underlying stable parameter IDs remain the same. Revision 4 (fitted models)
  adds a Fitted model group: Source morph, Velocity to brightness, Damper noise, Pedal
  resonance and Model tune (semitones, for a model fitted at the wrong root or slightly off
  pitch; the Sample layer is tuned with Root).
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

Controls, labels, focus and selection use native SDK widgets, theme palette tokens and
Suhina typography. With a light theme (a variant whose sheet declares a light colour scheme),
both canvases draw the maintainer's warm ivory artwork behind every control: a cream surface
with soft relief waves and the Suhina logo at the bottom right
(`devices/instrument/panel/`, with its provenance). The expanded editor is 880 × 560, its last
40 points free of controls so the logo stays clear; the compact panel's bottom-right label
stops short of it. A dark theme hides the artwork (the panel's own style sheet), since its
light text would not read on it, and keeps its own background. Text and controls stay native
over the artwork. The canvas uses a quiet centre with grouped controls, distinct primary
controls and explicit source limitations. Narrow windows scroll the full editor.

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
