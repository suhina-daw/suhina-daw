<!-- Copyright (C) 2026 The Suhina contributors; SPDX-License-Identifier: GPL-3.0-or-later -->
# Suhina theme package: author and AI guide

## What this package is

A theme changes Suhina's native egui interface through colours, SVG/PNG control artwork,
meter appearance, background images, and connected CSS styles and layouts. It contains
data, not executable plug-ins. Audio, parameter gestures and commands remain native.
Use this guide when creating, editing, explaining or sharing a theme, including when
asking an AI assistant to modify one. Inspect the supplied files before making changes.

## Files and why they are separate

```text
my-theme/
  my-theme.json       # identity, palette, skin, layout and resource references
  style.css           # canonical native properties mirrored with JSON
  advanced.css        # optional preserved modern CSS source
  assets/*.svg        # vector control artwork
  assets/*.png        # optional raster artwork and sequence frames
  background.png      # optional background image
  README.md           # this guide
  examples/dark-gloss.css  # optional paint-only example; not enabled automatically
  examples/compact-session.css  # optional native Session density; not enabled automatically
```

Installed themes keep `<id>.json` beside the `<id>/` resource directory instead.
Relative image and stylesheet paths in JSON are authoritative. Keep the whole
package together; do not move only the JSON when it references external resources.
Drop the ZIP on Suhina to import it, then select it in Settings → Appearance → Themes.
Open the themes folder from that page to edit the installed files.
The template ZIP also has `template.json` and reference palettes in `examples/`;
the importable example is `my-theme/`.

## How and when to edit each file

1. Duplicate a bundled theme in the editor or start with the supplied example.
2. Set the JSON `name` to a descriptive name. Theme file ids use letters, digits and `-`.
3. For palette changes, edit JSON `colors` or native `style.css` tokens. For example:

   ```css
   :root { --accent: #7FD1B9; --window-background: #14161A; }
   meter { style: segmented; segments: 24; }
   toolbar { offset-transport: 48px; }
   ```

   Native CSS token names replace JSON token dots with hyphens. JSON colours use
   `#RRGGBB` or `#RRGGBBAA`, alpha last. Missing palette tokens inherit Suhina Dark.
4. For conditional styles, selectors, variables, effects or connected Flex/Grid
   layout, edit `advanced.css`. JSON `skin.css` mirrors this source losslessly.
5. For a new control silhouette, edit an SVG or PNG and reference it in `skin.assets`.
   For layout positions, the toolbar's right-click → Edit toolbar mode is usually
   easier: drag the actual controls on the 8-point grid. Done saves; Cancel or
   Escape discards. Entering/leaving edit mode alone preserves the layout.
6. Save one source at a time and inspect Suhina's warnings and the actual interface.
   Files are watched approximately once a second. Independent JSON/CSS edits merge;
   conflicting values preserve both files and require correction before saving.
   Canonical `style.css` is normalized; its comments/formatting are not preserved.
   `advanced.css` retains authored source. Conditional CSS is not flattened into
   unconditional JSON palette tokens.
7. Test normal, hover, active and disabled states, narrow/wide windows, interaction,
   clipping and contrast. Export and reimport the ZIP to check portability.

To try the supplied dark metallic look, use **Load dark gloss example** in the
advanced editor and then Apply, or copy `examples/dark-gloss.css` into `advanced.css`.
It demonstrates original gradients, rounded SVG-backed buttons, box/text shadows
and coloured transport controls while preserving native geometry. Back up your
current CSS before replacing it with an example.

Use **Load compact Session geometry** under Native CSS, then **Apply CSS**, or copy
`examples/compact-session.css` into canonical `style.css`. It sets track/return/scene
widths and header/slot heights. These are native properties mirrored to `skin.session`:
widths are 64–512px, slot height 16–64px and header height 20–64px. Use `auto` to remove
an override. The same geometry drives drawing, scrolling and drop/input targets.
Mixer height keeps its existing native divider/preference. Session and Arrangement
remain separate; this does not introduce the separately planned mixer view.
The example also includes `.pan { --suhina-appearance: knob; }`, a portable native
pan-face override for Session and Arrangement. `field` forces a number field, while
`native` retains the global preference. Native parameter gestures remain unchanged.

