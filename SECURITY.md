# Security Policy

Suhina parses project files, audio files and plug-in state, hosts third-party plug-ins and
will expose CLI and MCP automation. Treat files, paths, plug-in data, automation requests and
command parameters as potentially malicious.

## Reporting a vulnerability

Do not open a public issue containing a working exploit, a weaponized file or secrets. Open an
issue with a minimal, non-exploitable summary and ask the maintainers for a private channel, or
use GitHub's private vulnerability reporting if it is enabled for the repository.

A useful report includes the affected version or commit, operating system and architecture,
the affected component, exact reproduction steps, expected and actual behaviour, and any
crash log or backtrace (with personal paths and data removed).

## Scope

- Project, clip, preset and audio file parsing (decompression bombs, oversized sizes, integer
  overflow, recursion, hangs).
- Plug-in hosting and plug-in state handling.
- Import/export path handling (traversal, symlinks, unsafe overwrites).
- The CLI, MCP server and any control channel.

## The app's agent endpoint

The desktop app can serve MCP for its open projects
([ADR 0028](docs/decisions/0028-mcp-in-the-running-app.md)). What it relies on, so a report can say
which part failed:

- It is off by default (Settings › Agents and updates › Agents) and nothing listens until it is turned on.
- The endpoint is local only: a named pipe that refuses remote clients on Windows, a Unix domain
  socket with 0600 permissions in a 0700 folder elsewhere; no network port. Only the user running
  Suhina (and, on Windows, administrators and the system) can open it for writing.
- Every connection waits for the person to allow it in the window; Deny closes it, and there is no
  "always allow". Clients are not trusted by the name they report.
- An attached agent writes only inside the folder of the project it acts on, reads only that folder
  and the browser's library folders, cannot open other projects from disk, cannot close tabs and
  cannot change the app's settings, the browser's folders or the plug-in folders.
- Each build has one more endpoint, which the first app of that build holds: it only opens files as
  tabs and brings the window forward, as starting the app with those files would.
- Dependencies, CI, packaging and release artifact integrity.
