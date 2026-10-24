---
description: The Settings window's pages and their headed sections, which setting is on which page, how a page scrolls, the search field, and the one table the window is drawn from
read_when: Settings window, settings page, where a setting goes, adding a setting, settings scroll, settings search, settings table, settings_rows, page order, General page, Appearance page
type: spec
status: current
last_verified: 2026-10-22
covers:
  - crates/ui/src/settings/settings_view.rs
  - crates/ui/src/settings/settings_rows.rs
  - crates/ui/src/settings.rs
  - crates/ui/tests/settings_window.rs
---

# The Settings window

Options › Settings (Ctrl+,) opens the Settings window over the main window. How it is drawn
(sizes, colours, the controls) is in [components](../design/components.md) › Settings; this spec
says what is on each page, how the pages scroll and how a setting finds its place.

## Sources

Suhina's own layout, shaped by public design guidance on settings screens (Apple's and Microsoft's
settings guidelines, and articles on settings UX) and by how three other programs arrange theirs,
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
| Projects and backups | Projects | When opening a project, At start, Projects folder, Templates folder, Start template (shown with At start › Template) |
| Projects and backups | Backups | Backups (the note names the newest backup of the active project), Clean up backups |
| Audio | Status | What plays now: output, latency, key to sound, input, List devices again (a panel, not a setting) |
| Audio | Output | Driver, Device, Sample rate, Buffer size (or Engine block), Exclusive mode, Input |
| Audio | Cue | Cue output, Cue volume |
| MIDI | MIDI inputs | Each MIDI input port, on or off |
| MIDI | Arming | Auto-arm the selected track |
| Plug-ins | Actions | Scan, Rescan all, Manage plug-ins (commands, set off above the folders) |
| Plug-ins | Folders | Folders (each on or off, with its format; add with a format, remove) |
| Plug-ins | Editors | Editors (Window, Device view) |
| Devices | NAM | NAM models folder, IR folder ([nam spec](nam-amp-and-cab.md) › Folders) |
| Devices | Device panels | Device chain, Detail area layout |
| Importing | Dropping files | Several files, Warp mode |
| Arrangement | Clip editing | Allow sample stacking, Allow MIDI stacking, Double-click makes a clip, Name field in clip menus, Moving a clip |
| Arrangement | Time and play | Ctrl+drag selects time over clips, Play from the selected clip, Play from, Stop |
| Arrangement | View | Grid in clips, Waveform zoom |
| Arrangement | Tracks | New track colour, Track headers, Return tracks |
| Arrangement | Advanced | Ctrl+Alt-drag on an edge stretches audio, Ctrl+R renames every selected clip |
| Session and mixer | Launching | Select on launch |
| Session and mixer | Mixer | Mixer controls |
| Clip editor | Piano roll | Pulled to the top, Piano roll grid, Toolbar size, Moved notes replace the notes under them |
| Browser | Browser | Browser side, Recent's kinds |
| Browser | Previews | Auto-play, Preview volume |
| Browser | Sample map | Map layout, Map preview on hover, Map: longest sample |
| Browser | Tags | Tag limit |
| Keys | Mouse and key behaviour | The clip editor's workflow settings (click, right click, double-click, duplicate key); kept in `keymap.json` |
| Keys | Shortcuts | The preset, every action's keys, Reset all |
| Appearance | Window | Zoom, Animations |
| Appearance | Waveforms | Waveform scale |
| Appearance | Advanced | Renderer |
| Theme | Theme | The themes, the theme in use, its colours |
| Agents and updates | Agents | Let agents work on open projects (off by default; [ADR 0028](../decisions/0028-mcp-in-the-running-app.md)) |
| Agents and updates | MCP write permissions | Allow the project's folder (on by default, with the active folder shown), MCP allowed write folders (Add folder… and Remove); all grants include subfolders |
| Agents and updates | Updates | Check for updates, Channel (Test builds only in developer mode) ([spec](updates.md)) |
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

## Scrolling

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
4. **One sentence of description**, saying what the setting does, shown whole. It holds no second
   sentence and does not end with a full stop; the window wraps it, so it may be long, but not
   two thoughts.
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
