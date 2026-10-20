---
name: trace-debugging
description: Find out what the running Suhina app did around a bug someone saw while using it - lag, a freeze or hang, a stall in a plug-in's editor, stutter, a flicker or flash, something late or out of time - with the trace log (suhina --trace) and a screen recording read frame by frame. Use when a tester reports such a bug, when asked to start the app with a trace, to read a trace or a recording, or to show with a trace that a fix works.
---

Read [`docs/debugging.md`](../../../docs/debugging.md) and follow it: part 1 for the trace log
and its lines, part 2 for a recording, part 3 for working a bug through with a tester. It is the
one copy of the guide, written for any agent or person; this skill only points Claude Code at it.

While the tester uses the app, do only what they asked: start or restart it on a trace when they
agree, read what they point at, and report what the trace shows with the lines and times that
show it. A fix follows [`AGENTS.md`](../../../AGENTS.md) like any other.
