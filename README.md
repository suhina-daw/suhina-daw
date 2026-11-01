# Suhina
<img width="12525" height="3663" alt="suhina-logo" src="https://github.com/user-attachments/assets/7efa72b4-fba5-42f3-9503-0ea9f4e864f6" />

<p align="center">
  <a href="https://loupystudio-software.itch.io/suhina"><img alt="Itch" src="https://img.shields.io/badge/Itch.io-download-orange?logo=itchdotio"></a><a href="https://discord.gg/rgwp4DDNsU"><img alt="Discord" src="https://img.shields.io/discord/1558859596783689758?label=Discord&logo=discord&logoColor=white&color=5865F2"></a>
</p>

An open-source, clip-launching DAW in pure Rust, comparable in workflow to Ableton Live.

Suhina was called Loopwerk until October 2026; settings kept under the old name carry over on the first start. <!-- rename_suhina: keep -->

<a href="docs/research/parity-live-12-by-area.png">
  <img width="700" alt="Parity by area: the share of each area of a reference DAW's manual that Suhina covers today" src="docs/research/parity-live-12-by-area.png">
</a>

> How much of that workflow exists today, measured chapter by chapter against a reference DAW's
> manual: [the report](docs/research/parity-live-12.md), [the order the gap gets closed in](docs/decisions/0016-parity-target.md).
> Rebuilt on `main` whenever the numbers change.

> **Status:** pre-alpha. Suhina opens Live Sets, plays and renders them (audio clips with
> real-time time-stretching, MIDI through the built-in FM synth, sandboxed CLAP and VST3 plug-ins,
> or sine tones), and lets agents edit them over MCP. The desktop app has the Launcher and
> Arrangement views, a browser with a sample map, and settings. See [`ROADMAP.md`](ROADMAP.md) for what works and what comes next.

Suhina is an independent clean-room project. It is not affiliated with, sponsored by or
endorsed by Ableton AG or any other company named here. Ableton and Live are trademarks of Ableton AG. VST is a registered trademark and ASIO a
trademark of Steinberg Media Technologies GmbH. DX7 is a trademark of Yamaha Corporation. Stems
and NKS are trademarks of Native Instruments. Other product names are trademarks of their
owners. They are named only to describe file compatibility and comparable workflows. Suhina contains no code or content from Ableton
products; see [`CONTRIBUTING.md`](CONTRIBUTING.md) › Clean-room.

## Goals

Sampler provides single-source sample and granular playback with an SDK panel,
editable waveform and sample loading by drag and drop.
Manual slice maps support double-click add/remove and draggable boundaries; the
waveform supports anchored zoom and horizontal navigation.

- A real-time audio engine that never glitches, with offline rendering on the same code path.
- Session-style clip launching with quantisation, plus a linear arrangement.
- Reading `.als` projects you made yourself; later other formats such as DAWproject and `.flp`,
  so projects are not locked to one program.
- CLAP and VST3 plug-in hosting, sandboxed outside Suhina's process: a crashing plug-in never takes the project down.
- Every action available headlessly: CLI and MCP for automation and agents.

## Downloads