The gloss example gives `.mixer` and `.track-name` their own materials. Meter gradients
under `.meter-vertical .meter-level` and `.meter-horizontal .meter-level` keep their
colour stops anchored to the full dB range; peaks/RMS reveal the appropriate portion.
Outset shadows form the glow around the lit region, without filling empty rails or
segment gaps. This differs from a decorative gradient that stretches with the level.

**Meter intensity** in the theme editor dims measured ink and glow from 0–100%.
Canonical `meter { intensity: 65%; }` mirrors to JSON `skin.meter-intensity: 65`.
It also affects needles, hold marks and signal-driven artwork frames, while keeping
the native clip warning visible and native bar rails unchanged. Frame artwork dims
as a whole, including any background baked into its image. This changes visual opacity, not audio
gain or saturation. Change palette/gradient colours for different hues or saturation.
Advanced `.meter-level { opacity: .5; }` additionally dims native bars, hold marks,
needles and CSS fills/glow. Transparent CSS fills suppress native fill fallback.
Generic CSS `filter: brightness()` / `saturate()` is not connected to native painting.

## Native element placement and artwork variants

The built-in **Suhina Sheet** is a complete green-and-white worksheet-style example
with original SVG artwork for all 16 base roles. Select it in Themes, duplicate it
to edit, or export it to obtain its JSON, CSS and editable SVG files together.
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
artwork behind Session slots; empty cells preserve it and native clip launches remain
unchanged. `off`/omission preserves native fills. Native custom properties inherit
from the nearest authored ancestor, and a closer scope can override them.

Right-click the toolbar and choose **Edit interface**. The real controls remain visible
under outlines and an 8-point grid. Drag an outline, or select its semantic name in the
tools window to change offsets, dimensions, bottom anchoring and volume direction.
Matching instances share the rule. **Done** saves; **Cancel** or Escape discards it.
Entering and leaving without changes preserves the theme. The editor suppresses normal
control commands while placing them. The containing panel bounds each element.

These same settings mirror between JSON `skin.controls` and canonical `style.css`:

```css
.control-mixer-name { bottom: 0px; }
.control-mixer-color { display: none; }
.control-mixer-volume { width: 100px; height: 20px; --suhina-direction: horizontal; }
.control-mixer-meter { width: 80px; height: 10px; left: 0px; top: 0px; }
```

Offsets are relative to the native anchor, in logical points, bounded to ±16384;
sizes are `auto` or 1–4096px. `bottom` anchors to the containing panel. This explicit
native positioning contract is separate from the advanced Stylo/Taffy stylesheet.
Placement overrides do not automatically resolve overlaps with neighbouring elements;
when moving a name into the footer, arrange the toggles and sends to leave it room.
Changing direction changes the native drag axis, rail, cap and scale together.
The dB labels/ticks can also be moved, resized or hidden independently with
`.control-mixer-volume-scale`; `.fader-scale` styles their ink. This keeps a swapped
meter/fader layout from forcing the scale to remain on the fader's original side.
Native compact-height thresholds remain in force. Placement coverage is still growing:
toolbar modules/transport, the Session mixer sections, solo/cue/arm/type glyphs and
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
following native return order. Session sends stay inside their movable sends region.

Supply `track.name` and `track.body` SVGs or PNGs for name and strip backgrounds. Exact raw
track colours choose variants independently of selection/mute tint:

```css
track.name { image: url("assets/name.svg"); }
track.name.color-00ff00 { image: url("assets/green-name.svg"); }
track.name.color-00ff00.hover { image: url("assets/green-name-hover.svg"); }
artwork { fps: 12; }
```

For an image sequence, supply contiguous `.frame-0`, `.frame-1`, … keys for that
variant (at most 32 frames), e.g. `track.name.color-00ff00.hover.frame-0`.
Frame rate is 1–30 fps, mirrored as `skin.artwork-fps`. With native animations off,
the first frame stays visible without continuous repainting. Colour/state defaults
fall back to the role's normal artwork and then Suhina's embedded original.
PNG artwork retains authored colours and transparency rather than applying `currentColor`.
Each PNG is limited to 8 MiB compressed, 4096 pixels per axis and 4 million pixels;
combined inline artwork is limited to 16 MiB. Rendered control textures are bounded
to 1024 pixels per axis. SVG remains self-contained: embedding images or external
resources inside an SVG is not supported. Use separate PNG role files instead.

