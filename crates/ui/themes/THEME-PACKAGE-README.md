<!-- Copyright (C) 2026 The Suhina contributors; SPDX-License-Identifier: GPL-3.0-or-later -->
# Suhina theme package: author and AI guide

## What this package is

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
  theme.css           # the theme's style sheet: its look and the values of its settings
  variants/*.json     # optional variants: other values for the same settings
  layout.css          # written by Suhina: placements, toolbar, artwork and name set in the app
  assets/*.svg        # vector control artwork
  assets/*.png        # optional raster artwork and sequence frames
  background.png      # optional background image
  README.md           # this guide
  examples/dark-gloss.css  # optional paint-only example; not enabled automatically
  examples/compact-session.css  # optional native Launcher density; not enabled automatically
```

`theme.css` is yours: Suhina reads it and never writes it. What a user changes in Suhina's
theme editor (colours, meter, liquid, gradients and any setting your theme offers) is saved in a
variant file, and their toolbar order and artwork choices in `layout.css`. So you edit CSS, users
edit variants, and no file is written by both.

Installed themes are the same folders, named by their id. Relative image paths in the sheets are
authoritative; keep the whole folder together. A theme saved by an earlier Suhina version (JSON,
or a `theme.css` that imports `advanced.css`) is converted once; its original files are kept in
`legacy/` inside the theme's folder.
Drop the ZIP on Suhina to import it, then select it in Settings → Appearance → Themes.
Open the themes folder from that page to edit the installed files.

A folder in the themes folder is a theme when it holds a file named exactly `theme.css` (on
Windows, check that it is not `theme.css.txt` with the extension hidden). Suhina finds a new
folder within about a second, or at once with **More › Reload themes**; a folder that is not a
theme is named in the status bar with what it is missing. The folder's name is the theme's id:
it stays when the theme is renamed. Two themes with the same name show their folder names on
their cards. **Duplicate theme** copies the whole theme, its `layout.css` placements included;
delete the copy's `layout.css` to start from Suhina's own placements.
The template ZIP also has the built-in themes as reference folders in `examples/`;
the importable example is `my-theme/`, with an example variant.

### Settings and variants

A theme's settings are custom properties. Every theme has the core set: each palette token
(`--accent`, `--window-background`, `--track-headerBackground`: the palette keys with hyphens),
`--meter-style` (`continuous`, `segmented`, `needle`, `frames`), `--meter-segments` (4–64),
`--meter-intensity` (0–100), `--artwork-fps` (1–30), `--liquid` (`auto` or three strengths
0–100 for panels, tracks and clips), `--gradient-panels`, `--gradient-tracks`,
`--gradient-clips` (`none` or `90deg, #RRGGBBAA 0%, #RRGGBBAA 100%`, 2–8 stops),
`--background-image` (`none` or `url("background.png")`), `--background-opacity` and
`--background-blur` (0–100), and `--session-track-width`, `--session-return-width`,
`--session-scene-width`, `--session-slot-height`, `--session-header-height` (`auto` or pixels).
Set them in `:root`; what you leave out keeps Suhina Dark's value.

Offer your own settings with `@property`. A property with a `label` appears under **Theme
settings** in the editor, with a control for its type:

```css
@property --glow { syntax: "<number>"; initial-value: 0.6; label: "Glow"; group: "Effects"; range: 0 1; step: 0.05; }
@property --edge { syntax: "solid | dashed"; initial-value: solid; label: "Edges"; }
:root { --accent: #7FD1B9; --accent-hover: color-mix(in oklch, var(--accent), white 20%); }
.button:hover { background-color: var(--accent-hover); opacity: var(--glow); }
```

Supported syntaxes: `<color>`, `<number>`, `<integer>`, `<length>`, `<percentage>`,
`<length-percentage>`, `<angle>`, `<string>`, `<image>`, keywords joined by `|`, `+` for a
space-separated list, and `*`. A value that does not match its syntax uses the initial value.

A variant is a JSON file in `variants/`; its file name is its id and it lists only the values
that differ from the theme's:

```json
{ "name": "Frost", "appearance": "light", "values": { "--accent": "#88C0D0", "--window-background": "var(--nord-snow)" } }
```

Values are CSS text and may refer to the theme's own variables. A variant may set a core
property, one of your `@property` settings, or any other variable your `:root` sets (a helper
such as `--my-well`, without `@property`); variables of yours that refer to it follow, in the
painted sheet and in the colours the editor shows. A name your sheet never sets is refused with
a warning. A variant's values win over the
theme's rules for the same property, including ones inside `@media`. The editor shows a theme's
variants as swatches under its card. A built-in theme's variants are kept in a folder of its id
holding only `variants/`, without a copy of its CSS.

### Your own overrides and device panels

`user.css` in the themes folder (beside the theme folders, not inside one) is yours: it
applies over every theme and device panel and stays when you switch themes. A device's own
panel style (in its `.sd` file) applies only inside that panel, over the theme. Each control
of a device panel is `#its-id`, for example:

```css
/* user.css */
#cutoff { accent-color: #ff8a00; }
.device-panel .knob:hover { color: #ffffff; }
```

## How and when to edit each file

1. Duplicate a bundled theme in the editor or start with the supplied example.
2. Set `theme { name: "My theme"; }` in `theme.css`. Theme folder ids use letters, digits and `-`.
   The folder's name stays its id when the name changes. A name given in Suhina (the editor's
   name field) is kept in `layout.css` and wins over the one in `theme.css`: to name the theme
   in `theme.css` again, remove the `name` line from `layout.css`.
3. For palette and other settings, set the core properties in `theme.css`'s `:root`. For example:

   ```css
   :root { --accent: #7FD1B9; --window-background: #14161A; --meter-style: segmented; --meter-segments: 24; }
   ```

   Colours may be any CSS colour the engine reads (`#RRGGBB`, `#RRGGBBAA`, `rgb()`, `hsl()`,
   `color-mix()`, names), and `var()` of your own variables.
4. For conditional styles, selectors, variables, effects, motion or connected Flex/Grid
   layout, write them in `theme.css` too. It is kept exactly as you write it.
5. For a new control silhouette, edit an SVG or PNG and name it in `theme.css`, e.g.
   `play { image: url("assets/play.svg"); }`.
   For layout positions, the toolbar's right-click → Edit toolbar mode is usually
   easier: drag the actual controls on the 8-point grid. Done saves to `layout.css`; Cancel or
   Escape discards. Entering/leaving edit mode alone preserves the layout.
6. Save one file at a time and inspect Suhina's warnings and the actual interface.
   Files are watched approximately once a second. Suhina never writes `theme.css`; a save of
   a variant over an edit you made to that variant's file outside Suhina is refused and your
   file is kept.
7. Test normal, hover, active and disabled states, narrow/wide windows, interaction,
   clipping and contrast, with each variant. Export and reimport the ZIP to check portability.

To try the supplied dark metallic look, copy `examples/dark-gloss.css` to the end of your
`theme.css`.
It demonstrates original gradients, rounded SVG-backed buttons, box/text shadows
and coloured transport controls while preserving native geometry. Back up your
current CSS before replacing it with an example.

Use **Load compact Session geometry** under Native CSS, then **Apply CSS**, which saves it in
the variant in use. It sets track/return/scene widths and header/slot heights (in `theme.css`,
the `--session-*` settings): widths are 64–512px, slot height 16–64px and header height
20–64px. Use `auto` to remove
an override. The same geometry drives drawing, scrolling and drop/input targets.
Mixer height keeps its existing native divider/preference. Launcher and Arrangement
remain separate; this does not introduce the separately planned mixer view.
The example also includes `.pan { --suhina-appearance: knob; }`, a portable native
pan-face override for Launcher and Arrangement. `field` forces a number field, while
`native` retains the global preference. Native parameter gestures remain unchanged.

The gloss example gives `.mixer` and `.track-name` their own materials. Meter gradients
under `.meter-vertical .meter-level` and `.meter-horizontal .meter-level` keep their
colour stops anchored to the full dB range; peaks/RMS reveal the appropriate portion.
Outset shadows form the glow around the lit region, without filling empty rails or
segment gaps. This differs from a decorative gradient that stretches with the level.

**Meter intensity** in the theme editor dims measured ink and glow from 0–100%.
In `theme.css` it is `--meter-intensity: 65;`.
It also affects needles, hold marks and signal-driven artwork frames, while keeping
the native clip warning visible and native bar rails unchanged. Frame artwork dims
as a whole, including any background baked into its image. This changes visual opacity, not audio
gain or saturation. Change palette/gradient colours for different hues or saturation.
In `theme.css`, `.meter-level { opacity: .5; }` additionally dims native bars, hold marks,
needles and CSS fills/glow. Transparent CSS fills suppress native fill fallback.
Generic CSS `filter: brightness()` / `saturate()` is not connected to native painting.

## Native element placement and artwork variants

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
or background PNG. Eight raw track colours choose distinct `track.body.color-RRGGBB`
textures. Canonical `.track-column { --suhina-track-artwork: on; }` extends body
artwork behind Launcher slots; empty cells preserve it and native clip launches remain
unchanged. `off`/omission preserves native fills. Native custom properties inherit
from the nearest authored ancestor, and a closer scope can override them.

Controls are placed with `.control-NAME` rules; matching instances share the rule and the
containing panel bounds each element. The visual placement editor (**Edit interface**) has been
removed for now, while layout moves to plain CSS
([issue #995](https://github.com/suhina-daw/suhina-daw/issues/995)), so write these rules by
hand in `theme.css` (a `layout.css` from an earlier build is still read):

```css
.control-mixer-name { bottom: 0px; }
.control-mixer-color { display: none; }
.control-mixer-volume { width: 100px; height: 20px; --suhina-direction: horizontal; }
.control-mixer-meter { width: 80px; height: 10px; left: 0px; top: 0px; }
```

Offsets are relative to the native anchor, in logical points, bounded to ±16384;
sizes are `auto` or 1–4096px. `bottom` anchors to the containing panel. This explicit
native positioning contract is separate from the Flex/Grid layout in `theme.css`.
Placement overrides do not automatically resolve overlaps with neighbouring elements;
when moving a name into the footer, arrange the toggles and sends to leave it room.
Changing direction changes the native drag axis, rail, cap and scale together.
The dB labels/ticks can also be moved, resized or hidden independently with
`.control-mixer-volume-scale`; `.fader-scale` styles their ink. This keeps a swapped
meter/fader layout from forcing the scale to remain on the fader's original side.
Native compact-height thresholds remain in force. Placement coverage is still growing:
toolbar modules/transport, the Launcher mixer sections, solo/cue/arm/type glyphs and
Arrangement name/colour/volume/pan/meter are connected. Device SDK controls, captions,
panels and other typed elements use `device-<control-id>` names; moving/resizing them
keeps their existing parameter commands and hit targets. Long/non-ASCII ids get a stable
bounded slug/hash name, visible in the editor. Knobs preserve their circular aspect;
other drawing coordinates scale with the control. Raw widgets, clip/scene cells,
browser elements and floating-window geometry still need adapters.
Arrangement fold, enable, automation, output/input routing and send navigation are
also connected (`arrangement-fold`, `arrangement-enable`, `arrangement-automation`,
`arrangement-output`, `arrangement-input`, `arrangement-sends-previous/next`).
Individual send slots use `mixer-send-1` and `arrangement-send-1`, then `-2`, etc.,
following native return order. Launcher sends stay inside their movable sends region.

Supply `track.name` and `track.body` SVGs or PNGs for name and strip backgrounds. Exact raw
track colours choose variants independently of selection/mute tint:

```css
track.name { image: url("assets/name.svg"); }
track.name.color-00ff00 { image: url("assets/green-name.svg"); }
track.name.color-00ff00.hover { image: url("assets/green-name-hover.svg"); }
:root { --artwork-fps: 12; }
```

For an image sequence, supply contiguous `.frame-0`, `.frame-1`, … keys for that
variant (at most 32 frames), e.g. `track.name.color-00ff00.hover.frame-0`.
Frame rate is 1–30 fps (`--artwork-fps: 12;` in `theme.css`). With native animations off,
the first frame stays visible without continuous repainting. Colour/state defaults
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
`--background-opacity`, is how much it darkens the picture; `--background-opacity: 100` with
an opaque colour hides it.

A theme whose surfaces show the desktop through them (a window, panel or header colour with
alpha) has the area under button, fader and track artwork cleared first; on an opaque theme
the artwork is drawn over the surface, so its transparent parts show the strip under them.

## Theme packages

**Settings › Appearance › Themes › More › Export theme…** writes the theme in use as one file,
`<theme>.theme.suhina`, into the `exports` folder beside the themes folder. It holds the
theme's folder and a `manifest.json` with the theme's id, its name and the SHA-256 sum of every
file. Drop a package on Suhina's window (Settings open or not) or open it with Suhina (a
double-click, once the system knows the ending) to bring the theme in. Before anything is
written Suhina checks the manifest, every file's sum, the paths, the sizes and what each file
holds (a `theme.css` that reads, SVGs that are SVG, PNGs that are PNG, variants that are JSON);
a damaged or changed package is refused and says why. The same checks run on every import.

From a terminal:

```sh
suhina-cli theme pack my-theme            # writes my-theme.theme.suhina beside the folder
suhina-cli theme pack my-theme -o out.theme.suhina
suhina-cli theme validate out.theme.suhina   # or a folder; lists the notices
```

A package is not signed. Sign the file with a key of your own, outside Suhina, when the
receiver needs to know it came from you.

## SVG inheritance and safe defaults

Supported roles are `fader.cap`, `fader.rail`, `knob.body`, `knob.pointer`, `button`,
`play`, `stop`, `record`, `solo`, `arm`, `cue`, `track.midi`, `track.audio`,
`track.name`, `track.body` and `meter`. State variants append `.hover`,
`.active` or `.disabled`. Example overrides:

```css
play { image: url("assets/play.svg"); }
button:hover { image: url("assets/button.hover.svg"); }
```

Lookup uses the theme's state SVG, then its base SVG, then Suhina's default SVG.
Themes only need to define the images they replace. Removing an override restores
inheritance. Knob pointers rotate with the parameter and fader caps follow its value.
Use self-contained SVG paths and shapes; native labels stay native. `currentColor`
uses the current theme/control colour. Default body/rail/cap SVGs additionally use
`data-suhina-ink` to retain their semantic palette colour when copied or exported.

`themes/_defaults/assets/` is a protected reference library. Editing its valid artwork
creates a numbered `Suhina Default (modified N)` user theme before restoring the
original. Select that new theme in the gallery to use the edit. Missing default
files and `_defaults/suhina-default.zip` are recreated from SVGs embedded in the app.
Malformed edits are preserved in a recovery ZIP before restoration when within
the resource limit; oversized or inaccessible files remain untouched with a warning.
Edit an ordinary user theme's own `assets/` for direct live changes.
Portable exports include inherited artwork so another machine receives the same images.

## What's possible now

| Area | Supported customization |
|---|---|
| Palette | Core colour properties, transparency, project-safe creation defaults |
| Settings | The core settings, your own `@property` settings in the editor, variants (light, dark and others) |
| Artwork | SVG/PNG roles, colour/state variants and frame sequences, moving faders and rotating knob pointers |
| Meters | `continuous`, `segmented` (4–64 segments), `needle`, signal-driven `frames`; signal remains dBFS |
| Materials | Optional bounded PNG background, opacity/blur; panel/track/clip gradient and liquid settings |
| Toolbar | Native layout plus anchored offsets, explicit order/visibility, connected Flex/Grid |
| Workspace | CSS layout of `#panel-browser`, `#panel-main`, `#panel-detail` under `#workspace` |
| Styles | Connected foreground/font size/radius, linear/radial/conic and repeating gradients, bounded inset/outset box shadows and shared native text shadows |
| Motion | Connected paint transitions/keyframes; native animation-off/high-contrast settings suppress motion |
| Selectors | Retained native class tree, child/sibling matching, connected hover/active/disabled/focus states, named object identities |

`theme.css` runs on Suhina's own style engine, with Taffy for connected layout. For example:

```css
.browser { color: #D8E8DF; background-color: #101C17;
           background-image: linear-gradient(135deg, #243A30, #101C17); }
#workspace { display: grid; grid-template-columns: auto minmax(0, 1fr);
             grid-template-rows: minmax(0, 1fr) auto; gap: 8px; }
#panel-browser { grid-column: 1; grid-row: 1 / -1; width: 240px; }
#panel-main { grid-column: 2; grid-row: 1; }
#panel-detail { grid-column: 2; grid-row: 2; height: 180px; }
```

Workspace layout respects native panel visibility. CSS-owned sizes replace the
browser/detail splitters; removing the layout rules restores saved native sizes. A panel the grid
places in the same cells as `#panel-main` is drawn over the view and takes the pointer there,
for example a browser floating over the left of the view:

```css
#workspace { display: grid; grid-template-columns: minmax(0, 1fr); grid-template-rows: minmax(0, 1fr); }
#panel-main { grid-column: 1; grid-row: 1; }
#panel-browser { grid-column: 1; grid-row: 1; width: 260px; justify-self: start; }
```

`.arrangement { --suhina-track-headers: left; }` (or `right`) puts the Arrangement view's track
headers on that side while the user's **Track headers** setting is *Theme*, its default. A user
who chose Left or Right keeps their side.
The toolbar exposes `browser`, `tempo`, `signature`, `grid`, `count-in`, `transport`,
`position`, `metronome`, `mapping`, `performance`, `views`, `agents` module ids.
Device SDK controls expose semantic classes such as `.device-panel`, `.knob`,
`.button` and `.device-control`. Stable named track/clip/slot/scene scopes also exist.
The native centred toolbar exposes the same module ids as CSS layouts. Its main
buttons additionally expose `#control-play`, `#control-stop`, `#control-record`,
`#control-browser-toggle` and `#control-metronome-toggle`. Paint-only rules preserve
the existing geometry. The selector reference below lists every class and id.

## Selectors by area

An element has its own class (and often an id), and belongs to a *family* of parts of its kind.
The area it is in is one of its ancestors. Combine them as CSS does:

```css
.value-control { background-color: #203040; }          /* every value a user drags, everywhere */
.arrangement .value-control { border-radius: 2px; }     /* only in the Arrangement */
#arrangement-pan { --suhina-value-position: top; }      /* one control */
```

**Families:** `.value-control` (levels, pans, faders, sends, knobs, sliders, device parameters,
clip gain), `.track-button` (solo, cue, arm, track-on, monitor, automation), `.transport-button`
(play, stop, record, metronome), `.list-row` (the browser's rows and places), `.timeline-part`
(ruler, loop region, loop lines, play position). A family works in host rules too
(`.value-control { --suhina-appearance: knob; }`).

| Area (ancestor) | Parts (class or id) |
|---|---|
| Toolbar `.toolbar` | `.toolbar-module`, modules `#browser`, `#tempo`, `#position`, `#signature`, `#grid`, `#count-in`, `#metronome`, `#views`, `#performance`, `#mapping`, `#agents`; `#control-play`, `#control-stop`, `#control-record`, `#control-metronome-toggle`, `#control-browser-toggle` |
| Arrangement header `.arrangement .track-header` | `.track-name`, `.track-number`, `#control-track-on`, `#control-solo`, `#control-cue`, `#control-arm`, `#control-automation`, `#control-monitor`, `.fold`, `#arrangement-volume`, `#arrangement-pan`, `.send`, `.value-fill`, `.value-handle` |
| Timeline `.arrangement` | `.ruler`, `.ruler-number`, `.ruler-tick`, `.loop-region`, `.loop-line`, `.playhead`, `.clip`, `.clip-name`, `.clip-waveform`, `.clip-notes`, `.clip-gain`, `.clip-fade-handle`, `.add-track`, `#add-midi-track`, `#add-audio-track`, `#add-return-track` |
| Launcher and mixer `.session`, `.mixer` | `.track-column`, `.slot`, `.scene`, `.scene-column`, `#mixer-volume`, `#mixer-pan`, `#mixer-send-N`, `.fader`, `.fader-cap`, `.fader-rail`, `.fader-scale`, `.meter`, `#mixer-divider` |
| Piano roll `.piano-roll` | `.piano-grid`, `.piano-row`, `.piano-row-black`, `.piano-key`, `.piano-key-black`, `.piano-key-c`, `.piano-key-hover`, `.piano-key-black-hover`, `.piano-key-line`, `.piano-scale-mark`, `.piano-scale-strip`, `.piano-range`, `.piano-note`, `.velocity`, `.piano-tool` with `#piano-<tool>`, `.midi-tools` (`#quantise-amount`), `.clip-info`, `.editor-tab` |
| Browser `.browser` | `.browser-place`, `.browser-row`, `.browser-play`, `.browser-tag-dot`, `.browser-preview-wave`, `#browser-search` (`.text-input`) |
| Devices `.device`, `#device-<id>` | `.device-header`, `.device-control`, `.slider`, `.slider-rail`, `.slider-fill`, `.slider-handle`, `.knob`, `.value` |
| Dialogs `.settings`, `.dialog`, `.start-page`, `.plugin-manager` | `.button` (also egui's check boxes and drop-downs), `.button:checked` (a chosen item), `.text-input`, `.settings-nav`, `.progress`, `.progress-fill`, `.menu`, `.popup` |

States: `:hover`, `:active`, `:disabled`, `:focus`, `:checked`, `:selected`, `:muted`,
`:playing`, `:recording`, `:armed`. Artwork names its state `.hover`, `.active`, `.on`.

**Glow:** a shadow without an offset, `box-shadow: 0 0 12px #00c8ff`. A button drawn with artwork
casts it in the artwork's shape, under it; a button without a background of its own glows round
about its glyph unless the sheet gives it a `border-radius`.

**Text in SVG** is sized in the SVG's own units, so it shrinks with the artwork: in a 24 by 16
button graphic, `font-size="9"` is 9/16 of the button's height. Give text a size for the
artwork's own `viewBox`, or draw the glyph as a path.

**Not yet:** the toolbar's modules as one block (position, tempo and signature as one LCD); the
MIDI tools in a compact one-line layout.

## Selector reference

Classes, each the native element it styles:

| Class | What it is |
|---|---|
| `.window`, `.menu-bar`, `.application-bar`, `.project-tabs`, `.toolbar`, `.status-bar` | the window and its bars |
| `.workspace`, `.workspace-panel` | the layout under the bars (`#workspace`, `#panel-browser`, `#panel-main`, `#panel-detail`) |
| `.toolbar-module` | one area of the toolbar, by its id (below) |
| `.browser`, `.session`, `.arrangement`, `.mixer`, `.detail`, `.clip-editor`, `.piano-roll`, `.sample-map` | the views and panels |
| `.track-header`, `.track-name`, `.track-column`, `.scene-column`, `.scene`, `.slot`, `.clip` | tracks, scenes, slots and clips |
| `.waveform`, `.envelope` | what a clip shows |
| `.device-panel`, `.device-section`, `.device-control`, `.xy-control` | device panels; `#id` selects a device control |
| `.button`, `.value`, `.value-fill`, `.dropdown`, `.text-input` | buttons, value fields, the lit part of a level field, menus and text fields |
| `.menu`, `.popup` | an open menu of the menu bar; a control's popup (`#metronome-level`, the metronome's level) |
| `.knob`, `.fader`, `.fader-cap`, `.fader-scale`, `.pan` | mixer and device controls |
| `.meter`, `.meter-level`, `.meter-vertical`, `.meter-horizontal` | level meters and their lit part |
| `.label`, `.separator`, `.settings`, `.dialog` | labels, rules, the Settings page and dialogs |
| `.start-page`, `.plugin-manager` | the page with no project open, the Plug-in manager |
| `.send` | a send, in a mixer strip and in a track header |
| `.add-track` | the **+ MIDI track**, **+ Audio track** and **+ Return track** buttons after the last track |
| `.ruler`, `.loop-region`, `.playhead` | the Arrangement view's ruler, its loop bar and the play position line |
| `.divider` | the line between a strip's device list and its controls (`#mixer-divider`) |
| `.clip-name`, `.clip-waveform`, `.clip-notes`, `.clip-gain`, `.clip-fade-handle` | an arrangement clip's name bar, waveform, notes, gain field and fade handles |
| `.value-handle`, `.fader-rail` | the handle where a level bar's fill ends; a fader's rail |
| `.piano-grid`, `.piano-row`, `.piano-row-black`, `.piano-key`, `.piano-key-black`, `.piano-note`, `.velocity` | the piano roll's grid, its rows, its keys (with `:selected` and `:playing`), the notes and their velocity stems |
| `.piano-key-c`, `.piano-key-hover`, `.piano-key-black-hover`, `.piano-key-line`, `.piano-scale-mark` | a C key, a key under the pointer, the line between white keys, the mark on a key in the clip's scale |
| `.piano-scale-strip`, `.piano-range` | the strip left of the keys that names the octaves and the row under the pointer, and a range selected from the keys |
| `.piano-tool`, `.midi-tools`, `.clip-info`, `.editor-tab` | the piano roll's tool buttons, the MIDI tools column, the clip's column and the **Clip** / **Devices** tabs |
| `.device`, `.device-header` | a device's panel in the device chain, and its header row |

Ids:

| Id | What it is |
|---|---|
| `#browser`, `#tempo`, `#signature`, `#grid`, `#count-in`, `#metronome`, `#transport`, `#position`, `#views`, `#performance` (RAM and CPU), `#mapping`, `#agents` | the toolbar's areas (`.toolbar-module`) |
| `#control-play`, `#control-stop`, `#control-record`, `#control-browser-toggle`, `#control-metronome-toggle` | the toolbar's main buttons |
| `#control-solo`, `#control-cue`, `#control-arm`, `#control-track-on` | a track's solo, cue, arm and on (the track number in the mixer) buttons |
| `#arrangement-volume`, `#arrangement-pan` | a track header's volume and pan fields |
| `#mixer-volume`, `#mixer-pan` | a mixer strip's fader and pan |
| `#workspace`, `#panel-browser`, `#panel-main`, `#panel-detail` | the workspace and its panels |
| `#mixer-send-1`, `#mixer-send-2`, … | a mixer strip's sends, from the left |
| `#add-midi-track`, `#add-audio-track`, `#add-return-track` | the three add-track buttons |
| `#mixer-divider` | a strip's divider |
| `#piano-select`, `#piano-draw`, `#piano-paint`, `#piano-erase`, `#piano-slice`, `#piano-mute`, `#piano-cut`, `#piano-copy`, `#piano-paste`, `#piano-duplicate`, `#piano-quantise`, `#piano-midi-tools`, `#piano-audition`, `#piano-hide` | the piano roll's tool buttons |
| `#tab-clip`, `#tab-devices` | the clip editor's two tabs |
| `#device-sampler`, `#device-fm-synth`, `#device-utility`, `#device-auto-pan`, `#device-dj-eq`, `#device-vinyl-distortion`, `#device-overdrive`, `#device-compressor`, `#device-limiter`, `#device-nam-amp-modeler`, `#device-nam-cab-loader`, `#device-delay`, `#device-reverb`, `#device-pitch` | one built-in device's panel; `#device-plugin` a plug-in's, `#device-sd` a device file's |

A view's or bar's background is the surface behind its controls: `.toolbar` or `.arrangement`
with a `background-color` does not repaint the buttons on it. A control's own surface takes
`background-color`, `background-image`, `border-radius`, `border-width` with `border-color`
(no border is drawn without a width), `box-shadow` and `opacity`; `transparent` paints nothing.
A button shaped this way is not covered by the artwork Suhina ships, only by your own
`button` artwork. For example, a toolbar as one surface with recessed displays:

```css
#browser .button, #metronome .button { background-color: transparent; box-shadow: none; }
#browser .button:hover, #metronome .button:hover { border-width: 1px; border-color: var(--accent); }
#performance { background-color: transparent; }
#tempo, #position { background-color: var(--my-well); border-radius: 4px; }
```

Volume and pan fields are level bars. The field (`#arrangement-volume`) is the track and its
`.value-fill` the lit part; `height` sets the thickness of either, so a thin line with a lit
segment is:

```css
#arrangement-volume { height: 4px; border-radius: 0; background-color: #2A3238; }
#arrangement-volume .value-fill { background-color: var(--accent); }
```

Artwork of your own replaces either: `volume.track`, `volume.fill`, `pan.track` and `pan.fill`.
`color: transparent` on the field hides its value text.

### States

Besides `:hover`, `:active` (pressed), `:focus` and `:disabled`, Suhina tells the sheet:

| State | On |
|---|---|
| `:checked` | a toggle that is on: solo, cue, arm, the metronome, the browser toggle, play while playing |
| `:playing`, `:recording` | `#control-play` while the transport runs, `#control-record` while it records |
| `:selected` | the selected track's `.track-header`, `.track-column`, `.mixer` strip and `.track-name` |
| `:armed`, `:muted` | an armed track's arm button and header; a muted track's header and strip |
| `:selected`, `:muted`, `:playing`, `:recording` | an arrangement clip that is selected, off (or on a muted track), under the play position while it plays, or under a take being recorded |
| `:selected`, `:muted` | a piano roll note that is selected or muted; a device that is selected or off |
| `:checked` | the loop bar while looping is on, the chosen piano roll tool or open column, the tab shown |

```css
#control-metronome-toggle:checked { background-color: var(--accent); color: #000; box-shadow: 0 0 6px var(--accent); }
.track-header:selected .track-name { color: #FFFFFF; }
```

A button's glyph takes the button's `color`, in every state. Artwork has an on state of its own,
`solo:on { image: url("assets/solo.on.svg"); }`, which falls back to `:active` artwork.

### Tracks

A track's header, Launcher column and mixer strip give the sheet `--track-colour`, the track's
colour, and its name `--track-fill`, the fill Suhina draws there (the colour at the user's header
colour strength). Use them to style each view on its own, whatever that setting is:

```css
.session .track-header .track-name { background-color: var(--track-colour); color: #101214; }
.arrangement .track-name { background-color: #2A2C2E; border-width: 1px; border-color: var(--track-colour); color: #E6E6E6; }
.mixer .track-name { background-color: color-mix(in srgb, var(--track-colour) 40%, #1E2023); }
```

A name's `color` sets its text; without one it is worked out from the fill. `.mixer` is a strip,
also in the Launcher view, so `.mixer .track-name` is the strip's name and
`.track-header .track-name` the column's. Artwork for one view only: `track.name.session`,
`track.name.mixer`, `track.name.arrangement`, `track.body.session` and
`track.body.arrangement`, over `track.name` and `track.body`.

### Clips

An arrangement clip gives the sheet `--clip-colour`, its own colour, and takes `opacity` (the
whole clip fades, its waveform, notes and name with it), `border-radius` and a background:

```css
.clip { border-radius: 4px; background-color: color-mix(in srgb, var(--clip-colour) 70%, #000); }
.clip:selected { border-width: 1px; border-color: #FFFFFF; }
.clip:muted { opacity: 0.5; }
.clip:playing .clip-name { background-color: var(--accent); color: #000; }
.clip-name { font-size: 10px; color: #F0F0F0; }
.clip-waveform, .clip-notes { color: #101214; }
.clip-gain { background-color: #00000080; color: var(--accent); }
.clip-fade-handle { background-color: #FFFFFF; border-radius: 2px; }
```

The waveform's and notes' `color` applies while **Settings › Arrangement › Waveform colour** is
**Theme** (its default); the user's other choices there win. A setting saved before Theme
existed reads as the choice it was (Dark, for most): pick Theme there to see the theme's
colours. `clip.fade-handle` is artwork for
the fade handles. A new clip takes its track's colour while **New clip colour** is **Track
colour** (the default); with **Theme default** it takes `clip.midiDefault` or
`clip.audioDefault`.

The piano roll's notes have `--clip-colour` too, and their own rules:

```css
.piano-note { background-color: var(--clip-colour); border-radius: 2px; }
.piano-note:selected { border-color: #FFFFFF; }
.piano-row-black { background-color: #15171A; }
.piano-key { background-color: #E8E8E8; color: #202020; }
.piano-key-black { background-color: #202020; }
.piano-grid { border-color: #3A3D42; }
.velocity { color: var(--accent); }
.piano-tool:checked { background-color: var(--accent); color: #000; }
```

Each tool button takes artwork of its own: `piano.draw`, `piano.draw.on`, `piano.select.hover`,
and so on for every id above, after `piano.`.

### The ruler, the loop and the play position

`.ruler` gives the ruler its background, its bar numbers' `color` and their lines'
`border-color`; `ruler` is its artwork. The loop bar is `.loop-region` (`:checked` while
looping is on): `height` sets its thickness, `background-color` and `border-color` its look,
and `loop-region` (`.on` while on) is its artwork. `.playhead { color; width }` sets the play
position line's colour and thickness; `width: 0` hides it.

### Sends and the add-track buttons

`.send` styles every send, in the mixer and in the Arrangement header, and `#mixer-send-1`
one of them. `.send { --suhina-appearance: knob; }` draws the mixer's sends as knobs. `send` is
a send's artwork and `send.fill` its level. `.add-track` and `#add-midi-track` (and the two
others) style the add-track buttons, `add-track` is their artwork.

### Level bars: the value and a handle

On a level bar (`#arrangement-volume`, `#mixer-pan`) `text-align` moves the value,
`font-size` sizes it, `.value-fill { color }` is the value's colour where it stands on the
lit part, and `color: transparent` hides it. `.value-handle { width: 6px; background-color: … }`
draws a handle where the fill ends; `volume.handle` and `pan.handle` are artwork for it.

### Piano roll sizes

`.piano-tool { width; height }` sizes the tool buttons (8 to 96 points wide), so stretched
artwork has room; `.midi-tools { width }` (80 to 400) and `.clip-info { width }` (120 to 480)
set the MIDI tools and clip columns' widths. `.loop-region { height }` above the native row
grows the Arrangement's loop row with it (up to 40 points).

### Fader size

`.fader-cap { width: 24px; height: 14px; }` sets the mixer fader's cap (4 to 64 by 4 to 96
points) and `.fader-rail { width: 3px; }` its rail (1 to 32).

### Device panels

`.device` styles every device's panel in the device chain and `#device-compressor` one kind of
device, by the built-in device's id in the table above. Its `:selected` and `:muted` (off)
states are there too, and everything in the panel is inside that scope: its header
(`.device-header`), its controls (`#device-compressor .device-control`) and the generated
parameter list's widgets (`#device-compressor .button`).

```css
#device-compressor { background-color: #1B1E22; }
#device-compressor .device-header { background-color: #E5484D; color: #FFFFFF; }
#device-reverb .device-control { accent-color: #7CC4FF; }
```

`width` on `.device` or `#device-plugin` widens a panel (up to 1600 points). A panel's
`background-color` is the surface behind its controls, not their fill, so its sliders keep
their rails. A parameter's slider has parts of its own once the sheet gives one of them a
background or artwork: `.slider-rail` (`height` is its thickness), `.slider-fill` (up to the
value) and `.slider-handle` (`width`), with the artwork roles `slider.rail`, `slider.fill` and
`slider.handle`. `.slider { --suhina-appearance: knob; }` draws sliders as knobs.

```css
#device-plugin { width: 640px; background-color: #0E1A24CC; }
#device-plugin .slider-rail { height: 3px; background-color: #FFFFFF22; }
#device-plugin .slider-fill { background-color: var(--accent); }
#device-plugin .slider-handle { width: 6px; background-color: #FFFFFF; border-radius: 3px; }
```

### Buttons and fields everywhere

`.button` (with `:hover`, `:active`, `:disabled` and `:checked` for a chosen row in a list or
menu) and `.text-input` style Suhina's ordinary widgets too: the buttons, check boxes,
drop-downs and text fields in Settings, in dialogs (Export audio, Save, the loading and closing
dialogs), on the start page, in the browser, the sample map, the Plug-in manager and in
popups. Scope them to one place with its class: `.settings .button`, `.dialog .text-input`.
`border-width: 0` removes egui's own outline. Settings' choices of several parts (Ask, Panel,
On) are `.button` parts too, the chosen one `:checked`.

```css
.button { background-color: #24272B; border-width: 0; border-radius: 4px; }
.button:hover { background-color: #2E3238; }
.button:checked { background-color: var(--accent); color: #000; }
.text-input { background-color: #15171A; }
```

### Bars and lines

The line under each bar is that bar's `border-color` (`.menu-bar`, `.application-bar`,
`.toolbar`), and the line above the view `#workspace`'s, so a theme can join the bars into one
surface without losing the lines elsewhere. `--border` is every other line: rulers, the
browser's edges and the piano roll's keys and rows. A bar with `border-width: 0` (or a
transparent `border-color`) fills its line with its own background, so no gap shows between
bars. A gradient over the whole stack of bars is not possible yet: give each bar its share.

