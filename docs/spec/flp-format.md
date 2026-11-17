---
description: The FL Studio project (.flp) container and event stream as read by crates/flp, a draft spec where most statements are hypotheses
read_when: reading .flp, FL Studio project, FLhd, FLdt, event stream, event id, varint, LEB128, UTF-16 strings, version event 199, flp reader errors
type: spec
status: draft
last_verified: 2026-10-15
covers:
  - crates/flp/src/lib.rs
  - crates/flp/src/event.rs
  - crates/flp/tests/fixtures.rs
  - crates/flp/tests/corpus.rs
---

# FL Studio project (`.flp`) format

What Suhina reads of an FL Studio project file: the two chunks that hold it and the stream of
events inside the second one. Mapping those events onto the project model is a later piece of
[epic #271](https://github.com/suhina-daw/suhina-daw/issues/271) and is not described here.

Written in our own words. **Sources:**

- The event list Image-Line published for its plug-in SDK users (the `FLP_*` constants: id ranges,
  the text events from 192 on) and the public documentation of PyFLP (GPL-3.0), the open-source
  reader that [ADR 0001](../decisions/0001-daw-neutral-model.md) names as an allowed source. Only
  their published descriptions were used, no code was copied or translated.
- Files authored by the maintainer in FL Studio 26.1.0.5530 (Windows), kept in the gitignored
  `corpus/flp` ([`flp-corpus.md`](flp-corpus.md)). What they showed is in
  [#272](https://github.com/suhina-daw/suhina-daw/issues/272#issuecomment-6158957721).

**This spec is a draft.** Every statement marked **(hypothesis)** still needs a corpus file that
proves it; statements marked **(corpus)** were seen in the files above. Where the spec says
nothing, the reader returns an error instead of guessing.

## Container

All integers are little-endian. A file is two chunks, one after the other, and nothing else:

| Offset | Size | Field |
|---|---|---|
| 0 | 4 | `FLhd`, the header chunk's magic |
| 4 | 4 | Header length, `u32`: 6 **(corpus)** |
| 8 | 2 | File kind, `u16`: 0 is a project **(hypothesis)**; other values are kept as numbers |
| 10 | 2 | Channel count, `u16` (the channels in the channel rack) **(hypothesis)** |
| 12 | 2 | PPQ, `u16`: ticks per quarter note, which every position in the events counts in **(hypothesis)** |
| 14 | 4 | `FLdt`, the data chunk's magic |
| 18 | 4 | Data length, `u32`: how many bytes of events follow |
| 22 | data length | The events |

So a well-formed file is exactly `22 + data length` bytes **(corpus)**. The reader refuses:

- a first or second magic that is not `FLhd` or `FLdt` (*bad magic*, at offset 0 or 14);
- a file shorter than 22 bytes (*truncated header*, at the offset where it ends);
- a header length other than 6 (*unsupported header*, at offset 4): nothing says where the
  data chunk would start then;
- a file whose size is not `22 + data length`, shorter or longer (*size mismatch*, with the size
  expected and the size found).

Files are capped at 512 MiB before anything is read into memory.

## Events

The data chunk is a sequence of events, each an id byte followed by its value. The id says how
long the value is **(hypothesis, apart from the exception below)**:

| Ids | Value |
|---|---|
| 0 to 63 | 1 byte |
| 64 to 127 | 2 bytes |
| 128 to 191 | 4 bytes |
| 192 to 255 | a length, then that many bytes |

The length of the last range is an unsigned LEB128 number: seven bits per byte, least significant
group first, the top bit set on every byte but the last **(hypothesis; the corpus has shown
one-byte lengths)**. A length that needs more
than five bytes, or does not fit in 32 bits, is refused (*bad length*); so is one whose bytes run
to the end of the chunk without a last byte (*unterminated length*).

**Exception: id 172 has a one-byte value (corpus).** FL Studio 26 writes id 172 (`0xAC`) with one
byte, not the four its range implies. Read with four, the walk still ends at the end of the chunk
but is out of step from the event after it, and reads the text that follows as one-byte events.
Every corpus file opens with these eleven bytes, which only make sense with the exception:

```text
a9 0f 00 00 00   id 169, 4-byte value 15
1c 01            id 28, 1-byte value 1
ac 01            id 172, 1-byte value 1
01 00            id 1, 1-byte value 0
```

Whether older versions write id 172 the same way is **(hypothesis)**; the reader applies the
exception in every file.

An event whose value runs past the end of the chunk is refused (*truncated event*, at the event's
offset). The walk must end exactly at the end of the chunk. An id the reader has no meaning for is
kept as it is and never an error: knowing an id's size is enough to step over it.

The offsets the reader reports count from the start of the file, so they can be found in a hex
editor directly.

## Values

The id's range fixes the value's size; what the bytes mean depends on the id. The reader decodes
these kinds, each checking that the value has the size it needs:

| Kind | Bytes |
|---|---|
| Bool | 1; zero is false, anything else true |
| Unsigned, signed | 1, 2 or 4 (`u8`/`i8`, `u16`/`i16`, `u32`/`i32`) |
| `f32` | 4, IEEE 754 single precision |
| Two `u16` | 4, the first `u16` then the second |
| Colour | 4: red, green, blue, then a fourth byte that is ignored **(hypothesis)** |
| String | see below |
| Binary | the bytes as they are, for structures a later piece decodes |

Ids whose kind the reader knows so far **(hypothesis unless marked)**:

| Id | Name in Suhina | Kind |
|---|---|---|
| 0 | channel enabled | bool |
| 28 | licensed | bool |
| 64 | new channel (the index of the channel the following events describe) | `u16` |
| 65 | new pattern (the same for patterns) | `u16` |
| 66 | tempo in whole beats per minute, older files | `u16` |
| 128 | colour | colour |
| 156 | tempo in thousandths of a beat per minute: 140000 is 140 BPM | `u32` **(corpus)** |
| 192 | name (of a channel or plug-in) | string |
| 193 | pattern name | string |
| 194 | project title | string |
| 195 | project comment | string |
| 196 | sample file path | string |
| 197 | project URL | string |
| 199 | FL Studio version | ASCII string **(corpus)** |
| 200 | licensee's name | string **(corpus)** |
| 201 | default plug-in name | string |
| 202 | project data folder | string |
| 203 | plug-in name | string |
| 204 | mixer insert name | string |
| 205 | time marker name | string |
| 206 | genre | string |
| 207 | author | string |

Every other id 192 to 255 is binary until a spec gives it a kind; every other id below 192 is a
plain number of its size.

Events 28 and 200 say whose licence saved the file. Tools that print events (dumps, test output)
never print their values.

## Strings

A string is the whole value of a text event, ending in a NUL **(corpus)**:

- **UTF-16LE** with a two-byte NUL, in files saved by FL Studio 11.5 or later **(hypothesis for
  the version; corpus for UTF-16LE in 26.1)**;
- **ASCII** with a one-byte NUL, in files saved by earlier versions **(hypothesis)**.

Event 199, the version, is ASCII in every version (for example `26.1.0.5530`, then the NUL)
**(corpus)**, so it can be read before the encoding is known; its first two numbers decide the
encoding of every other string. The first event 199 in the file counts. A version that is not
dot-separated decimal numbers is refused (*bad version*), and a file with no event 199 has no
readable strings (*no version*): the reader does not guess an encoding.

The text is everything before the final NUL. The reader refuses a string (*bad string*, at the
event's offset) that does not end in a NUL, a UTF-16 value of odd length, a UTF-16 string that is
not valid UTF-16, and an ASCII string with a byte above 127: what such a byte means in older files
is not known yet.

## Not described yet

- Which version first wrote which event, and how old a file the reader accepts (epic #271, piece 4).
- The meaning of ids 1, 169 and 172 seen in every file's opening, and most of the ids above.
- What the events say about channels, patterns, the playlist and the mixer (piece 2).
