# Colour names

`colour-names.tsv` contains 1,066 names, each with one RGB value. It combines two sources:

- **xkcd colour survey.** Randall Munroe and the survey's contributors'
  [name/RGB table](https://xkcd.com/color/rgb.txt), downloaded 2026-11-19. The source file's
  first line dedicates it under [CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/);
  the full text is in `LICENSE-CC0.txt`. It has 949 data rows (the 2010 article describes the
  original table as 954). Original file SHA-256:
  `450cca88fa6fa9a1e79c969969e05e6900b41a94f0a3a5f134e3d0b79077f890`.
- **Wikipedia's lists of colors.** Names and sRGB values from Wikipedia contributors' lists of
  colors ([A–F](https://en.wikipedia.org/wiki/List_of_colors:_A%E2%80%93F),
  [G–M](https://en.wikipedia.org/wiki/List_of_colors:_G%E2%80%93M),
  [N–Z](https://en.wikipedia.org/wiki/List_of_colors:_N%E2%80%93Z)), taken as a plain
  name/hex list (not the pages' text, tables or images), licensed
  [CC BY-SA 4.0](https://creativecommons.org/licenses/by-sa/4.0/); the full text is in
  `LICENSE-CC-BY-SA-4.0.txt`. CC BY-SA 4.0 is one-way compatible with GPL-3.0, so the file
  ships under it as a whole. The names and values in it each come from a single contributor
  list entry; Wikipedia's page histories name the authors.

Suhina uses these as approximate descriptive names, not CSS keywords or replacement RGB
values. Names are title-cased and sorted, with *de*, *au*, *of* and *and* left lowercase.

What was curated:

- **Omitted (xkcd):** 72 source names (crude or derogatory names, four ambiguous labels, and
  spelling or spacing variants whose correctly spelled name exists), listed below, and a
  further 148 that are not expected names of real colours: "-ish" and "-y" forms (Greenish
  Blue, Purpley Pink), slash variants (Blue/Green), jokes and characters (Boring Green, Barney,
  Kermit Green), materials and things (Dirt, Tea, Velvet) and Spanish names (Azul, Rosa).
- **Omitted (Wikipedia):** names that are brand-like, invented or not recognised as a colour
  (for example Generic Viridian, Solid Pink, Unmellow Yellow, Vanilla Ice), and names that
  repeat another entry's RGB value (one name is kept for each value, the one in common use:
  Payne's Gray over Dark Electric Blue, Café au Lait over French Beige and Tuscan Tan).
- **Normalised:** grey → gray (US spelling) and hyphens → spaces (Blue-gray → Blue Gray);
  vermillion → vermilion, Robin Egg/Robin's Egg → Robin's Egg Blue, Terra cotta →
  Terracotta.
- **One value per name.** The 240 names that both sources define with different values keep
  whichever fits the name better: Wikipedia's value for 90 of them (for example Brown,
  Crimson, Teal, Gray, Magenta) and the xkcd value for the rest (for example Green, Blue,
  Red, Lime, Mint). Related names were then checked against each other in Oklab so that Dark,
  Light, Pale and Bright forms move lightness, saturation and hue the right way from their
  base; names that could not be made consistent (Neon Fuchsia, Deep Chestnut, Deep Taupe,
  Vivid Tangerine) were dropped.

Omitted xkcd source names (72): nasty green, really light blue, blue blue, booger, bland, egg shell, baby shit brown, liliac, ugly brown, kelley green, darkgreen, light light green, toupe, light lavendar, baby puke green, burnt siena, terracota, ugly blue, booger green, greenblue, icky green, golden rod, gross green, light light blue, shit green, darkblue, orangered, poop green, dark, piss yellow, bluegrey, royal, barf green, pale, greyblue, baby shit green, diarrhea, ugly purple, puke brown, bile, yellowgreen, baby poo, vomit yellow, lightgreen, baby poop green, fluro green, perrywinkle, light urple, ugly pink, ocher, lightblue, ugly yellow, poo, snot, baby poop, poo brown, poop brown, ocre, aqua marine, shit brown, shit, snot green, bluegreen, puke yellow, terra cotta, poop, vomit green, ugly green, vomit, forrest green, puke, puke green.

The app embeds this bounded table, builds its CIELAB lookup once (matched by CIEDE2000), and computes a name
only when the selected RGB changes or a swatch tooltip opens. There is no runtime
download, API call or project-size-dependent lookup.
