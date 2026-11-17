---
description: Render threads - the pool of worker threads that renders chains beside the render thread, the setting and command that choose its size, lanes per renderer, the bit-identical sum, waiting for late work, missed blocks per track and what is not built yet
read_when: render threads, worker pool, render workers, SUHINA_RENDER_THREADS, audio.setRenderThreads, several cores, multicore, parallel rendering, missed block, render blocks late, bit-identical, lane
type: spec
status: draft
last_verified: 2026-11-17
covers:
  - crates/engine/src/pool.rs
  - crates/engine/src/pool_tests.rs
  - crates/engine/src/sends.rs
  - crates/engine/tests/no_allocation.rs
  - crates/session/src/audio.rs
---

# Render threads

The decisions are [ADR 0038](../decisions/0038-rendering-on-several-cores.md); this is what is
built of them. Phase 1 of [#1504](https://github.com/suhina-daw/suhina-daw/issues/1504): the
pool at the current lead, off by default, for chains without plug-ins.

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

- **One pool per process.** `RenderPool` starts its workers once. Each renderer takes a lane of
  it when it is built: a pair of lock-free rings (`rtrb`) to and from every worker, made with
  the pool, so each ring has one writer and one reader. A pool has 16 lanes; a renderer that
  finds none free renders alone. A renderer whose output plays to a device marks its lane
  real-time (`Renderer::join_real_time`): a worker takes the real-time lanes' chains before an
  export's, and the workers join real-time priority with the render thread. Changing the
  threads makes a new pool; renderers built before keep theirs until they are dropped.
- **What goes to a worker.** A chain with no plug-in slot and no ARA clip host, other than
  Main's, once the chains that play into it are done (`sends::run`'s order). A block with fewer
  than four such chains (`PARALLEL_MIN`) runs them all on the render thread. The render thread
  hands a chain to whichever of itself and the workers holds the fewest, itself on a tie, runs
  its own share one at a time and looks at the workers between them.
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
track (never offline, where there is no deadline); the worker judges it when it finishes, so a
render thread busy with its own share does not make a chain late that was done in time. The
count is the track's: an edit while playing carries it over to the new graph. The renderer publishes the total and
the track that missed most beside the plug-in misses. `audio.info` gives `render_threads`,
`render_misses`, `worst_render_track` and `worst_render_misses`; Settings › Audio › Status
shows *Render threads* ("4 threads; 3 blocks late, most on Bass (2)"), and the trace's `audio:`
line gives the late blocks, the track and the thread count.

## Not yet

- Chains with plug-ins on the workers, and their round trips overlapping (phase 2,
  [#1507](https://github.com/suhina-daw/suhina-daw/issues/1507)).
- Physical and performance cores for Automatic
  ([#1522](https://github.com/suhina-daw/suhina-daw/issues/1522)); the workers in a macOS audio
  workgroup and the thread allowance under rtkit
  ([#1511](https://github.com/suhina-daw/suhina-daw/issues/1511)).
- Flush-to-zero on every engine thread (ADR 0038 › 15), a change of its own that blesses the
  golden renders again ([#1521](https://github.com/suhina-daw/suhina-daw/issues/1521)).
- The allocation test counts the render thread only: the counter it uses counts one thread, so
  the workers' loop is covered by the same chain code on one thread.
- The threshold of four chains, measured ([#1512](https://github.com/suhina-daw/suhina-daw/issues/1512)).
- Rendering ahead, Live and Studio mode, and the live path's meters (phases 3 and 4).
- On by default (phase 5).
