---
description: How to author the Live Sets used to learn the .als format and test the importer, series by series, including the device series with bounces
read_when: making test sets, locators, corpus, new series, what to save in Live, test files, device corpus, bounce, device parameters, test signal, automation, envelopes
type: howto
status: current
last_verified: 2026-10-28
covers:
  - crates/als/tests/corpus.rs
  - scripts/test-signal.py
---

# Making the `.als` test corpus

How to author the Live Sets we use to learn the `.als` format and to test the importer. Read
[`CONTRIBUTING.md`](../../CONTRIBUTING.md) › Clean-room first: everything in the corpus must be
made by you, in your own copy of Live, from scratch.

## Rules

- **Change one thing per file.** Each file is the previous one plus exactly one edit, saved under
  a new name. Then a diff between neighbours shows only what that edit changed.
- **Only your own content.** No samples, presets, clips or devices from Live's Core Library or
  Packs. Audio comes from your own recording or from Suhina itself ([Series 2](#series-2-audio)).
- **Don't click around.** Live also saves view state (selected track, scroll position, open
  panels). Every extra click adds noise to the diff. Make the edit, save, move on.
- **Record what you did.** Fill in the log table at the end (copy it to `corpus/als/README.md`).

## Setup

1. Note your Live version and edition (Help › About Live, e.g. "Live 12.2.5 Suite") and your OS.
2. Create the folder `corpus/als/` in your Suhina checkout. It is gitignored; nothing here is
   committed.
3. Start from a new Live Set (File › New Live Set).

Use **File › Save Live Set As…** for every step and save into `corpus/als/`. Live creates a
project folder the first time; keep saving all sets into that same project folder, so they
share one `Samples/` folder.

## Series 1: basics

| # | File name | Edit from the previous file |
|---|---|---|
| 00 | `00-base.als` | Delete every track you can (select all tracks, Delete). Keep the master. Tempo 120, 4/4. Save. |
| 00b | `00b-base-resaved.als` | **No edit.** Open `00-base.als` again and save it under the new name. This shows which fields change on every save (ids, timestamps) and are noise. |
| 01 | `01-tempo-128.als` | From `00-base`: set the tempo to 128. |
| 02 | `02-midi-track.als` | From `00-base`: add one MIDI track (Create › Insert MIDI Track). No instrument. |
| 03 | `03-midi-clip-empty.als` | Double-click the first Session slot of the MIDI track to create an empty clip (default length, one bar). |
| 04 | `04-midi-clip-one-note.als` | In that clip, draw one note: C3, on beat 1, length 1/4, default velocity. |
| 05 | `05-note-moved.als` | Move the note to beat 2. Nothing else. |
| 06 | `06-note-velocity.als` | Change the note's velocity to 64. |
| 07 | `07-note-key.als` | Change the note to E3. |
| 08 | `08-clip-length-2-bars.als` | Set the clip length to 2 bars (Clip View, loop length). |
| 09 | `09-clip-loop-off.als` | Turn the clip's Loop switch off. |
| 10 | `10-clip-name-color.als` | Rename the clip to `Test` and pick a different clip colour. |
| 11 | `11-insert-scene.als` | Insert a scene (Create › Insert Scene). |
| 12 | `12-session-clip.als` | Create a clip in the first Session slot of the MIDI track with one note: C3 on beat 1. |
| 13 | `13-second-session-clip.als` | Create a clip in the second slot of the same track with one note: G3 on beat 1. |
| 14 | `14-arrangement-moved.als` | In Arrangement View, move the MIDI track's arrangement clip to bar 3. |
| 15 | `15-close-piano-roll.als` | Close the piano roll (Clip View). Nothing else. |
| 16 | `16-extend-mixer.als` | Show the whole mixer section in Launcher view. Nothing else. |
| 17 | `17-track-volume-pan.als` | Set the audio track's volume to -6 dB and pan to 50L (fully left). |
| 18 | `18-track-mute.als` | Mute the audio track (Track Activator off). |
| 19 | `19-return-send.als` | Unmute. Turn the audio track's Send A up to -12 dB. |
| 20 | `20-clip-length-2-bars.als` | Set the length of the session clip in slot 1 to 2 bars (Clip View, loop length). |

## Series 2: audio

Make the audio file with Suhina so its origin is clean:

```sh
cargo run -p suhina-cli -- render --demo -o corpus/als/loopwerk-demo.wav
```

Continue from the last file of series 1 and use an audio track that has no clips yet.

| # | File name | Edit from the previous file |
|---|---|---|
| 30 | `30-audio-clip.als` | In Launcher view, drag `loopwerk-demo.wav` from your file manager into the track's first clip slot. Leave warp settings as Live sets them. |
| 31 | `31-audio-warp-off.als` | Open the clip (Clip View) and turn Warp off. |
| 32 | `32-audio-warp-marker.als` | Turn Warp on again, add one warp marker on beat 2 and drag it a little to the right. |
| 33 | `33-audio-gain.als` | Set the **clip's** Gain (Clip View, not the mixer) to -6 dB. |
| 34 | `34-audio-transpose.als` | Set the clip's Transpose to +3 st. |
| 35 | `35-audio-in-arrangement.als` | In Arrangement View, drag `loopwerk-demo.wav` to bar 1 of the same track. |

## Series 3: devices

| # | File name | Edit from the previous file |
|---|---|---|
| 40 | `40-midi-track-instrument.als` | Drag Live's built-in **Simpler** (empty, no sample) onto a MIDI track. Not a preset. |
| 41 | `41-audio-effect.als` | Add a built-in **EQ Eight** after it, default settings. |
| 42 | `42-device-param.als` | Change one EQ Eight parameter (band 1 gain to +3 dB). |
| 43 | `43-device-off.als` | Turn EQ Eight off with its device activator. |

If you have a CLAP or VST3 plug-in with a free licence, also save `44-plugin.als` with it on the
track and note the plug-in's name and version. (The authored file is named after the plug-in
used.)

