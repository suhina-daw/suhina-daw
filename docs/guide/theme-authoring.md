---
description: Guide for people and AI assistants writing or editing a Suhina theme - the folder's files, theme.css, quick controls and variants, artwork, layout, limits and every colour token
read_when: write a theme, edit a theme, theme author guide, theme.css, theme package, theme folder files, colour tokens, quick controls, variants, AI theme edits
type: howto
status: current
last_verified: 2026-11-20
covers:
  - crates/ui/src/kit/theme.rs
---

# Writing a Suhina theme: author and AI guide

## What a theme is

A theme changes Suhina's native egui interface through colours, SVG/PNG control artwork,
meter appearance, background images, and connected CSS styles and layouts. It contains
data, not executable plug-ins. Audio, parameter gestures and commands remain native.
Use this guide when creating, editing, explaining or sharing a theme, including when
asking an AI assistant to modify one. Inspect the supplied files before making changes.

The themes folder is `themes` in Suhina's data folder (`%APPDATA%\Suhina\data` on
Windows) unless you choose another: **Settings › Themes › Themes folder** (Browse, or empty for
the default), the `SUHINA_THEMES` environment variable (which wins over the setting), or
`suhina --data-dir <folder>` / `SUHINA_DATA`, which moves all of Suhina's data for a portable
or test install. Programs installed from the Microsoft Store (MSIX), such as some editors and
AI apps, may see their own private copy of `%APPDATA%` and not Suhina's: choose a themes folder
in Documents, or link a theme's folder there. A theme's own folder inside the themes folder may
be a symbolic link or a junction (`mklink /J`) to a folder elsewhere; links inside a theme are
still refused.

Bringing in a package whose theme is one of yours already asks: **Replace** writes the
package in its place (its variants go with it), **Copy** brings it in under a new id.

## Files and why they are separate

```text
my-theme/
  theme.json          # the theme's name, author, description, swatch and suggested toolbar
  theme.css           # the theme's style sheet: its look, layout and the values of its settings
  thumbnail.svg       # optional 16:9 picture for the theme's card in the picker
  variants/*.json     # optional variants: other values for the theme's declared controls
  assets/*.svg        # vector control artwork
  assets/*.png        # optional raster artwork and sequence frames
  background.png      # optional background image
```

`theme.css` is yours. What a user changes in Suhina's variant editor (the settings your theme
offers with `@property` and a label) is saved in a variant file when they press Save. Moving a control or
choosing artwork in the app edits only that declaration in `theme.css` (Suhina changes or adds
the one declaration and leaves your comments, `@media` rules and everything else as you wrote
them); *Theme info…* (name, author, description) or rearranging the toolbar writes `theme.json`. There is no
`layout.css` any more: a folder that still has one is warned about and the file is not read, so
move its rules into `theme.css`.

### theme.json

```json
{
  "format": 1,
  "name": "My theme",
  "author": "Your name",
  "description": "Dark slate with a mint accent",
  "default-variant": "Dark",
  "swatch": "color-mix(in oklab, var(--panel-background), var(--accent) 35%)",
  "color-scheme": "dark"
}
```

- `format` (required) is `1`. A Suhina that reads only an older format refuses the theme and
  says to update.
- `name` (required) is the theme's name. It comes only from here; `theme { name }` in CSS is
  not read.
- `author` and `description` (optional) are shown with the theme.
- `default-variant` (optional) names the theme's own values, which are never a file in
  `variants/`; *Default* when left out.
- `swatch` (required) is the colour the picker shows for the theme and each variant: any CSS
  colour value, such as `#7FD1B9`, `var(--accent)` or a `color-mix()`. It is worked out for
  each variant from your `@property` rules and `:root` values alone, without applying the rest
  of the sheet, so `currentColor`, `attr()` and units relative to an element do not work in it.
  A theme whose swatch does not give a colour for every variant is refused on import.
- `color-scheme` (optional) is `"light"` or `"dark"`: whether your theme's own values are a light
  look. Left out, the theme is dark. A variant file says its own the same way
  (`"color-scheme": "light"`), or has the theme's; the variant editor's *Colour scheme* control
  sets it. Suhina never guesses it from your colours: the window's title bar follows it, and any
  sheet asks for it with `@media (prefers-color-scheme: light)` or
  `color: light-dark(#222, #eee)` (the first value for light, the second for dark). Device panels
  with light and dark artwork pick theirs by it, so say `light` for a light theme.
- `toolbar` (optional) suggests a toolbar arrangement:
  `{ "items": ["browser", "tempo", "transport"], "hidden": ["agents"] }`.
- `base` (optional): `"none"` leaves Suhina's base sheet out from under your theme (*Suhina's base
  sheet* below), for a theme that styles everything itself.

### Swatches and the thumbnail

The picker draws each swatch as a `theme-swatch` element with a `color` attribute, styled by
the theme in use. Suhina's default is a rounded rectangle filled with the colour; your sheet may change it:

```css
theme-swatch { background: attr(color type(<color>)); border-radius: 50%; }
```

`thumbnail.svg` is the picture on the theme's card. Make it 16:9 (`viewBox="0 0 160 90"`, say).
It may use your properties through `var()`; they are filled in for each variant as for the
swatch, colours as plain colours. The artwork rules apply: no scripts, embedded images, foreign
objects or links. Without a thumbnail the card is filled with the swatch colour. The built-in
themes' `thumbnail.svg` (*Export theme…* on a built-in theme writes its folder) is a good start.

Installed themes are the same folders, named by their id. Relative image paths in the sheets are
authoritative; keep the whole folder together. A theme saved by an earlier Suhina version (JSON,
or a `theme.css` that imports `advanced.css`) is converted once; its original files are kept in
`legacy/` inside the theme's folder.
Drop the ZIP on Suhina to import it, then select it in Settings → Appearance → Themes.
Open the themes folder from that page to edit the installed files.

A folder in the themes folder is a theme when it holds files named exactly `theme.json` and
`theme.css` (on Windows, check that they are not `theme.css.txt` or `theme.json.txt` with the
extension hidden). A theme made for the format before `theme.json` needs one added by hand. Suhina finds a new
folder within about a second; a folder that is not a
theme is named in the status bar with what it is missing. The folder's name is the theme's id:
it stays when the theme is renamed. Two themes with the same name show their folder names on
their cards. **New theme…** started from another theme copies its whole sheet, its layout rules
included. Built-in themes cannot be edited, only used as the start of a new theme.

### Settings and variants

A theme's colours are custom properties: every theme has each colour token (`--accent`,
`--window-background`, `--track-headerBackground`: the token names with hyphens), set in
`:root`. A theme needs to set only the colours it changes: any other is Suhina Studio's, and a
theme Suhina saves writes only the colours it sets. Token names from the first versions
(`--surface-0`, `--line`, …) are no longer read; use the names in the table below. A token colours many things at once; in the Launcher, the mixer and the Arrangement
each thing is also an element of its own (the selector reference below), and a rule for it
replaces the token there alone: `slot[kind="stop"] { background-color: … }`,
`grid-line[kind="beat"] { color: … }`. A line or a mark takes only a `color` given to it, not the
text colour a rule gives the view it is in. Its settings are properties of the elements they
belong to:

- `meter { meter-style; meter-segments; meter-intensity; }`: `continuous`, `segmented`,
  `needle` or `frames`; 4–64 segments; an intensity of 0–100.
- `window { artwork-fps; liquid; panel-gradient; track-gradient; clip-gradient;
  backdrop-image; backdrop-opacity; backdrop-blur; }`: 1–30 frames a second; `auto` or three
  strengths 0–100 for panels, tracks and clips; `none` or `90deg, #RRGGBBAA 0%, #RRGGBBAA 100%`
  (2–8 stops); `none` or `url("background.png")`; 0–100 each.
- `session { track-width; return-width; scene-width; slot-height; header-height; }`: `auto` or
  pixels.

What you leave out keeps Suhina Studio's value. The custom properties that held these
settings before (`--meter-style`, `--liquid`, `--session-track-width` and the others) are no
longer read; a theme that still sets one gets a notice naming the rule that replaces it.

Offer your own settings with `@property`. A property with a `label` appears under **Theme
settings** in the editor, with a control for its type:

```css
@property --glow { syntax: "<number>"; initial-value: 0.6; label: "Glow"; group: "Effects"; range: 0 1; step: 0.05; }
@property --edge { syntax: "solid | dashed"; initial-value: solid; label: "Edges"; }
:root { --accent: #7FD1B9; --accent-hover: color-mix(in oklch, var(--accent), white 20%); }
button:hover { background-color: var(--accent-hover); opacity: var(--glow); }
```

Supported syntaxes: `<color>`, `<number>`, `<integer>`, `<length>`, `<percentage>`,
`<length-percentage>`, `<angle>`, `<string>`, `<image>`, keywords joined by `|`, `+` for a
space-separated list, and `*`. A value that does not match its syntax uses the initial value.

A variant is a JSON file in `variants/`; its file name is its id and it holds its name and only
the values that differ from the theme's:

```json
{ "name": "Frost", "values": { "--accent": "#88C0D0", "--window-background": "var(--nord-snow)" } }
```

Values are CSS text and may refer to the theme's own variables. A variant may set only the
controls your theme declares with `@property` and Suhina's core properties; a value for any other
variable (a helper such as `--my-well`) is left out with a warning, so declare a helper with
`@property` when variants should change it. Anything in the file besides `name` and `values` is
a warning. A variant's values win over the
theme's rules for the same property, including ones inside `@media`. The editor shows a theme's
variants as swatches under its card. A built-in theme can come with variants of its own; variants you
add or edit for one are kept in a folder of its id holding only `variants/`, without a copy of
its CSS.

### Your own overrides and device panels

`user.css` in the themes folder (beside the theme folders, not inside one) is yours: it
applies over every theme and device panel and stays when you switch themes. A device's own
panel style (in its `.sd` file) applies only inside that panel, over the theme. Each control
of a device panel is `#its-id`, for example:

```css
/* user.css */
#cutoff { accent-color: #ff8a00; }
device-panel knob:hover { color: #ffffff; }
```

Built-in devices that draw displays (the FM synth, Multiband) take five more colours as custom
properties: `--device-display`, a display's dark ground, and `--device-series-1` to
`--device-series-5`, a device's series of colours (bands, operators, a gain line, a spectrum).
Each takes any CSS colour, its alpha too. Set them on `:root` or `device-panel` for every device,
or on `#device-<id>` for one (its expanded panel included):

```css
:root { --device-display: #101418; }
#device-multiband { --device-series-1: #ff6b6b; --device-series-2: #ffd93d; }
```

## How and when to edit each file

1. Start a new theme from a bundled one (*More › New theme…*), or from Suhina's defaults.
2. Set the name and swatch in `theme.json`. Theme folder ids use letters, digits and `-`.
   The folder's name stays its id when the name changes. A name given in Suhina (*Theme info…*)
   is written to `theme.json`.
3. Set the palette in `theme.css`'s `:root` and the settings on their elements. For example:

   ```css
   :root { --accent: #7FD1B9; --window-background: #14161A; }
   meter { meter-style: segmented; meter-segments: 24; }
   ```

   Colours may be any CSS colour the engine reads (`#RRGGBB`, `#RRGGBBAA`, `rgb()`, `hsl()`,
   `color-mix()`, names), and `var()` of your own variables.
4. For conditional styles, selectors, variables, effects, motion or connected Flex/Grid
   layout, write them in `theme.css` too. Suhina changes only the declarations its editors set.
5. For a new control silhouette, edit an SVG or PNG and name it in `theme.css`, e.g.
   `icon[kind="play"] { image: url("assets/play.svg"); }`.
   For layout positions, the toolbar's right-click → Edit toolbar mode is usually
   easier: drag the actual controls on the 8-point grid. Done saves to `theme.json`; Cancel or
   Escape discards. Entering/leaving edit mode alone preserves the layout.
6. Save one file at a time and inspect Suhina's warnings and the actual interface.
   Files are watched approximately once a second. A save of
   a variant over an edit you made to that variant's file outside Suhina is refused and your
   file is kept.
