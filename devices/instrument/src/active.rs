// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! When each control of the Instrument has an effect, for every presentation of the device:
//! a control that cannot change the sound in the current Source is drawn dimmed and says why
//! (the SDK's `active_when`). Presentation only: no parameter, automation or project changes.

use suhina_device_sdk::ui::{ActiveWhen, UiValue};

/// The waveform's id in [`active_rule`]: it shows and imports the sample layer's source.
pub const WAVEFORM: &str = "#waveform";

/// When a control of this device has an effect, by the parameter it edits (or [`WAVEFORM`]):
/// the modal body's controls in Modal and Hybrid, the sample layer's in Sample and Hybrid, the
/// blend in Hybrid only. Source, Gain, Release and the key and velocity ranges act on both
/// layers and have no rule. Any presentation of this device (the stock panel, an expanded editor) dims by it.
pub fn active_rule(id: &str) -> Option<ActiveWhen> {
    let source = |value: f32| UiValue::parameter("source", false).equals(value);
    Some(match id {
        "quality" | "body_decay" | "brightness" | "structure" | "strike" => ActiveWhen::new(
            source(0.0) + source(2.0),
            "Source is Sample, which plays no modal body. Choose Modal or Hybrid to hear it.",
        ),
        "root" | "start" | "end" | "loop_start" | "loop_end" | "playback" | "attack" | "sustain" => ActiveWhen::new(
            source(1.0) + source(2.0),
            "Source is Modal, which plays no sample. Choose Sample or Hybrid to hear it.",
        ),
        WAVEFORM => ActiveWhen::new(
            source(1.0) + source(2.0),
            "Source is Modal, which plays no sample. Dropping a file still imports it and switches Source to Sample.",
        ),
        "blend" => ActiveWhen::new(source(2.0), "Hybrid only: Modal and Sample each play one layer at full level."),
        _ => return None,
    })
}
