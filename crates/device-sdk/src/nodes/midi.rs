// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::{NodeInfo, PortInfo};

pub const MIDI_INPUT: NodeInfo = NodeInfo {
    kind: "midi_input",
    name: "MIDI Input",
    inputs: &[],
    outputs: &["out"],
    description: "Incoming MIDI events with their frame offsets.",
    configuration: "none",
    example: "{\"kind\":\"midi_input\"}",
    rust_variant: "MidiInput",
};

pub const MIDI_OUTPUT: NodeInfo = NodeInfo {
    kind: "midi_output",
    name: "MIDI Output",
    inputs: &[PortInfo { name: "in", default: None, unit: "midi" }],
    outputs: &[],
    description: "Emits processed MIDI events from a MIDI-effect graph.",
    configuration: "none",
    example: "{\"kind\":\"midi_output\"}",
    rust_variant: "MidiOutput",
};

pub const MIDI_TRANSPOSE: NodeInfo = NodeInfo {
    kind: "midi_transpose",
    name: "MIDI Transpose",
    inputs: &[PortInfo { name: "in", default: None, unit: "midi" }],
    outputs: &["out"],
    description: "Transposes note-on and note-off events by semitones; keys outside 0..127 are dropped.",
    configuration: " semitones: i8",
    example: "{\"kind\":\"midi_transpose\",\"semitones\":12}",
    rust_variant: "MidiTranspose",
};

pub const MIDI_TRANSPOSE_PARAMETER: NodeInfo = NodeInfo {
    kind: "midi_transpose_parameter",
    name: "MIDI Transpose Parameter",
    inputs: &[PortInfo { name: "in", default: None, unit: "midi" }],
    outputs: &["out"],
    description: "Transposes note events by an integer parameter; keys outside 0..127 are dropped.",
    configuration: "parameter: String",
    example: "{\"kind\":\"midi_transpose_parameter\",\"parameter\":\"semitones\"}",
    rust_variant: "MidiTransposeParameter",
};

pub const MIDI_KEY: NodeInfo = NodeInfo {
    kind: "midi_key",
    name: "MIDI Key",
    inputs: &[],
    outputs: &["out", "left", "right"],
    description: "Instrument signal containing the current MIDI key, or 0 when no key is held.",
    configuration: "none",
    example: "{\"kind\":\"midi_key\"}",
    rust_variant: "MidiKey",
};

pub const MIDI_VELOCITY: NodeInfo = NodeInfo {
    kind: "midi_velocity",
    name: "MIDI Velocity",
    inputs: &[],
    outputs: &["out", "left", "right"],
    description: "Instrument signal containing the current note velocity from 0 to 1.",
    configuration: "none",
    example: "{\"kind\":\"midi_velocity\"}",
    rust_variant: "MidiVelocity",
};

pub const MIDI_GATE: NodeInfo = NodeInfo {
    kind: "midi_gate",
    name: "MIDI Gate",
    inputs: &[],
    outputs: &["out", "left", "right"],
    description: "Instrument signal set to 1 while a note is held and 0 otherwise.",
    configuration: "none",
    example: "{\"kind\":\"midi_gate\"}",
    rust_variant: "MidiGate",
};