## Series 4: open questions

Answers the open questions in [`als-format.md`](als-format.md). Start from
`35-audio-in-arrangement.als`, not from a `44-*` file, so no commercial plug-in state ends up in these
sets. "The C3 clip" is the session clip in 1-MIDI slot 1 (C3, loop on, 2 bars).

| # | File name | Edit from the previous file |
|---|---|---|
| 50 | `50-time-signature-3-4.als` | In the transport bar, set the time signature to 3/4 (type `3` in the numerator). |
| 51 | `51-time-signature-6-8.als` | Set 6/8. |
| 52 | `52-time-signature-7-8.als` | Set 7/8. |
| 53 | `53-time-signature-4-4.als` | Back to 4/4. |
| 54 | `54-clip-start-marker.als` | Clip View of the C3 clip: type `1.2.1` in **Start**. |
| 55 | `55-clip-loop-position.als` | Type `1.3.1` in the loop **Position**. |
| 56 | `56-clip-loop-off.als` | Turn **Loop** off. |
| 57 | `57-clip-end-marker.als` | Type `2.3.1` in **End**. |
| 58 | `58-clip-deactivated.als` | Select the C3 clip in Launcher view and press `0` (deactivate clip). |
| 59 | `59-note-deactivated.als` | Open the G3 clip (1-MIDI slot 2), select the note and press `0` (deactivate note). |
| 60 | `60-group-track.als` | Select the headers of 3-Audio and 4-loopwerk-demo (Ctrl-click) and press `Ctrl+G`. |
| 61 | `61-audio-warp-off.als` | Session audio clip on 4-loopwerk-demo: Warp off. |
| 62 | `62-audio-unwarped-start.als` | Move the clip's **Start** to about one second. Log the exact value Live shows. |

## Series 6: devices

Teaches the importer how Live stores the parameters of its own devices, so they can be mapped
onto Suhina's devices ([device import](device-import.md)), and gives a bounce of each
setting to compare Suhina's sound with. It works differently from series 1–4 in three ways:

- **Its own project:** `corpus/devices/`, so device files do not mix with the format series.
- **Every file starts from the device's default file,** not from the previous file. Then a
  diff against the default shows exactly one parameter, and a wrong step spoils one file, not
  the rest of the series.
- **Every file gets a bounce:** a WAV of the test signal through the device, with the same name.

### Setup (once)

1. Write the test signal (8 s, one second per section; the sections are listed in
   [`scripts/test-signal.py`](../../scripts/test-signal.py)):

   ```sh
   python scripts/test-signal.py corpus/devices/test-signal.wav
   ```

2. In Live: File › New Live Set. Delete every track except one audio track, and the return
   tracks. Tempo 120, 4/4. At 120 bpm the 8-second signal is exactly 4 bars.
3. In Arrangement View, drag `corpus/devices/test-signal.wav` from your file manager to bar 1
   of the audio track. Open the clip and turn **Warp off**, so it plays at its own speed.
4. File › Save Live Set As… → `corpus/devices/100-base.als`. Live creates the project folder
   `corpus/devices/100-base Project/`; save every later device set into that same folder.
5. Bounce it, as described next, to `corpus/devices/bounces/100-base.wav`.

### Bouncing

File › Export Audio/Video, with the same settings every time:

| Setting | Value |
|---|---|
| Rendered Track | Master |
| Render Start / Length | `1.1.1` / `4.0.0` |
| File Type | WAV |
| Sample Rate | 48000 |
| Bit Depth | 24 |
| Normalize, Convert to Mono, Create Analysis File | off |
| Dither | No Dither |

