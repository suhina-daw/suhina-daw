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
of that row the gain reduction now (`GR -4.5 dB`, in figures of one width from a fixed left, so
it does not move as the number changes) and, while the device reads a key, an **SC** badge that
stays put beside it. Which view shows is panel state (kept while the app runs, per device), never a parameter:
it changes no sound and records nothing.

The Curve and Activity views have three meters beside them: at the left the level the detector
hears (the input, or the key while one is read), with a handle at the threshold that is dragged up
and down for it; at the right the gain reduction, hanging from the top over 24 dB, and the output.
Under them: the threshold, the view's caption and the output's level.

Every meter sits in a rounded groove a little darker than the display. A level meter spans −60 to
+6 dB and is lit from its foot in three solid zones: green up to −6 dB, amber to 0 dB, red past it
(the unlit zones show faintly), with lines at −12, −24, −36 and −48 dB. A line marks its highest
level lately: held 0.8 s, then falling 12 dB a second. The meters move with ballistics: they rise
within about 12 ms and settle with a 0.18 s time constant. The threshold is a light line with a dark
edge across the input meter and a handle beside it, so it reads over the meter's fill. The meters'
colors (green `#33c16a`, amber, red, the reduction's orange `#ea9c48`) are the display's own, not
the theme's. A pane of glass lies over the display: a soft light from the top and hairlines round
it.

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
- **Meters**: the three meters alone, side by side in the middle, up to 64 points wide, with their
  scales and the input's threshold handle. Above each its value, large, and its name under it
  (`In`, or `Sidechain` while a key is read; `GR`; `Out`). A level meter's value is chosen from
  its right-click menu: *Highest peak* (the default: the highest since it was reset; a click resets
  it), *Peak, held a moment* (as its peak line) or *Level now*, with *Reset peak* under them; red
  once past 0 dB. The choice is panel state, for both level meters of the device.

Every drag is one gesture of `device.setParam` commands, so it is one undo step and records as
automation like any other edit; Shift drags a quarter as far; a double-click resets what the
handle moves. The SDK's validator lets only a parameter set with `threshold`, `ratio`, `knee`,
`output` and `mix` claim the widget. The display asks to be drawn again about 30 times a second while
it shows, so the Activity history scrolls on and the meters fall after the music stops; it reads the
readouts the device publishes ([ADR 0032](../decisions/0032-device-readouts.md)).

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
