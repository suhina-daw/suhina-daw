---
description: Backups (autosave) of open projects - when they are written (an interval or before critical operations), where (the project's .backup folder, the settings folder for a project never saved), clean-up, and the recovery offer on start after a crash
read_when: backup, backups, autosave, auto save, recovery, crash, lost work, .backup folder, project.autosave, critical operation, clean-up, power cut
type: spec
status: draft
last_verified: 2026-10-25
covers:
  - crates/session/src/backup.rs
  - crates/ui/src/app/project_dialog.rs
---

# Backups

Suhina writes a copy of each open project with unsaved changes now and then, so a crash, a power
cut or a frozen plug-in host loses at most the work since the last copy, not the work since the
last save. The next time Suhina starts it offers the copy back.

- **Sources:** the maintainer's decisions in
  [#202](https://github.com/suhina-daw/suhina-daw/issues/202) (2026-10-13). The details this
  document decides (the list of critical operations, the clean-up rule, the file names, the
  journal) are Suhina's own design.
- **Builds on:** saving ([ADR 0010](../decisions/0010-project-files.md), decisions 7 and 11).

## Terms

- **Backup:** a `.suhina` file holding the project as it was at one moment, with the plug-ins'
  state, written beside the project and never in place of it. The project keeps its own file.
- **Project file:** the `.suhina` file the project was opened from or last saved to.
  A project made new or from a template has none until it is saved.
- **Critical operation:** an action after which the edits before it are hard to get back, listed
  below.

## When a backup is written

A setting, **Backup interval** in Settings › Projects and backups, says when:

| Choice | Settings file (`backup =`) | A backup is written |
|---|---|---|
| Before critical operations | `critical` | just before each critical operation |
| Every minute | `1m` | every minute |
| Every 5 minutes (the default) | `5m` | every 5 minutes |
| Every 10, 15, 30 or 60 minutes | `10m`, `15m`, `30m`, `60m` | that often |
| Every 4 hours | `4h` | every 4 hours |
| Never | `never` | never |

- A backup is written only when the project has **changes since its last backup and since it was
  last saved** (`project.info`'s `unsaved_changes`). A project that is saved, or unchanged since
  the last backup, is not written again; an untouched project never gets one. Confirmed plug-in
  state changes have their own revision, so a change from processing or MIDI can require a
  first or later backup without a project command.
- Every open project is backed up, not only the active tab.
- With an interval, the clock runs from when the project was opened or last backed up; a project
  that has been changed is written when the interval has passed since then. Changes after that
  wait for the next interval.
- `project.autosave` with `now: true` writes a backup of the active project at once, whatever the
  setting, when it has unsaved changes.

### Critical operations

With *Before critical operations*, a backup of the project is written just before:

1. **Closing the project** (its tab's close button, `project.close`), or **opening another
   project in its place**; with unsaved changes this is before the Save, Don't save or Cancel
   question, so *Don't save* still leaves a backup.
2. **Quitting Suhina** with unsaved changes (each such project).
3. **Relinking missing files** (`project.relinkFiles`).
4. **Adding files** to the project (`clip.addFiles`: a sample, MIDI file or Stems file dropped
   or added from the browser).
5. **Loading a plug-in** (`device.addPlugin`): a plug-in that crashes or hangs its host while it
   loads is the case backups are most for.
6. **Deleting a track** (`track.delete`).

A command is critical when its spec says so (`policy: Policy::CRITICAL`, or `critical: true`;
`suhina_session::policy`); a new command that is hard to undo or risky sets it and is added to
this list.

## Where backups go

- A project with a project file: in the folder `.backup` beside that file, made when first
  needed. `Song.suhina` in `Music/` backs up to `Music/.backup/`.
- A Live Set imported and not yet saved as a Suhina project: in `.backup` beside the Live Set,
  and the Live Set counts as its project file below.
- A project with no project file yet: in `backups` in Suhina's settings folder (the folder
  `settings.txt` is in), until it is saved.
- Each backup is a file of its own, named with the project's name and the local date and time it
  was written: `Song 2026-10-15 14-05-09.suhina`. The time has dashes, not colons, so the name is
  allowed on every system. Two backups in the same second get ` (2)`, ` (3)` before `.suhina`.
  An untitled project's name is its tab's (`Untitled 2`).
- Audio paths in a backup are written relative to the backup's own folder, with the absolute
  path as well, as Save as does (ADR 0010 decision 6), so a backup opens with its audio.

## How a backup is written

- The project is copied with each plug-in's state as it was last read, without asking the
  plug-ins again: asking waited up to a second for a busy plug-in on the window's thread at every
  backup. Every running plug-in is read on background control workers every 30 seconds,
  including one whose editor has never opened: processing or MIDI may change state without a
  dirty notification. Two matching changed reads establish an unsaved change (ADR 0010). An ARA
  plug-in (Melodyne) is not read while its editor is open, since storing its edits held the editor
  up for seconds: its edits reach backups once the editor closes or the project is saved. A save
  still asks every running plug-in. The last completed state remains available while a control
  worker is reading a newer one, so a concurrent backup does not fall back to the opened state.
- The copy is turned into JSON and written on a background thread (`suhina-backup`), never on
  the window's thread or the audio thread. It is written to a temporary file beside the backup
  (`… .suhina.saving`) that is then renamed to the backup's name, so a crash while writing leaves
  either no backup or a whole one.
- A backup that cannot be written (a read-only folder, a full disk) is reported in the status bar
  and changes nothing else; the next one is tried at the next interval or critical operation.
- The status bar's right end says when the active project was last backed up ("backed up
  14:05"), and Settings › Projects and backups shows the last backup's time and file under the setting.
  `project.autosave` reports them too.

## Clean-up

A setting, **Delete old backups** in Settings › Projects and backups (`backup_clean_up = on|off`, on by default),
deletes old backups:

- After each backup is written, the newest **20** backups of that project in that folder are kept
  and older ones are deleted. "That project's" means files named `<project name> <date> <time>`
  (with an optional ` (n)`) and `.suhina`, in the folder the backup went to; nothing else in the
  folder is touched, and a backup of another project with a longer name that starts the same way
  is not counted.
- With clean-up off nothing is ever deleted from `.backup`.
- Saving a project that had no project file deletes the backups this run wrote for it in the
  settings folder: they are copies of a project that now has its own file and folder, and nothing
  else would ever remove them. Backups in a project's own `.backup` folder are kept when it is
  saved; they are its history, and only clean-up removes them.

## The journal

`backups/journal.json` in the settings folder lists, for each open project that has been backed
up, its newest backup and its project file (none for one never saved):

```json
{ "format": "suhina-backups", "version": 1,
  "backups": [ { "run": "1760530000123", "project": 2,
                 "file": "/home/me/Music/Song.suhina",
                 "backup": "/home/me/Music/.backup/Song 2026-10-15 14-05-09.suhina" } ] }
```

- `run` tells one run of Suhina from another; `project` is the open project's id in that run.
- Writing a backup puts or replaces its project's entry; closing the project (saved or not) and
  quitting Suhina normally remove the entry; saving it removes it too, since the project file is
  then the newest. The journal is written by the backup thread only, atomically, so its writes
  follow the backups in order.
- What remains in the journal at start is therefore what a run left behind when it did not end
  normally. The journal is read with limits (at most 1 MiB, at most 256 entries); a broken journal
  is ignored and started again.

## The offer on start

When Suhina starts, every entry left in the journal whose backup still exists and is **newer than
its project file** (or has no project file, or whose project file is gone) is offered, one at a
time, in a dialog titled **Backup found**: "Suhina closed before *Song* was saved. A backup from
2026-10-15 14:05 is newer than the project file." (for a project never saved: "… is all there is
of it."), with three buttons:

- **Open the backup**: opens it in a new tab as an unsaved project tied to the original project
  file: its tab and the window's title show the original's name with the unsaved mark, Save
  (Ctrl+S) writes the original file, and the backup itself is left as it is. A backup of a
  project that was never saved opens untitled, and Save asks for a name as for a new project.
- **Show the folder**: shows the backup in the file manager; the dialog stays.
- **Ignore**: the project is not opened; the backup is left in its folder. The dialog's Close
  button and Escape do the same; Enter opens the backup.

Opening or ignoring takes the entry out of the journal, so the same backup is not offered again.
An entry whose backup is gone or older than its project file is taken out without asking.

## Commands

`project.autosave` (runs on the open projects; never changes a project):

| Parameter | Type | Meaning |
|---|---|---|
| `now` | bool | `true` writes a backup of the active project now, if it has unsaved changes, and waits up to ten seconds for it |
| `interval` | text | sets when backups are written for this run: `critical`, `1m`, `5m`, `10m`, `15m`, `30m`, `60m`, `4h`, `never` |
| `cleanUp` | bool | sets clean-up for this run |

It returns `interval`, `clean_up`, the active project's `folder` (where its backups go, `null`
when there is none: an untitled project where no settings folder is known), `last_backup`
(`{ "path", "written" }`, the local time as `YYYY-MM-DD HH:MM:SS`, or `null`), `due` (whether it
has changes not yet backed up) and, after `now`, `written` (the path, or `null` when nothing
needed writing). The window keeps the two settings in `settings.txt` and gives them to the projects at start and
whenever they are changed in Settings; the CLI and the MCP server have no settings file, so what
`project.autosave` sets there lasts for the run. Over MCP, `now` needs the project's folder below
the write root and `interval: "critical"` is refused ([`mcp.md`](../mcp.md) › Backups).
