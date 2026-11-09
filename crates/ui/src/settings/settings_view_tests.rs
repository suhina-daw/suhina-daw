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

#[test]
fn the_device_row_says_where_bluetooth_headphones_are_and_why_a_list_is_empty() {
    use suhina_session::audio::HostInfo;
    let host = |name: &str| HostInfo { name: name.into(), ..Default::default() };
    let plain = "The output device on that driver";
    let windows = [host("ASIO"), host("WASAPI")];
    assert!(super::device_note(Some(&windows[0]), &windows).contains("under WASAPI"));
    assert_eq!(super::device_note(Some(&windows[1]), &windows), plain);
    let linux = [host("PulseAudio"), host("ALSA")];
    assert!(super::device_note(Some(&linux[1]), &linux).contains("under PulseAudio"));
    assert_eq!(super::device_note(Some(&linux[0]), &linux), plain);
    // Only a driver that is there is pointed to.
    let alsa = [host("ALSA")];
    assert_eq!(super::device_note(Some(&alsa[0]), &alsa), plain);
    assert_eq!(super::device_note(None, &alsa), plain);
    let failed = HostInfo { error: Some("no answer".into()), ..host("WASAPI") };
    assert_eq!(super::device_note(Some(&failed), &[]), "WASAPI cannot list its devices: no answer");
}

#[test]
fn the_default_track_input_lists_the_devices_channels() {
    let values = track_input_options;
    assert_eq!(values(0, "none"), vec![("None".to_string(), "none".to_string())], "no input device: only None");
    let four: Vec<String> = values(4, "none").into_iter().map(|(label, _)| label).collect();
    assert_eq!(four, ["None", "In 1", "In 2", "In 3", "In 4", "In 1/2", "In 3/4"]);
    // Three channels make one pair; the third is a channel of its own.
    let three: Vec<String> = values(3, "1/2").into_iter().map(|(_, value)| value).collect();
    assert_eq!(three, ["none", "1", "2", "3", "1/2"]);
    // A choice the device does not have stays listed, marked, so it is not lost unseen.
    let missing = values(2, "5/6");
    assert_eq!(missing.last(), Some(&("In 5/6 (not on this device)".to_string(), "5/6".to_string())));
    // A pair the list leaves out (set from MCP) but the device has is shown as it is.
    let between = values(4, "2/3");
    assert_eq!(between.last(), Some(&("In 2/3".to_string(), "2/3".to_string())));
    assert_eq!(between.len(), 8);
}

#[test]
fn choosing_another_audio_device_brings_back_its_input() {
    use suhina_session::{InputChoice, OutputChoice};
    let device = |name: &str, input: InputChoice| OutputChoice {
        host: Some("ASIO".into()),
        device: Some(name.into()),
        input,
        ..OutputChoice::default()
    };
    let mut settings = Settings::default();
    settings.set_output_choice(&device("SSL 2+", InputChoice::Named("SSL 2+ In".into())));
    settings.set_track_input(suhina_engine::model::InputChannels::Stereo(1));
    let active = device("Thunderbolt", InputChoice::Default);
    settings.set_output_choice(&active);

    // The card chosen before gets its input back, and with it its default track input.
    let picked = restore_input(OutputPick::Device(device("SSL 2+", InputChoice::Default)), &active, &settings);
    let OutputPick::Device(restored) = picked else { panic!("a device pick stays one") };
    assert_eq!(restored.input, InputChoice::Named("SSL 2+ In".into()));
    settings.set_output_choice(&restored);
    assert_eq!(settings.track_input(), suhina_engine::model::InputChannels::Stereo(1));

    // A card never chosen keeps the input there is; another setting on the same card is left alone.
    let new = restore_input(OutputPick::Device(device("USB Mic", InputChoice::Default)), &active, &settings);
    assert!(matches!(new, OutputPick::Device(c) if c.input == InputChoice::Default));
    let same = device("Thunderbolt", InputChoice::Named("Other".into()));
    let kept = restore_input(OutputPick::Device(same.clone()), &active, &settings);
    assert!(matches!(kept, OutputPick::Device(c) if c == same));
}

/// A driver taking 128 to 512 frames with 128 set in its control panel: Default names that size,
/// and a size chosen before (2048) stays chosen and listed, saying the device does not take it now.
#[test]
fn the_buffer_list_names_the_drivers_default_and_keeps_an_earlier_choice() {
    let options = buffer_options(&[128, 256, 512], 128, None, 44_100.0, false);
    let labels: Vec<&str> = options.iter().map(|(label, _)| label.as_str()).collect();
    assert_eq!(labels, ["Default (128 frames)", "128 frames (2.9 ms)", "256 frames (5.8 ms)", "512 frames (11.6 ms)"]);
    let options = buffer_options(&[128, 256, 512], 128, Some(2048), 44_100.0, false);
    assert_eq!(
        options.iter().map(|(_, n)| *n).collect::<Vec<_>>(),
        [None, Some(128), Some(256), Some(512), Some(2048)]
    );
    assert!(options[4].0.contains("does not take it now"), "{}", options[4].0);
    // A choice the device offers is listed once.
    assert_eq!(buffer_options(&[128, 256], 128, Some(256), 44_100.0, false).len(), 3);
}

/// On WASAPI the number is the engine's block: a size chosen elsewhere (96 frames on a stepped
/// ASIO driver) plays as it is, so the list does not say the device refuses it. One past the
/// sizes offered is fitted down, and says so.
#[test]
fn an_unlisted_block_on_wasapi_plays_as_chosen() {
    let options = buffer_options(&[64, 128, 256, 512, 1024, 2048], 1024, Some(96), 48_000.0, true);
    assert_eq!(options[2], ("96 frames (2.0 ms)".to_string(), Some(96)));
    let options = buffer_options(&[64, 128, 256, 512, 1024, 2048], 1024, Some(4096), 48_000.0, true);
    assert!(options.last().is_some_and(|(label, _)| label.contains("does not take it now")));
}

/// A driver that takes one size, as one whose size is set in its control panel, has its size
/// said, not offered; a driver with sizes to choose from gets the dropdown.
#[test]
fn a_driver_with_one_size_says_it_instead_of_offering_it() {
    assert_eq!(buffer_row(&[2048], true, true), BufferRow::Only(2048));
    assert_eq!(buffer_row(&[128, 256], true, true), BufferRow::Choose);
    // Elsewhere a size chosen sets the engine's block, so there is always a choice.
    assert_eq!(buffer_row(&[256], true, false), BufferRow::Choose);
    assert_eq!(buffer_row(&[1024], false, false), BufferRow::Choose);
}
