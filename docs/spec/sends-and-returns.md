---
description: How sends and return tracks behave: what a send taps (before or after the fader), what a return plays, loops, solo and mute, latency, the commands and the send knobs
read_when: send, sends, return track, returns, pre-fader, post-fader, track.setSend, track.setSendMode, return.add, return.remove, return.rename, send knob, effect return, aux
type: spec
status: draft
last_verified: 2026-10-28
covers:
  - crates/engine/src/sends.rs
  - crates/engine/src/sends_tests.rs
  - crates/session/src/sends.rs
  - crates/session/tests/sends.rs
---

# Sends and returns

Own design. A **return track** is a track whose mixer channel has the role `Effect`: it holds no
clips and plays only what other tracks send it. A **send** is a channel's tap into a return
(`Channel::sends` in the model: the return's channel, a linear level and `pre_fader`). Projects
saved by Suhina keep sends ([suhina format](suhina-format.md)); Live Sets bring theirs in, one
per return track, all after the fader ([als format](als-format.md) › Mixer).

## What a send takes

- **After the fader (post, the default):** the track's sound after its devices, its own volume
  and its pan, times the send's level. A send at 0 dB from a centred track at 0 dB gives the
  return exactly what the track plays into its own output.
- **Before the fader (pre):** the track's sound after its devices, centred, times the send's
  level; the track's volume and pan do not change it. A track turned all the way down still
  feeds a return through a pre-fader send.
- The volume of a group the track plays into does not change its sends: they tap the track's
  own channel. A group's own sends take the group's sum. A group's pan does not apply to its
  output yet, so not to its post-fader sends either.
- A track that is not heard sends nothing: turned off (mute), or silent because other tracks
  are soloed, or inside a group that is turned off.

## What a return plays

A return sums every send it gets, runs its devices over the sum and plays into its output, the
main channel or a group, at its own volume and pan like any track. Turning a return off silences
it. While tracks are soloed, the returns are still heard, so a soloed track keeps its reverb;
tracks that are not soloed send nothing.

A return can send to another return. Nothing may feed back into itself: a send is refused when
the return it goes to already plays, through outputs and other sends, into the sending track
(`track.setSend` says it would make a loop), and a return cannot send to itself. A project that
holds such a loop anyway (a hand-edited file) opens and plays; the engine leaves out each send
that closes a loop, in channel order.

## Latency

Every send path is compensated like a group's: a return's input waits for the slowest track
that feeds it, and the output for the slowest path. A lookahead limiter on a return delays the
dry tracks by its lookahead, so dry and wet arrive together; a lookahead device on a sending
track delays what it sends, once.

## Commands

| Command | Parameters | Does |
|---|---|---|
| `track.setSend` | `track`, `return`, `level` (dB, at most 0; `"-inf"` is silence) | Sets the level of the track's send to the return, making the send (after the fader) when there is none |
| `track.setMixerValues` | `tracks`, `control` (`volume`, `pan` or `send`), `values` (one per track), `return` with `send`, `continue` | Sets each track's volume, pan or send to the return to its own value, one undo step (a drag on a selection) |
| `track.setSendMode` | `track`, `return`, `mode` (`pre` or `post`) | Takes the send before or after the track's volume and pan; makes a silent send when there is none |
| `return.add` | `name` (optional) | Adds a return after every other track, playing into the main channel; named A Return, B Return and so on when no name is given |
| `return.remove` | `return` | Removes the return and every send to it |
| `return.rename` | `return`, `name` | Renames it |

Each is one undo step. `return` names a return track (id or name); another kind of track is an
error, as is a level above 0 dB. A project saved with a louder send keeps it until the send is moved. `project.tracks` lists every track's sends (return, level in
dB, mode).

## In the window

The Launcher view's mixer strip has one knob per return beside the pan knob, as many as the row
holds (three in a track column), with the return's letter in it
([components](../design/components.md) › Mixer strip). A vertical drag sets the level
(`track.setSend`, 0.25 dB a point, Shift finer, up to 0 dB, −70 dB and below is off), a double-click turns
the send off, and a right-click opens a menu with **Before the fader (pre)** and **After the
fader (post)** (`track.setSendMode`); a dot under the knob marks a send before the fader. While
the knob is dragged, the strip's left value shows the return's letter and the level. Return
columns have no send knobs.

A send knob dragged on one of several selected tracks, in either view, moves the same send of them
all by the same amount while it is dragged (`track.setMixerValues`, one undo step), each keeping
its distance from the others; a send at silence counts as −70 dB, so sends not used yet come up
together, and a drag down to silence silences them all. A number typed or a double-click sets them
all to that value. A fader or a pan knob in the Launcher
view's strip moves its selection the same way.

## Not decided yet

- Send fields in the Arrangement track headers (planned at 114 px and taller), and a way to
  reach returns beyond the third in a narrow strip.
- Sends to returns as automation targets (with automation).
- Pan of a group on its output and its sends.
