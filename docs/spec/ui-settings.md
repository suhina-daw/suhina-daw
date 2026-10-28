---
description: The Settings window's pages and their headed sections, which setting is on which page, how a page scrolls, the search field, and the one table the window is drawn from
read_when: Settings window, settings page, where a setting goes, adding a setting, settings scroll, settings search, settings table, settings_rows, page order, General page, Appearance page
type: spec
status: current
last_verified: 2026-10-25
covers:
  - crates/ui/src/settings/settings_view.rs
  - crates/ui/src/settings/gradient_editor.rs
  - crates/ui/src/settings/settings_rows.rs
  - crates/ui/src/settings/track_sizes.rs
  - crates/ui/src/settings.rs
  - crates/ui/tests/settings_window.rs
---

# The Settings window

Options › Settings (Ctrl+,) opens the Settings window over the main window. The window's status
bar stays visible under it, undimmed, so an error or a confirmation from the page is read while
it is open. How it is drawn (sizes, colours, the controls) is in
[components](../design/components.md) › Settings; this spec says what is on each page, how the
pages scroll and how a setting finds its place.

The title's Open in window button moves Settings into a separate native, resizable
window that can be moved to another monitor. Dock returns it to the main-window
overlay, preserving page, search and edited settings. Closing either form closes
Settings. The detached window never dims or blocks the main view. Its background
is solid: it must not sample the unrelated main-window backdrop.
The embedded panel can also be dragged by its title, within the main window.
In the overlay, dimming disappears during control adjustments and returns immediately
on release, without a delayed comparison interval. Background
clicks remain blocked. Only actual value changes or holding/dragging value controls
start that preview; background and navigation drags must not toggle the dimmer.
Text inputs and dropdowns use transparent fills and only a
theme-coloured underline, stronger on hover and accented on focus. Navigation rows
have no opaque background: only selected/hovered rows add a translucent tint.

## Sources

Suhina's own layout: no other program was observed for it. The grouping follows what the user is
doing when they look for a setting (setting up audio, editing in the piano roll, browsing
samples), not which part of the code reads it.

## Pages and sections

The list on the left shows the pages in this order. Each page has headed sections; every setting
is on exactly one page, under one heading.