To try the current state, download the **nightly**: a build of `main` made every night, not a
release, so expect unfinished parts and bugs
([nightly downloads](https://github.com/suhina-daw/suhina-daw/releases/tag/nightly)).
It and tagged versions hold the desktop app `suhina`, the command-line tool `suhina-cli` and the
plug-in host `suhina-plugin-host`, for Linux (x86_64), Windows (x64) and macOS (universal). The
host is the process plug-ins run in: keep the three in one folder. On macOS the package holds
`Suhina.app`, which you can drag to Applications; the other two are inside it. Tell us what works and what breaks
on [Discord](https://discord.com/invite/e5JAZYMU) or in the
[issues](https://github.com/suhina-daw/suhina-daw/issues). The binaries are not signed yet,
so macOS and Windows warn before running them. On Linux they need the ALSA library (`libasound2`).
How a release is made: [`docs/releasing.md`](docs/releasing.md).

## Try it

```sh
# Open a Live Set in the desktop app and launch its clips and scenes with the mouse; the
# plug-in host is built first, since the app runs plug-ins in it (scan in Settings › Plug-ins)
cargo build --release -p suhina-plugin-host
cargo run --release -p suhina -- path/to/set.als

# Play a Live Set on the default audio device (release builds play heavy sets smoothly)
cargo run --release -p suhina-cli -- play path/to/set.als

# Play one launcher scene instead of the arrangement
cargo run --release -p suhina-cli -- play path/to/set.als --scene 2

# Launch clips and scenes by typing commands (see below)
cargo run --release -p suhina-cli -- play path/to/set.als --launcher

# Render to a 32-bit float WAV
cargo run --release -p suhina-cli -- render path/to/set.als -o out.wav

# Play every MIDI track on the built-in FM synth, with voice 3 of a DX7 bank (.syx)
cargo run --release -p suhina-cli -- play --demo --fm --voices path/to/bank.syx --voice 3

# List the voices in a DX7 voice or bank file
cargo run --release -p suhina-cli -- fm-synth voices path/to/bank.syx

# Serve the Model Context Protocol for agents (see docs/mcp.md)
cargo run --release -p suhina-cli -- mcp --read-root path/to/projects --write-root path/to/renders
```

`--demo` instead of a file uses a small built-in project. `--fm` works with `play` and `render`,
for the demo or any set; without `--voices` the synth plays its initial voice, a plain sine.

With `--launcher` the set starts stopped and Suhina reads one command per line:

| Type | Does |
|---|---|
| `scene 2` | launch scene 2 |
| `clip Bass 1` | launch the clip of track *Bass* in scene 1 |
| `stop Bass`, `stopall` | stop one track, or every track |
| `play`, `stop` | start or stop the transport |
| `stop here` | stop the transport and stay where it stopped |
| `pos 16` | move the song position to beat 16 |
| `quant 1/4` | launch quantisation: `none`, `8 bars` … `1 bar`, or a note value from `1/2` to `1/32`, `T` for triplets (`1/8T`) |
| `back` | return every track to the arrangement |
| `state` (or `s`), `grid` | what is playing and queued; the scenes and clips |
| `metronome on`, `metronome on -12`, `metronome off` (or `click …`) | a click on every beat, higher on the bar; level in dB (−6 by default) |
| `tempo 100`, `vol Bass -6`, `mute Bass`, `unmute Bass` | edits, heard while playing |
| `undo`, `redo` | undo or redo the last edit |
| `track.setMute {"tracks": [1], "mute": true}` | any command by id, with JSON parameters |
| `help` (or `?`), `quit` (or `exit`) | |

## Building

Linux builds talk to the sound card through ALSA, so its development files and `pkg-config`
must be installed before building:

| Distribution family | Packages |
|---|---|
| Debian, Ubuntu, Mint | `libasound2-dev pkg-config` |
| Fedora, RHEL | `alsa-lib-devel pkgconf-pkg-config` |
| Arch, Manjaro, EndeavourOS | `alsa-lib pkgconf` |

Building the whole workspace also builds `suhina-plugin-host`, the program the desktop app
runs plug-ins in; it has to sit next to `suhina`:

```sh
cargo build --release
cargo test --workspace
```

### When a build cannot write files (Windows)

Every build is a new, unsigned program, and security software that judges programs by reputation
may stop it from writing: Suhina then says "writing was denied" when it saves its settings, and
tests fail with "Access is denied" (os error 5) in `%APPDATA%` or `%TEMP%`. Exclude the `target`
folder (or Cargo's `target-dir`) from scanning in the security software, or allow the programs in
it. In Windows Security that is *Virus & threat protection › Manage settings › Exclusions*, and
*Ransomware protection › Allow an app through Controlled folder access* when that is on.

When only programs started from one folder are denied, look at that folder's integrity label:
`icacls <folder>` lists `Mandatory Label\Low Mandatory Level` when another tool's sandbox left
one, and every program built inside it then runs at low integrity and cannot write to the user's
folders. Build into a folder without the label (Cargo's `build.target-dir` outside it).

### ASIO on Windows

Windows release, nightly and test packages include ASIO: in Settings › Audio, choose **ASIO**
under **Driver**, then an installed driver under **Device**. WASAPI remains available.
ASIO drivers are installed separately; the package does not install a driver.

A default local Windows build plays through WASAPI (shared, or exclusive when chosen in Settings ›
Audio). ASIO, which talks to an audio interface's own driver, is a Cargo feature, because
building it compiles the Steinberg ASIO SDK with the C++ compiler and generates its bindings with
bindgen, which needs libclang:

1. Install LLVM (for libclang), for example `winget install LLVM.LLVM`, and set
   `LIBCLANG_PATH` to its `bin` folder (`C:\Program Files\LLVM\bin`) if the build does not
   find it.
2. Build with the feature:

```sh
cargo build --release -p suhina -p suhina-cli -p suhina-plugin-host --features suhina/asio,suhina-cli/asio
```

The SDK's host files are in [`third_party/asiosdk`](third_party/asiosdk/VENDORED.md), used under
the GPL-3.0 that Steinberg offers it under since October 2025, and `.cargo/config.toml` points the
build at them (`CPAL_ASIO_DIR`); nothing is downloaded. The SDK is GPL-3.0 only, so a build with
`asio` is distributed under the GPL-3.0; Suhina's own code stays GPL-3.0-or-later. "ASIO" is a
trademark of Steinberg Media Technologies GmbH.

## Contributing

Everything that is planned or being worked on is on the board,
[**Suhina** (GitHub project)](https://github.com/orgs/suhina-daw/projects/1), and every item there
is an [issue](https://github.com/suhina-daw/suhina-daw/issues) in this repository. To start:

1. **Read the rules once:** [`CONTRIBUTING.md`](CONTRIBUTING.md) (clean-room, licences, sign-off)
   and [`AGENTS.md`](AGENTS.md) (golden rules, crate map, what to run before you finish). They
   apply to people and to AI agents alike.
2. **Pick an issue** on the board in *Backlog* or *Ready* with no open pull request for it; the
   [`good first issue`](https://github.com/suhina-daw/suhina-daw/labels/good%20first%20issue) and
   [`help wanted`](https://github.com/suhina-daw/suhina-daw/labels/help%20wanted) labels mark good
   places to begin. Say in the issue that you take it, with a short plan.
3. **Find the documents for it** in [`docs/INDEX.md`](docs/INDEX.md), by what you are about to do
   or by the code you change, and read [`docs/architecture.md`](docs/architecture.md) to see where
   the code goes.
4. **Build and test** as [Building](#building) shows; before the pull request run `cargo fmt --all`,
   `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` and
   `python scripts/docs.py check`.
5. **Open a pull request** that says `Closes #<issue>` and fills in the template.

Something missing or a bug? Open an issue with one of the forms, or ask on
[Discord](https://discord.com/invite/e5JAZYMU).

Community audio effects, MIDI effects and monophonic instruments can be authored with the Rust
device SDK and shared as one `.sd` file, then added to a track. Version 3 can preserve node positions,
port anchors and wire routes for a future visual editor. See the [authoring guide](docs/device-sdk.md) and
[node reference](docs/spec/sdk-nodes.md), including node discovery for AI agents.

## Documentation

| | |
|---|---|
| [`docs/INDEX.md`](docs/INDEX.md) | Every document, what it is for, and the code it describes |
| [`docs/commands.md`](docs/commands.md) | Every command the CLI, MCP server and UI run, with its parameters |
| [`ROADMAP.md`](ROADMAP.md) | Milestones, what works now, what is next |
| [`AGENTS.md`](AGENTS.md) | Rules for contributors and agents, and the crate map |
| [`docs/architecture.md`](docs/architecture.md) | How the parts fit: layers, data flow, threads, commands, where new code goes |
| [`CONTRIBUTING.md`](CONTRIBUTING.md) | Clean-room rules, licences, sign-off (DCO), AI-assisted contributions |
| [`IP.md`](IP.md) | Reporting a rights or clean-room problem, and how material is removed |
| [`docs/decisions/`](docs/decisions/) | Architecture decisions: the model, commands, time-stretching, devices, plug-in hosting, project files and more |
| [`docs/spec/`](docs/spec/) | Clean-room specs: file formats (Live Sets, stems, NKS, DX7 voices, `.suhina`) and behaviour (clip launching, editing, the browser, the views) |
| [`docs/mcp.md`](docs/mcp.md) | The MCP server: tools, file roots, errors |
| [`docs/shortcuts.md`](docs/shortcuts.md) | Keyboard shortcuts in the desktop app, and how to change them |
| [`docs/releasing.md`](docs/releasing.md) | How a release is built and published |

## License

Suhina is free software, licensed under the [GNU General Public License v3.0 or later](LICENSE).
Contributions are licensed the same way.

Rust authors can use checked graph handles and standard panel-control constructors to reduce
manual wiring and repeated UI setup ([SDK authoring guide](docs/device-sdk.md#checked-rust-authoring)).

SDK effect panels can include custom vector controls, XY pads, gradients and embedded PNG/JPEG
icons, photos and repeating backgrounds. Artwork and bindings travel inside the single `.sd`;
see the [custom panel guide](docs/device-sdk.md#custom-panels-images-and-controls).


The Rust SDK also offers prepared sample resources, private polyphonic voice/grain graphs,
slicing, native stretch and editable sampler/granular templates. Run the
[`sampler_composition` example](crates/device-sdk/examples/sampler_composition.rs) to save and
render loop, reverse, slice, granular and tempo graphs using one shared resident sample.
