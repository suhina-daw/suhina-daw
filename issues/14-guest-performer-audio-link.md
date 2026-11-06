# A guest performer: streamed monitoring, a local take, and a delay quantised to the grid

**Type:** enhancement · **Area:** `audio-io`, `engine`, `ui` · **Phase:** M8 or later — **last of this epic** · **Priority:** P2

This is the one part of [the epic](https://github.com/suhina-daw/suhina-daw/blob/main/issue-collaboration-and-project-versions.md) with physics in it, and it is deliberately last. It is feasible, but only once an input path exists (issue 13), a protocol exists (issue 10) and a link exists (issue 11) — and it is the feature most likely to be built in the wrong order and to disappoint. The design below is what makes it a DAW feature rather than a jam toy.

**Depends on:** issue 13 (recording, inputs and monitoring — **nothing here is possible without it**), issue 10 (the protocol and the permission model), issue 11 (the transport) and issue 12 (the relay, for the pairs that need it). This issue should not be started until issue 13 has shipped.

## Goal

A guest performer plays on their own machine, hears themselves with no added latency, plays in time with the host's project, and the recording that ends up in the host's project is the guest's own uncompressed take — placed exactly where it was played.

## The three ideas that make it work

1. **Monitoring and recording are separate paths, and only one of them crosses the network.** The guest monitors themselves locally at zero latency; the network carries **only** a monitor stream, lossy and jitter-buffered. The take is recorded **on the guest's own machine**, uncompressed, and transferred afterwards as a blob (the manifest and content addressing of issue 03 already carry it). **The network therefore affects only what is heard, never what is kept** — which removes the jitter, the codec and the dropped packets from the result entirely.
2. **The measured delay becomes a per-path delay line.** Latency compensation is already capped at **2 seconds** (`MAX_LATENCY_SECONDS`, `crates/engine/src/lib.rs`) and is a per-path delay line built with the graph (`crates/engine/src/plugin.rs`). A network path is the same shape as a plug-in's latency, **four orders of magnitude larger** — so the alignment machinery exists and does not need inventing.
3. **Quantise the delay to a musical interval.** Do not try to hide a few tens of milliseconds; round the delay up to the next musical division so the guest plays against the grid rather than against a smear, and **rebuild the graph only when the interval changes** rather than on every jitter sample. That single decision makes the guest playable *and* keeps a jittery link from thrashing the graph. It is borrowed from the interval-based approach the established network-audio tools use, and it is the reason this issue can exist at all.

## Scope

1. **An ADR** for the audio link: what is sent, how the delay is measured, how it is quantised, how the graph learns about it, and what the guest hears. With alternatives considered and rejected: sending the take over the network in real time, recording the monitor stream at the host, and streaming the host's full mix at high quality.
2. **The monitor stream**, lossy and jitter-buffered, one direction, with the buffer as the latency/jitter knob — the shape the established tools use, and **worth learning from rather than inventing**: SonoBus is GPL-3.0 and JackTrip is MIT, so both may be ported with notices kept and credited in `ATTRIBUTION.md` under rule 9 (the clean-room rule concerns proprietary products, and neither is one).
3. **Latency measurement and its exposure.** The delay is measured, shown to the guest (they are the one who feels it), and turned into frames for the delay line. Bandwidth, stated: **uncompressed 48 kHz/24-bit stereo is about 2.3 Mbps per stream**, so one guest is cheap and several are not — which is also why the monitor stream is lossy and the take is not streamed.
4. **The take's journey.** Recorded locally, transferred as a blob (issue 03's manifest and content addressing), placed on the host's timeline at the offset the guest was playing against — **with the known constant delay subtracted exactly**, because it was constant and known, not guessed.
5. **Clock drift, named and handled.** Two machines' audio clocks differ by tens of ppm, so a stream needs rate adaptation or the buffer slowly drains. This is a real problem the ADR must address; it is not solved by the delay line.
6. **What the host hears while the guest plays, and the honest limit.** If the host also plays an instrument, aligning the guest means delaying the host's own monitoring by the same amount, which degrades the host's own performance. The published measurement on networked ensemble playing found **no latency cliff** — quality degrades smoothly with delay rather than falling apart — so "host produces, guest performs" is a defensible mode and "everyone plays tight" is not. **Say which modes are supported and let the ADR choose the default.**
7. Tests (AGENTS.md rule 7), including a degraded link, a dropped connection mid-take, and a guest whose take arrives late.

## Acceptance criteria

