# Attribution

Every non-code asset in this repository, with its author, source and license. Suhina's own
code and original assets are GPL-3.0-or-later (see [`LICENSE`](LICENSE) and [`NOTICE`](NOTICE)).
When you add an asset (sample, impulse response, preset, font, icon, image), add a row here in the
same change. Third-party assets must have a GPLv3-compatible license (for example MIT, BSD, ISC,
Apache-2.0, OFL, CC0, CC BY) and keep their license file next to them.

## Bundled in the app

| Path | Title | Author | Source | License |
|---|---|---|---|---|
| `crates/ui/fonts/manrope/Manrope-Variable.ttf` | Manrope (default UI typeface) | The Manrope Project Authors | [google/fonts](https://github.com/google/fonts/tree/main/ofl/manrope) | OFL-1.1 (`crates/ui/fonts/manrope/OFL.txt`) |
| `crates/ui/fonts/encode-sans/EncodeSansSemiExpanded-Regular.ttf` | Encode Sans Semi Expanded Regular (the UI typeface) | The Encode Project Authors (Pablo Impallari, Andres Torresi, Jacques Le Bailly) | [google/fonts](https://github.com/google/fonts/tree/main/ofl/encodesanssemiexpanded) | OFL-1.1 (`crates/ui/fonts/encode-sans/OFL.txt`) |
| `crates/ui/fonts/ibm-plex-mono/IBMPlexMono-Regular.ttf` | IBM Plex Mono Regular (numbers, times, positions) | IBM Corp. | [google/fonts](https://github.com/google/fonts/tree/main/ofl/ibmplexmono) | OFL-1.1 (`crates/ui/fonts/ibm-plex-mono/OFL.txt`) |
| `crates/ui/themes/dracula.theme.css` | Dracula colour palette, mapped onto Suhina's tokens | Dracula Theme | [dracula/dracula-theme](https://github.com/dracula/dracula-theme) | MIT (`crates/ui/themes/licenses/dracula.txt`) |
| `crates/ui/themes/nord.theme.css` | Nord colour palette, mapped onto Suhina's tokens | Sven Greb | [nordtheme/nord](https://github.com/nordtheme/nord) | MIT (`crates/ui/themes/licenses/nord.txt`) |
| `crates/ui/themes/catppuccin-mocha.theme.css`, `catppuccin-latte.theme.css` | Catppuccin Mocha and Latte colour palettes, mapped onto Suhina's tokens | Catppuccin | [catppuccin/palette](https://github.com/catppuccin/palette) | MIT (`crates/ui/themes/licenses/catppuccin.txt`) |
| `crates/ui/themes/solarized-dark.theme.css` | Solarized colour palette (dark), mapped onto Suhina's tokens | Ethan Schoonover | [altercation/solarized](https://github.com/altercation/solarized) | MIT (`crates/ui/themes/licenses/solarized.txt`) |
| `crates/ui/themes/editor-dark.theme.css`, `suhina-light.theme.css`, `high-contrast.theme.css` | Suhina's own themes | Suhina contributors | Original work | GPL-3.0-or-later |
| egui's default fonts (`epaint_default_fonts` crate, compiled in as fallbacks) | Ubuntu Light, Hack, Noto Emoji, emoji-icon-font | Canonical / Dalton Maag; Source Foundry; Google; John Slegers | [crates.io: epaint_default_fonts](https://crates.io/crates/epaint_default_fonts) | Ubuntu Font Licence 1.0, MIT, OFL-1.1 and Apache-2.0 (per font, in the crate) |

## Documentation

| Path | Title | Author | Source | License |
|---|---|---|---|---|
| `docs/design/reference/session-minimal-v3.png`, `arrangement-minimal-v3.png` | Suhina Session and Arrangement design mockups (minimal v3) | Suhina contributors | Original work, exported from the project's design canvas | GPL-3.0-or-later |

## Ported code

Code translated from other open-source projects; each file keeps its notices and says what
changed. The full list with copyright lines is in [`NOTICE`](NOTICE).

| Path | Ported from | Author | Source | License |
|---|---|---|---|---|
| `crates/stretch` | Signalsmith Stretch, Signalsmith Linear (STFT) | Geraint Luff / Signalsmith Audio | https://github.com/Signalsmith-Audio/signalsmith-stretch, https://github.com/Signalsmith-Audio/linear | MIT |
| `devices/fm-synth/src/{tables,env,lfo,core,voice}.rs` | MSFA engine as found in Dexed | Google Inc., Pascal Gauthier, Jean Pierre Cimalando | https://github.com/asb2m10/dexed (`Source/msfa`) | Apache-2.0 |

## Vendored code

Third-party source kept in `third_party/`. The ASIO SDK's host part is unchanged and built only
with the `asio` feature. LAME's encoder sources are unchanged and always built (for MP3 export),
with Suhina's own `fft.c` in place of upstream's, which is not redistributed.

| Path | Title | Author | Source | License |
|---|---|---|---|---|
| `third_party/asiosdk` | Steinberg ASIO SDK 2.3.4, host part (`asio` feature) | Steinberg Media Technologies GmbH | https://www.steinberg.net/asiosdk | GPL-3.0 (`third_party/asiosdk/LICENSE.txt`; see `VENDORED.md`) |
| `third_party/lame` | LAME 3.100, the MP3 encoder's sources | The LAME Project (Mike Cheng, Mark Taylor and the authors named in its README) | https://downloads.sourceforge.net/project/lame/lame/3.100/lame-3.100.tar.gz | LGPL-2.0-or-later, used as LGPL-2.1-or-later (`third_party/lame/COPYING`; see `VENDORED.md`) |

## Test data in the repository

| Path | Title | Author | Source | License |
|---|---|---|---|---|
| `devices/fm-synth/tests/golden/scenarios.txt` | FM synth test voices and notes | Suhina contributors | Original work | GPL-3.0-or-later |
| `devices/fm-synth/tests/golden/*.f32` | Reference renders of those scenarios | Suhina contributors | Rendered with the MSFA engine by `tests/reference/harness.cpp` | GPL-3.0-or-later |
| `devices/fm-synth/tests/reference/harness.cpp`, `stubs/` | The reference renderer and the stand-in headers it builds Dexed's MSFA sources with (a logger, scale tuning, the MTS-ESP client: declarations only, so the build needs nothing else); not built into Suhina, and no Dexed source is committed | Suhina contributors | Original work | GPL-3.0-or-later |
| `crates/engine/tests/golden/*.f32` | Reference renders of clip fades | Suhina contributors | Rendered by Suhina's engine (`src/fade_tests.rs`) | GPL-3.0-or-later |
| `crates/media/tests/data/tones-*.stem.mp4`, `tones-alac.stem.m4a` | Stem files of sine tones | Suhina contributors | Generated with ffmpeg by `scripts/stems-fixture.py` | GPL-3.0-or-later |

## Test data (not committed, not shipped)

`corpus/` is gitignored. Test projects are authored by Suhina contributors (see
[`CONTRIBUTING.md`](CONTRIBUTING.md) › Clean-room) and are GPL-3.0-or-later. Never add projects,
samples, presets or Packs that ship with a commercial DAW.

| Path | Title | Author | Source | License |
|---|---|---|---|---|
| _(none yet)_ | | | | |
