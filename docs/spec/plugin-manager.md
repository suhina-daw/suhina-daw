---
description: The plug-in manager and the plug-in folders: every plug-in and every file that gave none in one table, plug-ins switched on and off, failed files scanned again, folders per format and switched off, and the scan results that are settled
read_when: plug-in manager, owned plug-ins, mark as owned, plugin.setOwned, manage plug-ins, switch a plug-in off, disable plug-in, blocklist, plug-in folders, VST2 folder, folder format, scan again, failed plug-ins, incompatible architecture, Intel plug-in on Apple silicon, plugin.setEnabled, plugin.setFolderEnabled, plugin.scanFile
type: spec
status: current
last_verified: 2026-11-23
covers:
  - crates/ui/src/settings/plugin_manager.rs
  - crates/ui/src/settings/plugin_manager_tests.rs
  - crates/ui/src/settings/settings_view.rs
  - crates/plugins/src/arch.rs
  - crates/plugins/src/library.rs
  - crates/plugins/src/scan.rs
  - crates/session/src/plugins.rs
---

# The plug-in manager

Settings › Plug-ins holds the folders scans look in and the scan itself; everything the scans
found is in the **plug-in manager**, opened with **Manage plug-ins** on that page. The manager
takes the Settings dialog's place, larger than it where the window has room, until **Close**
(or Escape) shows Settings again. What it changes goes through the `plugin.*` commands, so an
agent and `suhina-cli plugins` do the same. Every color, font and surface in it comes from the
current theme: switching the theme changes the manager with the rest of the app.

## Layout

- **Top bar:** the title and how many plug-ins were found on the left; **Scan for new** (drawn
  as the page's main button, in the theme's accent), **Rescan all** and **Close** on the right.
  While a scan runs the scan buttons wait.
- **Library sidebar** on the left: **All** (every row), **Plug-ins** (the plug-ins, no files),
  **Enabled**, **Disabled**, **Owned** (the
  plug-ins marked as owned, below) and **Needs attention** (the files that failed and those built for another processor), each with how
  many rows it holds; under **Needs attention**, **Failed** and **Other processor** apart. Below,
  the filters: **Type** (all types, effects, instruments), **Format** (CLAP, VST3, VST2
  checkboxes; none ticked shows every format, a file's format comes from its extension and a
  library file counts as VST2) and **Vendor** (a drop-down of every vendor found). A type or a
  vendor leaves the files out, since they have neither. At the bottom **Scan locations**, which
  shows the folders scans look in in the table's place (below), and **Reset filters**, which
  shows every row again (the search too). The
  counts beside the shelves follow the type, format and vendor filters and the search, so a
  shelf never says more rows than it shows.