7. Test normal, hover, active and disabled states, narrow/wide windows, interaction,
   clipping and contrast, with each variant. Export and reimport the ZIP to check portability.

The Launcher's track, return and scene widths and its header and slot heights are set in
`theme.css` with `session { track-width; … }`: widths are 64–512px, slot height 16–64px and header height
20–64px. Use `auto` to remove
an override. The same geometry drives drawing, scrolling and drop/input targets.
Mixer height keeps its existing native divider/preference. Launcher and Arrangement
remain separate; this does not introduce the separately planned mixer view.
`pan { appearance: knob; }` is a portable native pan-face override for Launcher and Arrangement. `field` forces a number field, while
`native` retains the global preference. Native parameter gestures remain unchanged.

The gloss example gives `mixer` and `track-name` their own materials. Meter gradients
under `meter-vertical meter-level` and `meter-horizontal meter-level` keep their
colour stops anchored to the full dB range; peaks/RMS reveal the appropriate portion.
Outset shadows form the glow around the lit region, without filling empty rails or
segment gaps. This differs from a decorative gradient that stretches with the level.

**Meter intensity** dims measured ink and glow from 0–100%: `meter { meter-intensity: 65; }`
in `theme.css`, or a quick control of your own that sets it through `var()`.
It also affects needles, hold marks and signal-driven artwork frames, while keeping
the native clip warning visible and native bar rails unchanged. Frame artwork dims
as a whole, including any background baked into its image. This changes visual opacity, not audio
gain or saturation. Change palette/gradient colours for different hues or saturation.
In `theme.css`, `meter-level { opacity: .5; }` additionally dims native bars, hold marks,
needles and CSS fills/glow. Transparent CSS fills suppress native fill fallback.
Generic CSS `filter: brightness()` / `saturate()` is not connected to native painting.

## Laying out the strips, and artwork variants

The built-in **Suhina Sheet** is a complete green-and-white worksheet-style example
with original SVG artwork for all 16 base roles. Select it in Themes, duplicate it
to edit its CSS, or export it to obtain its CSS and editable SVG files together.
**Suhina Meadow** demonstrates classic paper controls and translucent glass panels over a private
background image. Its original generated meadow exports as `background.png`,
alongside the effective SVG artwork and blue/cream CSS. Its transparent empty
workspace reveals the image; it does not require desktop transparency support.
Its theme-owned liquid strengths are 48/24/12 for panels/tracks/clips. Actual liquid
requires animations and a supported GPU renderer; ordinary alpha/CSS materials
remain available with OpenGL or adapters without linear texture-view aliases.
**Suhina Blocks** has brown plank CSS and original pixel SVGs, with no landscape
or background PNG. `track-column { track-artwork: on; }` extends body
artwork behind Launcher slots; empty cells preserve it and native clip launches remain
unchanged. `off`/omission preserves native fills. Native custom properties inherit
from the nearest authored ancestor, and a closer scope can override them.

A track header, a mixer strip and the toolbar's transport lay their parts out with Flexbox (and
Grid) in `theme.css`, like the window and the bars. The parts are the elements that paint them;
the `group`s around them only lay them out. Suhina's own layout is the user-agent sheet's, so a
theme changes only what it writes a rule for:

```css
mixer track-colour { display: none; }                  /* no colour band */
mixer meter { order: -1; margin: 0 8px 0 0; }          /* the meter before the fader */
mixer-toggles { flex-direction: column; }              /* solo, cue and arm one above the other */
group#mixer-level { flex-direction: column; }          /* the fader lying across the strip */
#mixer-volume { height: 20px; }
#mixer-sends { display: none; }                        /* no sends in the strip */
toolbar-module#transport { gap: 4px; }                 /* play, stop and record further apart */
```

Which parts a strip has room for stays Suhina's: the mixer's height steps add the name, the
output, the device list and the sends as the strip grows, and a header takes one or two lines by
its width and height. Suhina gives each part its size (a fader's length, a meter's width, a
button's width); a `width` or `height` in the sheet wins. A part hidden with `display: none` is
neither drawn nor clicked, and the parts after it close up. A fader laid out wider than tall lies
down and is dragged sideways. Every strip of one kind shares one layout, so a rule cannot single
out one track's strip by its colour or id. Suhina decides which parts fit from the strip's own
padding and border without `[layout]`, `[size]`, `[name-first]` or `[narrow]`: give the padding
on `track-header` or `mixer` itself, as padding under one of those attributes moves the parts but
does not make room for them. The selector reference below lists each strip's parts
(*The strips' parts*). A device panel's controls keep Suhina's layout for now.

Control placement properties are gone, and a sheet that still writes one of them gets a notice
naming what to write instead. The notice comes from the property, not the selector: a
`.control-NAME` rule that sets only other properties, such as `width`, matches nothing and gets no
notice.

| Removed | Write instead |
|---|---|
| `.control-NAME { … }` | a rule for the part's element id, listed in *The strips' parts* (`#mixer-volume` for a mixer's fader, `button#control-play` for a toolbar button); a `.control-*` rule matches nothing |
| `left`, `top`, `right`, `bottom` | `margin-left`, `margin-top`, `margin-right`, `margin-bottom`, or `order` to move a part before or after the others |
| `direction`, `--suhina-direction` | `flex-direction` on the group that holds the parts (`mixer-toggles { flex-direction: column; }`) |
| `anchor-left`, `anchor-top`, `anchor-right`, `anchor-bottom` | `align-self` on the part, `justify-content` on its group, or `margin-left: auto` (and the other sides) to push it to an edge |
| `compact-below` on `arrangement` | nothing to set: Suhina picks a header's lines by its size; style the one-line header with `track-header[layout="short"]` |

Supply `track.name` and `track.body` SVGs or PNGs for name and strip backgrounds, with a
variant for a state if you like. Artwork is not chosen by a track's colour; an SVG that draws in
`currentColor` takes the track's colour as Suhina draws it (dimmed while muted, and grey in a
muted track's arrangement header), or the element's
own `color` where your sheet gives one, which a rule can take from the track's colour (see
*Tracks* below):

```css
track.name { image: url("assets/name.svg"); }
track.name.hover { image: url("assets/name-hover.svg"); }
window { artwork-fps: 12; }
```

For an image sequence, supply contiguous `.frame-0`, `.frame-1`, … keys for that
variant (at most 32 frames), e.g. `track.name.hover.frame-0`.
Frame rate is 1–30 fps (`window { artwork-fps: 12; }` in `theme.css`). With native animations off,
the first frame stays visible without continuous repainting. State variants
fall back to the role's normal artwork and then Suhina's embedded original.
PNG artwork retains authored colours and transparency rather than applying `currentColor`.
Each PNG is limited to 8 MiB compressed, 4096 pixels per axis and 4 million pixels;
combined inline artwork is limited to 16 MiB. Rendered control textures are bounded
to 1024 pixels per axis. SVG remains self-contained: embedding images or external
resources inside an SVG is not supported. Use separate PNG role files instead.

## Artwork: variables, text and nine-slice

An SVG may write the theme's variables, `fill="var(--accent)"` or
`stroke="var(--line, #808080)"` with a fallback, so one file serves every variant; a name the
theme does not set and that has no fallback stays as written. `currentColor` is still the
control's ink. `<text>` draws with Suhina's own typefaces (Manrope; `font-family="monospace"`
for IBM Plex Mono), never the computer's fonts.

Artwork that should keep its corners while it stretches says where its edges are:
`data-suhina-slice="6"` on the root `<svg>` (or `"top right bottom left"`, or
`"vertical horizontal"`), in the SVG's own units, which are points. Its corners keep their
size, its edges stretch along one axis and its middle along both; a header's name bar or the
loop bar keeps its rounded ends at any length.

```svg
<svg xmlns="http://www.w3.org/2000/svg" width="24" height="16" data-suhina-slice="6">
  <rect x="0.5" y="0.5" width="23" height="15" rx="6" fill="var(--accent)" stroke="currentColor"/>
</svg>
```

The desktop shows through only where the system composites a window's transparency (the GPU
renderer on a compositor that supports it; Windows and macOS usually do). Elsewhere the window
is opaque and a see-through colour reads dark, over black. A background image is the window's
own: it moves with the window, it is not the desktop's wallpaper. A picture larger than 8 MiB
or 4 megapixels (a 4K wallpaper) is scaled down to fit 2560 by 1600 when the theme is read.
`--window-background` is the surface over the picture: its alpha, scaled by
`window { backdrop-opacity }`, is how much it darkens the picture; `backdrop-opacity: 100` with
an opaque colour hides it.

Button, fader and track artwork is drawn over the surface under it, on a see-through theme
and an opaque one alike, so its transparent parts show what is under them.

## Theme packages

**Settings › Appearance › Themes › More › Export theme…** writes the theme in use as one file,
`<theme>.theme.suhina`, into the `exports` folder beside the themes folder. It holds the
theme's folder and a `manifest.json` with the theme's id, its name and the SHA-256 sum of every
file. Drop a package on Suhina's window (Settings open or not) or open it with Suhina (a
double-click, once the system knows the ending) to bring the theme in. Before anything is
written Suhina checks the manifest, every file's sum, the paths, the sizes and what each file
holds (a `theme.json` with a name and a swatch that gives a colour for every variant, a
`theme.css` that reads, SVGs that are SVG, a 16:9 thumbnail, PNGs that are PNG, variants that are
JSON); a damaged or changed package is refused and says why. The same checks run on every
import. The package's name comes from `theme.json`.

From a terminal:

```sh
suhina-cli theme pack my-theme            # writes my-theme.theme.suhina beside the folder
suhina-cli theme pack my-theme -o out.theme.suhina
suhina-cli theme validate out.theme.suhina   # or a folder; lists the notices
```

A package is not signed. Sign the file with a key of your own, outside Suhina, when the
receiver needs to know it came from you.

## Suhina's base sheet, and colouring a region

Under your theme sits Suhina's own design, the **base sheet** (built into the app), and under it
the **user-agent sheet**, the bare floor that keeps every element usable. Every rule of your theme
beats every rule of the base sheet, whatever its specificity or `@layer`: you never need a more
specific selector to win over Suhina's.

- **Set variables, keep every state.** The base sheet takes every colour from the colour variables
  (`--accent`, `--control-background` …). A theme that sets only `:root` variables keeps every look
  it gives, its states included. A rule of yours for an element overrides the base sheet's rules
  for that element, its states' too, for each property your rule sets: `button {
  background-color: … }` overrides the background of the base sheet's `button[checked]`, so write
  the states you want for that property as well (`button[checked] { background-color: … }`).
  A property you do not set keeps the base sheet's value.
- **Colour a region through its variables.** `mixer { --text-value: gold; }` reaches every part
  inside the mixer that Suhina colours with `--text-value`. A plain `mixer { color: gold; }`
  colours only the text that has no rule of its own: it no longer reaches buttons, value fields,
  drop-downs, knobs, list rows and the label kinds (`hint`, `caption`, `value`, `error`), which the
  base sheet colours with a rule of their own. Colour a region through its variables
  (`--text-primary`, `--text-value`, `--control-background` …), or write a rule for the part.
- **Your own sheet too.** `:root { --accent: #FF8A00; }` in `user.css` reaches everything, in
  Suhina Studio as in any theme.
- **Shared controls.** The base sheet gives Suhina's own controls their looks: a `button` (its
  fill, text, edge and the ring while it has the keys, a step brighter under the pointer), the
  toggles (`button[kind="toggle"]`, and `toggle` for a switch a view draws itself: dim words, and
  when `[checked]` the chosen fill and the text on it; solo, `button[kind="solo"]`, is lit in the
  accent, arm, `button[kind="arm"]`, in the recording colours), `knob`, the level bars' tracks (`value[kind="bar"]`, `pan[kind="bar"]`) and their
  `value-fill` and `value-handle`, the pan field and its `pan-center`, `send`, the line under a
  field (`text-input`, `value`, `dropdown`), a drop-down's `dropdown-arrow`, a typed number
  (`text-input[kind="typed"]`), a slider's `slider-rail`, `slider-fill` and `slider-handle`, a
  fader cap's fill, a meter's `meter-level` (`meter-level[kind="hot"]` near the top) and
  `meter-clip`, a `dialog`'s edge, a floating list's `list-row` (`[selected]` for the picked one),
  and the words (`label[kind="hint"]`, `"caption"`, `"value"`, `"error"`). **Your rule for any of
  these overrides the base sheet's for each property it sets, its states' too**: after `button {
  background-color: … }` a lit toggle takes that background as well, so write
  `button[kind="toggle"][checked] { background-color: … }`, and for a slider `slider-fill { … }`
  `slider[checked] slider-fill { … }`.
