---
description: What happens when a project's audio files cannot be found: the bar that says so, the panel that lists them and finds them by name (browser index, project folder, a chosen folder), and relinking in one undo step
read_when: missing files, missing samples, file not found, relink, locate, find files, moved samples, project.missingFiles, project.relinkFiles, library.findFiles
type: spec
status: draft
last_verified: 2026-10-13
covers:
  - crates/library/src/find.rs
  - crates/session/src/relink.rs
  - crates/ui/src/app/missing_files.rs
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

1. The project's audio is decoded as before; files not found are collected (with how many clips
   use each).
2. If any are missing, a bar shows under the control bar: "4 audio files are missing" with
   **Find…** and **Later**. Later hides the bar for this project; View › **Missing files (4)…**
   opens the panel until nothing is missing.
3. The browser's index is searched at once for candidates (instant: it is a list of names in
   memory), so the panel usually opens with most files found.

## The panel

A dialog over the window, 600 px wide, as high as it needs (at most the window), titled
"Missing files · <project>". Escape or **Close** closes it; nothing changes until **Apply**.

**Rows**, one per missing file. Each row shows three lines:

1. A dot for its state (red *missing*, accent *found*, `text.value` *several found*; the state
   is also the name's tooltip), the file name, how many clips use it, and at the right a
   **Locate** field for a file path: Enter takes it as the candidate.
2. The path it had (hint text).
3. "→" and the candidate it will be linked to: a drop-down of the candidates, best first
   (below), and *Leave it missing*.

**Searching** (buttons above the rows):

- **Search the browser's folders**: runs by itself when the panel opens, again on click.
- **Search a folder**: a field for a folder path (or a folder dropped onto the panel) and
  **Search**. The folder is walked on a thread of its own, its file count shown as it goes
  ("Searched 12 400 files"), with **Stop**. A search finds candidates only for files still
  missing; it never replaces a candidate the user chose.
- The project's own folder is searched with the browser's folders.

**Best candidate first:** the candidate whose folders end like the missing file's did (the most
folder names in common, counted from the file upwards) comes first; then the shortest path.
`Drums/Kicks/kick.wav` that was in `D:/Old/Drums/Kicks/` prefers `E:/New/Drums/Kicks/kick.wav`
over `E:/New/Other/kick.wav`.

**Apply** relinks every row that has a candidate, as one undo step, and closes the panel. The
files are decoded on another thread first (the window never waits), then the clips play them.
Rows with no candidate stay missing.

## What relinking changes

Sampler source paths participate in missing-file discovery and relinking alongside
audio regions. Relinking decodes the replacement before changing the source and
uses the same project undo step; the source remains silent while missing.

Every audio clip that names the missing file (by its relative or absolute path) is pointed at the
candidate: both its path and its absolute path become the candidate's absolute path, as for a
sample added from the browser. Nothing else about the clips changes (start, length, warp markers,
gain). Stems files are relinked the same way, each of their audio tracks with them. Saving the
project keeps the new paths in the file. External paths are reported missing on
reopen, so an external relink lasts only for the current session. Undo goes back
to the old paths.

## Commands

| Command | Kind | Params | Does |
|---|---|---|---|
| `project.missingFiles` | read | | the files the project's audio clips and sampler sources name that are not found: `file` (as the clips name it), `lastPath` (where it was looked for), `clips` |
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
- Collecting a project's files into its folder ("collect and save").
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
folder, without following symbolic links or junctions. Outside it, the app reads only
what the person chose themselves:

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

Anything else a project names outside its folder (another person's paths, a network
share) is treated as missing, decided from the path alone before anything is read.
Choose such a file through Locate/Apply to use it; saving then trusts it. The CLI and
the MCP server open projects with the folder rule alone; MCP paths stay inside its
configured read root.