`.arrangement { --suhina-header-tint: 0; }` (0 to 100) sets how strongly the track's colour
tints its Arrangement header, over the user's header colour strength: 0 leaves the header
`.track-header`'s own surface, for a see-through theme.

`border-radius` takes a percentage: `50%` makes a control round, and its `box-shadow` follows.

### Which file wins

`layout.css` is read after `theme.css`, so where both set the same thing (a placement, or the
artwork of one role such as `solo` or `track.body.arrangement`) the value in `layout.css` wins.
Suhina writes `layout.css` when you place controls or choose artwork in the app; delete a rule
from it to let your sheet's value show.

### Placement names

Each control has a placement name; a placement rule is `.control-NAME`, and a list of
them (`.control-a, .control-b`) places each. The Arrangement header has `arrangement-name`,
`arrangement-volume`, `arrangement-pan`, `arrangement-send-1`, `arrangement-send-2`,
`arrangement-sends-previous`, `arrangement-sends-next`, `arrangement-meter`,
`arrangement-input`, `arrangement-output`, `arrangement-automation`, `arrangement-color`,
`arrangement-enable`, `arrangement-fold`, `arrangement-monitor` (the MIDI monitoring chip),
`arrangement-number` (the track number, drawn as a label of its own while `arrangement-enable`
is hidden), `solo`, `cue`, `arm` and `track-type`; a mixer strip
`mixer-name`, `mixer-color`, `mixer-enable` (the track number), `mixer-toggles`,
`mixer-devices`, `mixer-divider`, `mixer-input`, `mixer-input-caption`, `mixer-monitor`, `mixer-output`,
`mixer-sends` (all of a strip's sends), `mixer-pan`, `mixer-volume`, `mixer-volume-readout`,
`mixer-volume-scale`, `mixer-compact-volume`, `mixer-meter` and `mixer-metronome`; the toolbar
`transport-play`, `transport-stop`, `transport-record`, `transport-metronome` and
`toolbar-MODULE`.

The Arrangement header changes its layout as its column narrows (about 315, 280, 240 and 160
points: names move to the lower row, the pan and then names are dropped, the volume moves).
Placements made at one width do not hold at another. Keep the column as wide as your
placements need with `.arrangement { --suhina-header-min-width: 330px; }` (160 to 520). A
control with no room left is not drawn, without a notice: check narrow tracks and strips too.

### Round 5 additions

- **Opaque themes never show the desktop.** Backgrounds write their alpha into the window only
  while `--window-background` has alpha; otherwise the window clears to its colour and every
  alpha in the theme blends with Suhina's own surfaces. A clip's body, the edited track's
  highlight and a control's surface always blend.
- **The Arrangement header:** the track-on toggle is `#control-track-on` as in the mixer
  (`:checked` on, `:muted` off, artwork `track-on`); its number is `.track-number` (`color`,
  `font-size`, `display: none`, `:muted`). `#control-automation` (`:checked` while its lanes
  show, artwork `automation`), `.fold` (`:checked` while folded, artwork `fold`) and
  `#control-monitor` (`:checked` while monitoring is In, artwork `monitor`).
- **Artwork reads scope variables:** `var(--track-colour)`, `var(--track-fill)` and
  `var(--clip-colour)` in an SVG take the track's or clip's value.
- **Values:** `--suhina-value-position: right | left | top | bottom | inside | none` on a knob
  (`.pan`, a volume knob, `.send`, the MIDI tools knob) places its value or hides it; `none` on
  a level bar hides its value.
- **Sizes:** `.send { width; height }` for the mixer's sends; `.arrangement {
  --suhina-track-height: 96px }` (20–400) for tracks without a height of their own, in place
  of the default size; `.piano-tool { height }` grows the piano roll's toolbar.
- **Device panels:** with `.device { width }` the generated list keeps one column that follows the panel and scrolls;
  `.device .value { width }` sets the value column (at most half the row);
  `.device { --suhina-param-columns: 3; --suhina-param-row: 48px; }` lays parameters side by
  side (1–8 columns, rows 16–160 points).
- **Browser:** `.browser-place` (a place or kind on the left: `:hover`, `:checked` and
  `:selected` while shown) and `.browser-row` (`:hover`, `:active`, `:selected`; its `color` is
  the name's).
- **Loop lines:** `.loop-line { color; width }`; the lines reach the returns and Main and
  follow the brace while it is dragged.
- **MIDI tools:** `.midi-tools .slider { --suhina-appearance: knob }` (or `#quantise-amount`)
  draws the quantise amount as a knob.