- **The Launcher, the mixer and the Arrangement.** The base sheet gives these views their looks
  too: a scene's button (`scene`, `[playing]`, `scene[kind="stop-all"]`), a track's stop button
  (`slot[kind="stop"]`, `[queued]`), an empty slot (`session slot`, `[selected]`) and its
  `slot-mark`, a deactivated, queued or selected clip in the Launcher (`session clip[muted]`,
  `[queued]`, `[selected]`), the record button of an armed track's slot (`slot-record`,
  `[recording]`), the key mapping's tint (`mapping-overlay`), Main's header
  (`track-header[kind="main"]`), the lines between the Launcher's bands, columns and slots
  (`separator[kind="band"]`, `"column"`, `"slot"`); a mixer strip's fill (`mixer`), its device
  list and rows (`device-list device-row`, `[selected]`, `[muted]`), its fader's value (`fader`)
  and `fader-scale`, a `send` and the control bar's and the strips' own switches
  (`button[kind="record"]`, `"metronome"`, `"monitor"`, lit with `[checked]`); in the Arrangement
  the colours under the grid (`arrangement`) and under its panels (`arrangement[kind="panel"]`),
  the `ruler`, the grid lines (`grid-line[kind="bar"]`, `"beat"`), the `tempo-mark`, the
  `start-marker`, the `playhead`, the loop (`loop-region`, `[checked]`, and `loop-line`), a
  `marker` (`[checked]` while dragged), the tools (`arrangement-tool`, `[checked]`), the selected
  time (`time-selection` and its kinds), a clip being recorded (`arrangement clip[recording]`),
  a selected track's lane (`arrangement track-lane[selected]`) and Launcher column
  (`track-column[selected]`), tinted from `selection.background`, the lane the piano roll edits
  (`arrangement track-lane[checked]`), a track's header
  (`arrangement track-header`, `[selected]`; Main's `arrangement track-header[kind="main"]`), a
  clip's gain field (`clip-gain`) and fade handles (`clip-fade-handle`), the automation lanes
  (`automation-lane[kind="group"]`, `automation-header`, `[selected]`,
  `automation-line[kind="fixed"]`, `automation-line[muted]`, `automation-point[kind="fixed"]`,
  `automation-point[selected]`, `automation-pin`) and the switches `button[kind="envelope"]`,
  `"lanes"` and `"shape"`. **Your rule for an element overrides the base sheet's for each property
  it sets, its states' too**: a theme with a rule for every `scene` or `clip` states `scene[playing]`
  or `clip[muted]`, `[queued]` and `[selected]` as well. A rule for every `button` hides the lit looks of these switches as well:
  Suhina Meadow, Blocks and Sheet and the Dark Gloss example state `button[kind="monitor"][checked]`
  and the others again, and so does a theme of yours with such a rule. A plain `color` on `session`, `mixer` or
  `arrangement` no longer reaches the parts above that the base sheet gives a `color`
  (a send, a fader's value, a scene's text, a tool, a marker, a clip's gain field …): colour them
  through the region's variables (`--text-primary`, `--text-value`) or give the part a rule
  (`send { color: … }`).
- **The detail area and the browser.** The base sheet colours the browser (its text, its words by
  kind from the `--browser-text-*` variables, its rows and places under the pointer, picked and
  where a drop lands, its filter chips `button[kind="chip"]`, a tag's box
  `button[kind="tag-check"]`, the preview's play `button[kind="preview"]`, the "…" of the tags that
  did not fit `button[kind="more"]`, and the fills it paints itself, `browser-fill`), the detail
  panels (their fills `detail-fill`, a device's header, nodes, outline, lines, parameters and
  canvas colours `device-canvas`, a group's branches `device-row[kind="branch"]`, the fields
  `value[kind="field"]`, the buttons `button[kind="panel"]`, switches `button[kind="panel-toggle"]`
  and lists `dropdown[kind="panel"]`, the Clip and Devices switch `editor-tab[kind="switch"]`) and
  the piano roll (its grid, rows, notes, velocities, ruler, loop bar, keys, strips, tools and
  toolbar menus). **A rule for every `button` hides their lit looks too**: state
  `button[kind="chip"][checked]`, `button[kind="tag-check"][checked]`,
  `button[kind="focus"][checked]` and `button[kind="more"]:hover`. A region's plain `color` no
  longer reaches the piano roll's ruler numbers (`ruler-number`) and toolbar lines
  (`separator[kind="bar"]`) or a device's values (`device-control[kind="value"]`,
  `device-canvas[kind="value"]`): colour the region through `--text-value`, or give them a rule.
- **The bars, Settings and the dialogs.** The base sheet colours the window's background
  (`window`), the fills the bars, Settings, the dialogs and menus paint for themselves (`app-fill`),
  the toolbar's words and switches (the time signature `toolbar-module[kind="signature"]`, the
  launch quantisation `[kind="grid"]`, Map `button[kind="map"]`, Play `button[kind="play"]`, the
  browser's switch `button[kind="browser"]`, a connected agent `button[kind="agent"]`), a menu's
  title on the menu bar (`bar-item[kind="menu"]`, which also names the words of the piano roll
  toolbar's Grid, Workflow and Ghosts menus: scope a rule with `menu-bar` or
  `piano-roll-toolbar` to reach one of them), the status bar's counts
  (`bar-item[kind="status-counts"]`), Settings' pages' list (`settings-nav`, `[checked]` for the page
  shown), its switches, choices and presets (`button[kind="switch"]`, `"option"`, `"preset"`), a tick
  box (`button[kind="check"]`; the track colour window's choices, tick boxes, buttons and sliders
  are these same elements, with no rules of their own to write), the start page's tabs (`editor-tab[kind="start"]`), the theme
  picker's cards (`theme-card`), a dialog's title row and list rows (`dialog-title`,
  `list-row[kind="dialog"]`) and the drag pill (`tooltip`). **A rule for every `button` hides their
  lit looks too**: state `button[kind="map"][checked]`, `button[kind="option"][checked]`,
  `button[kind="preset"][checked]`, `button[kind="switch"][checked]` and
  `button[kind="check"][checked]`. A bar's plain `color` no longer reaches a menu's title or the
  tabs' names: colour the bar through `--text-secondary`, or give them a rule
  (`menu-bar bar-item[kind="menu"], project-tabs label[kind="caption"] { color: … }`).
- **egui's own widgets** (the fields, drop-downs, check boxes and entries of Settings and the
  menus) take `widget`: at rest, `:hover`, `:active`, `[checked]` for the selection,
  `[kind="window"]` for egui's windows and menus, `[kind="field"]` behind a text field. Its words
  at rest are `--text-value`, a `button`'s `--text-primary`; your rules for `button`, `text-input`,
  `menu` and `scroll-bar` still go over it. What egui draws that takes no input, its separators,
  frames and labels, is `widget[kind="static"]` (`border-color` and `border-width` its lines,
  `color` its labels), and no `button` rule reaches it: a `button:disabled` rule no longer changes
  egui's lines (#1487).
- **The base sheet's own colours** are variables as well: the grey of a muted Arrangement clip
  and track header (`--clip-mutedBackground`, `--clip-mutedSelectedBackground`,
  `--track-headerMutedBackground`, `--track-headerMutedSelectedBackground`), the browser's icons by kind
  (`--browser-icon-midi`, `--browser-icon-sample`, `--browser-icon-instrument`,
  `--browser-icon-effect`) and the sample map's sounds (`--sample-kick`, `--sample-snare`, …
  `--sample-other`).
- **Opting out.** `"base": "none"` in `theme.json` leaves the base sheet out: your theme stands on
  the user-agent sheet, whose window has plain light text on an opaque dark background (the
  menus, pop-ups, tooltips and dialogs over it the same background) and no other colours, and
  styles the rest itself. The colour variables still resolve, so your rules can
  use them.

## SVG inheritance and safe defaults

Suhina's user-agent style sheet, the first sheet under the base sheet and your theme, gives every
control its default artwork, on the element that draws it:

| Element | File |
|---|---|
| `button` | `button.svg` |
| `icon[kind="play"]`, `icon[kind="stop"]`, `icon[kind="record"]`, `icon[kind="solo"]`, `icon[kind="arm"]`, `icon[kind="cue"]` | `play.svg`, `stop.svg`, … |
| `icon[kind="midi"]`, `icon[kind="audio"]` | `track.midi.svg`, `track.audio.svg` |
| `track-header`, `track-name` | `track.body.svg`, `track.name.svg` |
| `knob-body`, `knob-pointer` | `knob.body.svg`, `knob.pointer.svg` |
| `fader-cap`, `fader-rail` | `fader.cap.svg`, `fader.rail.svg` |
| `meter` | `meter.svg` |

It writes them as `knob-body { image: url("suhina:knob.body.svg"); }`; a `suhina:` URL names an
SVG built into Suhina, and your theme may use one too. A state follows the element:
`button:hover`, `fader-cap:active`, `button:disabled`, `button[checked]` (a toggle that is on).
Example overrides:

```css
icon[kind="play"] { image: url("assets/play.svg"); }
button:hover { image: url("assets/button.hover.svg"); }
knob-pointer { color: #FFB000; }
```

Lookup uses the theme's state SVG, then its base SVG, then Suhina's default SVG.
Themes only need to define the images they replace. Removing an override restores
inheritance. Knob pointers rotate with the parameter and fader caps follow its value.
Use self-contained SVG paths and shapes; native labels stay native.

The default artwork takes its colour only from `currentColor`, the `color` of the element that
draws it, so a copy of it works in any theme. A control with two colours is two elements: style
`knob-body` and `knob-pointer`, or `fader-cap` and `fader-rail`, each with its own `color`.
Without a `color` of the theme's on the part itself, each keeps the colour Suhina gives it today:
a `color` on `knob` or on a view around a fader does not change a knob's body or a fader's rail
or cap, while a knob's pointer takes the knob's colour.

A theme made for an earlier version: artwork of its own for `knob.body`, `fader.rail` or
`fader.cap` that uses `currentColor` drew in the knob's pointer colour or the values' text colour,
and now draws in the part's `color`. Give the part that colour to keep the look
(`knob-body { color: var(--accent); }`, `fader-cap, fader-rail { color: var(--text-value); }`).
The `--kind-*` colours of the sample map are no longer read: set `--sample-kick` and the others,
or write `sample-point[kind="kick"] { color: #D06E5E; }` and so on instead.

*New theme…* has an **Include the default artwork** checkbox: it copies every default SVG into
the new theme's `assets/` and writes a rule for each into its `theme.css`
(`knob-body { image: url("assets/knob.body.svg"); }`), ready to edit. A theme card's right-click
*Add default artwork* does the same for a theme of yours, adding only what it lacks.

The rules of an earlier form (`knob.body { image: … }`, `play`, `solo:on`) still read.

Suhina's default artwork is built into the app (its sources are in the repository's
`assets/themes/default/`); there is no copy in the themes folder to edit. To change a control's
look, put your own SVG in your theme's `assets/` and name it in `theme.css`.
Portable exports include inherited artwork so another machine receives the same images.

## What's possible now

| Area | Supported customization |
|---|---|
| Palette | Core colour properties, transparency, project-safe creation defaults |
| Settings | The core settings, your own `@property` settings in the editor, variants (light, dark and others) |
| Artwork | SVG/PNG roles, colour/state variants and frame sequences, moving faders and rotating knob pointers |
| Meters | `continuous`, `segmented` (4–64 segments), `needle`, signal-driven `frames`; signal remains dBFS |
| Materials | Optional bounded PNG background, opacity/blur; panel/track/clip gradient and liquid settings |
| Toolbar | Native layout plus offsets, explicit order/visibility, connected Flex/Grid |
| Window | One CSS grid of the bars and the panels on `window`, which a theme rearranges |
| Styles | Connected foreground/font size/radius, linear/radial/conic and repeating gradients, bounded inset/outset box shadows and shared native text shadows |
| Motion | Connected paint transitions/keyframes; native animation-off/high-contrast settings suppress motion |
| Selectors | Retained native class tree, child/sibling matching, connected hover/active/disabled/focus states, named object identities |

