# Instrument panel artwork

The background the Instrument's canvases draw with a light theme (`devices/instrument/src/panel.rs`
embeds them as image assets; see [instrument-editor.md](../../../docs/spec/instrument-editor.md)
› Appearance and validation). The files here are the only copy.

| File | Asset id | What it is | Size |
|---|---|---|---|
| `editor-light.jpg` | `artwork-light` | The expanded editor's background: a warm ivory surface with soft relief waves at the left and right edges and the Suhina logo at the bottom right | 1250 × 795, for the 880 × 560 canvas |
| `panel-light.jpg` | `artwork-light` | The compact panel's background: the bottom strip of the same surface, its right-hand wave and the logo | 1240 × 336, twice the 620 × 168 canvas |

A dark theme hides both (the panel's style sheet), so the theme's light text never sits on them.

## Provenance

- **Author:** juhamikael (the Suhina maintainer), made with AI image-generation tools. Neither
  the image nor its textures come from another product's artwork or from a stock library.
- **Licence:** CC0-1.0, as Suhina's own content is
  ([CONTRIBUTING.md › Shipped content](../../../CONTRIBUTING.md#shipped-content)).
- **What changed:** from one 1253 × 1253 WebP original, cropped and scaled with ffmpeg (Lanczos)
  to JPEG to fit the SDK's image limits ([sdk-custom-ui.md](../../../docs/spec/sdk-custom-ui.md)),
  at most 512 KiB and one million pixels an image: the editor's from its lower 797 rows, the
  panel's from its lower 340 rows, so both keep the logo at the bottom right.
