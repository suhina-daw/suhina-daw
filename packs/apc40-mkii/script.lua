-- The Akai APC40 MkII as a Suhina surface program: the 5x8 clip grid, the scene launch
-- buttons, the per-track stop, activator, solo, record-arm and select buttons, the transport's
-- play and metronome buttons, the device control section — the eight knobs on the selected
-- device's dials in banks of eight, Device and Bank left/right, Device On/Off, and the master
-- select —
-- and the mixer, the eight faders, the master fader and the pan encoders with their LED rings,
-- with feedback on every button.
--
-- Suhina's own, written from public documentation alone (the repository's clean-room rules):
-- Akai's public *APC40 Mk2 Communication Protocol v1.2* (January 19, 2015) for the note and CC
-- map, the RGB values and the Application/Configuration message that puts the device in the
-- mode the document names (0x41), and Akai's public MIDI implementation chart for the control
-- map. See README.md beside this file. Suhina content: CC0-1.0.

local TRACKS = 8
local SCENES = 5

-- The device wakes in its Generic mode, where the activator, solo and record-arm buttons are
-- latching toggles that drive their own lights. The Application/Configuration mode (0x41)
-- makes every button momentary — a press is always note-on 127, a release note-off —
-- and hands the buttons' LEDs to the host; the knobs' rings ride the hardware, and the script
-- writes a ring only for a value the knob did not just report, so a session value trailing a
-- turn cannot yank it back. The message is the Application/Configuration identifier of
-- the protocol document; its trailing bytes are the host application's version, as the
-- document and the public examples send. The mode lasts until the device is powered off, so it
-- goes out whenever the ports are (re)opened.
local LIVE_MODE = string.char(0xF0, 0x47, 0x7F, 0x29, 0x60, 0x00, 0x04, 0x41, 0x09, 0x07, 0x01, 0xF7)

-- The note that lights the pad of (track, scene): the chart numbers clip launch 1 at the
-- bottom-left, left to right, bottom row first — note 0 is track 1, scene 5 (the bottom), and
-- the top row (scene 1) is notes 32-39.
local function pad_note(track, scene)
    return 32 + track - 8 * scene
end

local function pad_of(note)
    local scene = 4 - math.floor(note / 8)
    return note - 32 + 8 * scene, scene
end

-- The pads' colours, by note-on velocity (the protocol document's RGB table): off, the two
-- greys, white, red, yellow and green.
local OFF, DIM, WHITE, RED, YELLOW, GREEN = 0, 2, 3, 5, 13, 21
-- The single-colour buttons (the per-track activator, solo and arm, play, metronome): the
-- chart's own values — 0 is off, 1 to 127 on. The track's stop button has its own table: 0
-- off, 1 lit, 2 triggered.
local ON, OFF_BUTTON, STOP_LIT, STOP_TRIGGERED = 127, 0, 1, 2
-- The A/B button has two colours, not the grid's: velocity 1 is yellow, 2 to 127 orange.
local CROSSFADE_A, CROSSFADE_B = 1, 2

