---
description: How to find out what the running app does around a bug: the trace log (suhina --trace), adding trace points, reading the log, reading a screen recording frame by frame, and working a bug through with a tester
read_when: debugging, bug hunting, trace, SUHINA_TRACE, --trace, diagnostic log, live debugging, UI lag, slow frame, frame time, stutter, plug-in host log, ARA, Melodyne, playhead jumps, flicker, timing, latency, screen recording, video frames, contact sheet, tester, hang, freeze, working a bug through
type: howto
status: current
last_verified: 2026-10-20
covers:
  - crates/plugins/src/trace.rs
  - crates/ui/src/kit/frame_trace.rs
  - scripts/video_frames.py
  - .claude/skills/trace-debugging/SKILL.md
---

# Debugging the running app

Some bugs only show in the running app with a real plug-in and a real person at the mouse: a
playhead that flickers, a plug-in that stays "Loading", an editor that lags. Tests cannot see them
and a debugger stops what you want to watch. Suhina has two tools for these, which together found
most of the Melodyne bugs of October 2026:

1. **The trace log:** the app and its plug-in host processes write one line per event to a file
   while you use them. You read it afterwards or follow it live.
2. **Frame-by-frame video:** a screen recording of the bug, cut into frames and read as contact
   sheets or as numbers.

