---
description: Track and clip colour selection, the colour menus' palette and saved colours, the floating colour window's Steps grid and field, and live edits
read_when: track colour, clip colour, colour picker, saved colours, palette, Steps grid, F2, colour field, hue band
type: spec
status: planned
last_verified: 2026-11-20
covers:
  - crates/ui/src/interaction/color_picker.rs
  - crates/session/src/colors.rs
  - crates/ui/src/app/color_window.rs
  - crates/ui/src/interaction/color_picker/panel.rs
  - crates/ui/src/interaction/color_picker/panel/canvas.rs
  - crates/ui/src/interaction/color_picker/panel/steps.rs
  - crates/session/tests/track_colours.rs
  - crates/ui/tests/window/colors.rs
  - crates/ui/tests/window/colors_native.rs
  - crates/ui/src/kit/controls.rs
---

# Track colours

This is Suhina's own design, agreed with the maintainer using original interface sketches.
Track colours remain exact sRGB bytes in project data, independent of the theme.

## Opening and choosing

Track menus and the menus of clips, arrangement and launcher clips alike (and a clip panel's
colour), offer 36 colours in an evenly spaced grid of three rows of twelve square swatches, as
large as fills the menu's width. Under the grid, two arrow buttons turn it to the user's saved
colours (**Saved colors**, in the palette's 36 cells: past 35 colours the last cell is **...**,
*More saved colors*, which opens the colour window; *No saved colors* while there are none) and
back to the **Palette**, the page's name beside them; the next menu opens on the page last shown.
A swatch applies its colour to the menu's tracks or clips as one undo step and closes the menu.
The **More colors** button at the end of the arrows' row and the configurable **F2** shortcut
open the same floating colour window; from a clip's menu it colours the clips, with **Default**
giving them their tracks' colour again. Reopening focuses
that window. It initially opens centred over Suhina, can be moved, and remains open while
the DAW is used. It follows selected track headers, showing the track name or selected
track count. Moving focus to a clip, a text field or the window itself does not change
its target. Switching projects closes it; removing its last target disables editing.
Closing it preserves applied edits. It follows the main window when minimised and does
not stay above unrelated applications. Remembered placement is kept on the current screen.

While the tool is open, its target track headers are drawn as unselected and unmuted ones in
their own colour (the Launcher's header in the exact colour, the Arrangement's in its header
shading of it, with the colour's band) instead of the selection fill, tint or muted grey. Selection outlines and all selection/track state stay
intact; clips and lane selection shading keep their ordinary look. Closing the tool restores
the configured header appearance without changing the selection or saved appearance setting.
The native client area must paint the active theme's background, and idle rendering must
settle without alternating content, geometry or scale. In its own window the picker is that
window's root (`dialog[name="color-picker"]` over `window`'s background, under the system's
title bar on macOS); where windows cannot open (the tests), it is a dialog over the main window,
with its title row and **Close** button.
Native position observations are saved for reopening; they never become continuous move
requests. Only opening and an explicit size-change adjustment may position the window.
The window is as tall as its contents, measured as they are drawn, and never taller than the
screen less 48 px (its contents then scroll). The two views are as tall as each other in the
built-in themes; where a theme makes one taller, choosing it grows the window first and draws
it in the frame after, so no frame shows it cut off, and choosing the shorter one draws it at
once and shrinks the window after.

The window's order is **Palette**, **Saved colors**, the current colour and its
editable/copyable hex value, the view (**Steps** or **Field**) with its Hue slider, numeric RGB
and HSL and its aid, and **Random** / **Default**. There is no per-track list and no Apply/Cancel transaction for the window.
Clicking a swatch applies it immediately. Copying a sibling's exact hex value and pasting
it on another track reuses it without transcribing its components.

The current colour and swatch tooltips also show a familiar colour name (for example
Gold, Khaki, Mustard or Ochre). Names use a curated copy of the public-domain xkcd colour
survey's downloadable table, with crude names omitted and spelling/spacing normalised.
For an arbitrary RGB value, show the nearest named colour by
CIEDE2000 colour difference, which weighs a tint on a near grey heavily (so #262626 is Dark Grey,
not the Dark Brown nearest its lightness), with no mark for an approximate match. The survey describes common usage; its values are not the CSS named-colour standard.
This is a descriptive aid, not quantisation: displaying a name never changes the selected
RGB or the value copied to the clipboard. The bounded table ships with the app and requires
no network access; its provenance and curation are recorded beside the data.