- **Piano roll tools:** artwork roles `piano.select`, `piano.draw`, `piano.paint`,
  `piano.slice`, `piano.erase`, `piano.mute`, `piano.cut`, `piano.copy`, `piano.paste`,
  `piano.duplicate`, `piano.quantise`, `piano.midi-tools`, `piano.audition` and `piano.hide`,
  each with `.hover` and `.on` (the chosen tool; `.checked` is not a state name). Their
  artwork keeps its aspect ratio.
- **Nine-slice:** the stretched edges and centre sample only their own pixels, so a hard edge
  stays hard.

### Round 6 additions

- **Families** (above) for rules over every part of a kind.
- **Formats:** `--suhina-value-decimals: 0 | 1 | 2` and `--suhina-value-unit: " dB"` on a
  value's control.
- **Device panels:** `.device { height }` (60–1200), `--suhina-param-label: left | top | hidden`,
  `.device .knob { width }`.
- **MIDI tools:** `.midi-tools { --suhina-gap: 2px }` and `.midi-tools .button { height }`.
- **Low tracks:** `.arrangement { --suhina-compact-below: 48px }`: a track lower than that keeps
  Suhina's own header layout instead of the placements, so nothing overlaps.
- **Settings folder:** Settings › Projects and backups › Projects › Settings folder moves all of Suhina's
  data. Changing it or the themes folder asks first, backs the files up beside the old folder
  and moves them.
