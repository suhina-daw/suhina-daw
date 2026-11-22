---
description: Real-time time-stretching of warped audio, stretch modes and backends, with measurements
read_when: warp, time-stretch, stretch mode, Signalsmith, keylock, timestretch, Rubber Band, audio crackles, CPU load, stretch quality
type: decision
status: accepted
built: yes
last_verified: 2026-11-19
covers:
  - crates/engine/src/stretch.rs
  - crates/stretch/src/lib.rs
  - crates/stretch/src/grains.rs
  - crates/stretch/src/slices.rs
  - crates/stretch/src/transients.rs
  - crates/media/src/lib.rs
  - crates/engine/src/ahead.rs
  - crates/engine/src/launcher/prepare.rs
  - crates/engine/src/audio.rs
  - crates/session/src/files.rs
---

# 0003: Time-stretching and stretch modes

- **Date:** 2026-10-11
- **Task:** LOOP-3
- **Amended:** 2026-10-11 (LOOP-31: transients on the grid; LOOP-27: arrangement clips stretched
  ahead on a worker thread, decision 6; the backend changed from timestretch to our port of
  Signalsmith Stretch, decisions 2-5); 2026-10-21 (#793: launched clips' stretchers built on the
  worker thread, decision 7)

## Context

Warped audio clips map beats to times in their file through warp markers. Playing them by
reading the file faster or slower ("re-pitch") keeps the timing right but changes the pitch with
the speed. In a real project (a lead vocal warped with 43 markers, segment speeds 0.486–0.516
s/beat against 0.5) this made the vocal wobble by about ±0.5 semitones at every marker.

DAWs solve this with time-stretching that keeps the pitch, done **while playing**, so the tempo
can change at any moment. They offer several algorithms because no single one suits everything:
drums need their transients kept, voices their pitch and formants, full mixes a heavier,
higher-quality algorithm. Ableton Live calls these warp modes (Beats, Tones, Texture, Re-Pitch,
Complex, Complex Pro); our own Live Set had `WarpMode="4"`, which Live showed as Complex. Live
also keeps a per-file analysis cache (`.asd`) so opening a set does not redo that work.

GPL-compatible stretchers usable from Rust (checked 2026-10-11):

| Library | Licence | Built from | Notes |
|---|---|---|---|
| timestretch (robmorgan/timestretch-rs) | MIT | pure Rust | Real-time engine: no allocation on the audio path, `forbid(unsafe_code)`, tempo changes at exact output frames, gapless loop re-anchoring. Quality gated in CI against Rubber Band. DJ-oriented: keylock profile within ±20 %, full-range profile 0.25×–2×. |
| Rubber Band | GPL-2.0-or-later | C++ | The reference for quality at any ratio (R3 engine). Needs a C++ compiler and FFI. |
| Signalsmith Stretch | MIT | C++ | Polyphonic, quality reported close to Rubber Band R3; about 1000 lines plus an STFT, small enough to port. The Rust wrappers need a C++ toolchain. |
| Bungee | MPL-2.0 | C++ | Small, little-used Rust bindings. |

## Decision

1. **Stretch modes are part of the model.** Every audio region has a DAW-neutral
   `StretchMode`: `Repitch`, `Rhythmic`, `Tonal`, `Texture`, `Smooth` and `HighQuality`.
   Format importers map their own codes to these; Live's `WarpMode` 4 (Complex) maps to
   `Smooth`. Codes not yet observed in our corpus map to `Smooth` with a warning. A sample
   placed from the browser (`clip.addFiles`) whose tempo is known (its metadata, its name, or
   detection in `media::tempo`) is warped with two markers at the file's tempo and the stretch
   mode `clip.addFiles`' `warp` parameter gives (the window passes its Warp mode setting;
   `Rhythmic` by default); without a tempo it is not
   warped ([browser spec](../spec/browser.md) › Warping).
