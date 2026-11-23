---
description: The built-in Compressor's panel - its display with the Curve, Activity and Meters views, the threshold dragged on the curve, the input meter or the activity line, the side-chain key shown as the detector hears it, and the readouts the device hands the panel
read_when: compressor, compressor panel, compressor display, transfer curve, gain reduction meter, gain reduction history, activity view, sidechain display, key level, CompressorDisplay, compressor readouts
type: spec
status: current
last_verified: 2026-11-19
covers:
  - devices/compressor/src/panel.rs
  - crates/ui/src/detail/compressor_display.rs
---

# Compressor

Suhina's `compressor` (`devices/compressor`): a stereo-linked, feed-forward compressor with a soft
knee, a high-pass on the detector, a dry/wet mix and a side-chain key
([sidechain.md](sidechain.md)). Its sound is described in the crate's documentation; this
document describes its panel and what the device hands it.

- **Sources:** Suhina's own design. The idea of a compressor display that switches between a
  transfer curve, a scrolling activity view and meters alone is common to many compressors and is
  observed from their published screenshots; the layout, colors and behavior here are our own.

## The panel

628 × 168 (`suhina_device_compressor::panel`), its dials grouped by what they do. At the left,
what is compressed: Thresh and Ratio above, Input and Knee below, beside the display's threshold
arrow. In the middle the display (`Widget::CompressorDisplay`, host-drawn, 316 wide). At the
right, how it sounds: Attack, Release and Dry/Wet above, Output and SC HP (the side-chain
high-pass) below. Every parameter has a dial;
the display only adds ways to move the threshold and the ratio.

The display has a row of tabs at its top, **Curve**, **Activity** and **Meters**, and at the right
of that row the gain reduction now (`GR -4.5 dB`) and, while the device reads a key, an **SC**
badge. Which view shows is panel state (kept while the app runs, per device), never a parameter:
it changes no sound and records nothing.

The Curve and Activity views have three meters beside them: at the left the level the detector
hears, with an arrow at the threshold that is dragged up and down for it; at the right the gain
reduction, hanging from the top over 24 dB, and the output. The level meters span −60 to +6 dB with
a line at 0 dB, and turn red above it. Under them: the threshold, the view's caption and the
output's level.

- **Curve**: the transfer curve, the detector's level across and the device's output level up,
  both −60 to +6 dB, drawn from the device's own gain computer (`gain_change`) with the mix and
  the output gain applied as the device applies them, so the curve is what sounds. A grid every
  12 dB, the unity diagonal dashed, the threshold dashed across the view and the knee's width as a
  faint band. A ring on the curve at the threshold is dragged sideways for the threshold and up
  and down for the ratio (down compresses more, the view's height spanning 1 : 1 to 20 : 1 on a
  logarithmic scale). A dot shows the level the detector hears now, at the height the gain
  reduction it has now puts it: on the curve when the reduction has settled, off it during the
  attack and the release. The ratio and the knee are written under the view.
- **Activity**: the last four seconds scrolling right to left, 40 readings a second taken from the
  readouts as the panel draws: the level the detector hears filled from the bottom (the key's
  while a key is set, so a kick drum keying a bass shows as the kick), the output's level as a
  line, and the gain reduction hanging from the top on its own 24 dB scale (marked −6, −12, −18 at
  the right edge), so the pumping a key causes is seen against what causes it. The threshold is a
  dashed line on the level scale, grabbed within 5 points and dragged up and down.
- **Meters**: the three meters alone, large, with their scales, the input's threshold arrow, and
  their values under them (`In` or `Key`, `GR`, `Out`).

Every drag is one gesture of `device.setParam` commands, so it is one undo step and records as
automation like any other edit; Shift drags a quarter as far; a double-click resets what the
handle moves. The SDK's validator lets only a parameter set with `threshold`, `ratio`, `knee`,
`output` and `mix` claim the widget. The canvas repaints at about 30 frames per second while the
device publishes readouts ([ADR 0032](../decisions/0032-device-readouts.md)), which it always does.

### Readouts

What `Device::readouts` holds, from the block's frames (`suhina_device_compressor::readout`):

| Index | Value |
|---|---|
| 0 | the level the detector read (the key while keyed), after the input gain and the side-chain high-pass, the louder side, dBFS, a falling peak (0.145 s time constant, about 60 dB per second), −100 for silence |
| 1 | the device's own input after the input gain, the louder side, dBFS, falling the same way |
| 2 | the output, the louder side, dBFS, falling the same way |
| 3 | the gain reduction, dB (zero or positive): the larger of the reduction now and the readout's last value falling with a 0.03 s time constant, so a short peak between two of the panel's frames still shows while the release keeps its shape |
| 4 | 1 while the detector reads a key, else 0 |

A reset (and a change of sample rate) clears them to silence and no reduction.

## Decisions

- **The view is panel state.** It changes no sound, so it is not a parameter that automation,
  presets and the import would have to carry; the EQ's selected band is kept the same way.
- **The history is kept by the panel, not the device.** The 35 readouts could hold only about 30
  readings of one value; the panel samples the readouts as it draws and keeps four seconds, so the
  device stays as it was apart from its meters. The history starts when the panel opens.
