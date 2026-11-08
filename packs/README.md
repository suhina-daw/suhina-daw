# The shipped pack

The files the executable carries ([ADR 0036](../docs/decisions/0036-library-and-shipped-content.md))
live here, one folder per device. A project names one of these files by its library key, and the
panels list them as the **Suhina** pack — first on every device, before the user's own.

## Licence

Everything in this folder that Suhina made is dedicated to the public domain under
[CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/) (`SPDX-License-Identifier: CC0-1.0`).
Nothing here comes from a commercial product; Suhina has no affiliation with the makers of any
hardware a name refers to, and means no endorsement.

## `packs/nam-amp-modeler/`

| File | Made by | What it is | Licence |
|---|---|---|---|
| `Suhina Overdrive.nam` | Suhina | An LSTM trained with the NAM trainer ([neural-amp-modeler](https://github.com/sdatkinson/neural-amp-modeler), MIT, **v0.13.0**, commit `f2611290`) over 1000 epochs, from our 90-second training phrase rendered through Suhina's own Overdrive chain at its defaults, offline. Fits its source at −18.4 dB ESR, checked by the pack's render-oracle test. | CC0-1.0 |

## `packs/nam-cab-loader/`

| File | Made by | What it is | Licence |
|---|---|---|---|
| `Synthetic Cabinet Plain.wav`, `Bright.wav`, `Dark.wav`, `Off Axis.wav`, `Room.wav`, `Worse.wav` | Suhina | Cabinet responses of our own: a series filter bank — a high pass at 70 Hz, the driver's resonance near 95 Hz, the closed back's low weight, box honk scaled in, mic presence, axis colour, the top rolled off past 3.5 kHz, breakup smoothed — with a damped tail, mic-distance delay and (Rooms) early taps, rendered headless. | CC0-1.0 |

The irresistible names are starting points, not reuse: the pack's listening pass may rename or drop;
a model we train is always ours, and a collected file ships only when the shipping rule
([ADR 0036](../docs/decisions/0036-library-and-shipped-content.md) › 1) is satisfied.
