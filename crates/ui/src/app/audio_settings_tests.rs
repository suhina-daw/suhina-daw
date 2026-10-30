// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use std::sync::Arc;
use suhina_session::{AudioSystem, LiveOutput, OutputChoice, OutputInfo, Player};

struct Panel;
impl LiveOutput for Panel {
    fn info(&self) -> OutputInfo {
        OutputInfo::default()
    }
    fn control_panel_open(&self) -> bool {
        true
    }
}

fn choices() -> (OutputChoice, OutputChoice) {
    (
        OutputChoice { host: Some("ASIO".into()), device: Some("Old".into()), ..Default::default() },
        OutputChoice { host: Some("ASIO".into()), device: Some("New".into()), ..Default::default() },
    )
}

#[test]
fn rejected_output_and_cue_choices_restore_settings_without_recording_history() {
    let mut app = App::new();
    let (current, requested) = choices();
    let mut session = Session::new(suhina_engine::model::Project::default());
    let mut renderer = suhina_engine::Renderer::stopped(session.project(), &Default::default(), 48_000).unwrap();
    session.attach_player(Player::with_output(renderer.controller(), None, Arc::new(Panel)));
    app.projects.add(session, None).unwrap();
    app.projects.set_output_choice(current.clone());
    app.settings.set_output_choice(&current);
    app.settings.remember_output(&current, None);
    let history = app.settings.audio_devices.clone();
    for selected in [requested, OutputChoice { cue_pair: Some(1), ..current.clone() }] {
        // The settings form stages a choice before the command is dispatched.
        app.settings.set_output_choice(&selected);
        app.choose_output(&selected);
        assert_eq!(app.settings.output_choice(), current);
        assert_eq!(app.projects.output_choice(), &current);
        assert_eq!(app.settings.audio_devices, history);
        assert!(matches!(app.status, Status::Error(_)));
    }
}

#[test]
fn successful_output_selection_records_history() {
    let mut app = App::new();
    let (current, selected) = choices();
    app.projects.set_output_choice(current);
    app.settings.set_output_choice(&selected);
    app.choose_output(&selected);
    assert_eq!(app.settings.audio_devices.get("asio"), Some(&selected.device));
    assert_eq!(app.projects.output_choice(), &selected);
}

struct FailedOpen;
impl AudioSystem for FailedOpen {
    fn hosts(&self) -> Vec<suhina_session::audio::HostInfo> {
        Vec::new()
    }
    fn open(
        &self,
        project: &suhina_engine::model::Project,
        _: &suhina_engine::AudioPool,
        _: &suhina_engine::DeviceAssets,
        choice: &OutputChoice,
        _: Option<Player>,
    ) -> Result<suhina_session::audio::Opened, String> {
        if choice.host.as_deref() == Some("WASAPI") {
            let mut renderer =
                suhina_engine::Renderer::stopped(project, &Default::default(), 48_000).map_err(|e| e.to_string())?;
            return Ok(suhina_session::audio::Opened {
                player: Player::new(renderer.controller(), None),
                notice: None,
                plugins_kept: false,
            });
        }
        Err("unavailable".into())
    }
}

#[test]
fn failed_device_open_keeps_requested_choice_for_retry_but_not_history() {
    let mut app = App::new();
    let (current, selected) = choices();
    let mut session = Session::new(suhina_engine::model::Project::default());
    let mut renderer = suhina_engine::Renderer::stopped(session.project(), &Default::default(), 48_000).unwrap();
    session.attach_player(Player::new(renderer.controller(), None));
    app.projects.add(session, None).unwrap();
    app.projects.set_audio_system(Arc::new(FailedOpen));
    app.projects.set_output_choice(current.clone());
    app.settings.remember_output(&current, None);
    let history = app.settings.audio_devices.clone();
    app.settings.set_output_choice(&selected);
    app.choose_output(&selected);
    assert!(matches!(app.status, Status::Error(_)));
    assert_eq!(app.settings.output_choice(), selected);
    assert_eq!(app.projects.output_choice(), &selected);
    assert_eq!(app.settings.audio_devices, history);
}

#[test]
fn switching_after_a_failed_open_preserves_the_previous_drivers_working_device() {
    let mut app = App::new();
    let (working, failed) = choices();
    let mut session = Session::new(suhina_engine::model::Project::default());
    let mut renderer = suhina_engine::Renderer::stopped(session.project(), &Default::default(), 48_000).unwrap();
    session.attach_player(Player::new(renderer.controller(), None));
    app.projects.add(session, None).unwrap();
    app.projects.set_audio_system(Arc::new(FailedOpen));
    app.projects.set_output_choice(working.clone());
    app.settings.remember_output(&working, None);
    app.choose_output(&failed);
    assert!(matches!(app.status, Status::Error(_)));
    assert_eq!(app.projects.output_choice(), &failed);
    let next = OutputChoice { host: Some("WASAPI".into()), device: Some("Speakers".into()), ..Default::default() };
    app.choose_output(&next);
    assert!(!matches!(app.status, Status::Error(_)));
    assert_eq!(app.projects.output_choice(), &next);
    assert_eq!(app.settings.audio_devices.get("asio"), Some(&working.device));
    assert_eq!(app.settings.audio_devices.get("wasapi"), Some(&next.device));
}