Save it into `corpus/devices/bounces/` under the set's name with `.wav` instead of `.als`.

### For each device

1. Open `100-base.als`. From Live's browser (Audio Effects) drag the **device itself** onto the
   audio track. Never a preset from the device's folder, never one from a Pack.
2. Save As the device's default file (`110-utility-default.als` and so on) and bounce it.
3. For each row of the device's table:
   1. Open the default file (File › Open Recent). If a row says "from 200", open that file
      instead.
   2. Make the one change. **Type** values into the field where you can (click the control,
      type `-12`, Enter), so the stored number is exact. A row that says *drag* means turning
      the control all the way to its end.
   3. Save As the row's file name, then bounce under the same name.
   4. Write the value Live **shows** into the log. Live's displayed value and its stored value
      often differ (a percentage stored as 0..1, decibels stored as a gain); that difference
      is what the mapping has to learn.
4. Never save over the default file. If you did, delete it and redo step 2.

**Example,** row 111 of Utility: open `110-utility-default.als`, click Utility's Gain, type `-12`,
press Enter, Save As `111-utility-gain-minus12.als`, Export Audio/Video to
`bounces/111-utility-gain-minus12.wav`, and log `-12.0 dB`.

When the series is done, the folder looks like this:

```text
corpus/devices/
  test-signal.wav
  README.md                      the log (template below)
  100-base Project/
    100-base.als
    110-utility-default.als
    111-utility-gain-minus12.als
    ...
  bounces/
    100-base.wav
    110-utility-default.wav
    111-utility-gain-minus12.wav
    ...
```

### Utility (110–135)

| # | File name | Change from `110-utility-default.als` |
|---|---|---|
| 110 | `110-utility-default.als` | Utility added, nothing changed. |
| 111 | `111-utility-gain-minus12.als` | Gain: type `-12`. |
| 112 | `112-utility-gain-plus6.als` | Gain: type `6`. |
| 113 | `113-utility-gain-inf.als` | Gain: drag fully down, until it shows -inf. |
| 114 | `114-utility-gain-max.als` | Gain: drag fully up. |
| 115 | `115-utility-balance-25l.als` | Balance: type `25L`. |
| 116 | `116-utility-balance-100r.als` | Balance: drag fully right (Balance shows 50L to 50R). |
| 117 | `117-utility-width-0.als` | Width: type `0`. |
| 118 | `118-utility-width-200.als` | Width: type `200`. |
| 119 | `119-utility-width-max.als` | Width: drag fully up. |
| 120 | `120-utility-midside-mid-50S.als` | Switch Width to Mid/Side mode (it is not on the panel: right-click Width). Leave the knob where it lands; log what it shows. |
| 121 | `121-utility-midside-centre.als` | Mid/Side mode, drag fully to Mid. |
| 122 | `122-utility-midside-side.als` | Mid/Side mode, drag fully to Side. |
| 123 | `123-utility-mono.als` | Mono on. |
| 124 | `124-utility-bassmono-120.als` | Bass Mono on, frequency `120`. |
| 125 | `125-utility-bassmono-min.als` | Bass Mono on, frequency dragged fully down. |
| 126 | `126-utility-bassmono-max.als` | Bass Mono on, frequency dragged fully up. |
| 127 | `127-utility-phase-left.als` | Ø L on. |
| 128 | `128-utility-phase-right.als` | Ø R on. |
| 129 | `129-utility-phase-both.als` | Invert both. |
| 130 | `130-utility-channel-left.als` | Input channel menu (shows Stereo): Left. |
| 131 | `131-utility-channel-right.als` | Input channel menu: Right. |
| 132 | `132-utility-channel-swap.als` | Input channel menu: Swap. |
| 133 | `133-utility-dc.als` | DC filter on. |
| 134 | `134-utility-mute.als` | Mute on. |
| 135 | `135-utility-bassmono-listen.als` | Bass Mono on, and the headphones button next to its frequency on. |

### Auto Pan-Tremolo (140–164)