- **A copy of a theme** says how many placed controls it keeps in its `layout.css`.
- **Choosing a theme** keeps the Themes page where it was.
- **Layers:** `--suhina-*`, `.control-*` and artwork rules work inside `@layer` blocks too; inside
  `@media` they do not (use plain CSS properties there).
- **Mixer volume readout:** `#mixer-volume-readout { background-color: #1E1E1E; border-radius: 6px;
  font-size: 11px; }`.
- **Empty slots:** `.slot { background-color: #262626; }`.
- **Pan and send knobs of their own:** `pan.knob.body`, `pan.knob.pointer`, `send.knob.body` and
  `send.knob.pointer` artwork change only those knobs, in the Launcher and the Arrangement.
  `.send { --suhina-appearance: knob; }` makes the Arrangement's sends knobs too.
- **The mixer's colour strip:** `.control-mixer-color { display: none; }` hides it; `top`, `height`
  and `width` move it (to the bottom, say).

## What a theme cannot set

- **Choices on the Settings page that change how things look:** the header and clip colour
  strengths, a waveform or note colour other than **Theme**, and knobs or fields for the mixer
  (apart from `.pan` and `.send { --suhina-appearance: knob | field; }`) are the user's. A
  shared theme can look different on another computer because of them. Only the track headers'
  side follows a theme.