-- The device control section (the protocol document's DEVICE KNOB 1-8 on CC 16-23, their ring
-- types on CC 24-31; DEVICE < > on notes 58-59, BANK < > on notes 60-61, DEV. ON/OFF on note
-- 62; the TRACK SELECT buttons on note 51 of each track's channel, MASTER on note 80).
local KNOBS = 8
local CROSSFADER = 15
local CROSSFADE_AB = 66
local TEMPO = 13
local CUE_LEVEL = 47
local PAN_MODE, SENDS_MODE, USER_MODE = 87, 88, 89
local RECORD, SESSION_RECORD, TAP_TEMPO, NUDGE_DOWN, NUDGE_UP = 93, 102, 99, 100, 101
local ARROW_UP, ARROW_DOWN, ARROW_RIGHT, ARROW_LEFT = 94, 95, 96, 97
-- How many selection looks after a knob's last report count as still turning: the ring is not
-- written while it turns, because a ring write resets an absolute encoder's own value and the
-- next report would jump.
local KNOB_SETTLE = 12
local DEVICE_LEFT, DEVICE_RIGHT, BANK_LEFT, BANK_RIGHT, DEVICE_ON_OFF = 58, 59, 60, 61, 62
local DEVICE_LOCK, CLIP_DEVICE_VIEW, DETAIL_VIEW = 63, 64, 65
local SHIFT, BANK_LOCK = 98, 103
local TRACK_SELECT, MASTER_SELECT = 51, 80

-- What the subscriptions last reported: the grid (which slots hold clips, which have stop
-- buttons), the launcher's state (what plays and what is queued, the transport), the tracks
-- (mute, solo, armed, pan, sends), the selection (the selected track and device, #1679), the
-- record state (whether the arrangement records), and the audio output (the cue's level).
local grid, state, strips, selection, record_state, audio_state = nil, nil, nil, nil, nil, nil

-- The first track and scene the 5x8 window shows, moved by the Bank Select arrows.
local track_offset, scene_offset = 0, 0
-- The window last reported to the window, so a box follows it without a command on every look.
local reported_window = nil

-- The encoders' mode: pan, a return track's sends, or the user's own. In sends mode the sends
-- are paged: pressing Sends again takes the next one.
local encoder_mode = "pan"
local send_index = 0
-- Whether the Session Record button arms empty slots to record on the pads.
local session_record = false
-- The taps whose spacing makes the tap tempo, in microseconds, at most the last four.
local tap_times = {}
-- The bank of eight parameters the device knobs show: 0 for the first eight, 1 for the next,
-- and back to 0 when another device is selected unless Bank Lock holds the bank.
local bank = 0
local bank_device = nil
local bank_locked = false

-- Device Lock keeps the device section on the device it was locked to, read live through
-- device.read while the selection moves elsewhere.
local device_locked = false
local locked_track, locked_path, locked_reading, locked_token, locked_id = nil, nil, nil, nil, nil
-- Shift is held while its button is down, for the arrow jumps and other combinations.
local shift_down = false

-- What each LED last had, so a refresh writes only what changed; and the output port.
local leds = {}
local out = nil

-- The subscriptions, by the token each returned.
local kinds = {}

-- JSON null crosses as suhina.null, which is neither nil nor false.
local function is_nil(value)
    return value == nil or value == suhina.null
end

-- The device the device section controls: the locked one, or the selected one.
local function selected_device()
    if device_locked and locked_reading ~= nil then
        return locked_reading
    end
    if is_nil(selection) or is_nil(selection.device) then
        return nil
    end
    return selection.device
end

-- The track whose device section is open: the locked one, or the selected one.
local function device_track()
    if device_locked and locked_track ~= nil then
        return locked_track
    end
    if is_nil(selection) then
        return nil
    end
    return selection.track
end

-- Lets go of a locked device: the lock button goes dark and the section follows the
-- selection again. A device that is removed or replaced does this by reporting no device.
local function unlock_device()
    device_locked = false
    if locked_token ~= nil then
        suhina.unsubscribe(locked_token)
        kinds[locked_token] = nil
    end
    locked_track, locked_path, locked_reading, locked_token, locked_id = nil, nil, nil, nil, nil
end

-- Every parameter the selected device lists, in the order its panel shows them.
local function device_params()
    local device = selected_device()
    if device == nil or is_nil(device.params) then
        return nil
    end
    return device.params
end

-- The controls a knob can turn: a built-in's continuous or whole-number parameter, an .sd
-- device's slider or knob, and a group's own continuous parameters. Switches, buttons and
-- choices stay on the panel, where a press is a press and not a knob's half turn.
local DIAL_CONTROLS = {
    knob = true,
    log_knob = true,
    slider = true,
    log_slider = true,
    number = true,
    vertical_slider = true,
    integer = true,
}

local function is_dial(param)
    if not is_nil(param.kind) then
        return param.kind == "continuous" or param.kind == "integer"
    end
    if not is_nil(param.control) then
        return DIAL_CONTROLS[param.control] == true
    end
    -- A group's parameters and an older reading carry no kind: a range is a dial.
    return type(param.min) == "number" and type(param.max) == "number"
end

-- A knob's value on a stepped or enumerated parameter, snapped to the step its panel draws.
local function stepped_value(param, value)
    if type(param.steps) == "number" and param.steps > 0 then
        local step = (param.max - param.min) / param.steps
        if step > 0 then
            value = param.min + math.floor((value - param.min) / step + 0.5) * step
        end
    end
    return math.max(param.min, math.min(param.max, value))
end

-- The selected device's dials, in the order its panel reads them: the reading's `dials` list
-- (left to right and top to bottom) when it has one, else the parameters' own order for an .sd
-- device, a plug-in or an older reading.
local function device_dials()
    local device = selected_device()
    if device == nil then
        return nil
    end
    local params = device_params()
    if params == nil then
        return nil
    end
    if not is_nil(device.dials) and #device.dials > 0 then
        local by_id = {}
        for _, param in ipairs(params) do
            by_id[tostring(param.id)] = param
        end
        local dials = {}
        for _, id in ipairs(device.dials) do
            local param = by_id[tostring(id)]
            if param ~= nil then
                dials[#dials + 1] = param
            end
        end
        return dials
    end
    local dials = {}
    for _, param in ipairs(params) do
        if is_dial(param) then
            dials[#dials + 1] = param
        end
    end
    return dials
end

-- The device a reading belongs to. A replaced device keeps its path, so its stable id is part
-- of the key too; an older reading without one still gets a key of its own.
local function device_identity(device, track)
    if device == nil then
        return nil
    end
    local key = tostring(track) .. ":" .. tostring(device.path)
    if not is_nil(device.device_id) then
        key = key .. "#" .. tostring(device.device_id)
    end
    return key
end

-- The device the bank belongs to, so another one starts at its first bank.
local function device_key()
    return device_identity(selected_device(), device_track())
end

-- The parameter a knob (0-7) shows in the open bank, or nil past the device's last one.
local function knob_param(knob)
    local dials = device_dials()
    if dials == nil then
        return nil
    end
    return dials[bank * KNOBS + knob + 1]
end

-- The last bank the device has: the one holding its last parameter.
local function last_bank()
    local dials = device_dials()
    if dials == nil or #dials == 0 then
        return 0
    end
    return math.floor((#dials - 1) / KNOBS)
end

-- Reconciles the bank with the device the section now controls: a new device starts at its
-- first bank unless Bank Lock holds it, and a held bank past the device's end moves to its last.
local function sync_bank()
    local key = device_key()
    if key ~= bank_device then
        bank_device = key
        if not bank_locked then
            bank = 0
        end
        local last = last_bank()
        if bank > last then
            bank = last
        end
    end
end

-- A parameter's position on its panel's dial as a ring position: the reading's normalized
-- position (0 to 1) where it has one, else its value across its range.
local function ring_position(param)
    if type(param.normalized) == "number" then
        return math.floor(math.max(0, math.min(1, param.normalized)) * 127 + 0.5)
    end
    local min, max, value = param.min, param.max, param.value
    if type(min) ~= "number" or type(max) ~= "number" or type(value) ~= "number" or max <= min then
        return 0
    end
    local position = (value - min) / (max - min) * 127
    if position < 0 then
        return 0
    elseif position > 127 then
        return 127
    end
    return math.floor(position + 0.5)
end

-- The project track and scene behind a strip or pad, once the Bank Select arrows have moved
-- the window.
local function window_track(track)
    return track_offset + track
end

local function window_scene(scene)
    return scene_offset + scene
end

local function strip_of(track)
    if is_nil(strips) then
        return nil
    end
    if not is_nil(grid) then
        local column = grid.tracks[window_track(track) + 1]
        if column == nil then
            -- The Launcher columns end here: the project's track list has groups and other
            -- columns the surface does not show, so its index cannot stand in for this one.
            return nil
        end
        for index = 1, #strips do
            if strips[index].id == column.track then
                return strips[index]
            end
        end
        return nil
    end
    return strips[window_track(track) + 1]
end

local function column_state(track)
    if is_nil(state) then
        return nil
    end
    return state.tracks[window_track(track) + 1]
end

-- The mixer's track under a surface index: the window's track, from its strip, or from the
-- grid's track list until the strips have arrived.
local function mixer_track(track)
    local strip = strip_of(track)
    if not is_nil(strip) then
        return strip.id
    end
    if is_nil(grid) then
        return nil
    end
    local column = grid.tracks[window_track(track) + 1]
    if is_nil(column) then
        return nil
    end
    return column.track
end

-- The most sends any strip has, which Sends pages through.
local function send_count()
    local most = 0
    for track = 0, TRACKS - 1 do
        local strip = strip_of(track)
        local sends = (strip ~= nil) and strip.sends or nil
        if type(sends) == "table" and #sends > most then
            most = #sends
        end
    end
    return most
end

-- The send at the open page on a strip, or nil when that track has none.
local function send_of(track)
    local strip = strip_of(track)
    local sends = (strip ~= nil) and strip.sends or nil
    if type(sends) ~= "table" then
        return nil
    end
    return sends[send_index + 1]
end

-- A send level in dB as its ring position, 0 at silence and 127 at 0 dB.
local function send_position(level_db)
    if is_nil(level_db) or level_db <= -60 then
        return 0
    end
    return math.floor((math.min(0, level_db) + 60) / 60 * 127 + 0.5)
end

-- The tempo the engine reports, or nil before its first reading.
local function reported_tempo()
    if state ~= nil and type(state.tempo_bpm) == "number" then
        return state.tempo_bpm
    end
    return nil
end

-- The tempo the engine plays now, read back after the previous click's command was answered.
-- A subscription can trail a burst or an edit elsewhere; the reading is the one base a
-- relative step must use, so values that repeat while feedback arrives cannot lose a step.
local function read_tempo()
    local value, error = suhina.read("launcher.state", {})
    if error == nil and type(value) == "table" and type(value.tempo_bpm) == "number" then
        return value.tempo_bpm
    end
    return nil
end

-- The tempo the engine plays, for the tempo knob's and the nudge buttons' next step.
local function current_tempo()
    return read_tempo() or reported_tempo() or 120
end

-- Sets the project tempo, limited to the tempo field's range and rounded to hundredths.
local function set_tempo(bpm, gesture)
    bpm = math.max(20, math.min(999, math.floor(bpm * 100 + 0.5) / 100))
    suhina.command("transport.setTempo", { bpm = bpm }, gesture)
end

-- The nudge buttons: a BPM a press, as the tempo field's own nudge does.
local function nudge_tempo(delta)
    set_tempo(current_tempo() + delta, "apc40-mkii nudge")
end

-- The tap tempo: the last taps' spacing, reset after a pause of two seconds.
local function tap_tempo(timestamp)
    local now = tonumber(timestamp) or 0
    if now <= 0 then
        return
    end
    if #tap_times > 0 and now - tap_times[#tap_times] > 2000000 then
        tap_times = {}
    end
    tap_times[#tap_times + 1] = now
    if #tap_times > 4 then
        table.remove(tap_times, 1)
    end
    if #tap_times < 2 then
        return
    end
    local span = tap_times[#tap_times] - tap_times[1]
    if span > 0 then
        set_tempo(60 * 1000000 * (#tap_times - 1) / span, "apc40-mkii tap tempo")
    end
end

-- The cue level an audio reading reports, or nil when it has no output yet.
local function cue_from(info)
    if info == nil or info.chosen == nil then
        return nil
    end
    local db = info.chosen.cue_db
    if is_nil(db) then
        return -60
    end
    if type(db) == "number" then
        return db
    end
    return nil
end

-- The cue level the output reports, or nil before its first reading.
local function reported_cue()
    return cue_from(audio_state)
end

-- The cue level now, read back after the previous click's command was answered, as the
-- tempo's relative steps are.
local function current_cue()
    local value, error = suhina.read("audio.info", {})
    if error == nil then
        local db = cue_from(value)
        if db ~= nil then
            return db
        end
    end
    return reported_cue()
end

-- The cue knob: a dB a click, from silence to the cue's +12 dB.
local function cue_step(delta)
    local current = current_cue()
    if current == nil then
        current = 0
    end
    local db = math.max(-60, math.min(12, current + delta))
    local level = "-inf"
    if db > -60 then
        level = math.floor(db * 10 + 0.5) / 10
    end
    suhina.command("output.setCueVolume", { db = level })
end

-- Whether a track's slot is the one its take records into, so a second press ends it.
local function slot_recording(track, scene)
    if record_state == nil or type(record_state.takes) ~= "table" then
        return false
    end
    for _, take in ipairs(record_state.takes) do
        if take.track == track and type(take.into) == "table" and take.into.scene == scene then
            return true
        end
    end
    return false
end

-- Whether a scene or arrangement take is recording, for the Session Record light.
local function session_recording()
    if session_record then
        return true
    end
    if record_state == nil or type(record_state.takes) ~= "table" then
        return false
    end
    for _, take in ipairs(record_state.takes) do
        if type(take.into) == "table" and not is_nil(take.into.scene) then
            return true
        end
    end
    return false
end

-- The protocol's RGB colour table, in velocity order 0 to 127: the palette the pads and scene
-- buttons draw. A track's or clip's colour is snapped to its nearest colour, so a pad shows the
-- colour the window shows rather than a fixed white (#1677).
local APC_RGB_HEX = ""
    .. "000000 1e1e1e 7f7f7f ffffff ff4c4c ff0000 590000 190000 ffbd6c ff5400 591d00 271b00 ffff4c ffff00 595900 191900 "
    .. "88ff4c 54ff00 1d5900 142b00 4cff4c 00ff00 005900 001900 4cff5e 00ff19 00590d 001902 4cff88 00ff55 00591d 001f12 "
    .. "4cffb7 00ff99 005935 001912 4cc3ff 00a9ff 004152 001019 4c88ff 0055ff 001d59 000819 4c4cff 0000ff 000059 000019 "
    .. "874cff 5400ff 190064 0f0030 ff4cff ff00ff 590059 190019 ff4c87 ff0054 59001d 220013 ff1500 993500 795100 436400 "
    .. "033900 005735 00547f 0000ff 00454f 2500cc 7f7f7f 202020 ff0000 bdff2d afed06 64ff09 108b00 00ff87 00a9ff 002aff "
    .. "3f00ff 7a00ff b21a7d 402100 ff4a00 88e106 72ff15 00ff00 3bff26 59ff71 38ffcc 5b8aff 3151c6 877fe9 d31dff ff005d "
    .. "ff7f00 b9b000 90ff00 835d07 392b00 144c10 0d5038 15152a 16205a 693c1c a8000a de513d d86a1c ffe126 9ee12f 67b50f "
    .. "1e1e30 dcff6b 80ffbd 9a99ff 8e66ff 404040 757575 e0ffff a00000 350000 1ad000 074200 b9b000 3f3100 b35f00 4b1502 "
local APC_RGB = {}
for hex in APC_RGB_HEX:gmatch("%x+") do
    APC_RGB[#APC_RGB + 1] = {
        tonumber(hex:sub(1, 2), 16),
        tonumber(hex:sub(3, 4), 16),
        tonumber(hex:sub(5, 6), 16),
    }
end

local color_cache = {}

-- A `#rrggbb` text as its three numbers, or nil for anything else.
local function hex_rgb(text)
    if type(text) ~= "string" or #text ~= 7 or text:sub(1, 1) ~= "#" then
        return nil
    end
    local r = tonumber(text:sub(2, 3), 16)
    local g = tonumber(text:sub(4, 5), 16)
    local b = tonumber(text:sub(6, 7), 16)
    if r == nil or g == nil or b == nil then
        return nil
    end
    return r, g, b
end

-- The palette velocity nearest a colour, by its squared distance.
local function nearest_color(r, g, b)
    local best, best_distance = WHITE, math.huge
    for velocity = 0, #APC_RGB - 1 do
        local color = APC_RGB[velocity + 1]
        local dr, dg, db = r - color[1], g - color[2], b - color[3]
        local distance = dr * dr + dg * dg + db * db
        if distance < best_distance then
            best, best_distance = velocity, distance
        end
    end
    return best
end

-- A clip's colour as a pad velocity: the nearest palette colour, dimmed when the clip is off.
local function pad_color(text, dim)
    local key = text .. (dim and " dim" or "")
    local known = color_cache[key]
    if known ~= nil then
        return known
    end
    local r, g, b = hex_rgb(text)
    local velocity = WHITE
    if r ~= nil then
        if dim then
            r, g, b = r * 0.25, g * 0.25, b * 0.25
        end
        velocity = nearest_color(r, g, b)
    end
    color_cache[key] = velocity
    return velocity
end

local function pad_velocity(track, scene)
    local project_scene = window_scene(scene)
    local column = column_state(track)
    if column ~= nil then
        if not is_nil(column.playing) and column.playing.scene == project_scene then
            return GREEN
        end
        if type(column.queued) == "table" and column.queued.scene == project_scene then
            return YELLOW
        end
    end
    if not is_nil(grid) then
        local column_grid = grid.tracks[window_track(track) + 1]
        if column_grid ~= nil then
            local slot = column_grid.slots and column_grid.slots[project_scene + 1] or nil
            if slot ~= nil and not is_nil(slot.clip) then
                local color = slot.color
                if not is_nil(color) and type(color) == "string" then
                    return pad_color(color, slot.enabled == false)
                end
                if slot.enabled == false then
                    return DIM
                end
                return WHITE
            end
        end
    end
    return OFF
end

local function stop_velocity(track)
    local column = column_state(track)
    if column == nil then
        return OFF
    end
    if not is_nil(column.playing) or type(column.queued) == "table" then
        return STOP_LIT
    end
    return OFF
end

-- Whether every clip in the scene's row plays, and there is one: the scene the window's
-- button calls playing, which lights its launch button and makes a press stop it.
local function scene_playing(scene)
    if is_nil(state) or is_nil(grid) then
        return false
    end
    local project_scene = window_scene(scene)
    local clips, playing = 0, 0
    -- The scene launches or stops every column in the project, not only the eight in view.
    for index = 1, #grid.tracks do
        local column_grid = grid.tracks[index]
        local slot = (column_grid ~= nil) and (column_grid.slots and column_grid.slots[project_scene + 1]) or nil
        if slot ~= nil and not is_nil(slot.clip) then
            clips = clips + 1
            local column = state.tracks[index]
            if column ~= nil and not is_nil(column.playing) and column.playing.scene == project_scene then
                playing = playing + 1
            end
        end
    end
    return clips > 0 and playing == clips
end

local function scene_velocity(scene)
    if scene_playing(scene) then
        return GREEN
    end
    if is_nil(state) then
        return OFF
    end
    local project_scene = window_scene(scene)
    for track = 0, TRACKS - 1 do
        local column = state.tracks[window_track(track) + 1]
        if column ~= nil and type(column.queued) == "table" and column.queued.scene == project_scene then
            return YELLOW
        end
    end
    return OFF
end

-- The activator lights while its track is heard (not muted); solo and record arm light while
-- they are on.
local function activator_velocity(track)
    local strip = strip_of(track)
    if strip == nil then
        return OFF
    end
    if strip.mute == true then
        return OFF_BUTTON
    end
    return ON
end

local function solo_velocity(track)
    local strip = strip_of(track)
    if strip == nil then
        return OFF
    end
    if strip.solo == true then
        return ON
    end
    return OFF_BUTTON
end

local function arm_velocity(track)
    local strip = strip_of(track)
    if strip == nil then
        return OFF
    end
    if strip.armed == true then
        return RED
    end
    return OFF_BUTTON
end

-- The crossfader A/B button's LED: yellow on A, orange on B, off when the track is off the
-- crossfader. A single LED, so off has no colour of its own.
local function crossfade_velocity(track)
    local strip = strip_of(track)
    if strip == nil then
        return OFF
    end
    if strip.crossfade_assign == "a" then
        return CROSSFADE_A
    elseif strip.crossfade_assign == "b" then
        return CROSSFADE_B
    end
    return OFF_BUTTON
end

local function play_velocity()
    if state ~= nil and state.running == true then
        return ON
    end
    return OFF_BUTTON
end

local function metronome_velocity()
    if state ~= nil and state.metronome == true then
        return ON
    end
    return OFF_BUTTON
end

-- One LED: the velocity that changed only. Note-on with velocity 0 is the pad's off.
local function led(channel, note, velocity)
    local key = "n" .. channel .. ":" .. note
    if leds[key] == velocity then
        return
    end
    leds[key] = velocity
    if out ~= nil then
        suhina.send(out, suhina.midi.note_on(channel, note, velocity))
    end
end

-- One ring LED: the ring's type goes on its own CC (0 off, 1 single, 2 volume style, 3 pan
-- spread), the position on the knob's CC, as the protocol document describes them.
local function led_cc(controller, value)
    local key = "c" .. controller
    if leds[key] == value then
        return
    end
    leds[key] = value
    if out ~= nil then
        suhina.send(out, suhina.midi.cc(0, controller, value))
    end
end

-- What each knob recently reported, six steps deep. The ring's own position rides the knob's
-- hardware, and the session's value trails the knob by a look or two: a session value matching
-- a recent report is that echo, not a change, and writing it yanks the ring backward mid-turn.
-- Only a value the knob never reported (a change made in the window or by a command) writes.
local knob_recent = {}
-- The dial each controller last showed, and how many looks it has left before it has settled.
local knob_shown = {}
local knob_turn = {}
-- The tracks the eight encoder rings showed last, so a project change that moves another
-- track under a knob forgets the old track's echo history just as scrolling does.
local encoder_tracks = nil

local function knob_reported(controller, value)
    local list = knob_recent[controller]
    if list == nil then
        list = { n = 0 }
        knob_recent[controller] = list
    end
    list.n = (list.n % 6) + 1
    list[list.n] = value
end

local function knob_saw(controller, value)
    local list = knob_recent[controller]
    if list == nil then
        return false
    end
    for i = 1, 6 do
        if list[i] == value then
            return true
        end
    end
    return false
end

-- A mode, send page or window move changes what an encoder's ring means: the positions it
-- reported as a pan are not echoes of its new send (or another track's pan). Forget both the
-- echo history and the last ring value we remember, so the next refresh writes the new target.
local function forget_encoder_rings()
    for track = 0, TRACKS - 1 do
        knob_recent[48 + track] = nil
        leds["c" .. tostring(48 + track)] = nil
    end
end

local function sync_encoder_rings()
    local tracks = {}
    for track = 0, TRACKS - 1 do
        tracks[#tracks + 1] = tostring(mixer_track(track))
    end
    local key = table.concat(tracks, ",")
    if key ~= encoder_tracks then
        encoder_tracks = key
        forget_encoder_rings()
    end
end

-- The device section's lights: each knob's ring shows its dial's position in the bank (the
-- single-dot style), the device arrows while a device is selected, the bank arrows where there
-- is another page, and Device On/Off whether the device is on.
local function refresh_device()
    sync_bank()
    local device = selected_device()
    local device_key = device_identity(device, device_track()) or ""
    for knob = 0, KNOBS - 1 do
        local controller = 16 + knob
        local param = knob_param(knob)
        if param ~= nil and type(param.min) == "number" and type(param.max) == "number" then
            led_cc(24 + knob, 1)
            local position = ring_position(param)
            local shown = device_key .. ":" .. tostring(param.id)
            local turning = knob_turn[controller] or 0
            -- A new dial is not the old knob's echo: put the ring at it. While the knob turns,
            -- leave the ring to the knob, so its next report is not reset elsewhere. Only a
            -- value from elsewhere writes, once the turn has settled.
            if shown ~= knob_shown[controller] then
                led_cc(controller, position)
                knob_reported(controller, position)
                knob_turn[controller] = 0
            elseif turning > 0 then
                knob_turn[controller] = turning - 1
            elseif not knob_saw(controller, position) then
                led_cc(controller, position)
                knob_reported(controller, position)
            end
            knob_shown[controller] = shown
        else
            led_cc(24 + knob, 0)
            led_cc(16 + knob, 0)
            knob_shown[controller] = nil
            knob_turn[controller] = nil
        end
    end
    led(0, DEVICE_LEFT, device ~= nil and ON or OFF_BUTTON)
    led(0, DEVICE_RIGHT, device ~= nil and ON or OFF_BUTTON)
    led(0, BANK_LEFT, bank > 0 and ON or OFF_BUTTON)
    led(0, BANK_RIGHT, bank < last_bank() and ON or OFF_BUTTON)
    led(0, DEVICE_ON_OFF, (device ~= nil and device.enabled == true) and ON or OFF_BUTTON)
    led(0, DEVICE_LOCK, device_locked and ON or OFF_BUTTON)
    led(0, BANK_LOCK, bank_locked and ON or OFF_BUTTON)
end

-- Tells the window which slots this surface sees, for its outline in the Launcher.
local function report_window()
    if is_nil(grid) then
        return
    end
    local key = tostring(grid) .. ":" .. tostring(track_offset) .. ":" .. tostring(scene_offset)
    if key == reported_window then
        return
    end
    reported_window = key
    suhina.command("surface.setWindow", {
        id = "apc40-mkii",
        name = "APC40 MkII",
        track = track_offset,
        scene = scene_offset,
        tracks = 8,
        scenes = 5,
    })
end

local function max_track_offset()
    if is_nil(grid) then
        return 0
    end
    return math.max(0, #grid.tracks - TRACKS)
end

local function max_scene_offset()
    if is_nil(grid) then
        return 0
    end
    return math.max(0, #grid.scenes - SCENES)
end

local function clamp_window()
    track_offset = math.max(0, math.min(track_offset, max_track_offset()))
    scene_offset = math.max(0, math.min(scene_offset, max_scene_offset()))
end

local function refresh()
    clamp_window()
    sync_encoder_rings()
    for track = 0, TRACKS - 1 do
        for scene = 0, SCENES - 1 do
            led(0, pad_note(track, scene), pad_velocity(track, scene))
        end
        led(track, 52, stop_velocity(track))
        led(track, 50, activator_velocity(track))
        led(track, 49, solo_velocity(track))
        led(track, 48, arm_velocity(track))
        led(track, CROSSFADE_AB, crossfade_velocity(track))
        -- The track select buttons are radio style: the selected track's is lit.
        local id = mixer_track(track)
        led(track, TRACK_SELECT, (id ~= nil and not is_nil(selection) and selection.track == id) and ON or OFF_BUTTON)
        -- The encoders' ring: the pan's spread in pan mode, a send's level in sends mode, and
        -- nothing in user mode, where the encoders are not the script's.
        local strip = strip_of(track)
        if encoder_mode == "pan" then
            led_cc(56 + track, 3)
            local pan = (strip ~= nil and not is_nil(strip.pan)) and strip.pan or 0
            local position
            if pan <= -1 then
                position = 0
            elseif pan >= 1 then
                position = 127
            else
                position = math.floor(pan * 63.5 + 64 + 0.5)
            end
            if not knob_saw(48 + track, position) then
                led_cc(48 + track, position)
            end
        elseif encoder_mode == "sends" then
            local send = send_of(track)
            if send ~= nil then
                led_cc(56 + track, 2)
                local position = send_position(send.level_db)
                if not knob_saw(48 + track, position) then
                    led_cc(48 + track, position)
                end
            else
                led_cc(56 + track, 0)
                led_cc(48 + track, 0)
            end
        else
            led_cc(56 + track, 0)
            led_cc(48 + track, 0)
        end
    end
    led(0, PAN_MODE, encoder_mode == "pan" and ON or OFF_BUTTON)
    led(0, SENDS_MODE, encoder_mode == "sends" and ON or OFF_BUTTON)
    led(0, USER_MODE, encoder_mode == "user" and ON or OFF_BUTTON)
    led(0, RECORD, (record_state ~= nil and record_state.arrangement == true) and ON or OFF_BUTTON)
    led(0, SESSION_RECORD, session_recording() and ON or OFF_BUTTON)
    for scene = 0, SCENES - 1 do
        led(0, 82 + scene, scene_velocity(scene))
    end
    led(0, 91, play_velocity())
    led(0, 90, metronome_velocity())
    led(0, MASTER_SELECT, (not is_nil(selection) and selection.track_name == "Main") and ON or OFF_BUTTON)
    refresh_device()
    report_window()
end

local function scroll(track_delta, scene_delta)
    track_offset = track_offset + track_delta
    scene_offset = scene_offset + scene_delta
    refresh()
end

-- A press: pads and scenes launch, the track buttons act, the transport runs.
local function press(channel, note, timestamp)
    if channel == 0 and note >= 0 and note <= 39 then
        local track, scene = pad_of(note)
        local id = mixer_track(track)
        if id == nil or is_nil(grid) then
            return
        end
        local project_scene = window_scene(scene)
        local column = grid.tracks[window_track(track) + 1]
        local slot = (column ~= nil) and (column.slots and column.slots[project_scene + 1] or nil) or nil
        if slot == nil then
            return
        end
        -- A second press on the slot a take records into ends it; an empty slot records on an
        -- armed track while Session Record is armed; otherwise an empty one with a stop button
        -- stops the track, and one without does nothing.
        if is_nil(slot.clip) then
            if slot_recording(id, project_scene) then
                suhina.command("clip.launch", { track = id, scene = project_scene })
                return
            end
            local strip = strip_of(track)
            if session_record and strip ~= nil and strip.armed == true then
                suhina.command("clip.record", { track = id, scene = project_scene })
                return
            end
            if slot.stop_button ~= true then
                return
            end
        end
        suhina.command("clip.launch", { track = id, scene = project_scene })
    elseif channel == 0 and note >= 82 and note <= 86 then
        local scene = note - 82
        local row = (not is_nil(grid)) and grid.scenes[window_scene(scene) + 1] or nil
        if is_nil(row) then
            return
        end
        -- A lit scene launch button stops its scene; a blank one launches it.
        if scene_playing(scene) then
            suhina.command("scene.stop", { scene = { id = row.scene_id } })
        else
            suhina.command("scene.launch", { scene = { id = row.scene_id } })
        end
    elseif channel == 0 and note == 91 then
        if state ~= nil and state.running == true then
            suhina.command("transport.stop")
        else
            suhina.command("transport.play")
        end
    elseif channel == 0 and note == 90 then
        suhina.command("transport.setMetronome", { on = not (state ~= nil and state.metronome == true) })
    elseif channel == 0 and note == PAN_MODE then
        if encoder_mode ~= "pan" then
            forget_encoder_rings()
        end
        encoder_mode = "pan"
        refresh()
    elseif channel == 0 and note == SENDS_MODE then
        local before_mode, before_index = encoder_mode, send_index
        if encoder_mode == "sends" then
            local count = send_count()
            if count > 0 then
                send_index = (send_index + 1) % count
            end
        else
            encoder_mode = "sends"
            send_index = 0
        end
        if before_mode ~= "sends" or send_index ~= before_index then
            forget_encoder_rings()
        end
        refresh()
    elseif channel == 0 and note == USER_MODE then
        if encoder_mode ~= "user" then
            forget_encoder_rings()
        end
        encoder_mode = "user"
        refresh()
    elseif channel == 0 and note == RECORD then
        suhina.command("transport.record", { on = not (record_state ~= nil and record_state.arrangement == true) })
    elseif channel == 0 and note == SESSION_RECORD then
        session_record = not session_record
        refresh()
    elseif channel == 0 and note == TAP_TEMPO then
        tap_tempo(timestamp)
    elseif channel == 0 and note == NUDGE_DOWN then
        nudge_tempo(-1)
    elseif channel == 0 and note == NUDGE_UP then
        nudge_tempo(1)
    elseif channel == 0 and note == ARROW_UP then
        scroll(0, shift_down and -5 or -1)
    elseif channel == 0 and note == ARROW_DOWN then
        scroll(0, shift_down and 5 or 1)
    elseif channel == 0 and note == ARROW_LEFT then
        scroll(shift_down and -8 or -1, 0)
    elseif channel == 0 and note == ARROW_RIGHT then
        scroll(shift_down and 8 or 1, 0)
    elseif channel == 0 and note == DEVICE_LEFT then
        suhina.command("device.selectPrevious")
    elseif channel == 0 and note == DEVICE_RIGHT then
        suhina.command("device.selectNext")
    elseif channel == 0 and note == BANK_LEFT then
        if bank > 0 then
            bank = bank - 1
            refresh()
        end
    elseif channel == 0 and note == BANK_RIGHT then
        if bank < last_bank() then
            bank = bank + 1
            refresh()
        end
    elseif channel == 0 and note == DEVICE_ON_OFF then
        local device = selected_device()
        if device ~= nil then
            suhina.command("device.setEnabled", {
                track = device_track(),
                device = device.path,
                enabled = device.enabled ~= true,
            })
        end
    elseif channel == 0 and note == DEVICE_LOCK then
        if device_locked then
            unlock_device()
        else
            local device = selected_device()
            if device ~= nil and not is_nil(selection) then
                locked_track = selection.track
                locked_path = device.path
                locked_reading = device
                locked_id = device.device_id
                device_locked = true
                if locked_token ~= nil then
                    suhina.unsubscribe(locked_token)
                    kinds[locked_token] = nil
                end
                locked_token = suhina.subscribe("device.read", { track = locked_track, device = locked_path })
                kinds[locked_token] = "locked_device"
            end
        end
        refresh()
    elseif channel == 0 and note == BANK_LOCK then
        bank_locked = not bank_locked
        refresh()
    elseif channel == 0 and note == CLIP_DEVICE_VIEW then
        suhina.command("view.devices", {})
    elseif channel == 0 and note == DETAIL_VIEW then
        suhina.command("view.detail", {})
    elseif channel == 0 and note == SHIFT then
        shift_down = true
    elseif channel == 0 and note == MASTER_SELECT then
        suhina.command("track.select", { track = "main" })
    elseif channel >= 0 and channel < TRACKS then
        local id = mixer_track(channel)
        if id == nil then
            return
        end
        local strip = strip_of(channel)
        if note == 52 then
            suhina.command("track.stop", { track = id })
        elseif note == 50 then
            local mute = not (strip ~= nil and strip.mute == true)
            suhina.command("track.setMute", { tracks = { id }, mute = mute })
        elseif note == 49 then
            local solo = not (strip ~= nil and strip.solo == true)
            suhina.command("track.setSolo", { tracks = { id }, solo = solo })
        elseif note == 48 then
            -- Record arm takes one track, not a list.
            local armed = not (strip ~= nil and strip.armed == true)
            suhina.command("track.setArm", { track = id, armed = armed })
        elseif note == CROSSFADE_AB then
            -- A, B, then off, as the strip's button does.
            local assign = strip ~= nil and strip.crossfade_assign or "off"
            local next
            if assign == "a" then
                next = "b"
            elseif assign == "b" then
                next = "off"
            else
                next = "a"
            end
            suhina.command("track.setCrossfadeAssign", { tracks = { id }, assign = next })
        elseif note == TRACK_SELECT then
            suhina.command("track.select", { track = id })
        end
    end
end

-- A fader or an encoder: the mixer. The fader's bottom is silence, its top +6 dB; the pan
-- encoder's centre is centre. One gesture per control, so a move is one undo step.
local function cc_received(channel, controller, value)
    if controller == 7 and channel >= 0 and channel < TRACKS then
        local id = mixer_track(channel)
        if id == nil then
            return
        end
        local db = "-inf"
        if value > 0 then
            db = math.floor((-60 + (value / 127) * 66) * 10) / 10
        end
        suhina.command("track.setVolume", { tracks = { id }, db = db }, "apc40-mkii volume " .. id)
    elseif controller >= 16 and controller <= 23 and channel == 0 then
        -- The device knobs: the bank's dials, the knob's 0-127. A dial with a normalized
        -- position (a built-in's, from its panel) sends that position, so a logarithmic time
        -- moves smoothly; an older reading's dial scales the knob onto its range instead. One
        -- gesture per knob and parameter, so a turn is one undo step.
        local device = selected_device()
        local param = knob_param(controller - 16)
        if device == nil or param == nil or type(param.min) ~= "number" or type(param.max) ~= "number" then
            return
        end
        local args = {
            track = device_track(),
            device = device.path,
            -- A built-in's id is text, a plug-in's a number: the command takes text.
            param = tostring(param.id),
        }
        if type(param.normalized) == "number" then
            args.normalized = value / 127
        else
            args.value = stepped_value(param, param.min + (value / 127) * (param.max - param.min))
        end
        suhina.command("device.setParam", args, "apc40-mkii device " .. tostring(device_track()) .. ":" .. tostring(device.path) .. ":" .. tostring(param.id))
        knob_reported(controller, value)
        knob_turn[controller] = KNOB_SETTLE
    elseif controller == CROSSFADER and channel == 0 then
        suhina.command("mixer.setCrossfader", { value = value / 127 }, "apc40-mkii crossfader")
    elseif controller >= 48 and controller <= 55 and channel == 0 then
        local track = controller - 48
        local id = mixer_track(track)
        if id == nil then
            return
        end
        if encoder_mode == "pan" then
            local pan = (value - 64) / 63.5
            if pan > 1 then
                pan = 1
            elseif pan < -1 then
                pan = -1
            end
            suhina.command("track.setPan", { tracks = { id }, pan = pan }, "apc40-mkii pan " .. id)
            knob_reported(controller, value)
        elseif encoder_mode == "sends" then
            local send = send_of(track)
            local target = (send ~= nil) and send["return"] or nil
            if send == nil or is_nil(target) then
                return
            end
            local db = "-inf"
            if value > 0 then
                db = math.floor((-60 + (value / 127) * 60) * 10) / 10
            end
            suhina.command(
                "track.setSend",
                { track = id, ["return"] = target, level = db },
                "apc40-mkii send " .. id
            )
            knob_reported(controller, value)
        end
        -- User mode leaves the encoders to the user: the script sends nothing for them.
    elseif controller == TEMPO and channel == 0 then
        -- The tempo knob is relative: 1 to 63 up, 127 down to 64 down.
        local delta = value
        if delta > 63 then
            delta = delta - 128
        end
        if delta ~= 0 then
            set_tempo(current_tempo() + delta, "apc40-mkii tempo")
        end
    elseif controller == CUE_LEVEL and channel == 0 then
        local delta = value
        if delta > 63 then
            delta = delta - 128
        end
        if delta ~= 0 then
            cue_step(delta)
        end
    elseif controller == 14 then
        -- The master fader (CC 14): the main output's volume, the same curve.
        local db = "-inf"
        if value > 0 then
            db = math.floor((-60 + (value / 127) * 66) * 10) / 10
        end
        suhina.command("mixer.setMainVolume", { db = db }, "apc40-mkii main volume")
    end
end

-- A button letting go; Shift is the one whose held state matters.
local function release(channel, note)
    if channel == 0 and note == SHIFT then
        shift_down = false
    end
end

suhina.on_port_message(function(bytes, port, timestamp)
    local message = suhina.midi.parse(bytes)
    if message == nil then
        return
    end
    if message.kind == "note_on" and message.velocity > 0 then
        press(message.channel, message.note, timestamp)
    elseif message.kind == "note_off" or (message.kind == "note_on" and message.velocity == 0) then
        release(message.channel, message.note)
    elseif message.kind == "cc" then
        cc_received(message.channel, message.controller, message.value)
    end
end)

suhina.on_state(function(token, value)
    local kind = kinds[token]
    if kind == "grid" then
        grid = value
    elseif kind == "state" then
        state = value
    elseif kind == "strips" then
        strips = value
    elseif kind == "selection" then
        selection = value
    elseif kind == "locked_device" then
        if device_locked then
            local same = not is_nil(value)
            if same and not is_nil(locked_id) and not is_nil(value.device_id) and value.device_id ~= locked_id then
                same = false
            end
            if same then
                locked_reading = value
            else
                unlock_device()
            end
        end
    elseif kind == "record" then
        record_state = value
    elseif kind == "audio" then
        audio_state = value
    end
    refresh()
end)

suhina.on_ports(function(inputs, outputs)
    out = outputs[1]
    if out ~= nil then
        suhina.send(out, LIVE_MODE)
    end
    -- The device may hold LEDs from before: send every control again, whatever changed.
    leds = {}
    refresh()
end)

kinds[suhina.subscribe("project.launcher")] = "grid"
kinds[suhina.subscribe("launcher.state")] = "state"
kinds[suhina.subscribe("project.tracks")] = "strips"
kinds[suhina.subscribe("project.selection")] = "selection"
kinds[suhina.subscribe("record.state")] = "record"
kinds[suhina.subscribe("audio.info")] = "audio"