| # | File name | Change from `140-autopan-default.als` |
|---|---|---|
| 140 | `140-autopan-default.als` | Auto Pan-Tremolo added, nothing changed. Log the default mode, rate and phase. |
| 141 | `141-autopan-amount-0.als` | Amount: type `0`. |
| 142 | `142-autopan-amount-100.als` | Amount: type `100`. |
| 143 | `143-autopan-rate-0.1hz.als` | Rate in hertz: type `0.1`. |
| 144 | `144-autopan-rate-10hz.als` | Rate: type `10`. |
| 145 | `145-autopan-rate-max.als` | Rate: drag fully up. |
| 146 | `146-autopan-sync-1bar.als` | Switch the rate to beat sync, choose 1 bar. |
| 147 | `147-autopan-sync-1-4.als` | Beat sync, 1/4. |
| 148 | `148-autopan-sync-1-8-dotted.als` | Beat sync, dotted 1/8 (log how Live names it). |
| 149 | `149-autopan-sync-1-16-triplet.als` | Beat sync, triplet 1/16. |
| 150–156 | `150-autopan-shape-<name>.als` | One file per waveform other than the default, in Live's order; `<name>` as Live shows it, in lower case (`150-autopan-shape-triangle.als`). |
| 157 | `157-autopan-phase-0.als` | Phase: type `0`. |
| 158 | `158-autopan-phase-90.als` | Phase: type `90`. |
| 159 | `159-autopan-offset-90.als` | Offset: type `90`. |
| 160 | `160-autopan-offset-270.als` | Offset: type `270`. |
| 161 | `161-autopan-shaping-50.als` | Shaping: type `50`. |
| 162 | `162-autopan-shaping-100.als` | Shaping: type `100`. |
| 163 | `163-autopan-invert.als` | Invert on. |
| 164 | `164-autopan-mode-other.als` | Switch the Pan/Tremolo mode to the one that is not the default. |

### EQ Three (170–187)

| # | File name | Change from `170-eq3-default.als` |
|---|---|---|
| 170 | `170-eq3-default.als` | EQ Three added, nothing changed. Log FreqLo and FreqHi. |
| 171 | `171-eq3-low-minus12.als` | GainLo: type `-12`. |
| 172 | `172-eq3-low-inf.als` | GainLo: drag fully down (-inf). |
| 173 | `173-eq3-low-max.als` | GainLo: drag fully up. |
| 174–176 | `174-eq3-mid-minus12.als`, `175-eq3-mid-inf.als`, `176-eq3-mid-max.als` | The same for GainMid. |
| 177–179 | `177-eq3-high-minus12.als`, `178-eq3-high-inf.als`, `179-eq3-high-max.als` | The same for GainHi. |
| 180 | `180-eq3-kill-low.als` | Low band switch off (the kill). |
| 181 | `181-eq3-kill-mid.als` | Mid band switch off. |
| 182 | `182-eq3-kill-high.als` | High band switch off. |
| 183 | `183-eq3-freqlo-min.als` | FreqLo: drag fully down. |
| 184 | `184-eq3-freqlo-max.als` | FreqLo: drag fully up. |
| 185 | `185-eq3-freqhi-min.als` | FreqHi: drag fully down. |
| 186 | `186-eq3-freqhi-max.als` | FreqHi: drag fully up. |
| 187 | `187-eq3-slope-other.als` | Slope: the one that is not the default (24 or 48 dB/oct). |

### Vinyl Distortion (190–208)

| # | File name | Change |
|---|---|---|
| 190 | `190-vinyl-default.als` | Vinyl Distortion added, nothing changed. Log which sections are on. |
| 191 | `191-vinyl-tracing-off.als` | Tracing section off. |
| 192 | `192-vinyl-tracing-drive-min.als` | Tracing Drive: drag fully down. |
| 193 | `193-vinyl-tracing-drive-max.als` | Tracing Drive: drag fully up. |
| 194 | `194-vinyl-tracing-freq-min.als` | Tracing Freq: drag fully down. |
| 195 | `195-vinyl-tracing-freq-1k.als` | Tracing Freq: type `1000`. |
| 196 | `196-vinyl-tracing-freq-max.als` | Tracing Freq: drag fully up. |
| 197 | `197-vinyl-tracing-width-min.als` | Tracing Width: drag fully down. |
| 198 | `198-vinyl-tracing-width-max.als` | Tracing Width: drag fully up. |
| 200 | `200-vinyl-pinch-on.als` | Pinch section on. |
| 201 | `201-vinyl-pinch-drive-max.als` | From 200: Pinch Drive fully up. |
| 202 | `202-vinyl-pinch-freq-1k.als` | From 200: Pinch Freq `1000`. |
| 203 | `203-vinyl-pinch-width-max.als` | From 200: Pinch Width fully up. |
| 204 | `204-vinyl-pinch-mono.als` | From 200: pinch Stereo/Mono switch to Mono. |
| 205 | `205-vinyl-hard.als` | Soft/Hard switch to Hard. |
| 206 | `206-vinyl-crackle-volume-max.als` | Crackle Volume fully up. |
| 207 | `207-vinyl-crackle-density-min.als` | From 206: Crackle Density fully down. |
| 208 | `208-vinyl-crackle-density-max.als` | From 206: Crackle Density fully up. |