- **In the middle:** the search field; under it, when files failed or are built for another
  processor, a notice that says how many ("6 files failed to scan · 6 use an incompatible
  architecture") with **Review issues**, which shows **Needs attention** and lets go of the type,
  format, vendor and search filters, which files have nothing for; then the table. Escape closes
  the manager, except while it closes an open menu or drop-down.
- **Scan locations**, in the middle in the table's place: the folder list of Settings ›
  Plug-ins (below, *Folders*), the same list with the same controls: each folder on or off, an
  added one removed, a folder added with its format, **Add** with nothing typed showing the
  system's folder picker. **Back to the plug-ins**, Escape, or any shelf or type in the sidebar
  shows the table again.
- **Selected plug-in** panel on the right: the row clicked last while it is picked (else the one
  picked last; nothing with no row picked), with its name and vendor (a file's folder), its
  format and type as tags, **Enabled** and **Owned** switches, its version, its architecture (a
  plug-in that loaded is built for the processor Suhina runs on; a file built for another names
  its processors in its reason) and status,
  the file's location with **Show in folder**, and **Rescan plug-in** (**Scan again** for a file),
  which reads that file again now (`plugin.scanFile`). A plug-in switched off is hidden from the
  browser, which the panel says. With several rows picked, the panel says how many ("14
  selected", so many plug-ins and so many files), has one **Enabled** switch for every picked
  plug-in (on while every one is on, and a click switches them all off; otherwise off, and a
  click switches on the ones that are off, as `plugin.setEnabled` each), **Scan N files again**
  for the picked files, and the list of what is picked with each one's state.
- **Status bar:** how many plug-ins, how many rows the filters show when not all, and how many
  are picked on the left; whether a scan runs on the right.

A dialog too narrow for all three drops the selected plug-in's panel first, then the sidebar.

## The table

One row per plug-in found, then one per file that failed or is built for another processor, in
one table (a file that loads and holds no plug-ins is not in it):

| Column | What it shows |
|---|---|
| (checkbox) | A plug-in's switch: on (the default) or switched off; greyed out while a scan runs, since the scan's result replaces the list. Files have none. The heading's checkbox picks every row shown, or none. |
| **Name** | The plug-in's name; for a file's row, the file's name. |
| **Vendor**, **Type**, **Version** | From the scan; the type is *Instrument* or *Effect*. |
| **Format** | VST3, CLAP or VST2 (VST2 where the host's `vst2` feature is on); for a file, from its extension. |
| **Status** | A mark and a word: *Ready*, *Disabled*, *Failed* or *Wrong architecture* (the two last with a warning mark, in the theme's recording and warning colors). |
| **File** | The plug-in's file; for a file that gave none, why. It takes the width the other columns leave. |
| (action) | **Scan again** for a file that failed or is built for another processor; the column is there only while such a row is shown. |

Each column heading sorts by it, again for the other direction (the name in natural order, the
others by their text, the name breaking ties). The edge between two headings, from **Name** to
**Status**, is dragged to make the column on its left wider or narrower, with the resize pointer
over it and no column narrower than its least width; a double-click on the edge fits the column
to its heading and the rows the table shows. The widths are kept in the UI settings
(`plugin_manager_columns` in `settings.txt`), so the manager opens with them again, after a
restart too. When the columns need more room than the dialog
gives (the file column gets at least room for a path), the table scrolls sideways, its headings
with its rows. **Columns**, beside the search, and a right-click on the headings show or hide
every column but the checkbox, the name and the action (**Show every column** brings them all
back); the hidden ones are kept in the UI settings too (`plugin_manager_hidden`), and a hidden
**File** leaves its room to the name. The action column
(**Scan again**) sits before **File**, beside the status, so it is in view without scrolling.
The rows scroll down and sideways in one area with its scroll bars at the table's edges, and the
headings follow the sideways scroll. Text a cell cuts ends with an ellipsis, and the pointer over the cell shows the whole
text (the whole path, the whole reason); a file row's name shows the file's whole path.

The search field keeps the rows whose name, vendor, file or reason hold every word typed, within
what the sidebar shows. A plug-in switched off is drawn greyed out, and the row picked last has
a mark in the theme's accent on its left.

Each file row's **Scan again** button has an id of its own, made from the file's whole path, and
a label that names the file and its folder ("Scan again d3dcompiler_47.dll in C:\Plugins\VST"),
so two files of the same name in different folders get a button each and a click reaches its own
row (#1461).

## Switching a plug-in off

`plugin.setEnabled { plugin, enabled }`, or `{ plugins: [...], enabled }` for several at once with one save (the checkbox; `suhina-cli plugins disable <plug-in>`,
`enable`). A plug-in switched off stays in the catalog and in `plugin.list` (`enabled: false`),
and is kept off across starts (`plugin-disabled.json` in the settings folder). Sessions get the
catalog without it: the browser does not offer it, `device.addPlugin` and `track.add` do not find
it, and a project that uses it opens with the device missing, so it never loads. Switched on
again, it is offered at once and a project opened after finds it.

Several at once: a click on a row picks it (**Ctrl** or **Cmd** adds a row, **Shift** takes the
run from the last one picked), and a plug-in's right-click menu has **Turn off** for the ones
switched on and **Turn on** for the ones switched off, each saying how many it acts on and
missing where there are none (with every picked plug-in off, only **Turn on**). When
the right-clicked row is one of the picked ones, the menu acts on every picked plug-in the table
shows (the menu says how many, "Turn off 3 plug-ins"), in the order they were picked; a picked
plug-in the search hides, or a rescan removed, is left alone. When the row is not picked, the
right-click picks it and the menu acts on that plug-in alone. They go through one
`plugin.setEnabled` with the list, saved once; a name not found changes none of them and shows its error. The menu
entries wait while a scan runs, as the checkbox does.

## Marking a plug-in as owned

`plugin.setOwned { plugin, owned }`, or `{ plugins: [...], owned }` for several saved at once
(`suhina-cli plugins own <plug-in>`, `disown`). A vendor's
installer often puts every plug-in of a bundle on the system, the ones the user bought and the
rest, or keeps those of a subscription that ended. The user marks their own: a mark only, which
changes nothing a session offers or loads, kept across starts (`plugin-owned.json` in the
settings folder) and listed by `plugin.list` (`owned`). A mark names the product, its vendor and
name, not one file of it: a plug-in installed both as CLAP and as VST3 is owned in both formats,
so **Turn off the rest** never switches off one of the user's own, and a mark outlives an update
that moves the file. A mark of a plug-in no longer installed is taken away by its name
(`disown <name>`).

In the manager a plug-in marked as owned has a ring in the theme's accent before its name, and
the **Owned** shelf lists them. A plug-in's right-click menu has **Mark as owned** for the picked
plug-ins not marked and **Unmark as owned** for those marked (each saying how many, missing
where there are none), and **Turn off the rest of** its vendor ("Turn off the rest of Alpha
(712)"), which switches off every plug-in of that vendor that is on and not marked; it is there
only once at least one of the vendor's plug-ins is marked, so a vendor nobody marked is never
switched off whole. **Owned first**, beside the search, lists the marked plug-ins above the rest, each
group in the sort chosen (by name in natural order, for one), and is kept in the UI settings
(`plugin_manager_owned_first`). The selected plug-in's panel has an **Owned** switch, and with several picked
one switch marks them all (or, when all are marked, takes every mark away).

## Files that gave no plug-ins

What a scan gives for a file is one of four (ADR 0006 › 9):

- **Plug-ins**: listed, and in the browser.
- **Failed** (crashed, hung, could not load, no factory): may be passing, so it is read once
  more in the same scan and later scans try it again (a hang at most once a day). Listed as
  *Failed* with the reason; **Scan again** (`plugin.scanFile { file }`, `suhina-cli plugins
  scan-file <file>`) reads it again now. A file row's right-click menu has **Scan again**, for
  every picked file row when the row is one of them ("Scan 3 files again"), and **Show in
  Explorer** (**Show in Finder** on macOS, **Show in folder** elsewhere), which opens the system's
  file manager at the file, picked in its folder on Windows and macOS (on Linux the folder
  opens); a plug-in's menu has it too.
- **Built for another processor**: the executable's header (Mach-O, PE or ELF) lists only other
  processors than the one Suhina runs on, such as an Intel-only plug-in on an Apple silicon Mac.
  It is never loaded and is settled: not read again until the file changes. Listed as *Wrong
  architecture* with the processors it is built for, apart from the failures
  (`plugin.list` › `incompatible`).
  A 32-bit Windows plug-in is not one of them when the 32-bit host is installed beside
  Suhina: it is scanned and loaded through that host, and one settled here before is read
  again ([plug-in hosting › 32-bit plug-ins on Windows](plugin-hosting.md#32-bit-plug-ins-on-windows)).
- **No plug-ins**: the file loads and holds no audio plug-ins (a vendor's shell bundle with no
  classes). Settled and not in the table; `plugin.list` with `ignored: true` lists it.

## Folders

The Settings page lists every folder scans may look in: the system's standard ones (marked
standard) and the ones added. Each has a checkbox: a folder switched off is not scanned, its
plug-ins leave the list with the next scan, and it stays listed to switch on again
(`plugin.setFolderEnabled { folder, enabled }`, `suhina-cli plugins folder-off <dir>`,
`folder-on`). An added one has **Remove**. While a scan runs, its checkbox is greyed out, and
adding, removing or switching a folder is refused with a message to wait: the scan's result
replaces the library, so a change made meanwhile would be lost.

Each folder is for a format: a standard one for the format its name says (`…/VST3`, `…/CLAP`,
`…/VST`), an added one for every format Suhina hosts (**All**, the default), **VST3**,
**CLAP** or **VST2**, chosen beside the field that adds it (`plugin.addFolder { folder,
format }`, `suhina-cli plugins add-folder <dir> --format vst2`). **Add** with nothing typed
shows the system's folder picker instead, and what it brings back goes in with the format
the row has chosen; a folder typed as `~` or starting `~/` is the one under the home folder.
Adding a folder already there with another format changes its format. A folder for one
format gives only that format's files, so a folder of mixed plug-ins can be split between
formats.

**VST2** ([ADR 0022](../decisions/0022-vst2-interface-description.md)): a VST2 plug-in is a
`.vst` bundle on macOS, and on Windows and Linux a plain library file, a `.dll` or a `.so`, that
only its folder marks as one. Loading a library runs its code, so a folder for every format takes
the bundles and no libraries: a VST2 folder is added with **VST2**, as many as the user keeps. The
system's VST folders are standard ones for that format. A build without the host's `vst2` feature
lists a VST2 file it finds as failed.

The folders file (`plugin-folders.json`) is `{ "added": [{ "path", "format" }], "off": [path] }`;
the earlier form, a list of paths, is read as folders for both formats.
