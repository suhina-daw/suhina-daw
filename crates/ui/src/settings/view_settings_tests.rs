// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use crate::settings::Settings;

#[test]
fn retired_mica_preference_is_ignored_and_not_written_back() {
    let (settings, warnings) = Settings::from_text("windows_mica = on\n");
    assert!(warnings.is_empty());
    assert!(!settings.to_text().contains("windows_mica"));
}

#[test]
fn view_settings_survive_a_round_trip_through_the_settings_file() {
    let view = ViewSettings {
        liquid: crate::kit::liquid::Options { panels: 40, tracks: 60, clips: 80 },
        track_sizes: super::super::track_sizes::TrackSizes::default(),
        waveform: WaveformAppearance { mode: WaveformColor::Custom, custom: egui::Color32::from_rgb(120, 210, 80) },
        automation: AutomationAppearance {
            mode: AutomationColor::Fixed,
            line: Some(egui::Color32::from_rgb(250, 200, 40)),
            point: None,
        },
        selection: crate::kit::selection::SelectionLook {
            mode: crate::kit::selection::SelectionColor::Custom,
            custom: egui::Color32::from_rgb(200, 20, 30),
        },
        audio_color_strength: 64,
        header_color_strength: 75,
        mixer_height: 300,
        strip_fader: 90,
        detail_layout: DetailLayout::FullWidth,
        double_click_clip: false,
        automation_click_anywhere: true,
        automation_group_plugins: false,
        automation_top_lane: crate::arrangement::automation_lanes::TopLane::Pan,
        envelope_show: false,
        envelope_ceiling: 24,
        envelope_step: 5,
        envelope_free: true,
        envelope_all: false,
        menu_name_field: true,
        notes_replace: false,
        covered_whole: false,
        time_select_ctrl: true,
        clip_halves: true,
        grid_in_clips: false,
        waveform_zoom: 400,
        stretch_alt: false,
        rename_selected: false,
        play_from_clip: false,
        device_list_vertical: false,
        stack_in_piano_roll: false,
        fixed_track_color: true,
        new_clip_track_color: false,
        recent_chips: true,
        returns_hidden: true,
        new_returns: false,
        copy_wrapped: false,
        audio_editor_theme_waveform: true,
        audio_editor_line_drag: true,
        alt_wheel_one: true,
        folder_drop: FolderDrop::Copy,
        remove_group_processing: true,
        arrangement_toolbar: true,
        default_grid: "adaptive narrow",
        replace_clip: true,
        take_crossfade: 40,
        take_crossfade_custom: 80,
        menu_bar: false,
        app_name_centered: true,
        bar_project_name: false,
        bar_project_timing: true,
        bar_project_centered: false,
        background_tint: 45,
        background_blur: 20,
    };
    let settings = Settings { view, ..Settings::default() };
    let (read, warnings) = Settings::from_text(&settings.to_text());
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(read.view, view);
    let (read, _) = Settings::from_text(&Settings::default().to_text());
    assert_eq!(read.view, ViewSettings::default());
}

#[test]
fn a_mixer_height_out_of_range_is_clamped_and_a_bad_value_warned_about() {
    let (read, warnings) = Settings::from_text("session_mixer_height = 3\n");
    assert!(warnings.is_empty());
    assert_eq!(read.view.mixer_height, MIXER_HEIGHTS.0);
    let (read, _) = Settings::from_text("session_mixer_height = 99999999\n");
    assert_eq!(read.view.mixer_height, MIXER_HEIGHTS.2);
    let (read, warnings) =
        Settings::from_text("session_mixer_height = tall\ndetail_layout = up\ndouble_click_clip = maybe\n");
    assert_eq!(warnings.len(), 3, "{warnings:?}");
    assert_eq!(read.view, ViewSettings::default());
}

