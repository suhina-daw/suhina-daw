---
description: Whether Suhina should build and ship a low-latency Windows audio driver of its own, what that would take (user or kernel mode, signing, licences, the ASIO name, upkeep), what the output crate already gives, and what reaches the same goals instead
read_when: own ASIO driver, custom audio driver, universal ASIO driver, FlexASIO, low latency on Windows, non-exclusive ASIO, shared audio device, several audio interfaces at once, aggregate device, virtual audio device, driver signing, WHQL, attestation signing, kernel mode driver, IAudioClient3, WASAPI low latency shared mode
type: decision
status: proposed
built: no
last_verified: 2026-11-02
covers:
  - crates/output/src/exclusive.rs
  - crates/output/src/devices.rs
---

# 0018: A Windows audio driver of Suhina's own

- **Date:** 2026-10-15
- **Issue:** [#307](https://github.com/suhina-daw/suhina-daw/issues/307) (this ADR is its research
  step; the issue stays open until the maintainer decides)

## Context

Issue #307 asks for a universal ASIO driver of Suhina's own, a fork of FlexASIO, that Suhina offers
to install at first start and that is also downloadable on its own. Its goals, in the issue's words
and grouped:

1. **Low latency** on Windows ("lightning fast audio").
2. **Non-exclusive**: several Suhina instances, and other programs, play on the same outputs at
   once.
3. **Several devices at once**: two sound cards as one set of inputs and outputs.
4. **Settings inside Suhina**, in Suhina's look, rather than a separate control panel.
5. Later: **virtual and networked inputs and outputs, custom routing** between programs.
6. **Usable without Suhina**, by any program that speaks ASIO.

This ADR looks at what each goal needs, what Suhina already has, and what a driver of its own
would cost. It writes no driver code.

### What Suhina has today

`crates/output` ([architecture](../architecture.md), ROADMAP › M2 and M3) is the only code that
opens an audio device:

- **WASAPI shared** through cpal: non-exclusive, the device stays usable by every other program.
  cpal opens it with the classic `IAudioClient::Initialize`, so the period is the Windows audio
  engine's default one, not the smallest the device allows.
- **WASAPI exclusive** (`exclusive.rs`, through the `wasapi` crate): Suhina alone on the device,
  the buffer it asks for, no system mixer in between. This is the lowest latency Windows gives
  without a vendor driver. Other programs go silent on that device while Suhina plays.