## SVG inheritance and safe defaults

Supported roles are `fader.cap`, `fader.rail`, `knob.body`, `knob.pointer`, `button`,
`play`, `stop`, `record`, `solo`, `arm`, `cue`, `track.midi`, `track.audio`,
`track.name`, `track.body` and `meter`. State variants append `.hover`,
`.active` or `.disabled`. Example JSON override:

```json
{"skin":{"assets":{"play":"assets/play.svg","button.hover":"assets/button.hover.svg"}}}
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
| Palette | JSON/native CSS tokens, transparency, project-safe creation defaults |
| Artwork | SVG/PNG roles, colour/state variants and frame sequences, moving faders and rotating knob pointers |
| Meters | `continuous`, `segmented` (4–64 segments), `needle`, signal-driven `frames`; signal remains dBFS |
| Materials | Optional bounded PNG background, opacity/blur; panel/track/clip gradient and liquid settings |
| Toolbar | Native layout plus anchored offsets, explicit order/visibility, connected Flex/Grid |
| Workspace | CSS layout of `#panel-browser`, `#panel-main`, `#panel-detail` under `#workspace` |
| Styles | Connected foreground/font size/radius, linear/radial/conic and repeating gradients, bounded inset/outset box shadows and shared native text shadows |
| Motion | Connected paint transitions/keyframes; native animation-off/high-contrast settings suppress motion |
| Selectors | Retained native class tree, child/sibling matching, connected hover/active/disabled/focus states, named object identities |

The advanced adapter uses Stylo for CSS and Taffy for connected layout. For example:

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
browser/detail splitters; removing the layout rules restores saved native sizes.
The toolbar exposes `browser`, `tempo`, `signature`, `grid`, `count-in`, `transport`,
`position`, `metronome`, `mapping`, `performance`, `views`, `agents` module ids.
Device SDK controls expose semantic classes such as `.device-panel`, `.knob`,
`.button` and `.device-control`. Stable named track/clip/slot/scene scopes also exist.
The native centred toolbar exposes the same module ids as CSS layouts. Its main
buttons additionally expose `#control-play`, `#control-stop`, `#control-record`,
`#control-browser-toggle` and `#control-metronome-toggle`. Paint-only rules preserve
the existing geometry. Button backgrounds (gradients/shadows) remain visible above
SVG body artwork, below glyphs and labels; CSS corner radius also clips that artwork.
Class coverage and host integration are still expanding: inspect the current spec
and test the actual target instead of inventing selectors.

## Signal-driven meter artwork

For a fully authored meter, use `meter { style: frames; }` in canonical `style.css`
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


This is an experimental native adapter, not a browser. Stylo parsing a property
does not mean egui paints it. Some raw widgets still need integration. Transforms,
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

Use **Edit interface → Full height** to stretch a control with its containing panel.
Y becomes the top inset; **Anchor to bottom** supplies the bottom inset (zero by default).
For example, a meter with four-point margins uses:

```css
.control-arrangement-meter { height: 100%; top: 4px; bottom: 4px; }
```

`height: auto` restores native height. Arrangement meter geometry follows the whole
track, so scrolling does not rescale its level. SVG symbol roles preserve their aspect
ratio and are centred; surface artwork such as button and track backgrounds stretches.

Interface dragging uses the 8-point grid and nearby peer edges/centres by default.
Hold **Shift** for one logical pixel steps with peer alignment; **Alt+Shift** for free
subpixel placement without snapping. Alt alone keeps normal snapping. Guides indicate alignment;
aligning does not create a persistent dependency on another control.

**Full width** uses `width: 100%`, with `left` and `right` as panel insets.
For example `.control-mixer-name { width: 100%; left: 4px; right: 4px; }`.
Fractional left/top offsets survive saving; existing integer offsets remain valid.
