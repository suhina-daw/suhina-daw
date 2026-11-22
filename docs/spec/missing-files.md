---
description: What happens when a project's audio files cannot be found: the bar that says so, the panel that lists them and finds them by name (browser index, project folder, a chosen folder), and relinking in one undo step
read_when: missing files, missing samples, file not found, relink, locate, find files, moved samples, project.missingFiles, project.relinkFiles, library.findFiles
type: spec
status: draft
last_verified: 2026-11-23
covers:
  - crates/library/src/find.rs
  - crates/session/src/relink.rs
  - crates/ui/src/app/missing_files.rs
  - crates/ui/src/kit/table.rs
---

# Missing files

A project names the audio files its clips play. When a file has moved or been renamed since the
project was saved, it cannot be decoded and its clips are silent. Suhina says so when the
project opens and helps find the files again.

- **Sources:** the maintainer's request (2026-10-13); other DAWs have such a view (a list of missing
  files with an automatic search by name and a folder to search), known from their public
  manuals. The design below is Suhina's own.
- **Builds on:** the browser's file index ([ADR 0012](../decisions/0012-file-library.md)) and
  decoding files ahead of a command ([ADR 0011](../decisions/0011-preview-and-added-audio.md)).

## Terms

- **Missing file:** a clip or sampler source that cannot be resolved inside the project
  folder, or an explicitly loaded source whose file has since moved.
- **Candidate:** a file elsewhere with the same name (compared without regard to case), which
  may be the missing one.
- **Relink:** point every clip that names a missing file at another file.

## When a project opens

1. The project's audio is decoded as before, a sampler's inside a device group too; files not
   found are collected (with how many clips and samplers use each). A name several clips and
   samplers share is missing only when none of their paths is found.
2. If any are missing, a bar shows under the control bar: "4 audio files are missing" with
   **Find…** and **Later**. Later hides the bar for this project; View › **Missing files (4)…**
   opens the panel until nothing is missing.
3. The browser's index is searched at once for candidates (instant: it is a list of names in
   memory), so the panel usually opens with most files found.

During a format import, unresolved sample paths may be matched automatically against the
browser index before audio is loaded. A unique best trailing-path match is accepted;
equal-best matches, saturated candidate lists and missing files stay unresolved for the
panel. The search does not add roots, scan the disk or move audio; media trust rules apply.

## Zipped projects

A project can arrive as a zip that holds it and its samples, as another DAW packs a song to send
it. Opening or dropping such a zip (`.flp`, `.als` or `.suhina` inside, exactly one) does not open
it at once: a bar asks where to unpack it, with a folder named after the zip beside it filled in
(a number added when that folder is taken), **Unpack and open** and **Cancel**. The project then
opens from the unpacked folder, so its samples are found beside it by the rules above.
`suhina-cli` takes the folder as `--extract-to <folder>`; the MCP server refuses a zip, as an
agent should not write files outside its roots unasked. A zip with no project is a zip of themes
as before; one with several projects is refused with a message to unpack it by hand.

Unpacking treats the zip as someone else's file (`crates/session/src/zipped.rs`): an entry whose
name would leave the folder, a link and an entry beyond 10,000 are refused; the files may unpack
to 8 GiB at most, counted as they are written rather than as the zip states; a file already in the
folder is never written over (the unpack stops there with an error, keeping what it wrote).

## The panel

A dialog over the window, as wide as the window allows (at most 1180 px) and as high as it
needs (at most the window), drawn from the theme like every dialog: its box from `dialog[name="missing-files"]`, its
title bar from `dialog-title`, its rows from `list-row[kind="missing-file"]`, its words from
`label`. Escape or **Close** closes it; nothing changes until **Apply**.

