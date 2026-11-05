# Stock presets

Presets that ship with Suhina for its built-in effects and instruments, one folder per device
(`presets/<device id>/`), one file per preset, named `<preset name>.<device id>.preset.suhina`. A new
file also gets a line in `STOCK_FILES` in `crates/device-registry/src/lib.rs`. The rules and the file
format are in [ADR 0014](../docs/decisions/0014-stock-presets.md).

## Licence

Everything in this folder is dedicated to the public domain under
[CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/) (`SPDX-License-Identifier: CC0-1.0`).
Use the presets, and anything you make with them, without conditions. By adding a preset here
you agree to release it under CC0.

## Adding presets

Open a pull request with the [preset template](https://github.com/suhina-daw/suhina-daw/compare/main...main?template=preset.md)
(choose your branch on that page). Names and tags describe the sound in your own words and do
not name commercial products; banks or files from commercial products are not accepted.
