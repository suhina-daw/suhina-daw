---
description: The Settings window's pages and their headed sections, which setting is on which page, how a page scrolls, the search field, and the one table the window is drawn from
read_when: Settings window, settings page, where a setting goes, adding a setting, settings scroll, settings search, settings table, settings_rows, page order, General page, Appearance page
type: spec
status: current
last_verified: 2026-10-29
covers:
  - crates/ui/src/settings/settings_view.rs
  - crates/ui/src/settings/gradient_editor.rs
  - crates/ui/src/settings/settings_rows.rs
  - crates/ui/src/settings/track_sizes.rs
  - crates/ui/src/settings.rs
  - crates/ui/tests/settings_window.rs
---

# The Settings window

Options › Settings (Ctrl+,) opens the Settings window over the main window. How it is drawn
(sizes, colours, the controls) is in [components](../design/components.md) › Settings; this spec
says what is on each page, how the pages scroll and how a setting finds its place.

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

### Choosing an audio driver and device

Each resolved host remembers its last applied device across application restarts,
including an explicit Default selection. The system-default host and the same host
chosen by name share this history. Choosing Driver restores an available remembered
device and applies the pair together, without opening an intermediate default.
For a new host or an unavailable remembered device, Driver only changes the devices
shown in the form. Device then shows “Choose a device…”; selecting a named device or explicitly
selecting Default applies the driver and device together with `audio.setOutput`.
Until then Now continues to describe the output still playing. Closing Settings
or selecting the active driver abandons the pending driver choice. Sample-rate
and buffer edits made while a driver is pending belong to that pending choice.
There are no preferred driver brands. A change that keeps the sample rate carries
existing plug-ins over; a different rate reconfigures the retained instances while
processing is stopped, without reloading their state or recreating their editors.

The active ASIO device's **Device settings → Control panel…** button sits directly
below Device, before Sample rate. It is absent for a pending selection or a device
other than the one currently running; it always addresses the retained active
ASIO instance. Exclusive mode is shown only for WASAPI. Audio status reports the
engine block and measured driver callback separately from total output latency;
internal driver buffers are configured through the native panel.

## Sources

