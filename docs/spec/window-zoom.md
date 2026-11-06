---
description: Zooming the whole window (every panel, text and control larger or smaller), its keys, limits and how it is remembered
read_when: zoom, UI zoom, window zoom, interface size, scale, Ctrl+Plus, Ctrl+Minus, Cmd+Plus, bigger text, high-DPI, zoom display
type: spec
status: current
last_verified: 2026-10-25
covers:
  - crates/ui/src/settings.rs
  - crates/ui/src/settings/keymap.rs
  - crates/ui/src/settings/settings_view.rs
---

# Window zoom

## Observed

Source: the public Live 12 manual (keyboard shortcuts › Zooming, Display and Selections, and
Settings › Display & Input), read on 2026-10-13. No binaries or files were looked at.

- The whole main window can be drawn larger or smaller: Ctrl and + zooms it in and Ctrl and -
  zooms it out (Cmd on macOS). These keys are separate from the ones that zoom the timeline.
- The same zoom is a setting (a Zoom control in the display settings), so it stays from one run
  to the next.

## Suhina's behaviour

- **Zoom in, Zoom out:** Ctrl and + (or Ctrl and =) and Ctrl and -; Cmd on macOS. Ctrl and = is
  needed on macOS: there + is Shift and =, and with Cmd held the window reports that key as =,
  never +, so Cmd and = is what zooms in ([shortcuts](../shortcuts.md)). It also spares the Shift
  on other keyboards where + is Shift and =. Ctrl and 0 goes back to 100 %. Each step is 10 %,
  from 50 % to 200 %. A value between steps (a hand-edited file) moves to the next step in the
  direction asked.
- Everything in the window scales together: panels, text, controls, the timeline and the
  hit areas. The arrangement's own zoom (Ctrl or Cmd and the wheel, a pinch) is unchanged.
- The keys are actions in the keymap (`view.zoomIn`, `view.zoomOut`, `view.zoomReset`), so they
  can be changed in Settings › Keys ([shortcuts](../shortcuts.md)). View › Window zoom in, Zoom out
  and Actual size do the same.
- Settings › Appearance › Window zoom shows the value between - and + buttons; clicking the value goes
  back to 100 %.
- The zoom is saved in `settings.txt` as `zoom = <percent>`. A value out of range is limited to
  it; one that is not a number is skipped with a warning and leaves 100 %.
