---
description: Rendering the mix to an audio file the user chose: the File › Export command in the app, the shared project.export command, the formats (WAV, FLAC, Ogg Vorbis) and their encoders, bit depths and dithering, ranges, rendering one node of the track tree (a track or a group, the foundation of the render matrix), and progress with a cancel
read_when: export, export dialog, mixdown, bounce, render to a file, render the mix, render a track, render a group, stems, premaster, source, node, WAV, AIFF, FLAC, Ogg Vorbis, MP3, bit depth, 32-bit float, 24-bit, 16-bit, dither, normalize, sample rate, tail, range, loop region, project.export, overwrite, PickField, export a project, parallel rendering, thread count, pre-roll, deterministic, check it renders
type: spec
status: draft
last_verified: 2026-10-24
covers:
  - crates/automation/src/render.rs
  - crates/engine/src/buses.rs
  - crates/engine/src/lib.rs
  - crates/engine/src/sends.rs
  - crates/media/src/lib.rs
  - crates/media/src/write.rs
  - crates/session/src/commands.rs
  - crates/session/src/export.rs
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

Export is **one file** of the whole mix — post-master, exactly the stereo pair the audio device
would play — or, with a `source`, one file of **one node of the track tree** (a track or a group),
which is the first slice of the [render matrix](#the-slices-to-the-render-matrix) (#205): several
files at once, sources × ranges, one per cell. Rendering more than one file is #205, a separate
feature that sits on the same encode step this spec adds.

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

> `project.export {path|output, format?, sampleRate?, bitDepth?, mp3Rate?, overwrite?, range?, tail?, normalize?, dither?}`

It renders the active project's arrangement with the same offline render the playback engine and
`suhina-cli render` use (AGENTS.md rule 4), with the project's plug-ins started as
`project_render` starts them today. It reads the running plug-ins' state as it is now (ADR 0006
decision 5), so the export plays what is heard — the changes made in a plug-in's own editor since
the project was opened — reading it on threads of its own, waited for at most a second. The
project changes nothing: no undo step is recorded, and a plug-in that did not answer in time
keeps the state it last gave, the session's notices saying so. `project_render` keeps its role
(scene renders, launch scripts, WAV only); `project.export` is the user-facing file.
`suhina-cli render --source <track or group>` reaches the same node render from the command
line, which is also how the golden renders of the tests run.

| Param | Meaning |
|---|---|
| `path` | The file to write, required when `output` is not given; aliases, same value. Absolute path from the desktop app; below the write root from the MCP server, like `project.render`'s `output` today |
| `format` | `wav` (default), `flac`, `ogg` or `mp3`, in any case; left out, the file name's extension decides it. Another name is refused, `"aiff is not a format; wav, flac, ogg or mp3"` |
| `sampleRate` | Hertz; default 48 000, the render default today. The dialog prefills the project's rate; with `mp3` the dialog offers 32 000, 44 100 and 48 000, the rates the encoder writes — an export at another rate is refused, `"sample rate 96000 is not one the encoder writes; 32, 44.1 and 48 kHz and their MPEG 2 halves are"` |
| `bitDepth` | `32f` (default for WAV), `24` or `16`; applies to PCM only, an error with `ogg` or `mp3` |
| `mp3Rate` | The MP3's rate: `320` (default), `256`, `192` or `128` kbps, or the variable qualities `v0` and `v2`; asked for only with `mp3` |
| `overwrite` | `false` by default: an existing file is never replaced (the text below) |
| `source` | A track id, or a track or group name; left out, the whole mix. Renders one node of the track tree — [Sources](#sources-rendering-one-node-of-the-track-tree) |
| `range` | `{start, end}` in beats from the arrangement's start; default the whole arrangement |
| `tail` | Extra beats rendered after the range's last sound; default 0 (the engine's own tail is already in) |
| `normalize` | Scale the file's peak to −0.3 dBFS; default `false` |
| `dither` | Triangular dither, applied when the file's depth is `16` or `24`; default `true` there and `false` at `32f` |

