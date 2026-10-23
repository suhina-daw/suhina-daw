// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

/// A settings file as Suhina wrote it before the Settings window was regrouped
/// (`docs/spec/ui-settings.md`): moving a setting to another page never changes its key.
const BEFORE_THE_REGROUPING: &str = "# Suhina settings; changed from Options > Settings.
animations = off
track_headers = left
renderer = glow
sample_stacking = on
midi_stacking = on
open_projects = tab
at_start = template
start_template = Beats
stop = here
play_start = clip
device_chain = stack
mixer_controls = knob
select_on_launch = off
plugin_editors = device
theme = night
projects_folder = /music/projects
templates_folder = /music/templates
browser = off
browser_side = left
browser_width = 320
preview_autoplay = off
preview_volume = -12
drop_files = separate
warp_mode = tonal
map_layout = grid
map_hover_preview = off
map_max_seconds = 30
map_dot_size = 150
map_window = on
map_window_rect = 10, 20, 640, 480
audio_host = ASIO
audio_device = Interface
audio_sample_rate = 96000
audio_buffer = 256
audio_exclusive = on
audio_cue_pair = 2
audio_cue_volume = -6
audio_input = Line In
zoom = 120
waveform_scale = db
piano_roll_top = window
piano_roll_grid = control_bar
piano_roll_toolbar = larger
backup = 1m
backup_clean_up = off
midi_input_off = Keys
";

#[test]
fn a_settings_file_from_before_the_regrouping_loads_with_the_same_values() {
    let (settings, warnings) = Settings::from_text(BEFORE_THE_REGROUPING);
    assert_eq!(warnings, Vec::<String>::new());
    // A setting added later keeps its default here.
    #[allow(clippy::needless_update)]
    let expected = Settings {
        animations: false,
        track_headers: Side::Left,
        graphics: Graphics::OpenGl,
        sample_stacking: true,
        midi_stacking: true,
        on_open: OnOpen::NewTab,
        at_start: AtStart::Template,
        start_template: "Beats".into(),
        stop: StopReturn::Here,
        play_from: PlayFrom::Clip,
        device_layout: DeviceLayout::Stack,
        mixer_controls: MixerControls::Knob,
        select_on_launch: false,
        plugin_editors: EditorPlacement::DeviceView,
        theme: "night".into(),
        projects_folder: "/music/projects".into(),
        templates_folder: "/music/templates".into(),
        nam_models_folder: String::new(),
        irs_folder: String::new(),
        browser_open: false,
        browser_side: Side::Left,
        browser_width: 320,
        preview_autoplay: false,
        preview_volume: -12,
        drop_separate: true,
        warp_mode: WarpMode::Tonal,
        map_grid: true,
        map_hover_preview: false,
        map_max_seconds: 30,
        map_dot_size: 150,
        map_window: true,
        map_window_rect: Some([10, 20, 640, 480]),
        audio_host: "ASIO".into(),
        audio_device: "Interface".into(),
        audio_sample_rate: 96_000,
        audio_buffer: 256,
        audio_exclusive: true,
        audio_cue_pair: 2,
        audio_cue_volume: -6,
        audio_input: "Line In".into(),
        zoom: 120,
        waveform_db: true,
        piano_roll_top: PianoRollTop::Window,
        piano_grid_follows: true,
        toolbar_size: ToolbarSize::Larger,
        backup: suhina_session::backup::Interval::Minutes(1),
        backup_clean_up: false,
        midi_inputs_off: vec!["Keys".into()],
        ..Settings::default()
    };
    assert_eq!(settings, expected);
    // Written again, it reads back the same.
    assert_eq!(Settings::from_text(&settings.to_text()).0, expected);
}

