// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

#[test]
fn native_panel_only_targets_the_shown_active_asio_device() {
    use super::native_panel_visible;
    let live = suhina_session::OutputInfo { host: "ASIO".into(), device: "Interface A".into(), ..Default::default() };
    assert!(native_panel_visible(false, Some("ASIO"), Some("Interface A"), Some(&live)));
    assert!(!native_panel_visible(true, Some("ASIO"), Some("Interface A"), Some(&live)));
    assert!(!native_panel_visible(false, Some("ASIO"), Some("Interface B"), Some(&live)));
    assert!(!native_panel_visible(false, Some("WASAPI"), Some("Interface A"), Some(&live)));
    assert!(!native_panel_visible(false, Some("ASIO"), Some("Interface A"), None));
}

use super::*;

#[test]
fn driver_memory_survives_restart_and_applies_only_an_available_pair() {
    use suhina_session::audio::{DeviceInfo, HostInfo};
    let about = About {
        audio_hosts: vec![
            HostInfo {
                name: "WASAPI".into(),
                default: true,
                devices: vec![DeviceInfo { name: "Speakers".into(), default: true, ..Default::default() }],
                ..Default::default()
            },
            HostInfo {
                name: "ASIO".into(),
                devices: vec![DeviceInfo { name: "Interface".into(), default: true, ..Default::default() }],
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    let wasapi = suhina_session::OutputChoice { device: Some("Speakers".into()), ..Default::default() };
    let asio = suhina_session::OutputChoice {
        host: Some("ASIO".into()),
        device: Some("Interface".into()),
        ..Default::default()
    };
    let mut settings = Settings::default();
    for choice in [&wasapi, &asio] {
        settings.remember_output(choice, Some("WASAPI"));
        settings.set_output_choice(choice);
    }
    let (mut settings, warnings) = Settings::from_text(&settings.to_text());
    assert!(warnings.is_empty());
    for choice in [&wasapi, &asio, &wasapi] {
        let driver = suhina_session::OutputChoice { host: choice.host.clone(), ..Default::default() };
        let OutputPick::Device(restored) = restore_driver(OutputPick::Driver(driver), &settings, &about) else {
            panic!("remembered pair must apply")
        };
        assert_eq!(restored.device, choice.device);
        settings.set_output_choice(&restored);
    }
    // A named default host and Default refer to the same remembered backend.
    settings.set_output_choice(&asio);
    let named = suhina_session::OutputChoice { host: Some("WASAPI".into()), ..Default::default() };
    assert!(
        matches!(restore_driver(OutputPick::Driver(named), &settings, &about), OutputPick::Device(c) if c.device == wasapi.device)
    );
    settings.set_output_choice(&wasapi);
    let mut missing = about.clone();
    missing.audio_hosts[1].devices.clear();
    assert!(matches!(restore_driver(OutputPick::Driver(asio.clone()), &settings, &missing), OutputPick::Driver(_)));
    // An explicitly selected Default is distinct from having no history.
    settings.remember_output(
        &suhina_session::OutputChoice { host: asio.host.clone(), ..Default::default() },
        Some("WASAPI"),
    );
    assert!(
        matches!(restore_driver(OutputPick::Driver(asio), &settings, &about), OutputPick::Device(c) if c.device.is_none())
    );
}

#[test]
fn a_driver_is_applied_only_together_with_the_selected_device() {
    let active = suhina_session::OutputChoice::default();
    let mut pending = None;
    for host in ["ASIO", "WASAPI", "CoreAudio"] {
        let driver = active.with_params(&serde_json::json!({"host": host})).unwrap();
        assert!(select_output(&mut pending, &active, OutputPick::Driver(driver.clone())).is_none());
        assert_eq!(pending.as_ref(), Some(&driver));
        let device = driver.with_params(&serde_json::json!({"device": "Selected interface"})).unwrap();
        assert_eq!(select_output(&mut pending, &active, OutputPick::Device(device.clone())), Some(device));
        assert!(pending.is_none());
    }
}

#[test]
fn a_pending_driver_can_be_cancelled_or_explicitly_use_its_default_device() {
    let active = suhina_session::OutputChoice::default();
    let driver = active.with_params(&serde_json::json!({"host": "ASIO"})).unwrap();
    let mut pending = Some(driver.clone());
    let rate = driver.with_params(&serde_json::json!({"sampleRate": 44100})).unwrap();
    assert!(select_output(&mut pending, &active, OutputPick::Setting(rate.clone())).is_none());
    assert_eq!(pending.as_ref(), Some(&rate));
    assert!(select_output(&mut pending, &active, OutputPick::Driver(active.clone())).is_none());
    assert!(pending.is_none());
    pending = Some(driver.clone());
    assert_eq!(select_output(&mut pending, &active, OutputPick::Device(driver.clone())), Some(driver));
    assert!(pending.is_none());
}

#[test]
fn the_theme_search_matches_names_ids_and_variant_names() {
    let variants = vec![("nord@frost".to_string(), "Frost".to_string())];
    assert!(theme_matches("", "nord", "Nord", &variants));
    assert!(theme_matches("nor", "nord", "Nord", &variants));
    assert!(theme_matches("frost", "nord", "Nord", &variants));
    assert!(!theme_matches("frost", "dracula", "Dracula", &variants));
    assert!(theme_matches("test-new", "test-new-theme", "Kajo", &[]));
}

#[test]
fn settings_tree_does_not_cover_the_material_with_opaque_rows() {
    let ctx = egui::Context::default();
    motion::set_enabled(&ctx, false);
    let mut output = ctx.run_ui(Default::default(), |ui| {
        page_list(ui, ui.painter(), Rect::from_min_size(Pos2::ZERO, Vec2::new(140.0, 300.0)), Some(Page::Projects));
    });
    assert!(!output.shapes.iter().any(|shape| matches!(&shape.shape,
            egui::Shape::Rect(shape) if shape.fill.is_opaque())));
    output.textures_delta.clear();
}

#[test]
fn form_fields_have_transparent_fills() {
    let ctx = egui::Context::default();
    let mut output = ctx.run_ui(Default::default(), |ui| {
        input_style(ui);
        assert_eq!(ui.visuals().text_edit_bg_color(), egui::Color32::TRANSPARENT);
    });
    output.textures_delta.clear();
}

#[test]
fn the_output_now_says_which_chosen_device_it_waits_for() {
    let info = suhina_session::OutputInfo {
        host: "WASAPI".into(),
        device: "Speakers".into(),
        sample_rate: 48_000,
        engine_block: 512,
        ..Default::default()
    };
    let usb = suhina_session::OutputChoice { device: Some("USB Audio".into()), ..Default::default() };
    assert_eq!(now_playing(&info, &usb), "Speakers (WASAPI, 48000 Hz, 512 frames)");
    let waiting = suhina_session::OutputInfo { fallback: true, ..info };
    assert_eq!(now_playing(&waiting, &usb), "Speakers (WASAPI, 48000 Hz, 512 frames), until USB Audio is back");
    let asio = suhina_session::OutputChoice { host: Some("ASIO".into()), ..Default::default() };
    assert!(now_playing(&waiting, &asio).ends_with("until ASIO is back"));
}