Crackle is random: two bounces of the same set do not match sample for sample. Bounce 206–208
twice (the second as `206-vinyl-crackle-volume-max-b.wav` and so on), so the density can be
measured from two takes.

### Overdrive (220–233)

| # | File name | Change from `220-overdrive-default.als` |
|---|---|---|
| 220 | `220-overdrive-default.als` | Overdrive added, nothing changed. |
| 221 | `221-overdrive-filter-freq-min.als` | Filter Freq: drag fully down. |
| 222 | `222-overdrive-filter-freq-max.als` | Filter Freq: drag fully up. |
| 223 | `223-overdrive-filter-width-min.als` | Filter Width: drag fully down. |
| 224 | `224-overdrive-filter-width-max.als` | Filter Width: drag fully up. |
| 225 | `225-overdrive-drive-0.als` | Drive: type `0`. |
| 226 | `226-overdrive-drive-50.als` | Drive: type `50`. |
| 227 | `227-overdrive-drive-100.als` | Drive: type `100`. |
| 228 | `228-overdrive-tone-0.als` | Tone: type `0`. |
| 229 | `229-overdrive-tone-100.als` | Tone: type `100`. |
| 230 | `230-overdrive-dynamics-0.als` | Dynamics: type `0`. |
| 231 | `231-overdrive-dynamics-100.als` | Dynamics: type `100`. |
| 232 | `232-overdrive-drywet-0.als` | Dry/Wet: type `0`. |
| 233 | `233-overdrive-drywet-50.als` | Dry/Wet: type `50`. |

If a control is named differently in your Live version, or a row's value cannot be typed, do
what is closest and say so in the log; the diff shows what was really stored.

### Log

`corpus/devices/README.md`:

```markdown
Live version: Live 12.x.x <edition>
OS:
Author:
Date:

| File | Done | Value Live shows | Bounce | Notes |
|---|---|---|---|---|
| 100-base.als | | | | |
| 110-utility-default.als | | | | |
| 111-utility-gain-minus12.als | | -12.0 dB | | |
```

### Test

`cargo test -p suhina-als --features corpus` also reads the device series and checks that
each Utility file imports as the Suhina utility with the values the file holds. It finds the
files in whichever project folder under `corpus/devices/` holds them.

### Extract and diff a device file

```sh
scripts/als-extract.sh corpus/devices/
P="corpus/devices/100-base Project"
git diff --no-index "$P/110-utility-default.als.xml" "$P/111-utility-gain-minus12.als.xml"
```

## Series 7: automation

Teaches the importer how Live stores automation: arrangement envelopes on the mixer, on device
parameters and on the tempo, their breakpoints and curves, the state when a control has been
moved by hand over its automation, and envelopes inside session clips. The arrangement files
also get a bounce, so Suhina's render of the same automation can be compared with Live's.

Like series 1–4, **each file is the previous one plus one edit**: automation is built up step
by step, and a diff between neighbours shows one change.

### Setup

1. Make the test signal if you have not (series 6, step 1).
2. Open `corpus/devices/…/100-base.als` (one audio track with the test signal at bar 1, warp off,
   tempo 120) and Save As `corpus/automation/300-base.als`. Live creates
   `corpus/automation/300-base Project/`; save the whole series there.
3. In Arrangement View, show the audio track's automation lane (the track's automation button,
   or press `A`). Choose envelopes with the lane's two menus: device (`Mixer`, `Utility`, …) and
   parameter (`Track Volume`, `Track Panning`, …).

**Placing a breakpoint at an exact value:** double-click the line at the bar to add a point,
then right-click the point › Edit Value and type the value. Positions are on bar lines, so
snap to the grid is enough. Log what you did if Live behaves differently.

Bounce the arrangement files (`301`–`314`) as in series 6 (Master, `1.1.1`, `4.0.0`, WAV 48 kHz
24-bit, no normalise, no dither) into `corpus/automation/bounces/` under the set's name. For
`311` (tempo) render 6 bars instead of 4, so the whole signal fits at the faster tempo.

### Arrangement automation (300–314)

