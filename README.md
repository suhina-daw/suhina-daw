<h1 align="center">
  <a href="https://suhina.org"><img width="520" alt="Suhina" src="https://github.com/user-attachments/assets/7efa72b4-fba5-42f3-9503-0ea9f4e864f6"></a>
</h1>

<p align="center"><b>A free, open-source, clip-launching DAW in pure Rust, comparable in workflow to Ableton Live. For Windows, macOS and Linux.</b></p>

<p align="center">Launch clips and scenes, lay out the song on an Arrangement timeline, edit notes, automation and audio, and explore your samples on a map that groups them by how they sound. Built-in instruments and effects, CLAP, VST3 and VST2 plug-ins in a sandboxed host, themes written in CSS, and an AI agent that can work in your project over MCP. Opens your Ableton Live (<code>.als</code>) and FL Studio (<code>.flp</code>) projects. Drawn with egui, played through cpal, on an engine whose audio thread never waits.</p>

<p align="center">
  <a href="https://github.com/suhina-daw/suhina-daw/actions/workflows/ci.yml?query=branch%3Anext"><img alt="CI" src="https://img.shields.io/github/actions/workflow/status/suhina-daw/suhina-daw/ci.yml?branch=next&event=push&label=CI"></a>
  <a href="https://suhina.org/#download"><img alt="Download the nightly" src="https://img.shields.io/badge/download-nightly-orange"></a>
  <a href="LICENSE"><img alt="License GPL-3.0-or-later" src="https://img.shields.io/badge/license-GPL--3.0--or--later-blue"></a>
  <img alt="Rust 1.95 or newer" src="https://img.shields.io/badge/rust-1.95%2B-orange?logo=rust">
  <img alt="Pre-alpha" src="https://img.shields.io/badge/status-pre--alpha-red">
</p>

<p align="center">
  <a href="https://suhina.org"><img alt="Website" src="https://img.shields.io/badge/website-suhina.org-2ea44f"></a>
  <a href="https://discord.gg/rgwp4DDNsU"><img alt="Discord" src="https://img.shields.io/discord/1558859596783689758?label=Discord&logo=discord&logoColor=white&color=5865F2"></a>
  <a href="https://www.reddit.com/r/suhina/"><img alt="r/suhina on Reddit" src="https://img.shields.io/badge/reddit-r%2Fsuhina-FF4500?logo=reddit&logoColor=white"></a>
</p>

<p align="center">
  <a href="https://www.opensourcealternatives.to"><img src="https://www.opensourcealternatives.to/badge-osa.svg" alt="As seen on Open Source Alternatives"></a>
</p>

<p align="center">
  <a href="https://suhina.org">Website</a> •
  <a href="https://suhina.org/guide/">Guide</a> •
  <a href="#downloads">Download</a> •
  <a href="#features">Features</a> •
  <a href="#building">Build from source</a> •
  <a href="docs/architecture.md">Architecture</a> •
  <a href="ROADMAP.md">Roadmap</a> •
  <a href="#contributing">Contributing</a>
</p>