`theme.css` runs on Suhina's own style engine, with Taffy for connected layout. For example:

```css
browser { color: #D8E8DF; background-color: #101C17;
           background-image: linear-gradient(135deg, #243A30, #101C17); }
#panel-browser { grid-area: right; margin: 0 0 0 8px; width: 240px; }
#panel-detail { height: 180px; }
```

**The window is one grid** on `window`: the bars, the lines over and under the views and the
three panels are its children, in named areas that Suhina's own sheet states, so with no rule of
yours the window looks as it always has. Give an element another `grid-area`, or rewrite
`window`'s `grid-template-areas`, to move it; `display: none`, or a template without its area,
hides it. The areas are `menu`, `tabs`, `toolbar`, `top` (the line over the views), `notice`,
`left` and `right` (the browser's sides), `main`, `side` (the detail area stacked beside the view),
`detail`, `bottom` (the line under the views) and `status`. The status bar at the top and the
toolbar under the views:

```css
window {
  grid-template-rows: auto auto auto auto minmax(0, 1fr) auto auto auto auto;
  grid-template-areas:
    "status status status status" "menu menu menu menu" "top top top top"
    "notice notice notice notice" "left main side right" "left detail detail right"
    "bottom bottom bottom bottom" "toolbar toolbar toolbar toolbar" "tabs tabs tabs tabs";
}
```

The settings that change the layout are attributes on `window`, which your rules can read:
`[detail="below"]`, `[detail="full-width"]` or `[detail="beside"]` (where the detail area lies),
`[browser="left"]` or `[browser="right"]` while the browser shows, and `[menu-bar]` while the
window has its own menu bar. A panel is a `workspace-panel` with `kind` `browser`, `main` or
`detail`; the closed detail area is `#panel-detail[closed]`.

Each panel draws its own background and border, as every element does, and holds its view in
its content box: `#panel-detail { border-width: 1px; border-color: var(--border); padding: 4px; }`
frames the detail area. A panel whose size the sheet sets (a `width` on `#panel-browser`, a
`height` on `#panel-detail`) has no splitter; one whose size it leaves alone keeps its splitter, in
the gap its margin leaves beside the view (4 points beside the browser, 6 over the detail area),
and dragging it sets the saved size. The splitters are elements of their own, `#detail-splitter`
and `#browser-splitter`, so a gap can stay clear: `#detail-splitter { background-color:
transparent; }`. A panel the grid places in the same cells as `#panel-main` is drawn over the
view and takes the pointer there, for example a browser floating over the left of the view:

```css
#panel-browser { grid-area: main; width: 260px; justify-self: start; margin: 0; }
```

`arrangement { track-headers: left; }` (or `right`) puts the Arrangement view's track
headers on that side while the user's **Track headers** setting is *Theme*, its default. A user
who chose Left or Right keeps their side.
The toolbar exposes `browser`, `tempo`, `signature`, `grid`, `count-in`, `transport`,
`position`, `metronome`, `mapping`, `performance`, `views`, `agents` module ids.
Device SDK controls are elements such as `device-panel`, `knob`, `button` and
`device-control`. Stable named track/clip/slot/scene scopes also exist.
The native centred toolbar exposes the same module ids as CSS layouts. Padding and a border on
`toolbar` pad it and keep it, and moving it in the window's grid (`grid-area`, margins) keeps it
too; `display`, flex and grid properties, `gap`, sizes or a module's placement or `order` switch
it to the CSS layout. Its main
buttons additionally expose `#control-play`, `#control-stop`, `#control-record`,
`#control-browser-toggle` and `#control-metronome-toggle`. Paint-only rules preserve
the existing geometry. The selector reference below lists every element and id.

## Selectors by area

Every part of the window is an element with a name, written as a type selector: `knob`,
`track-header`, `clip` (not `.knob`: elements have no classes). Many have an id too. The
area a part is in is one of its ancestors. Combine them as CSS does, and use `:is()` for a rule
over several kinds of parts:

```css
:is(value, pan, fader, send, knob, slider, device-control, clip-gain) { background-color: #203040; } /* every value a user drags */
arrangement :is(value, pan, fader, send, knob) { border-radius: 2px; }   /* only in the Arrangement */
#arrangement-pan { value-position: top; }                       /* one control */
```

Useful groups: every value a person drags is
`:is(value, pan, fader, send, knob, slider, device-control, clip-gain)`; a track's own buttons
`button:is(#control-solo, #control-cue, #control-arm, #control-track-on, #control-monitor, #control-automation)`;
the transport's `button:is(#control-play, #control-stop, #control-record, #control-metronome-toggle)`;
the browser's rows and places `:is(browser-row, browser-place)`; the timeline's parts
`:is(ruler, loop-region, loop-line, playhead)`. Settings such as `appearance` are properties
like the others: `mixer pan { appearance: knob; }` and `:is(pan, send) { appearance: knob; }`
both work, and the settings of a value (`appearance`, `value-position`, `value-decimals`,
`value-unit`, `drag-pointer`) are inherited, so `mixer { appearance: knob; }` reaches its pans
and sends. `drag-pointer: hidden | visible | auto` says whether the pointer hides while a value
control is dragged or shows the arrow of the drag's direction; `auto` keeps the user's setting.

| Area (ancestor) | Parts (element or id) |
|---|---|
| Toolbar `toolbar` | `toolbar-module`, modules `#browser`, `#tempo`, `#position`, `#signature`, `#grid`, `#count-in`, `#metronome`, `#views`, `#performance`, `#mapping`, `#agents`; `#control-play`, `#control-stop`, `#control-record`, `#control-metronome-toggle`, `#control-browser-toggle` |
| Arrangement header `arrangement track-header` | `track-name`, `track-number`, `#control-track-on`, `#control-solo`, `#control-cue`, `#control-arm`, `#control-automation`, `#control-monitor`, `fold`, `#arrangement-volume`, `#arrangement-pan`, `send`, `value-fill`, `value-handle` |
| Timeline `arrangement` | `ruler`, `ruler-number`, `ruler-tick`, `tempo-mark`, `loop-region`, `loop-line`, `marker`, `playhead`, `start-marker`, `grid-line`, `time-selection`, `marquee`, `track-lane`, `clip`, `clip-name`, `clip-waveform`, `clip-notes`, `clip-gain`, `clip-fade`, `clip-fade-handle`, `clip-envelope`, `take-header`, `automation-header`, `automation-lane`, `automation-line`, `automation-point`, `automation-pin`, `arrangement-tool`, `add-track`, `#add-midi-track`, `#add-audio-track`, `#add-return-track` |
| Launcher and mixer `session`, `mixer` | `track-column`, `slot`, `slot-mark`, `slot-record`, `scene`, `scene-column`, `mapping-overlay`, `#mixer-volume`, `#mixer-pan`, `pan-center`, `#mixer-send-N`, `fader`, `fader-cap`, `fader-rail`, `fader-scale`, `meter`, `meter-level`, `meter-clip`, `device-list`, `device-row`, `scroll-bar`, `#mixer-divider`, `#mixer-splitter` |
| Piano roll `piano-roll` | `piano-grid`, `piano-row`, `piano-row-black`, `piano-key`, `piano-key-black`, `piano-key-c`, `piano-key-hover`, `piano-key-black-hover`, `piano-key-line`, `piano-scale-mark`, `piano-scale-strip`, `piano-range`, `piano-time-selection`, `piano-insert-mark`, `piano-note`, `velocity`, `piano-slice`, `start-marker`, `loop-region`, `ruler-number`, `ruler-tick`, `piano-tool` with `#piano-<tool>`, `midi-tools` (with `slider#quantise-amount`), `clip-info`, `editor-tab` (`#tab-clip`, `#tab-devices`, `#tab-notes`, `#tab-transform`) |
| Browser `browser` | `browser-place`, `browser-row`, `browser-play`, `browser-tag-dot`, `browser-preview-wave`, `#browser-search`, `#browser-new-folder` (`text-input`) |
| Devices `device`, `#device-<id>` | `device-header`, `device-control`, `slider`, `slider-rail`, `slider-fill`, `slider-handle`, `knob`, `value` |
| Dialogs `settings`, `dialog`, `start-page`, `plugin-manager` | `button` (also egui's check boxes and drop-downs), `button[checked]` (a chosen item), `text-input`, `settings-nav`, `progress`, `progress-fill`, `menu`, `popup` |

States: the pointer's and focus are pseudo-classes, `:hover`, `:active`, `:disabled`, `:focus`;
what Suhina knows about a part is an attribute, `[checked]`, `[selected]`, `[muted]`,
`[playing]`, `[queued]`, `[recording]`, `[armed]` (`:checked` is not a state and does not parse). Artwork names its state `.hover`, `.active`, `.on`.

**Glow:** a shadow without an offset, `box-shadow: 0 0 12px #00c8ff`. A button drawn with artwork
casts it in the artwork's shape, under it; a button without a background of its own glows round
about its glyph unless the sheet gives it a `border-radius`.

**Text in SVG** is sized in the SVG's own units, so it shrinks with the artwork: in a 24 by 16
button graphic, `font-size="9"` is 9/16 of the button's height. Give text a size for the
artwork's own `viewBox`, or draw the glyph as a path.

**Not yet:** the toolbar's modules as one block (position, tempo and signature as one LCD); the
MIDI tools in a compact one-line layout.

## Selector reference

Elements, by name:

| Element | What it is |
|---|---|
| `window`, `menu-bar`, `application-bar`, `project-tabs`, `toolbar`, `status-bar` | the window and its bars |
| `workspace-panel` | the window's panels (`#panel-browser`, `#panel-main`, `#panel-detail`) |
| `notice-bar` | the strip over the views that asks about an opened project or names missing files |
| `toolbar-module` | one area of the toolbar, by its id (below) |
| `browser`, `session`, `arrangement`, `mixer`, `detail`, `clip-editor`, `piano-roll`, `sample-map` | the views and panels |
| `track-header`, `track-name`, `track-column`, `scene-column`, `scene`, `slot`, `clip` | tracks, scenes, slots and clips; Main's header is `track-header[kind="main"]`, a track's stop button `slot[kind="stop"]`, **Stop all** `scene[kind="stop-all"]`; a clip is `[playing]`, `[queued]`, `[selected]`, `[recording]` and, deactivated, `[muted]`; a selected track's column is `track-column[selected]`, whose translucent background the base sheet gives from `selection.background` and which is drawn over the whole column where it differs from `track-column`'s own (a background for every column stays under the slots) |
| `waveform`, `envelope` | what a clip shows |
| `device-panel`, `device-section`, `device-control`, `xy-control` | device panels; `#id` selects a device control |
| `button`, `value`, `value-fill`, `dropdown`, `dropdown-arrow`, `text-input` | buttons, value fields, the lit part of a level field, menus and text fields; a field's underline is its `border-color`, and its `accent-color` while it is edited; a drop-down's arrow keeps its own `color` (every drop-down's, the lists' pickers such as the quantization and count-in ones too), so a rule for the field's text names `dropdown-arrow` too; a track header's routing fields, which take the header's colours, are `dropdown[kind="subtle"]` (a rule for every `dropdown` leaves them alone); a number typed into a value control is `text-input[kind="typed"]` (its ring's `border-color`, its `background-color`, its `color`; a rule for every `text-input` is its too, until `text-input[kind="typed"]` has its own) |
| `button[kind="toggle"]`, `#control-solo`, `#control-cue`, `#control-arm` | a track's solo, cue and arm buttons and the other switches Suhina draws as buttons (`[kind="solo"]`, `[kind="arm"]`, `[kind="toggle"]`), `[checked]` while on (the arm `[armed]` too): a rule for every `button` (its colour or its background image) is the lit button's too, so state its lit look (`button[kind="toggle"][checked] { background-color: … }`, and for solo and arm `button[kind="solo"][checked]`, `button[kind="arm"][checked]`); under the pointer they are ringed in their `border-color`, and in their `accent-color` while they have the keys |
| `toggle` | a switch a view draws itself, not as a button (the detail panels' switches), `[checked]` while on: its `color` and `background-color`, a step brighter under the pointer; the base sheet gives it the colours of `button[kind="toggle"]` |
| `slider`, `slider-rail`, `slider-fill`, `slider-handle` | a slider and its parts' `background-color`; a swing amount is `slider[checked]` while it is the clip's own, its fill then the accent: a rule for every `slider-fill` is the lit one's too, so state `slider[checked] slider-fill` |
| `menu`, `popup` | an open menu of the menu bar; a control's popup (`#metronome-level`, the metronome's level) |
| `knob`, `fader`, `fader-cap`, `fader-scale`, `pan` | mixer and device controls (`fader-scale` also the clip gain fader's); a dial (an audio clip's Pitch, the EQ's) is a `knob[kind="dial"]` with its rim in its `border-color` (as wide as its `border-width`, else a sixth of the dial's radius), the disc between rim and face in `knob-ring`'s `background-color`, its inner disc in its `background-color` and its pointer in its `accent-color` |
| `meter`, `meter-level`, `meter-vertical`, `meter-horizontal` | level meters and their lit part |
| `label`, `separator`, `settings`, `dialog` | labels, rules, the Settings page and dialogs (one `dialog` element, told apart by `name`; see *Dialogs*) |
| `app-fill` | the fills the bars, Settings, the dialogs and menus paint for themselves where the region behind gives none, by `kind` (`window`, `panel`, `panel-hover`, `control`, `control-hover`, `control-active`; `knob` the knob of a switch that is on over a see-through window) |
| `widget` | egui's own widgets under your `button`, `text-input`, `menu` and `scroll-bar` rules: at rest, `:hover`, `:active`, `:disabled`, `[checked]` (the selection), `[kind="window"]`, `[kind="field"]` |
| `label[kind]` | the browser's and the detail panels' words by how much they stand out: dim words (`hint`) and a control's or figure's name (`caption`) take only a colour given to them, not one around them; a panel's title (`title`), a value shown as words (`value`), a fault (`error`), a plug-in's state (`plugin-state`), a device parameter's name (`parameter`), a check box's words (`check`), the head of the MIDI tools' figures (`head`, `[selected]` in its `accent-color`), words or a mark in the accent (`accent`), and the light and dark inks of text on a colour (`on-light`, `on-dark`, from `--text-onColorLight` and `--text-onColorDark`); inside the browser `hint`, `caption` and `value` take the browser's own colours |
| `browser-fill`, `detail-fill` | the fills the browser and the detail area paint for themselves where the region behind gives none, by `kind` (`panel`, `window`; the browser's splitter `splitter`, stronger under the pointer; the detail's `control` and `hover` for the hide and close buttons, `outline` the edge of a button that cannot be pressed) |
| `device-canvas` | the colours a device's own drawings take, read in the device's scope: `color` the text, `background-color` the controls, `border-color` the lines, `accent-color` the accent; `kind="secondary"`, `"value"`, `"inverse"` (`color`) and `"panel"`, `"window"`, `"active"` (`background-color`) the others |
| `#control-device-on` | a device's on switch (`[checked]` while it runs): a rule for every `button` reaches it, as it does any button, so a theme with one states `#control-device-on[checked]` |
| `button[kind="close"]`, `button[kind="more"]` | an expanded panel's × and the browser's more tags: only a rule for them reaches them, not one for every `button` |
| `device[kind="group"]`, `device-row` | a device group's panel, and its branches (`[selected]` while shown, `[muted]` while muted); its `+ Branch` is `add-track[kind="branch"]`; a device's `accent-color` outlines it while selected |
| `icon[kind]` in the piano roll | its tool pointer (`pointer`, the ring its `background-color`) and the strip's magnifier (`magnifier`); a clip's colour swatch is `icon[kind="swatch"]` |
| `start-page`, `plugin-manager` | the page with no project open, the Plug-in manager |
| `dialog-title`, `dialog-actions`, `list-row`, `colour-swatch`, `theme-card` | a dialog's title row (`background-color`; its words `label[kind="title"]`); a dialog's row of buttons (a flex row: `justify-content`, `gap`); a row of a list a click picks, in a dialog, on the start page, in the quick-add and project switcher lists and the Plug-in manager (`[selected]` for the picked one); a swatch of the colour picker (`border-color` outlines the chosen one); a theme's card in the picker (`[selected]` for the theme in use: `border-color`, `background-color` for its tint, `accent-color` for the word *Active*) |
| Control bar, tabs, Settings and dialogs | their words are `label` by kind (`hint` for headings and notes, `caption`, `title`, `value`, `error`), their rules, and the outlines of the start page's cards, `separator[kind="row"]`, their buttons `button` (lit ones `[checked]`: a rule for every `button` leaves them lit), the start page's tabs `editor-tab#tab-recent` and `#tab-templates`, the toolbar's RAM and CPU bars `meter` and `meter-level` (`[kind="hot"]`); `divider#detail-splitter[closed]` while the detail area is closed |
| `send` | a send, in a mixer strip and in a track header |
| `add-track` | the **+ MIDI track**, **+ Audio track** and **+ Return track** buttons after the last track |
| `ruler`, `loop-region`, `playhead` | the Arrangement view's ruler, its loop bar and the play position line (also the piano roll's and the browser preview strip's) |
| `divider` | the line between a strip's device list and its controls (`#mixer-divider`), the Launcher mixer's top line (`#mixer-splitter`), and the window's splitters (`#detail-splitter`, `#browser-splitter`) and the Arrangement's track header edge (`#arrangement-header-splitter`, painted only where a theme gives it a `background-color`) |
| `separator[kind]` | the views' lines: the Launcher's between its bands (`band`), its columns (`column`) and its slots (`slot`); the Arrangement's under its rows (`row`), between its bands (`band`) and under each lane (`lane`), in their `color` |
| `slot-mark`, `slot-record`, `mapping-overlay`, `drop-target` | the mark in an empty slot with a stop button; an armed slot's record button (`[recording]`); the tint over a control while keys are mapped (`[selected]` on the picked one); where a drop lands (`kind="area"`, `kind="insert"`, and `kind="append"`, the slot with a + after a chain's last device) |
| `grid-line`, `tempo-mark`, `start-marker`, `marker` | the Arrangement's grid lines (`kind="bar"`, `kind="beat"`), a tempo or time-signature change, the play start marker, a marker's tag |
| `time-selection`, `marquee` | selected time (`background-color` over the lanes, `color` at its edges; `kind="ruler"`, `"draw"`, `"editor"`, `"take"`, `"automation"`), and the rectangle that picks clips, notes (piano roll) or sample-map points |
| `clip-fade`, `clip-envelope`, `take-header` | a fade's line (`kind="crossfade"` between takes), a clip's gain envelope while it is edited, a take lane's header |
| `automation-header`, `automation-lane`, `automation-line`, `automation-point`, `automation-pin` | an automation lane's header (`kind="group"` for a group's), a group's time, the envelope (`[muted]` while off), its points (`[selected]`) and the pin; the line and points are `[kind="fixed"]` while Settings › Arrangement › Automation colour is Fixed, which the base sheet colours with `var(--accent)` |
| `device-list`, `device-row`, `scroll-bar`, `pan-center`, `meter-clip`, `arrangement-tool` | a mixer strip's device list and its rows, how far its sends (or the device chain) are scrolled, a pan field's centre ticks, a meter's clip light, the Arrangement's own tool buttons |
| `clip-name`, `clip-waveform`, `clip-notes`, `clip-gain`, `clip-fade-handle` | an arrangement clip's name bar, waveform, notes, gain field and fade handles |
| `value-handle`, `fader-rail` | the handle where a level bar's fill ends; a fader's rail (the mixer's and the clip gain fader's) |
| `piano-grid`, `piano-row`, `piano-row-black`, `piano-key`, `piano-key-black`, `piano-note`, `velocity` | the piano roll's grid (`kind="strong"` on an octave and a bar), its rows (`kind="hover"` under the pointer), its keys (with `[selected]` and `[playing]`), the notes (`kind="draft"` while a drag makes them; their own inks `kind="outline"`, `"dashes"` and `"name"`, `[checked]` on a strong note) and their velocity stems (`[selected]`, `[muted]`; `kind="line"` for a velocity drag's line) |
| `piano-slice` | a slice drag's line (`kind="cut"`, `kind="delete"` for the right button's, with a `background-color` over what it deletes) |
| `piano-key-c`, `piano-key-hover`, `piano-key-black-hover`, `piano-key-line`, `piano-scale-mark` | a C key, a key under the pointer, the line between white keys, the mark on a key in the clip's scale |
| `piano-scale-strip`, `piano-range` | the strip left of the keys that names the octaves and the row under the pointer, and the range a drag on the keys is selecting |
| `piano-clip-strip` | the strip over the ruler that shows several picked clips (background) |
| `piano-time-selection`, `piano-insert-mark` | what is selected in time (background over the rows, `border-color` for its edges and the bar on the ruler), and the mark a click leaves where a paste goes (background) |
| `piano-tool`, `midi-tools`, `clip-info`, `editor-tab` | the piano roll's tool buttons (`[checked]`, `:hover`, `:disabled`), the MIDI tools part of a MIDI clip's panel, the clip's column, and the **Clip** / **Devices** switch (`editor-tab[kind="switch"]`) and **Notes** / **Transform** tabs (`kind="tab-notes"`, `"tab-transform"`) |
| `device`, `device-header` | a device's panel in the device chain, and its header row |

