---
description: The NAM Amp Modeler and NAM Cab Loader devices: what a .nam model file and a .wav impulse response are to Suhina, the devices' parameters and signal paths, gain staging, the model and IR files' folders, the commands and the device panels
read_when: NAM, Neural Amp Modeler, nam, amp modeler, cab loader, neural amp, .nam file, impulse response, IR, cab, model file, device.setNamFile, nam models folder, IR folder
type: spec
status: draft
last_verified: 2026-10-24
covers:
  - devices/nam-amp-modeler/**
  - devices/nam-cab-loader/**
  - crates/dsp/src/delay.rs
  - crates/dsp/src/resampler.rs
  - crates/dsp/src/convolver.rs
  - crates/device-registry/src/lib.rs
  - crates/engine/src/chain.rs
  - crates/session/src/devices.rs
  - crates/session/src/commands.rs
  - crates/ui/src/detail/**
  - crates/ui/src/settings/settings_rows.rs
  - crates/project-file/src/tests.rs
---

# The NAM Amp Modeler and NAM Cab Loader

Own design. A **NAM Amp Modeler** is a built-in effect (`DeviceKind::Builtin("nam-amp-modeler")`)
that runs a trained **NAM model** — a Neural Amp Modeler capture of an amplifier, preamp or
pedal, saved as a `.nam` file. A **NAM Cab Loader** is a built-in effect
(`DeviceKind::Builtin("nam-cab-loader")`) that convolves the sound with a **cab capture**, an
impulse response (an IR) of a speaker cabinet and its microphone, saved as a `.wav` file. With
the two on one track, the Amp Modeler first and the Cab Loader after it, a guitar plays through
a modelled amp into a captured cabinet.

Built because [issue 489] asks for a NAM device; its UI sketch describes the two panels' layout,
which this spec follows. The devices are Suhina's own throughout — their behaviour is written
here first (clean-room, [ADR 0004](../decisions/0004-built-in-devices.md)).

- **Builds on:** [ADR 0004](../decisions/0004-built-in-devices.md) (the `Device` trait,
  parameters, real-time rules), [ADR 0011](../decisions/0011-preview-and-added-audio.md) (the
  session decodes a project's files and hands them to the engine), the `.sd` devices (their file
  in `Device::state`, their device rebuilt when the file changes) and the settings' folder rows
  ([ui-settings](ui-settings.md)).
- **Inference:** the `.nam` forward pass runs through `nam-rs` (crates.io, MIT; a from-scratch
  Rust port of Neural Amp Modeler's inference, parity-tested against the reference and
  allocation-free on the audio path), which the two device crates and the registry depend on.
  Suhina reads the format, it does not train models.

## Terms

- **Model:** a `.nam` file — JSON with the file's `version`, its `architecture`
  (`WaveNet`, `LSTM` or `SlimmableContainer`, NAM's "A2"), the architecture's `config`, the
  trained `weights`, the `sample_rate` the capture ran at and a `metadata` block whose name,
  gear and loudness the panel shows. The model runs on **one channel**; the device keeps one
  running instance per side.
- **IR:** a `.wav` impulse response. Its first samples answer "what the cabinet plays after a
  click"; convolving a signal with it plays the signal through the cabinet. The device keeps one
  convolution per side.
- **The model's rate:** `sample_rate` from the file, 48 000 Hz when it omits one. A model must
  be fed audio at that rate; feeding it another rate silently mistunes its response. When the
  project's rate differs, the device resamples around the model ([§ Sample rates](#sample-rates)).
- **Calibration numbers:** the file's `metadata` may record `input_level_dbu` and
  `output_level_dbu` (the analog level a 0 dBFS signal stands for at the capture's input and
  output) and `loudness` (the model's output level in dBFS RMS). They are shown nowhere yet;
  they exist in the file for a later auto gain-staging ([§ Limits](#limits)).

## The devices' parameters

Both devices are effects (`Role::Effect`). A parameter's index is its slot in the info the
registry and the automation carry; ids are stable — projects store values by them.

### NAM Amp Modeler (`nam-amp-modeler`)

| Index | Id | Name | Kind | Range | Default | Unit |
|---|---|---|---|---|---|---|
| 0 | `inputGain` | Gain | continuous, linear | −24…+24 | 0.0 | dB |
| 1 | `eqBass` | Bass | continuous, linear | −12…+12 | 0.0 | dB |
| 2 | `eqMid` | Mid | continuous, linear | −12…+12 | 0.0 | dB |
| 3 | `eqTreble` | Treble | continuous, linear | −12…+12 | 0.0 | dB |
| 4 | `eqMidFreq` | Mid Freq | continuous, log | 200…2000 | 600.0 | Hz |
| 5 | `eqOn` | EQ | toggle | — | on | |
| 6 | `delay` | Delay | continuous, linear | 0…0.02 | 0.0 | s (shown in ms) |
| 7 | `hq` | HQ | toggle | — | off | |
| 8 | `outputVolume` | Volume | continuous, linear | −24…+24 | 0.0 | dB |

Per side, the signal runs: the input gain (smoothed, 20 ms like the utility's), the EQ, — when
the project's rate differs from the model's — the resampler into the model's rate, the model, a
DC blocker, the resampler back, the delay line and the output gain (smoothed). With no model,
or one that cannot run, the chain is the same with the model as a pass-through, so the trims,
EQ and delay still work.

- **Gain** trims what the model is fed around its calibrated input; the capture mapped 0 dBFS
  to the file's `input_level_dbu`, so 0.0 dB plays the capture as it was trained.
- **EQ** — when **EQ** is on: a low shelf at 100 Hz (`Bass`), a peak band at `Mid Freq` with
  Q 0.8 (`Mid`) and a high shelf at 4 kHz (`Treble`), all gentle biquads. When it is off, the
  bands pass the signal through unchanged.
- **HQ** — NAM "A2" models (`SlimmableContainer`) carry a set of complete sub-models, widest =
  highest quality, most CPU; HQ runs the widest one. The file's own name for this is the width
  dial, and switching is real-time-safe with a short warm-up transient in the newly selected
  sub-model. A WaveNet or LSTM model has no quality levels; **HQ** then has no effect and the
  panel's hint says so.
- **Delay** holds the output back by 0…20 ms, a straight delay line per side. It is part of the
  sound and is **not** latency-compensated: set it by ear against a parallel path.
- **Volume** sets the output level, after the model.
- **No bypass of its own** (decided 2026-10-18, #645): a device has one on/off state, the
  header's square and the 0 key (`device.setEnabled`), which turns the whole device off.
- **Latency:** the model adds none. The resampler adds its own few frames when it runs; the
  device reports them in `latency()` so the engine's compensation keeps parallel chains in
  phase. The **Delay** knob's frames are not reported.

### NAM Cab Loader (`nam-cab-loader`)

| Index | Id | Name | Kind | Range | Default | Unit |
|---|---|---|---|---|---|---|
| 0 | `phase` | Phase | toggle | — | off | |
| 1 | `hpOn` | HP | toggle | — | on | |
| 2 | `cutBass` | Bass | continuous, linear | −24…0 | 0.0 | dB |
| 3 | `cutMid` | Mid | continuous, linear | −24…0 | 0.0 | dB |
| 4 | `cutTreble` | Treble | continuous, linear | −24…0 | 0.0 | dB |
| 5 | `cutMidFreq` | Mid Freq | continuous, log | 200…2000 | 600.0 | Hz |
| 6 | `delay` | Delay | continuous, linear | 0…0.02 | 0.0 | s (shown in ms) |
| 7 | `volume` | Volume | continuous, linear | −24…+24 | 0.0 | dB |

Per side: the phase invert, a 2nd-order high pass at 100 Hz (when **HP** is on), the tone cuts —
a low shelf below 100 Hz (`Bass`), a dip at `Mid Freq` (`Mid`) and a high shelf above 4 kHz
(`Treble`), each 0…−24 dB, a cut only, as the mock shows them at 0.0 dB — then the convolver,
the delay line and the volume (smoothed). An IR convolution is entirely wet: the convolved
signal replaces the input, and the convolver's first output is the input through the IR's start,
not a mix.

- **Phase** (Ø) inverts the output's polarities.
- **HP** takes the rumble out that a close mic records.
- **No bypass of its own**, as the Amp Modeler: the device's on/off turns it off.
- **Latency:** the convolver reports its one partition (512 frames, 5.3 ms at 96 kHz) in
  `latency()`; the engine's compensation aligns a chain with the IR's start against parallel
  paths, as with the limiter's look-ahead. Longer IRs are handled inside the same delay with the
  partition's tail buffers. The **Delay** knob is not reported.

Both devices process in fixed 256-frame internal blocks counted from `reset` — like the FM
synth's internal blocks — so the output never depends on how a stream is divided, whatever the
host's buffer size or the conformance tests throw at them, and nothing on the audio path
allocates ([ADR 0004] › rules 3 and 6). Both pass the contract tests (`device_api::conformance`),
which run hostile parameter values, hostile buffers, every block size and 8…384 kHz.

## Model and IR files

- `device-registry` knows which devices take files and the two device crates own their formats:
  `suhina-device-nam-amp-modeler::model` reads and parses `.nam` files (`serde_json` on capped
  bytes; the parse and the running model's construction are off the audio thread), and
  `suhina-device-nam-cab-loader::ir` reads and decodes `.wav` IRs (hound, as the audio pool's
  wav path does). File sizes are capped: a `.nam` at 256 MiB and a `.wav` at 64 MiB are the
  largest read; larger, unreadable, malformed or unsupported files fail with a message that
  names what went wrong. A model that parses but uses a feature `nam-rs` refuses (a layer width
  the runtime does not run) is kept and its device passes the audio through; the panel says why
  ([§ States](#states)).
- An IR is resampled to the project's rate **once, when it is loaded** (off the audio thread, in
  the session): the convolver always convolves at the project's rate. A model is **not**
  resampled at load — samples are its language; see [Sample rates](#sample-rates).
- The file's path is stored in the project: `Device::state` holds the **absolute path** as
  UTF-8 — the same slot plug-ins and `.sd` devices use, saved and read with the project
  ([suhina format](suhina-format.md)), round-trip tested. The panel's dropdowns read the folder,
  not the project, so a project moved to another machine finds the same models if the folder
  setting says where they are, and says **Model missing** when it does not
  ([§ States](#states)); joining the missing-files view and relink is later work
  ([§ Limits](#limits)).

## Sample rates

A model runs at `expected_sample_rate()`. When the project runs at another rate, each side's
signal is resampled into the model's rate before the model and back after it, by a windowed-sinc
polyphase resampler (`crates/dsp::resampler`, a fixed kernel table, no allocation): the input
under-runs nothing and the output is not silent when the project starts mid-stream. At the same
rate, both resamplers pass through untouched. The device reports the resampler's added frames —
about a millisecond — as latency; the offline render and the live engine run the same code, so a
render of a 44.1 kHz project with a 48 kHz model is what playback sounds like.

## Folders

[ui-settings](ui-settings.md) gains two folder rows on the General page, drawn like the
Projects folder row (a field on the right, the default shown when the setting is empty):

| Section | Row | Note | Key |
|---|---|---|---|
| NAM | **Amp models folder** | The folder the NAM Amp Modeler lists its amp models from | `nam_models_folder` |
| NAM | **Cabinet sounds folder** | The folder the NAM Cab Loader lists its speaker cabinet recordings from | `irs_folder` |

- **Defaults:** `Suhina/NAM models` and `Suhina/IRs` in the user's documents folder — beside the
  default Projects and Templates folders, with the same old-folder fallback. The desktop app
  makes both default folders at start when they do not exist, so a user's first step is only to
  drop files in; a chosen (non-default) folder is left as the user's, and the device panels'
  list of a folder that is not there is empty, with the path and a note to put files in it.
- The pick bridge gets two fields (`PickField::NamModelsFolder`, `PickField::IrFolder`) so the
  system dialog opens for them like for the Projects folder (macOS waits on the main thread, as
  those already do).
- **Listing:** the folder holds **packs** — a device's dropdown lists the folder's files
  ending in `.nam` (amp) or `.wav` (cab), non-recursive, name-sorted without regard to case, at
  most 2000 entries (past that, the first 2000 and a "2000 of N, showing 2000" note). The list
  is read when the dropdown opens or a panel first shows it — never while drawing.
- Changing the setting does not retune what devices already point at: their stored path stands.
  A project that points into the setting's folder picks up a changed folder the next time its
  file is chosen or the project reopens.

### Packs

The folders the users keep are an everyday thing: a NAM capture drop usually arrives as one
**pack** — a subfolder with several `.nam` files (one amp, many captures of it). So the lists
are two levels deep, the same for both devices:

```text
NAM models/
├── British Plexi (Slammin Captures)/   ← a pack: one subfolder
│   ├── Bright Cranked.nam
│   ├── Crunch.nam
│   └── …
├── Fender Super Reverb 1965/
│   └── …
└── Loose capture.nam                  ← files beside the packs
```

- A **pack** is a subfolder of the folder in Settings. Its name is the subfolder's
  (case-insensitive sorted). The pack's files are its `.nam` or `.wav` files, name-sorted,
  same 2000 cap per pack.
- Files that sit beside the packs form a pack of their own, shown as **(entered directly)**.
- The panels show the pack and the file at the two dropdown rows:
  - **The pack row** (the name row): the current file's pack's name (the file's parent folder;
    **(entered directly)** for a loose file), "No model" / "No IR" when none is set. Its
    dropdown lists every pack's name; its ▲ ▼ steppers step through the packs.
  - **The file row**: the current file's own name, its dropdown lists the current pack's files
    by their shown names, its ▲ ▼ steppers step through that pack, wrapping.
  - Labels: the pack row is `Amp Modeler: pack`, its steppers `previous pack` / `next pack`;
    the file row is `Amp Modeler: model file` with `previous file` / `next file` (the Cab
    Loader likewise, with `cab file`). Choosing a pack loads its **first** file; both choices
    run `device.setNamFile`, one undo step each, as everything else on the panel.
- The shown name of a model is still its `metadata` name ("Fender Super Reverb 1965") or its
  file's name without the extension; a cab file's shown name is its pack's name. The pack row
  itself always shows the pack's folder name.
- The scan reads the folder's and each pack's write time, re-reading the folder when its own
  writes change and a pack when its writes change — the app reads it once per frame, and file
  loading stays on the session thread (the session re-reads at open or by command only).

## Commands

Everything the user does is one of the existing device commands plus one new `Edit` command:

- `device.add{id, position?}` — unchanged; `"nam-amp-modeler"` and `"nam-cab-loader"` are
  ordinary effects. A fresh NAM Amp Modeler has no model: it passes the audio through and its
  panel says to choose a file. (There is no `device.addNam`; choosing a file is its own step.)
- `device.setNamFile{track, device, path}` — the file a device runs.
  - The device must be one of the two; the path must end in `.nam` for an Amp Modeler and
    `.wav` for a Cab Loader (message otherwise).
  - The reading, parsing or decoding and the running model's construction run at command time,
    off the audio thread, bounded by the caps above. The command errors with what the file's
    fault is ("…is not a NAM model", "…is unreadable", "…is more than 256 MiB"). The parsed
    model or decoded IR goes into the session's cache.
  - The device's `state` becomes the file's absolute path; the chain's device id hashes it
    (`.sd`'s trick), so changing the file rebuilds that one device and a project replay keeps
    its state. One undo step, as `device.setParam` takes.

The session keeps the parsed models and decoded IRs keyed by path — the same road the decoded
audio files take ([ADR 0011](../decisions/0011-preview-and-added-audio.md); the session fills a
pool, the engine reads from it, and `Player::provide_audio` hands a new file to a running
engine): a new `Player::provide_device_asset` and the same map at constructor time, so an offline
render (`render_offline…`) and a live player see the identical assets. Loading runs where the
audio pool's does — at command or project-open time, in the session's thread (as `.sd` graphs
are read and validated at command time) — never on the audio thread.

## The panels

`crates/ui/src/detail` draws Suhina's first built-in device panels — the existing header (the
square on/off switch, the name, "Suhina") is unchanged, the body is the devices' own drawing,
laid out as the issue's sketch. Everything interactive is `device.*` commands; labels are as a
user reads them, and the tests find the panels by them.

Both panels, top to bottom:

1. **The pack row** — a full-width dropdown over the folder's packs ([Folders](#packs) ›
   Packs: a subfolder of the folder in Settings, each one an amp or cab with several files;
   **(entered directly)** when files sit beside the packs) with **previous pack** and **next
   pack** stepper buttons on its right.
   - The shown text: the current file's pack's name — the file's parent folder; "No model"
     (Amp Modeler) / "No IR" (Cab Loader) when `state` is empty or the stored path is not a
     pack's file.
   - Choosing a pack loads its first file (`device.setNamFile`, one undo step). Labels:
     "Amp Modeler: pack", "Amp Modeler: previous pack" (next pack); the Cab Loader likewise
     ("Cab Loader: pack").
2. **The controls area** — knobs with a label over each and the value under it, drawn with the
   theme's tokens; a drag up or down turns, a double-click returns to the default, arrow keys
   step when a knob has focus.
   - **The On button** at the Amp Modeler's controls area's top left ("Amp Modeler: device
     on" / "… off"): a bigger copy of the header's square, the same `device.setEnabled`, lit
     while the device runs; both always show the same state.
   - Amp Modeler: the **EQ** and **HQ** buttons beside the On button, on its row; under them
     one row of knobs, left to right: **Gain**; **Bass / Mid / Treble** with the **Mid
     Freq**'s line under them — a line with a diamond handle at the value's place, a drag
     along it sets the frequency, a click jumps —; **Delay**; **Volume** last. The panel is
     its own width (440 points) whatever the stock graph every built-in has would give it,
     and everything fits a detail area of its default height, no control over another.
   - Cab Loader: the **Phase** (Ø) and **HP** buttons at the top; **Bass / Mid /
     Treble** with **Mid Freq** under them; **Delay**; a vertical **Volume** slider.
   - Toggles are lit when on: On, EQ, HQ, Phase, HP. Labels name the device: "Amp Modeler:
     gain" … "Cab Loader: volume".
3. **The file row** — the file's own name without its folder or extension ("FNDR BFSR VB Edge WRM2 CAB FREE", #644) in a dropdown
   with the same previous/next steppers; "Choose a model file…" when none is set. Its dropdown
   lists the current pack's files by their shown names. Labels: "Amp Modeler: model file",
   "…previous file", "…next file" (Cab Loader likewise).
4. **A note row** when the device is not in shape, one line, plain text: "No model — choose one
   in Amp Modeler: model file", "Model missing: <path>" (the name row falls back to the file's
   name), or what a file faulted with, from the command's error. An A2 model without quality
   levels notes "This model has no quality levels" when **HQ** is on.

## States

| | The panel shows | The audio |
|---|---|---|
| No file chosen | No model / No IR, the file row's note | Pass-through: the amp's trims and EQ, or the cab's phase, HP, cuts and delay, at unity |
| File chosen, running | Name and file, knobs alive | Through the model or the IR |
| The file is gone | Model missing: <path> | Through the trims and EQ, no model or IR |
| The file cannot run (feature refused, malformed after all) | The fault, the name of the file | Pass-through |

A mono IR is heard on both sides; a mono model captures both sides' moves through two instances.

## What the project stores

`Device::state` round-trips (project-file tests read a project with both devices and get back
the paths), and a project whose device names a file that moved opens with the state kept and
the panel's Model missing note.

## Limits

- The missing-files view and relink do not know the NAM and IR paths yet; joining them (the
  panel then benefits from a fetched file and the one-click relink) is its own issue.
- A2's width dial is run at its widest by **HQ**; a graduated quality choice (a knob over the
  sub-models) comes with a model that shows the trade-off, not with this pair.
- Models are fed per side through their own running instance; a mono signal both sides share
  fires the model twice. A cheaper shared instance for a mono track is later work.
- Browsers and libraries do not list `.nam` files yet; the folders and the pick dialog are the
  whole surface. Drag-and-drop of a `.wav` from the browser onto a Cab Loader is a natural
  follow-on (the file row's dropdown lists it all the same).

[issue 489]: https://github.com/suhina-daw/suhina-daw/issues/489
[ADR 0004]: ../decisions/0004-built-in-devices.md
