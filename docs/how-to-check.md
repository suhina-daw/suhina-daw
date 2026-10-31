---
description: How to write a pull request's "How to check it" section, steps a newcomer can follow from the test build to check a change in what Suhina does
read_when: how to check it, test steps, QA steps, how to test, pull request description, manual testing, reviewer steps, check a change
type: howto
status: current
last_verified: 2026-10-28
covers:
  - .github/pull_request_template.md
  - .claude/skills/how-to-check/SKILL.md
---

# How to check it

[workflow.md › How to check it](workflow.md#how-to-check-it) has the rule;
this is how to write the section, for a person or any agent. The reader has downloaded the test
build from the pull request's *Test build* comment, has never used Suhina or any other music
program, and should be done in a few minutes.

## 1. Does the pull request need it?

Yes for every change in what Suhina does: the window, the sound, a file it opens, imports or
writes, a `suhina-cli` command, an MCP tool, a setting. An engine change counts when a user can
hear or measure it (a render that sounds different, a file that now loads). No only for refactors,
tests, CI and documentation: then the section is one line, "Nothing a user can notice changes:
<what changed instead>."

The same pull requests update the parity measurement in the same change: the coverage they move in
`scripts/parity/score.py` (and `scripts/parity/devices.json` for a built-in effect or instrument);
the report's numbers are regenerated on `main` after the merge. One that moves no coverage writes `Parity: no
change` and the reason in its description, next to this section. [workflow.md › Parity](workflow.md#parity) has the rule, and the *Parity notice* check warns when
a change to `crates/`, `devices/` or `apps/` does neither.

## 2. Collect the facts first, never invent them

Every name in the steps must be what the screen shows. Look them up:

| What | Where |
|---|---|
| Menus, buttons, labels, tooltips | the text in `crates/ui/src` (`menu_button("File"`, `Button::new("Save as…"`, `.on_hover_text(`); the menu bar has **File**, **Edit**, **View**, **Projects**, **Options** |
| Default shortcuts | [`docs/shortcuts.md`](shortcuts.md) › Default keys (Ctrl is Cmd on macOS) |
| Where a region is and what it is called | [`docs/spec/ui-session-view.md`](spec/ui-session-view.md), [`ui-clip-editor.md`](spec/ui-clip-editor.md), [`arrangement-editing.md`](spec/arrangement-editing.md), [`browser.md`](spec/browser.md), [`sample-map.md`](spec/sample-map.md), [`docs/design/components.md`](design/components.md) |
| A label the tests click | the UI tests (`harness.get_by_label("…")`): the same text the user sees |
| Settings | Options › Settings, and its tabs as `settings_view.rs` names them |
| CLI commands and their arguments | `suhina-cli --help` and `suhina-cli <command> --help` in the test build; `apps/suhina-cli/src` |
| MCP tools and commands | [`docs/mcp.md`](mcp.md), [`docs/commands.md`](commands.md) |
| What a file format change reads or writes | the spec in `docs/spec/` the pull request follows |

Read the diff for what changed and the issue for what it is for; the steps check that, plus what
the change could break next to it.

## 3. Write the steps

```markdown
## How to check it

Platform: all (or: Windows only, because …)

**Set up**
1. Unpack the test build: unzip it, and on macOS and Linux extract the `.tar.gz` inside it too. Open `suhina`
   (`suhina.exe` on Windows, `Suhina.app` on macOS). An empty project opens.
2. <make what the check needs, from the menus: the project, track, clip or file>

**Check**
3. <one action>: <exact control, where it is, and its shortcut>.
   You should see: <what changes on screen, or what you hear>.
4. …

**Edge cases**
5. Press Ctrl+Z (undo). You should see: <the state before step 4>.
6. <a second click, an empty project, the smallest or largest value …>

If something else happens, note the step number and what you saw.
```

Steps outside the window follow the same shape. A command is given whole, in its own code block,
run from the unzipped test build's folder, with what it prints and what file it leaves:

```markdown
3. Open a terminal in the unzipped folder (Windows: in File Explorer, type `cmd` in the address
   bar and press Enter) and run:

       suhina-cli plugin run test:vst3 - check.wav --seconds 1 --tail 0

   You should see: no error, and a new file `check.wav` in the folder, about a second long when
   opened in any audio player.
```

On macOS and Linux the command starts with `./` (`./suhina-cli …`). For MCP, give the client
setup from `docs/mcp.md` and the exact request to send. For a file Suhina imports, say where the
reader gets one; never "use one of your own projects".

Rules for each step:

- **One action per step**, then **You should see:** or **You should hear:**. Add **Wrong:** when
  a failure is not obvious ("Wrong: the clip plays from where it stopped").
- **Name the control as the screen shows it, in bold, and say where it is**: "the **+** button
  at the bottom of the track list", "right-click the empty slot under the track name, then
  **Paste**". Give the shortcut too: "**Edit › Undo** (Ctrl+Z)".
- **Start from nothing.** Never "open a project with some clips". Make what is needed with the
  app's own menus, or name a file: one in the repository (with its path and a link to download
  it), or a short way to make one. Say what the screen looks like after setting up.
- **Explain a music term the first time**, in brackets, and then use the same word every time:
  see the glossary below. Use Suhina's own words, never another DAW's terms ([AGENTS.md › Golden
  rules](../AGENTS.md#1-golden-rules) › 8).
- **Sound:** say what to listen for and how loud ("a short beep, repeating every second"); remind
  the reader to turn the volume down first when the change can be loud.
- **Platform:** say when a step differs (Cmd on macOS, right-click on a trackpad) or when only
  one platform is affected.

## 4. Check the steps

Walk the steps in the test build (or `cargo run --release -p suhina`) before posting them, and fix
every step that does not match the screen. If you cannot run the window, say so in *How I
tested it* and say which steps a UI test covers; never present steps as checked when they were
written from the code.

## Glossary

Explain a term like this the first time the steps use it:

| Term | Plain words |
|---|---|
| project | the whole song, saved as one `.suhina` file |
| track | one column (Launcher view) or row (Arrangement view) holding one instrument or one kind of sound |
| slot | one box in a track's column; it holds one clip or is empty |
| clip | a block of sound or notes in a slot or on the timeline |
| scene | a row of slots across all tracks; launching it starts every clip in that row |
| launch | start a clip or scene playing; it starts on the next beat or bar, not at once |
| Launcher view | the grid of tracks and slots for starting clips live (Tab switches views) |
| Arrangement view | the timeline, left to right, where clips are laid out as a song |
| browser | the panel listing sounds and files (Ctrl+B shows or hides it) |
| sample | a recorded sound in an audio file |
| MIDI clip | a clip of notes that an instrument plays, instead of recorded sound |
| tempo | the speed in beats per minute (BPM), shown in the control bar at the top |
| effect, instrument | a sound processor on a track; an instrument makes sound from notes |
| plug-in | an effect or instrument from another maker, loaded from a file (CLAP, VST3) |
| undo | Edit › Undo (Ctrl+Z) reverses the last change |

## Example

Bad: "Tested slot selection, works. Open a set and click around the slots."

Good:

```markdown
Platform: all

**Set up**
1. Unpack the test build: unzip it, and on macOS and Linux extract the `.tar.gz` inside it too. Open `suhina`
   (`suhina.exe` on Windows, `Suhina.app` on macOS). Suhina opens with nothing loaded.
2. Click **File › New or open…** (Ctrl+T) and then **New blank project**. You should see: a grid
   of 4 tracks (columns) with empty slots (boxes that hold clips).

**Check**
3. Click **View › Zoom in** (Ctrl+Plus) twice. You should see: everything in the window, text
   and buttons included, gets larger each time; the menu now says **Actual size (120 %)**.
4. Close Suhina and open it again. You should see: the window is still at 120 %.
   Wrong: it is back to its normal size.

**Edge cases**
5. Click **View › Actual size (120 %)** (Ctrl+0). You should see: the normal size, and the
   entry is greyed out.
6. Press Ctrl+Minus until **Zoom out** is greyed out in the View menu. You should see: it
   stops at 50 % and nothing overlaps so much that it cannot be read.
```