The palette is twelve columns of three: each column one hue (orange, yellow, sky, lavender,
green, pink, teal, red, blue, lime, magenta, then grey), its full colour in the first row, a
pale one in the second and a deep one in the third. The first row is ordered so each hue is
far from those before it. Every swatch is a cell of the Steps grid (a column's three from one
hue step, the greys from its bottom row), so a palette colour is found again in Steps. New
audio and MIDI tracks take its orange and sky. These are suggestions,
not a change to the legacy defaults of tracks saved without an explicit colour, which keep
their earlier values and are not palette swatches. An older
keymap that already assigns F2 to another window action keeps that assignment; the colour
action starts unbound until the user chooses a key.

Saved colours are the user's own list, in the order they keep it, and persist in local settings
(`saved_colors`, read with at most 4096 colours). Choosing a colour never saves it: the **+**
after the saved colours (**Save current color**) adds the current colour at the end, and is dim
while that colour is saved already, as no colour is saved twice. A click applies a saved colour,
and a drag moves it to another place. Its right-click menu, or the Menu key or Shift+F10 on the
one with the keys, offers **Move earlier**, **Move later**, **Replace with current color** and
**Remove**; Alt+Left and Alt+Right move it and Delete removes it. After a removal, **Undo** puts
it back in its place until the next change to the list. Past four rows they scroll. Saving,
moving and removing change settings, not the project, and are not project undo steps.

The palette, the saved colours and the Steps grid are each one stop for Tab: the arrows move
the keys from swatch to swatch and choose it, Home and End go to the ends of its row, and the
ring of the swatch with the keys shows only once Tab moved them, not after a click.

## Steps and Field

The window offers two views, **Steps** (the first, and the one it opens in) and **Field**,
each with its hue control, then numeric RGB and HSL. The numeric HSL controls use HSL
lightness, not perceptual lightness. Switching views or toggling an aid does not change the
stored colour.

**Steps** shows a 7 × 7 grid of swatches for one hue, 268 × 110 points and centred in the window: the hue at full HSV saturation and
value in the middle of the top row, white bottom left and black bottom right. Colours are laid out
in Oklab with its lightness replaced by Lr, Björn Ottosson's toe-corrected lightness (an estimate
of CIELAB L\*; `Lr = (k3·L − k1 + √((k3·L − k1)² + 4·k2·k3·L)) / 2` with k1 = 0.206, k2 = 0.03,
k3 = 1.206 / 1.03). In the hue's slice of (Lr, a, b), the full colour, white and black make a
triangle, and the grid is that triangle warped onto a square so the vivid part near the full
colour fills its top half:

- The top row runs evenly along the triangle's two upper edges, from a quarter of the way to
  white, through the full colour, to a quarter of the way to black.
- The middle row is the line half way from the full colour to the greys, and the bottom row the
  greys from white to black in even steps of Lr. The side columns follow the edges, and the rows
  between run evenly from one of these rows to the next.
- The cells inside the top corners, off the middle column, are each the average of their four
  neighbours (relaxed until they settle): straight below a corner a column runs back along the
  edge the top row left by, and those cells would land on that edge.

