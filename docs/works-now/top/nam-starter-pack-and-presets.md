- The executable ships the NAM starter pack: the chosen amp models and cabinet impulse
  responses ride every build and unpack into the library's `Sounds/NAM/Amp models/Suhina` and
  `…/NAM/Cabinets/Suhina` folders at start, beside the user's own files and never over one; the
  old defaults `Documents/Suhina/NAM models` and `…/IRs` move into those folders at start. A
  file device's chosen state can be the pack's file by its `builtin:<device id>/<file name>`
  key, loading from the executable wherever the app runs. Six of the stock presets now name a
  shipped file, so adding them brings the model or cabinet with them.
- `device.savePreset` saves one device's values — and the file it selects — as the user's own
  preset into the library's `Presets/<device>/` folder; the device panel's menu has
  **Save as preset…**, the browser lists the user's presets beside the stock ones under their
  device, and its rows add them like any preset file.
