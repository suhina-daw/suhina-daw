---
description: The surface-program protocol - how Suhina talks to a surface program's runner process, the Lua runner Suhina ships, the environment and API a script gets, and the caps, the error boundary and the hang deadline
read_when: surface program, surface-program protocol, Lua runner, lua-runner, mlua, Lua 5.4, sandbox, script error, error boundary, send-back, subscribe, port message, system exclusive, control surface script
type: spec
status: current
last_verified: 2026-11-13
covers:
  - crates/plugins/src/surface.rs
  - crates/lua-runner/**
---

# Surface programs

A control surface whose layout is more than a list of mappings is a **surface program**
([ADR 0019](../decisions/0019-midi-controllers-and-mapping.md) › 10): an extension
([ADR 0015](../decisions/0015-extensions.md)) Suhina starts and supervises, which gets every
message of the ports its manifest claims and changes Suhina only through the command protocol.
What a surface program is and what it holds is in [MIDI controllers and mapping](midi-mapping.md)
› Surface programs and [extensions](extensions.md) › Running one; this document is the protocol
and the runner that speaks it.

Suhina embeds no scripting language. It ships the **Lua runner**: a separate executable, built
from this tree, which runs one surface program's Lua script in a process of its own. The runner
is not an extension and not a surface program; the script's folder is both. A runner process that
crashes or hangs costs that program a restart, the way a plug-in host's process is restarted
([ADR 0006](../decisions/0006-plug-in-hosting.md)); its ports fall back to their profiles
meanwhile.

## Transport

One JSON object per line, UTF-8, ended by `\n`, over the local socket Suhina creates and hands
the runner as `--control <address>` (`ns:<name>` or `fs:<path>`, the plug-in control protocol's
scheme; `crates/plugins` owns the address forms and both sides reuse them). The script's path is
the runner's other argument:

```sh
suhina-lua-runner --control <address> <script.lua>
```

The runner connects; each side sends `hello` as its first message and refuses a peer whose
version is not one it can speak (see *Versions*), by sending `error` and closing. Suhina's
first message after the handshake is `ports` — possibly empty — and the runner applies it
before the script runs, so a chunk that sends in its first lines sees its ports. After that,
either side may send any message at any time; a line longer than the line cap ends the
connection.

## Versions

Each change to a message raises the version; both sides refuse a peer whose version is newer
than theirs, and run an older one only where this table says the older version stays
compatible. The constant is `suhina_plugins::surface::VERSION`.

| Version | What it says |
|---|---|
| 1 | First version. |

## Messages

Every message is an object with an `op`; the names below are the op's value. Suhina → runner:

| Message | Fields | What it says |
|---|---|---|
| `hello` | `version` | The handshake; answered with the runner's `hello`. |
| `ports` | `inputs`, `outputs` | The program's claimed ports that are open now, names in the order the manifest matched them. Replaced whole when it changes; never answered. |
| `port_message` | `port`, `timestamp`, `bytes` | Raw bytes (hex) read from a claimed input port, with the driver's arrival timestamp in microseconds. Never answered. |
| `state` | `token`, `value` | A subscription's value, as it is now. The first one answers the `subscribe` that made the token. |
| `command_result` | `token`, `ok`, `error` | The answer to a `command`: `error` names the reason when `ok` is false. |
| `read_result` | `token`, `ok`, `value`, `error` | The answer to a `read`: the reading command's value, or the reason. |
| `ping` | `id` | Answered with `pong`, same `id`. |
| `quit` | — | The runner stops the script and exits. Never answered; the exit is the answer. |
| `error` | `message` | Why the handshake failed or the last message was refused; either side sends it, then closes. |

Runner → Suhina:

| Message | Fields | What it says |
|---|---|---|
| `hello` | `version` | The handshake's answer. |
| `ready` | — | The script loaded and registered its handlers; Suhina may start delivering `port_message`s. |
| `send` | `port`, `bytes` | Raw bytes (hex) to write on one of the program's output ports. Never answered: Suhina queues it and drains the queue; the script never waits on a send-back. Send-backs leave in the order the script made them, ahead of any later message from the runner. |
| `command` | `token`, `id`, `params`, `gesture` | Run the command `id` with the parameters object, the same command the window, CLI and MCP dispatch, undo included. `gesture`, when present, groups the stream into one undo step (ADR 0019 › Amendment: one gesture), ended by the command `history.endGesture` like any caller's. |
| `read` | `token`, `id`, `params` | Run a reading command and answer its value. |
| `subscribe` | `token`, `id`, `params` | Watch a reading command's result: the first `state` answers with the value now, then one comes each time the result changes from the last one sent. |
| `unsubscribe` | `token` | Stop watching; never answered. |
| `script_error` | `handler`, `message` | The error boundary's one report: which handler or the chunk failed, and the Lua error. |
| `pong` | `id` | The answer to a `ping`. |
| `error` | `message` | Why the handshake failed or the last message was refused; either side sends it, then closes. |

Rules the message set keeps:

- **The script reaches Suhina only here.** The amendment's list is the whole list: port messages
  in, send-backs out, commands, reading commands, subscriptions — and nothing else (ADR 0019 ›
  Amendment). No message reads a file, opens a port, or touches the project by another way.
- **One undo step per command run**, as for every front end; `gesture` makes a stream one step.
- **Tokens** are chosen by the script, unique among its live ones; a token's answer or updates
  carry it back unchanged.
- A message the other side does not know, or one whose fields are missing or of the wrong kind,
  is an `error` naming the reason; the sender decides whether to keep going.

## The runner

`crates/lua-runner` (package `suhina-lua-runner`) is a single-threaded executable: it reads the
socket, runs the script's Lua 5.4 code (`mlua`, MIT, built from the `lua-src` crate, so no
system Lua is needed), and answers, in one thread. The interpreter never runs in Suhina's
process, and the crate carries no first-party `unsafe`.

- **Start-up.** The runner connects, says `hello`, applies the host's `ports`, and waits for the
  script to load: it runs the chunk once, the chunk registers handlers through the `suhina`
  table, and the runner says `ready`. An error in the chunk is the error boundary (below), not
  a crash.
- **The loop.** Each message is handled to completion before the next is read: a port message
  calls the script's handler, a `state` the subscription's handler, an admin message is
  answered. Pings are answered between handlers, which is how a hang is seen (below).
- **Waiting on answers.** `suhina.command` and `suhina.read` wait for their token's answer:
  while one waits, the runner keeps answering pings and queues port messages, and runs their
  handlers only after the waiting handler returns. A handler never runs inside another handler.

### The script's environment

The environment is narrowed to what the protocol allows; the narrowing is the runner's
convenience, and the boundary that is enforced is the process (ADR 0015 › Trust). Removed from
the globals: `io`, `os`, `dofile`, `loadfile` and `package` (so no `require`); `load` accepts
text chunks only, refusing a function chunk and a binary chunk. The usual libraries stay:
`coroutine`, `math`, `string`, `table`, `utf8`, and the base functions. Every removal is tested.

### The `suhina` table

The runner preloads a `suhina` table, versioned with the runner and outside the protocol, for
scripts to compose instead of reimplement. Version 1 gives the protocol's API; the larger helper
library — the usual modes, layers and session, mixer and device components — stays open
(ADR 0019 › Amendment) and grows here when it is decided.

| Member | What it does |
|---|---|
| `suhina.version` | The runner's version, a string. |
| `suhina.on_port_message(handler)` | Sets the handler for `port_message`: `(bytes, port, timestamp)`, the bytes a Lua string (binary). `nil` clears it. |
| `suhina.on_state(handler)` | Sets the handler for `state`: `(token, value)`. `nil` clears it. |
| `suhina.on_ports(handler)` | Sets the handler for the claimed ports: `(inputs, outputs)`, two arrays of port names, called once the script has registered — the ports as they are — and again each time they change. `nil` clears it. |
| `suhina.send(port, bytes)` | Queues a send-back: true, or false and nothing sent when the queue is full or `port` is not an open output port. |
| `suhina.command(id, params, gesture)` | Runs a command and returns `ok, error`. `params` is a table or `nil`; `gesture` a string or `nil`. |
| `suhina.read(id, params)` | Runs a reading command and returns `value, error`. |
| `suhina.subscribe(id, params)` | Watches a reading command; returns the token. |
| `suhina.unsubscribe(token)` | Stops watching. |
| `suhina.midi` | Message helpers for the bytes the protocol carries: builders (`note_on`, `note_off`, `cc`, `program`, `pitch_bend`, `sysex`) and `parse(bytes)`. The list may grow with the helper library. |

JSON values cross as Lua: an object a table, an array a sequence, a string a string, a number a
number, true and false themselves, and null `suhina.null` (a value that is neither `nil` nor
`false`, so an array keeps its places).

## Error boundary and restart

- **A script's error is not a crash.** The chunk erroring as it loads or runs, or any handler
  erroring, sends `script_error` once: `handler` names the chunk or the handler kind, `message`
  the Lua error. The runner then stops calling every handler, keeps the process alive, answers
  pings and honours `quit`. Suhina shows the error in the extension's state and puts the
  program's ports on their profiles; `extensions.restart` starts the program again.
- **A crash or a hang is the process's.** The runner process ending, or five seconds passing
  with no answer to a ping (Suhina pings a running program at the plug-in host's health rhythm,
  every 250 ms), is a hang: Suhina ends the process and restarts the program, its ports on their
  profiles meanwhile. A handler that runs longer than the deadline is such a hang — the
  amendment's price for a script whose stopping nobody can know in advance.

## Caps

A value over its cap is refused with the reason, and input is hostile as everywhere:

- Longest line: twice the bytes cap in hex plus room for the JSON around it —
  `2 * MAX_MESSAGE_BYTES + 64 KiB`, `suhina_plugins::surface::MAX_LINE`. Longest one port
  message's or send-back's bytes: 1 MiB.
- The runner's send-back queue holds at most 4096 messages or 8 MiB of bytes; past it
  `suhina.send` says false and sends nothing.
- At most 1024 messages queue while a handler waits on an answer; past it the runner sends
  `error` and exits, and the pings its wedge delays are what makes Suhina see the hang.
- At most 64 live subscriptions per program; a `subscribe` past it is refused with an `error`
  naming the reason, and the token stays unused.
- JSON nesting at most 32 deep in either direction.

## Testing

Each part lands with tests, by what it is:

- the message set: round trip of every message, and the malformed cases — an unknown op, a
  missing or wrongly typed field, bytes that are not hex, a line over the cap, nesting past 32;
- the sandbox: every removed global is gone, `load` refuses a function chunk and a binary chunk,
  and the `suhina` table's members behave — a send-back refused past the queue's caps, a
  handler cleared by `nil`;
- the error boundary: a chunk that errors, a handler that errors — one `script_error`, no
  handler called after, pings still answered, `quit` still honoured;
- end to end: the runner binary as a child process over a real socket — handshake, `ready`, a
  port message in and its send-back out, a command and its answer, a subscription's first
  `state`, the ports handler's first call and its next one when the ports change, the error
  boundary, `quit`.

No test depends on timing (AGENTS.md rule 7): the harness reads answers by blocking on the
socket, and nothing waits a wall-clock while.

## Open

- The helper library's exact shape and its caps — the usual modes, layers and session, mixer and
  device components, and how they version with the runner (ADR 0019 › Amendment leaves it open
  while the protocol is open).
- The manifest side (`extension.json` with `runner`, the ports patterns, the SHA-256) and the
  session's supervision: starting, restarting, the extensions' state and `extensions.restart`.
  `docs/spec/extensions.md` › Running one and `docs/spec/midi-mapping.md` › Surface programs
  describe the behaviour; their code is a later slice.
- A diagnostics channel (`log`) is not in version 1; the amendment's list is the whole list, and
  adding one is a version bump with a reason.
