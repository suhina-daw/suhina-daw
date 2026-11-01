---
description: Themes become style sheets in a CSS dialect of Suhina's own (elements, states, properties from one registry), the window becomes a grid of named areas on taffy, control artwork and meter styles are properties, and glass is a materials model whose renderer is chosen by measurement
read_when: theme, style sheet, CSS, selector, cascade, media query, layout, taffy, glass, blur, transparent window, artwork, SVG controls, meter style, edit interface, theme package
type: decision
status: draft
last_verified: 2026-10-29
covers:
  - crates/style/
  - crates/ui/src/kit/theme.rs
  - crates/ui/src/kit/widgets.rs
  - crates/ui/src/app.rs
  - apps/suhina/src/main.rs
  - crates/ui/src/kit/theme_vars.rs
---

# 0031: Style sheets for the whole UI

- **Status:** proposed
- **Date:** 2026-10-22
- **Amends:** [ADR 0009](0009-ui-toolkit-and-look.md) decision 6 (themes are style sheets; JSON
  colour themes are dropped). Decision 1 (the window is drawn by eframe) is amended only if
  plan step 5 chooses Suhina's own frame renderer.
- **Alternative to:** ADR 0030 *Native theme CSS*, proposed in
  [PR 841](https://github.com/suhina-daw/suhina-daw/pull/841). This ADR takes that pull request's
  features and [PR 809](https://github.com/suhina-daw/suhina-daw/pull/809)'s glass experiment as
  prior work (*Alternatives considered*). Choosing between the two ADRs is a maintainer's decision.

## Implementation status

The target below is being implemented in stages on the PR 841 / PR 809 foundation.
It is not a statement that the existing app already supports this whole language.
`suhina-style` now supplies a toolkit-independent bounded selector tree, native
property registry, cascade, custom properties, basic viewport media conditions,
Taffy row/column/grid/stack rectangles and source-preserving declaration edits.
The desktop uses its source editor for native CSS saves. Its native paint adapter
now resolves complete sheets limited to supported colours, font size, radius,
opacity, box/text shadows and linear/repeating-linear sRGB gradients, including selectors, variables and supported media
conditions. It preserves native defaults through authored-property markers and
retains the computed snapshot until state, source, palette or viewport changes.
Box and text shadow painters consume toolkit-independent `ResolvedShadow` values.
The compatibility adapter converts Stylo values only at its boundary.

The Stylo/Blitz compatibility engine has been removed. `suhina-style` now covers what
themes used it for: cascade layers, `calc()`, `color-mix()`, media ranges, viewport
units, `[data-object]` selectors, radial/conic gradients, the `background` and layout
shorthands, standard Flex/Grid properties on Taffy for the toolbar and workspace, and
transitions and keyframe animations. Before removal, tests compared native and Stylo
gradient pixels and geometry, paint values and authored markers; every bundled theme
parses without a notice. Unknown declarations are skipped with a notice and the rest of
the sheet applies; a sheet that does not parse keeps the last working one.
Persistent component anchors (`suhina_style::anchor`: an edge follows another element's
edge in the same container with a gap; cycle and chain-length validation; missing or
hidden targets fall back) are connected to the native placement adapter and the Edit
interface. They are part of the layout model, not of the `skin.controls` offsets, and move
with it to `position: overlay` and component layout.
Theme storage follows *Amendment: theme settings and variants*: a theme's folder holds its
`theme.css` (the author's sheet, never rewritten), `variants/*.json` (values for its settings,
written by the settings editors) and `layout.css` (what the Edit interface sets); no JSON theme
is written. Settings are registered custom properties (`@property`, `suhina_style::registered`):
the core set in `crates/ui/src/kit/theme_vars.rs` and any a theme declares. A theme of an
earlier version (JSON, or a `theme.css` importing `advanced.css`) is migrated once, only after
the new files read back as the same theme; its old files are kept in `legacy/`. The built-in
themes are in the same form (`crates/ui/themes/<id>.theme.css` and `<id>.layout.css`); their
artwork and Meadow's background stay embedded beside them.

Device panels use the tiers: `.sd` version 6 adds an optional panel `style`, applied in the
Device tier bounded to its panel; `#id` selects a control by its id; `user.css` in the themes
folder is the User tier for every view. Panel layout from CSS (a group/layout tree in the SDK)
is not yet connected: panel sheets style colours, ink, accents and the other painted
properties.

## Device panels use the same style engine

The `.sd` device remains JSON data: DSP nodes, connections, parameters, stable
control ids, gestures and parameter bindings. CSS supplies presentation, including
layout, without changing sound, parameter ranges, automation or command routing.
Built-in native devices use the same SDK panel structures without acquiring a DSP
graph. Their layouts can adopt the same style interface.

The host applies explicit tiers: default, application theme, device style, user
overrides. A higher tier wins even over `!important` in a lower tier; within one
tier importance, specificity and source order apply. These are native ownership
tiers, not the browser `@layer` importance-reversal semantics. Device sheets are
bounded to their panel root: `#cutoff` cannot select another device's control and
ancestor/sibling selectors cannot escape the root. Missing declarations retain
application defaults through the cascade or inheritance, as appropriate for each
property. No device stylesheet is required.

An optional device stylesheet and a group/layout tree will require an explicitly
versioned SDK format amendment. Existing fixed canvases remain readable. Global
theme changes and device/user edits must use one style and layout path, including
identical paint/input rectangles and stable command ids. Embedded styles/assets
must remain portable with the device; external editing is an authoring workflow,
not a runtime dependency on a file elsewhere on the author's machine.

## Context

Today a theme is a JSON file of colour tokens (`crates/ui/src/kit/theme.rs`,
[brief](../design/brief.md) › Themes). Sizes, spacing, fonts, shapes and layout are constants or
arithmetic in the views (`app.rs` cuts the window into bars and panes by hand). The aim is to make
the look as customisable as possible, and convenient to customise: a person or an agent edits a
text file, saves it, and sees the change.

CSS is the shape for that file: people know it, editors highlight it, and its core (selectors,
interactive states, the cascade, variables, media queries, transitions) fits any UI. What does not
fit is everything tied to HTML documents: the HTML attributes, the box model's corner cases, inline
text flow, floats, hundreds of properties. So a theme is **CSS's core language with Suhina's own
elements and properties**.

What the look needs:

- fonts: family, size, weight, style, line height, letter spacing, colour;
- sizing and positioning with proportional sizes like flex and grid, and themes that move and
  reorder parts of the window;
- backgrounds: colours, gradients, images; borders and corner radius; shadows;
- **control artwork**: a theme replaces how a knob, fader, button or meter is drawn with its own
  SVG or PNG images, while the control keeps its gestures, commands and undo;
- **glass**: panes that blur what is behind them, inside the window and, through it, the desktop.
  How much desktop shows through is separate from colour alpha, which mixes a colour into the
  colour below it.

### Prior work

Two open pull requests explore this ground, and this ADR builds on what they found:

- **PR 809** adds a see-through window with `window-vibrancy` on Windows, and "liquid" surfaces:
  blur inside the window, made by replaying the frame's egui shapes into offscreen wgpu captures
  under eframe, with three strengths (panel, track, clip) in Settings.
- **PR 841** adds portable theme packages with SVG/PNG control artwork, meter styles, an *Edit
  interface* over the real controls, and CSS computed by Stylo (through Blitz) with Taffy layout
  for the toolbar and the workspace panels, mirrored to and from theme JSON.

They show that SVG artwork for controls, bounded PNG decoding, gradient and shadow painting on
egui, Taffy-driven toolbars and blur inside the window under eframe all work in Suhina. Where this
ADR decides differently, *Alternatives considered* says why.

### Is it feasible?

Yes.

| Part | How |
| --- | --- |
| Parser, selectors, cascade, variables, media queries | Small, pure code over `cssparser` (MPL-2.0) tokens |
| Fonts, line height, letter spacing | egui's `TextFormat` has `line_height` and `extra_letter_spacing`; fonts load from files. Weights and italics need a file each (no synthetic bold) |
| Backgrounds, gradients, images, borders, radius, shadows | First on egui meshes with cached gradient and shadow textures (as PR 841 does), later a box shader of Suhina's own (one signed-distance pipeline, crisp at any radius) |
| Control artwork | `resvg` (MPL-2.0) for SVG, `image` (MIT OR Apache-2.0) for PNG, rasterised once per asset, scale and colour, never on the audio thread (as PR 841 does) |
| Interactive states, transitions | egui reports hover, press and focus per widget; `Context::animate_value_with_time` runs transitions |
| Proportional layout, rearranging | `taffy` (MIT), a flexbox and grid engine without a GUI; the work is moving `app.rs`'s hand-written rects onto it |
| Glass | A transparent window and the OS's blur (`window-vibrancy`), and a blur inside the window: either captures under eframe (PR 809) or a frame renderer of Suhina's own (section 6). Plan step 5 chooses by measurement |

**The desktop's blur comes from the OS**, so its strength is the system's, not the theme's; the
blur inside the window is Suhina's and is themed:

| Platform | Desktop blur | Through |
| --- | --- | --- |
| Windows 11 | Acrylic or Mica | `window-vibrancy` (MIT OR Apache-2.0), DWM system backdrop |
| Windows 10 | Plain blur | `window-vibrancy` |
| macOS | Vibrancy materials | `window-vibrancy`, `NSVisualEffectView` |
| Linux | None at first (KWin's blur protocol later) | Plain transparency |

## Decision

### 1. A theme is a style sheet

A theme is a `.css` file, or a folder with `theme.css` beside its images and fonts. Style sheets
stack in layers, later ones winning: **built-in default < theme < user overrides** (`user.css`,
for small tweaks that survive switching themes).

The language is CSS's core, everything in it that does not depend on HTML:

- rules, selectors (section 2), declarations, comments;
- custom properties `--name: value` and `var(--name, fallback)`; they inherit as in CSS, so a
  variable set on any element applies inside it;
- `calc()`, `min()`, `max()`, `clamp()` on lengths and numbers;
- `inherit`, `initial`, `unset`, and `!important`;
- `transition` on colours, lengths and numbers (`transition: background 120ms ease-out`);
- `@import "file.css"`, `@font-face`, and `@media` (section 3).

The built-in default style sheet reproduces Suhina Dark exactly, so the move changes nothing on
screen until a theme does. **JSON themes are retired:** built-in themes are rewritten as `.css`. Existing user themes are converted only after a validated round trip, with their original files and artwork preserved. Legacy JSON remains readable during the staged migration. The style sheet is the theme's own source and Suhina never
writes it: the settings a person changes in Suhina are values in a variant of the theme, and the
Edit interface writes the theme's `layout.css`.
*Amended 2026-10-23, see Amendment: theme settings and variants.*

Settings › Appearance keeps choosing, duplicating, reloading, importing and exporting themes, and
chooses a theme's variant; its colour editor changes the variant's values.

**Packages.** A theme exports as a ZIP of its folder, with its images and fonts and the generated
README (section 8). Built-in themes and the default artwork are embedded in the app; editing one
first duplicates it into the themes folder, so the embedded originals are never changed and can
always be restored.

```css
window {
  --accent: #A6A3FF;
  font-family: "Inter", sans;
  font-size: 11px;
  background: var(--window-background);
  backdrop: acrylic;            /* the OS blur behind the window */
  glass: 0.9 inset;             /* the window itself slightly see-through */
}

#browser {
  background: linear-gradient(180deg, #252421cc, #1D1C1Acc);
  glass: 0.5 inset;             /* a more see-through pane cut into the window */
}

menu { glass: 0.8; blur: 24px; }   /* a frosted pane over what is under it */

#control-bar { layout: row; gap: 4px; padding: 0 6px; align: center; }
#control-bar > .spacer { width: 1fr; }

button { transition: background 120ms ease-out; }
button:hover { background: rgb(255 255 255 / 6%); }   /* colour alpha: tints, glass unchanged */
button:focus-visible { border: 1px var(--accent); }
track:nth-child(even) > track-header { background: var(--panel-hover); }
clip:playing { background: var(--track-color); radius: 3px; }

knob { body: image("knob.svg"); pointer: image("pointer.svg"); }
knob:hover { body: image("knob-hover.svg"); }
fader { cap: image("cap.svg"); rail: image("rail.svg"); }
meter { style: segmented; segments: 24; intensity: 65%; }
mixer pan { appearance: knob; }

session track { width: 86px; }
session slot { height: 18px; }

@media (max-width: 900px) {
  window { areas: "menu" "control" "views" "detail" "status"; }   /* the browser moves out */
  #browser { visible: false; }
}
@media (desktop-blur: off) { #browser { glass: 0.9 inset; } }
```

### 2. Elements and selectors

The UI declares **elements**. Each has a type (`button`, `knob`, `fader`, `meter`, `slot`, `clip`,
`track`, `track-header`, `mixer`, `pane`, `menu`), and may have an id, any number of classes, and
states; `button#play.toggle:active` names all four.

- **Ids** name one-of-a-kind parts (`#browser`, `#control-bar`) and project objects. A project
  object's element carries the object's stable id ([ADR 0025](0025-object-identity.md)):
  `track#track-12`, so a rule for one track follows it when tracks move or are reordered, as
  PR 841's identities do.
- **Classes** carry data (`track.audio`, `track.midi`, `track.return`, and `.color-ff9a3c` for
  the track's colour), and elements set variables from data (`--track-color` on each track, from
  the project), so a theme can style by what a thing is.

Selectors are CSS's own, without the HTML-only parts:

- type, `#id`, `.class`, `*`, and any combination of them;
- **interactive states**, as in CSS: `:hover` (the element or anything inside it under the
  pointer), `:active` (pressed), `:focus`, `:focus-visible` (focused from the keyboard),
  `:focus-within`, `:enabled`, `:disabled`, `:checked` (a toggle that is on), `:open` (a menu,
  dropdown or section that is open);
- the app's own states: `:selected`, `:playing`, `:queued`, `:recording`, `:armed`, `:muted`,
  `:dragged`, `:drop-target`;
- structure: `:first-child`, `:last-child`, `:only-child`, `:nth-child()`, `:nth-last-child()`,
  `:empty`;
- logic: `:not()`, `:is()`, `:where()`;
- combinators: descendant, `>`, `+`, `~`; comma lists.

Attribute selectors test the attributes the host gives an element (`[selected]`,
`[kind="kick"]`, with `=`, `~=`, `|=`, `^=`, `$=`, `*=` and the `i` flag); `attr()` reads them
(issue 1017). Not supported: pseudo-elements, and pseudo-classes about documents and links
(`:link`, `:visited`, `:target` and the like). Specificity, `!important`, source order and the
layers decide between rules as in CSS.

States come from egui as the widget is drawn (`Response::hovered`, `is_pointer_button_down_on`,
`has_focus`); `:hover` on a container comes from the pointer being inside its rect, so a container
and its children agree in the same frame. The elements of a frame form a tree, bounded in size and
depth (section 9), that selectors match against; its ancestry and siblings are the ones drawn.

Every element type, id, class and state lives in one registry with a sentence on what it is. The
registry turns typos into warnings ("no element called `buton`") and writes the theme template's
README, so an agent can write a theme from the README alone.

### 3. Media queries

`@media` takes CSS's syntax (`and`, `not`, `only`, comma lists, range syntax such as
`(400px <= width < 900px)`) over the features that mean something for a desktop app:

| Feature | Matches |
| --- | --- |
| `width`, `height`, `aspect-ratio`, `orientation` | The window (with `min-` and `max-`) |
| `resolution` | The display's scale factor (`2dppx` on a Retina screen) |
| `prefers-color-scheme`, `prefers-contrast`, `prefers-reduced-motion`, `prefers-reduced-transparency` | The OS settings |
| `transparency` | `on` when the window is transparent (section 6) |
| `desktop-blur` | `on` when the OS blurs the desktop behind it |
| `platform` | `windows`, `macos`, `linux` |

The media types `screen` and `all` match; `print` never does. An unknown feature is false, with a
warning. Which queries match is a small bit set: styles are cached per bit set, so crossing a
breakpoint while resizing costs one fresh cascade.

### 4. Properties: Suhina's own list

| Group | Properties | Values |
| --- | --- | --- |
| Text | `font-family`, `font-size`, `font-weight`, `font-style`, `line-height`, `letter-spacing`, `color`, `text-align`, `text-shadow` | families from `@font-face` and the built-in fonts; `px`, `em` |
| Box | `background` (a list of layers), `border`, `border-top` …, `radius` (one to four corners), `shadow` | colours, `linear-gradient()`, `radial-gradient()`, `conic-gradient()`, `image(url, fit, slice)` |
| Layout | `layout` (`row`, `column`, `grid`, `stack`), `width`, `height`, `min-*`, `max-*`, `gap`, `padding`, `align`, `justify`, `columns`, `rows`, `areas`, `area`, `order`, `visible`, `position` (`flow`, `overlay`), `inset` | `px`, `em`, `%`, `fr`, `auto`, `calc()` |
| Artwork | `body`, `pointer` (knobs), `cap`, `rail` (faders), `face`, `icon` (buttons), `frames`, `frame-rate` | `image(url)`, `frames(url, count)`, `none` |
| Controls | `appearance` (`auto`, `knob`, `field`), `direction` (`vertical`, `horizontal`) | keywords |
| Meters | `style` (`continuous`, `segmented`, `needle`, `frames`), `segments`, `intensity` | keywords, integers, `%` |
| Glass | `glass`, `blur`, `glass-content`, `backdrop` | section 6 |
| Motion | `transition` | property, duration, easing (`linear`, `ease`, `ease-in`, `ease-out`, `ease-in-out`, `cubic-bezier()`) |

Colours are `#rgb`, `#rrggbb`, `#rrggbbaa`, `rgb()`, `hsl()` and `color-mix()`. The text
properties inherit, as in CSS; the rest do not. There is no margin (gaps and padding cover it), no
floats, no transforms. `prefers-reduced-motion` turns transitions off unless a theme says otherwise.

**One proportional unit.** `fr` is "a share of the space left": in a `row` or `column` it is
flex-grow with a zero basis, in a `grid` a grid track. `width: 1fr` beside `width: 4fr` gives one
fifth and four fifths in either.

**Artwork replaces drawing, never behaviour.** An artwork property names the image for one part of
a control; the registry says which element types have which parts. The fader's cap moves along its
rail, the knob's pointer turns with its value, and a button's icon sits on its face. Variants are
selectors: `knob:hover`, `track.audio button#arm`, `track.color-ff9a3c track-header`. `currentColor`
in an SVG is the element's `color`. `frames("name-#.png", 32)` names a sequence (`#` is the frame
number, at most 32 frames): on a decorative element it plays at `frame-rate` (frozen on frame zero
when animations are off); on a meter with `style: frames` the signal picks the frame, from silence
to the top of the meter's range. A missing or invalid image falls back to Suhina's own drawing.
Command dispatch, keyboard access, value ranges, gestures, undo and the meter's clip light and
reset stay the application's.

**`appearance: auto`** keeps the person's own preference (Settings › Mixer); `knob` and `field`
override it for that theme. **`direction`** turns a control with an axis: a horizontal fader drags,
draws its rail and scale, and increases to the right.

Every property is one entry in a declaring macro, as `palette!` in `theme.rs` is today: name, the
element types it applies to, value type, initial value, whether it inherits, whether it can
transition, and its sentence for the README. Adding a property is adding a line and using it; the
parser, cascade, docs and warnings follow from the entry. A property that is not in the registry is
an "unknown property" warning, never a silent no-op: a theme can only say what Suhina draws.

### 5. Layout: themes may rearrange the window

The window is a **grid of named areas**: menu bar, tabs, control bar, browser, views, detail area,
status bar. A theme moves any of them anywhere by rewriting `areas` on `window`, or hides one with
`visible: false`, as CSS grid does with `grid-template-areas`. Inside a region, the components (the
control bar's groups, a track header's buttons, a mixer strip's parts, device panels) are rows,
columns and grids that a theme reorders with `order` and turns with `layout`. An element moves
within its container; it never moves into another container (CSS cannot reparent either).
`position: overlay` with `inset` takes an element out of its container's flow and places it freely
inside the container, for layouts that rows and grids cannot express.

The canvases (clip grid, arrangement, piano roll, waveforms, sample map) keep laying out their
contents from the music: a note's x comes from its time. They read **metrics and paint** from
style: `piano-roll note:selected { radius: 2px }`, `session track { width: 86px }`,
`session slot { height: 18px }`, `arrangement track { height: 20px }`. Rendering, scrolling, hit
testing and drop targets read the same metrics, so they always agree.

Settings that change the layout today (Settings › Devices › Detail area width, the browser
shown or hidden) become classes on `window` (`window.detail-full-width`, `window.browser-hidden`),
so the default style sheet states every layout in one place. Splitters stay: a drag stores a size
for the grid track or element it moves, in the UI state (serialisable, ADR 0009 decision 5), which
overrides the style sheet within its `min-*` and `max-*`. Resetting the layout is a command
(AGENTS.md rule 6).

**The Edit interface** (from PR 841) arranges the window by hand. A toggle (a menu entry, the
control bar's context menu and a shortcut, as commands) outlines every element that can be moved
over the real controls, with an 8-point grid. Dragging or resizing an outline, or editing its
fields, changes `order`, sizes, `visible`, `direction`, or `position: overlay; inset`. While it is
on, the controls under it dispatch no commands. **Done** writes the changes as rules into the
theme's style sheet (a built-in theme is duplicated first) and is one undoable command; **Cancel**
or Escape discards them; entering and leaving without a change writes nothing.

### 6. Glass

#### The model

The window is a stack of **materials**: panes of frosted glass. Two numbers describe a spot of the
window: its **colour**, and its **opacity** `A`, how much it blocks the desktop (1: solid, the
desktop hidden; 0: clear, only the blurred desktop). The OS shows
`A × colour + (1 − A) × blurred desktop`.

The window is the first material. An element with `glass` is another, with a glass value `g` from
0 (clear) to 1 (solid). An element without `glass` is not a material: what it paints goes onto the
nearest material around it (its **material parent**, the first container that has glass, up to the
window).

A material sits in one of two ways:

- **Over** (`glass: 0.5`, the default): a pane lying on its material parent. Behind it is
  everything drawn under it so far.
- **Inset** (`glass: 0.5 inset`): cut into its material parent, replacing it in that area. Behind
  it is whatever is behind the parent (the parent's **base**). Going down the material parents, the
  base is what the first over pane found under it, or the desktop for anything inset into the
  window. The window itself is inset into the desktop.

**Blur comes first.** A pane blurs everything behind it before anything else happens: `blur` (a
radius, `24px` by default, `0` for clear glass) blurs the window's content behind an over pane, or
behind the over pane an inset pane reaches down to; the desktop behind it all is blurred by the OS.
Write `B` for what is behind a pane, already blurred: its colour `V_B` and opacity `A_B` (for an
over pane, what is under it; for an inset pane, its base; `A_B = 0` for the desktop). Then the pane
is laid over `B`:

```text
A = g + (1 − g) × A_B
```

Glass stacks like panes: light through two panes is reduced by both.

**Colour alpha** is how much of a surface a paint covers. It mixes the paint's colour into the
colour of the material it is painted on, and never changes `A`: `rgb(255 255 255 / 6%)` on a pane
tints the pane. A pane's own `background` is its surface. Where it has no paint, a pane takes the
colour behind it: an over pane the blurred colour under it, an inset pane its parent's colour. So
`glass` and `blur` alone frost a spot without tinting it.

Painting colour `C` with colour alpha `α` on a pane with glass `g`:

```text
surface S = α × C + (1 − α) × U        (U: the unpainted colour, above)
A'        = g + (1 − g) × A_B
V'        = (g × S + (1 − g) × A_B × V_B) / A'
```

For an over pane `U = V_B`, which simplifies to `V' = V_B + (C − V_B) × g × α / A'`; on a solid
spot (`A_B = 1`) that is ordinary alpha blending with `g × α`. Paint on an element that is not a
material mixes in with `α` and leaves `A` alone.

Examples, with the window at `glass: 0.9 inset`:

| Element | Glass | Opacity there | So |
| --- | --- | --- | --- |
| `#browser` | `0.5 inset` | 0.5 | Half the blurred desktop shows through the browser |
| a card in the browser | `0.5` (over) | 0.5 + 0.5 × 0.5 = 0.75 | The card frosts the browser under it and is more solid |
| a well in that card | `0.3 inset` | 0.3 + 0.7 × 0.5 = 0.65 | The well shows the browser behind the card, blurred |
| a menu over the arrangement | `0.8` (over) | 0.8 + 0.2 × 0.9 = 0.98 | The menu's colour counts 0.8, the blurred arrangement 0.18, the desktop 0.02 |
| a hover tint on a button | none | unchanged | Only the colour changes |

**Text and icons are solid by default.** Everything that is not a styled box (text, icons, lines,
the canvases' drawing, and any view not yet moved to style) blocks the desktop by its own coverage,
so a label on a clear pane stays readable. `glass-content: glass` on a material makes its content as
see-through as the pane instead.

`backdrop` picks the OS material for the desktop's blur (`blur`, `acrylic`, `mica`; the nearest the
platform has is used).

PR 809's track and clip "liquid" rims (a narrow lensing edge with highlights) are not part of this
model; they may come back later as a property of their own (*Open questions*).

#### The setting

Settings › Appearance › **Transparent window** turns see-through windows on or off (off by
default). It replaces PR 809's three liquid strengths: how much glass each pane has is the theme's.
A window is made transparent when it opens, so the setting applies the next time Suhina starts.
When it is off the window is solid: opacity is 1 everywhere, but panes still blur and tint the
window's content, so frosted menus keep their look. When it is on, the OS blurs the desktop where
it can, and elsewhere the window is see-through without blur. The media features `transparency` and
`desktop-blur` let a theme adapt. Changing the setting is a command, as every setting is.

#### How it is drawn

The model above is fixed by this ADR; the renderer that draws it is chosen in plan step 5, between
two routes, by measurement. Either route must meet the same requirements:

- what is drawn so far can be captured and blurred **in the middle of the frame**, before an over
  pane is drawn;
- **desktop opacity is kept apart from colour alpha**: the final alpha is the running opacity `A`,
  not the alpha of the last colour drawn (egui blends the alpha channel with the same source alpha
  as the colour, so with egui's own pass every colour with alpha would change how much desktop
  shows);
- text, icons, images and paint callbacks are all present in a capture, with nothing left out;
- the snapshot tests draw through the same path.

**Route A: captures under eframe** (PR 809). Each over pane's capture replays the frame's egui
shapes painted so far into an offscreen wgpu target from a paint callback, blurs it, and draws the
pane over it. It keeps eframe. PR 809 lists its costs: shapes are cloned and tessellated again on
the UI thread for each capture, the font atlas is copied and compared, and what a capture cannot
bind (another paint callback, a texture only the main renderer has) is left out. Keeping desktop
opacity apart from colour alpha under egui's own pass is not shown yet.

**Route B: Suhina's own frame renderer.** Suhina runs its own window loop on egui's own parts:
`egui-winit` for windows and input, `egui-wgpu`'s `Renderer` for egui's meshes and textures, and a
frame renderer of Suhina's own that splits the frame where it must. It is all safe Rust (AGENTS.md
rule 2). wgpu 30 presents transparent windows on Windows (`Dx12SwapchainKind::DxgiFromVisual`,
through DirectComposition), and `egui_kittest` takes a custom renderer (`TestRenderer`). The frame:

1. **Segments.** The frame is drawn into an offscreen target whose colour channels hold `V` and
   whose alpha holds the running opacity `A`, starting at 0. Each over pane leaves a marker in
   egui's paint list (a paint callback that draws nothing). The renderer draws egui's meshes up to a
   marker, then:
2. **Capture and blur.** It copies the pane's rect, plus the blur's reach, out of the target and
   blurs it (downsample, then a dual-filter blur; cheap at any radius). That is `B` for the pane,
   and the base for any inset pane inside it.
3. **Boxes.** A material's box computes `S`, `A'` and `V'` above from its capture (or the parent's
   colour and base) and writes them; a box that is not a material writes colour only.
4. **Content.** Text, icons and the canvases are drawn by egui-wgpu's renderer: they blend colour
   normally and their coverage adds to `A`, which keeps them solid. Under `glass-content: glass`
   they are drawn with alpha writes off.
5. **Finish.** One pass copies the target to the window, multiplying the colour by `A` (the
   premultiplied form OS compositors expect; on a post-multiplied surface it copies as is). With the
   setting off, it writes `A = 1`.

Either way, over panes are few (menus, popups, cards), so a frame has a handful of captures; the
number is capped (64), and panes past it draw without a capture, as a pane with `blur: 0` over a
solid spot. The maths lives once in Rust in the style crate (`glass.rs`), as the oracle the
renderer's snapshot tests compare against; tests use a fixed picture as the desktop.

### 7. Code

```text
crates/style            suhina-style, L0: no GUI toolkit, no file I/O; strings in, values out
  syntax.rs             cssparser tokens → rules, declarations, @import, @font-face, @media; spans
  selector.rs           selectors, specificity, matching against an element and its ancestors
  media.rs              media queries → a bit set of what matches
  property.rs           the property registry macro → ComputedStyle and its parsers
  value.rs              Color, Length (px, em, %, fr, auto, calc), Gradient, ImageRef, Frames,
                        Glass, Transition
  cascade.rs            layers, specificity, inheritance, variables → ComputedStyle
  layout.rs             ComputedStyle → taffy::Style; a node tree → rects
  edit.rs               writing a changed rule back into a sheet, keeping the rest of the text
  glass.rs              the glass arithmetic (the renderer's oracle)
  diagnostic.rs         warnings with file, line and column

crates/ui/src/style/    the egui side
  element.rs            the element registry and the frame's element tree;
                        ui.element("button").id(..).class(..).state(..)
  sheet.rs              loads the layers from files: imports, fonts, images; reload on save
  artwork.rs            SVG (resvg) and PNG (image) decoding, bounded; textures per asset,
                        scale and colour
  paint.rs              ComputedStyle → boxes (gradients, shadows) and pane markers
  text.rs               ComputedStyle → TextFormat; @font-face → FontDefinitions
  motion.rs             transitions over Context::animate_value_with_time
  regions.rs            the window's grid of regions → rects for app.rs
  edit_mode.rs          the Edit interface
  inspect.rs            the inspector

crates/ui/src/render/   the glass renderer (wgpu): captures, blur, boxes.wgsl, finish; under
                        eframe (route A) or Suhina's own loop (route B), and egui_kittest's
                        TestRenderer
apps/suhina/src/        theme folders, ZIP import and export, transparency and window-vibrancy;
                        the window loop if route B is chosen
```

The style crate takes text and returns values, so parser, cascade, media queries, layout, rule
editing and the glass arithmetic are tested without a window. The UI module is the only place that
knows egui; the render module the only one that knows wgpu. A view never reads a property by name:
it declares an element, gets a `ComputedStyle`, and either hands it to `paint` and `text` or reads
typed metrics from it. Resolved styles are kept until the element tree, the viewport or the sheets
change. What is drawn in bulk (notes, clips, grid lines) is not in the tree: a drawn element is
styled by its parent and its identity (kind, classes, states, attributes such as a clip's `color`),
once per distinct identity, so a repaint costs a hash lookup per note. Such an element is styled as
its parent's only child. A new tree of the same structure drops only the cached styles under the
elements that changed.

`theme::panel_background()` and the other token functions stay during the move and read the
variables, so views move over one at a time and the snapshots must not change on the way.

### 8. Convenient to customise

- **Live reload:** saving any file of the theme in use shows within a second (the
  `notify-debouncer-full` watch the themes folder has).
- **Edit interface:** arranging the window by dragging the real controls (section 5).
- **Inspector:** a toggle (a menu entry and a shortcut, as commands) that shows, under the
  pointer, the element path (`window > #views > clip-grid > slot > clip.audio:playing:hover`), the
  rules that matched it with file and line, and its glass, like a browser's developer tools.
- **Problems list:** Settings › Appearance lists every warning with file, line and column; a bad
  declaration is skipped and the rest of the sheet still applies.
- **Template:** Export template writes the README of every element, class, state, media feature,
  property and artwork part (from the registries), the default style sheet and the default artwork,
  for people and agents. Every exported theme carries the same README.
- **Example themes:** besides Suhina Dark, the built-in themes include PR 841's Sheet, Meadow and
  Blocks, rewritten as style sheets with their original artwork, so authors start from working
  examples of artwork, density and glass.

### 9. Hostile input

A theme is a file from the internet and never crashes Suhina (AGENTS.md rule 2):

- parse errors become warnings; a bad declaration is skipped;
- capped: file size, rule count, selector length, import depth, `calc()` depth, variable
  substitution (cycles, size), transition count, element-tree size and depth, blur radius, shadow
  layers, pane captures;
- `url()` resolves only inside the theme's folder (no `..`, absolute paths, symlinks or network);
- SVGs are self-contained: no scripts, no external or embedded images, no system fonts;
- PNGs are bounded (PR 841's limits): 8 MiB per file, 4096 pixels on a side, four million pixels
  and 16 MiB decoded per image, 16 MiB of artwork per theme; textures are cached within a fixed
  budget;
- ZIP import validates the whole archive before writing anything: relative paths inside the theme
  folder, bounded file count and expanded size, supported files only; an import never overwrites a
  theme, it takes an unused name;
- images and fonts that fail to decode are warnings, and the element falls back to Suhina's own
  drawing.

## Alternatives considered

**Stylo through Blitz** (PR 841). Browser-grade selectors and cascade, without writing them. Not
chosen: Stylo knows only the browser's properties and pseudo-classes, so `radius`, `layout`,
`glass`, `areas`, the artwork parts and the app's states (`:playing`, `:armed`) cannot be written,
and a theme falls back on `--suhina-*` custom properties, state classes and attribute selectors. It
also accepts hundreds of properties egui does not paint, which a theme author cannot tell apart from
ones that work. Blitz is a beta pinned to an exact version and needs generated HTML markup. The
subset this ADR needs is a few modules over `cssparser`.

**Theme JSON mirrored to and from CSS** (PR 841). Keeps existing JSON themes, but two
editable representations need merging and conflict handling. One style sheet has
nothing to mirror. Existing user themes are worth preserving: migration keeps the
originals and validates the converted result, including artwork and materials.

**Pixel offsets from native anchors** (PR 841's `skin.controls`). Moves any control without moving
views onto a layout engine. Not chosen as the layout model: offsets break when a container resizes
and say nothing about order or proportion. The Edit interface stays, writing grid and flow rules,
and `position: overlay` covers free placement.

**Liquid strengths in Settings** (PR 809). Not chosen: how see-through each pane is belongs to the
theme. Settings keeps one *Transparent window* switch.

## Plan

Each step keeps the app working and its snapshots unchanged unless the step says otherwise.
Nothing before step 5 changes how a frame is drawn or replaces eframe.

1. **`suhina-style`:** syntax, selectors, media queries, cascade, variables, transitions, rule
   editing; the default style sheet; the built-in themes as `.css`; JSON reading removed.
2. **Boxes, text and artwork on eframe:** the element tree, `paint.rs` with cached gradients and
   shadows, `text.rs`, `artwork.rs`, states and transitions; move the kit's widgets
   (`widgets.rs`), then each view's boxes, onto elements. Control artwork, meter styles,
   `appearance` and `direction` land here, with PR 841's example themes.
3. **Layout:** the window's grid on `taffy`, replacing the rect arithmetic in `app.rs`; then the
   components and the canvases' metrics; then the Edit interface.
4. **Convenience:** inspector, problems list, template README, theme folders and ZIP packages in
   backups, import and export.
5. **Glass:** a spike of both routes in section 6 (one blurred over pane, a transparent window,
   `window-vibrancy` on Windows 11 and macOS, the finish). Measure frame time with a full project,
   check what each capture leaves out, whether desktop opacity stays apart from colour alpha,
   resizing, a second monitor and the sample map's window. Choose the route, record it in this ADR
   (and amend ADR 0009 decision 1 if it is route B), then build materials, captures and the
   setting. A platform where the OS side fails gets transparency without desktop blur, or none;
   blur inside the window works everywhere the route does.
6. **The box shader:** one signed-distance pipeline for backgrounds, borders, radius and shadows,
   replacing the cached textures of step 2.

ADR 0009 decision 6 and the [brief](../design/brief.md) › Themes are updated in step 1.

PR 841's pieces move into these steps as pull requests of their own: the artwork pipeline, meter
styles and example themes into step 2, the Edit interface into step 3, the resource limits and ZIP
handling into steps 1 and 4, its painters into step 2. PR 809's glass work is input for step 5.

## Consequences

- Almost all of the look moves out of code into one default style sheet, which also documents it.
  Designers and agents change the look without a build.
- Built-in themes still follow the brief and keep the familiar layout; a user's own theme may look
  and be arranged however its author likes, down to the artwork of every control.
- One file is the theme: no JSON, nothing mirrored.
- The style sheet is a dialect: a browser-only property is an "unknown property" warning, not an
  effect. The README and the inspector make that clear. Everything the registry lists is drawn.
- eframe stays until step 5, and is replaced only if measurements show route B is needed. If it is,
  Suhina owns its window loop (window events, the second window, size and position); the hard parts
  (input, IME, clipboard, accessibility) stay egui-winit's.
- The desktop's blur strength is the OS's; Linux gets transparency without desktop blur at first.
  Machines without Vulkan, DX12 or Metal may not get a transparent window; blur inside the window
  still works where the chosen route does.
- New dependencies: `cssparser` and `taffy` in the style crate; `resvg` and `image` in the UI
  (`image` is a dev-dependency today); `window-vibrancy` in the app; `egui-winit` and `egui-wgpu`
  only for route B (both already in the tree under eframe). All are GPL-compatible, checked with
  `cargo deny` when added.
- PR 841 is not merged as it is: its engine, JSON mirroring and offsets are dropped, and its
  features are rebuilt on this design.

## Open questions

1. **The glass route** (section 6): is owning the window loop acceptable if step 5 shows route A
   cannot keep desktop opacity apart from colour alpha, or should desktop transparency wait?
2. **Transitions on layout** (sizes and positions), not only colours and numbers: wanted?
3. **Liquid rims:** should PR 809's track and clip rims return as a property (`rim`), and in which
   step?
4. **Artwork by file name:** should a theme that is only a folder of images (`knob.body.svg`,
   `fader.cap.hover.svg`, as in PR 841) still work, with the names read as rules, so artists can
   theme without writing CSS?

## Amendment: theme settings and variants (2026-10-23)

[PR 880](https://github.com/suhina-daw/suhina-daw/pull/880). A theme is one style sheet,
`theme.css`, as its author wrote it; Suhina reads it and never writes it. What a person can change
in Suhina are **registered custom properties**: a core set every theme has (the palette tokens and
the former model fields: meter style, segments and intensity, artwork frame rate, liquid
materials, surface gradients, background strength, Session geometry), registered with Suhina
Dark's values, and any a theme declares with `@property` (`syntax`, `initial-value`, and Suhina's
`label`, `group`, `range`, `step` for the settings panel). A **variant** is a JSON file in the
theme's `variants/` folder with values for those properties, a name and an optional `appearance`
(light or dark); its values apply in a Variant tier above the theme's rules. The settings editors
write only variants: an edit to a theme's own values makes a variant, *Custom*, so a built-in
theme needs no copy for it (its variants are kept in a folder of its id without its CSS).
`layout.css`, written by Suhina only, holds what the Edit interface and editors set beyond
settings: placements and anchors, the toolbar, artwork and the name. A change of layout or
artwork to a built-in theme still copies it. The picker shows one card per theme with a swatch
per variant. Open: rendered thumbnails per variant, and following the system's appearance with
the variant's `appearance`.

## Amendment: control shapes, overlays and presentation from the theme (2026-10-23)

Accepted for the parts below; *presentation defaults* is proposed.

A first theme made by hand on disk ([#880](https://github.com/suhina-daw/suhina-daw/pull/880)) found
the controls' shapes fixed in code: the CSS reached their colours only. So:

- **Control surfaces.** A native control's surface (background, `border-width` and
  `border-color`, radius, shadows, opacity) comes from its element, the native values where the
  sheet sets none. A transparent background paints nothing. A view's or bar's background is the
  surface behind its controls, not their fill.
- **Level bars.** Volume and pan fields are one level bar with a track and a `.value-fill`, a
  `height` for thickness, ids by their Edit interface names and artwork roles of their own.
- **Overlays.** A workspace panel the grid lays over the view is drawn on a layer above it.
- **Track headers' side.** `--suhina-track-headers` on `.arrangement` is used while the user's
  setting is *Theme*, the new default; the user's Left or Right wins.

A second theme made by hand (2026-10-23, round 2) added: host states (`:checked`, `:selected`,
`:playing`, `:recording`, `:armed`, `:muted`) and host variables (`--track-colour`,
`--track-fill`) in the cascade, per-view track artwork, a bar's own separator colour, `.menu` and
`.popup` classes and a least header width a theme may set.

**Proposed: presentation defaults from the theme.** The Settings page's other presentation
choices (header and clip colour strength, waveform and note colours, mixer knobs or fields)
would work the same way: the theme gives a default as a `--suhina-*` property, the setting gains
*Theme* and stays the user's override, and the root element says the value in use
(`[data-track-headers=left]`) so a theme can lay out each case. Open: which choices count as
presentation; whether present users keep their values as overrides; how this meets the
appearance, layout and interaction split of [#623](https://github.com/suhina-daw/suhina-daw/issues/623),
[#625](https://github.com/suhina-daw/suhina-daw/issues/625) and
[#868](https://github.com/suhina-daw/suhina-daw/issues/868); and whether a profile that bundles a
theme, its variant, the workspace, interaction and keymap belongs here or in an ADR of its own.

## Amendment: the Edit interface is removed for now (2026-10-25)

The visual placement editor (section 5, **Edit interface**) is removed. It saved placements in
`layout.css`, a format of its own beside the theme's CSS, so two systems controlled the same
layout, and placing controls with it broke layouts in testing. Placement rules
(`.control-NAME`) are still read from `theme.css` and from an existing `layout.css`, and are
written by hand. The direction agreed in
[issue #995](https://github.com/suhina-daw/suhina-daw/issues/995): one `theme.css` is the single
source of truth for style and layout, and a future visual editor mirrors it, writing plain CSS
through the source-preserving edits of `crates/style/src/edit.rs`. The removed editor is kept for
reference on the branch `feature/placement-editor`. Toolbar order (**Edit toolbar**) stays.
