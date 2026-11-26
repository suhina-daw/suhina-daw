# APC40 MkII — Suhina's surface program

The Akai APC40 MkII as a Suhina surface program: the 5x8 clip grid, the scene launch buttons,
the per-track stop, activator (mute), solo, record-arm and select buttons, the transport's play
and metronome buttons, the device control section — the eight knobs on the selected device's
dials in banks of eight, Device and Bank left/right, Device On/Off, and the master select — and
the
mixer, the eight track faders, the master fader and the pan encoders with their LED rings, with
feedback on every button.

Suhina ships it: the files ride inside the executable and unpack into the library's
`Scripts/MIDI/apc40-mkii/` folder, and the app installs the program into its extensions store so
the port scan starts it for the device it names (`docs/spec/shipped-scripts.md`).

## Where the knowledge comes from

Suhina's own, written from public documentation alone — the repository's clean-room rules:

- **Akai's public *APC40 Mk2 Communication Protocol v1.2*** (January 19, 2015): the assignment
  of note numbers and controller numbers to the controls and LEDs, the RGB colours by velocity,
  the place of the per-track buttons (notes 48–52 on the track's own MIDI channel), and the
  **Application/Configuration message** the script sends when its ports open — the one that puts
  the device in the mode the document names Ableton Live (0x41): every button momentary, a
  press always note-on 127 with note-off on release, and the buttons' LEDs the host's. The
  knobs' rings ride the hardware; the script writes a ring only for a value the knob did not
  just report, so a session value trailing a turn cannot yank it back. The device wakes in its
  Generic mode, where the activator, solo and record-arm buttons latch and drive their own
  lights; the message is sent at every start, as the document prescribes.
- **Akai's public APC40 MkII MIDI implementation chart**: the control map the notation follows.

No part of it comes from another product's code, content or script. Suhina's own content:
**CC0-1.0** (the SPDX line in `script.lua`; `license` in `extension.json`). Suhina has no
affiliation with Akai, and the name says what hardware the script is for — no endorsement.

## What it wires, and what it does not yet

Wired: the clip grid (`clip.launch`, a slot's stop button for an empty slot), the scene
buttons (`scene.launch`), the track stop, the activator (`track.setMute`), solo
(`track.setSolo`), record arm (`track.setArm`), the track select buttons and master select
(`track.select`), play/stop and the metronome, the eight faders (`track.setVolume`, one gesture
each), the master fader (`mixer.setMainVolume`) and the pan encoders (`track.setPan`, one
gesture each) with their rings.

The device control section follows the selection the window, commands and agents share
(`project.selection`, #1679): the eight knobs turn the selected device's dials — its continuous
parameters — in banks of eight (`device.setParam`, one gesture each), Device left/right step
along the chain (`device.selectPrevious`, `device.selectNext`), Bank left/right take the next or
previous eight dials, and Device On/Off switches the device (`device.setEnabled`). Switches and
choices stay on the device's panel. Each knob's ring shows its dial's position, the bank arrows
light where there is another page, the device arrows while a device is selected, and the track
select buttons light for the selected track.

Not yet: the sends, user mode, crossfader, Device Lock, the Clip/Dev and Detail View buttons,
the tempo encoder, nudge, tap tempo, session record and shift; blinking queued pads; and the
clip's own colour on its pad (a slot with a clip lights white, its track's states light green,
yellow, red and grey).
