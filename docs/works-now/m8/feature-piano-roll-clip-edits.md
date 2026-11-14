- **Joining, fitting, cropping and splitting in the piano roll** ([#1284](https://github.com/suhina-daw/suhina-daw/issues/1284)):
  Join notes (Ctrl+J) merges the selected notes on a key into one, over any gap, Fit to time selection
  (Ctrl+Alt+J) stretches the selected notes to fill the time selection, Crop clip to time selection
  (Ctrl+Shift+J) keeps only the selection, cutting notes at its edges, Split clip (Ctrl+E) cuts an
  arrangement clip at the time selection's edges, and Set the clip's first beat here makes the
  clicked point beat 1.1.1 without changing the sound. Each is a command (`note.join`,
  `note.fitToTime`, `clip.crop`, `clip.split` with `ats`, `clip.setOrigin`) and an item of the
  right-click menu.