The result mirrors `RenderSummary`, so scripts can read it: `{path, format, sample_rate,
bit_depth, seconds, peak, warnings}`, `bit_depth` null for `ogg` and `mp3`; `warnings` lists missing files and the plug-ins that
would not start, as `project_render` does.

Rules the command keeps, wherever it runs:

- An existing file is replaced only with `overwrite: true`, the error text exactly
  `"<path> already exists; pass overwrite: true to replace it"`; a symlink at the target is
  never written through, and the target's folder must exist. These are
  [`Workspace::writable`](../architecture.md)'s rules today for `project.render`; the MCP server
  confines `project.export` to the write root the same way, and the desktop app, which passes an
  absolute path the user chose, reuses the same checks. The rules hold again in the step the
  bytes land: the export writes to a file of its own beside the destination first, and puts the
  finished one in place in one step — so a file made during the render is refused (`overwrite`
  off) or replaced itself (a symlink at the name is replaced, not written through), and a failed
  encode removes its own file and leaves the destination it would have replaced untouched.
- A range renders like `clip.consolidate`'s tracks render today: rendered at the export's sample
  rate from the project's own start, the samples before `start` dropped, so a clip's position in
  the file stays frame-true. `end` must be at least one beat after `start`:
  `"end must be at least one beat after start"`. A range is capped at an hour of audio and at
  the engine's 2 GiB of rendered samples, both checked before the first block; the whole
  arrangement is bounded by the render's own hour limit (`suhina_engine::MAX_RENDER_SECONDS`).
- An empty arrangement refuses to export: `"the arrangement is empty; add a clip before
  exporting"`.
- The extension in `path` and `format` must agree when both name one: a `.ogg` path exporting as
  WAV is an error, `"the file name says .ogg, and the format says wav"`.

Export changes no project state; it does not appear in undo.

Attached MCP runs `project.export` through the same snapshot and render path, including inside
`command_batch`. Preparation advances over window frames; rendering and encoding hold neither
the window thread nor project access. Settings › Agents and updates lists MCP write grants, including the
default project-folder grant, and lets the person add and revoke folders. Subfolders are
included. These permissions do not restrict the desktop user's chosen export destination.

State-read replies update the gather before its ledger is taken for settlement. A failed reply
is completed but unsuccessful: fallback emits a warning naming the plug-in. Preparation
warnings travel with the export data into the command result and the desktop completion status,
including when a different tab is active.

## Sources: rendering one node of the track tree

With `source`, the render takes its sum at one node of the track tree instead of at the master.
A source is any track or group, named by id or by name (a name shared by several tracks is an
error: `"several tracks are named \"Bass\"; use the track id"`). The render hears everything that
plays into the node and the
node's own chain, and nothing after it: a track comes with its own devices only; a group comes
with the sum of the tracks routed through it and the group's own devices — a compressor on the
group is in, its parent premaster and the master chain are out. The node's own volume, pan and
mute are in, exactly as the mixer would play it.

The engine builds the same graph as playback and the whole-mix render, and the offline render
(`render_offline_from` with a source) reads the source node's chain output where the master's sum would be
(AGENTS.md rule 4); there is no second export path. The source node always has a chain in the
graph, a passthrough when it would not have one (a track without devices, a group without
them), and everything that plays into it targets it, so the sum at the node is whole. Latency
compensation is unchanged: every path inside the source is brought to the same latency it has in
the whole mix, so two node renders start frame-true together and line up with each other and
with the whole mix.

- The mixer sounds as it does in playback: a muted track under the source is silent, a soloed one
  is alone.
- `range` bounds the source's render the way it bounds the whole mix; `tail`, `normalize`,
  `dither` and the formats apply unchanged.
- A send from a track under the source is **not** in a node render yet: return chains are left
  out, and every return a track under the source sends to is named in the result's `warnings`
  (`"sends to Echo are not in this render"`). Whether a node render carries its sends, and with
  which of their effects, is the render matrix's choice to offer (#205); until then it is said,
  never silent. The whole-mix export renders sends as always.
