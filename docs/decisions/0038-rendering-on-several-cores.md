---
description: How the engine renders on several cores - one worker pool per process, late work waited for so output stays bit-identical, a Live mode where every track is live and a Studio mode where leaf tracks' device chains render ahead while the mixer stays live, which tracks are live, switching paths, platforms and the unsafe exception, and the acceptance limits
read_when: multicore, several cores, worker pool, worker threads, render threads, SUHINA_RENDER_THREADS, parallel rendering, render ahead, Live mode, Studio mode, live track, live path, lead, headroom, late block, missed block, plug-in chains in parallel, audio workgroup, MMCSS, FTZ, denormals, bit-identical, acceptance limits
type: decision
status: accepted
built: no
last_verified: 2026-11-17
covers:
  - crates/output/src/lib.rs
  - crates/engine/src/sends.rs
  - crates/engine/src/lib.rs
  - crates/realtime/**
---

# 0038: Rendering on several cores, live and ahead

- **Date:** 2026-11-17
- **Issue:** [#1505](https://github.com/suhina-daw/suhina-daw/issues/1505), part of the epic
  [#1504](https://github.com/suhina-daw/suhina-daw/issues/1504), which holds the phases
- **Amends, when the phases land:** [ADR 0005](0005-edits-while-playing.md) › 3,
  [ADR 0006](0006-plug-in-hosting.md) › 4 and 6, [ADR 0017](0017-inputs-and-latency.md) › 6

## Context

One real-time thread renders the whole mix. The render thread in `crates/output` calls
`Renderer::process` ahead of the device into a lock-free ring and keeps four blocks queued, or a
device buffer and a block when that is more ([ADR 0017](0017-inputs-and-latency.md) › Amendment:
playback reserve); while a track is armed or monitors its input, the whole mix keeps one block
queued ([ADR 0017](0017-inputs-and-latency.md) › 6). `sends::run` runs the chains in dependency
order; chains that nothing plays into run their plug-ins at the same time
([ADR 0006](0006-plug-in-hosting.md) › 4), but a chain runs its own devices and plug-ins one after
another, each plug-in with its own round trip, all on the render thread. A plug-in that misses
its block's deadline is replaced for that block by its input or by silence, which is heard.

Two measurements frame the problem:

- **A prototype** ([#1447](https://github.com/suhina-daw/suhina-daw/pull/1447), not for merge)
  moved chains without plug-ins to worker threads by value through `rtrb` queues, with a spare
  chain in their place, and summed them in chain order. Three tracks of amp modelling rendered
  1.48 times faster on 2 threads and 2.4 times faster on 4 (the most three chains allow is 3),
  with the same WAV file, by its hash, on 1, 2 and 4 threads; total processor time grew about
  6 %.
- **A plug-in-heavy project** (an imported project of 77 tracks, ASIO at 44.1 kHz): at a
  1024-frame buffer about 130 plug-in blocks a second came back late while it played, at about
  52 % engine load and with no device underrun; most of them came from a linear-phase EQ with
  5120 frames of latency on a synth track. At 2048 frames only a handful were late. Each
  instance already has an audio thread of its own in its host process; what holds the chain up
  is that its plug-ins run in series on the one render thread.

Hosts solve this in two ways. **Hosts that render everything live** split the work by track or
by part of the signal path across cores, and controls answer at once, but a long chain on one
track is bound by one core, and its length sets the buffer the project needs. **Hosts that
render ahead** run most tracks a fixed time ahead (from about 20 ms to about 300 ms) in the
background and only a few tracks live (armed and monitored ones, a selected track, an open
device window, depending on the host); heavy mixes play, but everything on a track rendered
ahead answers late, the fader included, and switching a track between the two paths has been
reported to drop out or stutter when its latency compensation changes. An open-source engine
that renders in the device's callback, with no queue, drops a track that is not done at the
deadline for that block.

How late is too late: playing an instrument tolerates about 10 ms of delay and about 1 ms of
jitter (Wessel and Wright 2002); in-ear monitoring about 5 ms; continuous control 20 to 30 ms
(Mäki-Patola and Hämäläinen 2004); one note moved in time is noticed at about 6 ms (Friberg and
Sundberg 1995).

No host promises that a render on several threads equals the render on one, bit for bit.
Suhina's offline renders are its test oracle (AGENTS.md rule 4), so it has to.

## Decision

1. **One real-time worker pool per process, and one for exports.** Every renderer that plays
   to a device takes its workers from one real-time pool: the player of the active project
   ([ADR 0007](0007-several-open-projects.md) › 4). Offline renders and exports take theirs from
   a second pool whose workers stay at normal priority: an export has no deadline, so the
   system preempts its workers whenever a real-time thread needs a core, and an export beside
   playback takes only the time playback leaves. Ordering jobs could not do this: a worker
   cannot leave a chain's block it has started, so playback would wait for the export chains
   already running, one on every worker at worst. Each pool's size is the physical performance
   cores less one, at most 8, counted where the system says which cores are which, and the
   physical cores less one where it does not; one core means no workers.
   `SUHINA_RENDER_THREADS=n` and an advanced setting in Settings › Audio override it, for
   diagnosis. The render thread works too: it takes jobs while it waits. A block with fewer
   chains to run than a threshold (4 to start with, set by measurement in phase 1) runs them on
   the render thread, since waking workers would cost more than it saves. More real-time pools
   would compete for the same cores and the same real-time thread allowance (rtkit allows 25
   real-time threads per user, the plug-in hosts' threads among them); the export pool's
   workers take none of it.
2. **A chain moves by value and nothing is allocated.** As in the prototype, a job is a whole
   chain sent to a worker through an `rtrb` queue and sent back, with a spare chain in its place
   meanwhile, so the render thread allocates and frees nothing (AGENTS.md rule 3). The order is
   `sends::run`'s; a chain's output is summed in chain order, whichever finished first, so the
   result is the same bit for bit on any number of threads. Tests run under a counting allocator
   that fails on any allocation on the render thread or a worker.
3. **Late work is waited for.** When the block's sum needs a chain a worker has not finished, the
   render thread waits for it: it spins briefly, then sleeps on a wake-up the worker signals. It
   never gives its core away by yielding: on Windows a yield can hand the core to a thread of
   normal priority for a whole scheduler quantum. Playback never stops for it. A wait that eats
   the queue shows as a device underrun, counted as today: the device plays silence for the
   frames that were not there and then the frames as they come, nothing dropped. A chain that
   finished after its
   block's share of time is counted as a *missed block* and named by track. Dropping a late chain
   for the block, as an engine with no queue has to, is rejected: it cuts a gap into the track
   and skips its devices' state by a block, and a render on several threads would no longer
   equal the render on one. With a queue ahead of the device, a short wait costs nothing heard.
   What is bit-identical is a render on any number of threads against one on one thread; a
   render in real time equals an offline one only while no plug-in misses its deadline, since
   a late plug-in is replaced for its block ([ADR 0006](0006-plug-in-hosting.md) › 4) and offline
   it is waited for.
4. **Plug-in chains go to the pool too** (phase 2). A chain with plug-in slots is a job like any
   other: its round trips run on the worker, and chains overlap their round trips with each
   other and with built-in work. Inside a chain, parts that do not depend on each other (the
   branches of a device group) overlap their round trips as well. Plug-ins run sandboxed, so a
   plug-in has no thread of the engine to be bound to. A chain rendered ahead (6) waits for its
   plug-ins until its output is needed, its lead, instead of the one block of
   [ADR 0006](0006-plug-in-hosting.md) › 4; a live chain keeps that deadline.
5. **Two modes: Live and Studio.** In **Live mode** every track is live: rendered just before it
   plays, as now, with its chains spread over the pool, and every control answers at once. In
   **Studio mode** the leaf tracks that are not live (8) are rendered ahead by the lead (7), so a
   heavy chain has the lead's time instead of one block's, while the mixer stays live (6). The
   mode is a setting of the app, not of a project, set through a command (AGENTS.md rule 6) and
   reached from the transport bar while playing. Live mode is the default until the limits (17)
   pass on Windows, macOS and Linux; then Studio mode becomes the default (phase 5).
6. **Only leaf tracks' device chains before the fader are rendered ahead.** Fader, pan, mute,
   solo and send levels, and every summing point (groups, returns and Main, with their chains),
   are computed live when the block plays, so the mixer answers at once in both modes; the
   summing points run on the pool too. A group is not rendered ahead even when all its children
   are: in the measured project the heavy chain was a leaf track's. If the measuring tools of
   [#1512](https://github.com/suhina-daw/suhina-daw/issues/1512) show group or return chains to
   be the bottleneck in real projects, an amendment may let a group whose children are all
   rendered ahead, and whose own controls are not being moved, render ahead too.
7. **The lead.** In Live mode the lead stays what it is now: four blocks, or a device buffer and
   a block. In Studio mode it is 50, 100 or 200 ms, rounded up to whole blocks and never shorter
   than Live mode's. The default is chosen in phase 5 by measurement: the shortest of the three
   at which the plug-in-heavy row of the limits (17) passes. A longer lead costs memory only
   (77 stereo tracks at 200 ms and 44.1 kHz are about 5.4 MB of samples).
8. **Which tracks are live in Studio mode.** A track is live, and so is every track it feeds,
   while:
   - it is armed and monitored, or monitors its input with In ([ADR 0017](0017-inputs-and-latency.md) › 3);
   - it is selected; the detail area, docked and always visible, shows the selected track's
     devices, so an open device panel is a selected track;
   - an editor window of one of its plug-ins is open, floating or embedded;
   - a parameter or macro of it is mapped to a controller ([ADR 0019](0019-midi-controllers-and-mapping.md)),
     for as long as the mapping exists, so a controller's first message never waits for the lead;
   - automation is being written to it;
   - a launcher clip on it starts unquantized, or with a quantization shorter than the lead;
   - the user marks it *always live*, or one of its devices is marked *always real time*.

   A track marked *may render ahead* is left out of the automatic rules except arming and
   monitoring. Mapped tracks count even though a live set maps many: the cost is only that those
   tracks get one block, as in Live mode.
9. **Arming shortens only the live path.** In Studio mode the render thread keeps one block
   queued for the live path, and the tracks rendered ahead keep their lead, so a heavy mix no
   longer meets a one-block deadline while recording
   ([#1510](https://github.com/suhina-daw/suhina-daw/issues/1510)). In Live mode arming shortens
   the queue as [ADR 0017](0017-inputs-and-latency.md) › 6 says.
10. **Switching paths keeps the sound continuous.** A track going live stops rendering ahead,
    plays out what is queued, and goes on live from the next sample not rendered, so its devices
    see an unbroken stream; going back, it builds its lead up again by rendering more than one
    block per block while there is time. Its latency compensation stays the same across the
    switch, since a change in it is what makes a switch drop out or stutter. A seam that is not
    continuous (a stop, a jump) is declicked. Playing out the queue makes an action that needs
    a track live at once (arming it, an unquantized launch) wait up to the lead in Studio mode;
    Live mode has no queue to play out. This decision is checked by the prototype of
    [#1508](https://github.com/suhina-daw/suhina-daw/issues/1508) against the limits before the
    full two paths are built ([#1509](https://github.com/suhina-daw/suhina-daw/issues/1509)),
    among them the unquantized launch's 10 ms; what it changes comes as an amendment.
11. **Launcher, transport and edits.** Quantized launches are scheduled ahead and land on their
    sample on both paths. Stop and jump empty the queues and render from the new position. An
    edit's new graph ([ADR 0005](0005-edits-while-playing.md) › 3) takes over on each path at its
    own render position: on a live track at the frame chosen as now, on a track rendered ahead a
    lead later; a device parameter message likewise; a mixer message always at the play
    position.
12. **Meters tell the two paths apart.** The engine reports the live path's critical path (the
    wall time a block took over the block's time, with a peak hold), the load of rendering ahead
    apart from it, and missed blocks per track. Level meters and the spectrum read from the play
    position, not the render position, so they move with what is heard.
13. **Worker threads get the render thread's priority.** On Windows the workers join MMCSS
    *Pro Audio* through `suhina-realtime`, as the render thread does, and run at normal priority
    where it refuses. On Linux they take `SCHED_FIFO` or rtkit as the render thread does, within
    rtkit's thread allowance, with the same pacing under `RLIMIT_RTTIME`. On macOS the render
    thread and the workers join one audio workgroup the engine creates
    (`AudioWorkIntervalCreate`), since the render thread runs ahead of the device and has no
    device workgroup to join; without one, the scheduler may move workers to efficiency cores.
    The details are [#1511](https://github.com/suhina-daw/suhina-daw/issues/1511).
14. **`crates/realtime` may use `unsafe` for this.** The workgroup and the floating-point mode
    (15) are calls into the system with no safe binding: the Rust bindings available
    (`objc2-audio-toolbox`, `objc2-core-audio`, `dispatch2`) carry the workgroup's property
    constants but not `os_workgroup_join`. `crates/realtime` joins the crates allowed `unsafe`
    ([ADR 0006](0006-plug-in-hosting.md) › 6) on the same terms: small blocks, each with a
    `// SAFETY:` comment (`clippy::undocumented_unsafe_blocks` denied), behind a safe API, tested.
    The change that first needs it amends ADR 0006 › 6 and AGENTS.md rule 2.
15. **Every engine thread runs in the same floating-point mode.** Subnormals flush to zero on the
    render thread, the workers and the threads of offline renders, as they already do on the
    plug-in hosts' audio threads, so a chain gives the same result on any thread and never slows
    down on subnormal numbers. Switching the engine's threads to it is a change of its own, which
    blesses the golden renders again once.
16. **One render path.** An offline render renders every track ahead, without a deadline, with
    the same code. CI runs the engine tests three ways: every track live, every track rendered
    ahead, and on 4 threads; and a test switches a track between the paths in the middle of an
    offline render and gets the same output, bit for bit.
17. **The acceptance limits.**

    | Scenario | Buffer | Measure | Limit |
    |---|---|---|---|
    | Playing through Suhina | 64 at 48 kHz | round-trip latency with a loopback | at most 10 ms; Suhina adds nothing beyond two buffers and the plug-ins' reported latency |
    | Playing | 64 | MIDI in to sound out, spread | at most 1 ms (p99) |
    | Playing | 64 | dropouts in 30 minutes at a load stated with the result (the project, its tracks and its engine load) | 0 |
    | Recording with in-ear monitoring | 32-64 | round-trip latency | at most 5 ms |
    | Live set with controller moves | 128-256 | knob turn to first changed sample, any track | target 10 ms, ceiling 30 ms |
    | Live set | 128-256 | quantized launch position, drift between tracks against an offline render | 0 samples |
    | Live set | 128-256 | unquantized launch: delay / spread | at most 10 ms / 1 ms |
    | Heavy production | 256-1024 | instances of a fixed built-in device without dropouts; scaling T(n)/(n·T(1)); offline bit-identical | a published curve against one thread on the same machine |
    | Plug-in-heavy production | 1024 | the 77-track project of the context: late plug-in blocks while it plays | 0 |

    Each is measured on Windows, macOS and Linux, recording the processor and its split of
    performance and efficiency cores, the power plan, the audio interface and driver and the
    sample rate, on a warmed-up machine; the median and the worst of N runs are reported, with
    the callback's time against its period at p50, p99 and maximum. The tools are
    [#1512](https://github.com/suhina-daw/suhina-daw/issues/1512).

## Consequences

- The amendments named at the top are written in the changes that build what they describe:
  ADR 0005 › 3 and ADR 0017 › 6 with the two paths (#1509, #1510), ADR 0006 › 4 with plug-in
  chains on the pool (#1507), ADR 0006 › 6 and AGENTS.md rule 2 with the workgroup (#1511).
- Phase 1 (#1506) builds decisions 1 to 3, 12 (as far as one path goes) and 16 from `main`, off
  by default; the prototype's branch gives its pool and its bit-identical test, and is not
  rebased, since it predates the plug-in deadline measured from the device's take and the
  four-channel plug-in link.
- In Studio mode a device parameter on a track that is not live answers a lead later, up to
  200 ms; everything a user is touching (8) and the whole mixer (6) answer at once.
- A plug-in that adds a lot of latency still delays the tracks played through it, in both
  modes; bypassing such plug-ins on the path being played is a separate feature, not part of
  this decision.
- Total processor time grows a little (about 6 % in the prototype) for a shorter wall time.
- An export beside playback still shares the cores with the plug-in hosts' audio threads, which
  run at real-time priority whichever renderer sends them a block (ADR 0006 › 4): its plug-ins
  compete with playback's, and measurements (#1512) say whether an export needs slowing down
  while playback runs.

## Alternatives

- **Exports on the real-time pool, real-time work first, or some workers kept for playback.**
  Ordering cannot take a worker back from a chain it has started, and a reservation either
  leaves cores idle or guesses playback's need; the system's scheduler preempts a
  normal-priority export at once (1).
- **Drop late work at the deadline.** Needed by an engine with no queue; rejected (3).
- **Render everything ahead, the mixer included.** Simpler, but the fader and every send would
  answer a lead late; rejected (6).
- **One live thread for the live path.** A known bottleneck when many tracks are live; the live
  path runs on the pool (5).
- **Mapped tracks live from the controller's first message.** That message would wait up to the
  lead; rejected (8).
- **Studio mode only, no Live mode.** A live set would depend on the live rules guessing right;
  Live mode is the guarantee that nothing waits.
- **Rendering in the device's callback** to save the last queued block: rejected in
  [ADR 0017](0017-inputs-and-latency.md) › Consequences, and it would leave no room to wait (3).

## Sources

- Wessel and Wright, problems and prospects for intimate musical control of computers (2002): https://nime.org/proc/nime2001_wessel/
- McPherson, Jack and Moro, action-sound latency (2016): https://www.nime.org/proceedings/2016/nime2016_paper0005.pdf
- Buttazzo, Bertogna and Yao, limited preemptive scheduling for real-time systems, a survey (2013; blocking bounded by the longest region that cannot be preempted): https://iris.santannapisa.it/retrieve/dd9e0b31-c093-709e-e053-3705fe0a83fd/TII-13-Survey.pdf
- Apple, understanding audio workgroups (only real-time threads join one): https://developer.apple.com/documentation/audiotoolbox/understanding-audio-workgroups
- Apple, adding asynchronous real-time threads to audio workgroups: https://developer.apple.com/documentation/audiotoolbox/adding-asynchronous-real-time-threads-to-audio-workgroups
- rtkit's limits: https://github.com/heftig/rtkit/blob/master/rtkit-daemon.c
- JACK2 activation counters: https://github.com/jackaudio/jack2/blob/develop/common/JackActivationCount.cpp
