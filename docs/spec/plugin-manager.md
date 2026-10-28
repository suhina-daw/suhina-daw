---
description: The plug-in manager and the plug-in folders: every plug-in and every file that gave none in one table, plug-ins switched on and off, failed files scanned again, folders per format and switched off, and the scan results that are settled
read_when: plug-in manager, manage plug-ins, switch a plug-in off, disable plug-in, blocklist, plug-in folders, VST2 folder, folder format, scan again, failed plug-ins, incompatible architecture, Intel plug-in on Apple silicon, plugin.setEnabled, plugin.setFolderEnabled, plugin.scanFile
type: spec
status: current
last_verified: 2026-10-24
covers:
  - crates/ui/src/settings/plugin_manager.rs
  - crates/ui/src/settings/settings_view.rs
  - crates/plugins/src/arch.rs
  - crates/plugins/src/library.rs
  - crates/plugins/src/scan.rs
  - crates/session/src/plugins.rs
---

# The plug-in manager

Settings › Plug-ins holds the folders scans look in and the scan itself; everything the scans
found is in the **plug-in manager**, opened with **Manage plug-ins** on that page. The manager
takes the Settings dialog's place until **Close** (or Escape) shows Settings again. What it
changes goes through the `plugin.*` commands, so an agent and `suhina-cli plugins` do the same.

## The table

One row per plug-in found, then one per file that failed or is built for another processor, in
one table (a file that loads and holds no plug-ins is not in it):

| Column | What it shows |
|---|---|
| **On** | A checkbox for a plug-in: on (the default) or switched off; greyed out while a scan runs, since the scan's result replaces the list. Files have none. |
| **Name** | The plug-in's name; for a file's row, the file's name. |
| **Vendor**, **Type**, **Version** | From the scan; the type is `instrument` or `effect`. |
| **Format** | `vst3` or `clap` (`vst2` once hosted); for a file, its extension. |
| **Status** | `on`, `switched off`, `failed` (in red) or `other processor`. |
| **File** | The plug-in's file; for a file that gave none, why (in red for a failure). |
| (action) | **Scan again** for a file that failed or is built for another processor. |

Each column heading sorts by it, again for the other direction (the name in natural order, the
others by their text, the name breaking ties). The search field above keeps the rows whose
name, vendor or file (or reason) hold every word typed. The counts beside the title say how
many plug-ins, how many switched off, how many files failed and how many are built for another
processor. **Scan** and **Rescan all** are there as on the Settings page; while a scan runs the
buttons wait and "Scanning…" shows.

## Switching a plug-in off

`plugin.setEnabled { plugin, enabled }` (the checkbox; `suhina-cli plugins disable <plug-in>`,
`enable`). A plug-in switched off stays in the catalog and in `plugin.list` (`enabled: false`),
and is kept off across starts (`plugin-disabled.json` in the settings folder). Sessions get the
catalog without it: the browser does not offer it, `device.addPlugin` and `track.add` do not find
it, and a project that uses it opens with the device missing, so it never loads. Switched on
again, it is offered at once and a project opened after finds it.

Several at once: a click on a row picks it (**Ctrl** or **Cmd** adds a row, **Shift** takes the
run from the last one picked), and the right-click menu has **Turn off** and **Turn on**. When
the right-clicked row is one of the picked ones, the menu acts on every picked plug-in the table
shows (the menu says how many, "Turn off 3 plug-ins"), in the order they were picked; a picked
plug-in the search hides, or a rescan removed, is left alone. When the row is not picked, the
right-click picks it and the menu acts on that plug-in alone. Each plug-in goes through
`plugin.setEnabled`; the first one that fails stops the rest and shows its error. The menu
entries wait while a scan runs, as the checkbox does.

## Files that gave no plug-ins

What a scan gives for a file is one of four (ADR 0006 › 9):

- **Plug-ins**: listed, and in the browser.
- **Failed** (crashed, hung, could not load, no factory): may be passing, so it is read once
  more in the same scan and later scans try it again (a hang at most once a day). Listed in red
  with the reason's first line; **Scan again** (`plugin.scanFile { file }`, `suhina-cli plugins
  scan-file <file>`) reads it again now.
- **Built for another processor**: the executable's header (Mach-O, PE or ELF) lists only other
  processors than the one Suhina runs on, such as an Intel-only plug-in on an Apple silicon Mac.
  It is never loaded and is settled: not read again until the file changes. Listed with
  `other processor` and the processors it is built for, apart from the failures
  (`plugin.list` › `incompatible`).
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

Each folder is for a format: a standard one for the format its name says (`…/VST3`, `…/CLAP`),
an added one for both CLAP and VST3 (**Both**, the default), **VST3** or **CLAP**, chosen beside
the field that adds it (`plugin.addFolder { folder, format }`, `suhina-cli plugins add-folder
<dir> --format vst3`). **Add** with nothing typed shows the system's folder picker instead, and
what it brings back goes in with the format the row has chosen; a folder typed with a leading
`~` is the one under the home folder. Adding a folder already there with another format changes
its format. A folder for one format gives only that format's files, so a folder of mixed
plug-ins can be split between formats.

**VST2** is a format already (shown, not choosable): VST2 plug-ins are not hosted yet, and a
VST2 plug-in is a plain library file that only its folder marks as one, so VST2 folders will be
added this way, as many as the user keeps, when VST2 hosting lands. `plugin.addFolder` refuses
`vst2` until then.

The folders file (`plugin-folders.json`) is `{ "added": [{ "path", "format" }], "off": [path] }`;
the earlier form, a list of paths, is read as folders for both formats.
