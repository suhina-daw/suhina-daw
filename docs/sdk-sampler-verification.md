---
description: Reproduce sampler graph project relocation, runtime and desktop checks
read_when: SDK sampler QA, issue 733, sampler graph compatibility, sampler performance
type: howto
status: current
last_verified: 2026-10-22
covers:
  - crates/device-sdk/tests/composed_templates.rs
  - crates/device-sdk/tests/resource_nodes.rs
  - crates/device-sdk/tests/prepared_players.rs
  - crates/session/tests/devices.rs
  - crates/session/src/devices.rs
  - crates/session/src/files.rs
  - crates/session/src/commands.rs
  - crates/model/src/lib.rs
  - crates/engine/src/chain.rs
  - crates/device-registry/src/lib.rs
  - crates/automation/src/confine.rs
---

# Sampler graph verification

Platform checked: Windows. These checks use self-authored synthetic PCM and graph data.
They do not establish audio equivalence between the editable template and the native sampler.

## Prepare the check files

From a source checkout, in PowerShell:

```powershell
$env:SUHINA_SAMPLER_QA_DIR = "$PWD/target/sampler-qa"
cargo test -p suhina-session --test devices composed_sampler -- --nocapture
Remove-Item Env:SUHINA_SAMPLER_QA_DIR
```

The test creates `target/sampler-qa/sampler.suhina`, `sampler.sd` and `tone.wav`.
Keep the project and WAV together when copying the folder beside a test build.
The project contains one MIDI track called Keys and one short note in its arrangement.
The sample is a quiet synthetic tone. Its embedded graph uses two private voices.

## How to check it

1. Open Suhina with nothing loaded. Press Ctrl+O (or click **Open...** on the start
   screen). In **New or open a project**, put the full path to `sampler.suhina` in
   **Project path**, then click **Open**. The project opens with a Keys track and no
   missing-file warning.
2. Press Space (or click the triangle **Play** in the top transport). The Keys and
   Main level meters register a short tone. Silence throughout is a failure; do not
   rely only on the moving transport to confirm playback.
3. Press Space again to stop. The meters return to silence. Press Ctrl+S
   (**File > Save**). The save completes without an error.
4. Open the saved project again with Ctrl+O and its path. Repeat step 2. The same
   tone plays: graph state and the resource reference both survived saving.
5. Close the project. Copy the entire check folder to a different directory, then
   open `sampler.suhina` from that copy. It resolves `tone.wav` beside the project;
   playback still works. This also works when the original source directory no
   longer exists, as the automated relocation check verifies.
6. For the missing-resource edge case, close the project. Rename `tone.wav` to
   `tone-away.wav` in both check folders if the original folder is still available,
   then open the copied project. It must report the missing source. Restore the
   names before repeating playback. With only the relocated WAV missing, a saved
   absolute fallback may correctly find the original WAV instead.

The desktop check covered opening, playback meters, stopping, saving, reopening
from the relocated folder, and the missing-source case. When both WAV paths were
unavailable, the app showed **1 audio file is missing**, with **Find...** and **Later**.
With only the relocated WAV unavailable, the original absolute fallback resolved
correctly. The CLI rendered the relocated project with a peak of 0.19998.
Sample replacement and undo are covered by the project integration test; they were
not repeated through the desktop controls.

## Automated evidence and limits

- `resource_nodes` checks interpolation, invalid and foreign regions, sample metadata,
  typed resource ports, replacement retaining the old source, slice maps and crossfade heads.
- `voice_nodes` checks exact event offsets, sustain, release, retrigger, voice stealing,
  component state isolation and bounded MIDI overflow.
- `prepared_players` checks reverse/gated/one-frame slice playback, parameter bindings,
  aggregate state budgets, allocation/free counters and block-size independence.
- `composed_templates` checks the editable sampler and granular templates across block
  sizes 1, 7, 64, 129 and 512, allocation/free counters, memory and optional CPU measurements.
- The session integration test checks real WAV decoding, graph insertion failure without
  bindings, sample replacement without overwriting graph state, project round-trip,
  relocation after removing the original WAV, equal engine renders at blocks 64 and 257,
  missing-resource warnings and undo.
- Existing format, preset, device migration and native sampler tests remain the compatibility
  gate. Legacy `native_device` wrappers stay rejected, following the merged main branch.
- Preparation caps nesting at four, expanded nodes at 4096, voices at 128, grains at 256,
  bindings at 16384 and delay frames at the shared delay budget. These are rejection limits,
  not promises that a maximum-size graph meets an audio deadline.
- Project commands bind one source under `sample`; custom Rust hosts can supply several
  resource keys. Arbitrary paths are never executed or resolved by graph nodes.

The callback allocation counter observed zero allocations and frees for four sampler
voices and four granular voices with eight grains each, including sustain, release,
parameter updates and reset. Prepared resident state measured 831008 bytes for the sampler
and 1407209 bytes for that granular configuration (including the test source).
CPU timing is machine/load dependent; use the ignored release measurement to establish
an appropriate voice count on the target machine:

```powershell
cargo test -p suhina-device-sdk --release --test composed_templates composed_cpu_measurement -- --ignored --nocapture
```

The editable graph is more expensive than the ready-made sampler. Eight simultaneous
voices exceeded one second of CPU time per second of audio in a loaded local run.
Use smaller voice counts or the ready-made node when CPU budget is tight. No hard
real-time deadline guarantee is claimed by a successful functional test.

After workspace compilation finished, the same release measurement gave 23 ms for
the ready-made sampler, 87/323/552 ms for the editable sampler with 1/4/8 voices,
and 193 ms for four granular voices with sixteen grains. The loaded run was slower
(191/654/1092 ms for the editable sampler). These are total CPU times for one second
at 48 kHz, not worst-callback deadline or hardware-independent guarantees.