| # | File name | Edit from the previous file |
|---|---|---|
| 300 | `300-base.als` | No edit: the series 6 base, saved here. |
| 301 | `301-volume-two-points.als` | Mixer › Track Volume: a point at bar 1 at `0` dB and one at bar 3 at `-12` dB. |
| 302 | `302-volume-third-point.als` | Add a point at bar 4 at `-inf` (drag it to the bottom). |
| 303 | `303-volume-curve.als` | Curve the segment between bar 1 and bar 3: hold Alt and drag the line downwards. Log what the curve looks like. |
| 304 | `304-pan.als` | Mixer › Track Panning: bar 1 `50L`, bar 3 `50R`. |
| 305 | `305-track-activator.als` | Mixer › Track Activator (the mute): off from bar 2, on again from bar 3. |
| 306 | `306-send.als` | Mixer › Send A (add one return track first if the base has none, and log it): bar 1 `-inf`, bar 2 `0` dB. |
| 307 | `307-device-gain.als` | Add Utility to the track (the device itself). Utility › Gain: bar 1 `0` dB, bar 3 `-12` dB. |
| 308 | `308-device-switch.als` | Utility › Mono: off at bar 1, on from bar 2. |
| 309 | `309-device-choice.als` | Utility › Channel Mode (the input menu): Stereo at bar 1, Left from bar 3. |
| 310 | `310-main-volume.als` | The Main track's Mixer › Track Volume: bar 1 `0` dB, bar 4 `-6` dB. |
| 311 | `311-tempo.als` | The Main track's Mixer › Song Tempo: bar 1 `120`, bar 3 `140`. Bounce 6 bars. |
| 312 | `312-overridden.als` | Remove the tempo envelope again (select it, Delete), then drag the audio track's volume fader by hand, so its automation is overridden (the Re-Enable Automation button lights up). |
| 313 | `313-re-enabled.als` | Click Re-Enable Automation. |
| 314 | `314-volume-cleared.als` | Right-click the volume lane › Clear Envelope (or select all its points and Delete). |

### Clip envelopes (320–323)

No bounces: Export renders the arrangement, not session clips.

| # | File name | Edit from the previous file |
|---|---|---|
| 320 | `320-session-clip.als` | In Launcher view, drag `test-signal.wav` into the audio track's first clip slot. Warp off. |
| 321 | `321-clip-volume-envelope.als` | Clip View › Envelopes, Mixer › Track Volume: a ramp from `0` dB at the clip's start to `-12` dB at its end. |
| 322 | `322-clip-device-envelope.als` | In the same clip, Utility › Gain: `0` dB at the start, `-12` dB halfway. |
| 323 | `323-clip-envelope-unlinked.als` | Unlink the clip's envelope (the Link switch in the envelope panel) and give it a loop of 1 bar. Log how. |

### Log

`corpus/automation/README.md`, as in series 6, with a column for the values Live showed on each
point.

## Series 8: Max devices

Teaches the importer how a Live Set refers to a Max for Live device: the file reference, the
stored parameter values, and what Collect All and Save does with the device file. Suhina will
never run these devices; it keeps them so a project loses nothing
([ADR 0008](../decisions/0008-patch-devices.md)). Needs Live Suite (or Max for Live).

Build the devices yourself, as below: no devices from Packs or the Core Library beyond the
empty templates.

### Setup

1. Open `corpus/devices/…/100-base.als` (one audio track with the test signal) and Save As
   `corpus/max/400-base.als`. Save the whole series in the project folder Live creates.
2. Make a folder outside the project, `corpus/max/devices/`, for the device files you save from
   Max.

### Files (400–407)

| # | File name | Edit from the previous file |
|---|---|---|
| 400 | `400-base.als` | No edit. |
| 401 | `401-max-audio-effect.als` | Browser › Max for Live › Max Audio Effect (the empty template) onto the audio track. Do not open the editor. |
| 402 | `402-edited-device.als` | Click the device's Edit button. In Max, put a `*~ 0.5` between `plugin~` and `plugout~` (both channels), save the device as `corpus/max/devices/suhina-half.amxd`, close Max. Log what Live asked. |
| 403 | `403-device-parameter.als` | Edit again: add a `live.dial` named `Amount` (range 0–1) feeding the `*~` right inlets, save the device, close Max. On the device in Live, type `0.25` into Amount. |
| 404 | `404-collect-all.als` | File › Collect All and Save (include everything Live offers), then Save As this name. Log which files Live copied, and where. |
| 405 | `405-moved-original.als` | Close Live. Rename `corpus/max/devices/` to `devices-moved/`. Open `404-collect-all.als`, check the device still loads (log it), Save As this name. Rename the folder back. |
| 406 | `406-max-midi-effect.als` | Add a MIDI track, put Browser › Max for Live › Max MIDI Effect (empty template) on it. |
| 407 | `407-device-off.als` | Turn the `suhina-half` device off with its activator. |

### Log

`corpus/max/README.md`, as in series 6, plus the Max version (Max › About Max) and, for 404 and
405, the paths Live used.

## Series 9: audio fades

Shows where a Live Set stores an arrangement audio clip's fade-in and fade-out, in which unit,
and how the curve is shaped, so the importer can read fades into `Clip::fade_in` and
`Clip::fade_out`, and their shapes and curves into `Clip::fade_in_curve` and
`Clip::fade_out_curve` ([arrangement editing](arrangement-editing.md) › Fades). Until then the importer
leaves fades at 0 and `als-format.md` lists them as an open question.