Ids:

| Id | What it is |
|---|---|
| `#browser`, `#tempo`, `#signature`, `#grid`, `#count-in`, `#metronome`, `#transport`, `#position`, `#views`, `#performance` (RAM and CPU), `#mapping`, `#agents` | the toolbar's areas (`toolbar-module`) |
| `#control-play`, `#control-stop`, `#control-record`, `#control-browser-toggle`, `#control-metronome-toggle` | the toolbar's main buttons |
| `#control-solo`, `#control-cue`, `#control-arm`, `#control-track-on` | a track's solo, cue, arm and on (the track number in the mixer) buttons |
| `#arrangement-volume`, `#arrangement-pan` | a track header's volume and pan fields |
| `#main-volume` | the volume field in Main's row of the Arrangement view |
| `#mixer-volume`, `#mixer-pan` | a mixer strip's fader and pan |
| `#panel-browser`, `#panel-main`, `#panel-detail` | the window's panels |
| `#views-top`, `#views-bottom` | the lines over and under the views (`separator`) |
| `#mixer-send-1`, `#mixer-send-2`, … | a mixer strip's sends, from the left |
| `#add-midi-track`, `#add-audio-track`, `#add-return-track` | the three add-track buttons |
| `#mixer-divider` | a strip's divider |
| `#mixer-splitter` | the Launcher mixer's top line, which a drag resizes |
| `#control-main-metronome`, `#control-mixer-monitor-in`, `#control-mixer-monitor-auto`, `#control-mixer-monitor-off` | the metronome in Main's strip and a strip's monitor buttons; while on, they take a colour only from a rule for `[checked]`, so a rule for every `button` does not hide which one is on |
| `#control-back-to-arrangement`, `#control-returns-toggle`, `#control-take-delete`, `#control-take-solo`, `#control-take-promote`, `#control-clip-envelope-edit`, `#control-clip-envelope-free` | the Arrangement's buttons: back to arrangement, show or hide returns, a take lane's buttons, a clip's gain envelope buttons |
| `#waveform-zoom` | the Arrangement's waveform zoom (`slider`) |
| `#detail-splitter`, `#browser-splitter` | the splitters beside the detail area and the browser: `background-color` for the strip, `color` for the detail splitter's grip, with `:hover` and `:active` |
| `#piano-select`, `#piano-draw`, `#piano-paint`, `#piano-erase`, `#piano-slice`, `#piano-mute`, `#piano-cut`, `#piano-copy`, `#piano-paste`, `#piano-duplicate`, `#piano-quantise`, `#piano-midi-tools`, `#piano-audition`, `#piano-hide` | the piano roll's tool buttons |
| `#tab-clip`, `#tab-devices` | the clip editor's two tabs |
| `#device-sampler`, `#device-fm-synth`, `#device-utility`, `#device-auto-pan`, `#device-dj-eq`, `#device-vinyl-distortion`, `#device-overdrive`, `#device-compressor`, `#device-limiter`, `#device-nam-amp-modeler`, `#device-nam-cab-loader`, `#device-delay`, `#device-reverb`, `#device-pitch` | one built-in device's panel; `#device-plugin` a plug-in's, `#device-sd` a device file's |

