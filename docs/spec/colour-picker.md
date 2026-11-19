---
description: Track colour selection, the floating colour window, recent colours, perceptual shades and live edits
read_when: track colour, colour picker, recent colours, related shades, F2, colour wheel
type: spec
status: planned
last_verified: 2026-11-19
covers:
  - crates/ui/src/interaction/colour_picker.rs
  - crates/session/src/colours.rs
  - crates/ui/src/app/colour_window.rs
  - crates/ui/src/interaction/colour_picker/panel.rs
  - crates/ui/src/interaction/colour_picker/panel/canvas.rs
  - crates/session/tests/track_colours.rs
  - crates/ui/tests/window/colours.rs
  - crates/ui/tests/window/colours_native.rs
---

# Track colours

This is Suhina's own design, agreed with the maintainer using original interface sketches.
Track colours remain exact sRGB bytes in project data, independent of the theme.

## Opening and choosing

Track menus offer 24 colours in three rows of eight. The **+ More colours** entry and
the configurable **F2** shortcut open the same floating colour window. Reopening focuses
that window. It initially opens centred over Suhina, can be moved, and remains open while
the DAW is used. It follows selected track headers, showing the track name or selected
track count. Moving focus to a clip, a text field or the window itself does not change
its target. Switching projects closes it; removing its last target disables editing.
Closing it preserves applied edits. It follows the main window when minimised and does
not stay above unrelated applications. Remembered placement is kept on the current screen.

While the tool is open, its target track headers preview their own exact colour instead of
the selection fill, tint or muted grey. Selection outlines and all selection/track state stay
intact; clips and lane selection shading keep their ordinary look. Closing the tool restores
the configured header appearance without changing the selection or saved appearance setting.
The native client area must paint the active theme's background, and idle rendering must
settle without alternating content, geometry or scale.

The compact order is **Palette**, **Related shades**, **Recent colours**, the current
colour and editable/copyable hex value, collapsed **Fine adjustment**, and **Random** /
**Default**. There is no per-track list and no Apply/Cancel transaction for the window.
Clicking a swatch applies it immediately. Copying a sibling's exact hex value and pasting
it on another track reuses it without transcribing its components.

The current colour and swatch tooltips also show a familiar colour name (for example
Gold, Khaki, Mustard or Ochre). Names use a curated copy of the public-domain xkcd colour
survey's downloadable table, with crude names omitted and spelling/spacing normalised.
For an arbitrary RGB value, choose the nearest named colour by
Euclidean distance in Oklab and mark the name with `≈`; an exact match has no approximation
mark. The survey describes common usage; its values are not the CSS named-colour standard.
This is a descriptive aid, not quantisation: displaying a name never changes the selected
RGB or the value copied to the clipboard. The bounded table ships with the app and requires
no network access; its provenance and curation are recorded beside the data.

The palette's first row retains the eight bright defaults. The second adds red and deeper
warm/cool hues; the third offers four pale colours and four neutrals. These are suggestions,
not a change to the legacy defaults of tracks saved without an explicit colour. An older
keymap that already assigns F2 to another window action keeps that assignment; the colour
action starts unbound until the user chooses a key.

Recent colours holds the last eight distinct explicit colours successfully applied to
tracks, newest first. Reusing a colour moves it to the front. It is bounded even with
hundreds of tracks, and persists in local settings. Opening the window, intermediate drag
values, invalid input, default restoration, no-op changes and Undo/Redo do not add colours.
One completed drag contributes its final colour. Recent colours is use history; Related
shades generates variants of the current colour.

## Precise adjustment

Fine adjustment offers **Field** and **Wheel**, a hue control, and numeric RGB and HSL.
The field selects HSV saturation/value, the wheel hue/saturation with a separate value
control. The numeric HSL controls use HSL lightness, not perceptual lightness. Switching
views, opening the section or toggling aids does not change the stored colour.

**Steps** quantises pointer selection to 15-degree hue and 0.1 saturation/value steps.
**Soft colours** constrains pointer selection to HSV saturation 0.25–0.7 and value
0.45–0.9; the field/wheel indicates the restricted range. These are selection aids, not
accessibility guarantees. Exact RGB/hex/HSL entry bypasses them and turns the aids off.
Black and greys retain the last meaningful hue for further adjustment. An exact RGB value
is never round-tripped through another space merely by displaying it.

Related shades uses OKLCH: seven lightness offsets from lighter to darker around the
current colour, retaining its hue and chroma where possible. Out-of-sRGB candidates reduce
chroma by bounded bisection at fixed lightness/hue; achromatic colours stay achromatic.
The centre entry retains the source RGB exactly. Random chooses from a bounded, deduplicated
set derived from Suhina's palette, respects the pointer aids, and excludes the current
colour when another candidate exists.

