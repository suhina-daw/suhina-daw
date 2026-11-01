---
description: Why the MP3 export encodes with LAME 3.100 vendored as C source in third_party/lame and behind the safe suhina-mp3 crate, which files are vendored, how the licence was read, why fft.c is replaced with Suhina's own Hartley transform, and why crates/mp3 is allowed unsafe like the plug-in-host crates
read_when: MP3, mp3 export, LAME, libmp3lame, encoder crate, why C, vendored encoder, fft.c, Hartley, joint stereo, unsafe crate, lossy export
type: decision
status: accepted
built: yes
last_verified: 2026-11-02
covers:
  - third_party/lame
  - crates/mp3
  - crates/media/src/write.rs
---

# 0029: MP3 encoder — LAME 3.100, vendored, own fft.c

- **Date:** 2026-10-20
- **Issue:** [#760](https://github.com/suhina-daw/suhina-daw/issues/760)

## Context

The export renders WAV, FLAC and Ogg Vorbis (#757); the export spec's format table left MP3 out
pending a licence-clean encoder, with `rusty_mp3` (Apache-2.0, pure Rust) named as the one
candidate and left unevaluated. A DAW's mixdown is the use lossy codecs live or die on:
loud limited material and transients.

## Decision

**The MP3 export encodes with LAME 3.100's encoder, vendored as C source in
`third_party/lame/` and compiled by the new crate `crates/mp3` (`suhina-mp3`), whose
outward API is safe.** The decisions, made by the repository owner (2026-10-20):

1. **Encoder: LAME 3.100.** Not ffmpeg (a runtime dependency with many failure paths), not
   libsndfile (hides joint stereo and tag control). A comparison on DAW-like material (six
   synthesised stereo clips: drum loop, loud limited mix, dynamic mix, isolated hits, quiet
   reverb tail, ambience; 128, 192, 320 kbps CBR and VBR V2; same decoder for all; crude
   objective metrics, not PEAQ) measured LAME ahead of `rusty_mp3` 1.0.1 on transients:
   pre-echo before isolated hits 4–10 dB in LAME's favour across the rates, fewer
   noise-contaminated time–frequency cells, and a VBR that actually varies with the
   material (74–246 kbps against `rusty_mp3`'s constant ~190). On a casual listen the two
   sounded alike; the choice rests on LAME's twenty years as the reference encoder and the
   measured transient handling.
2. **Vendored source, not a `-sys` crate.** The official upstream tarball, not the copy
   inside `mp3lame-sys` (that copy carries a modified `libmp3lame/util.c`).
3. **`libmp3lame/fft.c` is not vendored** — it is the only built file without an LGPL
   header (an informal note from its author instead) — and Suhina's own implementation
   takes its place (below).
4. **`unsafe` is allowed only in `crates/mp3`**, with a safe API outwards: the same
   isolated-FFI-crate pattern as `crates/ipc` and `crates/plugin-host` ([ADR
   0006](0006-plug-in-hosting.md)).
5. **The paperwork ships in the same pull request**: `VENDORED.md`, the spec row, NOTICE
   and ATTRIBUTION.

## The licence, read file by file

- The kept encoder files and the headers they include carry the grant "version 2 of the
  License, or (at your option) any later version" (LGPL-2.0-or-later), so they are used
  under LGPL-2.1-or-later.
- Three small headers have no notice of their own (`id3tag.h`, `lame_global_flags.h`,
  `lameerror.h`); they fall under the package licence in `COPYING`, whose section 3 lets a
  copy be used under the ordinary GNU GPL, version 2 or any later version — compatible with
  Suhina's GPL-3.0-or-later. The reading is recorded in `third_party/lame/VENDORED.md`.
- MP3 patents expired in 2017; `deny.toml` is unchanged (cargo-deny sees crate licence
  fields, not vendored C).

## The fft.c replacement

`third_party/lame/libmp3lame/fft.c` is written from the interface alone — `fft.h`, the
`fft_fht` pointer in `util.h`, the call sites in `psymodel.c` — and a textbook description
of the fast Hartley transform (Bracewell): bit-reverse permute, then merge half
transforms stage by stage. The contract it implements, as `psymodel.c` consumes it:

- `init_fft` fills `cd_psy->window[BLKSIZE]` with a Blackman window (0.42 − 0.5cos +
  0.08cos2, half a sample off the grid) and `window_s[BLKSIZE_s/2]` with the first half of
  a Hann window, applied mirrored.
- `fft_long` and `fft_short` store the unnormalised natural-order Hartley transform of the
  windowed signal; `psymodel.c` reads bins `e` and `N−e` as the real and imaginary parts
  of DFT bin e, whose energies are what the model spreads. Short blocks are read at
  offsets 192, 384 and 576.

Verified during development only, against a build of the original (the file is never
committed): the windows are bit-identical; the transforms agree with the original's within
4.9e-6 relative on 1024-point long blocks and 3.5e-7 on short ones (criterion: 1e-5). The
original computes the transform in float internally (its own result sits 2.6e-6 from the
true double value; Suhina's sits 6e-8 from it), so an independent implementation cannot
match it closer than that without its source. Encoding six two-second clips with the
original library and with Suhina's gives decodes that agree to −94.7 dB relative and
better for the quiet materials, worst −78.8 dB on the loud drum loop — the level of the
psy model's occasional threshold flips, all below what a listen can hear.

## Consequences

- `crates/mp3` carries the crate-level `unsafe` exception (AGENTS.md rule 2's list grows
  by one): one FFI module, hand-written declarations for the `lame_*` and `id3tag_*`
  functions used, every `unsafe` block small with a `// SAFETY:` comment, no
  `unwrap`/`expect`/`panic`.
- The export gains MP3 at 128, 192, 256 and 320 kbps CBR and VBR V0 and V2; sample rates
  32, 44.1 and 48 kHz; ID3v2 with the project's name; the LAME tag frame written at the
  file's start after encoding.
- The encoder sits behind `suhina-mp3`'s API. Replacing it later touches only that crate
  and removes the C code.

## Revisit when

A pure-Rust encoder (`rusty_mp3` or another) matches LAME on transients in a test like the
one above and has a longer track record.