2. **Stretching happens in real time, in the engine.** Every warped clip placement gets a
   stretch voice when the renderer is built: a `suhina_stretch::Stretch` (built off the audio
   thread before the voice first plays, decisions 6 and 7) plus a playlist that says which part of the file plays when and how fast (warp marker segments and loop jumps,
   through the project's tempo map: an arrangement clip's pieces are also cut at every tempo
   change, and into sixteenths during a ramp, each starting at its exact time;
   [tempo map](../spec/tempo-map.md)). Launched clips are prepared at the start tempo. The stretcher reads the playlist as one continuous input stream; each call
   covers one stretch of constant speed, and the speed is the ratio of the input it is given to
   the output it makes. It never allocates once built (AGENTS.md rule 3). Where it runs:
   decision 6. Offline rendering runs the same voices (rule 4). A tempo change only means a new
   playlist, not new audio. A `Stretch` can be cloned, which copies its buffers and shares its
   FFT plans: far cheaper than building one, and a clone of one that has never processed
   anything sounds bit for bit like a new one (the Sampler makes its 32 voices that way, #1396, once, at the rate it is made for, #1402).
   - The stretcher runs at the **file's** sample rate, so the pitch is kept against the file's
     own pitch; its output is read at the project rate with linear interpolation. Transpose is
     the stretcher's own pitch shift.
   - Audio that already plays at its own speed and pitch everywhere (within 0.01 %) skips the
     stretcher and is read directly. Segments slower than 0.25x or faster than 4x, and files
     outside 8-192 kHz or with more than 8 channels, re-pitch the clip (read directly).
   - **Transpose needs the stretcher.** Read directly, a warped clip's speed is set by the tempo
     and its pitch with it, so transpose has nothing left to change: `Repitch` mode has no
     transpose (as in Live, which greys Transpose out in Re-Pitch mode), and a clip in a
     stretching mode that falls back to re-pitch (a segment outside 0.25x-4x) loses its
     transpose. Scaling the read speed by the transpose instead would take the clip off the
     beat.
   - **Timing.** The input runs ahead of what is heard by `input_latency + speed *
     output_latency`. What is fed now is heard `output_latency` frames later, so the voice feeds
     each call up to the input position heard then, plus `input_latency`: a speed change (a new
     warp segment) reaches the input that much before it is heard, and nothing has to be evened
     out afterwards. A call never gets less than half its speed's input, so the stretcher never
     runs dry. (Feeding for the speed playing now left the input ahead after a speed drop, and
     the stretcher got nothing for a few calls: 2x to 1x froze the sound for about 80 ms, 2x to
     0.5x for about 200 ms.) Transients land within 2 ms of the warp markers at 0.95-1.5x and within 4 ms at 0.5x,
     with no drift; the timestretch backend needed restarts before every onset and measured
     corrections to get there (see the measurements).
3. **Loading a project only decodes.** `suhina-media` decodes every file in parallel, one thread per core up to 16. The
   stretcher needs no pre-analysis. A region with no file yet (an empty audio clip) is skipped: nothing to decode.
4. **Modes choose backends behind one interface.** Now:

   | Mode | Backend |
   |---|---|
   | `Repitch` | none (read the file faster or slower) |
   | `Rhythmic`, `Tonal`, `Texture`, `Smooth`, `HighQuality` | `suhina-stretch`, our pure-Rust port of Signalsmith Stretch 1.3.2 (MIT), 120 ms blocks 30 ms apart; outside 0.25x-4x the clip is re-pitched |

   The modes all map to the same backend for now; they are kept in the model so projects keep
   the user's intent and later backends can tell them apart.
5. **Backend history and what comes next.** The first backend was timestretch (owner decision
   2026-10-11: timestretch first, Rubber Band later). Its keylock profile, used within 20 % of
   the file's speed, sounded harsh on larger tempo changes. In a listening test of the same stems
   at 100 bpm (from 120), its phase-vocoder profile and the Signalsmith port sounded about the
   same and both far better. The maintainer chose the Signalsmith port (2026-10-11): it is pure Rust
   without `unsafe`, has transpose built in, keeps transients on the grid by itself and opens a
   set without an analysis pass. Rubber Band stays possible for `HighQuality` as an isolated FFI
   crate (`crates/rubberband`, `unsafe` allowed only there, recorded in AGENTS.md when made), if
   a later listening test shows a difference worth it.
6. **Arrangement clips are stretched ahead of the audio thread.** Their stretch voices live on a
   worker (`StretchWorker`, `crates/engine/src/ahead.rs`) that keeps a lock-free ring (`rtrb`)
   per voice filled 0.5 s ahead; the audio thread only copies frames, asking for them by their
   position in the clip. A worker that falls behind costs silence, counted as underruns, never
   timing: late frames are dropped. When the transport stops or the play start marker moves, a
   lock-free handshake (request, worker paused, rings emptied) lets the worker move the voices to
   the marker and fill the rings again before the next start; a jump while playing moves them the
   same way, silent until the worker has caught up; that silence is the jump's and is not
   counted as underruns. Without a worker thread (offline rendering) the renderer fills the
   rings itself just before reading them, through the same voices and rings (rule 4); tests
   check that both give the same samples. Launched clips' voices still run on the audio thread;
   their stretchers are built on the worker (decision 7). An arrangement voice's stretcher is
   built when the worker first renders it.
   Engine unit tests that run this thread call `Controller::sync_stretch_worker` between fixed
   audio blocks: the thread accepts the workers already queued and acknowledges once its current
   work can make no more progress without the renderer. This also covers launch preparation and
   voice handover after an edit. The acknowledgement exists only under `cfg(test)`, outside
   `Renderer::process`; the tests wait on that signal rather than sleeping for a device period.
7. **Launched clips' stretchers are built on the worker thread.** Building one takes 1 to 4 ms
   (a mono clip at 48 kHz, release build: 1.3 ms on an Apple silicon laptop, about 4 ms
   measured elsewhere; 40 warped slots added 52 ms to every graph build), too long for the audio thread and, for every warped
   slot at every graph build, too long for an edit. So a graph's launcher plans hold their voices
   without stretchers, and a preparer per graph (`Preparer`, `crates/engine/src/launcher/prepare.rs`)
   builds them on the same thread as decision 6's workers, one per round so the rings keep
   filling: first the voices the audio side asks for (a launch queued, a clip playing), then the
   rest in slot order. The built stretcher and buffers reach the audio side through a ring
   (`rtrb`) per graph, and the launcher moves them into their voices at the start of a block,
   allocating and freeing nothing (parts it cannot use go back on a second ring).
   - **Across graph swaps** (ADR 0005), the new graph takes over the old one's built voices for
     the slots whose audio did not change, on the audio thread at the swap. One atomic claim
     per voice says who provides it, whichever comes first: the worker as it starts building it,
     or the swap as it takes the old one over. The worker leaves alone, until the new graph is
     swapped in, the voices the graph before has with the same audio, and stops building for a
     graph once it has been swapped out. An edit to one slot builds that slot's stretcher only.
   - **A launch whose voice has not arrived** starts on time all the same: its stretched audio
     is silent until the voice arrives, then plays from where the clip has got to. A voice asked
     for is built within one or two builds (under 10 ms), so with a bar's quantisation it is
     there long before the launch; an unquantised launch right after an edit that changed many
     warped slots (a new project tempo) can lose its first milliseconds.
   - Without a worker thread (offline rendering) the launcher builds a clip's stretchers as it
     launches it, so offline renders stay the oracle; a golden render
     (`tests/golden/launched-stretched.f32`) and a test that the worker gives the same samples
     check it.

## Amendment (2026-11-16): a sound of each mode, at any speed

Decision 4 kept the modes in the model while they all played through one backend, so picking one
changed nothing heard. Now each has its own stretcher, behind the same voice (`Engine` in
`crates/engine/src/stretch.rs`), with settings of the clip's own ([clip panel](../spec/clip-panel.md)
› Warp mode settings, saved as `warp_settings`, [format](../spec/suhina-format.md) version 16).
The descriptions of what each kind of mode is for come from public documentation of how DAWs
describe their warp modes; the designs and the code are Suhina's own, in `crates/stretch`.