Suhina's own layout, shaped by public design guidance on settings screens (Apple's and Microsoft's
settings guidelines, and articles on settings UX; the wording rule in *Adding a setting* follows [Microsoft's guidelines for app settings](https://learn.microsoft.com/windows/apps/design/app-settings/guidelines-for-app-settings) and [Toptal's settings UX article](https://www.toptal.com/designers/ux/settings-ux)) and by how three other programs arrange theirs,
all summarised in [#868](https://github.com/suhina-daw/suhina-daw/issues/868) in our own words: group by what the
user is doing, small groups, the most used first, no status or commands among settings, a change
applies at once. The grouping follows what the user is doing when they look for a setting (setting
up audio, editing clips, browsing samples), not which part of the code reads it.

## Pages and groups

The list on the left shows the pages in this order. Each page has headed groups; every setting is
on exactly one page, under one heading. A group that holds rarely changed settings is called
**Advanced**, comes last on its page and is folded until its heading is clicked (a search shows
what is inside it).

| Page | Group | Settings |
|---|---|---|
| Projects and backups | Projects | Open projects in, Startup project, Projects folder, Templates folder, Settings folder (used from the next start; changing it asks, backs up and moves the files), Startup template (shown with Startup project › Template) |
| Projects and backups | Backups | Backup interval (the note names the newest backup of the active project), Delete old backups |
| Audio | Status | Audio status: the output and input in use, how long sound takes to come out, dropouts, List devices again (a panel, not a setting) |
| Audio | Output | Audio device: Driver, Device, Sample rate, Buffer size (or Engine block), Exclusive mode, Input |
| Audio | Cue | Headphone output: Cue output, Cue volume |
| MIDI | MIDI inputs | Each MIDI input port, on or off |
| MIDI | Arming | Arm selected track |
| Plug-ins | Actions | Scan, Rescan all, Manage plug-ins (commands, set off above the folders) |
| Plug-ins | Folders | Folders (each on or off, with its format; add with a format, remove) |
| Plug-ins | Editors | Plug-in editors open in (Window, Device view) |
| Devices | NAM | Amp models folder, Cabinet sounds folder ([nam spec](nam-amp-and-cab.md) › Folders) |
| Devices | Device panels | Device chain layout, Detail area width |
| Importing | Dropping files | Tracks for dropped files, Warp mode for samples |
| Arrangement | Clip editing | Let audio clips overlap, Let MIDI clips overlap, Double-click makes a clip, Clip menu name field, Clip drag handle, Paste replaces clip |
| Arrangement | Time and play | Select time with, Start marker follows selection, Play from, Playhead after Stop |
| Arrangement | View | Grid inside clips, Waveform size, Waveform and note colour (Theme, Auto, Light, Dark, Custom), Custom waveform colour (shown with Custom), Audio clip colour strength |
| Arrangement | Tracks | New track colour, New clip colour, Track header colour strength, Track header side (Theme, Left, Right), Return tracks |
| Arrangement | Track sizes | Default track size, Default return size, Tiny height, Compact height, Small height, Medium height, Large height |
| Arrangement | Advanced | Edge drag stretches audio, Rename all selected clips, Exact track heights |
| Session and mixer | Launching | Select launched clip |
| Session and mixer | Mixer | Mixer control style |
| Clip editor | Piano roll | Full-height piano roll covers, Piano roll grid source, Piano roll toolbar size, Moved notes replace notes |
| Browser | Browser | Browser side, Recent item kinds layout |
| Browser | Previews | Preview selected file, Preview volume |
| Browser | Sample map | Sample map layout, Preview on map hover, Map sample length limit |
| Browser | Tags | Colour tag limit |
| Keys | Mouse and key behaviour | The clip editor's workflow settings (click, right click, double-click, duplicate key); kept in `keymap.json` |
| Keys | Shortcuts | The preset, every action's keys, Reset all |
| Appearance | Window | Window zoom, Animations, Menu bar in window |
| Appearance | Waveforms | Waveform scale |
| Appearance | Background | Background image, Surface opacity, Background blur |
| Appearance | Application bar | Application name position, Show project name, Show project timing, Project information position |
| Appearance | Advanced | Graphics renderer, Panel liquid, Track liquid, Clip liquid (experimental) |
| Themes | Theme | The themes, their variants, the theme in use and its settings; Edit current theme |
| Themes | Folder | Themes folder (changing it asks, backs up and moves the themes) |
| Agents and updates | Agents | AI agents edit projects (off by default; [ADR 0028](../decisions/0028-mcp-in-the-running-app.md)) |
| Agents and updates | MCP write permissions | Project folder for agents (on by default, with the active folder shown), More agent write folders (Add folder… and Remove); all grants include subfolders |
| Agents and updates | Updates | Check for updates, Update source (Test builds only in developer mode) ([spec](updates.md)) |
| Agents and updates | Advanced | Developer mode |

The Settings window opens on the page used last in the session. Descriptions are shown whole: they
wrap under the setting's name. A yes/no setting is a switch; a choice of up to five is a row of
option buttons, each as wide as its text needs; more than five is a drop-down.

Where a setting is shown never changes how it is saved: each keeps the key it has in
`settings.json` (or `keymap.json` for keys and the mouse switches), so a settings file written
before a setting moved loads with the same values.

Export permissions apply to attached MCP writes only. The person adds a folder through the
system folder picker or removes its grant; the project-folder grant can also be switched off.
The settings persist across restarts. Missing folders grant no access, and each subsequent
operation uses the current grants. File › Export still writes to the destination the person
chooses. Agents cannot change these settings. Standalone MCP keeps its `--write-root` option.

### Keeping playback through an output change

Changing the output or engine block restores the current playhead and keeps the play start
marker separate. If stretched audio is still being prepared on the new output, the pending
start accepts the restored playhead and launched clips before playback begins. Cancelling
that pending start clears the resume target; the next ordinary Play uses the marker.


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
shows that page; Escape in the field empties it (a second Escape closes Settings). Advanced groups are searched while folded.

## The table

The window is drawn from one table, `ROWS` in `crates/ui/src/settings/settings_rows.rs`: an entry per
setting with its page, its section, its name, its note, the keys it is saved under, and how its
control is drawn. A page shows its entries in the table's order, starting a section where the
section changes. Most entries are one row (a name, a note and a segmented control or a field); a
few are a block that draws several rows or a list (the audio output, the themes, the shortcuts).
Adding a setting to the window is adding its entry under its section; moving one is changing its
page and section. Tests check that every saved setting is on exactly one page.

## Adding a setting

The pages stay tidy only if each new setting follows these rules; a test in `settings_rows.rs`
checks the ones a machine can check and names this section when one fails.

1. **Pick the page by task**, from the table above: the page where someone doing that task would
   look. Do not add to a page because the code that reads the setting is near it. A new kind of task
   gets a new page, added to `Page::ALL` and to this table.
2. **Keep groups small:** at most seven entries in a group outside Advanced. Start a new group
   (with a heading that names the sub-task) rather than extending a full one. Rare or expert
   settings go under the page's **Advanced** group, last on the page.
3. **Settings only.** A readout of what is happening (latency, what is playing) is a status panel
   first on its page (`.status()`); buttons that do something once (Scan) are a row of actions
   first on its page (`.actions()`). Neither sits among the settings.
4. **Two columns, words a newcomer reads at once.** Names sit in the first column and every
   control starts at the second (`CONTROL_COLUMN` in `settings_view.rs`), so both run straight down
   the page; a row is its name and its control, with a second line only for what the setting
   reports now (the last backup). The wording follows (the Microsoft and Toptal guidance in Sources: plain words that
   say what a setting does, short, no jargon):
   - The **name** says exactly what the setting does, in one to four words, so that with its value it
     reads as one line (*Playhead after Stop: Marker*, *Arm selected track: on*, *Warp mode for
     samples: Rhythmic*). A noun for a choice, what it makes happen for a switch; not a sentence cut
     off before its buttons (*Grab a clip by*), not only the topic (*Moving clips*).
   - The **description** is one whole sentence that says what changes when the setting is on or
     chosen. How it is shown is chosen in **Help**, the button beside Close at the top of the window: as
     the name's tooltip (the default), as a ? after the name to point at, or under the name. The
     choice is kept as `setting_descriptions` (`tooltip`, `icon`, `shown`). It holds one thought: no semicolon, no second clause about the off state
     (the switch shows that), no shortcut keys (the Keys page lists them), and it does not end with
     a full stop.
   - **Plain words:** the effect, not the mechanism. Not *underruns* but dropouts, not *IR* but
     speaker cabinet recordings, not *MCP* but AI agent; a technical word may follow in the
     setting's options or help when someone needs it. No file paths into the docs, no ADR or issue
     numbers, no code names.
5. **One control per kind of choice:** a switch (`cx.on_off`) for yes/no, option buttons
   (`cx.choose`) for up to five choices that exclude each other, a drop-down beyond that
   (`cx.choose` switches by itself). No text is cut off.
6. **The stored key never changes** when a setting moves: it keeps its key in `settings.json`
   (`keymap.json` for keys and the mouse switches). A new setting gets a new key, listed in the
   entry's keys.
7. **Application settings apart from project settings.** What belongs to one project (its tempo,
   loop, a project's own defaults) lives with the project, not in Settings.
8. Update the table above and `docs/guide-data/settings-help.json`, and run
   `python scripts/settings_guide.py` for the website guide.

Named row sizes and their ordering, live application and persistence are specified in
[Arrangement editing](arrangement-editing.md#named-row-sizes). Pixel values are hidden until
Arrangement › Advanced › Exact track heights is on; the same bounded controls remain editable in both modes.

Track sizes is an interdependent search group: a match on any of its settings shows the whole
group and Exact track heights, so searching for the default also shows the per-size tuning. Default changes
and tuning apply on the next frame, including while Settings is open. Changing a default replaces
height overrides for that kind of row (regular tracks or returns); preset tuning preserves manual
heights that do not match a named size.

## Experimental surface materials

Appearance › Advanced has three experimental independent percentage sliders: Panel liquid,
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
explanation on hover. Selecting GPU in Graphics renderer requires restarting the app if it is
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

Appearance › Window › Menu bar in window shows File, Edit, View, Projects, Options and Help under the
title bar. It is off by default on macOS, whose menu bar at the top of the screen has the same
menus and items, and on by default on Windows and Linux, where it is the only one. Keys work
whether it shows or not.

### Application bar and private background

Appearance › Application bar keeps Suhina visible, with Left or Centre placement.
Project name (including its unsaved marker) and project timing (tempo and time signature)
are optional and share a separate Left/Centre position. Long names are clipped inside
non-overlapping cells. On Windows this bar replaces native decorations; dragging, double-click
maximise, minimise, close and all eight edge/corner resize directions remain available.
Other platforms keep the system's title bar and show no application bar, so these settings change nothing there.

Appearance › Background accepts a user-owned PNG, limited to 32 MiB and 8192 pixels per
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