- [ ] The guest hears themselves with no added latency, and hears the host's project delayed only by the quantised interval.
- [ ] The guest's take is recorded on the guest's own machine, uncompressed, and is **byte-identical to what a local recording of the same performance would be** — the network's quality does not appear in it.
- [ ] The take arrives at the host and is placed so that it lines up with the project's grid; the alignment is measured, not asserted, and the measurement is in the issue.
- [ ] The delay is quantised to a musical interval, and the graph is rebuilt when the interval changes and **not** on every measurement — shown by counting graph rebuilds across a run with varying latency.
- [ ] A link whose delay is well inside the 2-second compensation cap lines up; a link past it is refused with a message rather than producing a wrong placement.
- [ ] Clock drift is handled (rate adaptation or an equivalent) and does not empty the buffer over a long take; the test runs long enough to show it.
- [ ] A dropped connection mid-take loses nothing on the guest's side, and the host's project is untouched and playing.
- [ ] The supported modes are stated in the spec, including the case where the host also plays, and the user is told what to expect rather than left to discover it.
- [ ] Bandwidth is measured and stated for one guest at the chosen settings, and compared against the 2.3 Mbps uncompressed figure.
- [ ] No new code is reachable from the audio callback (AGENTS.md rule 3).
- [ ] `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` pass.
- [ ] Documents updated: the ADR, a spec for the link's behaviour, `docs/architecture.md` if a thread or crate is added, `ROADMAP.md`, `SECURITY.md` if the link changes the control channel's scope, and `ATTRIBUTION.md` for anything ported.

- [ ] The pull request's description says **how to check the change** — numbered steps a
      non-musician can follow from a test build — or, when nothing a user can notice changed, one line
      saying so.
## Not in scope

- **Sub-20 ms ensemble playing between distant sites.** The epic excludes it as a separate product with a separate budget.
- **Video, chat, or presence beyond the transport.** No cursors or fader-sharing here; that is the shared-session half (issue 10) and is a command-stream feature.
- **Streaming more than one guest** in the first version, and streaming the host's full mix at high fidelity.
- **A bit-identical render across two machines.** Floating-point summation order, FMA and plug-in nondeterminism mean bytes can be guaranteed and a bit-identical render cannot; any render-equality test stays scoped to the built-in effects and instruments (the epic says so, and it applies here).
- **Making this work over a shared-session extension.** Collaboration is core, not an extension (ADR 0015 §2); and this is the part of it that would be hardest to get back if it were optional.

## Notes

- **Where the code goes** (`docs/architecture.md` › Where new code goes): the link lives beside the session protocol in the same new crate as issue 10; the delay line and its graph parameter are the **engine's**, built in the renderer's constructor (`crates/engine/src/plugin.rs` is the existing delay line for compensation). The guest's own recording is issue 13's work, used twice. `ui` shows the measurement and the mode.
- **What is genuinely already solved, and worth stating so nobody rebuilds it:** the alignment. SonoBus and JackTrip are standalone audio applications; they mix streams with a jitter buffer and everyone hears the same delayed thing — they do not have a DAW's alignment problem. **This project has real latency compensation built with the graph, capped at two seconds, and a graph swap at a block boundary (ADR 0005).** So the work is the streaming layer and the quantisation; the alignment is a graph parameter.
- **The honest sequencing note, which belongs in the issue because it is the likeliest thing to be ignored:** this is the feature people ask for and the one that is least useful first. It is last for three reasons — it needs the input path (issue 13) that does not exist, it needs a protocol and a link, and it is the only part where the result depends on the two machines' networks rather than on us.
- **Related:** ADR 0003 (a take after a tempo change), ADR 0005 (the graph swap and its timing), ADR 0006 (the latency compensation this reuses, and the deadline-and-restart shape), ADR 0015 §2, [#402](https://github.com/suhina-daw/suhina-daw/issues/402) (the tempo map the delay is quantised against), issue 03 (the blob the take travels as), issue 10, issue 11, issue 12, issue 13, [the epic](https://github.com/suhina-daw/suhina-daw/blob/main/issue-collaboration-and-project-versions.md) › Appendix.
- **Decisions that belong in the ADR, not here:** the codec and the buffer's sizing rule; the measurement method; the interval the delay is quantised to and whether it is user-settable; how drift is corrected; whether the host may hear the guest at all in the first version; and what modes are supported and which is the default.
- **A question to resolve before the ADR:** whether the **first** version lets the host hear the guest live at all, or only receives the take afterwards. Receiving only the take is a much smaller feature — it needs no monitoring path, no jitter buffer and no drift correction, and it still delivers "play from your own room and send me the part", which is most of the value. If that is the answer, split this issue and let the live monitoring path follow.
- **Board:** `enhancement`; Phase M8 or later, last in the epic; Priority P2.