#[test]
fn settings_survive_a_round_trip() {
    let settings = Settings {
        animations: false,
        track_headers: Side::Left,
        graphics: Graphics::OpenGl,
        sample_stacking: true,
        midi_stacking: false,
        on_open: OnOpen::Replace,
        at_start: AtStart::Template,
        start_template: "Four on the floor = mine.suhina".into(),
        stop: StopReturn::Here,
        play_from: PlayFrom::Clip,
        device_layout: DeviceLayout::Stack,
        select_on_launch: false,
        view: crate::settings::view_settings::ViewSettings::default(),
        updates: crate::settings::update_settings::UpdateSettings::default(),
        piano_roll_top: PianoRollTop::Window,
        toolbar_size: ToolbarSize::Larger,
        piano_grid_follows: true,
        mixer_controls: MixerControls::Knob,
        plugin_editors: EditorPlacement::DeviceView,
        theme: "Night blue".into(),
        projects_folder: "C:/Music/Songs = mine".into(),
        templates_folder: "/home/me/templates".into(),
        nam_models_folder: "/home/me/nam models".into(),
        irs_folder: "/home/me/irs".into(),
        browser_open: false,
        browser_side: Side::Right,
        browser_width: 420,
        preview_autoplay: false,
        preview_volume: -12,
        drop_separate: true,
        warp_mode: WarpMode::Tonal,
        map_grid: true,
        map_hover_preview: false,
        map_max_seconds: 30,
        map_dot_size: 250,
        waveform_db: true,
        audio_host: "ASIO".into(),
        audio_device: "Universal Audio Thunderbolt".into(),
        audio_sample_rate: 48_000,
        audio_buffer: 256,
        audio_exclusive: true,
        audio_cue_pair: 2,
        audio_cue_volume: -6,
        audio_input: "Line In (Interface)".into(),
        midi_inputs_off: vec!["Keys, port 1".into(), "Pads:Pads MIDI 1 20:0".into()],
        zoom: 130,
        map_window: true,
        map_window_rect: Some([-1600, 40, 900, 700]),
        backup: suhina_session::backup::Interval::Minutes(240),
        backup_clean_up: false,
        agents: true,
        mcp_project_folder: false,
        mcp_write_folders: vec![r"C:\Music\Mixes = mine".into(), "/tmp/renders".into()],
        auto_arm: true,
    };
    assert_eq!(Settings::from_text(&settings.to_text()), (settings, Vec::new()));
    assert_eq!(Settings::from_text(&Settings::default().to_text()).0, Settings::default());
}

#[test]
fn at_start_reads_each_choice_and_keeps_the_panel_for_anything_else() {
    for (text, choice) in [
        ("panel", AtStart::Panel),
        ("dialog", AtStart::Panel),
        ("blank", AtStart::Blank),
        ("template", AtStart::Template),
        ("last", AtStart::Last),
    ] {
        let (settings, warnings) = Settings::from_text(&format!("at_start = {text}\n"));
        assert_eq!((settings.at_start, warnings.len()), (choice, 0), "{text}");
    }
    let (settings, warnings) = Settings::from_text("at_start = everything\nat_start\n");
    assert_eq!(settings.at_start, AtStart::Panel);
    assert_eq!(warnings.len(), 2, "{warnings:?}");
}

#[test]
fn browser_values_out_of_range_are_limited() {
    let (settings, warnings) =
        Settings::from_text("browser_width = 5000\npreview_volume = -900\nwarp_mode = elastic\n");
    assert_eq!((settings.browser_width, settings.preview_volume), (BROWSER_WIDTHS.1, PREVIEW_VOLUMES.0));
    assert_eq!(settings.warp_mode, WarpMode::Rhythmic);
    assert_eq!(warnings.len(), 1);
}

#[test]
fn the_audio_output_is_a_choice_and_nonsense_keeps_the_default() {
    let (settings, warnings) = Settings::from_text(
        "audio_host = WASAPI\naudio_device = Speakers (Interface)\naudio_sample_rate = 44100\naudio_buffer = 512\n",
    );
    assert!(warnings.is_empty(), "{warnings:?}");
    let choice = settings.output_choice();
    assert_eq!(choice.host.as_deref(), Some("WASAPI"));
    assert_eq!(choice.device.as_deref(), Some("Speakers (Interface)"));
    assert_eq!((choice.sample_rate, choice.buffer_frames, choice.exclusive), (Some(44_100), Some(512), false));
    let mut back = Settings::default();
    back.set_output_choice(&choice);
    assert_eq!(back.output_choice(), choice);
    assert_eq!(Settings::default().output_choice(), suhina_session::OutputChoice::default());
    // The input device: off unless chosen.
    let (settings, _) = Settings::from_text("audio_input = default\n");
    assert_eq!(settings.output_choice().input, suhina_session::InputChoice::Default);
    assert_eq!(Settings::from_text("audio_input = \n").0.audio_input, "off");

    let (settings, warnings) = Settings::from_text(
        "audio_buffer = 17\naudio_sample_rate = lots\naudio_exclusive = maybe\naudio_cue_pair = 40\n",
    );
    assert_eq!(settings, Settings::default());
    assert_eq!(warnings.len(), 4);

    // The cue: its pair, and its volume within range, lowest is silence.
    let (settings, warnings) = Settings::from_text("audio_cue_pair = 1\naudio_cue_volume = 40\n");
    assert!(warnings.is_empty(), "{warnings:?}");
    let choice = settings.output_choice();
    assert_eq!(choice.cue_pair, Some(1));
    assert!((choice.cue_gain - 10f32.powf(12.0 / 20.0)).abs() < 1e-5);
    let mut back = Settings::default();
    back.set_output_choice(&choice);
    assert_eq!((back.audio_cue_pair, back.audio_cue_volume), (1, 12));
    let silent = Settings::from_text("audio_cue_volume = -100\n").0.output_choice();
    assert_eq!(silent.cue_gain, 0.0);
}

