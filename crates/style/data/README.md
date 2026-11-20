# Colour names

`colour-names.tsv` contains 877 names derived from Randall Munroe and the xkcd
colour-survey contributors' [name/RGB table](https://xkcd.com/color/rgb.txt), downloaded
2026-11-19. The source file's first line dedicates it under
[CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/); the full text is in
`LICENSE-CC0.txt`. It has 949 data rows (the 2010 article describes the original table
as 954). Original file SHA-256: `450cca88fa6fa9a1e79c969969e05e6900b41a94f0a3a5f134e3d0b79077f890`.

Suhina uses these as approximate descriptive names, not CSS keywords or replacement
RGB values. The retained RGB values are unchanged. Names are title-cased and sorted;
crude/derogatory names and four ambiguous labels are omitted. Spelling/spacing variants
are omitted when their correctly spelled name already exists, keeping that original
name's RGB value instead of creating duplicate names with different values. Names use US
spelling, so the omitted list below keeps the source file's spellings.

Omitted source names (72): nasty green, really light blue, blue blue, booger, bland, egg shell, baby shit brown, liliac, ugly brown, kelley green, darkgreen, light light green, toupe, light lavendar, baby puke green, burnt siena, terracota, ugly blue, booger green, greenblue, icky green, golden rod, gross green, light light blue, shit green, darkblue, orangered, poop green, dark, piss yellow, bluegrey, royal, barf green, pale, greyblue, baby shit green, diarrhea, ugly purple, puke brown, bile, yellowgreen, baby poo, vomit yellow, lightgreen, baby poop green, fluro green, perrywinkle, light urple, ugly pink, ocher, lightblue, ugly yellow, poo, snot, baby poop, poo brown, poop brown, ocre, aqua marine, shit brown, shit, snot green, bluegreen, puke yellow, terra cotta, poop, vomit green, ugly green, vomit, forrest green, puke, puke green.

Renamed source entries: vermillion → vermilion.

The app embeds this bounded table, builds its Oklab lookup once, and computes a name
only when the selected RGB changes or a swatch tooltip opens. There is no runtime
download, API call or project-size-dependent lookup.