## Commands and history

`track.setColor` accepts one `track` or `tracks`, `color` as `#rrggbb` or `none`, and an
optional `gesture`. Targets and colour are validated before any mutation. A multi-track
choice is atomic and one undo step; `none` restores each track's existing default semantics.
All drag updates use the same gesture until release, so they display live and undo together.
Escape during an active colour drag cancels only that gesture if it is still the open
history gesture. It never undoes an intervening edit. Escape in a numeric/hex draft discards
the draft. Invalid or unfinished text cannot change the project. No-op choices add no undo.
An invalid submitted draft keeps its text focus until corrected or cancelled; Default also
discards drafts. Focused controls own Tab, arrows and activation keys. Outside a text field,
the tool permits transport and Undo/Redo shortcuts, while unrelated DAW shortcuts resume when
focus returns to the main view.
Taking keyboard focus into the tool releases held Gate/Repeat launch keys and computer
keyboard notes, so their eventual key-up cannot be lost in the other window.

Colour commands are unheard edits: they do not rebuild the playing audio graph. The colour
field's bounded geometry is reused until its hue/mode/size changes. The picker does no
project-sized colour enumeration or per-frame disk I/O.

## Readability

Names, hierarchy and focus/selection indicators remain available independently of colour.
Foregrounds and outlines adapt to their composited backgrounds without changing saved RGB.
Automatic text on track colours prefers the theme's two inks, falling back to black or white
when neither reaches 4.5:1. Explicit theme rules that override a label's ink remain the theme
author's responsibility. The same contrast helper covers solid, selected and muted fills.
Normal text targets 4.5:1 contrast and essential selection indicators 3:1. These do not
require every adjacent pair of palette swatches to have 3:1 contrast, or promise that 24
colours are distinguishable under every colour-vision deficiency.

[The palette check](colour-picker-palette.svg) shows the final 24 suggestions on dark and
light surfaces, a relative-luminance greyscale and Machado-model severity-1 protan/deutan
simulations plus a tritan approximation. The transformations apply the published matrices
to linear sRGB, clamp to the display gamut, then encode sRGB. The numbered chart was checked
visually: red/green and blue/purple distinctions converge, and many different hues share
similar greyscale values. It is not a 24-class accessible categorical palette. Track names,
hierarchy and independent selection outlines remain necessary. A simulation does not
replace feedback from people with colour-vision deficiencies or native-window testing.

The theme renderer includes compact, expanded-field and expanded-wheel colour-picker scenes.
They participate in every built-in theme's paint and clipped-text checks. The rendered examples
below show the actual controls in an embedded window, not the operating system's title bar.

| Theme | Compact | Field | Wheel |
|---|---|---|---|
| Dark | [Preview](../design/reference/colour-picker-dark-compact.png) | [Preview](../design/reference/colour-picker-dark-field.png) | [Preview](../design/reference/colour-picker-dark-wheel.png) |
| Light | [Preview](../design/reference/colour-picker-light-compact.png) | [Preview](../design/reference/colour-picker-light-field.png) | [Preview](../design/reference/colour-picker-light-wheel.png) |
| High contrast | [Preview](../design/reference/colour-picker-contrast-compact.png) | [Preview](../design/reference/colour-picker-contrast-field.png) | [Preview](../design/reference/colour-picker-contrast-wheel.png) |

## Sources

- Maintainer's project-arranging observations and approved original Suhina sketches, issue #1570.
- [W3C CSS Color 4: Oklab and OKLCH](https://www.w3.org/TR/css-color-4/#ok-lab).
- [W3C CSS Color 4: named colours](https://www.w3.org/TR/css-color-4/#named-colors).
- [xkcd colour survey](https://blog.xkcd.com/2010/05/03/color-survey-results/)
  and its [CC0 name/RGB table](https://xkcd.com/color/rgb.txt).
- [Carbon: categorical and sequential palettes](https://www.carbondesignsystem.com/building-blocks/data-visualization/color-palettes).
- [W3C: text contrast](https://www.w3.org/WAI/WCAG22/Understanding/contrast-minimum.html),
  [non-text contrast](https://www.w3.org/WAI/WCAG22/Understanding/non-text-contrast.html),
  [use of colour](https://www.w3.org/WAI/WCAG22/Understanding/use-of-color.html).
- [Machado, Oliveira and Fernandes (2009)](https://doi.org/10.1109/TVCG.2009.113);
  numerical matrices checked against the [Colour Science dataset](https://github.com/colour-science/colour/blob/develop/colour/blindness/datasets/machado2010.py),
  with the [model's tritan limitation](https://colour.readthedocs.io/en/master/_modules/colour/blindness/machado2009.html).
