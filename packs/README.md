# The shipped pack

The files the executable carries ([ADR 0036](../docs/decisions/0036-library-and-shipped-content.md))
live here, one folder per device, named after which device runs them. A project names one of
these files by its library key, and the panels list them as the **Suhina** pack — first on every
device, before the user's own packs. At app start the pack unpacks into the library's
`Suhina` folders when a file is not there; a file the user carries is never overwritten.

| Folder | What | Files |
|---|---|---|
| `packs/nam-amp-modeler/` | `.nam` models the NAM Amp Modeler runs | 17 |
| `packs/nam-cab-loader/` | `.wav` cabinet responses the NAM Cab Loader runs | 26 |

## Licences

- **Our own files say CC0-1.0** (`SPDX-License-Identifier: CC0-1.0`): the pack's trained models
  and synthetic responses are Suhina's, dedicated to the public domain.
- **The collected files carry their publishers' licences**, recorded per row below.
  `packs/nam-cab-loader/LICENSE.BestPlugins-Amps.txt` and `…-Bands.txt` are the two licence
  texts the Guitarix IR packs were published under. Nothing here comes from a commercial
  product's library, and Suhina has no affiliation with the makers of any hardware a name
  refers to; it means no endorsement.

## `packs/nam-amp-modeler/`

| File | Author/capturer (as named) | Source | Licence |
|---|---|---|---|
| `Suhina Overdrive.nam` | Suhina | trained with the NAM trainer ([neural-amp-modeler](https://github.com/sdatkinson/neural-amp-modeler), MIT, v0.13.0, `f2611290`) over 1000 epochs from our 90-second training phrase rendered through Suhina's own Overdrive chain at its defaults, offline; fits its source at −13.1 dB ESR, guarded by the pack's render-oracle test | CC0-1.0 |
| the 16 remaining `.nam` files (each name carries its capturer: "Helga B", "Jason Z", "Mikhail K", "Peter N", "Phillipe P", "Roman A", "Sascha S", "Tim R", "Tudor N" — the captures' authors, with the amp or pedal recorded from) | the Facebook NAM group's captures, gathered by pelennor2170 | [pelennor2170/NAM_models](https://huggingface.co/pelennor2170/NAM_models) — the collection the owner's decision of 2026-11-07 on [#1279](https://github.com/suhina-daw/suhina-daw/issues/1279) allows shipping per file | GPL-3.0 (the collection's COPYING), by the publisher's licence |

## `packs/nam-cab-loader/`

| File(s) | Author | Source | Licence |
|---|---|---|---|
| `Synthetic Cabinet Bright/Dark/Off Axis/Plain/Room/Worse.wav` | Suhina | our own synthetic cabinet responses: a series filter bank — a high pass at 70 Hz, the driver's resonance near 95 Hz, the closed back's low weight, box honk scaled in, mic presence, axis colour, the top rolled off past 3.5 kHz, breakup smoothed — with a damped tail, mic distance and room taps, rendered headless | CC0-1.0 |
| `Blackat Leon S7.wav`, `Cicognani Imperivm Luxury.wav`, `DV Mark Triple 6.wav`, `Engl Retro Tube.wav`, `Engl Special Edition.wav`, `EVH 5150 III.wav`, `Fortin Natas.wav`, `Kaos Sludge 15.wav`, `Krank Krankenstein.wav`, `Laney ironheart.wav`, `MakosampCustomHatred.wav`, `Marshall JMP 2203  Jose Arredondo mod.wav`, `Marshall MG 15.wav`, `Mesa Boogie Mark V.wav`, `Peavey Vypyr 15.wav`, `Randall Satan.wav`, `Randall thrasher.wav`, `Splawn Nitro.wav`, `Splawn Quick Rod.wav`, `Taurus Stomphead.wav` | © 2013 David Fau Casquel ("BESTPLUGINS MEGA PACK 2" and "1+3") | guitarix's `trunk/IR/BestPlugins_Amps` and `BestPlugins_Bands`, the [brummer10/guitarix mirror](https://github.com/brummer10/guitarix); a tone-match IR is a frequency response and copies no recording | GPL-2.0-or-later (each pack's own `LICENSE`, carried in this folder) |

The names are the files' names, which say where a capture was recorded from — the AGENTS.md
rule that permits hardware names on content licensed for redistribution. The names stand as the
panels show them.
