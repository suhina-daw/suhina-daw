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

-- The device control section (the protocol document's DEVICE KNOB 1-8 on CC 16-23, their ring
-- types on CC 24-31; DEVICE < > on notes 58-59, BANK < > on notes 60-61, DEV. ON/OFF on note
-- 62; the TRACK SELECT buttons on note 51 of each track's channel, MASTER on note 80).
local KNOBS = 8
-- How many selection looks after a knob's last report count as still turning: the ring is not
-- written while it turns, because a ring write resets an absolute encoder's own value and the
-- next report would jump.
local KNOB_SETTLE = 12
local DEVICE_LEFT, DEVICE_RIGHT, BANK_LEFT, BANK_RIGHT, DEVICE_ON_OFF = 58, 59, 60, 61, 62
local TRACK_SELECT, MASTER_SELECT = 51, 80

-- What the subscriptions last reported: the grid (which slots hold clips, which have stop
-- buttons), the launcher's state (what plays and what is queued, the transport), the tracks
-- (mute, solo, armed, pan), and the selection (the selected track and device, #1679).
local grid, state, strips, selection = nil, nil, nil, nil

-- The bank of eight parameters the device knobs show: 0 for the first eight, 1 for the next,
-- and back to 0 when another device is selected.
local bank = 0
local bank_device = nil

-- What each LED last had, so a refresh writes only what changed; and the output port.
local leds = {}
local out = nil

-- The subscriptions, by the token each returned.
local kinds = {}

-- JSON null crosses as suhina.null, which is neither nil nor false.
local function is_nil(value)
    return value == nil or value == suhina.null
end

-- The selected device from the selection reading, or nil with nothing selected.
local function selected_device()
    if is_nil(selection) or is_nil(selection.device) then
        return nil
    end
    return selection.device
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

-- The device the bank belongs to, so another one starts at its first bank.
local function device_key()
    local device = selected_device()
    if device == nil then
        return nil
    end
    return tostring(selection.track) .. ":" .. tostring(device.path)
end

-- The parameter a knob (0-7) shows in the open bank, or nil past the device's last one.
local function knob_param(dials, knob)
    if dials == nil then
        return nil
    end
    return dials[bank * KNOBS + knob + 1]
end

-- The last bank the device has: the one holding its last parameter.
local function last_bank(dials)
    if dials == nil or #dials == 0 then
        return 0
    end
    return math.floor((#dials - 1) / KNOBS)
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

local function strip_of(track)
    if is_nil(strips) then
        return nil
    end
    return strips[track + 1]
end

local function column_state(track)
    if is_nil(state) then
        return nil
    end
    return state.tracks[track + 1]
end

-- The mixer's track under a surface index: project.tracks counts every track, so a return
-- track (which has no launcher column) still has the fader, the pan and the buttons of its
-- index; the grid's track list stands in until the strips have arrived.
local function mixer_track(track)
    local strip = strip_of(track)
    if not is_nil(strip) then
        return strip.id
    end
    if is_nil(grid) then
        return nil
    end
    local column = grid.tracks[track + 1]
    if is_nil(column) then
        return nil
    end
    return column.track
end

local function pad_velocity(track, scene)
    local column = column_state(track)
    if column ~= nil then
        if not is_nil(column.playing) and column.playing.scene == scene then
            return GREEN
        end
        if type(column.queued) == "table" and column.queued.scene == scene then
            return YELLOW
        end
    end
    if not is_nil(grid) then
        local column_grid = grid.tracks[track + 1]
        if column_grid ~= nil then
            local slot = column_grid.slots and column_grid.slots[scene + 1] or nil
            if slot ~= nil and not is_nil(slot.clip) then
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

local function scene_velocity(scene)
    if is_nil(state) then
        return OFF
    end
    for track = 0, TRACKS - 1 do
        local column = state.tracks[track + 1]
        if column ~= nil and not is_nil(column.playing) and column.playing.scene == scene then
            return GREEN
        end
    end
    for track = 0, TRACKS - 1 do
        local column = state.tracks[track + 1]
        if column ~= nil and type(column.queued) == "table" and column.queued.scene == scene then
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

-- The device section's lights: each knob's ring shows its dial's position in the bank (the
-- single-dot style), the device arrows while a device is selected, the bank arrows where there
-- is another page, and Device On/Off whether the device is on.
local function refresh_device()
    local device = selected_device()
    local device_key = device ~= nil and (tostring(selection.track) .. ":" .. tostring(device.path)) or ""
    -- The dials once for the whole bank: rebuilding the reading's lookup table for each of the
    -- eight knobs and the bank arrow is the same answer nine times.
    local dials = device_dials()
    for knob = 0, KNOBS - 1 do
        local controller = 16 + knob
        local param = knob_param(dials, knob)
        if param ~= nil and type(param.min) == "number" and type(param.max) == "number" then
            led_cc(24 + knob, 1)
            local position = ring_position(param)
            local shown = device_key .. ":" .. tostring(param.id)
            local turning = knob_turn[controller] or 0
            -- A new dial is not the old knob's echo: put the ring at it. While the knob turns,
            -- leave the ring to the knob, so its next report is not reset elsewhere. Only a
            -- value from elsewhere writes, once the turn has settled.
            if shown ~= knob_shown[controller] then
                -- Another dial's reports are no guard for this one.
                knob_recent[controller] = nil
                led_cc(controller, position)
                knob_reported(controller, position)
                knob_turn[controller] = 0
            elseif turning > 0 then
                -- The turn has settled: forget its reports, so a later change that happens to
                -- land on one of those positions still writes the ring.
                if turning == 1 then
                    knob_recent[controller] = nil
                end
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
    led(0, BANK_RIGHT, bank < last_bank(dials) and ON or OFF_BUTTON)
    led(0, DEVICE_ON_OFF, (device ~= nil and device.enabled == true) and ON or OFF_BUTTON)
end

local function refresh()
    for track = 0, TRACKS - 1 do
        for scene = 0, SCENES - 1 do
            led(0, pad_note(track, scene), pad_velocity(track, scene))
        end
        led(track, 52, stop_velocity(track))
        led(track, 50, activator_velocity(track))
        led(track, 49, solo_velocity(track))
        led(track, 48, arm_velocity(track))
        -- The track select buttons are radio style: the selected track's is lit.
        local id = mixer_track(track)
        led(track, TRACK_SELECT, (id ~= nil and not is_nil(selection) and selection.track == id) and ON or OFF_BUTTON)
        -- The pan ring: pan spread style, then the position (0 at full left, 127 full right).
        led_cc(56 + track, 3)
        local strip = strip_of(track)
        local pan = (strip ~= nil and not is_nil(strip.pan)) and strip.pan or 0
        local position
        if pan <= -1 then
            position = 0
        elseif pan >= 1 then
            position = 127
        else
            position = math.floor(pan * 63.5 + 64 + 0.5)
        end
        -- The pan encoder's ring has the same echo guard as the device knobs: leave it to the
        -- encoder while it turns, then forget the reports so a later change still writes.
        local controller = 48 + track
        local turning = knob_turn[controller] or 0
        if turning > 0 then
            if turning == 1 then
                knob_recent[controller] = nil
            end
            knob_turn[controller] = turning - 1
        elseif not knob_saw(controller, position) then
            led_cc(controller, position)
        end
    end
    for scene = 0, SCENES - 1 do
        led(0, 82 + scene, scene_velocity(scene))
    end
    led(0, 91, play_velocity())
    led(0, 90, metronome_velocity())
    led(0, MASTER_SELECT, (not is_nil(selection) and selection.track_name == "Main") and ON or OFF_BUTTON)
    refresh_device()
end

-- A press: pads and scenes launch, the track buttons act, the transport runs.
local function press(channel, note)
    if channel == 0 and note >= 0 and note <= 39 then
        local track, scene = pad_of(note)
        local id = mixer_track(track)
        if id == nil or is_nil(grid) then
            return
        end
        local column = grid.tracks[track + 1]
        local slot = (column ~= nil) and (column.slots and column.slots[scene + 1] or nil) or nil
        if slot == nil then
            return
        end
        -- An empty slot with a stop button stops the track; an empty one without does nothing.
        if is_nil(slot.clip) and slot.stop_button ~= true then
            return
        end
        suhina.command("clip.launch", { track = id, scene = scene })
    elseif channel == 0 and note >= 82 and note <= 86 then
        local row = (not is_nil(grid)) and grid.scenes[note - 82 + 1] or nil
        if is_nil(row) then
            return
        end
        suhina.command("scene.launch", { scene = { id = row.scene_id } })
    elseif channel == 0 and note == 91 then
        if state ~= nil and state.running == true then
            suhina.command("transport.stop")
        else
            suhina.command("transport.play")
        end
    elseif channel == 0 and note == 90 then
        suhina.command("transport.setMetronome", { on = not (state ~= nil and state.metronome == true) })
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
        if bank < last_bank(device_dials()) then
            bank = bank + 1
            refresh()
        end
    elseif channel == 0 and note == DEVICE_ON_OFF then
        local device = selected_device()
        if device ~= nil then
            suhina.command("device.setEnabled", {
                track = selection.track,
                device = device.path,
                enabled = device.enabled ~= true,
            })
        end
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
        local param = knob_param(device_dials(), controller - 16)
        if device == nil or param == nil or type(param.min) ~= "number" or type(param.max) ~= "number" then
            return
        end
        local args = {
            track = selection.track,
            device = device.path,
            param = param.id,
        }
        if type(param.normalized) == "number" then
            args.normalized = value / 127
        else
            args.value = param.min + (value / 127) * (param.max - param.min)
        end
        suhina.command("device.setParam", args, "apc40-mkii device " .. tostring(selection.track) .. ":" .. tostring(device.path) .. ":" .. tostring(param.id))
        knob_reported(controller, value)
        knob_turn[controller] = KNOB_SETTLE
    elseif controller >= 48 and controller <= 55 and channel == 0 then
        local id = mixer_track(controller - 48)
        if id == nil then
            return
        end
        local pan = (value - 64) / 63.5
        if pan > 1 then
            pan = 1
        elseif pan < -1 then
            pan = -1
        end
        suhina.command("track.setPan", { tracks = { id }, pan = pan }, "apc40-mkii pan " .. id)
        knob_reported(controller, value)
        knob_turn[controller] = KNOB_SETTLE
    elseif controller == 14 then
        -- The master fader (CC 14): the main output's volume, the same curve.
        local db = "-inf"
        if value > 0 then
            db = math.floor((-60 + (value / 127) * 66) * 10) / 10
        end
        suhina.command("mixer.setMainVolume", { db = db }, "apc40-mkii main volume")
    end
end

suhina.on_port_message(function(bytes, port, timestamp)
    local message = suhina.midi.parse(bytes)
    if message == nil then
        return
    end
    if message.kind == "note_on" and message.velocity > 0 then
        press(message.channel, message.note)
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
        -- Another device starts at its first bank, as the controller's focus does.
        local key = device_key()
        if key ~= bank_device then
            bank_device = key
            bank = 0
        end
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

-- The controller's own quit reset: the host writes it when the app quits, so the session does
-- not have to know this device's LED map (docs/spec/surface-programs.md › Quitting). Every
-- note and controller off on every channel, as the script's own LED writes are.
local quit_reset = {}
for channel = 0, 15 do
    for note = 0, 127 do
        quit_reset[#quit_reset + 1] = suhina.midi.note_on(channel, note, 0)
    end
    for controller = 0, 127 do
        quit_reset[#quit_reset + 1] = suhina.midi.cc(channel, controller, 0)
    end
end
suhina.quit_reset(quit_reset)