#[test]
fn waveform_choices_round_trip_and_invalid_colors_preserve_defaults() {
    assert_eq!(Settings::from_text("").0.view.waveform.mode, WaveformColor::Theme);
    for mode in
        [WaveformColor::Theme, WaveformColor::Auto, WaveformColor::Light, WaveformColor::Dark, WaveformColor::Custom]
    {
        let mut settings = Settings::default();
        settings.view.waveform = WaveformAppearance { mode, custom: egui::Color32::from_rgb(23, 140, 212) };
        let (read, warnings) = Settings::from_text(&settings.to_text());
        assert!(warnings.is_empty());
        assert_eq!(read.view.waveform, settings.view.waveform);
    }
    let (read, warnings) = Settings::from_text("waveform_color = rainbow\nwaveform_custom_color = #xyz123\n");
    assert_eq!(warnings.len(), 2);
    assert_eq!(read.view.waveform, WaveformAppearance::default());
}

#[test]
fn automation_and_selection_colors_round_trip_and_bad_values_keep_the_defaults() {
    let read = Settings::from_text("").0.view;
    assert_eq!(read.automation, AutomationAppearance::default());
    assert_eq!(read.automation.mode, AutomationColor::Track);
    assert_eq!(read.selection.mode, crate::kit::selection::SelectionColor::Theme);
    for mode in [AutomationColor::Track, AutomationColor::Fixed] {
        for (line, point) in [(None, None), (Some(egui::Color32::from_rgb(1, 2, 3)), Some(egui::Color32::WHITE))] {
            let mut settings = Settings::default();
            settings.view.automation = AutomationAppearance { mode, line, point };
            let (read, warnings) = Settings::from_text(&settings.to_text());
            assert!(warnings.is_empty(), "{warnings:?}");
            assert_eq!(read.view.automation, settings.view.automation);
        }
    }
    for mode in [
        crate::kit::selection::SelectionColor::Theme,
        crate::kit::selection::SelectionColor::Own,
        crate::kit::selection::SelectionColor::Custom,
    ] {
        let mut settings = Settings::default();
        settings.view.selection =
            crate::kit::selection::SelectionLook { mode, custom: egui::Color32::from_rgb(10, 200, 90) };
        let (read, warnings) = Settings::from_text(&settings.to_text());
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(read.view.selection, settings.view.selection);
    }
    let (read, warnings) = Settings::from_text(
        "automation_color = rainbow
automation_line_color = #xyz
selection_color = loud
selection_custom_color = red
",
    );
    assert_eq!(warnings.len(), 4, "{warnings:?}");
    assert_eq!(read.view.automation, AutomationAppearance::default());
    assert_eq!(read.view.selection, crate::kit::selection::SelectionLook::default());
}

#[test]
fn color_strengths_are_bounded_and_invalid_input_does_not_change_defaults() {
    let (read, warnings) = Settings::from_text("audio_clip_color_strength = 9999\ntrack_header_color_strength = -1\n");
    assert_eq!(warnings.len(), 1);
    assert_eq!(read.view.audio_color_strength, 100);
    assert_eq!(read.view.header_color_strength, 82);
}

#[test]
fn liquid_strengths_default_off_and_parse_independently_with_bounds() {
    assert_eq!(Settings::from_text("").0.view.liquid, crate::kit::liquid::Options::default());
    let (read, warnings) =
        Settings::from_text("liquid_panel_strength = 999\nliquid_track_strength = -1\nliquid_clip_strength = 37\n");
    assert_eq!(warnings.len(), 1);
    assert_eq!(read.view.liquid, crate::kit::liquid::Options { panels: 100, tracks: 0, clips: 37 });
}

#[test]
fn a_see_through_custom_selection_color_keeps_its_opacity() {
    let mut settings = Settings::default();
    settings.view.selection = crate::kit::selection::SelectionLook {
        mode: crate::kit::selection::SelectionColor::Custom,
        custom: egui::Color32::from_rgba_unmultiplied(200, 40, 60, 128),
    };
    let text = settings.to_text();
    let saved = text.lines().find_map(|line| line.strip_prefix("selection_custom_color = ")).unwrap_or_default();
    assert_eq!(saved.len(), 9, "#RRGGBBAA while see-through: {saved}");
    let (read, warnings) = Settings::from_text(&text);
    assert!(warnings.is_empty(), "{warnings:?}");
    let [r, g, b, a] = read.view.selection.custom.to_srgba_unmultiplied();
    assert_eq!(a, 128);
    assert!(r.abs_diff(200) <= 2 && g.abs_diff(40) <= 2 && b.abs_diff(60) <= 2, "{r} {g} {b}");
}