A view's or bar's background is the surface behind its controls: `toolbar` or `arrangement`
with a `background-color` does not repaint the buttons on it. A control's own surface takes
`background-color`, `background-image`, `border-radius`, `border-width` with `border-color`
(no border is drawn without a width; `border-width: 0 0 1px` or `border-bottom-width` and the
other sides draw some sides only), `box-shadow` and `opacity`; `transparent` paints nothing.
A button shaped this way is not covered by the artwork Suhina ships, only by your own
`button` artwork. For example, a toolbar as one surface with recessed displays:

```css
#browser button, #metronome button { background-color: transparent; box-shadow: none; }
#browser button:hover, #metronome button:hover { border-width: 1px; border-color: var(--accent); }
#performance { background-color: transparent; }
#tempo, #position { background-color: var(--my-well); border-radius: 4px; }
```

Volume and pan fields are level bars. The field (`#arrangement-volume`) is the track and its
`value-fill` the lit part; `height` sets the thickness of either, so a thin line with a lit
segment is:

```css
#arrangement-volume { height: 4px; border-radius: 0; background-color: #2A3238; }
#arrangement-volume value-fill { background-color: var(--accent); }
```

Artwork of your own replaces either: `volume.track`, `volume.fill`, `pan.track` and `pan.fill`.
`color: transparent` on the field hides its value text.

### States

Besides the pseudo-classes `:hover`, `:active` (pressed), `:focus` and `:disabled`, Suhina gives
an element these attributes, without a value:

| Attribute | On |
|---|---|
| `[checked]` | a toggle that is on: solo, cue, arm, the metronome, the browser toggle, play while playing |
| `[playing]`, `[recording]` | `#control-play` while the transport runs, `#control-record` while it records |
| `[selected]` | the selected track's `track-header`, `track-column`, `mixer` strip and `track-name` |
| `[armed]`, `[muted]` | an armed track's arm button and header; a muted track's header and strip |
| `[selected]`, `[muted]`, `[playing]`, `[recording]` | an arrangement clip that is selected, off (or on a muted track), under the play position while it plays, or under a take being recorded |
| `[selected]`, `[muted]` | a piano roll note that is selected or muted; a device that is selected or off |
| `[checked]` | the loop bar while looping is on, the chosen piano roll tool or open column, the tab shown |

```css
#control-metronome-toggle[checked] { background-color: var(--accent); color: #000; box-shadow: 0 0 6px var(--accent); }
track-header[selected] track-name { color: #FFFFFF; }
```

A button's glyph takes the button's `color`, in every state. Artwork has an on state of its own,
`icon[kind="solo"][checked] { image: url("assets/solo.on.svg"); }`, which falls back to
`:active` artwork.

### Tracks

A track's colour is the project's, chosen by the user; a theme draws it but never chooses it,
and the colour picker offers the same colours in every theme. A track's header (both views),
Launcher column, mixer strip and Arrangement lane (`track-lane`) have it as a `color` attribute,
the final `#rrggbb` in lowercase. `attr(color type(<color>))` reads an element's own attribute,
so hand it to the parts inside with a variable of your own:

```css
track-header, mixer { --track: attr(color type(<color>)); }
session track-header track-name { background-color: var(--track); color: contrast-color(var(--track)); }
arrangement track-name { background-color: #2A2C2E; border-width: 1px; border-color: var(--track); color: #E6E6E6; }
mixer track-name { background-color: color-mix(in oklab, var(--track) 40%, #1E2023); }
mixer[muted] track-name { background-color: #2A2C2E; }
track-lane { background-color: color-mix(in srgb, attr(color type(<color>)) 8%, transparent); }
```

Keep the colours recognisable: people tell tracks apart by them. Tint them, mix them with the
background, use them for borders, rather than replacing them. Mix in `oklab` (or `srgb`):
`oklch` turns hues towards the hue of a nearly grey background, so a dark blue mixed with a warm
dark grey comes out maroon. On a light background keep at least about 60 % of the colour, or a
pale yellow fades into it. A lane's background is painted over
the grid lines, so keep it translucent. A selector for one colour takes the `i` flag:
`track-header[color="#FF9A3C" i]`.

A name's `color` sets its text; without one it is worked out from the fill. `mixer` is a strip,
also in the Launcher view, so `mixer track-name` is the strip's name and
`track-header track-name` the column's. Artwork for one view only: `track.name.session`,
`track.name.mixer`, `track.name.arrangement`, `track.body.session` and
`track.body.arrangement`, over `track.name` and `track.body`.

### Clips

A clip, in the Arrangement or in a Launcher slot, has its colour as a `color` attribute (its
own, or its track's when it has none), and takes `opacity` (the
whole clip fades, its waveform, notes and name with it), `border-radius` and a background:

```css
clip { border-radius: 4px; background-color: color-mix(in oklab, attr(color type(<color>)) 70%, #000); }
clip[selected] { border-width: 1px; border-color: #FFFFFF; }
clip[muted] { opacity: 0.5; }
clip[playing] clip-name { background-color: var(--accent); color: #000; }
clip-name { font-size: 10px; color: #F0F0F0; }
clip-waveform, clip-notes { color: #101214; }
clip-gain { background-color: #00000080; color: var(--accent); }
clip-fade-handle { background-color: #FFFFFF; border-radius: 2px; }
```

The waveform's and notes' `color` applies while **Settings › Arrangement › Waveform colour** is
**Theme** (its default); the user's other choices there win. A setting saved before Theme
existed reads as the choice it was (Dark, for most): pick Theme there to see the theme's
colours. `clip.fade-handle` is artwork for
the fade handles. A new clip takes its track's colour while **New clip colour** is **Track
colour** (the default); with **By clip type** it takes Suhina's MIDI or audio colour.

The piano roll has the edited clip's `color` attribute, and its notes their own rules:

```css
piano-roll { --clip: attr(color type(<color>)); }
piano-note { background-color: var(--clip); border-radius: 2px; }
piano-note[selected] { border-color: #FFFFFF; }
piano-row-black { background-color: #15171A; }
piano-key { background-color: #E8E8E8; color: #202020; }
piano-key-black { background-color: #202020; }
piano-grid { border-color: #3A3D42; }
velocity { color: var(--accent); }
piano-tool[checked] { background-color: var(--accent); color: #000; }
```

Each tool button takes artwork of its own: `piano.draw`, `piano.draw.on`, `piano.select.hover`,
and so on for every id above, after `piano.`.

### The ruler, the loop and the play position

`ruler` gives the ruler its background, its bar numbers' `color` and their lines'
`border-color`; `ruler` is its artwork. The loop bar is `loop-region` (`[checked]` while
looping is on): `height` sets its thickness, `background-color` and `border-color` its look,
and `loop-region` (`.on` while on) is its artwork. `playhead { color; width }` sets the play
position line's colour and thickness; `width: 0` hides it.

### Sends and the add-track buttons

`send` styles every send, in the mixer and in the Arrangement header, and `#mixer-send-1`
one of them. `send { appearance: knob; }` draws the mixer's sends as knobs. `send` is
a send's artwork and `send.fill` its level. `add-track` and `#add-midi-track` (and the two
others) style the add-track buttons, `add-track` is their artwork.

### Level bars: the value and a handle

On a level bar (`#arrangement-volume`, `#mixer-pan`) `text-align` moves the value,
`font-size` sizes it, `value-fill { color }` is the value's colour where it stands on the
lit part, and `color: transparent` hides it. `value-handle { width: 6px; background-color: … }`
draws a handle where the fill ends; `volume.handle` and `pan.handle` are artwork for it.

### Piano roll sizes

`piano-tool { width; height }` sizes the tool buttons (8 to 96 points wide), so stretched
artwork has room; `midi-tools { width }` (120 to 480) and `clip-info { width }` (120 to 480)
set the MIDI tools part's and the clip column's widths. `loop-region { height }` above the native row
grows the Arrangement's loop row with it (up to 40 points).

### Fader size

`fader-cap { width: 24px; height: 14px; }` sets the mixer fader's cap (4 to 64 by 4 to 96
points) and `fader-rail { width: 3px; }` its rail (1 to 32).

### Device panels

`device` styles every device's panel in the device chain and `#device-compressor` one kind of
device, by the built-in device's id in the table above. Its `[selected]` and `[muted]` (off)
states are there too, and everything in the panel is inside that scope: its header
(`device-header`), its controls (`#device-compressor device-control`) and the generated
parameter list's widgets (`#device-compressor button`).

```css
#device-compressor { background-color: #1B1E22; }
#device-compressor device-header { background-color: #E5484D; color: #FFFFFF; }
#device-reverb device-control { accent-color: #7CC4FF; }
```

`width` on `device` or `#device-plugin` widens a panel (up to 1600 points). A panel's
`background-color` is the surface behind its controls, not their fill, so its sliders keep
their rails. A parameter's slider has parts of its own once the sheet gives one of them a
background or artwork: `slider-rail` (`height` is its thickness), `slider-fill` (up to the
value) and `slider-handle` (`width`), with the artwork roles `slider.rail`, `slider.fill` and
`slider.handle`. `slider { appearance: knob; }` draws sliders as knobs.

```css
#device-plugin { width: 640px; background-color: #0E1A24CC; }
#device-plugin slider-rail { height: 3px; background-color: #FFFFFF22; }
#device-plugin slider-fill { background-color: var(--accent); }
#device-plugin slider-handle { width: 6px; background-color: #FFFFFF; border-radius: 3px; }
```

### Buttons and fields everywhere

`button` (with `:hover`, `:active`, `:disabled` and `[checked]` for a chosen row in a list or
menu) and `text-input` style Suhina's ordinary widgets too: the buttons, check boxes,
drop-downs and text fields in Settings, in dialogs (Export audio, Save, the loading and closing
dialogs), on the start page, in the browser, the sample map, the Plug-in manager and in
popups. Scope them to one place with its element: `settings button`, `dialog text-input`.
`border-width: 0` removes egui's own outline. egui draws one outline all round, so sides of
different widths (`border-width: 0 0 2px`) give these widgets only the `border-color`; the
browser's search field and Settings' theme swatches draw each side. Settings' choices of several parts (Ask, Panel,
On) are `button` parts too, the chosen one `[checked]`.

A rule that is not scoped reaches egui's own widgets everywhere, wherever a view draws one
outside a style scope: `button` (`:hover`, `:active`) their states, `button[checked]`
the selection, `text-input` their fill and text colour (`text-input:focus`'s `border-color` a
focused field's outline), `menu` the fill, border, `border-radius` and outer `box-shadow` of every
menu and pop-up egui frames (egui's own windows share them), and `scroll-bar` (`:hover`,
`:active`) a scroll handle's fill, which egui's sliders and check marks use too. egui has no
value of its own for tooltips: `tooltip` styles the frame painted over them. A theme with no such
rule keeps the colours of its tokens. A rule with a `border-color` and no `border-width` keeps the width
egui draws with; only a width, `0` too, changes it. A fully transparent `border-color` on egui's lines
(`widget[kind="static"]`) or on its menus' edge is width 0 as well, so a line you hide leaves no gap. `button[checked]` gives the selection only what it says unlike
the plain `button`.

egui's separators, the edges of its group frames and its labels are not buttons: style them with
`widget[kind="static"]`, and hide its lines with `widget[kind="static"] { border-color: transparent; }`.
A disabled egui widget is drawn faded in its own state's look, so `button:disabled` does not reach
egui's widgets at all.

A region's own `border-radius` and border (a `menu`'s, `settings`', a `dialog`'s) frame the region;
the widgets inside take theirs from the `button` rules that reach them, so a round menu does not make
its rows pills. Give the rows their own with a scoped rule: `menu button { border-radius: 4px; }`.
A drop-down's or a text field's own radius and border are its field's; the rows of a drop-down's
open list keep the field's words but take their radius, border and fill from `dropdown button`.

