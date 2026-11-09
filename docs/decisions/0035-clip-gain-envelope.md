---
description: Why every audio clip has its own gain, pitch and warp, and how its gain envelope is stored and played: dB offsets on the content's beats, straight in dB, multiplied in the same engine path offline and live
read_when: clip gain envelope, gain envelope, per-clip audio, independent audio clips, ClipAudio, clip.setEnvelope, envelope interpolation, envelope on loop, envelope on split, clip tempo source, project file version 10
type: decision
status: accepted
built: yes
last_verified: 2026-11-06
covers:
  - crates/model/src/clip_audio.rs
  - crates/engine/src/audio.rs
  - crates/session/src/clip_audio.rs
  - crates/project-file/src/lib.rs
---

# 0035: Per-clip audio and the clip gain envelope

- **Date:** 2026-11-05
- **Issues:** [#742](https://github.com/suhina-daw/suhina-daw/issues/742),
  [#824](https://github.com/suhina-daw/suhina-daw/issues/824),
  [#1190](https://github.com/suhina-daw/suhina-daw/issues/1190)

## Context

An audio clip's gain, transpose, warp switch, warp mode and warp markers were the content's
(`AudioRegion`). Clips that play one content (a duplicate, a comped part and its take) shared one
gain fader: `clip.setGain` changed all of them, and `clip.setAudio` copied the content first where
the markers would not fit the other clip (#874). A gain envelope drawn on one clip would have
reached every copy too. #742 asks for audio clips that are always independent, with a drawable
gain envelope, and #1190 for a clip panel whose controls change one clip.

## Decision

1. **Every editable audio property is the clip's.** `Clip::audio` (`ClipAudio`) holds the gain,
   gain envelope, transpose, warp switch, warp mode, warp markers and where the clip's tempo came
   from. The content's `AudioRegion` keeps only the source: the file, its stream, sample rate,
   channels and length. All of them move, not the gain alone, so a gain that is the clip's never
   sits beside a transpose or a warp that is shared. The decoded audio stays shared in the pool by
   file (ADR 0011); only the small state is per clip.
2. **No shared audio clip remains as a feature.** Duplicate, paste and split copy the clip and so
   its state; there is no make-unique step and no linked audio copy. The content can still be
   shared (a duplicate and its original, a take and its comped part), which keeps comping's
   take-by-content rule (#989) and costs nothing.
3. **The envelope is dB offsets on the content's beats.** A point is `{beat, db, curve}`: a beat of the
   content, as a warp marker's, and an offset from the clip's gain, −70 dB to +24 dB. An empty
   envelope is flat at the gain, so the fader moves the whole envelope. On the content's beats a
   loop plays the same stretch of envelope on every pass, a split clip's parts each keep theirs,
   a moved start marker reads it from there, and a stretch scales it with the markers.
4. **Straight lines in dB, bent as automation bends.** Between two points the level moves
   linearly in dB, so a fade drawn with two points sounds even; linear in amplitude was rejected,
   as its ramps sound sudden at the quiet end. A point's `curve` bends the segment to the next
   point the way an automation segment bends (`automation::Curve`, `docs/spec/automation.md` ›
   Lanes and points): the share of the way in dB follows the curve, so one shape and one drag
   work the same on a lane and on a clip. Before the first point and after the last the level holds; two points on one
   beat make a step.
5. **One engine path.** The envelope is part of the clip's `Fades`: worked out off the audio
   thread when the graph is built (its points sorted and clamped, how a frame since the clip's
   start maps to a content beat: offset, loop, the repitch rate of an unwarped clip, the tempo map
   of a warped one), and read per frame with a binary search, never allocating. Every reader (the
   direct one, the stretcher's, a launched clip, a clip host's gains) multiplies it in where it
   multiplies the fades, so offline renders and playback match sample for sample (AGENTS.md
   rules 3 and 4).
6. **The project file moves the state with a version.** Version 10 writes `audio` on the clip and
   no state on the region. A file before version 10 is read with each clip taking the state its
   content's region held, so every clip of a shared content keeps the sound it had.

## Consequences

- `clip.setGain`, `clip.setAudio`, `clip.setTempo` and `clip.setEnvelope` change one clip. The
  copy-on-warp of #874 is gone: two clips of one content warp each at the tempo where they play.
- A content's length in beats is no longer the same for every clip that plays it once their warps
  differ; `clip.stretch` and `clip.setTempo` still give a clip whose content another plays a copy,
  as they change the content's length.
- Importers set the state on the clip (the Live Set reader from each `AudioClip`).
- Out of scope: rendering the envelope into the file (a bounce), and an envelope per ARA region,
  which the clip host keeps its own way (ADR 0033).
