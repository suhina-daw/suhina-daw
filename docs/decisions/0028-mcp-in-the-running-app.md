---
description: Why the desktop app serves MCP for the projects it has open, how an agent reaches it (a local pipe or socket per build, bridged by suhina-cli mcp --attach), who may connect, what an attached agent may read, write and run, and why each build runs once
read_when: MCP in the app, attach, suhina-cli mcp --attach, live agent edits, agent and window share a project, shared undo, agent connection, allow agent, consent, named pipe, unix socket, control channel, single instance, one instance per build, --new-instance, open file in running app, agent roots
type: decision
status: current
last_verified: 2026-10-28
covers:
  - crates/automation/src/attach.rs
  - crates/automation/src/endpoint.rs
  - crates/automation/src/server.rs
  - crates/automation/src/workspace.rs
  - crates/ui/src/app/project_dialog.rs
  - apps/suhina/src/main.rs
  - apps/suhina-cli/src/main.rs
---

# 0028: MCP in the running app

- **Status:** accepted
- **Date:** 2026-10-17
- **Issue:** [#605](https://github.com/suhina-daw/suhina-daw/issues/605)

## Context

`suhina-cli mcp` is a process of its own. Its server (`crates/automation/src/server.rs`, `SuhinaMcp`)
holds its own `Projects` behind a mutex and opens `.suhina` files from disk into it; the desktop app
runs no MCP server. An agent and the window therefore never share a project: an edit over MCP is
invisible in the window until the agent saves and the person opens the file again, an edit in the
window is invisible to the agent, the two copies keep separate undo histories, and whichever saves
last overwrites the other.

Everything needed to share one project already exists. Every change is a command in the session's
registry, which the window, the CLI and MCP dispatch by id ([ADR 0002](0002-commands.md), which names
"UI, CLI, control channel and MCP server" as its front ends); a command records one undo step and
validates the project, whoever runs it. Each tab is a `Session` with its own undo history in memory
([ADR 0007](0007-several-open-projects.md)); nothing of the history is written to disk. What is
missing is a way for an MCP client to reach the `Projects` the running app holds, and the rules for
what it may do there. [SECURITY.md](../../SECURITY.md) puts "any control channel" in scope, and
[ADR 0023](0023-collaboration-and-project-versions.md) asks for a written threat model before
anything listens, so the rules are decided here, before the code.

## Decision

### 1. The app serves MCP for its open tabs, off by default

The desktop app can serve the same MCP tools as `suhina-cli mcp` (one `SuhinaMcp`, not a second
server) for the projects it has open. It is off until the person turns on **Settings › Agents and updates ›
Agents › AI agents edit projects**; with it off, nothing listens.

The tools run on the app's own `Projects`, on the thread that runs the window's commands: the
server hands each tool call to the window as a job, the window runs the jobs between frames, in order
with the person's own edits, and repaints. So an agent's edit appears in the window at once, and it is
one undo step in the history of the tab it acted on (decision 5).

### 2. One local endpoint per build, reached through `suhina-cli mcp --attach`

Clients are configured with a command on stdin/stdout, as for `suhina-cli mcp` today.
`suhina-cli mcp --attach` connects to the running app of **its own build** and copies stdin and
stdout to it and back, so a client changes only its arguments.

- **Endpoint names come from the build's folder.** Both executables of a build sit in one folder;
  the name of every endpoint is `suhina-` and a hash of that folder's canonical path (lower-cased on
  Windows). The CLI of a build therefore reaches that build's app and no other, and test builds of
  several pull requests, `main` and the nightly, each in a folder of its own, never meet.
- **Local only.** On Windows the endpoint is a named pipe (`\\.\pipe\suhina-<hash>-mcp`) that
  refuses remote clients. Elsewhere it is a Unix domain socket in a folder only the user can enter
  (0700): `suhina` in the session's runtime folder (`$XDG_RUNTIME_DIR`), or
  `~/.local/state/suhina/run` without one; the socket itself is 0600. There is no TCP port.
- `--attach` takes no `--read-root` or `--write-root`: the app decides the roots (decision 4). With
  no app of its build running, or with agent connections turned off, it says which and exits.

### 3. Who may connect: the current user, and only after the person allows it

- **The operating system decides who can open the endpoint: the user running Suhina.** The Unix
  socket's permissions allow only that user. A Windows pipe's default security lets only its creator,
  administrators and the system open it for writing; others may open it for reading only, which
  sends no request. Remote clients are refused.
- **Suhina cannot tell which program connects.** An MCP client's `clientInfo.name` is whatever it
  says, so no rule trusts a client for being a particular agent.
- **The person allows every connection.** At its first tool call a connection's calls wait while
  the window asks "*client name* wants to work on your open projects: Allow / Deny". Allow lasts as
  long as that connection does; Deny closes it. There is no "always allow", because a name can be
  faked.
