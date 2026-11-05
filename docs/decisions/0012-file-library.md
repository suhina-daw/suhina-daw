---
description: How the browser's folders are indexed, kept on disk and kept up to date (start-up check, file-system watching, polling), and the limits that protect it from hostile folders
read_when: file index, library index, folder scan, watching folders, notify, inotify, polling, network drive, browser.json, library crate, search speed
type: decision
status: accepted
built: yes
last_verified: 2026-11-02
covers:
  - crates/library/**
  - crates/session/src/library.rs
---

# 0012: The file library: an index on disk, watched

- **Date:** 2026-10-12

## Context

The browser ([spec](../spec/browser.md)) searches every file in the folders the user adds.
Sample libraries hold hundreds of thousands of files, sometimes on network drives; reading
them all on every start or every search is too slow, and a library that does not notice new
files (a sample pack unzipped, a bounce saved) is a nuisance. File-system notification
differs by system: Windows (`ReadDirectoryChangesW`) and macOS (FSEvents) watch a whole tree
with one handle per root; Linux (inotify) needs one watch per folder, limited per user
(`fs.inotify.max_user_watches`, 8 192 on older kernels, scaled with memory up to 1 048 576 on
newer ones) and may drop events when very many files change; network file systems may send no
events at all. Folders are user input and may be hostile: huge, deep, looping through links,
or full of odd names.

## Decision

1. **A crate of its own, `suhina-library` (layer 2, under `session`)**, owns the folders, the
   index, the pins, the recent items and the tags. `Projects` holds it, as it holds the plug-in
   library, and the `library.*` commands run on it. It depends on `media` only for what a file
   is (kinds by extension) and does no decoding of its own: durations, tempo and key are read
   on demand through `media` (`library.info`), never while indexing. Which installed
   plug-ins the browser's Plug-ins category lists (instruments or effects) is kept here too
   (`library.setPluginsView`, `plugins_view` in `browser.json`;
   [the plug-in browsing spec](../spec/plugin-browsing.md)).
2. **The index lists names, not contents.** Per file: its path below the folder, kind, size
   and modification time; per sub-folder: its modification time. Searching runs over this list
   in memory, on the calling thread up to 4096 files and on a thread of its own above that (a
   linear scan over at most 200 000 entries per folder takes milliseconds, which is still too
   long for every key on the window's thread).
   *Amended 2026-11-02, see Amendment: a large library's search.*
3. **Kept on disk**, in `library/index-<n>.tsv` beside `browser.json` (one file per folder,
   named by a hash of its path): a version line, the folder's path, then one tab-separated line
   per entry. A file that does not parse, is of an older version (an index written under the project's old
   name is still read) or larger than its limit is
   ignored and the folder is read again. It is written after every scan, to a file beside it
   that is moved over it.
4. **Brought up to date at start-up.** A background thread checks each folder against its saved
   index: a sub-folder whose modification time changed is read again (a folder's time changes
   when entries are added, removed or renamed in it), others are kept. The browser can search
   the saved index meanwhile; the folder shows *scanning* until the check is done.
5. **Watched while running**, with the `notify` crate (CC0-1.0) and its full debouncer
   (`notify-debouncer-full`, MIT or Apache-2.0), events gathered for 500 ms:
   - Windows and macOS: one recursive watch per folder.
   - Linux: a recursive watch (one inotify watch per sub-folder) only while the folder's
     sub-folders fit the budget: half of `fs.inotify.max_user_watches` shared by all folders.
    - An event re-reads only the sub-folder it names. An event's paths are matched against the
      folder's root as it was added and against that root with its symlinks resolved, because
      FSEvents (macOS) reports resolved paths (`/var` … as `/private/var` …) while inotify
      reports the paths as given. An event that says events were lost
      (rescan, overflow) re-reads the whole folder.
6. **Polling when watching cannot be trusted.** A folder is polled (every 5 minutes, the
   sub-folders' modification times compared with the index, as at start-up) when the watch
   cannot be made or fails later, when it is over the Linux budget, or when it is on a network
   file system (a `\\server\share` path on Windows; on Linux a mount of type `nfs`, `nfs4`,
   `cifs`, `smb3`, `smbfs`, `sshfs` or `fuse.sshfs` from `/proc/mounts`). `library.folders`
   reports each folder's state: `watching`, `polling`, `scanning`, `missing` or `idle` (not yet
   checked, before `Library::start`).
7. **Hostile folders are capped:** 256 folders, 200 000 files per folder, depth 32, names of at
   most 1 024 bytes, symbolic links not followed (so link loops cannot start), files of other
   kinds not listed. Reaching a cap stops that folder's scan with a note, never the program.
8. **Indexing starts only when asked** (`Library::start`), by the desktop app and the MCP
   server, so tests and the CLI never scan a user's folders by accident. Until then the
   library answers from its saved index and reads folders only when listed, or when `library.findFiles` walks one for missing files (at
   most 1 000 000 entries, depth 32).

## Consequences

- Search is instant on start, even for large libraries, and new files appear within a second on
  a local disk, within 5 minutes on a network drive (or at once with Refresh).
- Two new dependencies, both GPL-compatible.
- On Linux a folder too large for the watch budget is polled; raising the system limit
  (`sysctl fs.inotify.max_user_watches`) lets it be watched.
- Moving a folder that is in the library makes it *missing* until it is added again.
- A file moved through the browser (`library.moveTo`, #943) keeps what the library holds for
  it: its pin, its places among the recent items and its tags move to its new path, and the
  folders that held it and hold it now are read again at once, without waiting for the watch.
  A file moved outside Suhina loses them, as they are kept by path.

## Amendment: a large library's search (2026-11-02, #1111)

A search walked the whole list on the window's thread at every key, which stalled typing in a large
library. Above 4096 files it now runs on a thread of its own: the earlier results stay until it
answers, and the next key cancels it. Up to that the search stays on the calling thread, so a small
library answers in the same frame. What the browser does is in
[the browser spec](../spec/browser.md).

## Alternatives considered

- **Reading folders only when opened, no index:** no setup, but searching a library would mean
  walking it on every search.
- **Only polling:** works everywhere, but new files show up minutes late on local disks.
- **fanotify on Linux:** watches a whole file system without per-folder watches, but needs
  privileges (or a recent kernel with limits) Suhina should not ask for.
- **A database (SQLite):** faster to query with many folders, but a C dependency and more than
  this needs; the index is small enough to keep in memory.
