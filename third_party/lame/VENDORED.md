# LAME 3.100 (vendored encoder sources)

The MP3 encoder of LAME 3.100 (2017-10-26), from the official upstream tarball
`https://downloads.sourceforge.net/project/lame/lame/3.100/lame-3.100.tar.gz`,
SHA-256 `ddfe36cab873794038ae2c1210557ad34857a4b6bdc515785d1da9e175b1da1e`
(checked 2026-10-20), used under the **GNU Library General Public License version 2,
or any later version** — for Suhina's purposes read as **LGPL-2.1-or-later** — which
keeps the whole vendored tree under one licence family. `COPYING` is the package's
licence text; three headers with no notice of their own (`libmp3lame/id3tag.h`,
`libmp3lame/lame_global_flags.h`, `libmp3lame/lameerror.h`) fall under it, and
section 3 of that licence lets a copy be used under the ordinary GNU GPL instead,
version 2 or any later version, which makes those files compatible with Suhina's
GPL-3.0-or-later either way. MP3's patents expired in 2017.

`suhina-mp3` (crates/mp3) compiles the kept files into a static library and exposes
a safe API; [ADR 0029](../../docs/decisions/0029-mp3-encoder.md) records the decision
and the licence reading.

## What is kept, unchanged

Upstream layout, under `third_party/lame/`:

- `COPYING`, `LICENSE`, `README`, `ChangeLog` as they come.
- `include/lame.h`.
- `libmp3lame/`: `bitstream.c`, `encoder.c`, `gain_analysis.c`, `id3tag.c`, `lame.c`,
  `newmdct.c`, `presets.c`, `psymodel.c`, `quantize.c`, `quantize_pvt.c`,
  `reservoir.c`, `set_get.c`, `tables.c`, `takehiro.c`, `util.c`, `vbrquantize.c`,
  `VbrTag.c`, `version.c`, and every header in `libmp3lame/`.
- `libmp3lame/vector/xmm_quantize_sub.c` and `libmp3lame/vector/lame_intrin.h`
  (SSE; built only on x86 and x86_64, see `crates/mp3/build.rs`).

## What is left out, and why

- **`libmp3lame/fft.c`** — the one file without an LGPL header: an informal note from
  its author instead, so it is not redistributed. Suhina's own `fft.c` takes its
  place: written from `fft.h`, the `fft_fht` field in `util.h` and the call sites in
  `psymodel.c` and a textbook description of the fast Hartley transform (Bracewell),
  never from the original file. Its header marks it as Suhina's, under
  LGPL-2.1-or-later like the rest of the tree. Verified against the original during
  development only: the windows are bit-identical, and the transforms agree with the
  original's within 5e-6 relative (the original computes in float internally).
- `libmp3lame/i386/` — x86 assembly; LAME's C path is used instead.
- `mpglib/` — the MP3 decoder; Symphonia already decodes MP3 for Suhina.
- `frontend/`, `ACM/`, `Dll/`, `mac/`, `macosx/`, `vc_solution/`, `dshow/`,
  `debian/`, `doc/`, `misc/`, `testcase.*` and the autotools files — frontends,
  build systems and documents Suhina does not build.

## Local changes

- `libmp3lame/fft.c` — replaced with Suhina's implementation (above).
- `config.h` — Suhina's own, in `crates/mp3/lame-config/` (upstream's is generated
  by autotools and `configMS.h` is MSVC-only); found through `HAVE_CONFIG_H` by
  `build.rs`. Defines the portable C environment (`STDC_HEADERS`, the header and
  function probes, the package strings) and typedefs `ieee754_float32_t`.

`deny.toml` does not change: cargo-deny reads crate licence fields and does not see
vendored C. `suhina-mp3` itself is GPL-3.0-or-later like every workspace crate.

## How to update

Download the new LAME tarball, check its SHA-256 against the one recorded above,
and copy the same file list over, keeping the `fft.c` replacement and the crate's
`config.h`. Then `cargo build -p suhina-mp3 && cargo test -p suhina-mp3`, and
compare the replacement's transforms and the encoder's output against the new
upstream build once more, as the pull request that updates it does.