Both stay within [Clean-room](../CONTRIBUTING.md#clean-room): the trace records Suhina's own
processes and the calls they make, and a screen recording of another DAW is using it normally and
observing what it does. Neither steps into a third-party module: when a trace shows a plug-in's
call taking long, the fix follows what the public API and documentation require of a host (ARA,
VST3, Windows), not the plug-in's code.

## 1. The trace log

### Turning it on

```bash
suhina --trace trace.log
```

or, for a build started some other way, set the variable before starting it:

```bash
SUHINA_TRACE=trace.log suhina
```

On Windows in PowerShell: `$env:SUHINA_TRACE = "C:\path\trace.log"; .\suhina.exe`.

The plug-in hosts the app starts write to the same file. Without the flag and the variable nothing
is written, and a trace point costs one check. The switch stays in every build, release ones too,
so a user can send a log of what happened.

Each line is `<milliseconds since the Unix epoch> [<process id>] <what happened>`. The app and each
host process have their own id, so `grep "\[12345\]"` shows one process. To follow it live:
`tail -f trace.log | grep -v Memory`.

### What it records now

| Line starts with | Written by | When |
|---|---|---|
| `action <command> <params>` | app (`App::run`) | every command the window runs, with its parameters |
| `detail: …` | app | the detail area's state when it changes: the ARA editor shown, pulled to the top, focus |
| `rack starts / started / stops / rebinds key …` | app (`session/src/rack.rs`) | a plug-in starting, its result, stopping, loaded again with an ARA document |
| `host process started / connected after …` | app (`plugins/src/lib.rs`) | a host process starting and how long until it answers |
| `placement sent …` / `placement waits` | app | an embedded editor's placement going to its host |
| `app gets ara notes for clip-N: count` | app | notes an ARA plug-in found, taken in |
| `app: ara position S -> beat B` | app | an ARA plug-in's click in its timeline, as the transport takes it |
| `host up, connecting` / `host connected` | host | the host process starting |
| `host gets <message>` / `host sends <message>` | host (`plugin-host/src/main.rs`) | every control message in and out, cut to 300–400 characters |
| `host unloads N` / `host unloaded N in …` | host | an instance let go of, and how long it took |
| `vst3 unload: …` / `vst3 drop: …` / `vst3 audio: …` | host (`vst3.rs`) | each step of a VST3 plug-in's teardown, to find the call that hangs |
| `ara region …` | host (`ara.rs`) | an ARA playback region described to the plug-in: start, duration, offset |
| `ara analysis progress …`, `ara content changed …`, `ara notes …` | host | the plug-in's analysis and content notifications, and the notes read |
| `ara plug-in asks transport …` | host | the plug-in's play, stop and position requests |
| `ara musical context changed` | host | the tempo map or time signatures sent to the plug-in again |
| `slow frame N ms: part ms, …; what was shown` | app (`kit/frame_trace.rs`) | a frame whose drawing took 25 ms or more, part by part (› UI time) |
| `frames: N in 1.0 s …` | app | once a second while the window draws: frames, drawing time on average and the longest, the longest wait between frames |
| `audio: device underruns N (+n), stretched frames late N (+n), engine load …` | app | once a second, when sound went missing since the last look or the engine's load is 70 % or more (› Audio dropouts) |
| `slow <work>: N ms` | app | library listings, browser rows, the sample map and its cache, a drop, adding to a track: anything that took 5 ms or more |

### UI time

A lagging window shows in the log by itself, with no trace point to add. Each frame of the
window is timed part by part, and one that takes 25 ms or more (under 40 frames a second) is
written out:

```text
1760887012345 [9120] slow frame 41.3 ms: input 0.6, grid 1.2, bars 0.8, browser 2.1, detail 3.4, view setup 0.7, timeline 22.9, arrangement view 8.8, commands 0.4; arrangement, 6 lanes, 24 clips, playing running, timeline rebuilt, 1 commands
```

The parts, in the order the frame draws them: `input` (keys, polls, the sample map window),
`grid` (the session's state for the views), `bars` (menu, control and status bars), `browser`,
`detail` (the detail area: clip column, devices, piano roll), `view setup`, `timeline` (the
Arrangement's lanes and clips, rebuilt only after the project changed: `timeline rebuilt`),
`arrangement view` or `session view` (drawing it), `views`, `drags`, `dialogs`, `commands`
(running what the frame asked for) and `rest`. The part that stands out is where to look.

Once a second a `frames:` line sums up: how many frames, their drawing time on average and the
longest, and the longest wait between two frames. A long wait with short drawing means the time
went elsewhere: egui laying out and painting the shapes, the GPU, or the window at rest (it
draws only when something changes, so a wait of seconds while nothing moves is normal).

### Audio dropouts

A sample that sometimes does not sound, or sound that stutters, shows as `audio:` lines while it
happens. Two counters tell the causes apart:

- **device underruns:** the engine had not filled the audio device's buffer in time, so the
  device played silence for what was missing. The engine load and the buffer size on the line say
  whether the engine was too slow (a high load) or the buffer too small for the machine.
- **stretched frames late:** a warped (stretched) clip's audio, made ahead on a thread of its
  own, was not ready when the engine reached it, so that clip alone played silence. A Live Set
  with many warped clips leans on this.

Lines come only when a counter grew or the load is high, so a session that plays cleanly writes
none.

### Adding a trace point

In any crate that depends on `suhina-plugins` (the app, the session, the UI, the plug-in host):

```rust
suhina_plugins::trace!("rack stops key {key}");
```

The arguments are only formatted when tracing is on. For a state worked out every frame, trace it
only when it changes:

```rust
suhina_plugins::trace::state("detail", format!("pulled {pulled} focus {focus:?}"));
```

Inside the `suhina-plugins` crate itself the macro is `crate::trace!`. Name what happened in plain
words and put the numbers that tell one case from another in the line (ids, times, sizes). Leave a
trace point that helped find a bug in the code: the next bug in the same place needs it too. Trace
points on the audio thread must not allocate: there, set an atomic and trace from another thread.

### Reading it

- **Order and gaps:** the timestamps show what came before what, and how long each step took. A
  plug-in that stayed "Loading" showed as `host unloads 1` with no `host unloaded 1` after it, for
  120 seconds (the start timeout); the steps of `vst3 drop:` then named the call that hung.
- **What did not happen:** a step you expect but cannot find is often the bug. Three `device.addPlugin`
  actions with no `host gets Load` after them meant the app never sent the load.
- **Round trips:** a request and its answer in two processes, `ara plug-in asks transport Position`
  in the host and `app: ara position` in the app, show the latency (about 100 ms, which was one
  frame lost to the reply queue).
- **Noise:** `Memory` arrives once a second from every host; `grep -v` it away.

## 2. Frame by frame from a screen recording

A flicker of one frame, or which of two lines moved first, cannot be seen by watching. Record the
screen (OBS, ShareX or the like) while you reproduce the bug, then:

```bash
# Frames at 6 per second for an overview, at the video's own rate for timing.
python scripts/video_frames.py extract bug.mp4 frames --fps 6
python scripts/video_frames.py extract bug.mp4 full --fps 24 --from 21 --to 30

# A contact sheet: a run of frames in one image, each with its number and second.
python scripts/video_frames.py sheet frames overview.jpg --every 3 --cols 10 --size 320x180

# The same part of each frame at full size: the clip, the plug-in's timeline.
python scripts/video_frames.py sheet frames clip.jpg --first 130 --last 150 --cols 3 --size 640x210 --crop 760,520,1920,900

# Where thin vertical lines are in a strip of each frame, printed when they change: a playhead,
# an insert marker. --scale turns pixels into bars (pixels a bar, a pixel x and the bar there),
# --pointer follows the mouse in a band (the ruler).
python scripts/video_frames.py track full --strip 800,1700,740,746 --fps 24 --scale 99.5,806,4 --pointer 528,550
```

`track` printed this for the Melodyne playhead bug: after each click the line went to the new
place, back to the old one for one frame, then to the new one again; the same run in another
DAW never went back.

```text
  23.04s frame   50  lines=[7.05]  pointer=7.09
  23.09s frame   51  lines=[7.57]  pointer=7.09
  23.21s frame   54  lines=[7.05]  pointer=7.09
```

Tips:

- An image is shown to an AI model at about 2000 pixels; a sheet of many small frames loses the
  detail. Crop to what matters and keep a sheet to a dozen frames or so when the detail counts.
- Compare with another program doing the same thing: the same steps recorded in both make the
  difference plain.
- Run the trace log at the same time; the times of the log and the video can be matched by an
  action you can see in both (a click, a play).

## 3. Working a bug through with a tester

How the bugs of the October 2026 test rounds were found when one person used the app and an agent
read what it did. The tester reports in their own words ("it lags", "it hangs", "something
flashed"); the trace and a recording say where.

1. **Every run on a trace of its own.** Start the app with `--trace` into a numbered file
   (`trace-41.log`, then `trace-42.log`) and say which number is running, so a report and its
   trace stay together. A test build can carry a small start script that picks the next number.
   Restart only when the tester agrees: a running project may have changes they want.
2. **Ask what they did and roughly when.** The last `action` lines show what the window ran; a
   tester's "I closed it with Don't save" finds its place among them.
3. **Read process by process.** Tell each process id apart by a line only it writes: a plug-in
   host's start with `host up`, the window by its `frames:` lines (its first line can be another
   one). The last line of each says where it stopped. Then look for gaps in time
   within one process.
4. **Ask for a recording** when the bug is something seen (a flash, a jump, a late redraw), and
   read it as in part 2: the frames where the picture changes most, as a contact sheet.
5. **Fix, then have the tester repeat the same steps** on a new build and a new trace, and ask
   what they saw. The line that showed the bug is gone, or a line added for it says how long the
   step takes now; for something seen, a new recording shows it. A missing line alone proves
   nothing: the run may not have reached the failing step.

What told the causes apart:

| The tester said | The trace showed | The cause |
|---|---|---|
| The window hangs on quitting without saving | The window's `frames:` lines stop; the hosts unload one by one; one host prints nothing for 16 s, then a burst of `ara` lines and its `Unloaded` | The window waited on each plug-in in turn while an ARA plug-in finished an analysis on its host's main thread |
| Turning a knob lags | A `slow engine update` (a new graph, 60 to 80 ms) for every step of the drag | The change took a new graph rather than a message to the playing engine |
| It is sluggish while nothing moves | `frames:` at 75 or more a second at rest | A view asked for a frame on every tiny change (the meters, below hearing) |
| The plug-in's editor stalls | The host's calls into the plug-in taking seconds, at a steady interval | The host stored the plug-in's whole state on the thread its editor runs on |
| Something flashed | Nothing; the recording showed the start panel for two frames before the window closed | The dialog was taken down one frame before the window was gone |

Lines that simply stop do not say whether a process is waiting, busy or gone: a trace records
events, not a process's end, and a crash or an exit stops it too. While the bug is still on screen,
look at the processes first: in PowerShell
`Get-Process suhina* | Select-Object Id, ProcessName, CPU, Responding` (`Responding` is false for
a window Windows sees as hung) or `ps` elsewhere, and how its processor time moves over a few
seconds (busy, or waiting or idle). A line on the other side (`host gets Unload` with no
`host unloaded`, or the window's frames ceasing) then points at who may wait for whom; confirm it
in the code that sends and waits before calling it the cause. Ask the tester not to end the
process before you have looked.

## When to take a trace point out

Keep trace points. Take one out only when it is noisy and no longer tells anything, and say so
in the commit. The ones added for the Melodyne bugs of October 2026 stay until the maintainer
asks for them to go.
