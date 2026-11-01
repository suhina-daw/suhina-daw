# Recording, inputs and monitoring

**Type:** enhancement · **Area:** `audio-io`, `engine` · **Phase:** earlier — this is build order item **3** (ADR 0016), and it is not part of the collaboration work · **Priority:** P1

**Nothing of this exists.** `crates/output` opens no input stream at all, the model has no concept of an input, and there is no recording anywhere in the engine — verified, not assumed. It is listed in the build order of ADR 0016 as item 3, size **XL**, and for one reason above the others: *"it is the first place the engine answers to a device's clock rather than its own."* Everything the engine does today is generated against its own clock and rendered ahead into a ring; recording is where something arrives from outside and has to be placed in time.

This issue is written down here because issue 14 depends on it, and for no other reason. **It is independently valuable**: it is the only way a user's own material gets into a project at all. If the rest of [the epic](https://github.com/suhina-daw/suhina-daw/blob/main/issue-collaboration-and-project-versions.md) is never built, this issue is still the right thing to do — which is exactly why it should be tracked with the build order of ADR 0016 rather than here, and why the first thing to do is **check whether it already exists on the board** before creating anything.

**Depends on:** nothing — this can start today, and it is the prerequisite of issue 14.

## Goal

A user can arm a track, hear themselves while playing, record what they play into a clip at the right place in time, and have the result behave like any other audio in the project — including after a tempo change, a save and a reopen, and with the missing-files flow as the safety net.

## Scope (a starting point, not a specification)

This issue is XL and should be **split before it is claimed**; the list below is what the split has to cover, and the shape of the split is the first thing the issue decides.

1. **An input path in the audio device layer.** `crates/output` is the only code that opens an audio device (ADR-less but longstanding: *"The only code that opens an audio device"*). It opens output streams through cpal today and nothing else; an input path is a second kind of stream, with the same rule that it is never opened on the audio thread.
2. **Monitoring.** Hearing yourself while the project plays is the part users judge first, and the part with the hardest latency budget. The engine renders **ahead** into a lock-free ring, so a monitored input has a delay to account for; latency compensation across tracks already exists and is capped at **2 seconds** (`MAX_LATENCY_SECONDS`, `crates/engine/src/lib.rs`), which is the machinery this lines up against.
3. **Recording.** From an input to a clip: where the audio is written, how it is named, how it is added to the decoded pool, and how it reaches the engine without a gap — the shape `clip.addFiles` and `Controller::set_audio` already use (ADR 0011).
4. **Placement in time.** A take must land where it was played, against the transport, and stay there across a tempo change if it is warped. This is where ADR 0003's time-stretching and the tempo map ([#402](https://github.com/suhina-daw/suhina-daw/issues/402)) meet recording.
5. **Arming, count-in and the transport.** Arming a track is a command like every other user-visible action (AGENTS.md rule 6). ROADMAP already owes a count-in: *"Count-in comes with recording."*
6. **The model.** An input, an armed track and a take are project data and belong in `model` with the others — decided in an ADR before the code (CONTRIBUTING › Issues).
7. **Undo.** A recorded take is one undo step, and undoing it must not leave a file behind that nothing references.

## Acceptance criteria

*(The split decides the real ones. These are the properties that must hold however it is split.)*

- [ ] With one input device and one track armed, a take recorded against a playing project lands at the position it was played, measurably, and not at the block boundary or the buffer size.
- [ ] Monitoring works while the project plays, with the latency measured and stated, and without glitches at the chosen buffer size.
- [ ] A recorded clip behaves like any other audio: it survives a save and reopen, shows a waveform, can be warped, and is found again by the existing missing-files flow if it is moved ([docs/spec/missing-files.md](https://github.com/suhina-daw/suhina-daw/blob/main/docs/spec/missing-files.md)).
- [ ] A take is one undo step, and undoing it leaves no orphaned file that the project cannot explain.
- [ ] An input device that disappears, a device that refuses the sample rate, and an input with no signal are three states with three messages, none of them a crash or a silent hang (AGENTS.md rule 2).
- [ ] **`Renderer::process` still never allocates, locks, blocks or does I/O** (AGENTS.md rule 3), with the input path present and absent.
- [ ] Recording does not disturb the render: an offline render of a project is unchanged by whether an input device is open.
- [ ] `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` pass.
- [ ] Documents updated: an ADR for the input path, the model and the monitoring; a spec for recording behaviour; the crate map and `docs/architecture.md` (a new thread or an existing one, and what it may do); `ROADMAP.md` › What works now; and `docs/spec/ui-session-view.md` if the UI changes.

- [ ] The pull request's description says **how to check the change** — numbered steps a
      non-musician can follow from a test build — or, when nothing a user can notice changed, one line
      saying so.
## Not in scope

- **Input routing and sends, and monitoring through effects**, which ROADMAP lists separately.
- **Loop recording, comping, take lanes and punch-in**, all of which are later features built on this.
- **The guest performer** (issue 14). This issue makes an input exist; that one carries an input across a network.
- **Reading multiple inputs at once** beyond what the first version needs.
- **An input from a plug-in or another application** (a virtual device), unless it falls out for free.

## Notes

- **This is a dependency of the epic, not part of it.** The epic's issue 14 cannot start without it; nothing else in the epic touches it. **If it is already on the board as part of that build order, this issue should be closed as a duplicate and issue 14 should depend on that one instead** — the point of writing it down is that issue 14's `Depends on:` has something to name, not to create a second tracker item.
- **Where the code goes** (`docs/architecture.md` › Where new code goes): `output` owns the device and the threads; `engine` owns the graph the input feeds, prepared in the renderer's constructor so `process` never allocates; `session` owns arming, the command and the take; `media` decodes files, and a recorded take must end up in the same pool the rest of the audio is in (ADR 0011). **Nothing below the UI depends on a GUI toolkit, and the model stays pure data.**
- **The three facts that make this XL and are worth stating in the ADR:** the engine renders ahead of the device, so an input is *behind* the material it is played against; latency compensation is built per path with the graph and is capped at 2 seconds, which is the machinery monitoring lines up against; and a device's clock and the engine's are not the same clock, which is the whole difficulty and the reason this is the build order's riskiest item (ADR 0016).
- **Related:** the build order's (ADR 0016) own entry for recording and inputs (priority 3, XL), ADR 0003 (warping a take after a tempo change), ADR 0005 (the graph swap an edit uses while recording), ADR 0006 (latency compensation across paths), ADR 0011 (`set_audio`, the path a new file takes to the engine), [#402](https://github.com/suhina-daw/suhina-daw/issues/402) (the tempo map a take lands against), issue 03 (where a project's audio lives), issue 14 (what consumes this).
- **Decisions that belong in an ADR, not here:** the model's representation of an input, an armed track and a take; whether monitoring goes through the graph or beside it; how the input's clock is reconciled; what happens to a take when recording is stopped mid-bar; and how many inputs a track may have.
- **A question to resolve before anything else:** whether recording belongs to this epic's tracker or to the build order's (ADR 0016). It is listed in the epic only so that issue 14 has a predecessor; **it should not be counted as collaboration work**, and its phase and priority come from the build order (ADR 0016), not from here.
- **Board:** `enhancement`; Phase earlier (ADR 0016 build order item 3); Priority P1 — **but check for an existing issue first, and close this one as a duplicate if there is one.**
