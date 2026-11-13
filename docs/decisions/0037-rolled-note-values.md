---
description: Why a note's velocity range is rolled each time the note starts from a pure hash of what the note is and where it plays, in the one render path, so an offline render repeats and matches what was heard; the roll a note's chance (#1152) will share
read_when: velocity range, rolled velocity, note roll, seeded roll, note chance, probability, random each pass, note_roll, project file version 13, MIDI export velocity range
type: decision
status: accepted
built: yes
last_verified: 2026-11-13
covers:
  - crates/model/src/roll.rs
  - crates/engine/src/lib.rs
  - crates/engine/src/launcher.rs
  - crates/project-file/src/lib.rs
---

# 0037: Rolled note values

- **Date:** 2026-11-13
- **Issues:** [#1282](https://github.com/suhina-daw/suhina-daw/issues/1282),
  [#1152](https://github.com/suhina-daw/suhina-daw/issues/1152)

## Context

A looped clip plays every note the same way on every pass. #1282 gives a note a velocity range,
rolled each time the note starts; #1152 will give it a chance of playing at all, rolled the same
way. Both need a roll that an offline render repeats exactly and that matches what playback played
(AGENTS.md › One render path), with nothing on the audio thread that allocates, locks or keeps
state another thread must reset.

## Decision

1. **The note holds the range, the roll is never stored.** `Note::velocity_range` is a whole number
   from -127 to 127 (0 by default: the note plays its own velocity). The model and the file keep
   the note's own velocity; the rolled one exists only where the note starts.
2. **The roll is a pure function** (`suhina_model::note_roll`): splitmix64 over what the note is and
   where it plays (its track, where its clip plays it, its start, its key and the number of the
   pass through the clip's loop), giving a number in [0, 1). The same note in the same place on the
   same pass rolls the same number on every machine and every run; nothing about the note's id
   (which lives only while a project is open) goes in, so a reopened project rolls as before.
3. **Where it is rolled.** In the one render path, where a note becomes a start: the arrangement
   rolls as it lays out each pass's notes, a launched clip as its note starts, counting its passes
   from the launch. Offline rendering runs the same code, so a render repeats and matches playback.
   The roll is integer arithmetic on the audio thread, with no allocation, lock or state.
4. **The played velocity** is the note's velocity plus a whole number of MIDI steps between 0 and
   the range (towards it, so a negative range plays softer), kept within 1 to 127.
5. **MIDI export writes the note's own velocity.** The range is how the clip plays, not what it
   holds; a file for another program gets the note as it was written.
6. **The project file** writes `velocity_range` on a note that has one, from format version 13; a
   note without it, and every note of an older file, has a range of 0.
7. **The chance (#1152) will roll from the same function**, with a salt of its own so the chance and
   the velocity of one start are not the same number.

## Consequences

- Two identical notes (same key and start) on one track and in one clip roll the same velocity:
  stacked copies of a note sound together, as they would without a range.
- Moving a note or its clip changes its rolls; that is a different note in a different place.
- A launched clip starts its passes from 0 at every launch, so each launch varies the same way.