![Suhina's Arrangement: a song of seventy tracks, audio and MIDI clips in track colors along the timeline, with a clip's warp, gain and pitch controls below](https://suhina.org/assets/product/suhina-project.webp)

<table>
  <tr>
    <td><img src="https://suhina.org/assets/product/suhina-launcher.webp" alt="Suhina Launcher: columns of MIDI and audio clips above a mixer whose tracks hold a plug-in synth, a compressor, EQs and multiband effects"></td>
    <td><img src="https://suhina.org/assets/product/suhina-arrangement.webp" alt="Suhina Arrangement: lead, bass and drum clips along a timeline, with grouped tracks of plug-in synths in the track headers"></td>
  </tr>
  <tr>
    <td align="center">Launch clips and scenes in the Launcher</td>
    <td align="center">Lay out the song in the Arrangement</td>
  </tr>
  <tr>
    <td><img src="https://suhina.org/assets/product/suhina-theme-sheet.webp" alt="Suhina Session in the green and white Suhina Sheet theme"></td>
    <td><img src="https://suhina.org/assets/product/suhina-mixer-horizontal.webp" alt="Suhina Session in a dark theme whose mixer has horizontal faders and level meters"></td>
  </tr>
  <tr>
    <td align="center">The look is a theme, written in CSS</td>
    <td align="center">A theme can rearrange the mixer too</td>
  </tr>
</table>

## Status

<p align="center">
  <a href="https://github.com/suhina-daw/suhina-status">
    <picture>
      <source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/suhina-daw/suhina-status/main/status-dark.svg">
      <img width="680" alt="Suhina by area: how many features are done, in progress and planned, with the open bugs" src="https://raw.githubusercontent.com/suhina-daw/suhina-status/main/status.svg">
    </picture>
  </a>
</p>

How much of each area of Suhina works today. A box's size is its number of features, its fill the
share done; the red number is the area's open bugs. The maintainers count it from their feature
list and a bot redraws it every day ([suhina-status](https://github.com/suhina-daw/suhina-status)).
What comes next, and in what order: [`ROADMAP.md`](ROADMAP.md) and
[ADR 0016](docs/decisions/0016-scope.md).

> **Note:** Suhina is pre-alpha. Expect unfinished workflows, changing behavior and bugs, and
> work on copies of your projects: the nightly builds are development builds, not a stable release.

## Features

- **Launcher and Arrangement.** Launch clips and scenes with quantization and follow actions, and
  lay out the song on a timeline; both are views of the same project. Several projects open in
  tabs at once.
- **Editing.** A piano roll with scales, chords and quantize; automation lanes under each track;
  take lanes and comping for recorded audio; real-time time-stretching that follows warp markers and
  the song's tempo.
- **Instruments and effects.** FM Synth (six operators, reads DX7 voices), Sampler (sample, slice
  and grain playback), EQ, Compressor, Multiband, Limiter, Delay, Reverb, Saturator, Overdrive,
  Vinyl Distortion, DJ EQ, Auto Pan, Utility, NAM Amp Modeler, NAM Cab Loader and the Pitch MIDI
  effect. Every change to a control is one undo step, and automation can take it over.
- **Plug-ins.** CLAP, VST3 and VST2 instruments and effects run in a plug-in host process of their
  own: a plug-in that crashes or hangs never takes the project down. Experimental ARA support.
- **Sample map.** Your sample library laid out by sound, as a cloud or a grid: similar sounds
  become neighbors, and one good find leads to the next.
- **Themes in CSS.** The whole look is a theme in CSS with SVG and PNG artwork. Change a built-in
  theme's settings, rearrange the toolbar, or write your own and share it as one
  `.theme.suhina` file.
- **Agents and the command line.** Every action is a command with an id; the app, `suhina-cli` and
  an AI agent over MCP run the same ones, with one undo history. Agent access is off until you
  enable it.
- **Real-time engine.** The audio thread never allocates, locks or touches the disk; offline
  rendering runs the same engine code.
- **Your projects, not locked in.** Open Ableton Live (`.als`) and FL Studio (`.flp`) projects,
  bring in audio and MIDI, and save in Suhina's own format. FL Studio import brings patterns,
  samplers, audio clips, plug-ins with their state, mixer routing and automation, and names what it
  could not bring in; DAWproject comes next.
- **Your own devices.** Build audio effects, MIDI effects and instruments with the Rust device SDK
  and share each as a single `.sd` file ([Device SDK](#device-sdk)).

Suhina is an independent clean-room project. It is not affiliated with, sponsored by or
endorsed by Ableton AG, Image-Line or any other company named here. Ableton and Live are trademarks of Ableton AG. FL Studio is a trademark of Image-Line Software. VST is a registered trademark and ASIO a
trademark of Steinberg Media Technologies GmbH. DX7 is a trademark of Yamaha Corporation. Stems
and NKS are trademarks of Native Instruments. Other product names are trademarks of their
owners. They are named only to describe file compatibility and comparable workflows. Suhina contains no code or content from Ableton
or Image-Line products; see [`CONTRIBUTING.md`](CONTRIBUTING.md) › Clean-room.

## Goals

- A real-time audio engine that never glitches, with offline rendering on the same code path.
- Session-style clip launching with quantisation, plus a linear arrangement.
- Reading `.als` and `.flp` projects you made yourself, and later DAWproject, so projects are not
  locked to one program.
- CLAP and VST3 plug-in hosting, sandboxed outside Suhina's process: a crashing plug-in never takes the project down.
- Every action available headlessly: CLI and MCP for automation and agents.

## Downloads

Download the latest build for Windows, macOS or Linux from [suhina.org](https://suhina.org/#download).
It is the **nightly**: a build of `main` made every night, not a
release, so expect unfinished parts and bugs
([nightly downloads](https://github.com/suhina-daw/suhina-daw/releases/tag/nightly)).
It and tagged versions hold the desktop app `suhina`, the command-line tool `suhina-cli` and the
plug-in host `suhina-plugin-host`, for Linux (x86_64), Windows (x64) and macOS (universal). The
host is the process plug-ins run in: keep the three in one folder. On macOS the package holds
`Suhina.app`, which you can drag to Applications; the other two are inside it. Tell us what works and what breaks
on [Discord](https://discord.gg/rgwp4DDNsU) or in the
[issues](https://github.com/suhina-daw/suhina-daw/issues). The binaries are not signed yet,
so macOS and Windows warn before running them. On Linux they need the ALSA library (`libasound2`).
You download a nightly from GitHub only once: after that Suhina checks at each start whether a
newer nightly is out and says so, *New nightly available*, in a line on the start panel or a bar
over the open project (not a pop-up). Nothing is installed until you press **Download update
now**, which puts the new build in place of the running one and offers to restart Suhina
([`docs/spec/updates.md`](docs/spec/updates.md)).
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
must be installed before building (a running PulseAudio or PipeWire server is reached over its own
protocol, which needs nothing more):

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
   and [`AGENTS.md`](AGENTS.md) (golden rules, what to run before you finish, where to read more). They
   apply to people and to AI agents alike.
2. **Pick an issue** on the board in *Backlog* or *Ready* with no open pull request for it; the
   [`good first issue`](https://github.com/suhina-daw/suhina-daw/labels/good%20first%20issue) and
   [`help wanted`](https://github.com/suhina-daw/suhina-daw/labels/help%20wanted) labels mark good
   places to begin. Say in the issue that you take it, with a short plan.
3. **Find the documents for it** with `python scripts/docs.py find WORDS` (by what you are about
   to do) or `python scripts/docs.py covers PATH` (by the code you change), and read [`docs/architecture.md`](docs/architecture.md) to see where
   the code goes.
4. **Build and test** as [Building](#building) shows; before the pull request run `cargo fmt --all`,
   `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` and
   `python scripts/docs.py check`.
5. **Open a pull request** that says `Closes #<issue>` and fills in the template.

Something missing or a bug? Open an issue with one of the forms, or ask on
[Discord](https://discord.gg/rgwp4DDNsU).

## Device SDK

Community audio effects, MIDI effects and monophonic instruments can be authored with the Rust
device SDK and shared as one `.sd` file, then added to a track. Version 3 can preserve node positions,
port anchors and wire routes for a future visual editor. See the [authoring guide](docs/device-sdk.md) and
[node reference](docs/spec/sdk-nodes.md), including node discovery for AI agents.

Rust authors can use checked graph handles and standard panel-control constructors to reduce
manual wiring and repeated UI setup ([SDK authoring guide](docs/device-sdk.md#checked-rust-authoring)).

SDK effect panels can include custom vector controls, XY pads, gradients and embedded PNG/JPEG
icons, photos and repeating backgrounds. Artwork and bindings travel inside the single `.sd`;
see the [custom panel guide](docs/device-sdk.md#custom-panels-images-and-controls).

The Rust SDK also offers prepared sample resources, private polyphonic voice/grain graphs,
slicing, native stretch and editable sampler/granular templates. Run the
[`sampler_composition` example](crates/device-sdk/examples/sampler_composition.rs) to save and
render loop, reverse, slice, granular and tempo graphs using one shared resident sample.

## Documentation

| | |
|---|---|
| [`docs/INDEX.md`](docs/INDEX.md) | Every document, what it is for, and the code it describes |
| [`docs/commands.md`](docs/commands.md) | Every command the CLI, MCP server and UI run, with its parameters |
| [`ROADMAP.md`](ROADMAP.md) | Milestones, what works now, what is next |
| [`AGENTS.md`](AGENTS.md) | Rules for contributors and agents, and where to read more |
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
