## What and why

<!-- A few lines. Link the issue it closes: "Closes #123"; a bug or small change without an issue
     says what it fixes. A small fix found along the way in the same code goes under "Also fixed".
     One pull request per issue: when the
     issue already has an open one, push to it or offer your change there instead
     (CONTRIBUTING.md › Working on an issue). When it builds on another open pull request that it
     cannot be stacked on, add a line "Depends on #123": a check keeps it from merging first. -->

## Where the knowledge comes from

<!-- Own design, or for another product's behaviour or file format: the spec in docs/spec/ this
     follows (written before the code) and how it was found (using the program, files you made
     yourself, public documentation). -->

## How to check it

<!-- For anything a user sees, hears or does, in the app, its sound, the files it reads and
     writes, suhina-cli or MCP: numbered steps that someone who has never used Suhina or any
     other music program can follow from the test build linked below, in a few minutes. Start
     from opening Suhina with nothing loaded and say how to get every project, track, clip or
     file the check needs. Name each control as it reads on screen and where it is, give the
     shortcut too, give commands ready to copy, and explain a music term the first time. After
     each step, say what should happen. End with the edge cases (undo, empty project, a second
     click). For a change nobody can notice (refactor, tests, CI, docs), write one line saying so.
     docs/how-to-check.md shows how, with a template and an example.

1. Open Suhina. You see an empty project.
2. ...
-->

## How I tested it

<!-- What you tried by hand, on which OS, and what you saw or heard. For sound: the audio device
     and buffer size. -->

## Checklist

**Clean-room**
- [ ] Nothing here comes from disassembled, decompiled or leaked material, or from the content
      of a proprietary product (samples, presets, artwork, icons, interface text, visual design),
      and none of it was given to an agent. Screenshots used only to see where things are are
      fine ([ADR 0009](https://github.com/suhina-daw/suhina-daw/blob/main/docs/decisions/0009-ui-toolkit-and-look.md)).
- [ ] Every commit ends with a `Signed-off-by:` line (`git commit -s`): you confirm you may contribute
      the code under the project's licence ([DCO](https://github.com/suhina-daw/suhina-daw/blob/main/CONTRIBUTING.md#developer-certificate-of-origin)).

**Works and is tested**
- [ ] I tried every change by hand and it works as intended, also the parts an AI agent wrote.
- [ ] *How to check it* has steps a newcomer can follow, written by doing them in the build, or
      says there is nothing a user can notice.
- [ ] Tests cover it: round-trip and malformed input for file formats, offline renders for sound.
- [ ] `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`
      and `python scripts/rename_suhina.py --check` pass.

**Fits the architecture** ([AGENTS.md › Golden rules](https://github.com/suhina-daw/suhina-daw/blob/main/AGENTS.md#1-golden-rules))
- [ ] New things a user can do to a project are commands, so an agent can do them over MCP and
      core features work from the CLI (e.g. moving a clip, setting a track's volume, grouping
      tracks). Purely visual changes need none.
- [ ] Nothing on the audio thread allocates, locks, blocks, logs or does I/O, and no new code can
      panic.

**Documented**
- [ ] The documents that describe what changed are updated (`python scripts/docs.py impact`).
- [ ] The parity measurement is current: the coverage this change moves is updated in
      `scripts/parity/score.py` (and `scripts/parity/devices.json` for a built-in effect or
      instrument), or the description says `Parity: no change` and why. The report's numbers and
      the `last_verified` dates are written on `main` after the merge; leave them alone
      ([docs/workflow.md › Parity](https://github.com/suhina-daw/suhina-daw/blob/main/docs/workflow.md#parity)).