```css
button { background-color: #24272B; border-width: 0; border-radius: 4px; }
button:hover { background-color: #2E3238; }
button[checked] { background-color: var(--accent); color: #000; }
text-input { background-color: #15171A; }
```

### Dialogs

Every dialog is one element, `dialog`, told apart by its `name` attribute: `about`, `agent`,
`ara-edits`, `backup`, `closing`, `color-picker` (the track colour window), `export`,
`folder-drop`, `group-removal`, `loading`, `missing-files`, `move-folder`, `name-clash`,
`project` (New or open), `save`, `splash`, `theme-exists`, `unsaved` and `updates`. One rule
styles them all, and one more styles one:

```css
dialog { background-color: var(--panel-background); border-color: var(--border-strong); border-radius: 8px; padding: 12px; }
dialog[name="color-picker"] { padding: 6px; }
dialog-title { background-color: transparent; }
dialog-actions { justify-content: center; gap: 12px; }
```

- **Its box** is the sheet's: `background-color`, `background-image`, `border-*`,
  `border-radius`, `box-shadow`, `backdrop-filter` and `padding`.
- **`padding` is the room inside it**: what it holds sits that far in from its border. Its title
  row (`dialog-title`) runs along its top inside the border and outside the padding, so the
  padding never moves the title row.
