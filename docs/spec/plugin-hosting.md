---
description: How Suhina hosts plug-ins in detail - host processes and instances, joint round trips, events in a block, the deadline and real-time priority, state, formats, VST2 projects, scanning, editors, plug-ins in groups, latency, output changes and stopping instruments
read_when: plug-in host process, hosting choice, plugin-hosting.json, joint round trip, deadline, late blocks, real-time priority, rtkit, MMCSS, plug-in state, VST2 replacement, scanning, scan worker, plug-in editor, embedded editor, Device view, plug-in latency, retained instances, output change, stop fade
type: spec
status: current
last_verified: 2026-11-02
covers:
  - crates/plugins/**
  - crates/plugin-host/**
  - crates/realtime/**
  - crates/engine/src/plugin.rs
  - crates/engine/src/plugin_params.rs
  - crates/session/src/rack.rs
---

# Plug-in hosting

How the decisions of [ADR 0006](../decisions/0006-plug-in-hosting.md) behave. The messages, the
shared memory and the version history are in [the plug-in protocol](plugin-protocol.md). Own
design; how other hosts do it is in the ADR's Context.

## Processes and instances

a host process runs any number of instances, each loaded and unloaded on its own
under a number the engine gives it, with its own link (shared memory and doorbells) and its
own audio thread; the host's main thread serves every instance's callbacks and editors, and
several editors may be open at once. A VST3 module is loaded once per process however many
instances use it, so `InitDll`/`ModuleEntry` and `ExitDll`/`ModuleExit` run once (`clack-host`
does the same for a CLAP entry's `init`/`deinit`). An instance whose audio thread does not
stop when it is unloaded is stuck in the plug-in and is left as it is, so the others go on.
An unloaded instance's audio stops and the engine gets its answer before its editor closes:
the host takes the editor down and releases the plug-in from its main loop afterwards, as an
editor embedded in the engine's window sends that window messages while it goes, which the
engine answers only once it stops waiting. The engine waits for an unload at most
10 s (or the launch timeout, when shorter); a process that has not answered by then is stuck
in the plug-in's teardown and is ended, so a closed project or a removed plug-in never holds
up the window, and a restart then loads into a new process.
On the engine side, `Launch::group` names the process an instance starts in: instances with
the same host executable and group share the process a registry in `crates/plugins` keeps
while any of them lives; `None` is a process of its own. Requests from the instances are
taken in turn, and replies are matched by their `op` and instance. Each group counts the
processes started for it (its generation): when an instance is restarted because the
process died, does not answer, or this instance hung in it, the process is ended and a new
one started, and the instances loaded into the old one report *crashed* until their own
restart loads them into the new one (never a second new one). A restart of a healthy
instance unloads and loads it again in the same process. The last instance to go ends the
process. The hosting choice per plug-in (`Hosting`: `binary`, the default, `instance` or
`vendor`) is kept in `plugin-hosting.json` in the settings folder; `library::group_for`
turns it into the group: `file:<path>` (every plug-in in one file or shell together),
`vendor:<name>` (the file's group when the vendor is unknown) or none. An ARA plug-in is
never hosted alone: `instance` is refused for it and read as the file's group
([ADR 0033](../decisions/0033-ara-hosting.md) decision 2).

## One round trip for consecutive plug-ins (#108)

A block may name
instances of the same process to run after the link's own (`Link::send_joint`, up to 63;
the engine sends at most 16 plug-ins a block). The host keeps each instance's audio
processing (its started CLAP processor or VST3 processor with its buffers) behind a lock of
its own while the instance's audio thread serves, by instance number
(`crates/plugin-host/src/audio.rs`), so the audio thread that has the block runs the
followers in turn, in place, each with the block's events, and answers once. A thread
holds one lock at a time, so two threads running each other's instances never wait on each
other; a follower that is gone passes the sound through. In a chain, the plug-ins after a
plug-in that are enabled, run in the same process (the same generation: generations come
from one counter for every group, so they tell processes apart) and can take part now
(`PluginProcessor::joint`: no block of their own still out, no releases waiting to be sent)
follow it; a disabled plug-in is passed over, a built-in device or any other plug-in ends
the run. Each follower counts the block as its own (`PluginProcessor::followed`): a block
that is late or refused is a miss for every plug-in in it (the sound passes, an instrument
is silent, as when each misses alone), counts against each one's *hung* count, and a refused
block's releases wait in each. A hang anywhere in the block so makes every plug-in of it
*hung*, as the engine cannot tell which one is stuck; the process is restarted either way.
A plug-in with parameter events in the block (#109) runs on its own, and ends the run: a
joint block carries one event list, which every plug-in in it is given, so one plug-in's
values would reach the others. An instance restarted for a latency change (#110) serves
under the same number again, so blocks go on naming it.
The output is the same, sample for sample, as one round trip each.

## Events in a block

each block carries up to 1024 events (note on and off, MIDI controller, pitch
bend, channel pressure, parameter value) as fixed 16-byte records with their frame
(`crates/ipc/src/events.rs`; the block's followers too, *One round trip* above); the host reads them into a list made beforehand, so neither side allocates.
Two more kinds carry the song: `Transport` (playing, position in samples) and `Tempo`, sent
with every block to a plug-in that follows the song (one loaded with an ARA document,
[ADR 0033](../decisions/0033-ara-hosting.md)), which a VST3 host puts in the block's process context.
A VST3 host hands notes to `process` as an `IEventList` on event bus 0, and controllers,
pitch bend and channel pressure (aftertouch, #812) as parameter changes at their frame
through the plug-in's `IMidiMapping` (asked for the first MIDI channel only: some plug-ins
crash when asked about the second), and gives the latest value of each to the edit
controller as well (`setParamNormalized`, on the main thread between messages), so the
plug-in's editor shows a wheel moved on the hardware; a CLAP host sends note events to note port 0 and
controllers, bend and channel pressure as MIDI messages, and gives a plug-in without audio
inputs (an instrument) no input port. A parameter event (automation) reaches a VST3
processor as a parameter change at its frame and a CLAP plug-in as a parameter event at its
frame.
the engine sends them. A plug-in slot plans each block's parameter events
before any of the block runs (`crates/engine/src/plugin_params.rs`): the values the project
sets on the device (`Device::params`, by parameter id, in the plug-in's own range) at the
start of the first blocks a processor runs and again after a restart, over the state it
loaded; a value `device.setParam` sets while it plays, at the start of the next block
(the same parameter message as a built-in device's, ADR 0005); and, while the transport
runs, each automation lane's value at the block's first frame, at each point inside it and
every 64 frames while it moves (a VST3 plug-in ramps between them, a CLAP plug-in steps).
The slot hands each span it runs its share through `PluginProcessor::automate`, and the
session's processor merges them with the notes in frame order, a value before a note at
the same frame; a block the host could not take carries the latest value of each
parameter to the next. Nothing allocates on the audio thread: each slot has room for 512
events a block. A parameter value that reaches the processor from the engine (a panel
set, an automation lane) also goes to the edit controller on the main thread
(`setParamNormalized`: the same call a MIDI controller's mapped parameter gets, #812),
so the plug-in's editor follows the sound; and the host reports what was changed in its
editor back (`param_changed`, #828), so the session keeps
the project's values in step with it — no undo step of its own, and a change since the
last save.

## Block sync and the deadline

The engine signals the host, the host processes and signals
back. Primitives: a futex in the shared memory on Linux; duplicated auto-reset events on
Windows, waited on together with a high-resolution waitable timer that wakes the wait 1 ms
before the deadline, the rest spent yielding, so a miss ends on time and not on the system
timer's next 15.6 ms tick (#211); on macOS, checks with short naps between them — measured on
an M3, a mac wait that only yields never sleeps, and the timeshare scheduler then demotes the
thread for burning a core and steers it to the efficiency cores, where its round trip misses
the deadline: 138 µs at rest, p50 651 µs and growing to 17 % of blocks late with three busy
processes on the machine. A nap counts as waiting and the demotion does not accumulate;
250 µs caps each requested sleep, although OS scheduling can delay the actual wake-up. A semaphore wait
remains the surest wake, and the reason it has not landed: Mach semaphores live in one task
and cannot be shared with another process by name (their send rights would have to go over
Mach IPC), and named POSIX semaphores answer `sem_open` across processes but Darwin has no
`sem_timedwait` to end a wait by its deadline. The host's
audio thread runs at the engine's real-time priority (MMCSS "Pro Audio" on Windows; the
engine's audio workgroup on macOS). The engine waits at most the rest of the block's time.
On a miss it outputs the plug-in's input (an effect) or silence (an instrument) for that block
and counts the miss. Repeated unanswered requests, a dead process or an unanswered ping mark the plug-in
*crashed* or *hung*; the engine kills the process and offers a restart. Offline renders wait
without a deadline (with a long timeout), so they run as fast as the plug-in allows; a block
left unanswered there says whether the process died (the plug-in crashed) or is still stuck.
the host's audio threads and the desktop app's render thread take their priority
from `suhina-realtime`, which says what each system gives. **Windows:** MMCSS "Pro Audio"
(`audio_thread_priority`, MPL-2.0), which also bounds a thread's share of a core;
`THREAD_PRIORITY_HIGHEST` only where MMCSS refuses. **Linux:** `SCHED_FIFO` at priority 10
set directly where the user has real-time rights (`rtprio`, the `audio` group); otherwise
rtkit over the system D-Bus (`zbus`, pure Rust, so nothing to install or ship), which grants
`SCHED_RR` at 10 or its ceiling when lower, after `RLIMIT_RTTIME` is set within rtkit's
maximum as it requires; otherwise the highest normal priority, and otherwise the normal one.
Priority 10 keeps the sound server's own threads (PipeWire, JACK) above the ones feeding
them. The D-Bus exchange runs on a helper thread with a 2 s limit, so a system bus that never
answers costs no more. rtkit's grant comes with a limit the kernel enforces: a real-time
thread that runs `RLIMIT_RTTIME` (200 ms by default) without waiting ends its process. The
render thread tells `RealTime` when it waits and how long each block took: it rests 50 µs
after an eighth of the limit without waiting (a device that drains faster than it plays, a
project too heavy to render in time), and leaves real-time scheduling for good when one
block alone takes a quarter of it, so an overloaded project drops out instead of ending
Suhina. The hosts' audio threads wait for every block, so only a plug-in stuck in one block
that long hits the limit, and the host process ending is then a crash the engine already
reports and restarts. **macOS:** the highest priority (the audio workgroup is to come).
Measured on an M3, what a priority alone does not get: macOS refuses QoS on a thread whose
priority another call has already raised (set the class first); the user-interactive class
alone did not keep a waiting thread off the efficiency cores; and the time-constraint policy
(`THREAD_TIME_CONSTRAINT_POLICY`) on a thread that waits by yielding defers its work to the
policy's period — 81 % of blocks late in the same bench — so a scheduling class is worth it
only once the waits sleep. Until then the napped wait (above) keeps a mac thread's timeshare
band clean without either.
Nothing fails over it: when real-time scheduling is refused the first stream in a process
adds a notice (the app shows it, `suhina-cli play` prints it), and the hosts' threads, which
ask the same system, play on silently. `SUHINA_NO_REALTIME=1` turns it off, to compare.
A late reply resets the hang counter when the next request is accepted: missing
an audio deadline does not mean the host stopped answering. Superseded starts
after a rate change are unloaded off the session thread, like accepted instances.
With measured render lead, the deadline uses the complete device periods covered by queued
frames minus 3 ms, with half a render batch as the startup floor. All internal chunks
of the batch share that absolute deadline: the 1024-frame internal limit must neither
cap a short processing burst while queued audio covers it nor renew the time allowed
for a hanging processor. Sustained overload still exhausts the queue and misses its
deadline; extra processing time does not add buffered audio or latency. Playback keeps four engine blocks
queued (or a device period and a block, whichever is larger); armed tracks keep
their shallower queue. Without measured lead the deadline is 60 % of a block's
playing time for all of a block's plug-ins together, one after another, so it depends on
the block: the audio output renders the engine in blocks of its own ahead of the device into a
ring, rather than in the device's buffers, after a driver that gave 3 ms buffers left 25
plug-ins 30 to 50 % of their blocks late (notes dropped, "stopped answering"). The block is
the buffer size the user chooses (Settings › Audio, `audio.setOutput`: 64 … 4096 frames,
rounded to a size the driver allows; 1024 when none is chosen), so the deadline follows that
choice: a smaller buffer is quicker to answer a key and leaves the plug-ins less time.
Ordinary playback keeps four blocks queued; armed input keeps its shorter queue.
The half-batch minimum and queued-period budget above retain the room needed by
plug-ins at small device buffers without capping a queue-covered batch at one block. The
driver is never asked for a size outside the buffer range it declared: a range holding no size
the engine renders in leaves the driver its own, with a notice that says which size plays.
With ASIO and WASAPI exclusive mode the device's buffer is that size too; WASAPI shared mode
keeps the period Windows gives it, and the ring holds a device buffer and a block when the
device takes more than a block at once. WASAPI offers sizes up to 2048 frames: past that a
shared stream only gains latency, and interfaces refuse such an exclusive period (N48). Where the device's buffer is not Suhina's to set, the
Settings row says *Engine block*, since the number is the engine's block and not the device's
buffer. The session counts each plug-in's blocks and late blocks
(`device.plugin`: `blocks`, `late_blocks`, `longest_us`, `no_time`), and the device panel
shows the share. Even so the sum of 25 plug-ins' times (a heavy synth up to 9 ms a block)
passed the budget on half the blocks, so the chains no chain plays into now run their
plug-ins *at the same time*: every chain hands its block to its first plug-in
(`Link::send_with_events`, `PluginProcessor::send`); then the engine looks at the chains
whose blocks are out in turn (`Link::wait_answered`, `PluginProcessor::poll`), takes each
answer as it comes (`Link::wait`, `PluginProcessor::wait`) and runs that chain on to its
next plug-in at once, while the others are still out, until every chain is done (#151); a
bus runs after the chains that play into it. The budget is then set by the slowest chain,
not the sum: the same project went from 30 to 50 % of blocks late to none, the longest
wait 6 ms. (Until #151 the chains went a stage at a time, every chain waiting for every
chain's answer before any went on, so a chain waited once per plug-in for the slowest
plug-in of that stage elsewhere: measured with three chains of three plug-ins whose heavy
one, 3 ms a block, is at another place in each, see *Measurements*.) While no answer is in,
the render thread sleeps on the first chain still out for at most 50 µs at a time
(`ANSWER_SLICE`): that plug-in's answer wakes it at once, another's is seen at most that
late. A chain whose plug-in has not answered by the deadline misses that block, as before;
the others are not held up by it. An offline render gives each block out `OFFLINE_TIMEOUT`
from when it went. `longest_us` is each plug-in's longest round trip, timed from the block's
hand-over, since the engine may look at other chains before it takes an answer. A plug-in still at a block it was late with (one whose answer
came after its deadline, as when a synthesizer takes longer over the notes of a start or a
stop's all-notes-off) is waited for like an answer: its chain hands it the next block as
soon as it is done (`PluginProcessor::ready`, `PluginAudio::ready`, `Link::wait_ready`),
while the other chains go on, and only until that block's deadline (#506). Only a plug-in
late with the block before alone is waited for; one that has not answered several in a row
(hung, or far behind) is not. Measured with `measure_quick_play_and_stop` (see
*Measurements*): an instrument that took longer than a whole block over each press of play or
stop lost two blocks a press, the late one and the next, refused, with the notes it
brought; it now loses the late one only. A host still busy when the deadline comes refuses
the block (`Sent::Refused`): the round trip is one miss and the block is lost to the
plug-in, so an effect passes the sound through and an instrument is silent for that block,
as when it misses its deadline. The chain does not
try it again through `process`, which would count it a second time and make a slow host
*hung* after 16 round trips instead of 32 (#188). An offline render waits for the busy
host, up to `OFFLINE_TIMEOUT` (`PluginProcessor::send`'s patience, `Link::wait_ready`),
so no block of a render is lost to it. Misses count per round trip: a block is one, or
two when its events fill the chain's queue. `Sent::Unsupported` is for processors that do
not split a block, which run it with `process`. A restarted plug-in reads as running at
once (`Plugin::health` keeps what it found last, and a restart sets that), not as the
instance it replaced.

## State

The host pulls each plug-in's state on save and periodically;
the saved project keeps it in `Device::state` (a VST3 controller's in an extension), and the
open project keeps what it was opened with, so reading it is neither a change nor an undo
step (a state that changed in the editor since the last save still makes the project
unsaved, [ADR 0010](../decisions/0010-project-files.md) decision 7). A restart loads the last state and keeps the latency
the chain was compensated for. `project.save` asks every running plug-in for its state as it
is then, each on a thread of its own, waits at most a second for the answers (the last state
read stands in for one that has not answered) and writes that, with the last state of
stopped ones
([ADR 0010](../decisions/0010-project-files.md)).
A render from an open project (`project_render`) reads the states the same way
(`Session::project_as_heard`, which the save uses too) and starts its own offline instances
with them, so it plays what is heard, edits made in an editor since the project was opened
included (#363). The running instances are only asked for their state: the render's
instances are separate host processes, as for any offline render. An export from the app
gathers the same states over its window's frames, never waited for on its thread
(`Session::gather_export_data`): the round of reads is its own ledger, so a project stopped
mid-read takes the instances and not the answers, the deadline settles what did not answer
by name, and a cancelled export's late replies start nothing. Replies are consumed while
the round's ledger is still installed, including replies from detached instances. A failed
state read also names the plug-in before falling back. These warnings travel with the
export result, even when another tab is active. Attached MCP exports use the same frame
gather, including exports in batches; only one gather can own a project's reads at a time.

## Formats

CLAP first, then VST3, on the same boundary. CLAP goes through `clack-host`:
the instance stays on the host's main thread (with the plug-in's main-thread callbacks), the
started audio processor on its audio thread. Every audio port the plug-in declares gets a
buffer with the channels it declared (a plug-in may read them all, and one given fewer can
crash, #327); the link goes through the main ports and the rest get silence, by one channel
rule for both formats (`audio::feed_main` and `audio::collect_main` in
`crates/plugin-host/src/audio.rs`, #767): a channel of the main input the link has none for
repeats the link's last channel, so a mono source reaches both sides of a stereo input; a
channel of the link the main output has none for repeats the output's last, so a mono output
reaches both sides of a stereo link. A
plug-in is named `clap:<file>` or
`clap:<file>#<id>`; the only `unsafe` the host needs for it is opening the file. A CLAP gain
built into the host (`test:clap`, made with `clack-plugin`) lets the tests run the whole path
without a plug-in installed, and `test:clap-ports` has a rack's port layout (8-channel main
ports and stereo side ports); `SUHINA_TEST_CLAP` runs them against a real file too.
VST3 goes through the `vst3` bindings to the SDK: the component and controller on the main
thread (connected through their connection points, the controller given the component's
state; the host makes the messages and attribute lists they send each other,
`IHostApplication::createInstance` in `crates/plugin-host/src/vst3/message.rs`, without which
some plug-ins crash as soon as they are connected and others when given a state), `setProcessing` and `process` on the audio thread, the link on the main buses (by the rule above) and
silent buffers for any other bus. A plug-in is named `vst3:<bundle or file>[#<class id>]`;
in a bundle, the executable is looked up per system: `Contents/<arch>-win/<name>` on Windows,
`Contents/x86_64-linux/<stem>.so` on Linux, and on macOS in `Contents/MacOS` (the file named
after the bundle without the extension, or else the first non-hidden file in the folder, as
the bundle format lets the executable be named anything). The module's entry is called the
same way per system, and it is optional: `InitDll`/`ExitDll` on Windows,
`ModuleEntry`/`ModuleExit` on Linux, `bundleEntry`/`bundleExit` on macOS, all called only
when the module exports them. The entry matters where modules build the plugin their class
information is read from in it, as DPF-built modules (Cardinal, plugdata) do on macOS:
answering a class query before the entry has run crashes them (a null dereference in the
module), which a scan would then report as a crash while loading.

the state handed to the engine is the component's. `test:vst3`, a gain with a separate
controller built into the host, plays the part of `test:clap`. **Class ids** are written as
the SDK writes them (`moduleinfo.json`, the model): on Windows the first three fields of the
COM GUID layout are turned back into numbers, so the text matches across scanning, the
module's own description and imported projects.

## VST2 projects

A project that names a VST2 plug-in is opened
with the installed VST3 that declares it replaces it, and the saved VST2 state is handed to
it. Without such a VST3 the device stays in the project as a *missing plug-in* with its state
intact, and the user can relink it by hand to a VST3 or CLAP version. Hosting VST2 itself is
a separate decision (below).

*Finding the replacement* (`crates/plugins/src/matching.rs`, `plugin.check`). The catalog is
searched in order of how sure the answer is: the same id; an installed VST3 that declares it
replaces the plug-in (`Compatibility` in `moduleinfo.json`, or the module's
`Plugin Compatibility Class`, which the scanner asks through `IPluginCompatibility`); a VST3
whose class id is derived from the VST2 id by the SDK's convention (`VST`, the 4-byte id, the
first nine bytes of the name in lower case; the name part is not compared, as a project keeps
the name its host showed); the same name once build words (`x64`, `64bit`, `.64`) and a
leading vendor name are set aside, or the name without its number when the installed
plug-in's version starts with that number (`Synth 5` finds a VST3 named `Synth` whose
version is 5.x and whose class id follows no convention); and last the nearest newer version
of the same name (`Synth` → `Synth 2`, `EQ 2` → `EQ 3`, and with the number inside
the name, `Suite 8 Imager` → `Suite 11 Imager`). Only the first three promise that the state carries over;
for the last two the state is handed over and the plug-in decides, and the result says
which (`keeps_state`). Vendors declare their own older builds rather than older products:
an `EQ 3` module declares the VST2 builds of `EQ 3`, not `EQ 2`, which is why the name
steps exist.

*Handing over the state.* The importer keeps a VST2 plug-in's state as the standard VST2
bank (`CcnK`, the format the SDK's `vst2persistence` reads), and the host hands it to the
VST3 component's `setState` and mirrors it to the controller. A VST3 project's controller
state goes with the component's (`set_state` carries both) and is
restored after a restart. A plug-in that refuses the state runs with its defaults and the
caller is told (`offline::render_with_state`, `suhina-cli plugin run-device`). Measured on
a real project: two effects read their VST2 chunk banks (the output changes with the saved
settings); a third takes a parameter bank (`FxBk`) in `setState` without applying it. So the host also sets a parameter bank's current program as
parameters, value `i` to parameter id `i`, the numbering a VST3 version keeps from its VST2
one, but only when the plug-in has every id from 0 to the last value's: on the controller
(`setParamNormalized`) and on the processor through the next block's parameter changes
(`crates/plugin-host/src/vst3/params.rs`). With that the third plays with the project's settings.
A newer version of an EQ refuses its predecessor's state and runs with its defaults. An
instrument found by the id convention plays a project's notes with its saved settings. A
synth's second version refuses the first version's state in every form tried: the bank, a
single program (`FPCh`, an `.fxp` file's contents) and the bare chunk; it opens the first
version's presets only from its own browser, so such a project plays with the defaults
until a way is found.

In a real project with 14 different VST2 plug-ins, 4 were found by the id
convention, 1 by name and 3 as newer versions; the other 5 exist on that machine only as
VST2.

## Scanning

runs in its own short-lived process with a timeout per plug-in, a cache keyed by
path, modification time and size, and a block list of plug-ins that crashed or hung. CLAP's
factory and VST3's `moduleinfo.json` are read without instantiating plug-ins where possible.
It looks in the system's standard folders (and, for CLAP, the folders in `CLAP_PATH`) plus
folders the user adds, so no setting is needed to find installed plug-ins. Implemented in
`suhina_plugins::scan` and `scan_pool` for CLAP and VST3:
- A VST3 bundle with a `moduleinfo.json` (JSON5) is read from it without loading anything.
Both ways note whether a plug-in supports ARA (an `ARA Main Factory Class` paired with it,
[ADR 0033](../decisions/0033-ara-hosting.md)).
- Everything else goes to a few long-lived `suhina-plugin-host --scan-worker` processes
(half the processor's threads, at most 4), each reading file after file. A worker that
crashes or hangs on a file is ended, the file is marked failed with the reason, and a new
worker takes the rest. Shells (files with many plug-ins, such as a vendor's shell plug-in) go first
and get 180 s, other files 30 s.
- Measured on a Windows machine with 443 plug-in files: a process per file took 11.5 min and
timed the shells out; this takes 86 s and finds every plug-in (733). Starting a process
cost about a second there (antivirus checks each start), more than most plug-ins take to
load.
- A failure can be passing: a plug-in that waits on a lock another process of its vendor
holds (seen with a vendor's daily background sync) gives no factory or hangs, more often
when files load side by side. So the files that failed in the parallel pass are read once
more, one at a time, at the end of the same scan, and the second result is kept. A file
that still fails keeps its reason and is listed as failed; later plain scans try it again
while it is unchanged, one that hung at most once a day (a hang costs its whole timeout),
so a plug-in that always hangs slows one scan a day, not every start..
- Two results are settled: not read again while the file is unchanged, and not listed
among the failures. A file that loads and holds no audio plug-ins (a vendor's shell
bundle with no classes) is passed over; `plugin.list` with `ignored` lists it. A file
built only for another processor (an Intel-only plug-in on an Apple silicon Mac) is never
loaded: its executable's header (Mach-O, PE or ELF, `suhina_plugins::arch`) says so before
any load, and it is listed apart as `incompatible` with the processors it is built for.
The catalog (format 3) keeps when each file was last read; a catalog in format 2 is read
as it is. A failure's reason is kept to its first line (a loader can list every path it
tried).
- Each added folder is for one format (VST3 or CLAP) or for every format Suhina hosts, and
any folder, standard or added, can be switched off without removing it; its plug-ins leave
the catalog with the next scan. VST2 folders are a format already, refused until VST2
plug-ins are hosted, so adding them later is a matter of the scan, not of the folders.
- Plug-ins can be switched off (`plugin.setEnabled`, the plug-in manager). One switched off
stays in the catalog and in `plugin.list`, but sessions get the catalog without it: it is
not offered in the browser, not found by name, and a project that uses it finds it
missing, so it never loads.
The `plugin.*` commands run scans and choose the added folders (ADR 0002 amendment); the
folders, the catalog, the plug-ins switched off and the hosting choices are kept as JSON in
Suhina's settings folder.

## Editors

open from the host process, by default floating above Suhina's window and
owned by it, as most hosts do: the window stays above Suhina, has no taskbar entry of its
own and is minimised with it. Owning works on every system and keeps the editor apart from Suhina's
own layout; embedding another process's window works only on X11 and Windows. The engine
names its window in `open_editor` (`owner`: a Win32 `HWND`, an X11 window id); the host makes the editor's window owned by it (Win32 owner window, X11
`WM_TRANSIENT_FOR`). The window's title bar names the track as the views show it and the plug-in,
such as `2 Bass – Serum 2`, or only the track when its name already holds the plug-in's, as in
`2 Serum 2`. It says so from the moment it opens and follows a rename or a move of the track.
Embedding is offered as a choice on top
of that, Options › Settings › Plug-ins › Editors: *Window* (the default) or *Device view*,
where the editor shows inside the device panel as part of the chain, as the wrapper
devices some users buy do by hand for one plug-in at a time. With *Device view* the engine
asks for the editor `embedded`, the host opens it over Suhina's
window (X11 a child of the window id; on Windows a frameless window that Suhina's `HWND`
owns, placed in screen pixels and moved with that window, because Suhina's window draws
through DirectComposition for its see-through backgrounds and that covers its child
windows: an editor made a `WS_CHILD` stayed invisible behind it; made per-monitor DPI
aware, as Suhina's window is) and hidden, answers its size, and the window lays the panel out
around it (the device as wide as the editor, the panel as tall) and says each frame where
it goes (`place_editor`: the editor's origin and the panel's visible area in the window's
pixels; the host keeps the editor inside a *clip* window of that area, so a scrolled or
cut-off editor never paints over the rest of Suhina, and an empty area hides it, as when
another track is shown or Settings is open). The plug-in may still resize its
editor (`editor_resized`, unasked); the user cannot drag an embedded one. A floating
editor is never resized by the panel. Changing the setting closes the open editors and
opens them again the other way; their state is in the plug-in, so nothing is lost. The
choice is shown but does nothing where the window is not a Win32 or X11 one. **macOS:**
the host's process cannot put a view inside another process's window, so an embedded
editor cannot open there: the host reports it and the rack falls back once to a window of
the editor's own, whatever the setting says. Without an X server (`DISPLAY` unset) Linux
cannot open its X11 windows either.
An owned or child window shares its input queue with the owner's thread on Windows, so
the host making one waits for Suhina's window thread; the session therefore opens and
closes an editor on a thread of its own and the command answers at once (`editor_open:
"opening"`), or both would wait for each other. More generally the window thread never
waits for a host process: `Plugin::health`, `editor_open` and `editor_size` answer from
what they last knew while the process is busy (a sibling loading in a shared process takes
seconds), `place_editor` is sent only when the process is free (and again at the next sync
when it was not) and is never answered, and reading a plug-in's state now and then, and
stopping it, run on threads of their own. A host whose window thread hangs can be ended
like any other (ADR 0006 › 4). The host's main loop waits for its windows' messages
(`MsgWaitForMultipleObjectsEx`, woken by the control messages' thread) rather than looking
every 10 ms, which made some editors lag; on X11 it looks every millisecond while
an editor is open.
*Linux:* the desktop app opens its window as an X11 one whenever an X server is there
(`DISPLAY` is set: the desktop's own X11, or XWayland, which every distribution's Wayland
desktop ships), through winit's `with_x11`, as other DAWs do: the hosts open editors as X11
windows, and only an X11 window of Suhina's can own or embed them. Without `DISPLAY` the
window is a Wayland one and plug-in editors cannot open (the host's windows are X11 ones).
**On Windows:** the host's main thread initialises OLE first (`OleInitialize`, and
so COM, apartment threaded), as a host's window thread is expected to: editors use it for
drag and drop, the clipboard and text services, and some failed to open in `attached`
without it (`crates/plugin-host/tests/manual.rs` opens one installed plug-in's editor by
hand). The host opens a top-level window of its own on
its main thread (`crates/plugin-host/src/window.rs`: Win32 here, `x11rb` on Linux), or for an embedded editor a
clip and a view window as children of Suhina's, and attaches the plug-in's view to it (VST3 `IPlugView`
with an `IPlugFrame` for size requests; CLAP's `gui` extension, embedded), pumps its
messages in the main loop between control messages, and tells the engine unasked when the
user closes it (`editor_closed`) or the plug-in resizes it
(`editor_resized`). Checked with two VST3 effects; no
CLAP plug-in with an editor was at hand.
**On Linux (X11):** the host's window comes from `x11rb` (an embedded editor's clip and
view are children of Suhina's window id), the plug-in's view is embedded by window id
(VST3 `X11EmbedWindowID`, CLAP `x11`), and the host serves the plug-in's own event loop
from its main loop (`crates/plugin-host/src/run_loop.rs`): VST3's `IRunLoop` on the
editor's frame and CLAP's `posix-fd-support` give it the descriptors it waits on (polled
without waiting each turn) and timers; CLAP's `timer-support` serves timers on every system.
Tested in CI under Xvfb with the VST3 test plug-in, floating and embedded in a window the
test makes; no real Linux plug-in has been tried yet. macOS: floating editors open (the
test VST3 through the engine's commands, a real plug-in by hand); Wayland comes later.

## Plug-ins inside device groups

A plug-in inside a group still runs out of
process; a group is not a process boundary. Its key counts "how many of the same plug-in
come before it" depth-first through the channel's whole tree of devices, so moving it into or
out of a group keeps its process and state while the order of identical plug-ins stays.
Inside a branch it runs its round trip and is waited for within the block's deadline, one
after another with the branch's other devices; it does not take part in the joint blocks or
the staged runs of 4, which a later change can extend to a group's branches. A missed block
does what it does on a track: an effect passes its input, an instrument is silent, and the
rest of the group goes on. Its state is read and saved, and it restarts, by its key like any
other plug-in.

## In the engine

The engine never sees `plugins`: a plug-in device's slot in a
track's chain holds a `PluginProcessor` (`crates/engine/src/plugin.rs`), which the session
implements over `PluginAudio` (`Hosted`, `crates/session/src/rack.rs`). A slot runs once per
block with the block's notes at their frames (VST3 `inputEvents`, CLAP note events; an
all-notes-off becomes the sustain pedal and controller 123), so the chain runs its devices
one after another over the whole block rather than piece by piece. The block's plug-ins wait
together until 60 % of the block's playing time has passed (ADR 0006 › 4); a slot that misses
leaves the sound as it was (an effect) or silent (an instrument) and counts the miss, and the
next block goes on. A block that is not sent at all (the host still busy with the one
before) keeps its releases (note-offs, the sustain pedal, all notes off) for the next block
that is, so no note is left hanging; stopping releases each held key on its own before the
all-notes-off, which many plug-ins ignore. Offline (`Renderer::set_offline`, every
`render_*` function, the CLI's and MCP's WAV renders) each block waits up to 10 s for a
host still busy with the block before, and up to 10 s for its own answer, through the same
round trips as playback, so no block of a render is lost to a slow plug-in.
Each plug-in device has a key made from its channel, its plug-in and how many of the same
plug-in come before it on that channel, so it keeps its running plug-in when devices are
added, removed or moved around it; its processor reaches the audio thread as a message
(`Controller::send_plugin`), waits there for a graph that has its slot, and is handed from
graph to graph; a restarted plug-in's processor replaces the crashed one, which goes back to
be dropped off the audio thread.

## Projects

While a project plays, the session finds each plug-in device in the catalog
(`plugin.check`'s matching), starts it with the saved state (and VST3 controller state) on a
thread of its own, and hands it over; a project's plug-ins stop when it stops playing (a
tab switch) and each keeps its last state for when it starts again, as one whose device is
removed does for an undo. A plug-in that is not installed is *missing*: no slot, the sound
passes, its state stays in the project untouched and is saved back as it was; the status bar
names it. Every 30 s the session reads each running plug-in's state, so a restart after a
crash gets back what was set in its editor (ADR 0006 › 5); saving the project
(`project.save`) reads each one's state then and writes it into the project, so a reopened
project, or a plug-in started again from scratch, gets the same settings. Those reads also
tell when a setting was changed in the editor since the project was saved, which makes the
project unsaved without an undo step ([ADR 0010](../decisions/0010-project-files.md), decision 7). Crashes and hangs are noticed within 250 ms and shown on the
device; `device.restart` starts it again (one that could not start at all is started from
scratch with the project's state).

## Latency

Latency is compensated: every track is heard as late as the one whose devices (built-in
and plug-ins, as each reports) delay it most, up to 2 s, through delay lines made with the
graph. A plug-in's latency is known once it has started, which builds a new graph. The
playhead shows what the engine renders, so it runs ahead of what is heard by that delay.

Latency that changes while a plug-in runs (a lookahead the user sets; #110) is followed.
A CLAP plug-in asks for a restart (`request_restart`, the only way its latency may change
while it is active) or says `latency.changed`; a VST3 plug-in calls `restartComponent` with
`kLatencyChanged`. The host's main loop then stops the instance's audio thread (a VST3
processor first gets the parameter values still waiting, in a block of no samples, so it has
the value that changed its latency), deactivates and activates the plug-in as before, starts a
new audio thread on the same link and reads the latency again; when it differs, the host sends
`latency_changed` unasked. The engine side takes it in without waiting
(`Plugin::follow_latency`), and the session builds a new graph, as when a plug-in starts, so
the tracks are lined up again. Blocks that come during the restart (milliseconds) go
unanswered, as a late block does. An offline render started with its plug-ins keeps the
latencies they had when they started.

## Keeping instances across an output change

A renderer returning its processors marks them as carried. Before the replacement renderer
chases the current notes, its instrument chains queue explicit note-offs for all 128 keys,
then AllNotesOff for those retained processors. Explicit releases are necessary for instruments
that ignore controller 123, including VST3 instruments without its MIDI mapping.
The old renderer's note-off schedule no longer exists; keeping its voices while chasing notes
again would stack voices on every device or engine-block change. This does not reload presets,
restart host processes or reset effect tails. An ordinary graph edit keeps its existing note
schedule and does not use this output-replacement flag.

Protocol 14 adds `reconfigure {instance, sample_rate}` and
`reconfigured {instance, latency}`. The engine must quiesce the instance's audio
link before requesting a change. Its shared-memory maximum block capacity is
unchanged; engine block-size changes within that capacity require no new link.
Rates must be finite, positive and at most 768000 Hz.

The host stops and joins the instance's audio worker with the existing bounded
stop wait, deactivates the same component, changes its processing configuration,
and activates it again. CLAP uses its activate configuration; VST3 calls
setupProcessing while inactive on the host main thread. The editor and component
are retained, and the reply carries the newly reported latency. The caller must
not resume audio until the acknowledgement and its latency update are applied.
Failure is explicit and requires recovery; it must not be treated as a successful
rate change. Same-rate requests leave an already configured instance alone.

The output owner applies this transaction to retained processors before building
its new renderer, updating each slot's latency. The session acknowledges the
retained rate before its next rack sync; otherwise that sync would unload all
instances again. Starts still in flight are retired and restarted at the new
rate because they were not part of the returned renderer. A failed transaction
aborts opening the new renderer and reports the error instead of playing a graph
containing mixed processing rates. A later crash restart reads the acknowledged
rate shared with the control half.

## Stopping a hosted instrument

Some instruments reset their output immediately when the transport's playing flag
becomes false, even when ordinary note-off messages alone allow a smooth release.
The hosted instrument therefore fades its continuing output to zero over 5 ms
using a smooth polynomial gain envelope before delivering the stopped transport.
Its position continues during that short fade; Stop's return to the start marker
is deferred along with the flag. This is per instrument, before downstream effects
and delay compensation, so those effects can keep their tails. Instrument releases
are limited to this stop fade. A Play during the fade reverses it from its current
gain; a live note while paused reopens the gate at that note's frame offset.
Live notes arriving during the short fade wait, in order, until the first stopped
block, so that the delayed transport transition cannot reset their new voices.
This bounded queue does not allocate while rendering; overflow ends with a release.

While its gate changes or is closed, an instrument processes separately from
joint followers, so its envelope cannot mute downstream effects. Ordinary playback
keeps joint processing. The renderer does not add the older held-sample output
ramp when its instruments provide this soft-stop contract. Both offline and live
hosting use the same gate. This introduces no delay during ordinary playback.

## Host processes without a window

Host processes and scan workers start without a window of their own (`host_command`; on
Windows `CREATE_NO_WINDOW`), so the desktop app, which has no console, does not open a
console window for each; plug-in editors still open their windows.

## Measurements

These justify ADR 0006 › 2 and 4.

- Measured on Linux (4 cores, a container, the host at normal priority; test
`measure_joint_round_trips`, release build, four `test:gain` instances in one process, blocks
paced as in playback): at 64 frames four round trips took 350 to 365 µs a block on average
(99 %: 0.68 to 0.85 ms) and one joint round trip 120 to 140 µs (99 %: 0.31 to 0.54 ms); at
128 frames 395 to 450 µs (99 %: 1.06 to 1.77 ms) against 140 to 165 µs (99 %: 0.32 to
0.47 ms). The saving grows with the number of plug-ins, as each one left out of the chain of
round trips is a wake-up of each side less.
- Measured on Linux (4 cores, a container, hosts at normal priority; test
`measure_chains_waiting_for_their_own_plug_ins` in `crates/plugin-host/tests/chain_wait.rs`,
release build, three chains of three `test:busy` plug-ins each in its own process, one heavy
plug-in of 3 ms a block at another place in each chain, the others 0.3 ms, 1024 frames at
48 kHz, a deadline of 12.8 ms, three runs): a block took 10.2 to 10.4 ms on average a stage
at a time (99 %: 11.6 to 12.9 ms) and 5.1 to 5.3 ms with each chain going on as its own
plug-in answers (99 %: 9.6 to 10.0 ms), #151.
- Measured on Linux (4 cores, a container; test `measure_quick_play_and_stop` in
`crates/plugin-host/tests/quick_play_stop.rs`, release build, `test:burst` instruments that
take a set time over each block bringing notes, play and stop pressed every 4 blocks, 2000
blocks of 256 frames at 48 kHz, a deadline of 3.2 ms), #506: an instrument taking 6 ms over
a press (more than a whole block) was late with 999 blocks for 500 presses, the late block
and the next, refused; once a plug-in late with one block is waited for to take the next,
501. One taking 4 ms (past the deadline, within the block) loses its late block either way
(504 and 499); 2.5 ms and less loses next to none. With four instruments on four tracks,
one taking 6 ms and the others 1 ms or less, the others were late with 4 to 8 blocks of
2000, about as with 1 ms each on their own.
- Measured with `ipc` on Windows (test `round_trips_with_another_process`, 128 frames, the peer
at normal priority): blocks back to back take 0.7 µs per round trip, because the peer is still
spinning; with 2.5 ms between blocks, as in playback, 19 µs on average and 0.8 ms at worst,
the worst case being why the host's audio thread needs real-time priority.

## How the work went

Work goes in this order: this ADR; `ipc` with a host that crashes and hangs on purpose; CLAP;
a CLI command that runs a WAV through a plug-in; VST3; finding the replacements for a
project's VST2 plug-ins (done), then loading them with their state; plug-ins in the
engine's track chain together with built-in effects and instruments (LOOP-35, done, #100); editors (Windows
and X11 done, floating and embedded in the device panel).
