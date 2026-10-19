---
description: Stems files (.stem.mp4, .stem.m4a): an MP4 with the master and its stems as audio tracks, stem names, colours and mastering settings in moov/udta/stem; what Suhina reads and how it maps the mastering to its compressor and limiter
read_when: stems, stem file, .stem.mp4, .stem.m4a, Traktor stems, NI stems, master track, mastering_dsp, MP4 boxes, edit list, AAC priming, multitrack audio file, DJ, remix, folded group
type: spec
status: draft
last_verified: 2026-10-15
covers:
  - crates/media/src/stems.rs
  - crates/session/src/files.rs
  - crates/session/tests/stems.rs
  - crates/media/tests/data/**
  - scripts/stems-fixture.py
  - crates/media/src/stems_tests.rs
---

# Stems files

A Stems file holds a finished track and its parts in one file, so a DJ or a remixer can play
the parts separately: typically drums, bass, other and vocals. Suhina reads them into a group
of audio tracks, one per stem, whose group plays the stems through the file's mastering
settings.

- **Sources:** the format's public description by its authors (an ISMIR 2015 late-breaking
  paper on the Stem format), the README of the open-source `m4a-stems` library, the default
  metadata of the open-source stemgen tool (MIT), the IETF drafts on stems in Ogg and Matroska
  (for comparison), and files we make ourselves (`scripts/stems-fixture.py`). No commercial
  stem file is committed or was taken apart.

## Container

- An ISO MP4 file. Names end in `.stem.mp4` (the original spelling) or `.stem.m4a`; Suhina
  recognises both, in any case. A file named only `.mp4` is not taken as a Stems file.
- **Audio tracks, in the order of their `trak` boxes:** the first is the **master**, the finished
  mix that plays in ordinary players; each further one is a **stem**. Files have four stems;
  Suhina accepts from one to fifteen (at most 16 audio tracks) and refuses a file with only
  one audio track. Other tracks (cover art, video) are ignored.
- **Codecs:** AAC (the usual case) or ALAC (lossless). All tracks share one sample rate.
- **Edit lists.** An AAC encoder starts with priming frames (1024 at 44.1 kHz) and pads the end;
  the track's `edts/elst` box says where the sound starts (`media_time`, in the track's time
  scale from `mdhd`) and how long it plays (`segment_duration`, in the movie's time scale from
  `mvhd`). Suhina trims every track by its first edit that plays media (an empty edit, a
  delay, is skipped), so the stems line up exactly with each other and with the beat grid.
  Symphonia, which decodes the audio, does not read edit lists; `crates/media/src/stems.rs`
  reads these boxes itself.

## Metadata

A box `moov/udta/stem` holds a JSON document, as plain UTF-8 text or as base64 of it (some tools
write it so). It may end in NUL bytes. A missing or unreadable document is not an error: the
stems are then called "Stem 1", "Stem 2" and so on, without colours or mastering.

```json
{
  "version": 1,
  "stems": [ { "name": "Drums", "color": "#FD4A4A" }, … ],
  "mastering_dsp": {
    "compressor": { "enabled": false, "ratio": 3, "output_gain": 0.5, "release": 0.3,
                    "attack": 0.003, "input_gain": 0.5, "threshold": 0, "hp_cutoff": 300,
                    "dry_wet": 50 },
    "limiter": { "enabled": false, "release": 0.05, "threshold": 0, "ceiling": -0.35 }
  }
}
```

- `stems[n]` describes audio track `n + 1` (the master has no entry). Names lose control
  characters and are cut to 64 characters; an empty name falls back to "Stem n". A colour that is
  not `#RRGGBB` is ignored.
- Units, as the stemgen defaults show them: `threshold` and `ceiling` in dB, `attack` and
  `release` in seconds, `hp_cutoff` in Hz (a high-pass on the compressor's level detector),
  `dry_wet` in percent, `ratio` as *n*:1. `input_gain` and `output_gain` run from 0 to 1 with
  0.5 at unity; their scale in dB is not published.
- Numbers that are missing or not numbers are left out (the device keeps its default).

## Mastering

A player is meant to run the sum of the stems through the compressor and then the limiter, so
the stems together sound like the master. Suhina puts both on the stems' group track
(`mastering_devices`): the group sums its tracks and runs its devices over the sum
([ADR 0004](../decisions/0004-built-in-devices.md) › 10). A setting with `enabled` false is added
as a disabled device, so it can be switched on.

| File | Device parameter | Mapping |
|---|---|---|
| compressor `input_gain`, `output_gain` | compressor `input`, `output` (dB) | 0..1 → −12..+12 dB, 0.5 = 0 dB (assumed; to be checked against a file whose settings are known); a value outside 0..1 is taken as dB, kept to −24..+24 |
| compressor `threshold` | `threshold` | dB, kept to −60..0 |
| compressor `ratio` | `ratio` | kept to 1..20 |
| compressor `attack`, `release` | `attack`, `release` | seconds, kept to 0.1–100 ms and 10 ms–2 s |
| compressor `hp_cutoff` | `sidechainHighPass` | Hz, kept to 20..1000 |
| compressor `dry_wet` | `mix` | percent / 100 (a value ≤ 1 is taken as a fraction), 0..1 |
| limiter `threshold`, `ceiling` | limiter `threshold`, `ceiling` | dB, kept to −30..0 and −12..0 |
| limiter `release` | `release` | seconds, kept to 1 ms–1 s |

In the Arrangement view the group arrives **folded**: one lane named after the file, showing the
clips of the master track, with the stems' lanes folded away until the group's triangle is clicked
([arrangement editing](arrangement-editing.md) › Folded groups). The master track is muted so the
file does not sound twice; it is there to hear the whole mix and to compare with.

## Limits

| What | Limit | Beyond it |
|---|---|---|
| The `moov` box | 64 MiB | refused |
| The metadata document | 1 MiB | ignored |
| Audio tracks | 16 | refused |
| Boxes looked at | 100 000 | the rest are ignored |
| Box nesting followed | 8 | |
| Edit list entries looked at | 1 024 | |
| A track's length | 2 hours, as any sample | refused |

A box whose size is 0 runs to the end of its parent. At the top level of the file, a box that is
too small or runs past the end refuses the file as damaged; inside `moov` it ends the search at
that level. A damaged packet is skipped as players do.

## Test files

`crates/media/tests/data` holds three files made by `scripts/stems-fixture.py` (ffmpeg, then
the `stem` box added): five stereo sine tracks at 110, 220, 440, 880 and 1760 Hz (the master
first), in AAC with plain JSON (1 s), in ALAC with base64 JSON (0.25 s), and in AAC without
metadata.

## Not decided yet

- The dB scale of the compressor's input and output gain (above).
- Writing Stems files (exporting a group's tracks as one).
- Stems in Ogg and Matroska, as the IETF drafts describe them: the same model, read by another
  container reader.

All decoded streams together have a 1 GiB PCM budget. Each stream accepts
1000..384000 Hz and 1..32 channels; header and packet sizes are checked before
growing output buffers, with fallible allocation.
