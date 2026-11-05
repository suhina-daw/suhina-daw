## Presets

<!-- Which device, and what each preset is for, in a line each. Link the issue if there is one. -->

## How I tested it

<!-- Load each file (device.add with the preset, or from a folder in the browser) and listen to it. -->

## Checklist

- [ ] Every preset is a `<name>.<device id>.preset.suhina` file in `presets/<device id>/` in the format of
      [ADR 0014](https://github.com/suhina-daw/suhina-daw/blob/main/docs/decisions/0014-stock-presets.md),
      with every parameter given and inside its range, and has a line in `STOCK_FILES` in
      `crates/device-registry/src/lib.rs`.
- [ ] I release these presets under CC0 1.0 ([`presets/README.md`](https://github.com/suhina-daw/suhina-daw/blob/main/presets/README.md)),
      and every commit ends with a `Signed-off-by:` line (`git commit -s`).
- [ ] Names, tags and descriptions are my own words and don't name commercial products or their
      presets; no banks or files from commercial products are included.
- [ ] I listened to every preset; `cargo test --workspace` passes.
