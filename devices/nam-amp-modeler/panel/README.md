# Amp Modeler panel artwork

The images the Amp Modeler's compact panel draws (`devices/nam-amp-modeler/panel.json`). The
device registry embeds them into the panel's canvas as image assets when it loads the layout
(`crates/device-registry`), so the files here are the only copy.

| File | Asset id | What it is | Size |
|---|---|---|---|
| `background.jpg` | `background` | The amp head behind the controls: the frame, the grille with the dark slots the dropdowns, steppers and buttons sit in, the logo and the brushed plate under the knobs | 1284 × 504, three times the 428 × 168 canvas |
| `knob.png` | `knob` | A knob's face: the metal ring and the spun-metal cap inside it, transparent around it. The panel draws the value's arc around it and the pointer over it, and a theme's `device.knob.body` artwork replaces it | 128 × 128 |

## Provenance

- **Author:** juhamikael (the Suhina maintainer), made with AI image-generation tools. Neither
  the images nor their textures come from another product's artwork or from a stock library.
- **Licence:** CC0-1.0, as Suhina's own content is
  ([CONTRIBUTING.md › Shipped content](../../../CONTRIBUTING.md#shipped-content)).
- **What changed:** scaled down from the originals to fit the SDK's image limits
  ([sdk-custom-ui.md](../../../docs/spec/sdk-custom-ui.md)), at most 512 KiB and one million
  pixels an image: the background from 1712 × 672; the knob composed from a 1024 × 1024 ring and
  a 1024 × 1024 cap, centred on each other and cropped to the ring.