### Setup

1. Note whether Live's preferences create fades on clip edges by themselves, and log the
   setting. Leave it as it is for the whole series.
2. Open `35-audio-in-arrangement.als` (series 2: `loopwerk-demo.wav` at bar 1 of an audio track
   in Arrangement View) and Save As `corpus/als/500-fades-base.als`.
3. Set the arrangement grid to 1/4 (one beat), so dragged fades end exactly on beats.

### Files (500–506)

| # | File name | Edit from the previous file |
|---|---|---|
| 500 | `500-fades-base.als` | No edit. |
| 501 | `501-fade-in-1-beat.als` | Drag the clip's fade-in handle so the fade-in is exactly 1 beat long. Log how you made it exact (grid, or a field if Live shows one). |
| 502 | `502-fade-out-half-beat.als` | Set the grid to 1/8 and drag the fade-out handle so the fade-out is exactly half a beat. |
| 503 | `503-fade-in-2-beats.als` | Set the grid back to 1/4 and make the fade-in exactly 2 beats. |
| 504 | `504-fade-in-curve.als` | If the fade shows a curve handle, drag it to bend the fade-in. Log what you did, or "no curve control". |
| 505 | `505-fade-in-off.als` | Remove the fade-in (drag it back to the clip start). |
| 506 | `506-clip-shortened.als` | Drag the clip's end 1 beat to the left (the fade-out must stay half a beat). |

Bounce `501` and `502` (File › Export Audio/Video, the arrangement from bar 1 for the clip's
length, 48 kHz, 32-bit float, no normalising, no dither) as `501-bounce.wav` and
`502-bounce.wav`: comparing them with Suhina's render shows the curve's shape.

### Log

`corpus/als/README.md`, as in the Log section at the end, plus the fade preference from Setup, the exact fade
lengths Live shows, and the answer for 504.

## Series 10: sidechain (a device reading another track)

Teaches the importer how a Live Set stores a device that reads another track's signal — a sidechain or
key input. Suhina keeps that as an input on the device that reads it ([sidechain](sidechain.md) ›
Model), while the file may store it on the other side of the edge, as the source track's own output
routing into the destination **device**. This series exists to find out which, and what the file holds
for a group track, a return track, a deleted source and two keys on one track.

No bounces: this series is about reading a reference, not about a sound. One optional bounce of `602`
is in the log, to confirm by ear that the reference really keys the device.

### Setup

1. Make the test signal if you have not (series 6, step 1).
2. Open `corpus/devices/…/100-base.als` (one audio track with the test signal at bar 1, warp off,
   tempo 120) and Save As `corpus/als/600-sidechain-base.als`. Live creates
   `corpus/als/600-sidechain-base Project/`; save the whole series there.
3. Add a second audio track, drag the same test signal into its first Session slot and then into
   Arrangement View at bar 1, so both tracks play the same short tone. Log it if the set already has a
   track you can reuse instead.

### Files (600–611)

| # | File name | Edit from the previous file |
|---|---|---|
| 600 | `600-sidechain-base.als` | No edit: two audio tracks with the tone, nothing else. |
| 601 | `601-compressor.als` | Add the built-in **Compressor** to the **second** track (default settings, no preset). |
| 602 | `602-device-sidechain.als` | In the Compressor, open its sidechain section and choose the **first** track as the sidechain source with the device's own chooser. Log every entry that chooser offers, and whether a tap (pre/post) or a similar choice appears anywhere in it. |
| 603 | `603-other-source.als` | Change that chooser to the **second** track itself. Log whether its own track is offered at all. |
| 604 | `604-track-output-route.als` | From `601` (no device sidechain): set the **first track's output** to the second track, then pick the Compressor's sidechain input in the lower chooser. Log the exact entries that lower chooser lists. |
| 605 | `605-from-group.als` | From `602`: group the first track (select its header, Ctrl+G), then key the Compressor from the **group** track. |
| 606 | `606-from-return.als` | From `602`: turn the first track's Send A up, then key the Compressor from the **return** track (A). Log the return track's name. |
| 607 | `607-tap-variant.als` | From `602`: if the chooser offers a second tap or a pre/post choice, set the one that is not the default. If it offers none, save this file as a copy of `602` and write "no tap choice" in the log. |
| 608 | `608-source-deleted.als` | From `602`: delete the source track and save without fixing anything. |
| 609 | `609-two-keys.als` | From `602`: add a **second** Compressor after the first on the same track and key it from the other audio track, so one track holds two devices that each read a track. |
| 610 | `610-in-rack.als` | From `602`: wrap the Compressor in an audio effect rack (select the device, Ctrl+G) and key it from inside the rack. Log what the rack's own routing offers. |
| 611 | `611-from-chain.als` | From `610`: add a **second** chain to the rack (Chain List › drop area), put a Utility in it, and key the Compressor **from that chain** — the routing list should name the rack's chains. Log every entry it lists for a chain, and whether the three routing points of a chain (before its devices, after them, after its fader) appear as separate entries. |
| 612 | `612-sidechain-vst3.als` | From `601`: if any installed VST3 exposes a sidechain input in the program's own sidechain chooser, use that plug-in instead of the Compressor and log the chooser's entries. If none does, save a copy of `601` and write in the log that no installed plug-in exposed one — a negative result is a result. |

