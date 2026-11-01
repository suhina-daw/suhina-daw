---
description: Suhina's visual language: principles, what stays familiar and what is our own, layout decisions and design tokens
read_when: UI design, colours, tokens, typography, theme, accent, track colours, browser design, squint test, reference mockups
type: spec
status: draft
last_verified: 2026-10-28
covers:
  - crates/ui/src/kit/theme.rs
  - crates/ui/src/settings/themes.rs
  - crates/ui/src/interaction/grid.rs
  - apps/suhina/src/theme_files.rs
  - crates/ui/themes/**
  - crates/ui/src/settings/settings_view.rs
---

# Design brief

How Suhina looks. Read with [ADR 0009](../decisions/0009-ui-toolkit-and-look.md): the workflow
is familiar to Ableton Live users, the look is Suhina's own. This brief is what a designer (or a
design tool) gets, together with the functional specs in `docs/spec/ui-*.md`. A screenshot of Live
may show a design tool where things are; the look always comes from this brief, never from the
screenshot (ADR 0009, decision 4).

Reference mockups (our own work, minimal v3):
[Session](reference/session-minimal-v3.png) and [Arrangement](reference/arrangement-minimal-v3.png).
They are the visual target; this file records the decisions behind them, and
[`components.md`](components.md) the measurements and states of every component. Where the text and an
image disagree, the text wins. These images may be given to design tools and agents. Checked
against the "Make our own" table below on 2026-10-12: every row holds. The menu bar still uses
the same top-level names as Live (File, Edit, Create, View, Options, Help); menu structure is a
method of operation and free to follow, but our own names (for example Insert, Preferences) are
an option for the UI.

## Character

Minimal, dense, calm. A tool for long sessions: the grid and the timeline dominate, everything
else stays quiet. Colour appears in one place, the tracks and clips; the rest is graphite.
Hierarchy comes from lightness steps, not from cards, shadows, gradients or rounded corners.

## Principles

From [research](#sources) and the first design rounds:

1. **Grey first.** Build hierarchy with lightness only, check it in greyscale, add colour last.
2. **Quiet the surroundings** instead of shouting the important thing.
3. **States differ in lightness, not hue alone** (playing is brighter, not just "accent").
4. **Two type sizes:** 12 px for UI text, 11 px for small values. No bold headings. 9 px is
   kept for tags beside a name, 13 px for a page title (components › Global rules).
5. **One window, no pop-ups.** Menus and pickers open in place. The exceptions are plug-in
   editors, which float in their own windows (ADR 0006), the project dialogs, and the sample map
   when the user moves it into a window of its own for another screen
   ([spec](../spec/sample-map.md) › Where the map is shown).
6. **Progressive disclosure.** Controls appear when there is room or need (track height, toggled
   sections), never all at once.
7. **Every grid shows its lines.** Columns and rows of the Launcher grid, bars and beats, key rows.
8. **Not pure black.** The darkest surface is a warm near-black so the eye can adapt.

## Keep familiar (behaviour and structure)

- Window regions: control bar on top, browser on the left, the main view in the centre, the
  detail area (devices or clip editor) at the bottom, info text and status at the very bottom.
- Launcher view as a grid: tracks are columns, scenes are rows, scene launch on the right, return
  tracks and the main track after the regular tracks.
- Arrangement view: track headers with their controls on the left of the timeline by default
  (#1046); the right is a setting, and a theme may choose it.
- Interactions: click to launch, pending state while quantised, drag from the browser, double-click
  to open a clip (on an empty slot or lane of a MIDI track, to make one), Tab between Session and
  Arrangement; keyboard shortcuts and menu structure.
- Number fields that are their own sliders (drag to change, the fill shows the value).

## Make our own (appearance)

| Area | Suhina | Avoid |
|---|---|---|
| Base palette | Warm graphite surfaces in four lightness steps (see tokens) | Live's neutral mid-grey |
| Accent | Iris `#A6A3FF`, used sparingly: transport play, solo, queued, device on, selected parameter | Orange as the active colour; cyan as the value colour |
| Track colours | Our own muted palette (terracotta, ochre, steel blue, lavender, …) | Live's saturated pastel palette |
| Type | Manrope for UI text, IBM Plex Mono for numbers and time | Live's typeface |
| Knobs | Small circle with a pointer line, value below | Live's open arc knobs |
| Value fields | Neutral grey fill; volume fills full height with a level tick at the edges only; pan is a thin bottom bar growing from a centre tick | Live's cyan-filled boxes |
| Toggles | Track on = light grey number button; solo = accent; arm = red ring, filled when armed | The orange numbered track button |
| Track headers | A band of the track colour down the Arrangement header's edge; the Launcher header filled with it, its name in the theme's light or dark text, whichever reads | A dot of colour too small to tell tracks apart |
| Clip slots | Flat cells, a small square in the track colour; playing = brighter cell with an accent progress line; queued = accent outline; recording = red dot and line | Fully coloured slots with triangle and square glyphs |
| Arrangement clips | Neutral body, 2 px top strip and note or wave marks in the track colour | Colour-filled clip bodies |
| MIDI notes | Track colour, velocity as brightness, selected note with a light outline | Velocity as saturation; blue selected notes |
| Device panels | Plain panels, a square on/off switch in the header; a device with more room to show draws its own panel (the NAM Amp Modeler's and Cab Loader's: a name dropdown with steppers, knob columns with labels over and values under, lit toggles, a file dropdown at the bottom — [nam spec](../spec/nam-amp-and-cab.md)) | Live's title bar with a round activator; Live's Amp and Cabinet & Mic devices' look |
| Meters | Two thin bars, green with yellow only near the top | Live's meters |
| Browser | Search, category filters as buttons, sidebar of places only (Pinned, Recent, Folders), tree with type icons, colour tags as dots on rows, preview with waveform, BPM and key | Live's sidebar of Collections, Library categories and Places |
| Icons | Lucide (ISC) or our own simple strokes | Any glyph traced from Live |
| Text | Our own wording ("Drop clips or devices to add a track", "All MIDI in") | Live's interface strings |
| Logo, name, splash | Entirely Suhina's own | Anything resembling Ableton or Push branding |

## Layout decisions

- **Grid lines:** Launcher columns are separated by `border.strong`, scene rows by `border`; the
  separators run through the mixer section. Arrangement tracks are separated by `border.strong`.
- **Track headers grow with track height** (Arrangement): compact = name and on/solo/arm; then
  volume and pan; then routing (output, and input, channel and monitor In/Auto/Off with
  recording); sends ("Send A", "Send B") go above routing once the engine plays them. Groups are
  separated by space only, routing gets a little extra space above it (components › Track header
  levels has the heights).
- **Detail area is resizable** with a splitter, up to nearly the full window height
  ([clip editor spec](../spec/ui-clip-editor.md)).
- **Clip properties panel:** one label row and one value row per pair, no group titles; Loop is a
  toggle row.

## Tokens

All colours, spacing, radii, stroke widths and type sizes are named tokens in one theme module.
Widgets never contain literal colours. The colours below are the built-in theme, Suhina Dark;
a theme file can change any of them (see Themes). Current values:

| Token | Suhina Dark | Use |
|---|---|---|
| `window.background` | `#1D1C1A` | The window behind everything: grid, empty slots, lanes, Settings |
| `panel.background` | `#252421` | Panels and headers: menu, control and status bars, track headers, beat lines |
| `panel.hoverBackground` | `#2A2926` | A panel or list row under the pointer |
| `control.background` | `#2E2D2A` | Fields, buttons and stopped clips |
| `control.hoverBackground` | `#34332F` | A field, button or clip under the pointer |
| `control.activeBackground` | `#45433F` | Selected or pressed: a selected clip or track, a dragged field |
| `control.playingBackground` | `#4A4843` | A playing clip or scene |
| `control.selectedBackground` | `#C9C4BB` | The chosen option of a switch, a lit toggle, the level of a volume or pan field |
| `border` | `#383733` | Hairlines between regions and rows |
| `border.strong` | `#45433F` | Launcher columns and arrangement track lines |
| `text.primary` | `#ECE8E1` | Names and values |
| `text.secondary` | `#9A958C` | Labels and notes |
| `text.hint` | `#66625B` | Inactive text, hints and section titles |
| `text.value` | `#C9C4BB` | Clip names and values on fields |
| `text.inverse` | `#1D1C1A` | Text on lit fills: a switch's chosen option, a lit button |
| `text.playing` | `#FFFFFF` | The name of a playing clip |
| `text.onColorLight`, `text.onColorDark` | `#ECE8E1`, `#1D1C1A` | The contrast-selected text on coloured tracks, returns and clips |
| `browser.text.primary`, `browser.text.secondary`, `browser.text.hint`, `browser.text.value` | `#ECE8E1`, `#9A958C`, `#66625B`, `#C9C4BB` | Independent browser names, labels, hints and values; older themes inherit the corresponding text tokens |
| `accent` | `#A6A3FF` | The playhead, queued clips, a key being set |
| `recording` | `#E5484D` | Recording and errors |
| `recording.background` | `#3A2B2A` | Behind an armed track's arm button and a recording clip |
| `meter.normal` | `#8BC48A` | A level meter below the top |
| `meter.hot` | `#E3C86A` | A level meter near the top |
| `slot.empty` | `#4A4743` | The mark in an empty clip slot |
| `track.mutedBackground` | `#34322F` | The fills of a muted track |
| `pan.background` | `#232220` | Behind a track header's pan field |
| `pan.centre` | `#55524C` | The centre mark of a pan field |
| `track.1`–`track.8` | `#FF9A3C` `#F2D541` `#4FC3F7` `#B38BFF` `#7FDB5A` `#FF6FA8` `#5C8DFF` `#2FD9C4` | Eight colour-picker swatches, also used to resolve legacy uncoloured tracks once on opening. New tracks and clips use the MIDI/audio creation defaults; + picks any colour |
| `track.5`–`track.8` | `#8FA87A` `#C27C8E` `#7F8FB0` `#B9B2A6` | The other colours of the track colour palette (track menu, colour areas, colour tags' menu) |
| `kind.kick`, `kind.snare`, `kind.clap`, `kind.closedHat`, `kind.openHat`, `kind.hat`, `kind.rim`, `kind.tom`, `kind.crash`, `kind.ride`, `kind.cymbal`, `kind.percussion`, `kind.bass`, `kind.tonal`, `kind.vocal`, `kind.effect`, `kind.other` | `#D06E5E` `#73AAC0` `#5EC0B0` `#C6AC5E` `#D08F6E` `#D8C47A` `#8CA8E0` `#B074A4` `#E0A0C8` `#C8B8E8` `#A497CC` `#8FC06E` `#A84F4F` `#6EB0D0` `#E8D0A0` `#9CC49C` `#8A8680` | Sample-map points of each kind ([spec](../spec/sample-map.md)) |
| type | 12 px UI, 11 px values, 9 px rulers | |
| row height | 20 px grid rows and fields, 15–16 px inline fields, 17 px browser rows (`theme::BROWSER_ROW`) | |
| radius | 0 everywhere | |

## Themes

A theme is a JSON file in the `themes` folder of Suhina's app-data folder, beside
`settings.txt`; its file name without `.json` is its id, which `settings.txt` keeps as
`theme = <id>`. It names the theme and gives colour tokens as `#RRGGBB`:

```json
{ "name": "Night", "colors": { "window.background": "#14161A", "accent": "#7FD1B9" } }
```

Text and backgrounds never share a token: text on a lit fill is `text.inverse`, text on a track
colour is `text.onColorLight` or `text.onColorDark`, and the chosen option of a switch is filled
with `control.selectedBackground`. A file from before these tokens takes `window.background`
(opaque) for `text.inverse` and `text.onColorDark`, and `text.value` for
`control.selectedBackground`.

A background token's alpha is how much of the desktop shows through the window there. A
background (`window.background`, `panel.*`, `control.*Background`, `track.headerBackground`,
`track.headerSelectedBackground`, `track.mutedBackground`, `pan.background`,
`recording.background`, and colours mixed from them) sets the colour and alpha of the pixels it
covers rather than blending with what is under it; text, lines, marks and overlays blend. While a
background has alpha, the system blurs what is behind the window. Dialogs, menus and floating
panels blend with the window behind them, and menus (the menu bar's, right-click menus and
dropdowns) blur the window's content under them, with the GPU renderer and effects on.

The window is see-through with the GPU renderer where its swapchain hands alpha to the system's
compositor, which Suhina asks the GPU on the first frame: on macOS always, on Windows with DX12
(its swapchain is made from a DirectComposition visual), and on Linux where the GPU offers
premultiplied or postmultiplied alpha (usually Wayland; X11 usually not). Elsewhere, and with
OpenGL, the window stays opaque and backgrounds blend. The blur behind the window is the
system's: macOS's, DWM's acrylic on Windows, and the compositor's on Wayland where it offers one
(KDE); other Linux desktops show the desktop through unblurred. On macOS the window's content
runs under the title bar, which takes `panel.background`; Windows draws its own title bar (the
application bar), and Linux keeps the system's.

A displayed private background image takes precedence over desktop transparency: background
fills blend over that image. Clearing the image restores the host's transparent fill path.

A token the file leaves out keeps Suhina Dark's colour, so a file needs only the colours it
changes; an unknown token or a value that is not `#RRGGBB` or `#RRGGBBAA` is skipped with a warning. Transparent RGB is preserved under canonical token names, including inherited text roles. Files with
the first token names (`surface.0`, `line`, `text.tertiary` …) still read. Only colours belong to
a theme: sizes, spacing and type stay the design's. A track's own colour comes from the project; tracks without one take
a numbered palette colour resolved once when opened. `clip.midiDefault` and
`clip.audioDefault` are creation-only RGB defaults, copied to new tracks and clips;
existing project colours never follow edits to those defaults. The theme editor
offers these two defaults; numbered `track.1`–`track.8` remain legacy swatches for file
compatibility and colour pickers, rather than a list of default track colours.

Forest Amber is an original bundled woodland palette with warm amber accents, saved gradients
and theme-specific liquid strengths. Its built-in id is `suhina-forest-amber`, leaving existing
custom `forest-amber` files intact. Background images remain separately chosen local files.

Themes that come with Suhina (`crates/ui/themes/`, read-only; editing one makes a copy):
Suhina Sheet is an original playful spreadsheet-inspired light theme: green chrome,
white cells, compact Session geometry and embedded square SVG controls. Its palette,
CSS and artwork travel together when duplicated or exported.
Suhina Meadow pairs classic blue/cream paper surfaces with an original generated
green-hill background and translucent liquid materials. Its SVG controls and PNG are embedded and export with the theme. Suhina Blocks adds original brown plank surfaces, stone-like controls and colour-keyed pixel textures without landscape wallpaper.
Suhina Dark (the default), Editor Dark (cool greys and blue, like a code editor), Suhina
Light, High Contrast, and four popular open palettes mapped onto the tokens: Dracula, Nord,
Catppuccin Mocha and Latte, Solarized Dark (all MIT, licences in `crates/ui/themes/licenses/`).

Settings › Appearance › Themes picks a theme from preview cards. Edit current theme changes its grouped colours (writing the file); the name
of a theme file is changed there too, and its file is renamed to match. Duplicate copies the
theme in use, Reload reads the files again, Open folder shows the themes folder, Back up writes
every theme file into `exports/suhina-themes-<date>.zip`, and Export template writes
`exports/suhina-theme-template.zip`: `template.json` (every token written out), `README.md`
(the format and every token, written so an AI agent can make a theme from it alone) and the
built-in themes in `examples/`. A theme file or a zip of them (a backup) dropped on the window is
added; a theme already there is not doubled, and one whose id is taken gets `-2`. A theme file
saved in another editor shows within a second.

## Squint test

Before a view ships, put it next to the same view in Live:

1. An Ableton user can operate it without help. **Familiar enough.**
2. At thumbnail size, nobody mistakes it for Live. **Different enough.**

If (2) fails, change colours, control shapes, typography and wording, not the layout.

## Sources

- Robert Henke on one window and no pop-ups, via
  [linux-audio-user 2003](https://ccrma.stanford.edu/mirrors/lalists/lau/2003/01/0328.html).
- [How we redesigned the Linear UI](https://linear.app/now/how-we-redesigned-the-linear-ui):
  LCH-based theme from base, accent and contrast; limited accent use.
- [Ardour: meter contrast](https://tracker.ardour.org/view.php?id=6529),
  [linux-audio-dev on dark themes](https://lists.linuxaudio.org/archives/linux-audio-dev/2022-June/038377.html),
  [MOD forum on knob readability](https://forum.mod.audio/t/plugin-artwork/7353/98).
- [Mycroft, QMUL: visual load and listening](https://qmro.qmul.ac.uk/xmlui/handle/123456789/44047?show=full).

## Local Studio appearance experiment

Studio Light and Studio Dark are opt-in themes using warm surfaces and gold accents.
Arrangement headers keep their track-color edge band on neutral, theme-editable surfaces
(`track.headerBackground`, `track.headerSelectedBackground`). Missing new tokens in older
theme files fall back to the built-in palette. The selected token supplies the base of selected headers before track-colour strength and the selection highlight are applied; selection retains an outline.
The larger transport and initial Arrangement dimensions are described in components.
These initial dimensions are shared implementation constants, not yet user-configurable
appearance metrics; the planned metrics settings remain follow-up work.

### Website-to-application direction

Use the Suhina website as the primary visual reference: warm paper/charcoal, muted clip
colors, a restrained gold accent, thin quiet grid lines and readable hierarchy. Preserve a
dense music workspace without making every field equally prominent. Glass is an optional
material, not the visual foundation; solid mode must look complete on its own. Grid strength
is independently editable through `grid.major` and `grid.minor`.

Public references reviewed on 20 October 2026: [Bitwig display profiles](https://www.bitwig.com/userguide/latest/a_musical_swiss_army_knife/),
[Live display and theme settings](https://www.ableton.com/en/manual/first-steps/), and
[FabFilter scaling](https://www.fabfilter.com/help/micro/using/scaling). These inform general
principles (adaptable density, separate color controls, usable scaling), not copied graphics,
text, widget shapes or layouts. No proprietary assets were downloaded into this repository.

### Default UI typeface

Manrope is the default proportional UI font, matching the website. The original Encode Sans
Semi Expanded asset and license remain embedded as a fallback and the named `encode-sans`
font family. This does not yet add a user-facing font selector (#513). Number fields retain
IBM Plex Mono. All bundled fonts load offline.


### Theme skins and native CSS

Themes also own SVG control artwork, continuous/segmented/needle meter presentation,
and optional control-bar module order/visibility. These travel in a portable theme ZIP,
including a theme's background PNG. The theme editor and the initial native CSS subset
share the same model, with live CSS/JSON file synchronization. See
[Theme skins](../spec/theme-skins.md) for supported declarations and failure handling.
The broader modern CSS target and candidate engines are recorded separately in
[ADR 0030](../decisions/0030-native-theme-css.md); this initial subset is not full CSS.