| Mode | Stretcher | Settings |
|---|---|---|
| `Repitch` | none (unchanged) | none |
| `Rhythmic` | **Slices** (`suhina_stretch::Slices`): the file cut at its transients (`suhina_stretch::transients`: 5 ms hops, a rise of 3.5 dB and twice the rises around it, 40 ms apart, placed on the attack), at the warp markers and on a beat where no transient is near, or on every note value; each slice starts when the clip's time reaches it and plays at the file's speed, so attacks land on their beats and keep their length; cut by the next with a 4 ms crossfade, faded in over 0.25 ms; a gap after it is filled by its tail | Slice at, Tail, Envelope |
| `Tonal` | **Grains** (`suhina_stretch::Grains`): Hann-windowed grains half a grain apart, each read at the file's speed (times the transpose) from where the clip's time is, its start moved within a quarter grain to best match how the grain before goes on (a coarse search, then every frame around the best), so a held note carries on without beating | Grain |
| `Texture` | **Grains** without the lining up; each start moved at random by up to *scatter* grains, the randomness a function of the grain's number so a render repeats exactly | Grain, Scatter |
| `Smooth` | the Signalsmith port, 120 ms blocks 30 ms apart (unchanged) | none |
| `HighQuality` | the Signalsmith port, 200 ms blocks 20 ms apart: more detail for low notes and full mixes, about four times Smooth's work | none |

