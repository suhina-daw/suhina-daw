---
description: How the crates, threads and commands fit together, from a file to the speakers, and where new code goes
read_when: adding a crate or feature, where does code go, threads, audio thread, data flow, layers, player, controller
type: system
status: current
last_verified: 2026-11-01
covers:
  - crates/style/
  - apps/suhina/src/theme_io.rs
  - crates/session/src/record_writer.rs
  - Cargo.toml
  - crates/*/Cargo.toml
  - devices/*/Cargo.toml
  - crates/engine/src/launcher.rs
  - crates/engine/src/swap.rs
  - crates/engine/src/ahead.rs
  - crates/engine/src/cpu.rs
  - crates/session/src/projects.rs
  - crates/session/src/rack.rs
  - apps/suhina-cli/src/play.rs
  - crates/output/src/lib.rs
  - crates/output/src/devices.rs
  - crates/output/src/exclusive.rs
  - crates/session/src/audio.rs
  - crates/ui/src/lib.rs
  - crates/ui/src/arrangement.rs
  - crates/ui/src/session.rs
  - crates/ui/src/detail.rs
  - crates/ui/src/interaction.rs
  - crates/ui/src/kit.rs
  - crates/library/src/worker.rs
  - crates/ui/src/browser.rs
  - crates/ui/src/app/missing_files.rs
  - apps/suhina/src/main.rs
  - crates/automation/src/server.rs
---

# Architecture

How Suhina's parts fit together: the layers, how a project gets from a file to the speakers,
which threads exist and what each may do, and where new code goes. The rules behind it are in
[`AGENTS.md`](../AGENTS.md); the larger decisions in [`decisions/`](decisions/). This page
describes the code as it is; planned parts are marked **(planned)**. A change that makes this
page wrong updates it in the same pull request ([`workflow.md`](workflow.md) › Documents).

## Layers

A crate may depend only on crates in lower layers. Nothing below the UI depends on a GUI toolkit.

```text
L3  automation        output, midi    ui                apps/suhina-cli, apps/suhina
      MCP server,       audio device,   window, views      the CLI; the desktop app
      renders           MIDI ports      (egui)
        |                 |               |
L2  session ------- library, soundmap ------- media
      commands, undo,   the browser's folders,    finds and decodes audio files,
      the player,       index, pins, tags;        reads MIDI and Stems files,
      the project's     the sample map            metadata, detects tempo
      audio, plug-ins   separation: the stem
                        separation pipeline
        |        \                                  |
L1  als, flp, project-file  engine ------------------+      plugins
      Live Set, .flp,   rendering, launcher,                   plug-in host processes
      .suhina files     no file I/O                             (suhina-plugin-host)
        \        /                                              |
L0        model        stretch      dsp, device-api, device-registry      ipc, realtime
      pure data        DSP          device-sdk, devices/*                 shared-memory blocks,
                                                                          thread priority
```

