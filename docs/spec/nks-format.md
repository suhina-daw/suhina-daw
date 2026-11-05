---
description: What is known of Native Instruments preset files (.nksf, .nksfx) and their audio previews, from public sources, and what must be observed on our own files before Suhina reads them
read_when: NKS, nksf, nksfx, Native Instruments presets, preset previews, .previews folder, NISI, NICA, PLID, PCHK, MessagePack, Komplete Kontrol presets, plug-in state chunk
type: spec
status: planned
last_verified: 2026-11-02
---

# NKS preset files

- **Sources:** public pages and open-source readers only (listed under *Sources*). Native
  Instruments publishes no description of the format; its SDK is given to partners on request
  and is **not** a source for Suhina: nobody who has read it writes this code.
- **Observed with:** nothing yet. Every statement is from the sources and is marked
  **(to observe)** until it is checked on preset files we saved ourselves from plug-ins we own
  (see *To observe*).
- **Used by:** the browser and the sample map ([`sample-map.md`](sample-map.md) › NKS presets).
- **Naming:** in user-facing text "Native Instruments preset files (.nksf)"; never the NKS logo,
  never "NKS-ready" or "NKS-compatible" as Suhina's own branding (AGENTS.md rule 9).

## Files

| Extension | What |
|---|---|
| `.nksf` | an instrument preset |
| `.nksfx` | an effect preset; the same layout as `.nksf` **(to observe)** |
| `.previews/<preset file name>.ogg` | the preset's audio preview, OGG Vorbis, e.g. `.previews/Bass.nksf.ogg` |

- The `.previews` folder is hidden and sits in the same folder as the presets it covers; it
  does not cover sub-folders.
- Length and sample rate of previews: **(to observe)**.
- Older content of the maker's own products keeps previews in a separate library instead; not
  supported.
- User and third-party presets usually live under `Documents\Native Instruments\User Content\
  <plug-in>` (Windows) **(to observe** on macOS**)**. Suhina finds them only in folders the
  user adds to the browser; the maker's database (`komplete.db3`), registry entries and service
  files are undocumented and are not read.

## Layout

A RIFF file with the form type `NIKS`, holding four chunks. Each chunk's payload starts with a
32-bit little-endian version number (1 in known files), followed by the data.

| Chunk | Contents | Encoding |
|---|---|---|
| `NISI` | summary: name, author, vendor, comment, bank chain, types, modes, device type, uuid | MessagePack map |
| `NICA` | controller pages: parameter names, sections and ids | MessagePack map |
| `PLID` | which plug-in the preset belongs to | MessagePack map |
| `PCHK` | the plug-in's own state | bytes, opaque |

### NISI

A map with string keys:

| Key | Value |
|---|---|
| `name` | preset name |
| `author`, `vendor`, `comment` | strings |
| `bankchain` | up to three strings: product, bank, sub-bank |
| `types` | a list of `[type]` or `[type, sub-type]` (e.g. `["Bass", "Sub"]`) |
| `modes` | a list of strings (newer content: `characters` instead **(to observe)**) |
| `deviceType` | `INST`, `FX` (`MIDIFX` reported) |
| `uuid` or `UUID` | the preset's id |

Missing keys are normal. Unknown keys are ignored.

### PLID

A map naming the plug-in in one or more formats: `VST.magic` (a VST2 id, 32-bit), `VST3.uid`
(four 32-bit numbers), `VST3.plugin_name`, `AU.type`, `AU.subtype`, `AU.manufacturer`,
`AU.name`, `plugin_name`. Suhina matches `VST3.uid` against installed VST3 plug-ins; the VST2
id can only be matched when the VST3 version declares it is the same plug-in **(to observe)**.

### NICA

A map with `ni8`: a list of pages, each a list of up to eight `{name, section, id}`, where `id`
is the plug-in's parameter id. Suhina does not use it in the first version (it would map
controller knobs).

### PCHK

The plug-in's state, handed to the plug-in only. For a preset made with a VST2 plug-in it is
believed to be that plug-in's VST2 chunk. For a VST3 plug-in it is believed to be the
component state, maybe followed by the controller state **(to observe)**. Whether the VST3
version of a plug-in accepts a VST2 chunk depends on the plug-in: some do, some do not.

## Reading rules

Every number and length in the file is hostile input (AGENTS.md rule 2):

- A file larger than 64 MiB, a chunk whose length passes the end of the file, a version other
  than 1, MessagePack nested deeper than 16 levels or a string over 64 KiB: the file is skipped
  with a reason.
- A missing `NISI` gives a preset named after its file with no metadata; a missing `PLID` or
  `PCHK` gives a preset that can be previewed but not loaded.
- `PCHK` is passed, as is, only to the plug-in `PLID` names, in its sandboxed host process
  ([ADR 0006](../decisions/0006-plug-in-hosting.md)); a plug-in that crashes or rejects it is
  reported as any plug-in failure is.

## To observe

With presets we save ourselves (in the maker's own host, from plug-ins we own, VST2 and VST3
versions of the same plug-in where possible), stored outside the repository; small self-made
test files may be committed only when the plug-in's state in them is our own:

1. The RIFF layout, the version word and the MessagePack keys above.
2. `.nksfx` against `.nksf`.
3. Preview length, sample rate and channels.
4. `PCHK` from a VST3 plug-in: compare its bytes with the state the plug-in returns to a VST3
   host, to learn whether it is component state alone or component and controller state.
5. Which plug-ins load a VST2-made `PCHK` in their VST3 version.
6. What the newer version of the format changes in these files, if anything.

## Sources

- Community descriptions of the chunk layout and metadata, written from inspecting preset
  files: the *NKS libraries* site ("Going deep") and the README and code of
  `jhorology/gulp-nks-rewrite-meta` and `nks-json` (MIT, 2015).
- Open-source readers (read for how they behave, not copied): `helgoboss/helgobox` (Rust,
  GPL-3.0, its preset browser for REAPER reads these files, plays the previews and loads the
  state into VST2 and VST3 plug-ins), `cannuri/openmk3` (`nks-parse`, Rust, MIT OR Apache-2.0),
  `perivar/PresetConverter` (C#, MIT).
- Native Instruments' public pages and community forum: where previews and user content live,
  that previews are OGG Vorbis, and which plug-ins migrate VST2 presets to VST3.
- All of these describe files, not the maker's programs: none comes from disassembling a
  program, so they fall within CONTRIBUTING.md › Clean-room. Code without a licence is not
  copied, ported or translated.
