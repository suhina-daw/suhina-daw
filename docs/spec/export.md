---
description: Rendering the mix to an audio file the user chose: the File › Export command in the app, the shared project.export command, the formats (WAV, FLAC, Ogg Vorbis) and their encoders, bit depths and dithering, ranges, and progress with a cancel
read_when: export, export dialog, mixdown, bounce, render to a file, render the mix, WAV, AIFF, FLAC, Ogg Vorbis, MP3, bit depth, 32-bit float, 24-bit, 16-bit, dither, normalize, sample rate, tail, range, loop region, project.export, overwrite, PickField, export a project, check it renders
type: spec
status: draft
last_verified: 2026-10-17
covers:
  - crates/automation/src/render.rs
  - crates/media/src/lib.rs
  - crates/session/src/commands.rs
  - crates/session/src/editing.rs
  - crates/ui/src/interaction/pick.rs
  - crates/ui/src/app.rs
  - apps/suhina-cli/src/main.rs
---

# Export

Rendering the mix into a file the user chose. Suhina renders offline already: `suhina-cli render`
writes a 32-bit float stereo WAV of the arrangement (or one launcher scene, or a script of clip
launches), the MCP `project_render` reaches the same calls, and `clip.consolidate` renders one
track's range into a new clip. What is missing is the export a user reaches from a menu, and the
compressed and reduced formats a finished mix is shared in.