| Page | Section | Settings |
|---|---|---|
| General | Projects | When opening a project, At start, Projects folder, Templates folder, Start template (shown with At start › Template) |
| General | NAM | NAM models folder, IR folder — where the two NAM devices' dropdowns list their files ([nam spec](nam-amp-and-cab.md) › Folders) |
| General | Backups | Backups, Clean up backups, Last backup (read only) |
| General | Agents | Let agents work on open projects (off by default; [ADR 0028](../decisions/0028-mcp-in-the-running-app.md)) |
| General | Dropping files | Several files, Warp mode |
| General | Updates | Check for updates, Channel (Test builds only in developer mode), Developer mode ([spec](updates.md)) |
| Export | MCP write permissions | Allow the project's folder (on by default, with the active folder shown), MCP allowed write folders (Add folder… and Remove); all grants include subfolders |
| Audio | Output | Driver, Device, Sample rate, Buffer size (or Engine block), Exclusive mode, Input |
| Audio | Cue | Cue output, Cue volume |
| Audio | Now | Output, Latency, Key to sound, Input now (read only), List devices again |
| MIDI | MIDI inputs | Each MIDI input port, On or Off |
| MIDI | Arming | Auto-arm the selected track, On or Off (Off by default) |
| Plug-ins | Editors | Editors (Window, Device view) |
| Plug-ins | Library | Folders (each on or off, with its format; add with a format, or by the system's picker with nothing typed, remove), Scan and Rescan all with a count of what was found, Plug-in manager (opens the [plug-in manager](plugin-manager.md)) |
| Arrangement | Clips | Allow sample stacking, Allow MIDI stacking, Waveform and note colour (Auto, Light, Dark, Custom), Custom waveform and note colour, Audio clip colour strength |
| Arrangement | Tracks | Track headers, Track header colour strength |
| Arrangement | Track sizes | Default track size, Default return size, Advanced track sizes, Tiny height, Compact height, Small height, Medium height, Large height |
| Session and mixer | Launching | Select on launch |
| Session and mixer | Mixer | Mixer controls |
| Clip editor | Piano roll | Pulled to the top, Piano roll grid, Toolbar size |
| Clip editor | Mouse | Left click draws, Right click deletes a note, Shift+right click splits, Alt+right plays from the point |
| Transport | Play and stop | Play from, Stop |
| Detail area | Devices | Device chain |
| Browser | Browser | Browser side |
| Browser | Previews | Auto-play, Preview volume |
| Browser | Sample map | Map layout, Map preview on hover, Map: longest sample |
| Browser | Tags | Tag limit |
| Appearance | Liquid (experimental) | Panel liquid, Track liquid, Clip liquid: independent 0–100% strengths, 0% off |
| Appearance | Window | Zoom, Animations |
| Appearance | Waveforms | Waveform scale |
| Appearance | Graphics | Renderer |
| Themes (under Appearance) | Theme | Two-column preview gallery; click applies immediately; Edit current theme opens grouped colours |
| Shortcuts | Keys | The preset, every action's keys, Reset all |

Where a setting is shown never changes how it is saved: each keeps the key it has in
`settings.json` (or `keymap.json` for keys and the mouse switches), so a settings file written
before a setting moved loads with the same values.

Export permissions apply to attached MCP writes only. The person adds a folder through the
system folder picker or removes its grant; the project-folder grant can also be switched off.
The settings persist across restarts. Missing folders grant no access, and each subsequent
operation uses the current grants. File › Export still writes to the destination the person
chooses. Agents cannot change these settings. Standalone MCP keeps its `--write-root` option.

## Theme gallery and colour editor

Themes has a search field beside *Other themes*: it shows the themes whose name, id or a
variant's name contains the text, ignoring case, and *No themes match* when none does; the theme in
use always shows. A theme left in an earlier file format says *Not converted* on its card, with
the reason when hovered. Themes shows a miniature musical scene for every built-in or custom palette. The scenes
use the same browser, MIDI/audio clips, grid, controls and meters; drawing them never
installs a theme or loads another project. The active card has an accent border and Active
label. The active theme is featured once at full width with a 170 px preview; other
themes remain in a two-column grid. Clicking a card immediately applies and saves the
selected theme and returns the gallery to the top. Holding the primary button on the
active card keeps the background clear, including stationary frames; release restores
the blur and dimming immediately.
More holds Duplicate, Reload, Open folder, Back up and Export template.

Restore built-in opens a choice of the original built-in palettes, in both gallery and
editor. Selecting one asks whether to save a new copy of the current theme first:
Save a copy & restore, Restore without a copy, or Cancel restore. Nothing changes until
that choice. Existing custom files are already saved by live editing and remain in the
library in either restore path; this is stated in the prompt. Restore never overwrites
a built-in or deletes the custom theme. A saved copy gets a free file name.

The four miniature lanes use the open project's first four track colours and their first
arrangement clips' explicit colour overrides. An unset colour follows the candidate theme's
MIDI or audio clip default. Clip fills use the Arrangement's 82% colour strength over the
candidate panel surface, rather than raw saturated swatches. A short gallery note explains
that existing tracks and clips keep their colours when selecting a theme. Previews never
recolour the project.

Edit current theme opens a child view, with Back to themes and the current theme name.
Its Liquid section defaults to Use global liquid settings. Turning that off copies the
current global Panel/Track/Clip strengths into independent theme overrides. The three
sliders then apply live and save with the theme; turning inheritance back on removes
the override. Settings edits are saved in a variant of the theme (Custom, made the first time
on the theme's own values), so a built-in theme is not copied for them. The renderer uses theme
values when present, otherwise global settings; GPU support, reduced effects and high contrast
remain final gates. A theme's `--liquid` property is `auto` or three integers from 0 to 100
(panels, tracks, clips); `auto` preserves global inheritance. Copies
and exports retain these values; Reset changes restores both entry colours and inheritance.
Colours are grouped into Surfaces, Text, Tracks & clips, Controls, Meters and Sample map.
Each colour has its swatch, hex and wrapping token on the first line, followed by the
full wrapping explanation on a separate line. No fixed token/description columns or
truncation hide the purpose or let long token names overlap it.
Tracks & clips exposes two defaults: Default MIDI clip (`clip.midiDefault`) and Default
audio clip (`clip.audioDefault`). The session copies these RGB colours only into newly created tracks and clips.
Opening a legacy project resolves its unset track colours from the existing numbered
palette once, without adding an undo step or marking the project dirty. Existing
tracks/clips never read the live creation defaults. Changes apply only to future
creations; explicit colours and copies of existing clip content remain unchanged. The old numbered
`track.1`–`track.8` swatches remain readable in theme files and in colour pickers/browser
icons for compatibility, but are hidden from this editor. Older theme files inherit their
MIDI default from `track.3` and audio default from `track.1`; explicit new tokens win.
Changes apply and save live into the variant in use; moving controls or changing artwork of a
built-in theme creates a custom copy. A theme with variants shows a row of swatches under its
card, the theme's own values first; clicking one uses that variant. More › New variant starts
one from the values in use, and with a variant in use the name field renames the variant.
*Theme settings* lists the settings the theme declares with `@property`, each with a control
for its type ([theme skins](theme-skins.md#theme-storage-one-sheet-variants-and-layout)).
Save as new theme duplicates the current palette and focuses its editable name. Reset
changes restores the palette captured when entering the editor, without changing the
built-in source. The name field reserves the same space for built-in and custom themes,
so making the first custom edit does not shift the picker.

An open colour picker holds the undimmed preview for the entire primary-button press,
including frames with no pointer motion or colour change. Release restores dimming on
the same frame. This also applies to the custom waveform colour picker.

Theme surface colours expose Opacity (0–100%) and the picker's alpha channel. Files
write opaque colours as `#RRGGBB`, translucent colours as `#RRGGBBAA` (alpha last).
The two clip creation defaults are RGB assignments, so they keep an opaque picker.
Premultiplied interpolation preserves transparency, including the liquid body gradient.
Opacity changes apply live and save with the theme; holding their slider previews the
background without dimming, as with the other colour controls.

Under the gallery, *Themes folder* (`themes_folder`) is where themes are kept and read from. It is
shown, not typed into: **Browse** chooses a folder and **Reset** goes back to `themes` in Suhina's
data. Either asks *Move the folder?* first; **Move** makes a backup of the current folder beside it
(`<name>-backup-<date and time>`), moves its themes into the new one (keeping what that has) and
reads the themes from there at once; **Cancel** changes nothing. `SUHINA_THEMES` wins over it.

General › Projects › *Settings folder* (`data_folder`) chooses Suhina's data folder: its
settings, keys, recent projects and themes. It is not written in the settings file, which lives
in that folder: `data-location.txt` in the default folder names it. It changes as *Themes folder*
does, through Browse or Reset and the same question; **Move** backs the files up beside the
current folder, moves them into the new one (keeping what it has) and uses it at once.
`--data-dir` and `SUHINA_DATA` win over it.

## Scrolling

Liquid panel refraction samples the current frame's captured backdrop through a bounded
edge lens. Texture coordinates bend smoothly near the perimeter and stay unchanged in
the centre; sampling is clamped to the captured screen. Text, inputs and musical marks
are painted after the lens. No prior frame or another window's pixels are sampled.
Reduced effects, unsupported renderers and zero strength retain the solid fallback.

Text inputs and closed selection fields throughout the native UI use the shared
transparent, underline-only treatment. Popup lists keep opaque theme surfaces and
selection highlights. Keyboard focus accents the underline; hit targets, text editing,
selection, commit/cancel behaviour and accessibility labels remain unchanged.

Each page scrolls when it is taller than the room under the title, at any window size: with the
mouse wheel over the page, by dragging its scroll bar on the right, and by moving the keyboard
focus (Tab) to a control out of view, which scrolls it into view. Opening another page shows it
from its top. A list that fills the space left on its page (the theme colours, the shortcuts, the
plug-ins found) is at least 160 px tall, so in a short window the page scrolls down to it, and
the list scrolls on its own.

## Search

A field above the list of pages, "Search settings", filters the settings by what is typed. A
setting matches when every word typed is in its name, its note, its section or its page (case
does not matter). While the field has text, the page shows only the matching settings, from
every page, under headings that name the page and section ("Browser › Previews"), and no page
in the list is lit; "No setting matches" when none does. Clicking a page empties the field and
shows that page; Escape in the field empties it (a second Escape closes Settings).

## The table

The window is drawn from one table, `ROWS` in `crates/ui/src/settings/settings_rows.rs`: an entry per
setting with its page, its section, its name, its note, the keys it is saved under, and how its
control is drawn. A page shows its entries in the table's order, starting a section where the
section changes. Most entries are one row (a name, a note and a segmented control or a field); a
few are a block that draws several rows or a list (the audio output, the themes, the shortcuts).
Adding a setting to the window is adding its entry under its section; moving one is changing its
page and section. Tests check that every saved setting is on exactly one page.

Named row sizes and their ordering, live application and persistence are specified in
[Arrangement editing](arrangement-editing.md#named-row-sizes). Pixel values are hidden until
Advanced track sizes is enabled; the same bounded controls remain editable in both modes.

Track sizes is an interdependent search group: a match on any of its settings shows the whole
section, so searching for the default also exposes Advanced and per-size tuning. Default changes
and tuning apply on the next frame, including while Settings is open. Changing a default replaces
height overrides for that kind of row (regular tracks or returns); preset tuning preserves manual
heights that do not match a named size.

## Experimental surface materials

Appearance › Liquid (experimental) has three independent percentage sliders: Panel liquid,
Track liquid and Clip liquid. All default to 0 (solid/flat), update the material on the next
frame and persist as `liquid_panel_strength`, `liquid_track_strength` and
`liquid_clip_strength`. Input is clamped to 100; invalid values preserve the default and
produce the usual settings warning. Older files leave all three off.

Panel liquid controls how much of the Settings backdrop shows through the theme tint.
Track liquid controls a curved directional rim on MIDI/audio track headers in both
views; Clip liquid controls the same coating on clip bodies. A source-coloured body
gradient supplies the broad reflection under the rim. Both effects decrease on smaller
surfaces. Labels, controls, notes and
waveforms are drawn afterward. The latter two are surface reflections, not per-clip blur.
The renderer is described in [Glass rendering experiment](../design/glass-rendering.md).

Controls remain editable and save their values even in a host without the GPU provider.
The theme editor explains when preview is unavailable; global controls also show this
explanation on hover. Selecting GPU in Renderer requires restarting the app if it is
currently running on OpenGL. Disabled animations or
the built-in high-contrast theme keep the material off without erasing saved strengths.
The feature remains experimental: full playback performance qualification is not complete.

The colour editor has a separate Browser text group: primary, secondary, hint and value.
These change browser text without recolouring the rest of the interface. Text on coloured
tracks, returns and clips uses `text.onColorLight` or `text.onColorDark`, whichever has
greater contrast with the surface, rather than reusing the window background as text.
Older theme files inherit these new roles from their previous text/background tokens.

### Surface gradients and content refraction

Edit current theme has independent Panel, Track and Clip gradient overlays. Each has
a toggle, clockwise direction (0 degrees left to right, 90 down), and 2–8 stops, each
with position 0–100, colour and opacity. Stops can be added and removed. Changes apply
live, save, duplicate, export and reset with the theme, without changing project colours.
Gradients work without GPU liquid; high contrast and reduced effects suppress materials.
An optional `gradients` object stores `panels`, `tracks`, `clips`; each enabled surface
has an `angle` integer 0–360 and `stops` array of `position` integer and RGBA `color`.
Missing surfaces remain unmodified. Invalid surfaces warn and fall back independently.

Track and clip refraction samples a shared current-frame substrate of visible coloured
surface bodies and their gradients, with a bounded edge lens. Foreground notes, labels,
waveforms and controls are excluded from this capture and painted sharply afterward.
Only one content capture is made per frame, independently of the panel backdrop; no
per-clip capture, previous-frame image or recursive sampling is allowed. Missing GPU
sources keep the optical gradient/rim fallback.

### Menu bar

Appearance > Menu bar in the window shows File, Edit, View, Projects, Options and Help under the
title bar. It is off by default on macOS, whose menu bar at the top of the screen has the same
menus and items, and on by default on Windows and Linux, where it is the only one. Keys work
whether it shows or not.

### Application bar and private background

Appearance > Application bar keeps Suhina visible, with Left or Centre placement.
Project name (including its unsaved marker) and project timing (tempo and time signature)
are optional and share a separate Left/Centre position. Long names are clipped inside
non-overlapping cells. On Windows this bar replaces native decorations; dragging, double-click
maximise, minimise, close and all eight edge/corner resize directions remain available.
Other platforms keep the system's title bar and show no application bar, so these settings change nothing there.

Appearance > Background accepts a user-owned PNG, limited to 32 MiB and 8192 pixels per
axis. Loading and image blur run away from the UI thread; the displayed image is reduced
to at most 1600 pixels per axis. The decoded image is cached separately from filtering. After a failed load, selecting the repaired file again with Browse retries it; ordinary frames do not retry failures.
Only one image job runs at a time; obsolete results are discarded before publication.
Background blur is measured in logical display pixels, converted to source pixels using
the cover scale, so resizing and different image sizes retain the requested visual strength.
Zero blur bypasses filtering. Background blur ranges from 0 to 40 pixels and Surface
opacity from 0 to 100 percent. Clearing the path removes the image. High Contrast does
not show it. While the image is displayed, neutral UI surfaces blend over it, including
on hosts that support desktop transparency. Surface opacity reveals the chosen image,
not other windows. Clearing the image restores the theme's desktop transparency on a
supported GPU host; OpenGL and unsupported compositors keep ordinary opaque rendering.
The image feature does not capture the desktop or discover its wallpaper.

Colour and gradient editors retain unmultiplied RGB at zero opacity, including after
saving and loading a theme. Returning opacity to 100 percent restores the original hue.

Suhina Glass is a bundled, editable blue glass preset. Its JSON includes all colour tokens,
subtle surface gradients and liquid overrides. A background image remains a local user
choice, so sharing the theme does not share an image or a machine-specific file path.

The background source, image processing and placement are separate internal modules.
They do not introduce an extension protocol; future animated sources must follow ADR 0015.
Settings uses its unattenuated theme surface as the liquid base, independently of the
main background surface opacity, to avoid multiplying two transparency controls.

Holding the Background blur slider keeps the Settings live preview active even while the pointer is stationary; the modal dimmer returns only on release, so individual value changes do not flash the surrounding view.

Global liquid strength sliders are disabled while the active theme supplies liquid overrides. Their notes explain how to enable Use global liquid settings in the theme editor. Stored global values remain unchanged and become editable again when inheritance is restored.

The gradient editor uses a single horizontal colour ramp with numbered draggable stops. Selecting a stop exposes only its colour, position and opacity; Add inserts and selects a midpoint in the largest gap, and Remove acts on the selected stop (at least two remain). Stops cannot cross their neighbours. Surface direction is a separate control below the stop editor. Colour-picker popups use an opaque readable surface even when Settings uses liquid transparency.