**Title bar:** "Missing files", the project's name and how many files; **Search again** on the
right searches the browser's folders and the project's folder again (after the walk running,
when one is, even a walk of the project's folder), offers only the files still on disk, and
forgets the candidates no longer there. Whether a file is still there is looked at on another
thread, for Search again as for a walk's finds, so a network share that does not answer never
holds up the window. Search again looks at the disk first and then walks the project's folder,
so a file whose choice by hand has gone is searched for by the same click; a file chosen by
hand while the disk is looked at keeps that choice.

**Searching** (above the table):

- **Search a folder**: a field for a folder path, **Browse…** (the system's folder dialog) and
  **Search** (Enter in the field too); a folder dropped onto the panel is searched the same
  way. The folder is walked on a thread of its own, with **Stop** in Search's place while it
  runs and its file count in the bar at the bottom ("Searching D:\Samples: 12 400 files"). A
  folder asked for while one is walked waits its turn; Stop drops the ones waiting too. A
  search finds candidates only for files still missing; it never replaces a candidate the user
  chose. Opening another project stops a walk for the one before.
- The browser's index and the project's own folder are searched when the panel's list is made.
- **Filter files**: a field that keeps the rows whose name or old path holds every word typed,
  and **All**, **Missing** and **Found**, each with its count.

**The table**, one row per missing file:

| Column | Shows |
|---|---|
| (mark) | a dot in the state's color |
| File name | its name; its old path on hover |
| Used by | "38 clips", "1 sampler", "2 clips, 1 sampler" |
| Status | *Missing* (`label[kind="error"]`), *Found* (`ok`), *3 found* (`warning`: others of its name to choose from), *Chosen* (by hand, the accent) |
| Replacement | the file it will be linked to, cut in its middle so the drive and the name stay; whole on hover |
| | **Locate…** |

The name's, the uses' and the state's columns are resized by dragging the edge between the
headings (a double-click fits one to what it holds); the replacement takes the rest. A cut cell
ends with an ellipsis and shows its whole text on hover. The rows scroll down under headings that
stay, and sideways with them where the dialog is narrow. A click picks a row (Up and Down move
through the rows shown), a double-click is Locate…, and the right-click menu has **Copy file
name**, **Copy original path**, **Locate…** and, for a row with a replacement, **Show in
Explorer** (Finder on macOS) and **Leave it missing**.

**The selected file**, at the right (left out when the dialog is too narrow for it and the
table): its name, its state as a tag and what uses it; **Original location**, with **Copy file
name** (to paste into a file search tool) and **Copy path**; **Replacement**, with **Show in
Explorer**, and **Locate file…**; then the files of its name, best first, and *Leave it
missing*, a click choosing one. The panel scrolls when what it holds is taller than it.

**Locating a file by hand** gives the row its file, as the user's choice:

- **Locate…** opens the system's file dialog in the nearest folder of the old path still there.
- A file **dropped on a row** is that row's file, whatever its name; dropped on the panel at the
  right, the selected row's; dropped elsewhere, the missing file of its name. Several files
  dropped at once each find the file of their name.
- A path **pasted** (Ctrl+V while no field has the keys) is the selected row's file, unless it
  has another missing file's name and not the selected one's, then that file's; without the
  quotes a file manager's "copy as path" puts round it. A folder's path is searched. A file
  copied as a file (not its path) gives no text to paste.

A file located tells where the others went, at once:

1. **Beside it:** each other missing file of its folder is a candidate.
2. **Where its folder moved to:** the old and the new path less the folders and name they end
   in alike (without regard to case) give the folder that moved, `C:\Old` → `D:\Samples` for
   `C:\Old\Lib\Drums\kick.wav` found as `D:\Samples\Lib\Drums\kick.wav`; each other missing
   file that was under the old folder is looked for at the same place under the new one.
3. **Under it:** the located file's folder and its subfolders are walked, as a folder search, while
   files are still missing (after the walk running, when one is).

Rows chosen by hand keep their choice. The bar at the bottom says what was found ("Located
kick.wav, found 4 more beside it").

**The bar at the bottom:** a dot and "4 ready to relink · 15 still missing" (or the search
running), the last thing done under it; **Close** and **Apply N** on the right.

**Best candidate first:** the candidate whose folders end like the missing file's did (the most
folder names in common, counted from the file upwards) comes first; then the shortest path.
`Drums/Kicks/kick.wav` that was in `D:/Old/Drums/Kicks/` prefers `E:/New/Drums/Kicks/kick.wav`
over `E:/New/Other/kick.wav`.

**Apply** relinks every row that has a candidate, as one undo step, and closes the panel. The
files are decoded on another thread first (the window never waits), then the clips play them.
Rows with no candidate stay missing.

## What relinking changes

Sampler source paths participate in missing-file discovery and relinking alongside
audio regions, a sampler inside a device group's branch as one on the track. Relinking decodes the replacement before changing the source and
uses the same project undo step; the source remains silent while missing.

Every audio clip that names the missing file (by its relative or absolute path) is pointed at the
candidate: both its path and its absolute path become the candidate's absolute path, as for a
sample added from the browser. Nothing else about the clips changes (start, length, warp markers,
gain). Stems files are relinked the same way, each of their audio tracks with them. Saving the
project keeps the new paths in the file and trusts them on this machine, so the project opens
with them again, a file on a network share or a device path included: the rule that leaves such
a path unread is for files nobody chose ([Project file boundaries](#project-file-boundaries)).
Undo goes back to the old paths.

## Commands

| Command | Kind | Params | Does |
|---|---|---|---|
| `project.missingFiles` | read | | the files the project's audio clips and sampler sources name that are not found: `file` (as the clips name it), `lastPath` (where it was looked for), `clips`, `samplers` (inside device groups too) |
| `library.findFiles` | library | `names`, `folder?` | candidates for each file name: from the browser's index and the active project's folder, or, with `folder`, from walking that folder; each name with its paths |
| `project.relinkFiles` | edit | `files`, `to` | points the clips and sampler sources naming each of `files` at the file at the same place in `to`, one undo step; refused when a `to` file does not exist or cannot be decoded |

Over MCP, `folder` and every `to` path must be inside the server's read root ([`mcp.md`](../mcp.md)).

## Limits

| What | Limit | Beyond it |
|---|---|---|
| Walking a folder | depth 32, 1 000 000 entries, symbolic links not followed | the walk stops and says so |
| Candidates per file | 32, from the index and the project folder together, without repeats, best first | the rest are left out |
| Names in one `library.findFiles` | 4 096 | refused |
| Files in one `project.relinkFiles` | 4 096 | refused |

## Not decided yet

- Matching by content (length, sample rate) when names differ.
- Missing plug-ins are reported by the plug-in loading dialog (ADR 0006), not here.

## Collect and save

File › Collect and save copies the audio the project plays from outside its folder into
`Samples/Collected` in that folder, points every clip and sampler at the copies
(`project.collect`, one undo step) and saves. A copy of the same name and size already there
is used; another file of the same name gets a number, as `kick (2).wav`. A project not saved
yet asks for its name first and collects into the folder chosen. Afterwards the folder holds
everything the project plays, so it can be moved or shared as it is.

## Project file boundaries

On project open, audio clips and sampler sources are resolved inside the project
folder, without following symbolic links or junctions. Outside it, the desktop app reads a
file the project names wherever it is on a local disk (a drive letter on Windows), as the
person opened the project themselves; a network share or a device path is not read until
they choose it. The CLI and the MCP server read only what the person chose themselves. All
three open a project the same way (`Projects::open_path`, #764 › 3) and read their trust from the
same `browser.json`, so a project the app plays the CLI renders and an agent hears:

- **Files they loaded and saved a project with.** Saving a project keeps the absolute
  paths of the audio it has decoded (dropped, loaded, recorded or relinked) as trusted,
  in `browser.json` beside the settings (`trusted_audio`, newest first, at most 20,000).
  Any project that names one of them opens with it on this machine.
- **The browser's library folders**, the folders the person added; a path with a parent
  step out of one is not inside it.
- **The folders a project's audio came from.** Saving also keeps, for that project file, the
  folders outside it that its audio was loaded from (`project_folders` in `browser.json`, at
  most 64 a project). Opening the project reads them, and the missing files search looks in
  them as it does in the project's folder, so a sample moved inside the folder it was
  dropped from is found by name. They are kept on this machine, never in the project file:
  a project from someone else cannot name folders to read.

Anything else a project names outside its folder is treated as missing, decided from the
path alone before anything is read: in the desktop app a network share or a device path, in
the CLI and the MCP server any path not trusted. A project that comes from someone else (live
collaboration, when it arrives) is opened with the stricter rule too.
Choose such a file through Locate/Apply to use it; saving then trusts it. MCP paths stay
inside its configured read root.

The same rule holds after opening (#617). Saving, the automatic backup, Collect and save, the
trust a save keeps and the tempo warp reads all use the file that was decoded, as the session
noted it then: the one the rule above found when the project opened, or the one the person
loaded, relinked, recorded or consolidated in the session (`Session::loaded_path`). A file of
that name turning up in the project's folder later does not take its place. A path nothing was
decoded from, because it was not found or did not decode, is never looked at: saving moves its
relative path by name and keeps its absolute path as the project had it. So a project that names
a network share, or another file on the disk beside a sample in its folder, cannot get it read,
copied or trusted by being edited and saved. A NAM model or IR a project names is read only from
where its audio may be ([nam-amp-and-cab.md](nam-amp-and-cab.md)); one the person chose is read
wherever it is, and saving trusts the ones the project's devices name with the audio.

An importer follows the same boundary before anything is decoded: of the absolute paths a
foreign project file names it looks only at those on a local disk without parent steps
(`suhina_media::on_local_disk`), and it opens a sample (to read its duration, for example) only
once it has resolved it to a file inside the project folder, a local path that is there or a
library match permitted by the caller's media trust. Header reads use the same trust as
audio decoding, including when a CLI or MCP import has audio decoding disabled.
