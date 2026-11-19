---
description: Render threads - the pool of worker threads that renders chains beside the render thread, plug-in chains included, the setting and command that choose its size, lanes per renderer, the bit-identical sum, waiting for late work, missed blocks per track and what is not built yet
read_when: render threads, worker pool, render workers, SUHINA_RENDER_THREADS, audio.setRenderThreads, several cores, multicore, parallel rendering, missed block, render blocks late, bit-identical, lane, plug-in chains in parallel, device group branches
type: spec
status: draft
last_verified: 2026-11-18
covers:
  - crates/engine/src/pool.rs
  - crates/engine/src/pool_tests.rs
  - crates/engine/src/sends.rs
  - crates/engine/src/group.rs
  - crates/engine/tests/no_allocation.rs
  - crates/session/src/audio.rs
---

# Render threads

The decisions are [ADR 0038](../decisions/0038-rendering-on-several-cores.md); this is what is
built of them. Phases 1 and 2 of [#1504](https://github.com/suhina-daw/suhina-daw/issues/1504):
the pool at the current lead, off by default, for every chain but Main's, plug-in chains
included ([#1507](https://github.com/suhina-daw/suhina-daw/issues/1507)).

## Choosing the threads

- **Settings › Audio › Advanced › Render threads**: Off (the default), Automatic, 2, 4 or 8, the
  render thread included. Saved as `render_threads` (1 Off, 0 Automatic, otherwise the number).
- **`audio.setRenderThreads {threads}`**: 0 Automatic, 1 the render thread alone, 2 to 9 that
  many; anything else is refused. It opens the output in use again, so its renderer is built
  with the new pool, and answers with the threads chosen, the threads the process renders on
  and whether `SUHINA_RENDER_THREADS` decided it. Agents attached over MCP may not run it.
- **`SUHINA_RENDER_THREADS=n`**, when set, wins over both, for diagnosis.
- **Automatic** is the threads the system offers (`std::thread::available_parallelism`, logical
  cores), at most 9: the render thread and 8 workers.
- A choice `audio.setRenderThreads` refuses (an open ASIO control panel) puts the setting back to
  the one the engine runs with.

## How a block is rendered

- **Two pools per process.** One real-time pool for renderers that play to a device, one at
  normal priority for offline renders and exports (`PoolKind`); both have the threads chosen.
  A renderer is built with a lane of the real-time pool, and `Renderer::set_offline(true)`
  (every offline render and export calls it before its first block) trades it for a lane of
  the offline pool. The offline pool's workers (`suhina-offline-worker-N`) never join
  real-time priority, so the system preempts them whenever a real-time thread needs a core:
  an export beside playback takes only the time playback leaves, and playback never waits for
  an export's chain.
- **Lanes.** Each pool starts its workers once. A renderer's lane is a pair of lock-free rings
  (`rtrb`) to and from every worker of its pool, made with the pool, so each ring has one
  writer and one reader. A pool has 16 lanes; a renderer that finds none free renders alone.
  A renderer whose output plays to a device marks its lane real-time
  (`Renderer::join_real_time`), and the real-time pool's workers join real-time priority with
  the render thread. Changing the threads makes new pools; renderers built before keep theirs
  until they are dropped.
- **What goes to a worker.** Every chain but Main's (which runs once every other chain is done,
  so beside none), its plug-in slots and ARA clip host included, once the chains that play into
  it are done (`sends::run`'s order). A block with fewer than four such chains (`PARALLEL_MIN`)
  runs them all on the render thread. The render thread hands a chain to whichever of itself and
  the workers holds the fewest, itself on a tie, runs its own share one at a time and looks at
  the workers between them.
- **Plug-in chains on a worker** (ADR 0038 › 4). A worker holds up to eight chains at once
  (`HOLD`) and runs them as the render thread runs its own: each as far as it goes without
  waiting (`Chain::advance`), its next plug-in handed its block, and on as soon as that plug-in
  answers (`Chain::answered`, `Chain::collect`), while it waits for any of its chains' plug-ins
  together (`suhina_ipc::wait_any`), at most half a millisecond at a time (`JOB_LOOK`) so a chain
  sent to it meanwhile is taken soon. The chains' round trips so overlap one another and the
  built-in work of every thread, instead of the render thread taking each answer in turn between
  its own devices. A live chain keeps [ADR 0006](../decisions/0006-plug-in-hosting.md) › 4's
  one-block deadline: its worker waits for a plug-in only until the block's deadline (the chain's
  limit, from the block's start), and a late plug-in is replaced for its block by its input or by
  silence, as on the render thread. Offline every plug-in is waited for.
- **A group's branches overlap.** Inside a chain, on any thread, the branches of a device group
  hand their plug-ins the block at the same time: each branch runs as far as it goes without
  waiting, then the group waits for any branch's plug-in and runs that branch on, so a group
  takes as long as its slowest branch, not the sum of them (`group.rs`). Each branch keeps its own
  events, so the order the answers come in changes nothing it makes. A nested group yields its
  outstanding plug-ins to the same scheduler, through every enclosing group and the chain:
  neither its sibling branches nor the worker's other chains wait for it to finish. A branch
  resumes when its own input is ready; a device after a group waits for all of that group's
  branches. The scheduler watches the outstanding leaf plug-ins together, with the same bounded
  polling fallback when a watch is unavailable or the fixed watch buffer is full.
- **By value, without allocating.** A chain goes to its worker through the lane and comes back
  the same way, its devices run and its sound staged (`Chain::stage`); meanwhile one of the
  lane's spare chains, made when the lane was taken, holds its place. Nothing is allocated or
  freed on the render thread (`crates/engine/tests/no_allocation.rs`).
- **The same sum.** The render thread feeds every chain's input and adds what each staged in
  chain order, whichever thread ran it and whenever it finished, so a render on any number of
  threads is the same, bit for bit (`a_render_on_several_threads_is_the_render_on_one_bit_for_bit`,
  1, 2, 3, 4 and 8 threads). CI runs the engine's tests a second time with
  `SUHINA_RENDER_THREADS=4`.
- **Waiting.** The render thread waits for every chain it sent: it spins a moment, then sleeps
  until a worker wakes it (at most 2 ms, or a plug-in's share when one is out too). It never
  yields its core.

## Late blocks

A chain its worker finished after its block's deadline is counted as a missed block of its
track (never offline, where there is no deadline), unless one of its plug-ins missed that block:
that is counted once, as the plug-in's late block; the worker judges it when it finishes, so a
render thread busy with its own share does not make a chain late that was done in time. The
count is the track's: an edit while playing carries it over to the new graph. The renderer publishes the total and
the track that missed most beside the plug-in misses. `audio.info` gives `render_threads`,
`render_misses`, `worst_render_track` and `worst_render_misses`; Settings › Audio › Status
shows *Render threads* ("4 threads; 3 blocks late, most on Bass (2)"), and the trace's `audio:`
line gives the late blocks, the track and the thread count.

## Not yet

- Physical and performance cores for Automatic
  ([#1522](https://github.com/suhina-daw/suhina-daw/issues/1522)); the workers in a macOS audio
  workgroup and the thread allowance under rtkit
  ([#1511](https://github.com/suhina-daw/suhina-daw/issues/1511)).
- Flush-to-zero on every engine thread (ADR 0038 › 15), a change of its own that blesses the
  golden renders again ([#1521](https://github.com/suhina-daw/suhina-daw/issues/1521)).
- The allocation test counts the render thread only: the counter it uses counts one thread, so
  the workers' loop is covered by the same chain code on one thread.
- A joint round trip (several plug-ins of one host process in one block) stays inside a chain:
  plug-ins of one process on different chains, now on different threads, each take a round trip
  of their own, as before.
- The threshold of four chains, measured ([#1512](https://github.com/suhina-daw/suhina-daw/issues/1512)).
- Rendering ahead, Live and Studio mode, and the live path's meters (phases 3 and 4).
- On by default (phase 5).