- **ASIO**, as a host, with the `asio` feature: cpal's ASIO host over the host part of the
  Steinberg ASIO SDK, vendored in `third_party/asiosdk` under GPL-3.0. Any installed ASIO driver
  works, including universal ones such as FlexASIO. The driver's own control panel is not opened
  yet ([#176](https://github.com/suhina-daw/suhina-daw/issues/176)).
- Settings › Audio already chooses the driver, device, sample rate, buffer size and exclusive
  mode, and shows the measured latency and underruns.

So goals 1 and 4 are already met *inside Suhina* by WASAPI exclusive and by vendor ASIO drivers,
with settings in Suhina's own window. What is missing is goal 1 *without* exclusive mode (goal 2),
and goals 3, 5 and 6.

### What an ASIO driver is

An ASIO driver is a user-mode in-process COM server: a DLL, registered under
`HKEY_LOCAL_MACHINE\SOFTWARE\ASIO` with its class id, that each host loads into its own process
(the SDK's host code reads that key and `InprocServer32`, `third_party/asiosdk/host/pc/asiolist.cpp`).
A universal driver such as FlexASIO owns no hardware: it opens the device through a Windows API
(WASAPI shared or exclusive, kernel streaming, MME, DirectSound; FlexASIO does this through
PortAudio) and presents the result through the ASIO interface.

Three facts follow:

- **It is never faster than the API underneath.** A universal driver over WASAPI exclusive has the
  latency of WASAPI exclusive, which Suhina already opens directly. Over WASAPI shared it has the
  shared engine's period. The layer adds a copy and a thread hand-off, not speed. The speed in a
  vendor's ASIO driver comes from its kernel driver for its own hardware, which a universal driver
  does not have.
- **It is user mode, so it needs no Microsoft signature.** Windows loads an unsigned COM DLL;
  registering it needs administrator rights once (HKLM). An Authenticode signature on the installer
  and DLL is still needed in practice, or SmartScreen warns every user who downloads it. Hosts
  of both bitnesses need a matching DLL each (64-bit and 32-bit, and ARM64 for Windows on Arm).
- **It runs inside every host that loads it**, Suhina included. A crash in it takes the host down.

### Kernel mode: what goals 3 and 5 would need

A device that appears in Windows' own list of playback and recording devices (a virtual cable, a
virtual mixer output other programs can pick, a networked endpoint) is a kernel-mode audio driver:
PortCls/WaveRT, or since Windows 10 version 2004 the ACX class extension, which Microsoft builds on
KMDF so that streaming does not cross between user and kernel mode
([ACX overview](https://learn.microsoft.com/windows-hardware/drivers/audio/acx-audio-class-extensions-overview)).
There is no user-mode way to add an endpoint. Kernel mode brings:

- **Signing by Microsoft.** Since Windows 10 version 1607, Windows loads no new kernel-mode driver
  that is not signed through Microsoft's Hardware Dev Center
  ([kernel-mode code signing policy](https://learn.microsoft.com/windows-hardware/drivers/install/kernel-mode-code-signing-policy--windows-vista-and-later-)).
  Opening a Dev Center account requires an **EV code-signing certificate**, bound to a company
  ([driver code signing requirements](https://learn.microsoft.com/windows-hardware/drivers/dashboard/code-signing-reqs)).
  *Attestation signing* skips the Hardware Lab Kit (HLK) tests, but Microsoft now describes it as
  for testing scenarios, and attestation-signed drivers aimed at retail users are not published on
  Windows Update ([attestation signing](https://learn.microsoft.com/windows-hardware/drivers/dashboard/code-signing-attestation),
  page dated 2025-07-15). A driver for wide release is expected to pass the HLK and be submitted
  with its logs. Every release goes through that submission again.
- **A crash is a blue screen**, not a crashed program. Golden rule 2 ("never crash") then means the
  machine, not the process.
- **Language and code to start from.** The workspace forbids `unsafe` outside `crates/ipc` and
  `crates/plugin-host` (ADR 0006). A kernel driver is almost entirely FFI; Microsoft's
  `windows-drivers-rs` (MIT or Apache-2.0) makes Rust possible but is young, and most driver code
  and samples are C or C++. Microsoft's audio driver samples (SysVAD and others in
  `Windows-driver-samples`) are under the **Microsoft Public License**, which the FSF lists as
  incompatible with the GPL, so they cannot be copied into a GPL-3.0 driver; they can only be read
  as documentation and the driver written from that.

### Licences of what a user-mode driver could build on (checked 2026-10-15)

| Component | Licence | GPL-3.0 |
|---|---|---|
| FlexASIO (the driver) | MIT, with a note that the ASIO SDK has its own terms | compatible |
| PortAudio (FlexASIO's backend) | MIT-style (PortAudio licence) | compatible |
| Steinberg ASIO SDK, driver side (`common/asiodrvr.*`, `driver/`) | dual: Steinberg proprietary or GPL-3.0 | compatible under GPL-3.0; the whole driver is then GPL-3.0 |
| ASIO SDK `common/combase.*`, `dllentry.cpp` | Microsoft's COM base classes under Microsoft's own terms (`third_party/asiosdk/VENDORED.md`) | unclear; avoid, write the COM entry points instead |
| Microsoft `low-latency-audio` (UAC2 driver with an ASIO interface) | MIT; its README says Microsoft's ASIO licence does not transfer to anyone building it | code compatible; the ASIO part then rests on the SDK's GPL-3.0 option |
| `windows` / `windows-drivers-rs` crates | MIT or Apache-2.0 | compatible |
| Microsoft driver samples (SysVAD) | Ms-PL | **not** compatible |

So a FlexASIO fork is licensable as GPL-3.0: MIT code, plus the ASIO SDK under its GPL-3.0 option,
as Suhina already does for the host side. That makes the driver GPL-3.0, which is what the project
wants anyway.

### The name

The ASIO SDK's trademark rules (in `third_party/asiosdk/README.md`, "Prohibited Use") forbid
"using 'ASIO' (or derivatives like 'ASIOi') in company or product names". This applies under the
GPL-3.0 option too: the GPL grants the code, not the mark. **"SuhinASIO" cannot be the product
name.** A driver could be called something without "ASIO" and say it is an ASIO driver in its
documentation, as the guidelines allow.

### Windows itself is moving

Microsoft is building, with Yamaha and Qualcomm, a new in-box USB Audio Class 2 driver for
low-latency music work with an ASIO interface, open source under MIT
([microsoft/low-latency-audio](https://github.com/microsoft/low-latency-audio)). Its stated aims
include high channel counts and letting Windows and a music program use the same interface at once.
It slipped from a mid-2025 preview; Microsoft's February 2026 post placed the preview later in 2026
([Windows Experience Blog, 2026-02-17](https://blogs.windows.com/windowsexperience/2026/02/17/making-music-with-midi-just-got-a-real-boost-in-windows-11/)).
At the time of writing the repository still says there is no public release. When it ships, every
class-compliant USB interface gets a low-latency, shareable ASIO driver from Windows, which is
goals 1, 2 and 6 for most of the hardware musicians use, maintained by Microsoft.

Windows also has a low-latency shared mode of its own: since Windows 10, `IAudioClient3` lets a
program open a *shared* stream at a smaller period than the default, down to the smallest the
driver supports, while other programs keep playing
([`IAudioClient3`](https://learn.microsoft.com/windows/win32/api/audioclient/nn-audioclient-iaudioclient3),
[low latency audio](https://learn.microsoft.com/windows-hardware/drivers/audio/low-latency-audio)).
How small depends on the device's driver (Microsoft's in-box HD Audio driver supports it; many
vendor drivers do not). The `wasapi` crate Suhina already uses wraps
`GetSharedModeEnginePeriod` and `InitializeSharedAudioStream` in its current source; cpal does
not use them.

### Upkeep

A driver is a second product with its own users, most of whom would not run Suhina (goal 6):

- Releases for x64, x86 and ARM64, an installer that registers and unregisters the COM class, a
  yearly paid code-signing certificate (EV, company-bound, for any kernel part).
- Support for every host program's quirks with ASIO, every device's quirks under WASAPI and kernel
  streaming, and Windows updates that change audio behaviour. FlexASIO's issue tracker is a public
  record of how much of a single maintainer's time this takes.
- Tests that need real Windows audio hardware, which CI does not have.
- For a kernel part: a Dev Center account, HLK runs or attestation on every release, and crash
  dumps from users' machines.

### The project's own rules

- **Golden rule 1 (clean-room)**: forking FlexASIO is fine (MIT, public source). Nothing may come
  from closed drivers' binaries.
- **Golden rule 2 and the `unsafe` ban**: a COM DLL in Rust needs `unsafe` at the COM boundary.
  It would be a third exception after `crates/ipc` and `crates/plugin-host`, in a crate of its own,
  under the same conditions (small, tested, explained blocks behind a safe API). A C++ fork would
  not touch the Rust rule but would bring a second language into the tree.
- **ADR 0015 (extensions)**: Suhina does not download components by itself. Offering to *install*
  a driver at first start is installing system software from inside a program, with an
  administrator prompt; that needs the same care.

## Options

### A. Fork FlexASIO as Suhina's universal ASIO driver (what the issue asks)

A separate GPL-3.0 project (own repository, own releases), FlexASIO's code with Suhina's look in
its settings, installed by Suhina's installer or on its own.

- Gives goal 6 and a Suhina-styled control panel (goal 4, though inside Suhina the settings
  already exist).
- Does **not** give goal 1 beyond what Suhina already has: over WASAPI exclusive it equals Suhina's
  exclusive mode; over WASAPI shared it equals the shared engine's period, the same as other
  programs get.
- Goal 2 only over WASAPI shared, with shared-mode latency: low latency and sharing pull against
  each other below the driver, and an ASIO wrapper does not change that.
- Goal 3 needs clock-drift compensation between devices that each run on their own crystal: an
  adaptive resampler per extra device, in the real-time path. FlexASIO does not do it; it would
  be new work.
- Cost: a second product, a rename (not "SuhinASIO"), a C++ code base or a third `unsafe`
  exception, signing, three architectures, support for every other host.

### B. Kernel-mode virtual audio driver (for goal 5)

An ACX or PortCls driver that adds virtual endpoints for routing between programs.

- The only option that gives goal 5.
- Cost: an EV certificate and a Microsoft Dev Center account held by a company, HLK or attestation
  on every release, blue screens instead of crashes, mostly C/C++ (or an early-stage Rust
  toolkit), and no Microsoft sample code to start from because of Ms-PL.

### C. Do it in `crates/output`, without a driver

Reach the goals for Suhina itself where the code already is:

1. **Low-latency shared mode**: open WASAPI shared through `IAudioClient3` at the smallest period
   the device allows, beside the present exclusive mode. Gives goals 1 and 2 together on devices
   whose driver supports small periods, and falls back to the default period elsewhere.
2. **Several devices as one**: open several devices and keep them in step with drift
   compensation, in Rust in the output crate, under the real-time rules. Gives goal 3 for Suhina.
3. **The ASIO driver's own panel** from Settings › Audio (#176), so vendor drivers' settings are a
   click away.
4. **Several Suhina instances on one device**: already works in shared mode; with item 1 it works
   at low latency too.

- Gives goals 1 to 4 for Suhina, in Rust, under the project's existing rules, tested in its
  existing CI where it does not need hardware.
- Does not give goal 6 (other programs) or goal 5 (routing between programs).

### D. Wait for, and support, the in-box Windows driver

Make sure Suhina's ASIO host and WASAPI paths work well with Microsoft's new USB Audio Class 2
driver when its preview ships, and say so in the docs.

- Goals 1, 2 and 6 for class-compliant USB interfaces, maintained by Microsoft, at no cost to
  the project.
- Timing is Microsoft's, and has slipped once.

### E. Point users at existing free drivers

Document FlexASIO (and the in-box driver once it exists) as the way to use ASIO with hardware that
has no vendor driver.

- No cost; goal 6 is someone else's.
- No Suhina look, and no help for the programs Suhina does not control.

## Recommendation

**C and D now, E in the docs; not A or B for now.**

- **C** gives Suhina's own users what the issue is after (low latency without taking the device
  away from other programs, several Suhina instances on one device, several interfaces at once,
  settings in Suhina's look) with code the project already knows how to write, test and ship. It
  belongs in `crates/output` and follows the existing real-time and `unsafe` rules. It should be
  split into issues: low-latency shared mode (`IAudioClient3`), several devices with drift
  compensation, and #176.
- **D** costs almost nothing and may make goal 6 moot for most USB hardware within a year. Track
  the preview and test Suhina against it when it appears.
- **E** is a paragraph in the README's building and audio sections.
- **A** buys mainly goal 6, for programs other than Suhina, at the price of a second product with
  its own users, platforms and support. Its latency is bounded by the same Windows APIs Suhina
  already calls directly. If the maintainer still wants it after C and D, it should be a separate
  repository and project, not part of this workspace, under a name without "ASIO", with its own
  ADR for the language (C++ fork or Rust with an `unsafe` exception) and for how Suhina offers it
  (in the installer, never downloaded by Suhina, per ADR 0015).
- **B** should wait until there is a company to hold an EV certificate and a Dev Center account,
  someone to run the HLK on every release, and someone to maintain kernel code. Nothing else in the roadmap needs it.

## Consequences

If accepted:

- New issues for option C under the audio output area; ROADMAP gets the low-latency shared mode and
  several-devices items when they land, not before.
- #307 is closed as answered by those issues, or kept open as the place to revisit A once the
  in-box driver has shipped.
- Nothing changes in the code with this ADR.

## Open questions for the maintainer

1. Is goal 6, a driver that programs other than Suhina use, a goal of the project in itself, or
   was it a way to give Suhina low latency? The recommendation assumes the second.
2. If a separate driver is still wanted: a C++ fork of FlexASIO (fast to start, second language,
   no Rust `unsafe` question) or a Rust rewrite (one language, a third `unsafe` exception)?
3. Who would hold the code-signing certificate (a person or a company), and is a yearly cost for
   it acceptable? An EV certificate for kernel work must be bound to a company.
4. Is routing between programs (goal 5) in scope at all? ADR 0016 leaves out what is not
   clip-launching DAW work; a virtual cable is system software.
5. Should the first-start screen offer to install drivers at all, or only point to them? ADR 0015
   keeps Suhina from downloading components; installing system software from the app needs an
   administrator prompt and a decision of its own.
6. Which name, if a driver is made, given that "ASIO" cannot be part of it?

## Sources

All public, checked 2026-10-15:

- Steinberg ASIO SDK 2.3.4 README (dual licence, trademark and logo rules), vendored in
  `third_party/asiosdk/README.md`; host registry lookup in `third_party/asiosdk/host/pc/asiolist.cpp`.
- FlexASIO: [README](https://github.com/dechamps/FlexASIO/blob/master/README.md) and
  [LICENSE.txt](https://github.com/dechamps/FlexASIO/blob/master/LICENSE.txt).
- PortAudio: [LICENSE.txt](https://github.com/PortAudio/portaudio/blob/master/LICENSE.txt).
- Microsoft: [low-latency-audio](https://github.com/microsoft/low-latency-audio) (README and
  LICENSE), [Windows-driver-samples LICENSE](https://github.com/microsoft/Windows-driver-samples/blob/main/LICENSE),
  [windows-drivers-rs](https://github.com/microsoft/windows-drivers-rs),
  [kernel-mode code signing policy](https://learn.microsoft.com/windows-hardware/drivers/install/kernel-mode-code-signing-policy--windows-vista-and-later-),
  [driver code signing requirements](https://learn.microsoft.com/windows-hardware/drivers/dashboard/code-signing-reqs),
  [attestation signing](https://learn.microsoft.com/windows-hardware/drivers/dashboard/code-signing-attestation),
  [ACX overview](https://learn.microsoft.com/windows-hardware/drivers/audio/acx-audio-class-extensions-overview),
  [low latency audio](https://learn.microsoft.com/windows-hardware/drivers/audio/low-latency-audio),
  [`IAudioClient3`](https://learn.microsoft.com/windows/win32/api/audioclient/nn-audioclient-iaudioclient3),
  [Windows Experience Blog, 2026-02-17](https://blogs.windows.com/windowsexperience/2026/02/17/making-music-with-midi-just-got-a-real-boost-in-windows-11/).
- FSF, [Various Licenses and Comments about Them](https://www.gnu.org/licenses/license-list.html)
  (Ms-PL: free, GPL-incompatible).
- `wasapi` crate source ([HEnquist/wasapi-rs](https://github.com/HEnquist/wasapi-rs), `src/api.rs`):
  `get_shared_mode_engine_period`, `initialize_shared_audio_stream` (check the version Suhina pins
  before relying on it).