- **The person sees and can end it.** While an agent is connected the control bar says so, with a
  **Disconnect** button. Turning the setting off disconnects every agent.

### 4. Files: the person's allowed folders to write, and the library to read

An attached agent works on the projects open as tabs, not on files of its choosing.

- **It writes inside the folders the person allows in Settings › Agents and updates**, including their
  subfolders. The saved project's folder is allowed by default, shown on that page, and can be
  switched off. Additional folders are added through a folder picker and can be removed. An
  unsaved project can write only when the person has granted an additional folder. With no
  effective grant, writes are refused. Grants persist in the existing settings file, cannot be
  changed through commands, and are checked again on subsequent operations and before a prepared
  export starts its worker. Removing a grant does not cancel a render already started.
- **It reads that folder and the library folders the browser shows**, so it can add the samples the
  person sees. Nothing else.
- **It cannot open other projects from disk**: `project_open` is refused over an attached
  connection. `project.list`, `project.switch` and the `project` argument act on the window's tabs.
- **It cannot change the person's preferences**: the commands that ADR 0002 already keeps from
  agents (the browser's and the plug-in scan's folders) stay refused, and so do commands that change
  the app's settings rather than a project (the audio and MIDI devices it opens).

### 5. Undo: each tab's own history

An agent's edit goes into the undo history of the tab it acts on, like an edit made in the window;
the person undoes it there with Ctrl+Z, and `edit.undo` over MCP walks the same history, so it can
undo the person's last edit as well. Nothing about history is added to the project file.

### 6. The agent may play and stop

Transport commands are allowed: the person is there and hears and sees everything. They act on the
window's own audio output, as the window's transport buttons do.

### 7. The agent never closes a tab

Closing a tab loses its unsaved changes and its undo history and cannot be undone, so
`project.close`, and anything else that closes a project, is refused over an attached connection.
Only the person closes tabs. When the person closes a tab, commands aimed at it return an error and
the connection stays open.

### 8. One instance per build

Starting a build's `suhina.exe` while the same build is running opens the given file as a tab in the
running app, brings its window forward and exits, so two copies of one project in two processes
cannot overwrite each other, and an agent has one app to reach. The lock is the build's own endpoint
(`suhina-<hash>`, decision 2), which the first app creates and holds while it runs; it carries only
"open these files" and "come forward", and exists whether or not agents may connect.

- Builds in different folders run side by side (decision 2).
- `suhina.exe --new-instance` skips the lock and starts another app anyway, for development (saving
  in one process and opening in another). It serves no agents, so an attached client can never be
  unsure which app it reaches.
- There is no setting for several instances: it would bring back the question of which one an agent
  reaches.

## Consequences

- Attached `project.export` commands gather plug-in states over window frames, using a bounded,
  caller-owned preparation round. Rendering, encoding and host cleanup run on a blocking worker
  after the snapshot has left the window. Batches containing exports await each step in order,
  releasing project access while the export runs; ordinary batches retain their existing path.
  Cancelling a request during preparation releases its gather. A tab switch cannot retarget the
  pending export, and a closed source project ends preparation with an error.

- Agents can work beside a person in the window, and the person keeps control: nothing listens until
  they turn it on, each connection waits for their answer, they see who is connected, and every edit
  is in their undo history.
- `SuhinaMcp` runs its tools through a host it is given: the mutex-guarded `Projects` of
  `suhina-cli mcp`, as now, or the window's job queue. The file rules become per call (decision 4)
  instead of the two roots given at launch, so `Workspace` takes several read roots.
- The app gains a tokio runtime on a thread of its own for the endpoints; nothing below the UI layer
  learns about windows or prompts, and nothing new is reachable from the audio callback.
- Starting the same build twice no longer opens a second window. `--new-instance` is the way to get
  one.
- All builds still share Suhina's settings folder (settings, the plug-in catalog, backups), as they
  do today; this decision does not change that.
- `docs/mcp.md` describes the attached mode, and SECURITY.md the endpoint and its rules.

## Alternatives considered

- **A TCP port on localhost.** Any process of any user on the machine can connect, and so can a
  web page through the browser; a pipe or socket keeps it to the user. Rejected.
- **A token in a file that the client must present.** Every process of the user can read the file,
  so it adds nothing over the endpoint's own permissions, and it is one more thing to set up.
  Rejected; the person's Allow is the check that a process of theirs cannot fake.
- **Trusting known clients by name, or "always allow".** The name is self-reported. Rejected.
- **Letting `suhina-cli mcp` open the same file and merge on save.** Two histories and a merge is
  the collaboration problem of ADR 0023, not a shortcut to it. Rejected.
