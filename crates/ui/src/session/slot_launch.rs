// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A clip slot's launch button, pressed and released, and its menu of launch settings
//! (`docs/spec/ui-session-view.md` › Launch settings). A press sends `clip.launch` at once,
//! without waiting for the click to end, so the launch is as close to the beat as the hand was;
//! the release sends `clip.release` for a clip in gate or repeat mode, which listens for it.

use egui::{Response, Ui};
use serde_json::json;
use suhina_engine::model::{LaunchMode, LaunchQuantization, LaunchSettings, TrackId};

use crate::kit::widgets::Action;

/// Turns the pointer going down and up on a slot's launch button into `clip.launch` and
/// `clip.release`. A click without the pointer (a keyboard or an accessibility tool) launches.
/// `scene` counts from 1. `launch` is the slot clip's settings, `None` for an empty slot.
/// `playing`: the slot's clip is playing, so a press or click on a trigger clip (one that
/// listens for no release) stops it instead — the mark is the stop square then.
/// Returns whether it acted (launched, or stopped).
pub fn press_and_release(
    ui: &Ui,
    response: &Response,
    track: TrackId,
    scene: usize,
    launch: Option<LaunchSettings>,
    playing: bool,
    actions: &mut Vec<Action>,
) -> bool {
    let key = response.id.with("held");
    let was_down = ui.ctx().data(|d| d.get_temp::<bool>(key)).unwrap_or(false);
    let down = response.is_pointer_button_down_on() && ui.input(|i| i.pointer.primary_down());
    let params = json!({ "track": track.0, "scene": scene });
    // A clip in gate, toggle or repeat mode listens for the release; its button keeps its own
    // press-and-release meaning even while it plays.
    let stopping = playing && !launch.is_some_and(|l| l.mode.uses_release());
    let mut acted = false;
    if down && !was_down {
        if stopping {
            actions.push(Action::new("track.stop", json!({ "track": track.0 })));
        } else {
            actions.push(Action::new("clip.launch", params.clone()));
        }
        acted = true;
    } else if !down && was_down {
        if launch.is_some_and(|l| l.mode.uses_release()) {
            actions.push(Action::new("clip.release", params));
        }
    } else if response.clicked() && !was_down {
        if stopping {
            actions.push(Action::new("track.stop", json!({ "track": track.0 })));
        } else {
            actions.push(Action::new("clip.launch", params));
        }
        acted = true;
    }
    if down != was_down {
        ui.ctx().data_mut(|d| d.insert_temp(key, down));
    }
    acted
}

/// What a menu calls a launch mode.
fn mode_label(mode: LaunchMode) -> &'static str {
    match mode {
        LaunchMode::Trigger => "Trigger",
        LaunchMode::Gate => "Gate",
        LaunchMode::Toggle => "Toggle",
        LaunchMode::Repeat => "Repeat",
    }
}

/// What a menu calls a launch quantisation.
fn quantization_label(quantization: LaunchQuantization) -> &'static str {
    match quantization {
        LaunchQuantization::Global => "Global",
        LaunchQuantization::None => "None",
        other => other.name(),
    }
}

/// The right-click menu of a slot's clip: its launch mode, legato and launch quantisation.
/// `scene` counts from 1.
pub fn launch_menu(ui: &mut Ui, track: TrackId, scene: usize, launch: LaunchSettings, actions: &mut Vec<Action>) {
    ui.set_min_width(160.0);
    let slot = |extra: serde_json::Value| {
        let mut params = json!({ "track": track.0, "scene": scene });
        if let (Some(params), Some(extra)) = (params.as_object_mut(), extra.as_object()) {
            params.extend(extra.clone());
        }
        params
    };
    ui.menu_button(format!("Launch mode: {}", mode_label(launch.mode)), |ui| {
        for mode in LaunchMode::ALL {
            if ui.radio(launch.mode == mode, mode_label(mode)).clicked() {
                actions.push(Action::new("clip.setLaunchMode", slot(json!({ "mode": mode.name() }))));
                ui.close();
            }
        }
    });
    ui.menu_button(format!("Launch quantisation: {}", quantization_label(launch.quantization)), |ui| {
        for quantization in LaunchQuantization::ALL {
            if ui.radio(launch.quantization == quantization, quantization_label(quantization)).clicked() {
                actions.push(Action::new("clip.setQuantization", slot(json!({ "value": quantization.name() }))));
                ui.close();
            }
        }
    });
    let mut legato = launch.legato;
    if ui.checkbox(&mut legato, "Legato").clicked() {
        actions.push(Action::new("clip.setLegato", slot(json!({ "on": legato }))));
        ui.close();
    }
}