- **Its buttons** stand in a `dialog-actions` row along the bottom of what it holds, laid out as a
  flex row: `justify-content` and `gap` place them. Where a dialog puts a button at each end
  (Updates' *Check now*), a `flexible-space` between them takes the room left.
- **Without the base sheet** (`"base": "none"`) a dialog has the window's opaque background,
  `padding: 10px` and its buttons at the end of the row, 8 points apart. The base sheet gives
  Suhina's themes `padding: 8px`, buttons 4 points apart, and no padding on `missing-files`, whose
  own title and status bars run to its edges.

### Windows of their own

Settings, the track colour window and the sample map can open in a window of their own. Each is
made like the main window: its base is `window`'s background, so a see-through theme shows the
desktop there too, and on macOS its content runs under the system's title bar. The element that
fills such a window (`settings`, `dialog[name="color-picker"]`) is its root, as a page's root
element is in CSS: its `background-color` and `background-image` cover the whole window, title bar
included, and its `padding` keeps what it holds in from the window's edge. Its `border-*`,
`border-radius`, `box-shadow` and `backdrop-filter` are not drawn there: the system draws the
window's edge and its corners, and nothing of Suhina's is behind it to blur. So one rule for
`dialog` styles the colour window over the main window and in its own:

```css
dialog { background-color: rgb(255 255 255 / 0.6); border-radius: 12px; backdrop-filter: blur(24px); }
```

Over the main window it is a rounded, blurred pane; in its own window it is that window's
see-through background over `window`'s.

### How an element is painted: padding, overflow and outline

Every element with a box paints in one order, as CSS does: its outer `box-shadow`, its
`background-color`, its `background-image` gradients (or its artwork), its inset shadows and
its `border`, then its contents, and last its `outline` (`outline: 2px solid #FFFFFF;
outline-offset: 1px;`, or `outline-width`, `outline-style`, `outline-color` and
`outline-offset`), outside its border, over its contents. A row or highlight that reaches a
panel's edge is drawn over the panel's inset shadows, as in a browser, and over its border only
with `overflow: visible`: a panel is `overflow: hidden`, which cuts its children at the padding
box, so its border stays whole. To keep an edge in sight, pad the panel (Suhina Meadow pads its
panels by 1 px on the sides it lights).

An element's contents go in its **content box**: its box less its border and its `padding`
(`padding: 4px 8px;` or `padding-top` and the others), in points or `em`. A percentage is
taken only on `window` and `toolbar`, whose children the CSS layout places; elsewhere the
sheet refuses it with a notice. A size Suhina works out from what an
element holds grows by its border and padding, as a CSS box with `height: auto` does: each
bar's height, each dialog's size, a device panel's size and the height the detail area opens at
for a row of devices or the piano roll. A size you drag (the detail area, the browser) is the
whole box, and so is a `width` or `height` your sheet gives. Padding larger than its box leaves
no room for the contents.

`overflow: hidden` (or `clip`) cuts everything an element's children paint at its padding box
(its box less its border), their outer shadows and outlines included; `overflow: visible`, the
default for every element but a region, lets them pass. Regions (the bars, the panels, the
views, dialogs) are `hidden`. The cut is square: a panel with a `border-radius` does not round
its children's corners, so pad it and round the children too. It cuts painting only: a control
cut off still takes the pointer. Menus, tooltips and dragged items are drawn above the views,
and no region cuts them; a menu's or popup's own `padding` is not used yet. A toolbar control's
glow passes the edge of its section (`toolbar-module`) and stops at the toolbar's. A list
scrolled so that a control is partly out of view cuts its glow and outline at the list's edge
too: they never cover what is beyond the scrolled list.

### Bars and lines

A bar's border is a box round it, as any element's is. The line under each bar over the views
(`application-bar`, `menu-bar`, `project-tabs`, `toolbar`) is its bottom border, and the line
over `status-bar` its top border: 1 px each from Suhina's own sheet, in `--border` unless you
give the bar a `border-color`. `border-width: 0` takes a bar's line away, and the bar is that
much shorter, so no gap shows between bars; `border-width: 0 0 2px` makes it thicker. The
lines over and under the views are `#views-top` and `#views-bottom`, painted in their
`border-color`, so a theme can join the bars
into one surface without losing the lines elsewhere. `--border` is every other line: rulers,
the browser's edges and the piano roll's keys and rows. A gradient over the whole stack of bars
is not possible yet: give each bar its share.

`arrangement { header-tint: 0; }` (0 to 100) sets how strongly the track's colour
tints its Arrangement header, over the user's header colour strength: 0 leaves the header
`track-header`'s own surface, for a see-through theme.

`border-radius` takes a percentage: `50%` makes a control round, and its `box-shadow` follows.

### One sheet

There is one sheet, `theme.css`. When you choose artwork in the app, Suhina
changes that declaration in it, or adds it at the end when the sheet has none.

### The strips' parts

A mixer strip (`mixer`; `mixer[narrow]` at 96 points or less, Main's `mixer[kind="main"]`) is a
row of `track-colour` and `group#mixer-body`, a column from the bottom up: `track-name`,
`group#mixer-output`, `device-list`, `divider#mixer-divider`, `pan#mixer-pan`,
`value#mixer-volume-readout`, `group#mixer-level` (`fader#mixer-volume`, `fader-scale`, `meter`),
`value#mixer-volume` (a low strip's volume), `group#mixer-input` (its caption, the source and
`group#mixer-monitor`), `group#mixer-buttons` (`#control-track-on` with the track's number, and
`mixer-toggles` of `#control-solo`, `#control-cue` and `#control-arm`; Main's
`#control-main-metronome`) and `group#mixer-sends`.

A track header (`track-header`) is a row of `group#header-indent` (the groups' bands),
`track-colour`, `group#header-body` and `meter`. The body holds `group#header-first` and
`group#header-second`, the lines of `fold`, `#control-automation`, the take lanes' button,
`#control-track-on`, `#control-solo`, `#control-cue`, `#control-arm`, `#control-monitor`,
`track-name`, `#arrangement-volume`, `#arrangement-pan`, the `send`s and their ‹ ›, then the
routing `dropdown`s. Which line holds what is `track-header[layout]`: `short` (one line),
`narrow`, `side` or `stacked`; `[name-first]` when the name's line comes first, and `[size]`
(`tiny`, `small`, `normal`, `spacious`) by the track's height. Keep the column as wide as your
layout needs with `arrangement { header-min-width: 330px; }` (160 to 520).

The transport (`toolbar-module#transport`) lays out `#control-play`, `#control-stop` and
`#control-record`.

### Round 5 additions

- **Opaque themes never show the desktop.** Backgrounds write their alpha into the window only
  while `--window-background` has alpha; otherwise the window clears to its colour and every
  alpha in the theme blends with Suhina's own surfaces. A clip's body, the edited track's
  highlight and a control's surface always blend.
- **The Arrangement header:** the track-on toggle is `#control-track-on` as in the mixer
  (`[checked]` on, `[muted]` off, artwork `track-on`); its number is `track-number` (`color`,
  `font-size`, `display: none`, `[muted]`). `#control-automation` (`[checked]` while its lanes
  show, artwork `automation`), `fold` (`[checked]` while folded, artwork `fold`) and
  `#control-monitor` (`[checked]` while monitoring is In, artwork `monitor`).
- **Artwork reads the theme's variables:** `var(--name)` in an SVG takes the theme's `:root`
  value; a track's colour reaches artwork only as `currentColor`.
- **Values:** `value-position: right | left | top | bottom | inside | none` on a knob
  (`pan`, a volume knob, `send`, the MIDI tools knob) places its value or hides it; `none` on
  a level bar hides its value.
- **Sizes:** `send { width; height }` for the mixer's sends; `arrangement {
  track-height: 96px }` (20–400) for tracks without a height of their own, in place
  of the default size; `piano-tool { height }` grows the piano roll's toolbar.
- **Device panels:** with `device { width }` the generated list keeps one column that follows the panel and scrolls;
  `device value { width }` sets the value column (at most half the row);
  `device { grid-template-columns: repeat(3, 1fr); }` lays parameters side by side (1–8
  columns), and `device device-control { height: 48px; }` makes each row that tall (16–160
  points).
- **Browser:** `browser-place` (a place or kind on the left: `:hover`, `[checked]` and
  `[selected]` while shown) and `browser-row` (`:hover`, `:active`, `[selected]`; its `color` is
  the name's).
- **Loop lines:** `loop-line { color; width }`; the lines reach the returns and Main and
  follow the brace while it is dragged.
- **MIDI tools:** `midi-tools slider { appearance: knob }` draws the quantise amount
  (Transform › Quantise) as a knob, dragged up for more; `#quantise-amount { height }` sizes it
  (16 to 96 points, 32 by default) and `value-position` places its value.
- **Piano roll tools:** artwork roles `piano.select`, `piano.draw`, `piano.paint`,
  `piano.slice`, `piano.erase`, `piano.mute`, `piano.cut`, `piano.copy`, `piano.paste`,
  `piano.duplicate`, `piano.quantise`, `piano.midi-tools`, `piano.audition` and `piano.hide`,
  each with `.hover` and `.on` (the chosen tool; `.checked` is not a state name). Their
  artwork keeps its aspect ratio.
- **Nine-slice:** the stretched edges and centre sample only their own pixels, so a hard edge
  stays hard.

### Round 6 additions

- **Formats:** `value-decimals: 0 | 1 | 2` and `value-unit: " dB"` on a
  value's control.
- **Device panels:** `device { height }` (60–1200), `param-label: left | top | hidden`,
  `device knob { width }`.
- **MIDI tools:** `midi-tools { gap: 2px }` spaces the part's rows (0 to 16 points);
  `midi-tools button { height }` sets its fields', buttons', switches' and Apply's height (10 to
  40 points). Its buttons and switches are `button` (`:hover`, `:active`, `[checked]` for a
  switch that is on), its lists `dropdown` and its number fields `value`, as in a clip's panel:
  a rule sets their background, border, radius and `color`. A transformation's check box
  (**Move ends too**) is a `button` too: the rule paints its box, not
  its name. A switch that is on keeps Suhina's
  lit look unless `button[checked]` has a background or colour of its own, and a button that
  cannot be pressed keeps Suhina's look.
- **Settings folder:** Settings › Projects and backups › Projects › Settings folder moves all of Suhina's
  data. Changing it or the themes folder asks first, backs the files up beside the old folder
  and moves them.
- **Choosing a theme** keeps the Themes page where it was.
- **Layers:** artwork rules work inside `@layer` blocks too; inside `@media` they do not (use
  plain CSS properties there).
- **Mixer volume readout:** `#mixer-volume-readout { background-color: #1E1E1E; border-radius: 6px;
  font-size: 11px; }`.
- **Empty slots:** `slot { background-color: #262626; }`.
- **Hovered clips:** `clip:hover { background-color: #3a3a3a; }` in place of the lighter mix.
- **Pan and send knobs of their own:** `pan.knob.body`, `pan.knob.pointer`, `send.knob.body` and
  `send.knob.pointer` artwork change only those knobs, in the Launcher and the Arrangement.
  `send { appearance: knob; }` makes the Arrangement's sends knobs too.
- **Devices with knobs of their own:** a device that draws its knobs with its own artwork (the
  Amp Modeler's) keeps them: no theme's `knob-body` or `knob-pointer` artwork replaces them,
  since the theme styles only what a device leaves unstyled. A line pointer takes
  `knob-pointer { color }`; the arc around the knob keeps the knob's `accent-color`.
- **Devices with buttons of their own:** a device panel's own sheet can say `image: none` on its
  buttons (the Amp Modeler's EQ, HQ and Mono do), and then no theme's `button` artwork replaces
  their look. `image` cascades: a panel's sheet beats the theme's, and `user.css` beats both.
- **The mixer's colour strip:** `mixer track-colour { display: none; }` hides it; `width`
  sizes it.

## What a theme cannot set

- **Choices on the Settings page that change how things look:** the header and clip colour
  strengths, a waveform or note colour other than **Theme**, and knobs or fields for the mixer
  (apart from `pan` and `send { appearance: knob | field; }`) are the user's. A
  shared theme can look different on another computer because of them. Only the track headers'
  side follows a theme.
- **Variants change variables only:** a colour written directly in a rule is the same in every
  variant. Write colours you want a variant to change as variables; artwork that writes
  `var(--name)` follows them too (below).

`track.name` and `track.body` artwork is drawn with `currentColor` as the header's fill: the
track's colour at the header colour strength.

## Signal-driven meter artwork

For a fully authored meter, set `meter { meter-style: frames; }` in `theme.css`
and supply contiguous `meter.frame-0`, `meter.frame-1`, … SVG/PNG roles, up to 32.
The louder channel's measured peak selects the frame along the native -70 to +6 dBFS
range. Frame zero represents silence; the last frame represents the top of the range.
Unlike decorative image sequences, this display remains signal-driven with animations
off and ignores artwork fps. Missing/invalid sequences use native continuous bars.
Colour/state-specific meter sequences follow the same precedence as other roles.
The native clip light, accessible level description and click-to-reset remain active.

## Backgrounds on the see-through window

Every background blends with what is under it, as in a browser. The window clears once
(to nothing when `--window-background` has alpha), `window` paints its background once,
and each view, panel, bar or control paints only its own background over that. A
translucent panel therefore composites over the window: a `window` at 50% under a panel
at 50% shows the desktop through 25%. To let a view show the desktop at exactly its own
alpha, make what is under it transparent:

```css
window { background-color: transparent; }
browser { background-color: rgb(20 30 40 / 70%); }
```

`background:` resets `background-color` to transparent when no colour is included, so
the element then shows only its gradient over what is under it. Text, artwork and
outlines blend the same way, and nothing an element paints punches a hole to the desktop.

## Closed-panel Grid tracks

CSS cannot open a closed browser: its element is absent. A fixed track still reserves space
for that absent element, so keep the `auto` tracks of Suhina's grid and put sizes on the panels:

```css
/* Wrong: reserves 248px even when the browser is closed. */
window { grid-template-columns: 248px minmax(0, 1fr) auto auto; }
/* Right: the absent browser leaves no fixed-size track. */
#panel-browser { width: 248px; }
```

Check browser and detail both open and closed, separately, at narrow and wide sizes.

## Value text on lit controls

`text.value` and `text.inverse` must contrast with `control.selectedBackground`,
not just the panel. Aim for at least **4:1** on the lit fill of volume/pan fields;
also inspect the unlit part, disabled states and partially filled controls.

```css
/* Wrong: mint text disappears on a mint selected fill. */
:root { --control-selectedBackground: #7CF0C4;
        --text-value: #7CF0C4; --text-inverse: #7CF0C4; }
/* Right: dark ink on that bright fill. */
:root { --control-selectedBackground: #7CF0C4;
        --text-value: #14212C; --text-inverse: #14212C; }
```

## Limits: what an author or AI must not assume

Follow the background, closed-panel Grid and lit-fill contrast rules above before
copying a CSS example. A parsed stylesheet is not evidence that those checks passed.


This is Suhina's own style engine, not a browser. A declaration it does not know is
skipped with a notice (line and column) and the rest of the sheet applies; a sheet that
does not parse at all keeps the last working one. Some raw widgets still need integration. Transforms,
filters, downloadable fonts, full typography, geometry animation and
the reduced-motion media feature are not connected. Floating windows stay host-managed; the
inside of each bar is laid out by Suhina, and its controls keep their native height. Do not claim universal browser CSS
or container-query support. Notices are useful but are not a complete CSS validator.
Check the current support specification for Flex and Grid.

Stylesheets are limited to 128 KiB, bounded nesting and native scope budgets.
SVGs are limited to 128 KiB each, the base roles and 64 additional variants per theme. External/network images,
scripts, HTML, shell commands and SVG external resources are not loaded. Resource
paths must stay relative inside the package. SVG controls and effect textures are
cached with bounded resolution/counts. Box-shadow blur is an approximation.
Text shadows affect shared native text and SDK vector labels; ordinary egui labels
that bypass that helper are not covered. Up to four text-shadow layers use offsets
within ±256 points and at most 32 points of blur. Blur uses a fixed nine-tap weighted
approximation on cached glyphs, not an image convolution.
Keep assets small, use readable contrast, and retain useful native labels.

Track/clip colours belong to the project: a theme draws them from the `color` attribute and
never chooses them, so the colour picker and new tracks and clips take Suhina's own colours.
Use `text.primary` contrast of at least 7:1 and hints of at least 3:1 where possible.

## Finding out why a rule does nothing

`suhina-cli theme render` (see the specification's *Seeing a theme without the app*) draws your
theme without opening Suhina, and answers the usual questions about a rule that seems to do
nothing:

```sh
suhina-cli theme render my-theme --scene launcher --probe 640,60   # what is at that point, and why
suhina-cli theme render my-theme --outline                         # every element's box and name
suhina-cli theme render my-theme --coverage                        # rules that matched nothing
```

- **Which element is this?** Draw with `--outline` and open `<scene>.outline.png`: each box is
  labelled with its element and id. Then write the selector it shows.
- **Why is it this colour?** `--probe x,y` (points from the window's top left corner, in a scene)
  lists every rule that matches the element at that point, strongest first, with each
  declaration's fate: *applies*, *overridden* by a stronger rule, or *dropped* with the reason. A
  dropped declaration (a misspelt variable, a value the property does not take) does not fall
  back to the next rule: the property keeps its inherited or initial value. The probe also shows
  the elements around the element, which is what a selector with a space tests.
- **Does my selector match anything?** `--coverage` lists your rules that matched no element in
  any scene. Check each one: a misspelt element, an id the element does not have, or a state no
  scene shows (use `--scene` with `--hover` or `--press` to show one).

A part of the window that is painted with an element's colour without being that element has no
box of its own; the probe then names the element around it.

## For AI-assisted edits

When a rule does nothing, use the probe and coverage above rather than guessing, and quote what
they printed in the report.

Before editing, read the three authoring rules above: check each background's intended
alpha (every background blends over what is under it, see "Backgrounds on the see-through
window"), collapse absent-panel Grid tracks, and contrast value/inverse text against
selected fills. Report those checks explicitly.


Describe the requested visual change and choose the smallest supported change.
Preserve unrelated palette, assets, layout, project data and native behavior.
Return a complete portable package and explain modified files, preview limitations
and verification. Never invent a successfully tested feature or silently discard
conflicting edits. Back up the user's theme before replacing it.

Current specifications: https://github.com/suhina-daw/suhina-daw/blob/main/docs/spec/theme-skins.md
and https://github.com/suhina-daw/suhina-daw/blob/main/docs/decisions/0030-native-theme-css.md
(on a development branch, use that branch's corresponding documents).

### Full-height meters and symbol proportions

A part stretches with `align-self: stretch`; its margins keep it off the edges. For example, a
header's meter as tall as the track, four points from its top and bottom:

```css
track-header > meter { align-self: stretch; margin: 4px 6px 4px 0; }
```

Arrangement meter geometry follows the whole track, so scrolling does not rescale its level. SVG
symbol roles preserve their aspect ratio and are centred; surface artwork such as button and
track backgrounds stretches.

## Colour token reference

Each token's colour in Suhina Studio; a test keeps this table in step with Suhina's list of
tokens.

| Token | Suhina Studio | Used for |
|---|---|---|
| `grid.major` | `#2C2B29` | Arrangement bar lines and lane separators |
| `grid.minor` | `#252422` | Arrangement subdivisions between bar lines |
| `track.headerBackground` | `#252421` | The neutral surface behind Arrangement track controls |
| `track.headerSelectedBackground` | `#3E3E4C` | The surface behind a selected Arrangement track's controls |
| `window.background` | `#1D1C1A` | The window behind everything: grid, empty slots, lanes, Settings |
| `panel.background` | `#252421` | Panels and headers: menu, control and status bars, track headers, beat lines |
| `panel.hoverBackground` | `#31302C` | A panel or list row under the pointer |
| `control.background` | `#343330` | Fields, buttons and stopped clips |
| `control.hoverBackground` | `#3F3D3A` | A field, button or clip under the pointer |
| `control.activeBackground` | `#4B4A46` | Selected or pressed: a selected clip or track, a dragged field |
| `control.playingBackground` | `#545472` | A playing clip or scene |
| `control.selectedBackground` | `#D0CCC6` | The chosen option of a switch, a lit toggle, the level of a volume or pan field |
| `border` | `#343330` | Hairlines between regions and rows |
| `border.strong` | `#484642` | Launcher columns and arrangement track lines |
| `text.primary` | `#ECE8E1` | Names and values |
| `text.secondary` | `#B5B2AC` | Labels and notes |
| `text.hint` | `#85837E` | Inactive text, hints and section titles |
| `text.value` | `#DAD6D0` | Clip names and values on fields |
| `text.inverse` | `#1D1C1A` | Text on lit fills: a switch's chosen option, a lit button |
| `text.playing` | `#FFFFFF` | The name of a playing clip |
| `text.onColorLight` | `#ECE8E1` | Light text on dark coloured tracks, returns and clips; paired with text.onColorDark for contrast |
| `text.onColorDark` | `#1D1C1A` | Dark text on bright coloured tracks, returns and clips; paired with text.onColorLight for contrast |
| `browser.text.primary` | `#ECE8E1` | Browser selection, search input and highlighted names |
| `browser.text.secondary` | `#B5B2AC` | Browser item names, categories and preview labels |
| `browser.text.hint` | `#85837E` | Browser section headings, search placeholder and hints |
| `browser.text.value` | `#DAD6D0` | Browser values and active preview controls |
| `accent` | `#A6A3FF` | The playhead, queued clips, a key being set |
| `automation.pin` | `#A6A3FF` | The pin of an automation lane pinned below its track's top lane (older theme files: their accent) |
| `selection` | `#E3C86A` | The selected device's outline |
| `selection.background` | `#DBDAFF` | Selected track headers, clips and, faintly, their lanes and columns |
| `recording` | `#E5484D` | Recording and errors |
| `recording.background` | `#4D2F2B` | Behind an armed track's arm button and a recording clip |
| `meter.normal` | `#8BC48A` | A level meter below the top |
| `meter.hot` | `#E3C86A` | A level meter near the top |
| `slot.empty` | `#484642` | The mark in an empty clip slot |
| `track.mutedBackground` | `#21201D` | The fills of a muted track |
| `pan.background` | `#1F1E1C` | Behind a track header's pan field |
| `pan.center` | `#817F7A` | The center mark of a pan field |