#[test]
fn a_map_window_out_of_bounds_or_misread_is_not_kept() {
    let (settings, warnings) = Settings::from_text(
        "map_window = on
map_window_rect = 10, 20, 800, 600
",
    );
    assert_eq!((settings.map_window, settings.map_window_rect), (true, Some([10, 20, 800, 600])));
    assert!(warnings.is_empty());
    for bad in ["10, 20, 800", "10, 20, 50, 600", "99999, 0, 800, 600", "a, b, c, d", "1,2,3,4,5"] {
        let (settings, warnings) = Settings::from_text(&format!(
            "map_window_rect = {bad}
"
        ));
        assert_eq!((settings.map_window_rect, warnings.len()), (None, 1), "{bad}");
    }
}

#[test]
fn the_zoom_steps_within_its_limits_and_a_saved_one_is_limited_too() {
    let mut settings = Settings::default();
    assert!(settings.step_zoom(1));
    assert_eq!((settings.zoom, settings.zoom_factor()), (110, 1.1));
    assert!(settings.step_zoom(-3));
    assert_eq!(settings.zoom, 80);
    assert!(settings.step_zoom(-100));
    assert_eq!(settings.zoom, ZOOMS.0);
    assert!(!settings.step_zoom(-1), "already the smallest");
    assert!(settings.step_zoom(i32::MAX));
    assert_eq!(settings.zoom, ZOOMS.1);
    settings.zoom = 115;
    assert!(settings.step_zoom(1));
    assert_eq!(settings.zoom, 120, "onto the next step");
    settings.zoom = 115;
    assert!(settings.step_zoom(-1));
    assert_eq!(settings.zoom, 110);

    let (settings, warnings) = Settings::from_text(
        "zoom = 900
",
    );
    assert_eq!((settings.zoom, warnings.len()), (ZOOMS.1, 0));
    let (settings, warnings) = Settings::from_text(
        "zoom = big
",
    );
    assert_eq!((settings.zoom, warnings.len()), (ZOOM, 1));
}

#[test]
fn play_from_reads_marker_or_clip_and_nothing_else() {
    // The selected clip by default.
    assert_eq!(Settings::default().play_from, PlayFrom::Clip);
    assert_eq!(Settings::from_text("play_start = clip\n").0.play_from, PlayFrom::Clip);
    assert_eq!(Settings::from_text("play_start = marker\n").0.play_from, PlayFrom::Marker);
    let (settings, warnings) = Settings::from_text("play_start = selection\n");
    assert_eq!((settings.play_from, warnings.len()), (PlayFrom::Clip, 1));
    assert!(Settings::default().to_text().contains("play_start = clip\n"));
    // The old key, written "marker" into every file while that was the default, is left
    // alone, so the selected clip takes over.
    let (settings, warnings) = Settings::from_text("play_from = marker\n");
    assert_eq!((settings.play_from, warnings.len()), (PlayFrom::Clip, 0));
}

#[test]
fn every_backup_choice_is_kept_and_others_keep_the_default() {
    for interval in suhina_session::backup::Interval::ALL {
        let settings = Settings { backup: interval, ..Settings::default() };
        assert_eq!(Settings::from_text(&settings.to_text()).0.backup, interval);
    }
    let (settings, warnings) = Settings::from_text("backup = 7m\nbackup_clean_up = maybe\n");
    assert_eq!((settings.backup, settings.backup_clean_up), (suhina_session::backup::Interval::Minutes(5), true));
    assert_eq!(warnings.len(), 2);
}

#[test]
fn unknown_lines_keep_the_defaults() {
    let (settings, warnings) = Settings::from_text("animations = sometimes\ncolour = pink\nnonsense\n\n# fine\n");
    assert_eq!(settings, Settings::default());
    assert_eq!(warnings.len(), 3);
}

#[test]
fn the_toolbar_size_round_trips_and_an_unknown_one_is_large() {
    assert_eq!(Settings::default().toolbar_size, ToolbarSize::Large);
    for size in ToolbarSize::ALL {
        let settings = Settings { toolbar_size: size, ..Settings::default() };
        let (back, warnings) = Settings::from_text(&settings.to_text());
        assert_eq!((back.toolbar_size, warnings.len()), (size, 0), "{size:?}");
        assert!(settings.to_text().contains(&format!("piano_roll_toolbar = {}\n", size.param())));
    }
    // A file without the line, and a value the program does not know, both take the default.
    let (missing, warnings) = Settings::from_text("piano_roll_top = window\n");
    assert_eq!((missing.toolbar_size, warnings.len()), (ToolbarSize::Large, 0));
    let (unknown, warnings) = Settings::from_text("piano_roll_toolbar = huge\n");
    assert_eq!((unknown.toolbar_size, warnings.len()), (ToolbarSize::Large, 1));
    // And each size scales the toolbar by its own factor.
    assert_eq!(
        ToolbarSize::ALL.map(ToolbarSize::scale),
        [1.0, 1.25, 1.5],
        "Normal, Large and Larger scale the toolbar"
    );
    assert_eq!(ToolbarSize::ALL.map(ToolbarSize::name), ["Normal", "Large", "Larger"]);
}
