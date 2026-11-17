# Amp Modeler panel artwork

The images the Amp Modeler's compact panel draws (`devices/nam-amp-modeler/panel.json`). The
device registry embeds them into the panel's canvas as image assets when it loads the layout
(`crates/device-registry`), so the files here are the only copy.

| File | Asset id | What it is | Size |
|---|---|---|---|
| `background-dark.jpg` | `background-dark` | The amp head behind the controls, in black tolex with a dark grille: the frame, the grille the dropdowns, steppers and buttons sit on, the logo and the brushed plate under the knobs. Shown with a dark theme | 1284 × 504, three times the 428 × 168 canvas |
| `background-light.jpg` | `background-light` | The same head in cream tolex with a light grille and a bright plate. Shown with a light theme (`#light`) | 1284 × 504 |
| `knob.png` | `knob` | A knob's face, the same on either head: a flat dark body with a grey rim, transparent around it | 128 × 128 |

The panel draws the value's arc around a knob and its pointer over it, in the theme's text
colour where that reads on the dark face and in a light ink where it does not, and a theme's
`device.knob.body` artwork replaces the face. The originals' dark slots marked where each
control sits; they are filled with the grille here, and the panel draws the controls' boxes in
the theme's colours.

## Provenance

- **Author:** juhamikael (the Suhina maintainer), made with AI image-generation tools. Neither
  the images nor their textures come from another product's artwork or from a stock library.
- **Licence:** CC0-1.0, as Suhina's own content is
  ([CONTRIBUTING.md › Shipped content](../../../CONTRIBUTING.md#shipped-content)).
- **What changed:** scaled down from the originals to fit the SDK's image limits
  ([sdk-custom-ui.md](../../../docs/spec/sdk-custom-ui.md)), at most 512 KiB and one million
  pixels an image: the backgrounds from 1712 × 672, their control slots filled with grille copied
  from beside them; the knobs cropped to their circles from 1024 × 1024.
