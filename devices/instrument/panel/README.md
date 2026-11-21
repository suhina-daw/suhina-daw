# Instrument panel artwork

The images the Instrument's canvases draw (`devices/instrument/src/panel.rs` embeds them as image
assets; see [instrument-editor.md](../../../docs/spec/instrument-editor.md) › Appearance and
validation). The files here are the only copy.

| File | Asset id | What it is | Size |
|---|---|---|---|
| `editor-surface.jpg` | `artwork-light` | The expanded editor's background: a warm ivory surface whose relief waves at the left and right edges are blurred to soft folds, with no logo | 1250 × 795, for the 880 × 560 canvas |
| `panel-light.jpg` | `artwork-light` | The compact panel's background: the bottom strip of the same surface, its right-hand wave and the Suhina logo | 1240 × 336, twice the 620 × 168 canvas |
| `logo.png` | `logo` | The Suhina wordmark in dark espresso on transparency, at the editor's top center | 324 × 90, three times its 108 × 30 place |
| `knob-large.png` | `knob-large` | The face of the editor's large dial: an ivory dome with a fine brushed sheen and an engraved inner ring on an espresso rim, with its shadow, on transparency | 468 × 468, three times its 156 × 156 place |
| `knob-small.png` | `knob-small` | The face of the editor's other dials: the same dome and rim, smaller, without the inner ring | 192 × 192, three times 64 × 64 |

A dark theme hides the backgrounds and the logo (the panels' style sheets), so the theme's light
text never sits on them.

## Provenance

- **Backgrounds.** Author: juhamikael (the Suhina maintainer), made with AI image-generation
  tools; neither the image nor its textures come from another product's artwork or from a stock
  library. `panel-light.jpg` was cropped and scaled with ffmpeg (Lanczos) from one 1253 × 1253
  WebP original, from its lower 340 rows so it keeps the logo at the bottom right.
  `editor-surface.jpg` comes from the same surface without the logo (a 1254 × 1254 PNG), its
  middle 798 rows cropped and scaled to 1250 × 795, then blurred (Gaussian, sigma 12) and its
  contrast lowered to 0.86 with ffmpeg, so the relief stays behind the controls.
- **Logo.** The Suhina wordmark, the maintainer's own (an SVG), colored `#34261d` and drawn to
  PNG with headless Chrome.
- **Dial faces.** Suhina's own: drawn from CSS (gradients, shadows and a conic sheen, no image
  inputs) and captured with headless Chrome at three times their size.
- **Licence:** CC0-1.0, as Suhina's own content is
  ([CONTRIBUTING.md › Shipped content](../../../CONTRIBUTING.md#shipped-content)).

All the images together stay within the SDK's limits for a canvas
([sdk-custom-ui.md](../../../docs/spec/sdk-custom-ui.md)): at most 512 KiB and one million pixels
an image, 1 MiB and four million pixels in all.