Grains and slices need no input buffer and add no latency, so a voice of theirs seeks by starting
afresh where the clip's time is. Slices read the file at the clip's time; grains read the playlist's
input stream at the same place (its loop passes one after another), so a grain that runs on past a
loop's end goes on with the next pass from the loop's start instead of playing what lies past the
loop in the file. The first grain plays its first half whole, where no grain before it overlaps,
so a voice does not fade in from silence. A Tonal voice's search was first a full one (every
second offset over a quarter grain): a new voice could not render ahead in time after an edit while
playing, and its first 15 ms were silent; the two-stage search reads about thirty times less.
Rhythmic and Texture change a sound even at its own speed, so playing at unity speed does not by
itself let their clips skip the stretcher: they always play through their slices or grains.

**Speeds.** Decision 2 re-pitched a whole clip in a stretching mode when one of its segments ran
slower than 0.25x or faster than 4x, so a warp marker dragged close to its neighbour (a 166 BPM loop
taken past 664 BPM) changed the clip's pitch. Every stretcher now takes 1/64x to 32x; only past that
is a clip re-pitched. The spectral stretcher's input buffer grows with the fastest speed: about
1 MB for a stereo voice at 32x and 48 kHz.

## Amendment (2026-11-22): the stretch voices on several threads

Decision 6 ran every arrangement voice on one worker thread, and its cost grows with the number
of warped clips playing at once. Eight songs of a multitrack, about three and a half minutes each,
warped together in Smooth mode fell behind on it: the trace's *stretched frames late* grew by
50 000 to 350 000 frames a second and playback broke up, with no device underrun (measured under
the Relax JIT; a native build is not measured). So the voices are now shared among a few threads
(`StretchThread`, `crates/engine/src/ahead.rs`): the cores but two (the audio thread and the
window's), from one to four. Each graph's worker is split into one shard per thread
(`StretchWorker::split`) by the voices' clip ids, which run on from one clip to the next, so a
multitrack is shared out evenly (by the voices' keys, a hash, eight clips went seven to one on two
threads) and a clip stays on its thread through its edits: its voice in the next graph falls to the
same shard, and the thread that renders it takes it over as before
(`StretchWorker::adopt_from`, ADR 0005). A rewind waits until every shard has said it stopped
pushing (one *paused* slot per shard), then the audio side empties the rings as before; jumps and
handovers stay per voice, so nothing else changes on the audio side. The launcher's preparers
(decision 7) run on the first thread. Offline rendering keeps one unsplit worker, filled by the
reader (rule 4), and a test checks that voices rendered on several threads give the same samples
as on one. Tests that run the threads wait on every thread's acknowledgement
(`StretchThread::sync`).

## Measurements (2026-10-11, the warped-vocal project: 80 s, 58 warped clips, 25 files)

With the Signalsmith port (current):

| | timestretch (before) | Signalsmith port |
|---|---|---|
| Opening the set (decode, plus analysis before) | 2.8-7.8 s | 0.4 s |
| Offline render of the 80 s set | about 22 s | 11.5 s |
| Real time on the worker: audio thread's load / 99th-percentile block / underruns | 2 % / under 1 ms / none | 3 % / 1.3 ms / none (single blocks of 11-15 ms in the paced test were the test thread being preempted, as before) |
| Transient error at 0.95x / 0.7x / changing speed | 1.7 / 5 / 2.4 ms, with restarts and corrections | 1.2 / 2.3 / 1 ms, none needed |

The rest of this section is the timestretch backend's history.

| | Pre-render at load (first try) | Real time, full-range profile only | Real time, as decided |
|---|---|---|---|
| Opening the set | 10.6–11.8 s | 3.1 s | 3.1 s |
| Average load on one core while playing | – | 52 % | 20 % |
| Worst 512-frame block (budget 10.7 ms) | – | 42 ms | 8.6 ms |
| Tempo change | stretch everything again | new playlists only | new playlists only |

Transient timing, measured with clicks every 0.5 s (`crates/engine/src/stretch.rs` tests; error
from where the warp markers put each click):

| | Before restarts | With restarts |
|---|---|---|
| Keylock 0.95× | up to 9.4 ms early | within 1.7 ms |
| Keylock with changing segment rates | up to 9.6 ms | within 2.4 ms |
| Full range 0.7× / 1.5× | 19 ms early / 4 ms late on average, up to 30 ms | 5 ms early / 1 ms early on average |

On the same project the restarts cost about 3 % more CPU; the worst 512-frame block measured
8.3–10.3 ms against 8.0–8.7 ms before (budget 10.7 ms). Rendering ahead on a worker thread (decision 6)
is what gives the real margin:

| Same project, 512-frame blocks, paced in real time | Stretching on the audio thread | Worker thread |
|---|---|---|
| Average load on the audio thread | 20 % | 2 % |
| Worst block (budget 10.7 ms) | 8.3–10.3 ms | 1.3–7.9 ms in quiet runs |
| Underruns | – | none in quiet runs; 0.75 s once with a 0.2 s ring while the machine compiled, hence 0.5 s |

## Consequences

- Warped clips keep their pitch while playing, and their timing still comes from the warp
  markers.
- CPU use grows with the number of warped clips playing at once, on the worker thread; the audio
  thread's share stays small. The worker runs at normal priority, so a heavily loaded machine
  can starve it: that is heard as dropouts of stretched clips, never as clips out of time.
- Each arrangement voice holds a 0.5 s ring (about 190 KB at 48 kHz) from the start, whether or
  not it is near the playhead.
- Every clip placement owns a stretcher and a ring, so memory grows with the number of warped
  placements; pooling voices and rings across clips that do not overlap is a later
  optimisation.
- Building a graph builds no launcher stretcher (decision 7): an edit costs the stretchers of the
  slots it changed, on the worker thread, and nothing on the caller's. Every enabled warped slot's
  stretcher is still built once in the background, so memory grows with the number of warped
  slots as with arrangement placements; building only the slots likely to launch is a later
  optimisation.
- Our own algorithm choice, not Live's: modes with the same name in another DAW will not sound
  identical. The modes describe intent, not a copy.
- Comparing backends will be a mode switch on the same clip once Rubber Band is in.