- **Variants change variables only:** a colour written directly in a rule is the same in every
  variant. Write colours you want a variant to change as variables; artwork that writes
  `var(--name)` follows them too (below).

`track.name` and `track.body` artwork is drawn with `currentColor` as the header's fill: the
track's colour at the header colour strength.

## Signal-driven meter artwork

For a fully authored meter, set `--meter-style: frames;` in `theme.css`
and supply contiguous `meter.frame-0`, `meter.frame-1`, … SVG/PNG roles, up to 32.
The louder channel's measured peak selects the frame along the native -70 to +6 dBFS
range. Frame zero represents silence; the last frame represents the top of the range.
Unlike decorative image sequences, this display remains signal-driven with animations
off and ignores artwork fps. Missing/invalid sequences use native continuous bars.
Colour/state-specific meter sequences follow the same precedence as other roles.
The native clip light, accessible level description and click-to-reset remain active.

## Backgrounds on the see-through window

`background:` resets `background-color` to transparent when no colour is included.
Suhina treats that computed transparent colour as authored. Native background fills
write colour **and alpha** directly into the main window, so a transparent fill can
expose the desktop even when its gradient is opaque. For an opaque surface, use an
opaque base and a separate image. This applies to views, panels, bars and buttons.

```css
/* Wrong: resets the native base to transparent. */
.browser { background: linear-gradient(#243A30, #101C17); }
/* Right: opaque base and independently painted gradient. */
.browser { background-color: #101C17;
           background-image: linear-gradient(#243A30, #101C17); }
```