### Log

`corpus/als/README.md`, as in the Log section at the end, plus for every file what the choosers listed,
whether a tap choice appeared, and (optionally) a bounce of `602`'s first bar (48 kHz, 32-bit float) as
`602-bounce.wav`, to hear the ducking.

## Series 11: locators

Shows where a Live Set stores the arrangement's locators (named positions on the timeline) and
what each one holds, so the importer can read them into arrangement markers. Until then
`als-format.md` › Locators lists the structure as a hypothesis.

### Setup

1. Start from `00-base.als` (series 1) and Save As `corpus/als/700-locators-base.als`.
2. Switch to Arrangement View. Set the arrangement grid to one bar, so locators land on bars.

### Files (700–704)

| # | File name | Edit from the previous file |
|---|---|---|
| 700 | `700-locators-base.als` | No edit. |
| 701 | `701-locator-bar-5.als` | Put the insert marker at bar 5 and add a locator there (Create › Add Locator). |
| 702 | `702-locator-renamed.als` | Rename that locator to `Verse`. |
| 703 | `703-locator-bar-2.als` | Put the insert marker at bar 2 and add a second locator there, so the second one added is the earlier one. |
| 704 | `704-locator-moved.als` | Drag the `Verse` locator from bar 5 to bar 9. |

### Log

`corpus/als/README.md`, as in the Log section at the end, plus the default name Live gave each new
locator.

## Series 12: arrangement loop

Shows where a Live Set stores the arrangement's loop region and whether it is on, so the
importer can read them into the transport's loop region. Until then `als-format.md` › Arrangement
loop lists the structure as a hypothesis.

### Setup

1. Start from `00-base.als` (series 1) and Save As `corpus/als/800-loop-base.als`.
2. Switch to Arrangement View. Set the arrangement grid to one bar.

### Files (800–803)

| # | File name | Edit from the previous file |
|---|---|---|
| 800 | `800-loop-base.als` | No edit. |
| 801 | `801-loop-bars-5-9.als` | Set the loop brace to start at bar 5 and end at bar 9 (four bars), and turn the loop on. |
| 802 | `802-loop-off.als` | Turn the loop off; leave the brace where it is. |
| 803 | `803-loop-bars-3-4.als` | Drag the brace to start at bar 3 and set its length to one bar; turn the loop on again. |

### Log

`corpus/als/README.md`, as in the Log section at the end, plus the loop start and length the
transport bar showed for each file.

## Tips from the first run

- Live's default new set already has two MIDI tracks, two audio tracks, two returns and eight
  scenes. Edits that "add" one of these may not be needed; check the log of what was really done.
- Clip View controls only appear for the clip you double-clicked. Mixer controls in the track
  column change the **track**, not the clip.
- Typing values into fields (`-6`, `50L`, `2`) gives exact numbers; dragging gives values like
  `64.251297`.
- Saving before or after closing a panel moves view-state changes between files. Harmless, but
  log it.

## Extract and diff

Live puts the sets in the project folder it created on the first save (`00-base Project/`):

```sh
scripts/als-extract.sh corpus/als/
P="corpus/als/00-base Project"
git diff --no-index "$P/00-base.als.xml" "$P/00b-base-resaved.als.xml"
git diff --no-index "$P/00-base.als.xml" "$P/01-tempo-128.als.xml"
```

`code --diff a.xml b.xml` gives a side-by-side view in VS Code.

## Log

Copy this table to `corpus/als/README.md` and fill it in as you go.

```markdown
Live version: Live 12.x.x <edition>
OS:
Author:
Date:

| File | Done | Notes (anything unexpected, extra clicks, dialogs) |
|---|---|---|
| 00-base.als | | |
| 00b-base-resaved.als | | |
| 01-tempo-128.als | | |
```

## Later series

Not yet; each gets its own section when the importer needs it: groups, return tracks, follow actions, launch quantisation and
launch modes, MIDI CC and pitch-bend envelopes, comping and take lanes, and sets saved by older
Live versions.