A point's chroma is taken as its share of the triangle's straight edge at its lightness, and that
share of the chroma sRGB holds there, so cells near the full colour, where the edges run outside
sRGB, follow the gamut's edge instead of clipping together.
The swatch whose RGB is the current colour is ringed, as in the palette. Every palette colour is
a Steps swatch, so a palette colour chosen anywhere (a menu, a command) shows ringed in both; any
other colour rings none. Clicking a swatch applies it.
Under the grid, its Hue slider looks like Field's (a band of the hues at full saturation and
value with the same handle) but has 36 positions: the six 60° sectors between the primaries and
secondaries (red, yellow, green, cyan, blue, magenta), each cut into six steps of equal length
along the path the sector's full colours take in (Lr, a, b), the first 0.375 of a step into its
sector. Starting part of a step in puts steps on colours between the primaries and secondaries,
such as the palette's orange. A click or drag moves the handle to
the nearest step, and the left and right arrows move one step. The grid shows the step nearest
the current colour's hue. Moving the slider applies the same row and column of the new hue's grid
(the swatch of the current colour, or the one nearest it in Oklab), live while dragging. A chosen
swatch keeps its hue step, even though its own RGB hue may round slightly off it, and white, greys
and black keep the step too. **Hue steps**, shown in Steps only and on by default, turns the
steps off: the slider then moves to any hue as Field's does (the arrows move a degree), and the
grid is drawn for that hue.

**Field** selects HSV saturation (across) and value (up) on a plane of the current hue, the
size of the Steps grid and centred like it. Its Hue slider is a band of the hues at full
saturation and value, as tall as a row of the grid like Steps' Hue band; its handle is a thin rounded
rectangle with a stroke, filled with the chosen hue. **Soft colours**, shown in Field only,
constrains pointer selection to HSV saturation 0.25–0.7 and value 0.45–0.9; the field indicates
the restricted range. It is a selection aid, not an accessibility guarantee. Exact
RGB/hex/HSL entry bypasses it and turns it off. Black and greys retain the last meaningful hue
for further adjustment. An exact RGB value is never round-tripped through another space merely
by displaying it.

Random chooses from a bounded, deduplicated
set derived from Suhina's palette, respects Soft colours, and excludes the current
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
field's bounded geometry is reused until its hue/aid/size changes, and the Steps grid until its
hue step changes. The picker does no
project-sized colour enumeration or per-frame disk I/O.

## Readability

Names, hierarchy and focus/selection indicators remain available independently of colour.
Foregrounds and outlines adapt to their composited backgrounds without changing saved RGB.
Automatic text on track colours prefers the theme's two inks, falling back to black or white
when neither reaches 4.5:1. Explicit theme rules that override a label's ink remain the theme
author's responsibility. The same contrast helper covers solid, selected and muted fills.
Normal text targets 4.5:1 contrast and essential selection indicators 3:1. These do not
require every adjacent pair of palette swatches to have 3:1 contrast, or promise that 36
colours are distinguishable under every colour-vision deficiency.

[The palette check](colour-picker-palette.svg) shows the 36 suggestions on dark and
light surfaces, a relative-luminance greyscale and Machado-model severity-1 protan/deutan
simulations plus a tritan approximation. The transformations apply the published matrices
to linear sRGB, clamp to the display gamut, then encode sRGB. The numbered chart was checked
visually: red/green and blue/purple distinctions converge, and many different hues share
similar greyscale values. It is not a 36-class accessible categorical palette. Track names,
hierarchy and independent selection outlines remain necessary. A simulation does not
replace feedback from people with colour-vision deficiencies or native-window testing.

The theme renderer includes the colour window in Steps and in Field and a track menu turned to
its saved colours (`color-picker`, `color-picker-field`, `color-menu-saved`).
They participate in every built-in theme's paint and clipped-text checks. The rendered examples
below show the actual controls in the dialog drawn where windows cannot open, not the operating
system's title bar.

| Theme | Steps | Field |
|---|---|---|
| Dark | [Preview](../design/reference/colour-picker-dark-steps.png) | [Preview](../design/reference/colour-picker-dark-field.png) |
| Light | [Preview](../design/reference/colour-picker-light-steps.png) | [Preview](../design/reference/colour-picker-light-field.png) |
| High contrast | [Preview](../design/reference/colour-picker-contrast-steps.png) | [Preview](../design/reference/colour-picker-contrast-field.png) |

## Sources

- Maintainer's project-arranging observations and approved original Suhina sketches, issue #1570.
- [W3C CSS Color 4: Oklab and OKLCH](https://www.w3.org/TR/css-color-4/#ok-lab).
- [Sharma, Wu and Dalal: The CIEDE2000 colour-difference formula](https://hajim.rochester.edu/ece/sites/gsharma/ciede2000/), its implementation notes and test data.
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