Within layer 2, `session` uses `library`, `soundmap` and `media`; `library` and `soundmap` use
`media`; and `media` uses neither. `separation` sits beside them and uses none of them yet; the
session will hold it and `media` will write its stems (planned, #146). The built-in effects and instruments sit apart: `dsp`,
`device-api` and `device-sdk` depend on no Suhina runtime crate, and each device crate under
`devices/` depends only on `dsp`, `device-api` and, for the SDK's widgets and runtimes,
`device-sdk` ([ADR 0004](decisions/0004-built-in-devices.md) › 1).
The engine runs them in chains on tracks, made through `device-registry`.

| Crate | Owns | Depends on |
|---|---|---|
| `style` | Toolkit-independent native style sheets ([ADR 0031](decisions/0031-style-sheets.md)): bounded selectors and device scopes, typed properties and shadow data, cascade and inherited variables, viewport conditions, Taffy rectangles and source-preserving edits. It takes text and element data, never a window, file or audio callback. The desktop uses the source editor and resolves supported paint-only sheets directly; sheets requiring remaining layout, gradient or animation features use the compatibility adapter during migration. | cssparser, taffy |
| `model` | The DAW-neutral project: tracks, mixer channels, a pool of clip contents, clips that place them, the launcher grid ([ADR 0001](decisions/0001-daw-neutral-model.md)). Validation, with groups nested at most 16 deep and never inside themselves, and the one conversion from beats to time and bars through the tempo and time-signature changes (`TempoMap`, `Meter`; [spec](spec/tempo-map.md)), and which audio clips overlap on a track (`audio_overlaps`, [ADR 0033](decisions/0033-ara-hosting.md)). No behaviour beyond that. | nothing |
| `stretch` | Time-stretching and pitch-shifting DSP for interleaved audio: a pure-Rust port of Signalsmith Stretch (MIT), behind every stretch mode except `Repitch` ([ADR 0003](decisions/0003-time-stretching.md)). | nothing (rustfft) |
| `als` | Reading Live Sets into `model`, with warnings for what it skips ([spec](spec/als-format.md)). One crate per format later. | model |
| `flp` | Reading an FL Studio project's container and event stream: the `FLhd` header, the `FLdt` chunk, events with typed values and strings in the encoding the version event decides, every error with its offset ([spec](spec/flp-format.md)). No model mapping yet ([epic #271](https://github.com/suhina-daw/suhina-daw/issues/271)). | nothing |
| `project-file` | Suhina's own project file (`.suhina`): the model as versioned JSON, written and read with nothing lost, every value checked; which paths are project files (`is_project_file`); audio paths moved when a project is saved elsewhere ([ADR 0010](decisions/0010-project-files.md), [spec](spec/suhina-format.md)). | model |
| `engine` | Turns a project into audio: note voices, audio clips with real-time stretching ([ADR 0003](decisions/0003-time-stretching.md)), the clip launcher ([spec](spec/clip-launching.md)), device chains with plug-in slots ([ADR 0004](decisions/0004-built-in-devices.md), [device chains](spec/device-chains.md)), the tempo map ([spec](spec/tempo-map.md)), latency compensation, sends, the preview voice ([ADR 0011](decisions/0011-preview-and-added-audio.md)), live notes and audio input ([ADR 0017](decisions/0017-inputs-and-latency.md)) and the cue bus. Main types: `Renderer`, `Controller`, `PluginProcessor`. Never touches files and never loads a plug-in. | model, stretch, dsp, device-api, device-registry, device-sdk |
| `media` | Finds and decodes a project's audio (symphonia, on all cores) into an `AudioPool`; reads what a browser file is (`info`), MIDI files (`midi`), loop tempo (`tempo`) and Stems files (`stems`, [spec](spec/stems-format.md)); writes what an export renders (`write`: WAV, FLAC, Ogg Vorbis, MP3; [export](spec/export.md) › Formats). | engine, project-file, mp3, devices/compressor, devices/limiter |
| `mp3` | MP3 encoding through LAME 3.100's encoder, vendored as C source in `third_party/lame/` and compiled into a static library by the build ([ADR 0029](decisions/0029-mp3-encoder.md)): the settings, the encoder behind a safe API that owns and frees the C handle, the stream's blocks and flush, the LAME tag frame, and putting the finished file's bytes together. Never touches files; encoding runs on an export's path only, never in the audio callback. One of the three crates allowed `unsafe`. | nothing (cc) |
| `library` | The browser's library ([ADR 0012](decisions/0012-file-library.md)): the folders the user added, an index of their samples, MIDI files, presets and projects kept on disk and up to date (checked at start, then watched with `notify` or polled), search, pins, recent items and colour tags in `browser.json`; finding files by name in the index or by walking a folder (`find`, for missing files). | media |
| `soundmap` | The sample map ([spec](spec/sample-map.md), [ADR 0013](decisions/0013-sample-map-analysis.md)): features of samples (MFCC, spectral shape, envelope, pitch), analysed on background threads and cached per library folder; layouts of a scope by PCA and t-SNE, kept; grids; kinds from names or guessed from neighbours; similar items. The session holds one (`Projects::sound_map_mut`) and runs the `soundmap.*` commands on it; the window shows its maps. | media |
| `test-support` | What tests across the workspace build again and again, written once ([#764](https://github.com/suhina-daw/suhina-daw/issues/764) › 8): `ProjectBuilder`, `write_wav`, `sine`, `peak` and `rms`, and the comparison with golden files (`SUHINA_BLESS=1` rewrites them). A dev-dependency only. | model |
| `separation` | Stem separation ([spec](spec/stem-separation.md), planned in steps): resampling, windowing and adding back a model's output (`resample`, `chunk`, `shape`) around a `Backend`; `BandSplitter` runs the pipeline in tests. No model, runtime or commands yet. | nothing |
| `theme-package` | Theme packages, `.theme.suhina` ([spec](spec/theme-skins.md#theme-packages)): a theme's folder as a zip with `manifest.json` naming the theme and each file's SHA-256 sum; packs a folder, reads a package and checks it (paths, sizes, sums, a `theme.css` that parses, SVG, PNG and JSON that are what they say) before anything is written. The app's export and import and `suhina-cli theme` use it. | style |
| `update` | Updates ([spec](spec/updates.md)): finds the nightly release or the test builds on GitHub (`github`, through the system's `curl` in `net`), compares versions (`version`), and downloads, checks (SHA-256) and installs a build in place of the running one (`install`). | nothing |
| `session` | An open project with its undo history (`Session`) and the open projects (`Projects`, [ADR 0007](decisions/0007-several-open-projects.md)); the command registry and every command ([ADR 0002](decisions/0002-commands.md), [commands](commands.md)); the `Player` that connects a session to an engine; the audio and MIDI choices (`AudioSystem`, `MidiSystem`); the running plug-ins (`rack`, [plug-in hosting](spec/plugin-hosting.md)); the browser's library and sample map; missing files; backups ([spec](spec/backups.md)). | model, engine, media, library, soundmap, als, project-file, plugins, device-api, device-registry, device-sdk, devices/fm-synth |
| `automation` | The MCP server over the open projects, file roots for agents (every path a command reads is checked against the read root), the user's browser library, offline WAV rendering ([`mcp.md`](mcp.md)); the desktop app's endpoints, a named pipe or Unix socket per build: one instance per build, and agents working on the window's open projects through a job queue the window runs between frames ([ADR 0028](decisions/0028-mcp-in-the-running-app.md)). | session, engine, media, library |
| `output` | The audio device: lists hosts and devices (cpal: WASAPI, ASIO, ALSA, JACK, CoreAudio; WASAPI exclusive through `wasapi`) and plays a `Renderer` that renders ahead on a real-time thread into a lock-free ring the device reads, with the main pair and the cue ([spec](spec/cue-output.md)); opens the chosen input ([ADR 0017](decisions/0017-inputs-and-latency.md)). `open_chosen` gives a `Player`; `DeviceOutput` is the session's `AudioSystem`. The only code that opens an audio device. | engine, session, realtime |
| `midi` | MIDI controllers ([ADR 0017](decisions/0017-inputs-and-latency.md)): lists the system's MIDI input ports and opens them with `midir` (ALSA sequencer, WinMM, CoreMIDI); `parse` turns note on and off, the sustain pedal, every other control change, pitch bend and channel pressure, and all-notes-off, on every channel, into the engine's `LiveEvent`s (#812), and the driver's callback hands them to the session's `LiveInput`. `Midi` is the session's `MidiSystem`. The only code that opens a MIDI device. | engine, session, plugins (the trace log) |
| `ui` | The user interface ([ADR 0009](decisions/0009-ui-toolkit-and-look.md)): the main window with the browser ([spec](spec/browser.md): `browser`, `browser_view`, and `drop` for dragging into the views), the Launcher and Arrangement views (the Launcher view's channel strips in `mixer_strip`; the track colour palette and colour areas in `colour_picker`; the Arrangement's automation lanes in `automation_lanes`; `nesting` for groups inside groups: folding them and moving tracks between them), the detail area (device chain, clip properties, embedded plug-in editors, the piano roll in `piano_roll`), the sample map, the missing-files dialog, the Settings dialog and the updates (`app/updates`: the start panel's *New version available* line and Help › Check for updates…, [spec](spec/updates.md)), drawn with egui from the project and the `LauncherMonitor`; every change is a command. `memory-stats`
reads the program's RAM for the control bar and the plug-in hosts' reports theirs, which `plugins::host_memory` adds together. Design tokens in `theme.rs`; the colours are the theme in use (`themes.rs` keeps the list). No windowing or GPU code, so tests drive it headless (egui_kittest). | engine, session, plugins, library, media, output, soundmap, device-api, device-registry, device-sdk, update |
| `suhina-cli` | The command line: render, play, mcp (with `--audio`, the MIDI inputs too). | all of the above |
| `suhina` | The desktop app: opens and makes projects, runs `ui` in an eframe window with `output` playing, starts the browser's library, shows the system's file dialogs (`rfd`), keeps the settings, keymap and theme files, opens the MIDI inputs, builds the macOS menu bar (`mac_menu.rs`), draws see-through backgrounds where the GPU allows (`surface.rs`), and lets agents work on the open projects ([ADR 0028](decisions/0028-mcp-in-the-running-app.md)). | engine, session, media, library, plugins, output, midi, ui, update |
| `dsp` | Signal-processing blocks for devices: conversions and smoothing, followers, filters (biquads, state-variable, first-order, crossovers), LFOs with tempo sync, waveshapers, oversamplers, delay lines and allpasses, a resampler and IR convolution. Real-time safe, panic-free on any input ([ADR 0004](decisions/0004-built-in-devices.md) › 1). | nothing |
| `device-registry` | Every built-in effect, instrument or MIDI effect by its stable id: makes the device a project's `DeviceKind::Builtin` names, lists them, and gives its panel (`panel`: its parameters from `DeviceInfo` and its layout, or the generated list; ADR 0021 › Amendment: device panels). | device-api, device-sdk, devices/* |
| `device-api` | The `Device` trait, `DeviceInfo` (an effect, an instrument or a MIDI effect) and `ParamInfo` (plain values in their unit, conversion to knob travel), `NoteEvent` for instruments and MIDI effects (with the clip's `MusicalScale`), and the contract tests every device runs (`conformance::check`, with note checks for instruments and event checks for MIDI effects). | nothing |
| `device-sdk` | Builds, validates, serializes and prepares data-only `.sd` effect, MIDI-effect and instrument graphs ([ADR 0021](decisions/0021-device-graph-files.md), [format](spec/sd-device-format.md), [nodes](spec/sdk-nodes.md)): the node catalog, polyphonic voice and grain graphs, generated parameter UI and custom panels ([spec](spec/sdk-custom-ui.md)). Graphs run over bounded buffers allocated when prepared, sample-accurate for parameters and MIDI. No executable code in `.sd` files. | dsp, stretch, device-api, serde, serde_json, base64, image (PNG/JPEG) |
| `ipc` | Audio blocks between the engine and a plug-in host process: a shared-memory region per instance with the block's events and followers, a doorbell per direction and a deadline; `AudioSource`, a decoded file mapped read only for an ARA plug-in; memory the system frees when the last holder ends (`shm`, `sweep`). One of the two crates allowed `unsafe` ([ADR 0006](decisions/0006-plug-in-hosting.md) › 3, 4, 6; [protocol](spec/plugin-protocol.md)). | nothing |
| `realtime` | Real-time priority for the threads that make sound (`RealTime::join`): MMCSS on Windows, `SCHED_FIFO` or rtkit on Linux, the highest priority elsewhere. Says what it got, never fails, and paces a render thread under rtkit's limit ([plug-in hosting](spec/plugin-hosting.md) › Block sync and the deadline). No `unsafe`. | nothing |
| `plugins` | The engine's side of plug-in hosting ([ADR 0006](decisions/0006-plug-in-hosting.md), [plug-in hosting](spec/plugin-hosting.md)): `Plugin::start` loads an instance into a host process and returns a control half and a real-time audio half (`PluginAudio`); the JSON control `protocol` ([spec](spec/plugin-protocol.md)); crashes, hangs and restarts; `scan` and `scan_pool` for CLAP and VST3; VST2 replacements (`matching`); `offline::render`. | ipc |
| `plugin-host` | The `suhina-plugin-host` executable: several instances per process, each with its own link and audio thread, and joint blocks; CLAP (`clack-host`) and VST3 (`vst3`) plug-ins with notes, parameters, state, latency and editors, floating or embedded; ARA documents ([ADR 0033](decisions/0033-ara-hosting.md)); scan workers; test plug-ins (`test:*`). One of the two crates allowed `unsafe` ([plug-in hosting](spec/plugin-hosting.md)). | ipc, plugins, realtime, clack |
| `devices/utility` | The utility device: gain, balance, width, mid/side, mono, bass mono (with listening to the low band alone), phase inversion, channel selection, DC filter, mute. | dsp, device-api |
| `devices/auto-pan` | The auto-pan device: an LFO (six shapes, shaping, inversion) that pans or, with no stereo phase, does tremolo; free in hertz or synced to the song position. | dsp, device-api |
| `devices/dj-eq` | The DJ EQ: low, mid and high bands with gains and kills, movable crossovers at 24 or 48 dB/octave; with every gain at 0 dB the sum keeps a small dip at the low crossover, as the measured three-band EQ does ([spec](spec/dj-eq.md)). | dsp, device-api |
| `devices/eq` | Eight-band parametric EQ: cuts at 12 or 48 dB/octave, shelves, bells and a notch on trapezoidal state-variable filters with the cookbook responses, two curves for Stereo, Left/Right or Mid/Side, Scale and Adaptive Q ([spec](spec/eq.md)); no latency. | dsp, device-api |
| `devices/vinyl-distortion` | Vinyl distortion: tracing (even harmonics in a band), pinch (odd harmonics, out of phase between the sides or mono) and crackle; shaped at twice the sample rate, 31 frames of latency. | dsp, device-api |
| `devices/overdrive` | Pedal-style overdrive: a band into a biased, asymmetric soft curve at twice the sample rate, then tone, dynamics, dry/wet and output; 31 frames of latency. | dsp, device-api |
| `devices/saturator` | Saturator: eight curves (Analog Clip to a six-amount waveshaper) after a drive, Color filters before the curve and their exact inverse after it, a post clip, output and dry/wet, at four times the sample rate through two IIR half-bands ([spec](spec/saturator.md)); 4 frames of latency. | dsp, device-api |
| `devices/compressor` | Compressor: stereo-linked feed-forward, peak detector on a high-passed side-chain, soft knee, attack and release in decibels, input and output gain, dry/wet for parallel compression; no latency. | dsp, device-api |
| `devices/limiter` | Brickwall peak limiter: threshold as a drive into the ceiling, release, 1.5 ms lookahead (a sliding minimum and a moving average), so no sample comes out over the ceiling; the lookahead is its latency. | dsp, device-api |
| `devices/delay` | Stereo delay: one line per side read with cubic interpolation, a Butterworth band-pass inside the feedback loop, times synced to the tempo or free, an LFO on the time and the band, Repitch/Fade/Jump, ping-pong and freeze ([spec](spec/delay.md)); no latency. | dsp, device-api |
| `devices/reverb` | Algorithmic reverb: input band, Live's early-reflection taps and their families, a measured feedback delay network of 4 to 10 lines with allpasses and damping in the loop, chorus, freeze and a stereo rotation ([spec](spec/reverb.md)); no latency. | dsp, device-api |
| `devices/pitch` | The pitch MIDI effect: transposes notes by semitones or by steps along the clip's scale, and keeps them in a window of keys by blocking, folding or limiting them; note-offs follow their note-ons ([spec](spec/pitch.md)). Events only, no latency. | device-api |
| `devices/fm-synth` | The FM synth, an instrument: six operators, 32 algorithms, sixteen voices; the engine is a port of MSFA (the engine in Dexed, Apache-2.0) that matches it sample for sample; reads and writes DX7 voice files ([spec](spec/dx7-voice-format.md)). | device-api |
| `devices/nam-amp-modeler` | The NAM Amp Modeler: a `.nam` model through `nam-rs` (MIT) inference, one instance per side, with the input trim, an EQ (shelves and a mid peak), an HQ toggle (the A2 width dial's widest sub-model), a trim delay and an output trim; a rate mismatch runs both ways through `dsp`'s resampler. The fixed schedule and the one-chunk trail make the sound independent of the host's block sizes ([spec](spec/nam-amp-and-cab.md)). | dsp, device-api, nam-rs |
| `devices/nam-cab-loader` | The NAM Cab Loader: a `.wav` impulse response through `dsp`'s IR convolution, per side, with a phase invert, a high pass, bass/mid/treble cuts, a trim delay and a volume; the same fixed schedule ([spec](spec/nam-amp-and-cab.md)). | dsp, device-api, hound |

Output startup, shutdown fades and diagnostic publication are described in
[the output lifecycle spec](spec/asio-host.md#audio-output-startup-and-diagnostics).

The `output` crate owns the ASIO driver and routes native panel requests through its
owner thread. Its host initialization, enumeration and replacement contracts are in
[the ASIO host spec](spec/asio-host.md). Superseded plug-in starts are retired on a
session worker so unloading does not block the UI.

### Inside the `ui` crate

The `ui` crate is sorted into folders, one per view, and two folders the views share, so that a
change to one view touches the files of that view and parallel pull requests seldom meet:

```text
crates/ui/src/
  lib.rs         the modules, and the types the app takes (App, Status, View)
  app.rs, app/   the window: its state and frame in app.rs; in app/ its jobs, one file each
                 (actions, keys, menu_bar, control_bar, tabs, detail_area, browser_panel, drops,
                 plugin_host, audio_settings, exports) and its dialogs (project, loading,
                 missing files), the start panel and the splash
  arrangement/   the Arrangement view: arrangement_view (with its parts in arrangement_view/),
                 timeline, automation_lanes
  session/       the Launcher view: session_view, mixer_strip, slot_launch, launch_keys,
                 record_controls
  detail/        the detail area: detail_view (device chain; its model, parameters, expanded panel,
                 clip properties and group panel in detail_view/), device_canvas,
                 follow_view, the piano roll (piano_roll, its parts in piano_roll/, and the piano_roll_*
                 files) and midi_tools
  browser.rs, browser/   the browser's state (browser.rs) and its views: browser_view,
                 sample_map, sample_map_panel
  settings.rs, settings/ the stored settings (settings.rs, view_settings, keymap, workflow) and
                 the Settings window (settings_view, settings_rows, themes)
  interaction/   what views share: drop (dragging between views), pick, menus, nesting, grid
                 (the tracks and slots as the views see them), colour_picker, empty_space, status,
                 timeline_grid (the lines and snapping of the arrangement, piano roll and automation)
  kit/           what every view draws with: theme, widgets, meter, motion, logo, waveform, scroll,
                 surface (backgrounds that set the window's pixels)
  demo.rs        the demo project the app opens with `--demo` and the tests draw
```

The rule: a view uses `interaction/`, `kit/` and the stored settings, never another view's
files. `interaction/` uses `kit/`; `kit/` uses neither. Only `app` puts the views together.
Something two views need, such as dragging a clip from the Launcher view to the Arrangement view,
goes through `interaction/` (the dragged data in `drop`) and the session's commands; a helper two
views share moves into `interaction/` or `kit/`. A view module is declared in its folder's module
file (`arrangement.rs` for `arrangement/`), and code names it by its full path
(`crate::arrangement::timeline::Timeline`).

#### Private background images

The UI background coordinator owns the active texture and one bounded worker. Separate
source, filter and placement modules decode the local image, apply alpha-correct blur,
and map it into display coordinates. Cached source pixels survive effect changes.
Obsolete workers cannot publish into a newer request. Main-window surface opacity is
separate from the Settings panel material; neither changes text opacity. This is an
internal boundary, not an extension host or a new dependency of the audio engine.

An active private image keeps background fills in ordinary blend mode, including on hosts
capable of desktop transparency. Liquid captures release shared texture handles and renderer
registrations when the next capture no longer uses them, bounding retained image history.

#### Native theme styles

The UI kit owns static SVG rasterization, scoped theme values and the native style
adapter over `suhina-style` (ADR 0031): Suhina's own parser, selectors, cascade layers,
variables, media queries, Taffy layout and motion, with no browser engine. The host
supplies bounded role trees; themes supply CSS, never markup or scripts. The adapter has
no network or file access and caches computed class contexts on the UI thread.
For what is drawn in bulk (notes, clips), the resolver offers a leaf cache that styles by
parent and identity, once per distinct identity rather than once per note. The UI does not
draw through it yet.
A retained scope tree carries project identities and native interaction states; changed
snapshots publish computed values on the following repaint. Connected surfaces paint
bounded cached gradient ramps/spatial textures and inset/outset shadow textures; scoped
guards apply authored values to ordinary egui controls without changing widget identity.
The toolbar and optional workspace panel layout consume Taffy's rectangles for both
native controls and input clipping. CSS-owned browser/detail sizes bypass native
splitters while preserving saved native sizes. Token-level CSS validation bounds source
size and nesting and reports known unsupported effects without deleting source.
Computed targets persist for an unchanged scope structure. State changes start native
transitions from the painted value, and egui's clock samples transitions and keyframe
animations for connected paint properties. Active motion
requests repainting at 16 ms and respects the global animation/high-contrast settings;
geometry animation is not yet consumed by native panel layout.
No theme work runs on the audio thread. The desktop theme-file service owns portable
resources: one folder per theme with its author's `theme.css` (read, never written),
`variants/*.json` (values for its settings, written by the settings editors) and Suhina's
`layout.css`, with the one-time, validated migration of earlier themes. Settings are registered
custom properties (`kit/theme_vars.rs`); a variant's values apply in their own cascade tier,
between the theme and device panel sheets. Details and current adapter limits are in
[ADR 0031](decisions/0031-style-sheets.md) and the [skin spec](spec/theme-skins.md).

## Crate names, folders and source files

Every package is named `suhina-*`; the directory has no prefix. The project had another name
until October 2026 (README); nothing new may use it, in names, text or code.

| Kind | Directory | Package (`Cargo.toml`) | In Rust code |
|---|---|---|---|
| Library crate | `crates/<name>` | `suhina-<name>` | `suhina_<name>` |
| Built-in effect or instrument | `devices/<name>` | `suhina-device-<name>` | `suhina_device_<name>` |
| App (binary) | `apps/<name>` | `suhina` or `suhina-<name>` | |

A new crate inherits the workspace settings (`version.workspace = true`, `edition`, `license`,
`rust-version`, `[lints] workspace = true`), is added to `[workspace.dependencies]` in the root
`Cargo.toml` (`suhina-<name> = { path = "crates/<name>" }`), and other crates depend on it with
`suhina-<name>.workspace = true`. Add its row to the crate table above, in its layer, and to the
diagram. Environment variables start with `SUHINA_`. CI runs `python scripts/rename_suhina.py
--check`, which fails when the old name is left anywhere in the tree (links to the repository and
lines marked `rename_suhina: keep` excepted). A branch started before the rename catches up as the
script's docstring describes.

Beside the crates:

- `docs/`: `architecture.md` (this), `decisions/` (ADRs; read 0001 before changing the model, 0004
  before a device), `spec/` (clean-room specs written from observation), `design/` (the UI design
  brief, component measurements and mockups, ADR 0009), `research/` (status analysis of our own
  project: parity against another DAW's public manual, ADR 0016, and its figures), the generated
  `INDEX.md` and `commands.md`, and `works-now/`;
- `assets/brand/`: the logo as SVG (`suhina-logo.svg`, drawn on the splash by
  `crates/ui/src/kit/logo.rs`; `suhina-mark.svg`, the wave alone) and `suhina-icon.png`, the app's
  icon;
- `third_party/`: vendored source, each folder with a `VENDORED.md`: the ASIO SDK's host part
  (`asiosdk`, GPL-3.0, built with `asio`), LAME 3.100's encoder (`lame`, LGPL, ADR 0029), `cpal`
  0.18.2 and `asio-sys` 0.4.0 with local patches (`[patch.crates-io]` in `Cargo.toml`), and the ARA
  headers (`ara-api`);
- `scripts/`: research helpers (extracting and diffing `.als` files), the parity measurement
  (`parity/`, reading a manual the user owns; its output is never committed), `docs.py` (the docs
  index and checks), `release_notes.py` and `discord_post.py`, `agent_post.py` (an agent's issue
  or comment, [`workflow.md`](workflow.md) › Posting as the bot).

**Licence header.** Every source file (`.rs`, `.py`, `.sh`, `.ps1`) starts with the licence
header, after a `#!` line when there is one, then an empty line:

```text
// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later
```

(`#` instead of `//` in scripts). Run `python scripts/license_header.py` after adding a file; it
adds the header where it is missing and nothing else, and CI runs it with `--check`. Ported code
keeps its original notice and gets only the SPDX line of its own licence; the script's docstring
lists those files and what else is left out (`third_party/`, fixtures, generated files).

## From a file to the speakers

An `.sd` device enters through `device.addSd` or a browser drop. The session reads and validates
the file off the audio thread, then stores its canonical JSON in the device state and its parameter
defaults in project state. Project saves embed both. Renderer construction compiles the graph into
a fixed processor off the audio thread; the render path runs its supported nodes in the same chain
as built-in effects; MIDI effects transform note events before the track instrument, and instrument
graphs render audio from those events. The detail panel reads the embedded graph to draw its visual node flow and
parameter controls. The UI caches custom canvases and bounded textures; image decoding and drawing
expressions run only on the UI thread. Custom controls dispatch parameter commands, with a shared
gesture id grouping both XY axes into one undo step. See [ADR 0021](decisions/0021-device-graph-files.md).

```text
.als ──als::read_file──> Project ──media::load_project_audio──> AudioPool
                            │                                       │
                            └──────────> Renderer::build      <─────┘
                                              │ (all allocation happens here)
                                              ▼
                      render thread: Renderer::process(block), forever, ahead of the device
```

1. **Import.** A format crate reads the file into a `Project`. Anything it does not understand
   becomes a warning; the project still opens.
2. **Load audio.** `media` decodes every file the project's audio clips use. The result, an
   `AudioPool`, holds the samples in memory, so playback never reads the disk. The session
   keeps it; a file added later (`clip.addFiles`) is decoded off the audio thread, joins the
   pool and reaches the engine with the next graph (`Controller::set_audio`, ADR 0011).
3. **Build the renderer.** `Renderer::build` (the project and its `RenderInputs`) turns the project into flat, sorted lists the
   audio thread can walk without searching: note events, audio events, one stretch voice per
   warped clip placement (two stretchers each, so a fresh one takes over before every strong
   onset and transients stay on the grid; ADR 0003), the prepared launcher clips, the
   per-track gains (group and master chain included) and a device chain for each track whose
   channel holds built-in effects and instruments, made through `device-registry` and prepared (ADR 0004;
   built-in MIDI effects change the chain's note events before its instrument, and each note
   carries its clip's scale for them; MIDI effects without an instrument after them get the
   engine's sine tones as one, `sine.rs`, ADR 0004 › 14); a
   group with devices is a bus that sums the chains of its tracks before its own run (ADR 0004
   › 10, `buses.rs`), a return track a chain that sums the sends into it (ADR 0004 › 12,
   `sends.rs`), with
   a slot for each plug-in device the session knows of (ADR 0006), and the delay lines of
   latency compensation. Every buffer the renderer will ever need
   is allocated here, except the launcher clips' stretchers, which the stretch thread builds
   afterwards (ADR 0003 › decision 7). The player takes the stretch worker
   (`Renderer::take_stretch_worker`) and runs it on its own thread, with the launcher's
   preparer (`Controller::run_stretch_worker`).
4. **Play.** `Renderer::process` fills one block of interleaved stereo
   (`Renderer::process_with_cue` the cue bus beside it). It never allocates,
   locks, blocks or does I/O. Offline rendering (`render_offline*`, `render_launches`, and
   `render_track` for one track's clips without the mix) calls the
   same `process` in a loop, so an offline render is the test oracle for playback, and the output
   does not depend on the block size.

## Devices from the SDK

Built-in devices are native Rust crates; `.sd` devices are data-only graphs that `device-sdk`
prepares ([ADR 0021](decisions/0021-device-graph-files.md), [format](spec/sd-device-format.md),
[nodes](spec/sdk-nodes.md)). Both run in the same chains ([device chains](spec/device-chains.md)).

Native stereo sources and effects use the device-neutral `NativeNode` contract. The graph
interpreter stores prepared trait objects; a separate factory and implementation adapters
own concrete kernel selection and off-thread resource binding. Rendering, span preparation,
note events and reset remain allocation-free. Shared modulation math lives outside concrete
instruments.

The SDK also preserves optional editor geometry (node rectangles, wire bends and port
anchors). This remains authoring data in the embedded JSON; it is validated off the audio thread
and does not add processing nodes or a runtime/UI dependency. The current node list does not
render this layout. See the [layout contract](spec/sd-device-format.md#visual-connection-layout).

### The sampler

The sampler in `devices/sampler` uses the shared SDK sample runtime and SDK UI
widgets. A sampler's versioned source path is kept in `Device.state`; media loads
it into the same audio pool as clips. Chain preparation installs immutable decoded
source audio off the audio thread. Dynamic manual slice maps persist in the
device's `suhina/sample_slices` extension, with legacy fixed parameters as fallback.
The command `device.setSampleSlices` edits the list with snapshot undo. Preparation
validates immutable maps, and graph adoption swaps their Arc storage without
allocating or freeing on the audio thread; held voices keep their current bounds.
Versioned source corrections are decoded into the Device API's immutable `SampleEdits`
resource off the audio thread. Chain and group adoption exchange its Arc without
freeing the old data. The sampler merges candidates and pins without allocating
when selecting a MIDI slice; UI and commands share the same correction format.
Source identity participates in graph transfer,
so replacing a sample cannot keep an old voice reading the previous source.
The sample instrument publishes up to 32 independent voice positions through the
Device API and fixed atomic meter slots; the UI polls them without audio-thread
allocation or locking.

### SDK panel widgets

The device SDK owns reusable, data-only UI widget descriptions and their drawing
programs (`crates/device-sdk/src/ui/widgets.rs`). Each widget is one control bound
to parameters; device examples compose these controls. The UI prepares and caches
the drawing programs per canvas alongside its image resources, then evaluates them
against current parameter values. Widgets use the existing command/undo path and
do not add DSP graph nodes or run on the audio thread.

### Shared samples, kernels and parameter links

Sampler read-head positions travel through the engine meter monitor as atomic values. The UI reads them without accessing audio-thread device state.

Host-owned AudioData implements AsRef<[f32]> so sample instruments share decoded PCM through Device::load_shared_sample without a copy. SDK transient analyses are shared by source identity and format in an off-thread weak cache, which retains no unused source or analysis. Device::set_context receives each block context before its MIDI events, including events at frame zero.

Registered zero-latency audio effects used as `native_effect` graph kernels resolve in
`device-registry::compile_graph`. Track chains and nested device groups use this same factory;
the session validates it before insertion. The SDK interpreter knows only `NativeNode`.
Prepared sample PCM and checked read regions are independent SDK resource data; file loading,
decoding, binding and retirement remain host responsibilities outside the render thread.

SDK parameter edits can expand conditional peer links into several parameter messages.
The session stores and validates the complete command, records one undo gesture, then sends
every message to the player. If a message cannot be sent it updates the complete graph.

### Prepared sample and component nodes

SDK implementations are grouped by type under `device-sdk/src/nodes/`; the scheduler owns only
generic NativeNode input/output, event, parameter and completion interfaces. Sample resources are
immutable Arc-backed PCM supplied by the host off-thread. Resource keys persist in .sd and resolve
through PreparedSamples; the SDK does no file decoding. Missing keys fail preparation. Replacing a
binding prepares a new graph; existing voices retain their old Arc until graph retirement off-thread.
VoiceGroup, GrainPool and versioned Component definitions contain private precompiled graphs, bounded
by nesting and aggregate state budgets. Their states are never constructed or freed during rendering.
Shared envelope stages, RNG, grain windows/scheduling, source analysis and stretch processing serve
both the existing sampler and graph nodes. Existing Delay and Reverb implementations are unchanged.

SDK sampler graphs receive decoded immutable `SampleAsset` resources from the host while
track chains are prepared. The engine maps an SDK device's model sample reference to binding
`sample`; it does not decode files. Graph state and source references persist independently,
so relocation and relinking do not rewrite graph definitions. `device.addSd` checks the same
resources before insertion, and MCP confines both graph and source paths.

## Threads

| Thread | Runs | May allocate, lock, do I/O |
|---|---|---|
| Main / session | The CLI, the desktop app's window (egui, one frame at a time), or the MCP server's tokio runtime. Owns the `Session` (the MCP server: all open `Projects`), runs commands, imports and loads audio, builds the `Renderer` and, after each edit, a new graph for it (ADR 0005). In the desktop app an attached agent's commands run here too, as jobs between frames (ADR 0028). | yes |
| Background image | At most one short-lived `background-image` worker per UI app decodes, bounds and blurs the user-selected PNG. It returns pixels through a bounded channel; the UI uploads them once and shares the cached image with the liquid offscreen renderer. It never captures desktop pixels or touches the audio graph. | yes |
| Agent endpoints | The desktop app's `suhina-agents` tokio runtime (two threads): holds the build's endpoints, reads "open these files" from later starts and serves attached agents' MCP; it never touches a project, only hands jobs to the window. | yes |
| Render | `suhina-render` in `output`, at real-time priority (`realtime`; it says what it got before the device starts, and the first stream in a process adds a notice when real-time scheduling was refused, since playback may then drop out under load): `Renderer::process_with_cue` in blocks of the buffer size the user chose (`engine_block`: 64 … 4096 frames, 1024 when none is chosen, fitted to the sizes the device allows, `SUHINA_ENGINE_BLOCK` to try another in tests), ahead of the device into a lock-free ring (`rtrb`). It keeps four blocks queued, or a device buffer and a block when that is larger; while a track is armed, one block or one device buffer, so a key on a MIDI controller sounds sooner ([ADR 0017](decisions/0017-inputs-and-latency.md)). Every plug-in device's deadline counts from this block, so it follows the buffer size chosen. | **no** (AGENTS.md rule 3) |
| Audio | The device callback (cpal's thread; in WASAPI exclusive mode `suhina-wasapi`, an event loop on the device's buffer): copies from the ring (silence, counted as an underrun, for what is not there yet), the main and cue pairs to their channels, sample conversion, setting atomics, measuring the latency from the frames queued and the device's own delay. | **no** (AGENTS.md rule 3) |
| Stretch worker | One thread, owned by the `Controller`, running a `StretchWorker` and a launcher `Preparer` per graph: renders the arrangement's stretched clips 0.5 s ahead into a lock-free ring per clip, and builds the launcher's stretch voices (the stretcher and its buffers, 1 to 4 ms of allocation each) one per round, handing them to the launcher through an `rtrb` ring (ADR 0003). A new graph's worker and preparer join it, an old one leaves once its graph is freed. | yes: it builds voices and joins graphs; never locks |
| Audio owner | In the desktop app and over MCP, one thread per open project opens and holds the output stream (`output::open_chosen`) and closes it when the session drops its `Player`. When another output is chosen (`audio.setOutput`) it stops the device and the render thread and hands the renderer's plug-in processors (`Renderer::into_plugins`) to the next owner, which reconfigures them through bounded host control requests when the rate changes, updates their latencies and builds its renderer with the same instances. The session adopts the acknowledged rate so its rack sync does not unload them. A failed reconfiguration reports an error and does not start a mixed-rate graph. Streams are never opened or closed on the render or device thread. | yes, but does nothing while playing |
| Device watch | While the output plays in place of a chosen device that was not there (`OutputInfo::fallback`), `Projects::check_output` asks every 3 s whether it is back (`AudioSystem::chosen_present`); `output` looks on a short-lived `suhina-device-watch` thread (the chosen host's output devices, names only) and keeps the answer for the next ask, so the frame never waits for the device list. When it is back the output is opened on it again, as `audio.setOutput` does. | yes |
| MIDI input | `midir`'s own thread per open port (the ALSA sequencer's, WinMM's or CoreMIDI's callback): parses each message and sends its note through the session's `LiveInput` (a mutex around the playing engine's live-note sender) into the engine's lock-free queue ([ADR 0017](decisions/0017-inputs-and-latency.md)). Ports are opened, closed and looked at again on the main thread. | yes, but never the audio thread's locks: it only pushes into the queue |
| Audio input | The input device's callback (cpal's thread, on the output's host), when an input device is chosen: converts every channel, up to 64, to `f32` and writes whole frames into a lock-free ring that the render thread reads a block at a time ([ADR 0017](decisions/0017-inputs-and-latency.md)). | **no** (AGENTS.md rule 3) |
| Recording | One thread per engine in `session` (`suhina-record`), started when a player is attached: empties the engine's recording rings as they fill (the notes the armed tracks play, their input in chunks), hands the notes to the session and writes each audio take into its WAV file in `Recorded/`, so neither the render thread nor the window waits for a disk ([spec](spec/recording.md)). It ends when its engine is gone and nothing is left to write. | yes |
| Decode workers | Scoped threads in `media` while a project loads; in the window, a thread per drop (`suhina-drop`) that decodes its samples before `clip.addFiles` runs (ADR 0011). | yes |
| Missing files and plug-in scans | In the window, `suhina-find-files` searches the index and folders for a project's missing files, and `suhina-plugin-scan` runs a plug-in scan, so the frame never waits. | yes |
| Library | One thread in `library` (`suhina-library`): checks the browser's folders against their saved indexes, reads the ones that changed, and takes the file-system watcher's events (`notify`, its own threads) or polls (ADR 0012). Started by the desktop app and the MCP server only. | yes |
| Theme files | One desktop thread (`suhina-theme-files`, `apps/suhina/src/theme_io.rs`) owns theme persistence, conflict checks, exports/imports and live-reload polling. At most one batch is in flight; the window coalesces later saves without crossing file-operation barriers, previews changes immediately and polls replies without waiting. Normal exit drains pending saves. | yes |
| Backups | One thread in `session` (`suhina-backup`), started with the first backup: turns each project copy the window's thread made (`Projects::back_up`, which waits at most a second for the plug-ins' state, as a save does) into JSON, writes it through a temporary file, cleans up old ones and keeps the journal ([spec](spec/backups.md)). | yes |
| Browser files | One thread in the window (`suhina-browser-files`): reads the length, tempo and key of the files the browser shows, and decodes the selected sample for its waveform and preview. | yes |
| Sample analysis | `soundmap-analysis` threads, half the cores, while a `SoundMap` has files queued; they decode and analyse samples and send the results back. Started by the desktop app and the MCP server only. In the window, a `soundmap-layout` thread per new layout computes it (t-SNE) so the frame never waits. | yes |
| Plug-in host processes | Not threads of Suhina's process: `suhina-plugin-host` processes, by default one per plug-in file with every instance of it inside, each instance with an audio thread of its own (ADR 0006). Started by the session through `plugins`, on short-lived threads (`suhina-plugin-start`; `suhina-plugin-control`, `-stop` and `-restart` for editors, state, stopping and restarts), so the window never waits for a host process. The render thread hands every chain's block to its first plug-in, then takes each answer as it comes and runs that chain on to its next plug-in at once, while the other chains' blocks are still out, at most until the block's deadline (the chains no chain plays into; a bus runs after them; #151), so the deadline is shared by the slowest chain rather than by all its plug-ins in turn, and a chain never waits for another chain's plug-in. Consecutive plug-ins of a chain in the same host process go as one block: the first one's host audio thread runs them all, one after another, and answers once (`Link::send_joint`). The hosts' audio threads run at real-time priority, as the render thread does (`realtime`). | the host's own business; a crash or hang there costs Suhina missed blocks only |

The main thread and the audio thread share nothing mutable. They talk in two directions:

- **To the audio thread:** a `Controller` holds the producer end of a lock-free queue (`rtrb`)
  of `LaunchCommand`s (play, stop, set the play start marker, launch clip or scene, stop column,
  quantisation, back to arrangement). `process` drains the queue at the start of each block. A second queue carries
  new graphs after edits, and a third takes the old graphs back to be freed on the main thread
  (ADR 0005). The preview voice has a pair of its own: what to preview in (a sample's
  `Arc<AudioData>`, a MIDI preview's notes with its fallback instrument, stop), and what it let
  go of back, to be freed on the main thread (ADR 0011). Live notes from MIDI controllers come
  over a queue of their own, from the MIDI driver's thread rather than the main one; the main
  thread arms tracks with a `LaunchCommand` (ADR 0017). Live notes carry the time they were
  played, and the render thread tells the renderer before each block when it renders it
  (`Renderer::set_live_clock`), so each note lands a fixed delay after it was played. The input
  device's frames come over a ring from its own callback (`Renderer::set_audio_input`).
- **From the audio thread:** a `LauncherMonitor` of atomics (running, beat, tempo, what each
  column plays and has queued, the playhead and the play start marker, stretch underruns), read by the `launcher.state`
  command and by `Controller::update` to pick the frame a new graph takes over at; a
  `PreviewMonitor` (whether a preview plays, and where) and a `CpuMonitor` (the audio
  thread's share of wall time spent inside the render call, smoothed over 0.2 s windows
  and kept in per mille; nothing while offline renders run flat out), both of which
  `launcher.state` reports too — the control bar draws the CPU load from it; and a
  `MeterMonitor` (each track's and the main output's peak and RMS level per side and clip
  indicator, measured every block by the `meters` module and stored as atomics; read by
  `mixer.levels` and the window's meters, [spec](spec/level-meters.md)). While recording, two
  rings carry what the armed tracks play out (`Controller::take_recorder`), each piece stamped
  with the beat that was heard when it was played; the recording thread empties them
  ([spec](spec/recording.md)).

## Commands

Export preparation is bounded work across window frames. Attached MCP jobs can remain pending
between frames; a pending job is advanced only once per frame, and cancelled requests release
their preparation state. `project.export` then renders and encodes on a worker without holding
`Projects` or a session lock. The desktop uses the same preparation and render functions. The
host supplies MCP write-folder preferences to `Projects`; no command changes them, and the
automation layer applies its existing canonical-path validation to every allowed write root.

Every user-visible action is a command in `session` with an id, typed parameters and a kind
([ADR 0002](decisions/0002-commands.md)). The CLI, the MCP server and the UI all call
`Session::execute(id, params)`, or `Projects::execute` when several projects are open (it runs
the `project.*` ones that list, switch and close projects, `plugin.*`, `library.*`, `soundmap.*`, `audio.*` and `midi.*` itself, on what belongs to
Suhina rather than one project, and passes everything else to the active project); none of them changes a
project directly.

| Kind | What it touches | Undo |
|---|---|---|
| Read | Reads the project (or the monitor) and returns JSON. | none |
| Edit | Changes the session's `Project`. Validated afterwards; a failed or invalid edit restores the project. | one step, a snapshot |
| History | Undo and redo. | moves between snapshots |
| Transport | Sends a `LaunchCommand` (or a preview, `preview.*`) to the attached `Player`; fails if no engine is playing, except `track.setArm`, which the session keeps and tells every new player. | none |
| Projects | Lists, switches and closes open projects (`Projects`); only the active one keeps its `Player`. | none |
| Library | The browser's folders, search, pins, recent items and colour tags (`library.*`, on the `Library` that `Projects` holds; ADR 0012), and the sample map (`soundmap.*`). A successful `clip.addFiles`, `track.add` or `device.add*` puts what it added among the recent items. | none |
| Plugins | Scans for installed plug-ins, lists them, chooses the folders scans look in and how a plug-in shares host processes, and finds the installed plug-in for each of a project's devices (`plugin.*`, on the `Library` that `Projects` holds; ADR 0006). | none |
| Plugin | Acts on one of the project's running plug-ins: opens or closes its editor, restarts it (`device.openEditor`, `device.closeEditor`, `device.restart`). | none |
| Audio | Lists or chooses the audio output (host, device, sample rate, buffer size) and reports on it (`audio.*`, on `Projects`); reopens the active project's output. Lists the MIDI inputs and turns them on or off (`midi.*`). | none |

**Edits while playing** ([ADR 0005](decisions/0005-edits-while-playing.md)): after an Edit or
History command changes the project, the session calls `Player::update`. The controller builds a
new `Graph` (everything the renderer derives from the project) on the main thread and the audio
thread swaps it in at the start of the next block (0.15 s ahead when stretched arrangement clips
need their voices moved first), carrying over the playhead (on the same beat when the tempo
map changed), the transport, sounding notes (which take their track's new mix), device chains and
the launcher's playing and queued clips. A device parameter is not a new graph: it goes to the
audio thread as a message, applied at the next block (ADR 0005, amendment); a plug-in's goes
to its slot, which sends it to the plug-in as a parameter event.

**Plug-in parameters and automation** (#109, ADR 0006 › 3): each block, before any of it runs,
every plug-in slot plans its parameter events (`engine::plugin_params`): the project's values
for the device when its processor starts, values set while playing, and its automation lanes'
values at their frames while the transport runs. Each span the slot runs hands its share to
the processor (`PluginProcessor::automate`), and the session's `Hosted` merges them with the
block's notes for the host, which applies them sample-accurately.

**Automation of built-in and `.sd` devices and the mixer** (#395, [spec](spec/automation.md)):
`engine::automation` turns lanes into arrangement frames when the graph is built. A chain's
`DeviceLanes` plan each block's parameter changes with the plug-in events, and the chain splits
its built-in and `.sd` devices' blocks at them; `.sd` processors receive parameter changes through
the SDK's allocation-free setters. A track whose mixer has lanes gets a chain (one without devices
when it has none) whose `MixLanes` give its gains frame by frame after its devices. The graph is
built from a copy of the project with those mixer parameters at rest (volume 1, centred, not muted),
so routing and gains need no second path. A manual change to an automated parameter turns its lane
off, which is an edit that builds a new graph rather than a parameter message.

**Plug-ins** (ADR 0006): `Projects::poll_plugins` (the window calls it every frame, commands
call it too) keeps the active project's running plug-ins in line with it. A new plug-in device
is looked up in the catalog and started with its saved state on a thread of its own; once it
runs, its audio half goes to the audio thread as a message (`Controller::send_plugin`) and takes
its slot, and every later graph takes it over from the one before; what it replaces goes back
to be dropped on the main thread. Its latency, once known, makes a new graph with the delays of
latency compensation, and so does a latency it reports while it runs (the host tells the engine
unasked). A plug-in that is not installed is *missing*: it has no slot and its
state stays in the project untouched. Offline renders start the plug-ins first and run the same
renderer without a deadline.

Before quitting, the window advances `Session::poll_plugin_edits` each frame for every open
project together. Two background state reads cover processing and MIDI changes without an
editor or dirty notification. Each project's check shares one second across both rounds;
the window keeps handling events while hosts work, then uses the last known states on timeout.
Periodic background reads refresh closed-editor plug-ins too, for backups; an open ARA editor's
expensive archive waits until the editor closes. Confirmed state changes have a revision of
their own for deciding when another backup is due, and the last completed state stays available
while a control worker is busy.

## Where new code goes

| You are adding | It goes in | Keep in mind |
|---|---|---|
| A field the project needs to store | `model`, then every reader and writer that should fill it | Neutral names; read ADR 0001 first |
| Something read from a Live Set | `als`, after it is written down in `docs/spec/` | Clean-room rules; a corpus test |
| Something that makes or changes sound | `engine`, prepared in the renderer's constructor | `process` never allocates; an offline render test |
| A way for the user or an agent to change something | a command in `session` | Typed params, undo, a test in `session/tests` |
| A new kind of file to decode or analyse | `media` | Input sizes are hostile; cap them |
| Something agents need | `automation`, on top of commands | File roots; `isError` for failed commands |
| A view or control in the UI | `ui`, in the folder of its view, drawn from `TrackGrid`-like data built from the project and monitors ([Inside the `ui` crate](#inside-the-ui-crate)) | Dispatches commands; never edits the model directly; sizes and colours from `theme` and `docs/design/` |
| A built-in instrument or effect | a crate under `devices/`, blocks in `dsp` | ADR 0004; `conformance::check` in a test; import mapping in the format crate |

## Planned parts

Each with its issue, or the document that plans it where there is none yet.

- **Projects:** copying clips, tracks and devices between open projects, and switching without
  reopening the audio output ([ADR 0007](decisions/0007-several-open-projects.md) › Not decided
  yet, Consequences).
- **Project folders and packages:** a project as a folder with a manifest, and a package that
  carries its audio ([ADR 0026](decisions/0026-project-folder-and-package.md), proposed;
  [#606](https://github.com/suhina-daw/suhina-daw/issues/606)).
- **Patch devices:** community devices as Pure Data patches run in the plug-in host process,
  with a command channel ([ADR 0008](decisions/0008-patch-devices.md), proposed).
- **Stem separation:** a model, its runtime and the commands
  ([#146](https://github.com/suhina-daw/suhina-daw/issues/146), [spec](spec/stem-separation.md)).
- **Render-ahead for launched clips:** arrangement clips are stretched ahead on the worker;
  launched clips' stretch voices still run on the audio thread.
- **Automation of send levels** ([automation](spec/automation.md) › Not yet).
- **Plug-ins:** hosting VST2 directly ([#1066](https://github.com/suhina-daw/suhina-daw/issues/1066),
  [ADR 0022](decisions/0022-vst2-interface-description.md)); editors on Wayland, and embedded
  ones on macOS ([plug-in hosting](spec/plugin-hosting.md) › Editors).
- **Recording:** overdubbing into a clip that exists, punch in and out, and recording
  automation ([spec](spec/recording.md) › Not yet).