- The master chain is the whole mix's, so a node render's peak can be over 1.0 where the mix
  would not clip; `normalize` brings the file back under, the same as anywhere else.

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
| MP3 | n/a | LAME 3.100, vendored as C source in `third_party/lame/` (encoder files only; its `fft.c` is replaced by Suhina's own Hartley transform), compiled by `crates/mp3` behind a safe API ([ADR 0029](../decisions/0029-mp3-encoder.md)) | LGPL-2.0-or-later used as LGPL-2.1-or-later; the three headers with no notice fall under the package licence, whose section 3 offers the GPL reading; recorded in `third_party/lame/VENDORED.md`. MP3 patents expired 2017. `deny.toml` unchanged (cargo-deny does not see vendored C) |

Dither (triangular, seeded from the export's parameters so renders stay golden-file
reproducible) applies when the file's depth is reduced; an oversampled renderer's noise is
already shaped elsewhere, so nothing else hunts it here. AIFF rides the same writer and joins
when a user asks; it changes nothing above the writer.

The export holds one whole-file buffer — the render itself — whichever format is written. FLAC
quantizes and encodes frame by frame and writes its bytes out as they come, where the encoder's
own whole-stream `ByteSink` would hold the whole file in memory again before writing it; the
staged rules keep the file's landing atomic all the same.

The MP3 file's ID3v2 header carries the title — the project's own name, the stem of the file
it was opened from; an unsaved project writes none, and so does `suhina-cli render --source`
of the demo — and nothing else. After encoding, the
LAME tag frame replaces the empty frame LAME leaves at the stream's start, so players get
gapless playback and the true length.

## In the app

The basic dialog is in: file (typed or browsed), format, sample rate, depth, normalise, dither
and overwrite. The source and range rows, and the progress bar with cancel, are the slices after
it; the command carries them already.

**File › Export…**, beside Save as…, enabled when a project is open and not empty.

The dialog:

- **File name and folder**, with the project's name filled in (`<project name>.wav`), chosen
  with the system save dialog like Save as… does — a new `PickField::ExportFile` through the
  same [`PickRequest`](../architecture.md) path, so the window and its tests need no dialog and
  every path can still be typed.
- **Source**: *Whole mix* ‹default›, then every track and group in tree order, groups shown with
  the tracks inside them indented. Choosing a node renders that node ([Sources](#sources-rendering-one-node-of-the-track-tree)).
- **Format** with the file's extension following the choice; **Depth** shown only for PCM
  formats (Ogg Vorbis and MP3 take none); **Sample rate** (44 100, 48 000, 88 200, 96 000, the
  project's prefilled; with MP3 the row offers 32 000, 44 100 and 48 000); **MP3 rate**
  (320 kbps ‹default›, 256, 192, 128, VBR V0, VBR V2), shown only for MP3;
  **Range** (Whole ‹default›, Loop region, or Start/End beat fields the user fills; Loop region
  takes the project's loop region when it has one, Start/End stay grey otherwise);
  **Tail** in beats; **Normalize**; **Dither**.

Export runs while playback keeps playing: the render happens on a worker off the audio thread,
like recording's WAV writing and the previews do. The dialog turns into a progress bar (frames
rendered against the total) with **Cancel**; a cancelled export stops at the next block and
deletes its partial file. One export at a time; starting another is refused with `"an export is
already running"`. When it finishes the dialog says where and how long:
`"Exported to <name> (2:31.4)"`; a failed export keeps the dialog open with the error.

In what is in today, the render and the encoding run on a worker of their own and the window
keeps drawing; the dialog shows **Exporting…** while the job is out. Until the worker starts,
the running plug-ins' states are gathered over the window's frames, at most a second; while they
are, the dialog's **Cancel** and its close cancel the gather — a late reply changes nothing and
starts nothing, and the next export gathers its own round. Once the render runs, the buttons
wait (cancelling a render is the progress bar's slice): a dialog closed over a rendering export
leaves the worker on, its result taken, and the status bar says where the file went. The
feedback names the project the export belongs to — whatever tab is showing after, and once that
project is closed — and a project closed while its export is gathering stops the export with it
said.

## Tests

- **Same path as render:** the demo arrangement exported as WAV `32f` is byte-identical to a
  `suhina-cli render` of the same project — the golden render stays one oracle.
- **Depths and dither:** a `16` export is bit-true triangular dither from a `32f` source, seed
  fixed, byte-identical across runs; `24` likewise; `normalize` lands at −0.3 dBFS within one
  round-off.
- **Compressed formats round-trip in the tree:** the FLAC and Ogg exports decode through
  symphonia to the same length and sample rate as the WAV golden; Ogg's RMS matches it within a
  tolerance named in the test; the MP3 export likewise, and the same export is byte-identical
  across runs, header and LAME tag included.
- **Ranges and tails:** an explicit range starts frame-true at its `start` beat (the same skip
  `clip.consolidate` does), a `tail` grows the file, the loop-region preset matches the model,
  and the empty-arrangement, range-validity and overwrite refusals carry the texts quoted above.
- **One node of the track tree** (`source`): a group render carries the group's own devices and
  nothing after it (a device on the parent premaster changes the master render and not the group
  render); the renders of a group's children — through a project where every group only sums, no
  devices anywhere above a track — add sample for sample to the render of the group, and the
  renders of all top-level tracks add to the master's; a track and a group start at the same
  frame when their devices have different latencies, and line up with the whole-mix render; a
  muted child is silent, a send to a return is absent and named in `warnings`; a name shared by
  several tracks is refused with the id guidance in the text (`track_at`'s words). The export
  and a `suhina-cli render` of the same project and settings are the same bytes — the
  determinism the promises build on.
- **In the app:** the menu item, the dialog's fields, and the cancel path deleting the partial
  file, as window tests.

## The engine's promises

The offline render the matrix stands on is parallel, and its result does not depend on how many
threads ran. These are promises, written down so the first slice does not foreclose them; they
are built when the slices below need them.

- **Parallel.** An offline render may use every core: independent sources render at the same
  time, and a group sums its children once they are all done. Ranges render in parallel too,
  each starting with a pre-roll so reverb and delay state is warm — the range render of this
  spec already guarantees the warm state the long way, from the project's own start, and the
  pre-roll is the shorter way to the same state. Encoding and writing run on threads of their
  own, a file's samples never waiting for another file's encoder. The thread count is a
  setting; the default is all cores minus one.
- **Bit-identical whatever the thread count.** The summing order is fixed — a group's children
  in the project's track order, the buses after the tracks that feed them — never whoever
  finishes first. A render with one thread and a render with many are the same bytes; the
  golden-file tests run both ways and must match.
- **The scheduler lives only in the offline path.** The audio callback stays lock-free (AGENTS.md
  golden rule 3), and the node-processing code is shared with real-time playback (rule 4), which
  is what keeps an offline render the test oracle for playback.
- **Cancel leaves nothing behind.** A cancelled render stops without a half-written file, at any
  time. The dialog's Cancel does this today; the matrix's job cancels the same way.

The first slice honours them by not making them impossible: a render starts as a *job* — its
source, range and settings are the command's params, handed to the offline render as data — not
as a loop hard-wired to one file.

## The slices to the render matrix

The render matrix (#205) is several files in one pass: any nodes of the track tree as sources ×
any ranges, one file per cell, with the usual exports as presets on top of it — the whole mix,
every top-level track as stems, selected tracks only, returns with and without the master's
effects. It builds on the encode step and the node rendering this spec adds, slice by slice:

1. **One chosen node to one file** — the `source` param of this spec (a track or a group, its own
   devices, nothing after it). Everything else here is this slice.
2. **The familiar options as settings of the same render job**, not a separate feature: render as
   loop (the tail wraps to the range's start), a mono fold, and the sends-and-returns choice a
   node render has to offer.
3. **The matrix**: several sources and several ranges in one pass, one file per cell, the presets
   above as ways to fill it, the progress and cancel of one job over all its files, and the
   parallel scheduler the [promises](#the-engines-promises) describe.

New file formats and new options plug into the writer and the render job without touching the
core: the format table above is where an encoder joins, and a render job's settings are the same
command's params whatever the file. Stems — what the matrix's presets produce — stay out of
#601, with video, MIDI files and cloud places.
