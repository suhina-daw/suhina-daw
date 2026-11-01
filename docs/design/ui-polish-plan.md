---
description: Local UI usability audit and incremental visual polish, preserving independent customization
read_when: UI polish, Studio themes, skinning, gradients, glass, panel customization
type: plan
status: draft
last_verified: 2026-10-22
covers:
  - crates/ui/src/kit/theme.rs
  - crates/ui/themes/**
---

# UI polish and customization

Local working proposal. No GitHub claims, comments or publication are authorized for this work.

## Direction

Translate the website's warm neutral surfaces, clear hierarchy and golden brand accent into a
dense music workspace. Keep the existing editing workflow. Use readable typography, quiet grid
lines and clear control groups rather than hiding frequently used tools. The initial Studio
themes are opt-in; the existing default and user themes remain available.

Appearance, layout, interaction and keymap remain independent. Preserve old theme JSON and
keymap files. New appearance properties need defaults when omitted. Avoid separate application
implementations for different profiles.

## Issue audit

Issue bodies checked on 20 October 2026; open status does not establish missing functionality.

| Area | Existing tracking | Local conclusion |
|---|---|---|
| Independent customization | #623, #625 | Extend the existing architecture proposal; no competing profile system. |
| Corner radius, font | #512, #513 | Planned features; keep separate from the color-only first slice. |
| Docking | #514 | Planned; fixed by default, opt-in movement and reset. Validate focus, drag/drop and embedded plug-in editors before a library choice. |
| Library beside piano roll | #461 | Open and assigned; preserve existing work and inspect implementation before changes. |
| MIDI tools | #349, completed #460 | Commands and visible tools already exist; audit gaps rather than reimplementing the umbrella issue. |
| Ghost notes | completed #348 | Existing mapping is preserved. Opacity, source filters and caching are separate proposals requiring measurement. |
| Shortcut customization | completed #71 | Existing capability; preserve it and test compatibility. |
| Piano roll copy drag and collisions | #582 | Interaction bugs and policy decisions, separate from visual styling. Reproduce first; collision behavior belongs to interaction settings. |
| Audio clip editing | #561 | Open and assigned; verify each requested operation independently. |
| Gradients | #515 | Existing tracking, not a new duplicate feature. |

Several open PRs touch app.rs, Arrangement headers and widgets, including #758 and #756.
Re-check their current diffs before changing those paths; a historical branch diff alone does
not prove a conflict in the intended change. Keep the first theme slice in independent files.

## Incremental implementation

1. Add opt-in Studio Light and Studio Dark palettes through the existing theme mechanism.
   Validate complete tokens, JSON loading, round-trip and readable foreground/background pairs.
   Light mode uses dark gold for accent ink because pale brand yellow is not readable on paper.
2. Establish shared appearance metrics before enlarging controls. Introduce persisted, bounded
   appearance settings with backward-compatible defaults; avoid one-off hardcoded dimensions.
3. Group the transport controls and clarify track-header hierarchy. Preserve hit targets,
   accessible names, control visibility thresholds, keyboard actions and narrow-window behavior.
4. Reproduce #582 interactions and address one bug per change with input-level tests and undo
   checks. Keep workspace/layout work independent.
5. Add panel visibility, resizing and reset through existing layout work. Restore previous
   Arrangement scroll and dimensions after focused/full-height editor mode. User state belongs
   outside the music project.

The MIDI editor should expose clip properties, its toolbar, note tools and selection context by
default where space allows. Group advanced controls inside those panels. Hiding a panel must
not disable its commands or shortcuts.

## Advanced skinning: gradients and glass

Editable panel, track and clip gradient overlays and GPU liquid/refraction are implemented.
Theme strengths can override global settings. A private user-supplied background image can
sit beneath tinted UI surfaces, which blend over it. Without that image, supported GPU hosts
can show the desktop through theme backgrounds; this uses compositor alpha, not desktop capture.
Playback performance, fallback and high-DPI qualification remain verification work.

- Describe materials through appearance tokens, independently of layout and interaction.
  Keep solid fills as the default and as the fallback for old themes.
- Distinguish a translucent tinted panel from actual backdrop blur. Alpha compositing alone
  does not create frosted glass. Native window glass and in-app backdrop blur are separate
  renderer/platform capabilities.
- Limit glass to selected stationary chrome, such as a toolbar or panel header. Keep note grids,
  waveforms, meters and editing surfaces solid and readable.
- Cache reusable gradient/blur resources and bound blur radius, affected area and quality.
  Invalidate only when inputs change; do not read pixels back to the CPU every frame.
- Measure frame time and GPU cost during playback, continuous scrolling, zooming and resizing
  before deciding an implementation is suitable. Keep all rendering work off the audio thread.
- Check OpenGL and wgpu separately, plus Windows high-DPI and embedded plug-in windows.
  Unsupported rendering paths fall back to a solid material without losing controls.
- Provide a single reduced-effects option and a high-contrast override. Text contrast must hold
  over the worst allowed background, not just an example screenshot.
- Do not add a renderer or docking dependency until compatibility and license checks pass.

## Verification still required

Build and inspect the native application at common and high display scales, narrow windows and
both themes. Exercise transport, theme switching, selection, mute/solo/arm, editing, undo and
repeated editor maximize/restore. Record actual observations before claiming visual QA passed.
Run repository-required formatting, clippy and workspace tests before delivery of a finished
application change.

## Track-color surface follow-up

User feedback on 20 October: neutral headers can look disconnected from their track color.
Propose independent Neutral / Subtle tint / Full track color appearance modes. Keep the
colored edge band and readable text; selected-track focus should not dominate the entire
header. Do not apply full saturation to the whole timeline lane by default. This is a
proposal, not an implemented setting.
