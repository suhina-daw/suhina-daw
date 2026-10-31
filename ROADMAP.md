# Roadmap

Suhina is pre-alpha. Milestones are in order; each one builds on the previous. Linux is the
primary platform; Windows and macOS are supported.

| | Milestone | Status |
|---|---|---|
| M0 | **Foundation:** workspace, GPL-3.0-or-later, clean-room rules, DAW-neutral model | done |
| M1 | **First sound:** offline renderer and the `.als` reader | done |
| M2 | **Real-time playback:** audio output, `suhina-cli play` | done (verified on Windows) |
| M3 | **Commands and agents:** command registry with undo; CLI and MCP server on the same commands | in progress: commands, undo, MCP, edits heard while playing, and saving projects in Suhina's own format (`.suhina`) work; more commands to come |
| M4 | **Clip launching:** launch quantisation, scenes, stop buttons, follow actions | in progress: launching, scenes, stops, quantisation, follow actions, the metronome, launch modes, legato, per-clip quantisation and launch keys work; launching from live MIDI notes to come |
| M5 | **Audio playback:** imported audio clips with gain, transpose, warping and time-stretching | done (verified by ear on a heavily warped set) |
| M6 | **Plug-ins:** CLAP and VST3 hosting, sandboxed out of process (AGENTS.md rule 5, ADR 0006), and projects with VST2 plug-ins opened through their VST3 versions | in progress: CLAP and VST3 plug-ins load, play in the app and in renders with latency compensation, keep their state and survive crashes in their own process; editors on Windows and Linux X11; still to do: automation lanes in the UI, macOS and Wayland editors ([#30](https://github.com/suhina-daw/suhina-daw/issues/30)) |
| M7 | **More formats:** DAWproject import and export | not started |
| M8 | **UI:** Launcher and Arrangement views on top of the commands; familiar workflow, Suhina's own look; installers (AppImage, deb, Flatpak, MSI) and signed macOS and Windows builds | in progress: Launcher and Arrangement views, browser, sample map, settings, themes and a nightly build work; installers and signed builds to come ([#180](https://github.com/suhina-daw/suhina-daw/issues/180)) |

How far this is from a full clip-launching DAW, and the order the gap is built in: the
[parity report](docs/research/parity-live-12.md) and
[ADR 0016](docs/decisions/0016-parity-target.md) (proposed).

## Next (order proposed in ADR 0016)

The gaps that matter most, in the order [ADR 0016](docs/decisions/0016-parity-target.md) proposes
(its build order has the reasons and sizes):

1. **MIDI note editing:** the piano roll in the detail area and note commands (add, move, resize,
   velocity, deactivate), quantise ([#200](https://github.com/suhina-daw/suhina-daw/issues/200),
   [spec](docs/spec/ui-clip-editor.md)).
2. **Automation:** device, plug-in and mixer parameters first, then tempo
   ([#109](https://github.com/suhina-daw/suhina-daw/issues/109) for plug-in parameters).
3. **Recording and inputs:** audio and MIDI recording, audio inputs, monitoring, count-in
   ([#263](https://github.com/suhina-daw/suhina-daw/issues/263); arming and playing from MIDI
   controllers work).
4. **The crossfader and track delay:** sends and return tracks play, level meters are done and
   the cue output works.
5. **Arrangement editing:** the loop brace ([#98](https://github.com/suhina-daw/suhina-daw/issues/98)),
   locators ([#99](https://github.com/suhina-daw/suhina-daw/issues/99)).
6. **EQ and filter**, then bus compressor and a modulation delay (see the devices below; the delay
   and reverb are done).
7. **Launching from live MIDI notes** (the rest of M4, [#264](https://github.com/suhina-daw/suhina-daw/issues/264)).

Finishing what is in flight (M3's commands, M6's automation and editors, M7, M8's installers and
detail area) goes alongside this list, not after it.

## Built-in effects and instruments

Instruments and effects that ship with Suhina. They run inside the engine (rule 5 covers
third-party plug-ins only), so they follow the real-time and never-crash rules like the engine
itself. Written in Rust; DSP from GPL-compatible open-source projects may be ported (rule 9) with
its copyright notices kept and credited in [`ATTRIBUTION.md`](ATTRIBUTION.md).

- **Crates, in this repository:** `dsp` (smoothing, gain, biquad filters, crossovers, DC filter,
  LFOs, shapers, oversampling; later oscillators, envelopes, delay lines, convolution; no engine
  or model dependency), `device-api` (the device trait, parameter descriptions, state) and one
  crate per device under `devices/`. Every parameter is set through a command
  (`device.setParam`), from the UI, CLI and MCP alike. Because devices do not depend on the
  engine, they can later be split into their own repository or built as CLAP plug-ins.
  The interface is [ADR 0004](docs/decisions/0004-built-in-devices.md).
- **Status:** the table below. The engine runs them in chains on tracks and on group buses
  (ADR 0004 › 10), through the `device.*` commands. Chains on return and master channels, and
  grouping devices inside a chain ([#147](https://github.com/suhina-daw/suhina-daw/issues/147)),
  are still to do.
- **Importing devices from Live Sets, in three levels:** a placeholder that keeps the device's
  type, name and on/off state (its parameters are not kept yet);
  then an approximate mapping to an open-source plug-in over CLAP; then a Suhina device for the
  most used ones. Which stored device lands in which Suhina device, and how each mapping is learnt
  and tested: [`docs/spec/device-import.md`](docs/spec/device-import.md). The sampler and drum
  sampler as import targets: [#101](https://github.com/suhina-daw/suhina-daw/issues/101).
- **Order:** EQ and filter first, then bus compressor and the modulation delay (the delay and
  reverb are done); multiband dynamics,
  saturation, the sampler and drum sampler later
  ([ADR 0016](docs/decisions/0016-parity-target.md)).

### The devices

Each one has its own name, parameter layout and look. Sources are candidates; each licence is
checked when the work starts, and every ported file keeps its notices. **Done** means the device
exists in `devices/`, passes its tests and runs in track chains.

| Suhina device | DSP from | Status |
|---|---|---|
| utility, auto-pan | own | done |
| three-band DJ EQ with kills | own crossovers (Linkwitz-Riley) | done |
| vinyl distortion | own, oversampled | done |
| overdrive | own, oversampled | done |
| compressor: stereo-linked, soft knee, side-chain high-pass, dry/wet | own | done |
| bus compressor | own, Airwindows (MIT) for reference | planned |
| limiter: brickwall, 1.5 ms lookahead | own | done |
| NAM Amp Modeler: a `.nam` capture through the pure-Rust `nam-rs` inference (MIT), with trims, its own EQ, HQ for the A2 width dial, a trim delay | nam-rs (MIT) | done |
| NAM Cab Loader: a `.wav` impulse response through uniform-partitioned FFT convolution, with a phase invert, a high pass, tone cuts, a trim delay and a volume | fft-convolver (MIT) under `dsp` | done |
| pitch, a MIDI effect: transposes by semitones or scale degrees, Block, Fold or Limit to a key window | own | done; step buttons planned |
| multiband dynamics, upward and downward | own crossovers and detectors | planned |
| delay: stereo, synced or free, filter in the loop, LFO, Repitch/Fade/Jump, ping-pong, freeze | own (`dsp` delay lines) | done |
| modulation delay | own delay lines | planned |
| reverb: algorithmic, early reflections, damped feedback delay network, chorus, freeze | own | done |
| convolution reverb | own convolution | planned |
| multi-stage saturation | own, oversampled | planned |
| EQ: eight bands of eight types, two curves for Stereo, Left/Right or Mid/Side, Scale, Adaptive Q | own (`dsp` state-variable filters) | done; analyser, Audition and a curve display planned |
| filter | own (`dsp` filters) | planned |
| FM synth | MSFA (Apache-2.0) via Dexed | DX7-style engine (32 algorithms) done; more waveforms and a filter planned |
| wavetable synth | Vital (GPL-3.0), under its own name | planned |
| subtractive synth | own, Surge XT (GPL-3.0) for reference | planned |
| sampler | own | single-source Simple/Grain instrument, SDK panel, source loading and recording, tempo-synchronised sample Warp with six selectable modes and automatically initialized, editable source BPM |
| drum sampler | own | planned |
| physical-modelling synth | Mutable Instruments Rings and Elements (MIT) | planned |

## Audio to MIDI

Turning audio into something playable from MIDI, offline (not on the audio thread), each as a
command with undo. Behaviour, detectors and tests: [`docs/spec/audio-to-midi.md`](docs/spec/audio-to-midi.md).

- **Goals:** slice an audio clip to a new MIDI track (one slice per pad of a sampler or drum
  sampler); convert drums, bass, melody and chords to MIDI with four narrow detectors rather than
  one that writes every note of a mix; snap results to the project's tempo, warp grid and key.
- **Stem separation** before detecting is the largest single gain in quality, and is also offered
  on its own, as separating stems to new audio tracks
  ([#146](https://github.com/suhina-daw/suhina-daw/issues/146), [spec](docs/spec/stem-separation.md);
  the pipeline around a model and a test backend exist in `crates/separation`).
- **Order:** after the sampler and drum sampler exist; slicing first (smallest), then drums, bass,
  melody and chords, then stem separation. An ADR comes before the detectors.

## Later, not ordered

FL Studio projects (`.flp`), MIDI controllers and remote mapping (hardware controllers: scope in
[ADR 0016](docs/decisions/0016-parity-target.md)), devices of an imported project that
Suhina cannot run kept as placeholders and copied along with the project
([device import](docs/spec/device-import.md)), bridging Windows plug-ins.

**Patch devices:** devices built by the community as Pure Data patches, with access to
commands; plugdata is expected to load as an ordinary CLAP or VST3 plug-in now that hosting works,
but has not been tried yet ([ADR 0008](docs/decisions/0008-patch-devices.md), proposed).

**ARA** after VST3 hosting works in the sandbox: Arrangement first, since looping, quantised
Launcher clips do not fit ARA's linear timeline; Launcher view later. The design is
[ADR 0028](docs/decisions/0028-ara-hosting.md) (proposed) and the stages are the epic
[#587](https://github.com/suhina-daw/suhina-daw/issues/587): the document lives in the plug-in host,
audio reaches it through shared memory, and an ARA clip that would overlap another audio clip is
refused or suspended, never by losing its edits.

## What works now

- Audio settings can open the active ASIO driver's native control panel, and
  show the engine block separately from the measured driver callback size.

- Theme skins: editable SVG/PNG controls with colour/state variants and image sequences, configurable level-meter presentation including signal-driven artwork frames and theme-owned intensity, theme-owned toolbar layouts and native element placement, and portable CSS/JSON/SVG/PNG packages with live literal-value synchronization. Suhina's own style engine (`suhina-style`, no browser engine) adds a retained native style tree, cascade layers, variables, calc(), media ranges, linear/radial/conic gradients, color-mix(), bounded box shadows, paint transitions/keyframes, Flex/Grid toolbar/workspace panel layout and persistent anchors between controls. Bar/window geometry, remaining raw controls and further CSS effects still need integration.
- Theme states and tracks: `:checked`, `:selected`, `:playing`, `:recording`, `:armed` and `:muted` in theme CSS, a track's colour as `--track-colour`, name colours, per-view track artwork, each bar's own separator line, styled menus, and a least Arrangement header width; a crash leaves a file in the data folder's `crashes/`.
- Theme control shapes: buttons, the toolbar's fields and the volume and pan fields take their background, border, corner radius, thickness and shadows from the theme's CSS (`#arrangement-volume`, `.value-fill`, `volume.track` artwork and the rest); a workspace panel can float over the view; a theme can put the Arrangement track headers on a side; Settings › Themes has a search.
- Theme presentation: native Launcher column widths and row heights come from the theme's CSS; mixer/name backgrounds consume authored materials, and stereo meter gradients retain fixed dBFS colour positions with native peak/RMS opacity and lit-region glow. Fader scales move independently of the rail/cap.

- Forest Amber, the green-and-white Suhina Sheet, blue/cream glass Suhina Meadow and brown pixel-textured Suhina Blocks are available as built-in themes. Sheet and Meadow include compact Session geometry, native CSS and original editable SVG controls; Meadow embeds an original generated green-hill background and liquid settings. Blocks uses brown plank CSS without landscape wallpaper, plus colour-keyed SVG track textures.

- Private image backgrounds use a cached source and display-scaled, alpha-correct blur; Settings keeps its own readable material. Native Mica has been removed.

- Theme variants: a theme's settings are CSS custom properties, the core set every theme has and any it declares with `@property` (shown under Theme settings with a control for its type). A variant (`variants/<id>.json`) holds other values for them, such as a light and a dark version of one theme; the picker shows a swatch per variant, and editing a theme's settings saves a variant, so built-in themes need no copy for it.
- Theme packages: Export theme… writes one theme as a `.theme.suhina` file with a SHA-256 manifest; dropping or opening one brings it in after checking every file, and `suhina-cli theme pack` and `theme validate` do the same from a terminal.
- Theme reach: sends, the add-track buttons, the ruler, loop bar and play position, clip parts and states (`:playing`, `:recording`), level-bar handles, fader sizes, a placeable mixer divider, the piano roll (grid, keys, notes with the clip's colour, tools), each built-in device's panel (`#device-compressor`), and the ordinary buttons and fields in Settings, dialogs and pages take the theme's CSS; artwork reads the theme's variables, draws text and can be nine-sliced. New clips take their track's colour (New clip colour).
- Theme customization includes independent Browser/coloured-surface text, alpha-preserving gradients, editable liquid strengths, a private image backdrop and configurable application-bar identity/project information.
- Private image backgrounds retain image compositing on transparent GPU windows; narrow Arrangement headers keep names, recording controls and meters separate.

- SDK sampler graphs can bind a sample while being added to a track; project saving,
  relocation and missing-file relinking retain the graph and its source separately.

- **Sampler graph building blocks:** typed sample resources, routed events, private voice/grain
  graphs, envelopes, loops, slicing, deterministic modulation, native stretch, versioned components
  and editable sampler/granular templates are executable through the Rust SDK host API.

- **Composable SDK sample source:** Rust-authored instrument graphs can bind PCM to
  independent polyphonic `sampler` nodes and process their audio with ordinary SDK nodes.
- Windows release, nightly and test packages include ASIO support: choose ASIO in Settings ›
  Audio › Driver, then an installed audio driver in Device. WASAPI remains available.

- ASIO device choices remain available during playback. Late plug-in replies do not
  mark a responsive host as hung, and obsolete starts unload away from the window.

- **Device SDK authoring:** checked Rust graph handles, parameter-source helpers and standard
  panel-control constructors produce the same data-only `.sd` devices and use the existing runtime.

- **SDK panel composition:** numeric formatting chains, inherited parameter metadata and local
  control-group coordinates simplify panels without changing their saved format or runtime.

- Every built-in effect, instrument and MIDI effect has editable controls in the device panel:
  its layout (in Rust, or as `devices/<name>/panel.json`: dials, buttons and band graphs in the compact panel, an expanded panel where a device has many parameters, and a generated list below the compact panel for parameters the compact layout does not place), or a parameter list generated from the device's own parameters,
  260 px wide and at most 168 px high, scrolling past that. Each change is `device.setParam`, so it
  is one undo step and an automation lane takes it over. Built-ins are native code with a layout
  loaded by their id, not `.sd` files ([ADR 0021](docs/decisions/0021-device-graph-files.md) ›
  Amendment: device panels).
- A device with an expanded panel (`device.expanded` in a `.sd` file, version 5, or in a
  built-in's layout) has a button in its header that opens it over the main area; Escape or its
  close button closes it. Both panels change the same parameters.
- A running plug-in's device panel lists its parameters under its editor button, as a generated
  list that sets them by id (hidden and read-only ones left out), and stays in step both ways
  with the plug-in: what was changed in its own editor becomes the device's value with the
  next block, and the plug-in's editor follows the panel and automation.

- **Sampler:** single-source polyphonic sample and granular playback with an editable
  waveform, SDK controls, browser/system sample drops and recording from the armed
  track's selected audio input. Sample replacement is undoable; missing sources can
  be relinked. Multisample zones remain planned.

### M0 Foundation
- Rust workspace, GPL-3.0-or-later, CI (fmt, clippy `-D warnings`, tests) on Linux, Windows and
  macOS for every change; macOS does not block merging yet.
- A nightly build of `main` for Linux, Windows and macOS, with the desktop app, the CLI and the
  plug-in host, published every night as the `nightly` release
  ([`docs/releasing.md`](docs/releasing.md#nightly)).
- On macOS the desktop app is `Suhina.app`, opened from Finder and dragged to Applications, with
  the CLI and the plug-in host inside it ([`docs/releasing.md`](docs/releasing.md#packages)).
- Clean-room rules for people and agents ([`CONTRIBUTING.md`](CONTRIBUTING.md)) and the agent
  guide ([`AGENTS.md`](AGENTS.md)): no panics, no `unsafe` outside the two plug-in hosting crates, real-time safety, sandboxed plug-ins.
- Contributor process: DCO sign-off, a pull request template with a clean-room confirmation and a
  hand test, CI running fmt, clippy and tests, and a channel for reporting IP concerns.
- `docs/INDEX.md` and `docs/commands.md` are generated on `main` after every merge; issue forms
  for bugs, plug-in problems, features, presets and new devices set area and platform labels;
  release notes come from git, and the nightly and releases are announced on Discord.
- DAW-neutral project model: tracks and mixer channels apart, a pool of clip contents placed by
  clips, so per-track clips and shared patterns both fit
  ([ADR 0001](docs/decisions/0001-daw-neutral-model.md)).

### M1 Reading Live Sets
From Live Sets we authored ourselves in Live 12.4 ([format spec](docs/spec/als-format.md),
[corpus guide](docs/spec/als-corpus.md)):
- tempo and time signature (read from their automation default, where Live keeps them)
- tempo and time-signature changes over the song, from the breakpoints of the same envelopes
  (a reading still to be confirmed by a corpus file; [tempo map](docs/spec/tempo-map.md))
- MIDI, audio, group and return tracks; volume, pan, track on/off, sends (read, kept and
  played), routing into groups
- scenes, session clip slots and arrangement clips
- MIDI clips with notes (velocity, release velocity, deactivated notes), start and end markers,
  loop, deactivated clips
- audio clips: file reference, warp on/off, warp markers, warp mode, clip gain, transpose
- device chains: VST2 and VST3 plug-ins with their id and state kept; the format's own devices:
  those Suhina has a counterpart for with their settings, the rest as placeholders
  ([device import](docs/spec/device-import.md))
- a delay and an algorithmic reverb; the matching devices in a Live Set to import come in as them
  with their panel settings; the delay's Equal-Loudness option is not read and imports as the
  linear mix law ([delay](docs/spec/delay.md), [reverb](docs/spec/reverb.md),
  [import](docs/spec/device-import.md))
- the Pitch MIDI effect, with its settings ([pitch](docs/spec/pitch.md))
- an eight-band EQ; the matching device in a Live Set comes in as it with both curves of every
  band, its mode, Scale and Adaptive Q; its oversampling option is noted and not used
  ([EQ](docs/spec/eq.md), [import](docs/spec/device-import.md))
- a MIDI clip's scale (root and mode), which Pitch follows; a scale Suhina has no mode for comes
  in as none, with a warning
- anything not understood yet is skipped with a warning; the project still opens

### M2 Playing
- `suhina-cli play` plays a set, or one launcher scene, on the default output or the one chosen
  (`--host`, `--device`, `--buffer`, `--exclusive`; WASAPI exclusive, ASIO with the `asio`
  feature); `render` writes a 32-bit float WAV. Both run the same engine code.
- Built-in effects and instruments on a track run in a chain before its fader; MIDI notes play
  on the track's instrument when the chain starts with one (the FM synth), otherwise as sine tones. Group and
  master volume and mute apply. A group with devices is a bus: its devices run over the sum of
  its tracks, with latency compensation on every path. `--fm` on `play` and `render` puts the FM synth on every MIDI
  track, with a voice from a DX7 file; `suhina-cli fm-synth voices` lists a file's voices.
- Device groups play: devices grouped on a track run in parallel branches, each with its own
  volume, pan, mute and solo, summed with a mix against the dry signal and an output gain;
  instruments in a group are layered on the same notes, and groups nest four deep
  ([spec](docs/spec/device-groups.md)). Ctrl+G puts the devices picked in a strip's list or the
  chain into a group, Ctrl+Shift+G takes one apart; the group's panel sets its mix and gain and
  each branch's volume, pan, mute and solo, and shows a branch's devices after it. Plug-ins run
  inside groups too. Commands: `device.group`, `device.ungroup`, `device.addGroup`,
  `device.move`, `device.duplicate`, `branch.*`, with device paths such as `"2/1/0"`.
- The NAM Amp Modeler and NAM Cab Loader run a `.nam` model and a `.wav` impulse response from
  the NAM folders in Settings, with `device.setNamFile` to load a file and `device.file` to read
  it ([nam spec](docs/spec/nam-amp-and-cab.md)).
- MIDI effects run before a track's instrument: the pitch effect transposes the notes by
  semitones or by steps along the playing clip's scale and keeps them inside a window of keys
  (Block, Fold, Limit); a note-off always ends the key its note started
  ([spec](docs/spec/pitch.md)). On a track with no instrument the changed notes play as the
  track's sine tones. It is in the browser under **MIDI Effects** and is set through
  `device.setParam`, an automation lane or its generated parameter list in the device panel.
- Sends and return tracks play: each track sends to each return before or after its fader; a
  return runs its devices over what it is sent and plays into the main channel or a group, with
  latency compensation on every send; loops are refused (`track.setSend`, `track.setSendMode`,
  `return.add`, `return.remove`, `return.rename`; [spec](docs/spec/sends-and-returns.md)).
- Tempo and time-signature changes over the song (the tempo map): tempo steps and ramps, and
  signature changes at bars, saved in the project file and edited with `tempo.addPoint`,
  `tempo.movePoint`, `tempo.deletePoint` and `timeSignature.add`/`move`/`delete`. Notes, audio
  clips (stretched or re-pitched), fades, the playhead, launched clips, launch quantisation and
  the metronome all follow it; a ramp is converted exactly, so nothing drifts. The bar ruler
  numbers bars through the signature changes and marks every change
  ([spec](docs/spec/tempo-map.md)). The arrangement loop keeps its beats through a tempo change.
- A launched scene's tempo or time signature overrides the tempo map until
  `transport.followTempoMap`, the next start of the transport or an edit to the map
  ([spec](docs/spec/tempo-map.md#scenes-and-the-tempo-map)).

### M3 Commands and agents (in progress)
- Every edit is a command with an id, typed parameters and one undo step; failed or invalid edits
  leave the project unchanged ([ADR 0002](docs/decisions/0002-commands.md)).
- Commands for the project, tracks, clips, undo, the open projects, plug-ins, devices, the audio output, the library and the sample map:
  [`docs/commands.md`](docs/commands.md) lists every one with its parameters (generated from the
  registry; `suhina-cli commands` prints them).
- Several projects open at once, like tabs, one of them active and playing; commands go to the
  active one or to a named one ([ADR 0007](docs/decisions/0007-several-open-projects.md)).
- `suhina-cli mcp` serves them to agents over the Model Context Protocol, with file access
  only below granted roots ([`docs/mcp.md`](docs/mcp.md)).
- `suhina-cli mcp --attach` lets an agent work on the projects open in the running app, once the
  person turns it on in Settings and allows the connection: its edits show in the window at once
  and undo with Ctrl+Z; starting a build that is running opens the project there as a tab
  ([ADR 0028](docs/decisions/0028-mcp-in-the-running-app.md)).
- Edits and undo/redo are heard by a playing engine from its next audio block, without a gap:
  a new graph is swapped in at an exact frame and playing clips carry on; device parameters go
  as messages, without a new graph
  ([ADR 0005](docs/decisions/0005-edits-while-playing.md)).
- Projects are saved in Suhina's own format, `.suhina` (versioned JSON, plug-in state included,
  audio referred to by path): `project.save { path?, copy? }`; `.suhina` files open in the app,
  `suhina-cli` and MCP like Live Sets do ([ADR 0010](docs/decisions/0010-project-files.md),
  [spec](docs/spec/suhina-format.md)).
- Automation envelopes for a track's volume, pan and mute and every built-in and `.sd` device parameter:
  lanes of points with curves and jumps, edited point by point with `automation.addPoint`,
  `movePoint`, `deletePoint`, `clear`, `setEnabled` and `list`, saved in the project and followed by
  playback and renders (device parameters split at their points and every 64 frames, mixer gains
  moving frame by frame); setting an automated parameter by hand turns its lane off until it is
  turned on again ([#395](https://github.com/suhina-daw/suhina-daw/issues/395),
  [spec](docs/spec/automation.md)).
- Automation lanes in the Arrangement view: the envelope button on a track header shows lanes
  under the track, one per chosen parameter (volume, pan, mute, each built-in or `.sd` device parameter);
  a click adds a point on the grid (Alt: free), points are dragged, selected (a box, Shift-click)
  and moved or deleted together, a segment's curve is bent by dragging it, and a lane a manual
  change turned off is turned on again from its header; each edit one undo step through the
  `automation.*` commands ([#420](https://github.com/suhina-daw/suhina-daw/issues/420),
  [spec](docs/spec/automation.md#in-the-arrangement-view)).
- Automation points added on the line: a click on or near an envelope adds a point on it, so a
  level or straight segment stays exactly so, and a lane without points shows a faint line at
  the parameter's value that a click puts the first point on
  ([#736](https://github.com/suhina-daw/suhina-daw/issues/736),
  [spec](docs/spec/automation.md#in-the-arrangement-view)).
- Backups: each project with unsaved changes is copied to `.backup/` beside its file (a project
  never saved: to the settings folder) every 5 minutes by default, at another interval, before
  critical operations (closing, quitting, relinking, adding files, loading a plug-in, deleting a
  track) or never, written on a background thread, the newest 20 kept; after a crash the
  next start offers the backup back, to open as an unsaved project tied to its file
  (`project.autosave`, Settings › Projects and backups › Backups, [spec](docs/spec/backups.md)).
- Clips, arrangement lanes and scenes have ids that stay the same when they are moved or edited and
  are never used twice; commands name a clip by its id (`clip`), so an agent's target does not move
  when other clips do, and the positional `index` is deprecated
  ([#568](https://github.com/suhina-daw/suhina-daw/issues/568),
  [ADR 0025](docs/decisions/0025-object-identity.md)).
- Still to do: more commands (notes, [#200](https://github.com/suhina-daw/suhina-daw/issues/200);
  clips), MCP resources.

### M4 Clip launching (in progress)
- The engine launches clips and scenes on a quantisation grid: bars or note values (triplets
  too, or none), following the time signature; 1 bar by default
  ([spec](docs/spec/clip-launching.md)).
- One clip per track; a clip loops until it is stopped or replaced, a clip without a loop plays
  once. Scenes launch their row, empty slots with a stop button stop their track, and a scene
  tempo changes the tempo at the launch.
- A track that launched a clip leaves its arrangement clips until "back to arrangement": in
  the Arrangement view its lane dims, and while any track plays launched clips the ruler shows
  a **Back to arrangement** icon at its right end, which returns every track to the arrangement.
- Scenes are edited with commands and from the scene button's right-click menu: insert,
  duplicate (with copies of its clips), delete, rename, and a tempo and time signature that
  apply when the scene launches (bars and the metronome count in the new time signature from
  there). Playing clips stay with their row when scenes are inserted or deleted while playing
  ([#404](https://github.com/suhina-daw/suhina-daw/issues/404)).
- A slot's mark is the shape its click does: a play triangle in the track colour while stopped,
  a stop square while playing — a playing trigger clip's click stops it, and a playing scene's
  row stops on its button's click (`scene.stop`). The Delete key takes a selected slot's clip
  off its slot.
- A sample dropped into a slot loops over the whole file even when its tempo is not known (it
  plays at its own speed); `clip.setLoop` switches a slot clip's loop off to make a one-shot.
- An arrangement clip goes into a clip slot, and a slot's clip onto the arrangement, as a copy
  with its gain, warping, loop and fades: Copy and Paste in the right-click menus, Ctrl+C and
  Ctrl+V on the selected clip or slot (`clip.copy` and `clip.paste` take `scene`), or a drag held
  over the other view's name in the control bar until the view switches
  ([spec](docs/spec/clip-launching.md) › Clips between the arrangement and the slots). Within
  the Launcher view a drag **moves** a clip to the slot under the pointer, swapping with what is
  there (`clip.moveSlot`); Ctrl+drag copies.
- A clip is renamed from its right-click menu, a slot's or an arrangement clip's: the name field
  at the menu's top, applied with Enter or **Rename** (`clip.rename`; an empty name clears it).
- A metronome (`transport.setMetronome`, off by default): a click on every beat while the
  transport runs, higher on the first beat of a bar, following tempo edits and scene tempos.
  Count-in comes with recording.
- Warped launched clips are time-stretched like arrangement clips, and re-pitched after a tempo
  change.
- Transport commands (`clip.launch`, `scene.launch`, `track.stop`, `launcher.*`,
  `transport.play`/`stop`/`setPosition`) reach the audio thread through a lock-free queue:
  `suhina-cli play --launcher` takes them as typed lines, `suhina-cli mcp --audio` from
  agents, and `project_render` renders a script of them offline.
- Follow actions: after its follow time (beats, or the clip's length) a slot clip runs action A
  or B, picked by chance: stop, play again, previous, next, first, last, any, other, or jump to
  a scene's slot, on the exact frame, saved in the project (`clip.setFollowAction`).
- Launching a slot clip selects it, and the detail area shows its follow action: an on switch,
  the follow time and a switch that links it to the clip's length, actions A and B (a jump picks
  its scene) and their chances, each change one undo step (`clip.setFollowAction`).
- Launch modes (trigger, gate, toggle, repeat), legato and per-clip launch quantisation:
  `clip.setLaunchMode`, `clip.setLegato`, `clip.setQuantization`, and `clip.release` for the
  release of a slot's launch button; in the Launcher view a slot launches as the mouse button goes
  down, and right-clicking a clip sets its launch settings
  ([spec](docs/spec/clip-launching.md#launch-settings)).
- Computer keys launch slots, scenes and stop buttons: Ctrl+M or **Map keys** in the title bar maps
  the next key to the clicked control, and a key down and up are the press and release, so gate
  and repeat clips play from the keyboard. `launcher.addMapping` maps MIDI notes too, kept in the
  project; playing them waits for MIDI input
  ([spec](docs/spec/clip-launching.md#launching-from-keys-and-notes)).
- Still to do: launching from live MIDI notes, fades on launched clips
  ([#104](https://github.com/suhina-daw/suhina-daw/issues/104)), reading the global
  quantisation from a Live Set, and checking the behaviour marked as hypotheses in the spec
  against Live.

### M5 Audio (done)
- Audio files (WAV, AIFF, FLAC, MP3, Ogg Vorbis) are decoded in parallel when a set opens.
- Audio clips play with clip gain, transpose, loops and, on the arrangement, fades (launched
  clips: [#104](https://github.com/suhina-daw/suhina-daw/issues/104)); on warped clips transpose
  changes the pitch only.
- Warped clips are **time-stretched in real time** along their warp markers and keep their
  pitch, with our pure-Rust port of Signalsmith Stretch
  ([ADR 0003](docs/decisions/0003-time-stretching.md)). An 80 s set with 58 warped clips opens
  in 0.4 s. Transients stay within 2 ms of the warp markers, so stretched drum loops do not flam
  against unstretched ones.
- Arrangement clips are stretched 0.5 s ahead on a worker thread; the audio thread only copies
  their frames (3 % of one core on the same set, 99 % of blocks within 1.3 ms of their 10.7 ms).
- Later, not needed for M5: launched clips stretched ahead too; formant shifting for transposed
  vocals. Warp modes other than re-pitch share one stretcher today; separate algorithms per mode,
  and Rubber Band for a high-quality mode if a listening test shows it is worth it.

### M6 Plug-ins (in progress)
- CLAP and VST3 plug-ins run in `suhina-plugin-host` processes, never in Suhina's: by
  default one process per plug-in file, or per vendor, or one per instance
  (`plugin.setHosting`); a crash or hang costs that plug-in's sound, not the project
  ([ADR 0006](docs/decisions/0006-plug-in-hosting.md)). The host runs on Linux, Windows and
  macOS (editors on macOS still to come).
- An opened Live Set's plug-ins play in the app: each is found among the installed ones (a VST3
  version for a VST2 plug-in too) and started with the settings the project saved; instruments
  play their track's notes, effects process their track. Missing ones are named in the status
  bar and keep their settings untouched.
- While an opened project's plug-ins start, a dialog shows how many are ready and which are
  still loading or could not start; it can be sent to the background, and the project plays
  meanwhile. A scan finds plug-ins that were missing without opening the project again.
- Tracks are lined up for the latency of their devices and plug-ins.
- A plug-in that changes its latency while it plays (a lookahead set in its editor) is
  followed: Suhina hears of it, restarts the plug-in's processing as its format asks, and lines
  the tracks up again.
- The device panel shows each plug-in's state (loading, running, missing, crashed) with Open
  editor, Close editor or Restart; the same commands work from the CLI and MCP. P opens or
  closes the shown track's plug-in's editor (the device the chain panel selects, else the
  track's first running one; mappable, Settings › Keys). Editors float
  above Suhina's window, owned by it (Windows, X11), and respond at once; a setting
  (Plug-ins › Editors › Device view) shows them inside the device panel instead, as part of
  the chain. On Linux the window is an X11 one whenever an X server (or XWayland) is there,
  so this works on Wayland desktops too.
- Renders (`suhina-cli render`, MCP `project_render`) run the same plug-ins.
- MCP `project_render` from an open project starts its plug-ins with the state the running ones
  have now, so a change made in a plug-in's editor is in the render as it is heard.
- The plug-ins of different tracks run at the same time (one block out to every host, then
  every answer collected), the engine renders ahead of the audio device in blocks of its own,
  and host audio threads run at real-time priority: a project with 25 plug-ins plays without a
  late block. The device panel says when a plug-in does fall behind ("N% of blocks late"; a
  block a busy host refuses counts once, and a restarted plug-in reads as running at once).
- Plug-in parameters are automated from the engine: `device.pluginParams` lists a running
  plug-in's parameters, `device.setParam` sets one (heard from the next block, saved with the
  project) and `device.setAutomation` draws a lane of beats and values over the arrangement,
  which playback and renders follow to the frame (a value every 64 frames while it moves)
  ([#109](https://github.com/suhina-daw/suhina-daw/issues/109)). Their lanes are set by command;
  the Arrangement's lanes show built-in and `.sd` parameters only so far.
- The render thread and the plug-in hosts' audio threads get real-time scheduling on Linux
  without setup: `SCHED_FIFO` where the user has real-time rights, otherwise through rtkit, the
  desktop's own service for it; on Windows they join MMCSS "Pro Audio". Where the system refuses,
  Suhina plays on and says once that the sound may drop out under load.
- Plug-ins next to each other in a chain that run in the same host process (by default every
  instance of one plug-in file) share one round trip a block: the host runs them one after
  another and answers once, which at small buffers takes about a third of the time of a round
  trip each, with the same sound
  ([#108](https://github.com/suhina-daw/suhina-daw/issues/108)).
- Each track's chain goes on to its next plug-in as soon as its own plug-in answers, while the
  other tracks' plug-ins are still working, so heavy plug-ins on different tracks cost a block
  about one heavy plug-in, and a slow or hung plug-in holds up only its own track
  ([#151](https://github.com/suhina-daw/suhina-daw/issues/151)).
- Still to do: checking an imported Live Set
  with real plug-ins ([#112](https://github.com/suhina-daw/suhina-daw/issues/112)), Wayland and
  macOS editors, VST2 hosting — decided in
  [ADR 0022](docs/decisions/0022-vst2-interface-description.md) (VeSTige, pinned) and begun
  with the interface layer, the mock and the probe
  ([#1066](https://github.com/suhina-daw/suhina-daw/issues/1066),
  [spec](docs/spec/plugin-vst2.md)).

### M8 UI (in progress)
- `suhina` opens a Live Set (as an argument, or dropped on the window) or `--demo`, plays it
  stopped on the output chosen in Settings › Audio and shows the **Launcher view**: tracks as
  columns, return tracks beside the scene column, clip slots with their state (stopped, queued,
  playing with progress), scene launch, stop buttons, and a mixer strip with pan, volume and
  track on/off
  ([ADR 0009](docs/decisions/0009-ui-toolkit-and-look.md), [spec](docs/spec/ui-session-view.md)).
- A **splash** while Suhina starts: a 300 × 200 px panel centred in the window with the logo and
  the version, shown while the browser's library, the sample map and the project asked for start,
  then fading into the ready window ([components](docs/design/components.md#splash)).
- **At start** (Settings › Projects and backups): with no project given, Suhina opens the start panel,
  a new blank project, a new one from a chosen template, or the project opened last
  ([ADR 0010](docs/decisions/0010-project-files.md)).
- A **start panel** while no project is open: New, a template or Open, the recent projects with
  when each was saved and a search field, and the audio output with a button to its settings
  ([components](docs/design/components.md#start-panel)).
- **Browse…** beside every path field (the projects and templates folders, the start template,
  Save as, Open, + Add folder) picks the path in the system's own file or folder dialog.
- With **no project open**, the browser works as with one: browsing, searching, the sample map
  and previews, which then play through an output of their own
  ([ADR 0011](docs/decisions/0011-preview-and-added-audio.md)).
- The **Arrangement view** (Tab switches): bar ruler, one lane per track with its clips and their
  notes (loops repeated), the playhead, track headers on the right that show volume and pan once
  a lane is tall enough (drag its lower edge), return and main rows along the bottom. Clips move
  by dragging (`clip.move`), snapped to the grid (Ctrl and the wheel zoom).
- A track's pan and volume (the mixer strip in the Launcher view, the track headers in the
  Arrangement view) are heard while they are dragged, and one drag is one undo step
  (`track.setPan`, `track.setVolume` with `continue`;
  [#313](https://github.com/suhina-daw/suhina-daw/issues/313)).
- A click on the ruler sets where playback starts (`transport.setPosition`, on the clip grid,
  Alt for free), shown as a marker; while playing, playback jumps there. Stop returns to the
  marker, or stays where it stopped (Settings › Arrangement › Time and play › Stop); a second Stop goes to the
  beginning ([spec](docs/spec/arrangement-editing.md)).
- **Play from the selected clip:** Shift+Space plays from the start of the clip selected in the
  Arrangement view, and the setting Settings › Arrangement › Time and play › Play from (**Marker** or **Selected
  clip**) makes Play and Space do the same; the marker moves to the clip's start, so Stop returns
  there (`transport.play {from: "clip", track, index}`, `play <track> <i>` in
  `suhina-cli play --launcher`; [spec](docs/spec/arrangement-editing.md) › Play position).
- The grid value in the control bar (`1 bar`, `1/16`, `1/8T` … `none`) is also the Arrangement
  view's grid: lines at every step, coarser as the view zooms out, and moved clips, dropped files,
  fade handles and ruler clicks snap to them (Alt held, or `none`: off the grid); each project tab
  keeps its own value ([spec](docs/spec/arrangement-editing.md#the-grid)).
- Clips move between tracks by dragging them up or down (`clip.move` with `to_track`); a MIDI
  clip then plays through the new track's instrument and effects.
- **Markers** on the arrangement's timeline, in a row under the ruler: M adds one at the
  playhead, a right-click on the ruler or the row adds one there; a click jumps to it, a drag
  moves it, its menu renames or deletes it (`marker.add`, `marker.move`, `marker.rename`,
  `marker.delete`, `marker.jump`, `project.markers`). Saved in the project file, and a Live Set's
  locators import as markers ([spec](docs/spec/arrangement-editing.md#markers)).
- **Loop region** in the arrangement: drag in the loop brace row under the ruler to draw it, drag
  its ends or its middle to change it, click it or press Ctrl+L to turn looping on or off. While
  on, playback repeats the span, wrapping on the exact sample (`transport.setLoop`,
  `transport.setLoopEnabled`). Saved in the project file, and a Live Set's loop imports
  ([spec](docs/spec/arrangement-editing.md#loop-region)).
- **New, open and save:** Ctrl+T shows a dialog to make a new blank project, one from a template
  (the templates folder is a setting) or open one (a typed path or the recent ones); Ctrl+S saves
  as `.suhina` (asking for a name and folder the first time), Ctrl+Shift+S saves as, and File ›
  Save as template writes a copy to the templates folder. The tab and window title mark unsaved
  changes with a dot, and closing a project, opening another in its place or quitting with
  unsaved changes asks whether to save them.
- **MCP export permissions:** Settings › Agents and updates lists allowed write folders, including the
  optional project-folder grant. The person adds or revokes folders; agents cannot grant
  themselves permission. Attached exports prepare across frames and render off the window
  thread, including exports in batches.
- **Export:** File › Export… renders the mix to an audio file the user chose — wav (32-bit
  float, 24 or 16 with dither), flac, ogg or mp3 (through the vendored LAME 3.100 encoder,
  at 128/192/256/320 kbps or VBR V0/V2), at a chosen sample rate, with normalise to
  −0.3 dBFS and an overwrite question ([spec](docs/spec/export.md)). The same `project.export`
  command serves the MCP server and scripts, and adds a range and tail, and a source (one node
  of the track tree), which the command line reaches with `suhina-cli render --source` too. One
  export runs at a time — a second is refused — and the export reads the running plug-ins'
  state as it is heard, without changing anything in the project.
- A change made only in a plug-in's editor counts as an unsaved change: the dot appears within
  about a minute, and closing or quitting asks whether to save it.
- The **browser** (Ctrl+B shows and hides it, left or right of the view): search (Ctrl+F),
  category filters (All, Samples, MIDI, Instruments, Effects, Presets, Plug-ins), a sidebar of
  Pinned, Recent (by kind) and the folders the user added (a path, or a folder dropped on the
  window), a tree with type icons, lengths and colour tags, a preview strip with the waveform,
  length, tempo and key, and a footer with the tags as filters. Samples and MIDI files preview
  through the engine (auto-play, or the play button; MIDI through the selected track's
  instrument), whether the project plays or not. Dragging samples and MIDI files onto a lane,
  a slot or the empty space after the tracks places them (`clip.addFiles`; one track, or one
  each with Ctrl; a sample with a known tempo is warped; a Stems file `.stem.mp4` becomes a group
  of its stems and its muted master, with its mastering compressor and limiter on the group),
  devices, DX7 presets and plug-ins onto
  a track or the empty space add them (`device.add`, `track.add`); Enter or a double-click adds
  to the selected track. The folders are indexed on disk and watched for changes
  ([spec](docs/spec/browser.md), [ADR 0011](docs/decisions/0011-preview-and-added-audio.md),
  [ADR 0012](docs/decisions/0012-file-library.md)).
- The browser's **tag editor** (the footer's Edit tags button): every tag in a list — its
  colour, its name (typed to rename), how many items carry it and Remove (asking first), and a
  row that adds one; a click on a row filters by it, as the footer's dots do.
- Samples, Stems files and MIDI files dragged from the system's file manager land like a drag
  from the browser: on the slot, lane or empty space under the pointer, outlined while they hover
  (`clip.addFiles`). Where the system reports no pointer position during the drag, they go where
  Enter puts them ([spec](docs/spec/browser.md#files-from-the-system)).
- The browser's **Plug-ins category** lists the plug-in instruments or the plug-in effects (a
  switch kept with the library) as one list of name and format, sorted by clicking a heading;
  the name always shows, the format column drops when the browser is narrow. The other
  categories keep the plug-ins under their vendors, and pins and tags work on plug-in rows like
  on files ([spec](docs/spec/plugin-browsing.md)).
- The browser's **Sample map** is a button of its own in the category row; a place stays chosen
  on a second click and its × lets go of it, and a recent group's place brings its category
  ([spec](docs/spec/browser.md)).
- The **plug-in manager** (Settings › Plug-ins › Manage plug-ins): every plug-in found and every
  file that gave none in one table (on, name, vendor, type, format, version, status, file),
  sorted by any column and searched; a plug-in switched off is not offered and does not load,
  and a failed file is read again with its **Scan again**
  ([spec](docs/spec/plugin-manager.md)).
- Plug-in **folders per format**: each added folder for CLAP, VST3 or both, any folder switched
  off without removing it (`plugin.setFolderEnabled`); VST2 folders are ready for when VST2
  plug-ins are hosted.
- Plug-in **scans try failures again**: a file that failed is read once more, one at a time, in
  the same scan, and later scans try it again (one that hung at most once a day); a file that
  holds no plug-ins, or is built only for another processor (read from its header without
  loading it, such as an Intel-only plug-in on an Apple silicon Mac), is settled and listed
  apart.
- **Missing files**: a project whose audio files cannot be found says so in a bar when it opens
  (Find…, Later; View › Missing files). The panel lists them with the clips that use them, finds
  them by name in the browser's folders and the project's folder at once, or in a folder typed or
  dropped on it (walked in the background), best match first; Apply relinks them in one undo step
  ([spec](docs/spec/missing-files.md)).
- The **sample map** (the browser's Sample map button, large in the main area, or in a window of its own for a second screen, Ctrl+Alt+M, which keeps its place): the samples
  of the browser's place laid out by how they sound, coloured by kind (kick, snare, clap, hats,
  toms, cymbals, percussion, bass, tonal, vocal, effect), as a free cloud or snapped to a grid that follows the view's shape (`soundmap.layout`
  takes `aspect`), with the kinds' names over their regions and a
  legend that filters. Pointing at a sample previews it; click, Ctrl+click, Shift+drag and the
  arrow keys select; a drag places it like a drag from the list; right-click › Show similar rings
  and lists the 16 that sound most alike, and with it on, Up and Down step through that list.
  It stays readable with up to 200 000 samples: a density view when points crowd, and labels per
  cluster that never overlap. Samples are analysed in the background once and kept
  ([spec](docs/spec/sample-map.md), [ADR 0013](docs/decisions/0013-sample-map-analysis.md)).
- The sample map **recognises kinds by sound**: features read from the sample's onset, with
  its attack apart from its body, energy in five bands and the fall of a kick's pitch; a
  classifier learned from the library's own named samples gives each sample its kind before the
  layout, and the layout keeps each kind in regions of its own, so a tambourine in a folder of
  kicks lands among the percussion. Right-click a point › **Kind ▸** sets a kind by hand
  (`soundmap.setKind`); it wins over the name and the sound and is kept
  ([spec](docs/spec/sample-map.md) › Kinds and colours, #325).
- A menu bar and a **Settings** dialog over the window (Options › Settings, Ctrl+,; Escape closes
  it) with its pages listed down the
  left: General, Audio, MIDI, Plug-ins, Arrangement, Launcher and mixer, Clip editor, Transport,
  Detail area, Browser, Appearance and Shortcuts; saved in `settings.txt`.
- Every Settings page scrolls (wheel, scroll bar, a control given the focus) at any window size,
  groups its settings under headings by what the user is doing, and a **Search settings** field
  shows the settings that match from every page ([spec](docs/spec/ui-settings.md)).
- **Help › About Suhina** says which build this is: the version (a nightly's names its night and
  commit), the platform and the licence, and copies one line naming the build for a bug report
  ([spec](docs/spec/about.md)).
- The whole window zooms from 50 % to 200 % (Ctrl and + or -, Cmd on macOS; Ctrl and 0 for
  100 %; View › Zoom in and Settings › Appearance › Zoom), and the zoom is kept for the next run
  ([spec](docs/spec/window-zoom.md)).
- **Themes**: every colour comes from the theme in use, a folder with a `theme.css` in the
  `themes` folder (`theme { name: …; } :root { --accent: #A6A3FF; … }`), which Suhina never
  rewrites; themes from earlier versions are converted once, keeping the originals; track colours and the sample map's
  kind colours are theme tokens too (`track.1`–`track.8`, `kind.*`). Nine come with Suhina (Suhina
  Dark and Light, Editor Dark, High Contrast, Dracula, Nord, Catppuccin Mocha and Latte, Solarized
  Dark). Settings › Appearance › Theme picks one, changes its colours, renames, duplicates, reads
  the files again, backs every theme up into a zip and exports a template zip with a README an AI
  agent can make a theme from; a theme file or backup dropped on the window is added, and a file
  saved in an editor shows within a second.
- Short animations (hover fades, a pulsing queued clip, values easing after an undo, the view
  switch), which Settings turns off.
- The control bar has tempo, launch quantisation, play, stop, the position, the CPU load
  (how much of the time the audio engine spends rendering, from the engine's monitor; hidden
  while no engine plays) and the program's RAM in megabytes or gigabytes (its resident
  memory, measured once a second, with every plug-in host process's own added, which each
  reports to the engine once a second); Space plays and stops, Ctrl+Z and Ctrl+Shift+Z undo and
  redo. Every click is a command, the same the CLI and MCP run.
- Level meters for every track, group and return and the main output, in the Launcher mixer and
  the Arrangement headers: peak and RMS per side on the volume's dB scale, a held peak line and a
  clip light a click turns off; `mixer.levels` reads them and `mixer.resetClip` resets them over
  the CLI and MCP ([spec](docs/spec/level-meters.md),
  [#397](https://github.com/suhina-daw/suhina-daw/issues/397)).
- Solo buttons in the Launcher mixer and the Arrangement headers (`track.setSolo`): exclusive by
  default, Ctrl adds; the engine hears soloed tracks, their groups and the return tracks.
- Send knobs in the Launcher mixer strip, one per return in the strip's sends row: drag for the level,
  double-click for off, right-click for before or after the fader.
- **A channel strip per track in the Launcher view** ([#491](https://github.com/suhina-daw/suhina-daw/issues/491)):
  the mixer under the slots is resized from its top edge (46 px up to just under the last
  scene's stop row, 166 px by default, kept in Settings), and each strip shows more as it grows:
  a compact volume at the least height, then a volume fader with the meter beside it and its dB
  value, pan, the sends, the track's name, and a list of its devices and plug-ins (a click shows
  them in the detail area). The fader is one undo step per drag, a double-click sets 0 dB
  ([spec](docs/spec/ui-session-view.md)).
- **The detail area across the full width** ([#492](https://github.com/suhina-daw/suhina-daw/issues/492)):
  Settings › Interface › Detail area › Layout puts it beside the browser (as before) or under the
  browser too, spanning the whole window.
- **File › Open recent** ([#314](https://github.com/suhina-daw/suhina-daw/issues/314)): the
  recent projects, newest first, each opened as File › Open… opens a file (by the When opening a
  project setting); a project that is gone says so in the status bar.
- **Double-click makes a clip** ([#493](https://github.com/suhina-daw/suhina-daw/issues/493)):
  a setting in Settings › Interface, On by default; Off, a double-click on an empty slot or lane
  spot no longer makes an empty clip of the track's kind.
- Mixer commands take a list of tracks, so a selection moves together: `track.adjustVolume` and
  `track.adjustPan` move each track by the same amount, keeping what is between them, and
  `track.setMute`/`track.setSolo` take the whole list as one undo step. Dragging one header's
  volume or pan moves every selected track with it, the wheel over a value changes it (1 dB a
  tick) instead of scrolling, and Settings › Launcher and mixer › Mixer controls draws the
  volume and pan as fields or as knobs ([spec](docs/spec/arrangement-editing.md#volume-pan-mute-and-solo)).
- A cue output for headphones: Settings › Audio › Cue puts browser previews and the tracks whose
  **C** button is on (`track.setCue`) on another output pair of the same device (outputs 3/4 and
  up, `output.setCuePair`) at their own volume (`output.setCueVolume`), while the main outputs
  stay as they are; `suhina-cli render --cue` writes the cue beside the mix
  ([spec](docs/spec/cue-output.md)).
- A track header 70 px or taller shows where the track plays: the main output or a group
  (`track.setOutput`, loops refused).
- The Launcher view's channel strip shows where each track plays ("→ Main" or a group) once the
  mixer is 192 px or taller, and its dropdown routes the track (`track.setOutput`, one undo step).
- A clip dropped over others replaces what it covers (shortens, splits or removes them), or
  stacks with them when Allow sample / MIDI stacking is on; stacked clips share the lane in
  sub-rows ([spec](docs/spec/arrangement-editing.md)).
- A plug-in scan notes which VST3 plug-ins support ARA (`plugin.list` shows `ara`), and such a
  plug-in is never given a host process of its own, since its document and every instance
  editing it share one ([ADR 0028](docs/decisions/0028-ara-hosting.md)).
- A plug-in that edits clips (ARA, such as a pitch editor) on an audio track is loaded with the
  track's arrangement audio clips: its editor shows them and it plays its edited version of them
  in time with the song. Its edits are saved with the project, and it follows the clips as they
  are moved, trimmed, added or deleted while it plays, without loading again; an undo brings a
  deleted clip back with its edits. Warped and looped
  clips and the overlap guard come later ([ADR 0028](docs/decisions/0028-ara-hosting.md)).
- Selecting an audio clip on such a track shows the plug-in's editor in the detail area, where a
  MIDI clip has its piano roll, with that clip shown in it; the clip's menu (**Open in** the
  plug-in) and Alt+M open it, adding the installed one (Melodyne first) to the track when it has
  none. The plug-in goes first in its track's chain
  ([clip editor](docs/spec/ui-clip-editor.md) › Where it opens).
- `arrangement.audioOverlaps` lists the audio clips that sound together on one track because their
  spans overlap, clips that only touch left out, for agents and the CLI; the first step towards ARA
  ([ADR 0028](docs/decisions/0028-ara-hosting.md)).
- Arrangement clips split and consolidate: Ctrl+E cuts the selected clip at the playhead and
  a clip's right-click menu has Split here (`clip.split`, the second part playing on from where
  the clip was); Ctrl+J or Consolidate turns a range of one track into one clip, MIDI by joining
  the notes, audio by rendering the clips without devices to a WAV file next to the project
  (`clip.consolidate`, [spec](docs/spec/arrangement-editing.md#splitting)).
- Tracks: click a header to select it (Ctrl adds), Ctrl+G groups the selection, drag a header
  to move the track (a group moves with its tracks); right-click a header or a Launcher column for
  name, colour, group, ungroup and delete. Clips: right-click or Delete and Ctrl+D.
- Track colours show at a glance: an Arrangement header has a band of the track's colour down
  its edge, a Launcher column's header and its channel strip's name are filled with it, and the
  strip has a band down its left edge. A click on a band opens the colour palette (the theme's eight
  track colours, the same swatches as the track's menu and a colour tag's menu); a swatch sets the
  colour in one undo step (`track.setColor`, [spec](docs/spec/ui-session-view.md#track-colour)).
- Groups inside groups, up to 16 levels ([#209](https://github.com/suhina-daw/suhina-daw/issues/209)):
  the sound goes through every group's volume, mute and devices to the output, solo follows the
  nesting, and the fold triangle on a group's header hides its tracks in both views. Drag a header
  into a group, or use Move into group / Move out of group in its menu (`track.setGroup`); nested
  groups are saved and read from Live Sets ([spec](docs/spec/arrangement-editing.md#groups-inside-groups)).
- A double-click on an empty Launcher slot or an empty spot of an Arrangement lane makes an empty
  clip of the track's kind — MIDI on a notes track, an audio clip with no file yet on an audio
  track — a bar long and on the grid (`clip.create`, also over MCP); in the Arrangement it is
  selected ([spec](docs/spec/arrangement-editing.md#making-an-empty-clip)).
- Clips: Ctrl-click adds one to the selection and Shift-click takes the run up to one, Ctrl+A
  selects the lot, and Ctrl+C, Ctrl+X, Ctrl+D, Delete and 0 (clip mute) act on the whole
  selection as one undo step ([spec](docs/spec/arrangement-editing.md#selecting-clips)). Header
  clicks take a run of tracks the same way.
- A group's triangle folds its tracks away into one lane, which stands in for the track being
  worked on (its clips, waveform and colour), or the first track inside it; a Stems file's group
  arrives folded ([spec](docs/spec/arrangement-editing.md#folded-groups)).
- Waveforms in clips, the browser's preview and the sample map's tooltips are drawn by one
  renderer ([spec](docs/spec/waveforms.md)): peaks at three resolutions with RMS, computed once
  when a file is decoded; a filled, smoothed shape with a brighter RMS band; left and right
  lanes in tall clips; the samples with dots when zoomed in past one sample per pixel; a linear
  or dB scale (Settings › Appearance › Waveforms).
- Audio clips show their gain, which a drag changes (`clip.setGain`); the waveform follows it
  while it is dragged.
- Audio clips on the arrangement fade in and out: handles at a clip's top corners set the fades
  on the grid (Alt for free, double-click removes), a right-click on a handle picks the shape
  (linear, equal power, fast, slow, S-curve) and a round handle on the fade bends its curve, the
  fades are drawn over the waveform, and
  the engine plays them sample-accurately in renders and playback (`clip.setFades`,
  [spec](docs/spec/arrangement-editing.md#fades)).
- Launched audio clips play their own fades: the fade-in once from the launch, the fade-out at
  the end marker of a clip without a loop (`clip.setFades` with `scene`,
  [spec](docs/spec/clip-launching.md#suhina-decisions)).
- Several projects in **tabs** (ADR 0007): a tab row once two are open, Ctrl+Tab between them,
  only the active one plays; opening one while another is open asks (new tab or replace, with
  Remember and a setting). A project file dragged over the window darkens it.
- Settings › Audio: the driver (WASAPI, or ASIO in a build with the `asio` feature), the output
  device, sample rate and buffer size as dropdowns of what the driver allows, WASAPI exclusive
  mode (off by default), and what plays now with its measured latency and underruns. A change
  reopens the output with the transport where it was and the plug-ins running; the engine
  renders in blocks of the buffer size, so the plug-ins' deadline follows it. A device that is
  gone at start plays the default output, with a notice. Still to do: the ASIO driver's control
  panel ([#176](https://github.com/suhina-daw/suhina-daw/issues/176)).
- A new audio output is inaudible apart from the moment it takes to open: the play start marker
  stays, launched clips play on from where they were, waiting launches wait again, and the
  launch quantisation and a scene's tempo carry over
  ([#178](https://github.com/suhina-daw/suhina-daw/issues/178)).
- When the chosen audio device is unplugged or missing at start, the default output plays in its
  place (Settings › Audio › Now says "until … is back"); Suhina looks every few seconds and moves
  back to the chosen device when it is connected again, with a notice
  ([#177](https://github.com/suhina-daw/suhina-daw/issues/177)).
- **MIDI controllers** play the armed tracks' instruments, built in or plug-ins, with the
  transport running or stopped, and with the sustain pedal
  ([ADR 0017](docs/decisions/0017-inputs-and-latency.md)): an arm button in the Launcher mixer
  and the Arrangement headers (`track.setArm`; a click arms one track, Ctrl adds), every MIDI
  input port opened and Settings › MIDI to turn one off (`midi.inputs`,
  `midi.setInput`); a controller plugged in later is found within seconds. The latency follows
  the buffer size.
- **Auto-arm the selected track:** with Settings › MIDI › Auto-arm on, selecting a track arms
  it (a MIDI or an audio track, also by clicking one of its clip slots) and lets go of the one it
  armed before, so a MIDI controller or the audio input plays into the track in view; tracks
  armed by hand stay armed, returns and groups are skipped, and the arm stays put while a take
  records ([#834](https://github.com/suhina-daw/suhina-daw/issues/834), ADR 0017).
- **Audio input and monitoring:** choose an input device in Settings › Audio (off by default);
  each audio track hears one channel or a stereo pair of it, monitored while armed (Auto),
  always (In) or never (Off), through its devices and mix (`track.setAudioInput`); arm many
  tracks at once to hear a band on separate tracks; the source and the monitor sit on the
  track's strip — Audio From and the three buttons — where the picker is visible without a
  menu ([#267](https://github.com/suhina-daw/suhina-daw/issues/267), ADR 0017).
- **A MIDI input per track:** each track hears every MIDI port, one port, or none, and every
  channel or one, so a keyboard and a pad controller (or two players) each play their own armed
  track; set from the track's menu or the Arrangement header (`track.setMidiInput`), saved with
  the project ([#278](https://github.com/suhina-daw/suhina-daw/issues/278), ADR 0017).
- **Lower latency for live playing:** while a track is armed the engine keeps one block queued
  ahead of the device instead of two, and Settings › Audio and `audio.info` say how long a key
  takes to be heard (`live_latency_ms`); at 128 frames, at most about 5 ms plus the device
  ([#265](https://github.com/suhina-daw/suhina-daw/issues/265), ADR 0017).
- **Notes keep their timing:** a note from a MIDI controller is played a fixed delay after the
  time the driver stamped it with, at the frame where that falls, instead of on the next block's
  first frame, so fast playing no longer jitters by up to a block
  ([#269](https://github.com/suhina-daw/suhina-daw/issues/269), ADR 0017).
- **Take lanes and comping:** recording into the arrangement draws the take as it grows, with
  its notes or waveform, and keeps every pass over the same beats (the loop wrapping, a click back
  on the ruler) as a take lane under the track, the latest on the track and playing; a click on a
  take's clip, or a drag across part of a take, copies it onto the track (`take.comp`), and takes
  can be deleted (`take.delete`) or folded away
  ([#909](https://github.com/suhina-daw/suhina-daw/issues/909)).
- **Copying samples out:** Copy to folder… in a sample's menu in the browser or the sample map
  copies it, or every selected one, into a folder (`library.copyTo`, numbered names, nothing
  overwritten), and samples dragged out of Suhina onto the file manager or the desktop are copied
  there on Windows and macOS ([#865](https://github.com/suhina-daw/suhina-daw/issues/865)).
- **Sample map labels:** a Labels control (Off, 50, 75, 100 %) beside Size scales or hides the
  cluster names, kept as a setting ([#864](https://github.com/suhina-daw/suhina-daw/issues/864)).
- **Recording:** an armed track records into an empty clip slot from the next launch boundary
  (the slot's record button, `clip.record`; a second click ends the take on the next boundary
  and plays it, looped) or into the arrangement (the control bar's record button,
  `transport.record`), with a count-in of 1, 2 or 4 bars (`transport.setCountIn`). Tracks that
  play notes record what their MIDI controllers play; tracks with an audio input record it to a
  WAV file in `Recorded/` next to the project, written off the audio thread. Takes are lined up
  with what was heard (the key-to-sound and input latency taken off), and each take is one undo
  step ([#266](https://github.com/suhina-daw/suhina-daw/issues/266),
  [#268](https://github.com/suhina-daw/suhina-daw/issues/268),
  [spec](docs/spec/recording.md)).
- A buffer size is never asked of a driver that did not declare it: the dropdown lists the sizes
  the device's own range allows, a range holding none of the engine's sizes (or no range at all)
  leaves the driver its own buffer size and the notice says which size plays, and on WASAPI shared
  mode the row is *Engine block* — Windows sets the device's buffer there, so the number is
  Suhina's render block and the note names the period Windows gives the device, in milliseconds.
  A device is not given up over a size its driver refuses: the same output is tried without one
  before anything falls back.
- Settings › Plug-ins: the folders scanned (add with a format, remove, switch off), Scan and
  Rescan all in the background, and the plug-in manager.
- Every key is the user's to change: Settings › Keys and `keymap.json` bind any command
  that needs no parameters, the actions on the selection (copy, cut, paste, duplicate, delete,
  group) and the window's own ([shortcuts](docs/shortcuts.md)).
- A first **piano roll** ([#200](https://github.com/suhina-daw/suhina-daw/issues/200),
  [spec](docs/spec/ui-clip-editor.md)): selecting a MIDI clip in the Arrangement view shows its
  notes in the detail area beside its properties, with a keyboard, a grid with its own grid value,
  and a velocity lane. A double-click draws or deletes a note; a click,
  Shift+click or a rectangle selects; dragging moves notes (by grid steps, Alt for free) or their
  right end resizes them; Delete deletes the selected notes; dragging a stem changes velocity;
  the wheel scrolls and Ctrl (with Shift, or Alt alone, for key height) zooms. Every edit is a `note.*` command
  (`note.list`, `.add`, `.delete`, `.move`, `.resize`, `.setVelocity`), one undo step each, that
  the CLI and MCP run too.
- Editing the notes of a **duplicated clip** changes that clip only: its first note edit gives
  it its own copy of the notes, in the same undo step
  ([#352](https://github.com/suhina-daw/suhina-daw/issues/352)). A shared pattern stays shared.
- **Quantise** in the piano roll ([#345](https://github.com/suhina-daw/suhina-daw/issues/345)):
  Ctrl+U moves the selected notes, or every note, onto the grid; the **Q** button over the keys
  sets the grid (triplets too), how far notes move (0 to 100 %) and whether their ends move.
  It is the command `note.quantize`, one undo step, that the CLI and MCP run too.
- **Keyboard editing in the piano roll** ([#344](https://github.com/suhina-daw/suhina-daw/issues/344)):
  after a click in it, the arrows move the selected notes by a grid step or a semitone
  (Shift+Up and Down by an octave), Shift+Left and Right change their length, Ctrl+A selects every
  note and Escape clears; Ctrl+C, X, V and D copy, cut, paste and duplicate notes (`note.copy`,
  `.cut`, `.paste`, `.duplicate`), pasting after the selection or at the playhead.
- **Piano roll, step 2** ([#442](https://github.com/suhina-daw/suhina-daw/issues/442)): its
  splitter pulls it all the way up (a double-click too), leaving only the edited track above it,
  lit, or nothing (Settings › Clip editor › Piano roll); Alt+wheel zooms its key rows; a toolbar with
  the tools Select, Draw (B), Paint and Erase, each stroke one undo step; and its own grid value,
  so the control bar's sets the Arrangement view's only, unless Settings links the two.
- **Piano roll, step 3** ([#443](https://github.com/suhina-daw/suhina-daw/issues/443)): the
  toolbar's tools are icons drawn in code, with Cut, Copy, Paste, Duplicate (the same as Ctrl+X,
  C, V and the duplicate key) beside them and the keys 1 to 5 choosing Select, Draw, Paint, Erase
  and **Slice**; a slice drag splits every note it crosses (`note.split`, one undo step, in the
  CLI and MCP too); with the right button held the wheel steps through the tools; Shift and Draw
  lays notes back to back at the last note's length; the quantise controls moved into the clip
  column on the left (nothing floats over the grid) and Ctrl+U still quantises at once; the key
  column plays through the track's instrument while a key is held (`preview.noteOn`,
  `preview.noteOff`), with Ctrl+click and Ctrl+drag selecting the notes of a pitch or a range of
  them; and Settings › Keys has the presets **Select-first** (Ctrl+D duplicates) and
  **Draw-first** (Ctrl+B duplicates, a right-click in Draw and Paint deletes), kept in
  `keymap.json`.
- **Piano roll, step 4** ([#460](https://github.com/suhina-daw/suhina-daw/issues/460)): the toolbar
  takes a size in Settings › Clip editor › Piano roll (Normal, Large, Larger, Large at first) and its
  six tool icons are drawn as outlines; the **Mute** tool (key 6) mutes and unmutes the notes a
  click or drag touches, and a muted note keeps its place in the clip, is drawn faded and is never
  played (`note.setMuted`, and `Note.muted` in the model, the `.suhina` file and a Live Set's
  `IsEnabled="false"`); a **MIDI tools column** beside the notes holds quantise (Ctrl+U still
  works) and the tools Transpose, Humanise, Fit to scale, Invert, Reverse, Legato, ×2, ÷2 and the
  velocity Randomise and Ramp, each one command (`note.humanize`, `.fitToScale`, `.invert`,
  `.reverse`, `.legato`, `.stretch`, `.randomizeVelocity`, `.rampVelocity`) and one undo step; the
  clip column lists what is selected (Notes "N of M", From, To, Pitch, Velocity); and
  Settings › Keys gains the **Mouse-first** preset, which gives the mouse the four jobs
  (left draws, right deletes, Shift+right slices, Alt+right plays from the point) as switches of
  their own, kept in `keymap.json`.
- **Piano roll workflow modes** ([#490](https://github.com/suhina-daw/suhina-daw/issues/490)): a
  workflow menu in the piano roll's toolbar switches between Select-first, Draw-first, Mouse-first
  and **Custom** at any time; every behaviour that differs between them (click on empty grid,
  right-click on a note, Shift+right, Alt+right, the duplicate key, double-click on empty grid and
  on a note) is a setting of its own, and changing one makes the mode Custom, kept in
  `keymap.json`. A double-click is counted by the piano roll itself, so it works right after
  another click, adds one note in Mouse-first instead of adding and deleting it, and mutes or
  erases one note in the Mute and Erase tools.
- **The piano roll's grid menu** ([#346](https://github.com/suhina-daw/suhina-daw/issues/346)): the
  grid value at the right end of the piano roll's toolbar opens one menu with **Snap to grid**, an
  **adaptive grid** in five widths whose lines get finer as the view zooms in and coarser as it
  zooms out, the **fixed** values from 8 bars to 1/32, **Narrow grid** and **Widen grid**, and
  **Triplet grid**, each with its key: Ctrl+1 and Ctrl+2 make the grid finer and coarser, Ctrl+3
  toggles triplets and Ctrl+4 snapping (in the keymap). With snapping off the lines stay and notes
  are placed freely.
- **The piano roll's ruler** ([#353](https://github.com/suhina-daw/suhina-daw/issues/353)): an
  8 px loop row shows the clip's loop as a bar with a handle at each end, and the bar numbers under
  it the clip's start marker and, for a clip that does not loop, its end marker; dragging a handle,
  the bar or a marker changes the loop, the start or the length on the piano roll's grid (Alt
  free), one undo step each (`clip.setLoop {start, end}`, which now takes arrangement clips too,
  `clip.setStart`, `clip.setLength`), and a click on the bar turns the loop on or off. Notes
  outside the part the clip plays are shaded like the grid past its end. Dragging the ruler up and
  down zooms time, sideways scrolls it; **Z** zooms to the selected notes and **Shift+Z** to the
  whole clip.
- **The piano roll's note gestures** ([#351](https://github.com/suhina-daw/suhina-daw/issues/351)):
  dragging a note's left edge moves the selection's starts and keeps their ends (on the grid, Alt
  free; `note.resize {startBy}`); Ctrl+drag (Cmd on macOS) copies the selection to where it is let
  go and selects the copies (`note.duplicate {beats, semitones}`); Alt+drag on a note changes the
  selection's velocity and shows the value; a drag from an empty spot of the velocity lane draws a
  line that sets every stem it spans in one step (`note.setVelocity {velocities}`); the lane's top
  edge drags it taller or shorter; and the toolbar's **Audition** button (off at start) makes drawn
  and moved notes sound once through the track's instrument (`preview.noteOn {keys}`).
- **Ghost notes in the piano roll** ([#348](https://github.com/suhina-daw/suhina-daw/issues/348)):
  the notes of other clips are drawn faintly behind the edited clip's, in their own track's colour,
  so a part can be written against another (a bass line against the chords). The toolbar's
  **Ghosts** menu shows the clips playing at the same time (the default; for a slot's clip, the
  other slots of its scene), the other clips of the same track, one clip picked from a list, or
  none. Ghost notes are mapped through each clip's start, offset and loop, never take a click, and
  the choice is a view setting kept until Suhina closes, not saved with the project.
- **Clip scales and folding in the piano roll** ([#347](https://github.com/suhina-daw/suhina-daw/issues/347)):
  a MIDI clip can have a scale (a root, C to B, and a mode: major, minor, harmonic and melodic
  minor, the church modes, the pentatonics and blues), saved with the project. The toolbar's
  **Scale** menu sets it (`clip.setScale`, one undo step) and the rows of its keys are lighter, with
  a mark on the key column; **Fold** shows only the keys that have notes and **Fold to scale** the
  scale's keys (and any key with a note), and clicks, drawing and drags work on the rows shown. The
  project's default scale (`project.setDefaultScale`) is taken by new MIDI clips, and
  `note.fitToScale` fits to the clip's scale when given none.
- **The piano roll in a short detail area:** it keeps a least height (120 px: the toolbar, the
  ruler and five key rows, with the clip column whole beside them) and its notes stay centred in
  view as the height changes until the rows are scrolled by hand; it closes by dragging the
  splitter down, by the **×** at the end of its toolbar or by **View › Hide editor**
  (Ctrl+Shift+E), and selecting a clip opens it again. A click on an empty spot of the Arrangement
  view clears the clip selection, and a lane pulled up with the piano roll shows its clip's notes.
- A **detail area** under the view (or a column beside it, Settings › Devices › Device panels) shows the selected
  clip's properties or the selected track's devices with their parameters, and each plug-in's
  state with a button that opens its editor or restarts it; Shift+Tab hides it.
  Every change from it is a device command (`device.add`, `device.remove`, `device.setParam`, …).
- **A MIDI clip's track devices one click away** ([#505](https://github.com/suhina-daw/suhina-daw/issues/505)):
  the clip column's **Clip / Devices** switch (Ctrl+Shift+D, View › Show devices) puts the track's
  device chain where the piano roll was and back, and a double-click on a track's header, a Session
  column's name or its channel strip shows that track's devices, opening the detail area if it was
  hidden ([spec](docs/spec/ui-clip-editor.md)).
- In the Launcher view a click on a clip's name selects it and its mark launches it; the detail area
  shows a slot's clip (scene, length, loop, gain, fades). Launching selects the clip too (Settings ›
  Launcher and mixer › Select on launch), Enter launches the selected slot and the arrow keys
  move through the slots ([spec](docs/spec/ui-session-view.md)).
- Still to do: fades read from Live Sets ([#103](https://github.com/suhina-daw/suhina-daw/issues/103)),
  input routing, arm, level meters, device editing
  ([#64](https://github.com/suhina-daw/suhina-daw/issues/64)) and the rest of the clip editor
  ([#200](https://github.com/suhina-daw/suhina-daw/issues/200)) in the detail area, installers and
  signed builds ([#180](https://github.com/suhina-daw/suhina-daw/issues/180)).

- Sampler: Poly/Mono selection, mono glide control, pitch attack/decay envelope, live sample read head, and recording controls beside the editor tabs.

- Sampler panel: full-width waveform with context-sensitive S/E source/loop handles, grouped controls, fine drag calibration, compact filter dials and 12/24 buttons, and LFO modulation of pitch/filter/volume/pan within a 200-pixel device height.

- Sampler envelope editing: AMP/PITCH tabs with stage points and a responsive release display; playback-mode changes stop held voices.

- Sampler voice controls: independent polyphonic waveform heads, smooth voice replacement, Retrigger, filter response buttons, 12/24 slopes, filter ADSR and velocity/key cutoff modulation.

- Sampler has waveform LFO selection including Random, independent source Fade In/Out, conditional loop fields and adjacent header Input/Record controls.

- Sampler offers Slices playback with Transient, Beat, Region and editable Manual boundaries, mapped from the root note.

- Sampler envelope graphs have padded dark surfaces, large editable points, cumulative stage timing and direct point dragging without value jumps.

- Sampler waveform: dynamic manual slice maps with double-click add/remove, draggable
  lines, gray areas outside S/E, anchored zoom and horizontal navigation.

- Sampler LFO rate supports Hz, milliseconds, beat divisions, triplets, dotted notes and sixteenth-note counts.

- Drop one audio file onto an empty MIDI track to add Sampler with that source.

- Project-open audio references stay inside the project folder; external files require explicit relinking.

- Sampler Loop playback uses Fade in and Fade out for its edges, without a separate Loop fade field or a shortened loop.