Export is **one file of the whole mix** (post-master, exactly the stereo pair the audio device
would play). Rendering more than one file — per-track stems, group stems, a render matrix where
rows of tracks go into columns of files — is [the render matrix](#not-in-this-change) (#205), a
separate feature that would sit on the same encode step this spec adds.

## Observed in Live

The behaviour is taken from the public Live 12 manual: *Saving and Exporting* in [Live
Concepts](https://www.ableton.com/en/manual/live-concepts) and [Managing Files and
Sets](https://www.ableton.com/en/manual/managing-files-and-sets). What a finished project gets
there: **File › Export Audio/Video** opens a dialog with a rendered-range section (whole project
or the loop/section chosen), a samplerate, a PCM section (WAV, AIFF or FLAC, with record bit
depth choices and dithering for reduced depths), and a separate 320 kbps MP3 encode; export
shows a progress bar. Suhina reads those pages for the *shape* only and writes its own design:
no video, no SoundCloud, no cloud, MP3 last (see [Formats](#formats) for why). The names and
controls here are Suhina's own.

## The command

One command carries the whole feature, per ADR 0002, so the app, `suhina-cli` and the MCP server
all export the same way:

> `project.export {path|output, format?, sample_rate?, bit_depth?, overwrite?, range?, tail?, normalize?, dither?}`

It renders the active project's arrangement with the same offline render the playback engine and
`suhina-cli render` use (AGENTS.md rule 4), with the project's plug-ins started as
`project_render` starts them today. `project_render` keeps its role (scene renders, launch
scripts, WAV only); `project.export` is the user-facing file.

| Param | Meaning |
|---|---|
| `path` | The file to write, required when `output` is not given; aliases, same value. Absolute path from the desktop app; below the write root from the MCP server, like `project.render`'s `output` today |
| `format` | `wav` (default), `flac` or `ogg`; left out, the file name's extension decides it |
| `sample_rate` | Hertz; default 48 000, the render default today. The dialog prefills the project's rate |
| `bit_depth` | `32f` (default for WAV), `24` or `16`; applies to PCM only, an error with `ogg` |
| `overwrite` | `false` by default: an existing file is never replaced (the text below) |
| `range` | `{start, end}` in beats from the arrangement's start; default the whole arrangement |
| `tail` | Extra beats rendered after the range's last sound; default 0 (the engine's own tail is already in) |
| `normalize` | Scale the file's peak to −0.3 dBFS; default `false` |
| `dither` | Triangular dither, applied when the file's depth is `16` or `24`; default `true` there and `false` at `32f` |

The result mirrors `RenderSummary`, so scripts can read it: `{path, format, sample_rate,
bit_depth, seconds, peak, warnings}`; `warnings` lists missing files and the plug-ins that
would not start, as `project_render` does.

Rules the command keeps, wherever it runs:

- An existing file is replaced only with `overwrite: true`, the error text exactly
  `"<path> already exists; pass overwrite: true to replace it"`; a symlink at the target is
  never written through, and the target's folder must exist. These are
  [`Workspace::writable`](../architecture.md)'s rules today for `project.render`; the MCP server
  confines `project.export` to the write root the same way, and the desktop app, which passes an
  absolute path the user chose, reuses the same checks.
- A range renders like `clip.consolidate`'s tracks render today: rendered at the export's sample
  rate from the project's own start, the samples before `start` dropped, so a clip's position in
  the file stays frame-true. `end` must be at least one beat after `start`:
  `"end must be at least one beat after start"`. A range is capped at a day of audio, checked
  before the first block; the whole arrangement has no cap.
- An empty arrangement refuses to export: `"the arrangement is empty; add a clip before
  exporting"`.
- The extension in `path` and `format` must agree when both name one: a `.ogg` path exporting as
  WAV is an error, `"the file name says .ogg, and the format says wav"`.

Export changes no project state; it does not appear in undo.

## Formats

One writer lives in `crates/media` (new `media/src/write.rs`, taking interleaved stereo `f32`
frames), so `automation/render.rs`, the session command and the CLI stop carrying their own WAV
writing — automation's `write_wav` and the plug-in path's `hound::WavWriter` today — and every
format below goes through the same entry point.

| Format | Depth offered | Encoder | Licence check |
|---|---|---|---|
| WAV | `32f`, `24`, `16` | [hound](https://github.com/hound) (already a dependency for the plug-in path; PCM and float WAV and AIFF) | Apache-2.0 OR MIT, on the allowlist |
| FLAC | `24`, `16` | `flacenc` (pure Rust) — to verify at adoption | MIT OR Apache-2.0, on the allowlist |
| Ogg Vorbis | n/a | bindings to libvorbis (`quality` 5) — the bindings crate is chosen at implementation | libvorbis is BSD-3-Clause; the licence of the chosen binding crate is checked with `cargo deny check` before it is added |
| MP3 | — | not in the first version | LAME is LGPL-2.0-only, and the allowlist in `deny.toml` deliberately stops at LGPL-2.1; only a licence-clean encoder (LGPL-2.1, such as shine) reopens MP3, under this same table |

Dither (triangular, seeded from the export's parameters so renders stay golden-file
reproducible) applies when the file's depth is reduced; an oversampled renderer's noise is
already shaped elsewhere, so nothing else hunts it here. AIFF rides the same writer and joins
when a user asks; it changes nothing above the writer.

## In the app

**File › Export…**, beside Save as…, enabled when a project is open and not empty.

The dialog:

- **File name and folder**, with the project's name filled in (`<project name>.wav`), chosen
  with the system save dialog like Save as… does — a new `PickField::ExportFile` through the
  same [`PickRequest`](../architecture.md) path, so the window and its tests need no dialog and
  every path can still be typed.
- **Format** with the file's extension following the choice; **Depth** shown only for PCM
  formats; **Sample rate** (44 100, 48 000, 88 200, 96 000, the project's prefilled);
  **Range** (Whole ‹default›, Loop region, or Start/End beat fields the user fills; Loop region
  takes the project's loop region when it has one, Start/End stay grey otherwise);
  **Tail** in beats; **Normalize**; **Dither**.

Export runs while playback keeps playing: the render happens on a worker off the audio thread,
like recording's WAV writing and the previews do. The dialog turns into a progress bar (frames
rendered against the total) with **Cancel**; a cancelled export stops at the next block and
deletes its partial file. One export at a time; starting another is refused with `"an export is
already running"`. When it finishes the dialog says where and how long:
`"Exported to <name> (2:31.4)"`; a failed export keeps the dialog open with the error.

## Tests

- **Same path as render:** the demo arrangement exported as WAV `32f` is byte-identical to a
  `suhina-cli render` of the same project — the golden render stays one oracle.
- **Depths and dither:** a `16` export is bit-true triangular dither from a `32f` source, seed
  fixed, byte-identical across runs; `24` likewise; `normalize` lands at −0.3 dBFS within one
  round-off.
- **Compressed formats round-trip in the tree:** the FLAC and Ogg exports decode through
  symphonia to the same length and sample rate as the WAV golden; Ogg's RMS matches it within a
  tolerance named in the test.
- **Ranges and tails:** an explicit range starts frame-true at its `start` beat (the same skip
  `clip.consolidate` does), a `tail` grows the file, the loop-region preset matches the model,
  and the empty-arrangement, range-validity and overwrite refusals carry the texts quoted above.
- **In the app:** the menu item, the dialog's fields, and the cancel path deleting the partial
  file, as window tests.

## Not in this change

Stems and everything multi-file — per-track, groups, the render matrix — is #205, which would
build on this spec's encode step. Video, MIDI files, SoundCloud and friends are nobody's
problem today. MP3 waits for a licence-clean encoder; the format table above is where it joins.