For overlays such as `.track-header` and `.track-name`, use `background-image` alone
to preserve the native fill. The `.meter-level` measured overlay may use the
shorthand. Meadow deliberately uses transparency over its private background PNG;
do not make an intentionally translucent theme opaque as a blanket correction.

## Closed-panel Grid tracks

CSS cannot open a closed browser or detail panel: its element is absent. A fixed
track still reserves space for that absent element. Put the dimensions on panels
and use `auto` for their tracks, so they collapse when the panel is closed.

```css
/* Wrong: reserves 248px and 200px even when those panels are closed. */
#workspace { display: grid; grid-template-columns: 248px minmax(0, 1fr);
             grid-template-rows: minmax(0, 1fr) 200px; }
/* Right: absent panels leave no fixed-size track. */
#workspace { display: grid; grid-template-columns: auto minmax(0, 1fr);
             grid-template-rows: minmax(0, 1fr) auto; }
#panel-browser { grid-column: 1; grid-row: 1 / -1; width: 248px; }
#panel-main { grid-column: 2; grid-row: 1; }
#panel-detail { grid-column: 2; grid-row: 2; height: 200px; }
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
the reduced-motion media feature are not connected. Bars and floating windows stay
host-managed; the toolbar's height stays native. Do not claim universal browser CSS
or container-query support. Notices are useful but are not a complete CSS validator.
Use the editor's Flex/Grid examples and check the current support specification.

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

Track/clip colours belong to the project. `clip.midiDefault` and `clip.audioDefault`
apply when new tracks/clips are created; theme edits must not recolour existing ones.
Use `text.primary` contrast of at least 7:1 and hints of at least 3:1 where possible.

## For AI-assisted edits

Before editing, read the three authoring rules above: preserve an opaque native base
with `background-color` plus `background-image`, collapse absent-panel Grid tracks,
and contrast value/inverse text against selected fills. Report those checks explicitly.


Describe the requested visual change and choose the smallest supported change.
Preserve unrelated palette, assets, layout, project data and native behavior.
Return a complete portable package and explain modified files, preview limitations
and verification. Never invent a successfully tested feature or silently discard
conflicting edits. Back up the user's theme before replacing it.

Current specifications: https://github.com/suhina-daw/suhina-daw/blob/main/docs/spec/theme-skins.md
and https://github.com/suhina-daw/suhina-daw/blob/main/docs/decisions/0030-native-theme-css.md
(on a development branch, use that branch's corresponding documents).

### Full-height controls and symbol proportions

Use `height: 100%` to stretch a control with its containing panel.
`top` is then the top inset and `bottom` the bottom inset (zero by default).
For example, a meter with four-point margins uses:

```css
.control-arrangement-meter { height: 100%; top: 4px; bottom: 4px; }
```

`height: auto` restores native height. Arrangement meter geometry follows the whole
track, so scrolling does not rescale its level. SVG symbol roles preserve their aspect
ratio and are centred; surface artwork such as button and track backgrounds stretches.

### Anchoring one control to another

A persistent anchor makes one edge of a control follow an edge of another control in
the same container, with a gap. The value is `<control> <edge> [<gap>px]`; the target's edge must lie on the
same axis (left/right, or top/bottom). For example, a meter that fills the track's
height but ends four points above the volume control under it:

```css
.control-arrangement-meter {
  height: 100%; top: 4px;
  --suhina-anchor-bottom: arrangement-volume top 4px;
}
```

Opposite edges keep the gap between the two controls (a bottom edge above a top edge);
matching edges inset by it (a top edge 4 points below another top edge). With one edge
anchored, a full-width or full-height control keeps its other edge and stretches;
otherwise it moves and keeps its size. Anchoring both edges of an axis sets both.
`--suhina-anchor-bottom: none` removes an anchor.

When the target is hidden, absent, or in another container, the anchored edge keeps the
control's own placement. Anchors that form a cycle (including a control anchored to
itself) or a chain longer than eight links are ignored with a warning. A control never leaves its container, and an anchor that leaves no room
draws nothing: in a mixer strip too short for the chains from its top and its bottom, the
anchored control that would stand on another one is not drawn. A strip without an audio input
(a MIDI track's) keeps the room its input would take, so a control anchored to `mixer-monitor`
or `mixer-input` lines up across every strip. The divider is a control as well:
`.control-mixer-devices { --suhina-anchor-bottom: mixer-divider top 0px; }` ends the device
list where the theme puts the divider, and its drag area follows it.

**Full width** uses `width: 100%`, with `left` and `right` as panel insets.
For example `.control-mixer-name { width: 100%; left: 4px; right: 4px; }`.
Fractional left/top offsets survive saving; existing integer offsets remain valid.
