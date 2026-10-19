// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Every setting the Settings window shows, in one table: its page, its section, its name and
//! note, the keys it is saved under and how its control is drawn (`docs/spec/ui-settings.md`).
//! A page shows its entries in this order, a new section heading where the section changes.
//!
//! Adding a setting is one entry under its section, most often a `row` whose control is one call:
//! `cx.on_off(r, |s| &mut s.field)` or `cx.choose(r, &[("Text", value), …], |s| &mut s.field)`.

use crate::settings::settings_view::{Entry, Page, block, row};
use crate::settings::view_settings::DetailLayout;
use crate::settings::{
    AtStart, DeviceLayout, EditorPlacement, Graphics, MixerControls, OnOpen, PianoRollTop, PlayFrom, Side, StopReturn,
    ToolbarSize, WarpMode,
};
use suhina_update::Channel;

/// The keys of `keymap.json`, where the keys and the mouse switches are saved.
pub(crate) const KEYMAP: &[&str] = &["keymap.json"];

/// Every setting the window shows, page by page.
pub(crate) static ROWS: &[Entry] = &[
    // General.
    row(
        Page::General,
        "Projects",
        "When opening a project",
        "While another is open: ask, open it in a new tab, or replace it",
        &["open_projects"],
        |cx, r| {
            let options = [("Ask", OnOpen::Ask), ("New tab", OnOpen::NewTab), ("Replace", OnOpen::Replace)];
            cx.choose(r, &options, |s| &mut s.on_open);
        },
    ),
    row(
        Page::General,
        "Projects",
        "At start",
        "With no project given: the start panel, a new one, one from a template, or the last",
        &["at_start"],
        |cx, r| {
            let options = [
                ("Panel", AtStart::Panel),
                ("New", AtStart::Blank),
                ("Template", AtStart::Template),
                ("Last", AtStart::Last),
            ];
            cx.choose(r, &options, |s| &mut s.at_start);
        },
    ),
    row(
        Page::General,
        "Projects",
        "Projects folder",
        "Where a new project is saved first",
        &["projects_folder"],
        |cx, r| {
            let default = cx.about.projects_default.clone();
            cx.folder(r, &default, crate::interaction::pick::PickField::ProjectsFolder, |s| &mut s.projects_folder);
        },
    ),
    row(
        Page::General,
        "Projects",
        "Templates folder",
        "Projects listed under New › From a template, and where Save as template puts one",
        &["templates_folder"],
        |cx, r| {
            let default = cx.about.templates_default.clone();
            cx.folder(r, &default, crate::interaction::pick::PickField::TemplatesFolder, |s| &mut s.templates_folder);
        },
    ),
    row(
        Page::General,
        "Projects",
        "Start template",
        "The template At start makes a new project from",
        &["start_template"],
        |cx, r| {
            let hint = "A template's file name, or a path";
            cx.folder(r, hint, crate::interaction::pick::PickField::StartTemplate, |s| &mut s.start_template);
        },
    )
    .when(|s| s.at_start == AtStart::Template),
    row(
        Page::General,
        "NAM",
        "NAM models folder",
        "Where the NAM Amp Modeler lists .nam models ([docs/spec/nam-amp-and-cab.md])",
        &["nam_models_folder"],
        |cx, r| {
            let default = cx.about.nam_models_default.clone();
            cx.folder(r, &default, crate::interaction::pick::PickField::NamModelsFolder, |s| &mut s.nam_models_folder);
        },
    ),
    row(
        Page::General,
        "NAM",
        "IR folder",
        "Where the NAM Cab Loader lists .wav impulse responses",
        &["irs_folder"],
        |cx, r| {
            let default = cx.about.irs_default.clone();
            cx.folder(r, &default, crate::interaction::pick::PickField::IrFolder, |s| &mut s.irs_folder);
        },
    ),
    row(
        Page::General,
        "Backups",
        "Backups",
        "When a copy of each changed project is written to .backup beside it",
        &["backup"],
        |cx, r| {
            let options: Vec<(String, suhina_session::backup::Interval)> =
                suhina_session::backup::Interval::ALL.iter().map(|i| (i.title(), *i)).collect();
            cx.dropdown(r, &options, |s| &mut s.backup);
        },
    ),
    row(
        Page::General,
        "Backups",
        "Clean up backups",
        "Keep the newest 20 backups of each project and delete older ones",
        &["backup_clean_up"],
        |cx, r| cx.on_off(r, |s| &mut s.backup_clean_up),
    ),
    row(Page::General, "Backups", "Last backup", "The active project's newest backup", &[], |cx, _| {
        let note = cx.about.last_backup.clone();
        cx.note(note);
    }),
    row(
        Page::General,
        "Agents",
        "Let agents work on open projects",
        "An agent started with suhina-cli mcp --attach may work on the open projects once you allow it; you see every change and can undo it",
        &["agents"],
        |cx, r| cx.on_off(r, |s| &mut s.agents),
    ),
    row(
        Page::General,
        "Dropping files",
        "Several files",
        "Onto one track, or one track each; Ctrl while dropping does the other",
        &["drop_files"],
        |cx, r| cx.choose(r, &[("One track", false), ("One each", true)], |s| &mut s.drop_separate),
    ),
    row(
        Page::General,
        "Dropping files",
        "Warp mode",
        "How a sample with a known tempo is stretched to the project's",
        &["warp_mode"],
        |cx, r| {
            let options: Vec<(&str, WarpMode)> = WarpMode::ALL.iter().map(|m| (m.title(), *m)).collect();
            cx.choose(r, &options, |s| &mut s.warp_mode);
        },
    ),
    row(
        Page::General,
        "Updates",
        "Check for updates",
        "Once per start, asks GitHub whether a newer build is out; sends nothing about you",
        &["check_updates"],
        |cx, r| cx.on_off(r, |s| &mut s.updates.check),
    ),
    row(
        Page::General,
        "Updates",
        "Channel",
        "Which builds an update comes from: the nightly, or in developer mode a pull request's test build",
        &["update_channel"],
        |cx, r| {
            if cx.settings.updates.test_builds_listed() {
                let options = [("Nightly", Channel::Nightly), ("Test builds", Channel::TestBuilds)];
                cx.choose(r, &options, |s| &mut s.updates.channel);
            } else {
                cx.choose(r, &[("Nightly", Channel::Nightly)], |s| &mut s.updates.channel);
            }
        },
    ),
    row(
        Page::General,
        "Updates",
        "Developer mode",
        "Lists test builds of pull requests as an update channel; downloading one needs the GitHub CLI",
        &["developer_mode"],
        |cx, r| cx.on_off(r, |s| &mut s.updates.developer),
    ),
    // Audio.
    block(
        Page::Audio,
        "Output",
        "Output",
        "Driver, device, sample rate, buffer size, exclusive mode and the input device",
        &["audio_host", "audio_device", "audio_sample_rate", "audio_buffer", "audio_exclusive", "audio_input"],
        crate::settings::settings_view::audio_output,
    ),
    block(
        Page::Audio,
        "Cue",
        "Cue",
        "Where previews and cued tracks play, such as headphones, and how loud",
        &["audio_cue_pair", "audio_cue_volume"],
        crate::settings::settings_view::audio_cue,
    ),
    block(
        Page::Audio,
        "Now",
        "Now",
        "What plays now: the output, its latency and underruns, key to sound, the input",
        &[],
        crate::settings::settings_view::audio_now,
    ),
    // MIDI.
    block(
        Page::Midi,
        "MIDI inputs",
        "MIDI inputs",
        "Each MIDI controller's port, on or off; the ports on play the armed tracks",
        &["midi_input_off"],
        crate::settings::settings_view::midi_inputs,
    ),
    // Plug-ins.
    row(
        Page::Plugins,
        "Editors",
        "Editors",
        "In a window above Suhina, or inside the device panel as part of the chain",
        &["plugin_editors"],
        |cx, r| {
            if !cx.about.editors_embeddable {
                cx.note("Editors need Windows or an X server (DISPLAY); they do not open on this system yet");
            }
            let options = [("Window", EditorPlacement::Window), ("Device view", EditorPlacement::DeviceView)];
            cx.choose(r, &options, |s| &mut s.plugin_editors);
        },
    ),
    block(
        Page::Plugins,
        "Library",
        "Plug-in library",
        "The folders plug-ins are looked for in, each on or off and for a format, scanning them, and the plug-in manager",
        &[],
        crate::settings::settings_view::plugin_library,
    ),
    // Arrangement.
    row(
        Page::Arrangement,
        "Clips",
        "Allow sample stacking",
        "Audio clips moved over others all play, instead of replacing them",
        &["sample_stacking"],
        |cx, r| cx.on_off(r, |s| &mut s.sample_stacking),
    ),
    row(
        Page::Arrangement,
        "Clips",
        "Allow MIDI stacking",
        "MIDI clips moved over others all play, instead of replacing them",
        &["midi_stacking"],
        |cx, r| cx.on_off(r, |s| &mut s.midi_stacking),
    ),
    row(
        Page::Arrangement,
        "Clips",
        "Double-click makes a clip",
        "On an empty slot or an empty spot of a lane",
        &["double_click_clip"],
        |cx, r| cx.on_off(r, |s| &mut s.view.double_click_clip),
    ),
    row(
        Page::Arrangement,
        "Clips",
        "Name field in clip menus",
        "A clip's right-click menu carries its name to edit; off, Rename (Ctrl+R) edits the name on the clip",
        &["menu_name_field"],
        |cx, r| cx.on_off(r, |s| &mut s.view.menu_name_field),
    ),
    row(
        Page::Arrangement,
        "Clips",
        "Moved notes replace the notes under them",
        "In the piano roll, a note moved onto another of the same key and start takes its place; off, both play",
        &["notes_replace"],
        |cx, r| cx.on_off(r, |s| &mut s.view.notes_replace),
    ),
    row(
        Page::Arrangement,
        "Clips",
        "Ctrl+drag selects time over clips",
        "On, a plain drag moves clips and Ctrl+drag selects time; off, a drag from a lane's or a clip's upper half paints time and from the lower half moves clips or picks them. A drag on the ruler always selects time",
        &["time_select"],
        |cx, r| cx.on_off(r, |s| &mut s.view.time_select_ctrl),
    ),
    row(
        Page::Arrangement,
        "Clips",
        "Moving a clip",
        "Name bar: a clip moves by the bar with its name and is picked whole there (play then starts from its start), its body selects time and a click there sets where play starts; Mirror: its lower half moves it and its upper half selects time",
        &["clip_drag"],
        |cx, r| cx.choose(r, &[("Name bar", false), ("Mirror", true)], |s| &mut s.view.clip_halves),
    ),
    row(
        Page::Arrangement,
        "Clips",
        "Grid in clips",
        "The grid's lines show through the clips, faintly, so a clip's beats and bars are seen on it",
        &["clip_grid"],
        |cx, r| cx.on_off(r, |s| &mut s.view.grid_in_clips),
    ),
    row(
        Page::Arrangement,
        "Clips",
        "Waveform zoom",
        "How much larger audio clips' waveforms are drawn, the sound unchanged; also the thin slider beside the ruler, Ctrl+Alt+Up and Ctrl+Alt+Down",
        &["waveform_zoom"],
        |cx, r| cx.choose(r, &[("×1", 100), ("×2", 200), ("×4", 400), ("×8", 800)], |s| &mut s.view.waveform_zoom),
    ),
    row(
        Page::Arrangement,
        "Clips",
        "Alt-drag on an edge stretches audio",
        "An audio clip's edge dragged with Alt held time-stretches its audio to the new length, its pitch kept; off, Alt does nothing there",
        &["stretch_alt_drag"],
        |cx, r| cx.on_off(r, |s| &mut s.view.stretch_alt),
    ),
    row(
        Page::Arrangement,
        "Clips",
        "Ctrl+R renames every selected clip",
        "With several clips selected, the name typed after Ctrl+R goes to all of them, as one undo step; off, only the clip clicked last is renamed",
        &["rename_selected_clips"],
        |cx, r| cx.on_off(r, |s| &mut s.view.rename_selected),
    ),
    row(
        Page::Arrangement,
        "Clips",
        "Play from the selected clip",
        "Clips picked while stopped move the play start marker to the first one's start, so play starts there; off, a click on a clip puts it where it was clicked",
        &["play_from_selected_clip"],
        |cx, r| cx.on_off(r, |s| &mut s.view.play_from_clip),
    ),
    row(
        Page::Arrangement,
        "Tracks",
        "New track colour",
        "Alternating: each new track takes the next colour of the palette, by its place; fixed: every new track takes the palette's first colour. A track keeps the colour it was made with",
        &["new_track_colour"],
        |cx, r| cx.choose(r, &[("Alternating", false), ("Fixed", true)], |s| &mut s.view.fixed_track_colour),
    ),
    row(
        Page::Arrangement,
        "Tracks",
        "Track headers",
        "Which side of the timeline they are on",
        &["track_headers"],
        |cx, r| cx.choose(r, &[("Left", Side::Left), ("Right", Side::Right)], |s| &mut s.track_headers),
    ),
    row(
        Page::Arrangement,
        "Tracks",
        "Return tracks",
        "Whether the Arrangement shows the return tracks' rows along the bottom; its Hide returns button by the ruler switches it too",
        &["arrangement_returns"],
        |cx, r| cx.choose(r, &[("Shown", false), ("Hidden", true)], |s| &mut s.view.returns_hidden),
    ),
    // Session view and mixer.
    row(
        Page::Session,
        "Launching",
        "Select on launch",
        "Launching a clip in the Session view shows it in the detail area",
        &["select_on_launch"],
        |cx, r| cx.on_off(r, |s| &mut s.select_on_launch),
    ),
    row(
        Page::Session,
        "Mixer",
        "Mixer controls",
        "A track's volume and pan as a field, or as a knob: the wheel over either changes it",
        &["mixer_controls"],
        |cx, r| {
            let options = [("Field", MixerControls::Field), ("Knob", MixerControls::Knob)];
            cx.choose(r, &options, |s| &mut s.mixer_controls);
        },
    ),
    // Clip editor.
    row(
        Page::ClipEditor,
        "Piano roll",
        "Pulled to the top",
        "What stays above the piano roll when its splitter is dragged all the way up",
        &["piano_roll_top"],
        |cx, r| {
            let options = [("Edited track", PianoRollTop::Track), ("Whole window", PianoRollTop::Window)];
            cx.choose(r, &options, |s| &mut s.piano_roll_top);
        },
    ),
    row(
        Page::ClipEditor,
        "Piano roll",
        "Piano roll grid",
        "Its own grid value, or the one in the control bar, which otherwise sets the Arrangement view's only",
        &["piano_roll_grid"],
        |cx, r| cx.choose(r, &[("Its own", false), ("Control bar", true)], |s| &mut s.piano_grid_follows),
    ),
    row(
        Page::ClipEditor,
        "Piano roll",
        "Toolbar size",
        "The toolbar's box, its gaps and the icon strokes inside it, scaled together",
        &["piano_roll_toolbar"],
        |cx, r| {
            let options: Vec<(&str, ToolbarSize)> = ToolbarSize::ALL.iter().map(|s| (s.name(), *s)).collect();
            cx.choose(r, &options, |s| &mut s.toolbar_size);
        },
    ),
    block(
        Page::ClipEditor,
        "Workflow",
        "Workflow settings",
        "Click and double-click on the grid and on notes, the right and Alt+right buttons, the duplicate key",
        KEYMAP,
        crate::settings::settings_view::mouse_switches,
    ),
    // Transport.
    row(
        Page::Transport,
        "Play and stop",
        "Play from",
        "Where Play starts: the play start marker, or the start of the clip selected in the Arrangement view",
        &["play_start"],
        |cx, r| {
            let options = [("Marker", PlayFrom::Marker), ("Selected clip", PlayFrom::Clip)];
            cx.choose(r, &options, |s| &mut s.play_from);
        },
    ),
    row(
        Page::Transport,
        "Play and stop",
        "Stop",
        "Back to the play start marker, or stay where it stopped; stop again for the start",
        &["stop"],
        |cx, r| cx.choose(r, &[("Marker", StopReturn::Marker), ("Stay", StopReturn::Here)], |s| &mut s.stop),
    ),
    // Detail area.
    row(
        Page::DetailArea,
        "Devices",
        "Device chain",
        "In a row under the view, or stacked in a column beside it",
        &["device_chain"],
        |cx, r| {
            let options = [("Row", DeviceLayout::Row), ("Stack", DeviceLayout::Stack)];
            cx.choose(r, &options, |s| &mut s.device_layout);
        },
    ),
    row(
        Page::DetailArea,
        "Devices",
        "Detail area layout",
        "Beside the browser, or across the whole window under it",
        &["detail_layout"],
        |cx, r| {
            let options = [("Beside", DetailLayout::Beside), ("Full width", DetailLayout::FullWidth)];
            cx.choose(r, &options, |s| &mut s.view.detail_layout);
        },
    ),
    // Browser.
    row(
        Page::Browser,
        "Browser",
        "Browser side",
        "Which side of the window the browser is on",
        &["browser_side"],
        |cx, r| cx.choose(r, &[("Left", Side::Left), ("Right", Side::Right)], |s| &mut s.browser_side),
    ),
    row(
        Page::Browser,
        "Browser",
        "Recent's kinds",
        "Indented: Samples, MIDI and the other kinds of recent items are items under Recent in the sidebar; chips: they are chips over the list while Recent is chosen",
        &["browser_recent"],
        |cx, r| cx.choose(r, &[("Indented", false), ("Chips", true)], |s| &mut s.view.recent_chips),
    ),
    row(
        Page::Browser,
        "Previews",
        "Auto-play",
        "Selecting a sample or MIDI file previews it",
        &["preview_autoplay"],
        |cx, r| cx.on_off(r, |s| &mut s.preview_autoplay),
    ),
    row(
        Page::Browser,
        "Previews",
        "Preview volume",
        "How loud previews play, after the main channel",
        &["preview_volume"],
        |cx, r| {
            let options = [("-18 dB", -18), ("-12 dB", -12), ("-6 dB", -6), ("0 dB", 0)];
            cx.choose(r, &options, |s| &mut s.preview_volume);
        },
    ),
    row(
        Page::Browser,
        "Sample map",
        "Map layout",
        "The sample map as a free cloud or snapped to a grid",
        &["map_layout"],
        |cx, r| cx.choose(r, &[("Cloud", false), ("Grid", true)], |s| &mut s.map_grid),
    ),
    row(
        Page::Browser,
        "Sample map",
        "Map preview on hover",
        "Pointing at a sample on the map previews it",
        &["map_hover_preview"],
        |cx, r| cx.on_off(r, |s| &mut s.map_hover_preview),
    ),
    row(
        Page::Browser,
        "Sample map",
        "Map: longest sample",
        "Longer samples are left off the map",
        &["map_max_seconds"],
        |cx, r| {
            let options = [("5 s", 5u32), ("10 s", 10), ("30 s", 30), ("60 s", 60)];
            cx.choose(r, &options, |s| &mut s.map_max_seconds);
        },
    ),
    row(Page::Browser, "Tags", "Tag limit", "The most colour tags there may be", &[], |cx, r| {
        let options = [("64", 64usize), ("128", 128), ("256", 256), ("1024", 1024)];
        cx.tag_limit(r, &options);
    }),
    // Appearance.
    row(
        Page::Appearance,
        "Window",
        "Zoom",
        "The size of the whole window; Ctrl and + or -, Ctrl and 0 for 100 %",
        &["zoom"],
        |cx, r| cx.zoom(r),
    ),
    row(
        Page::Appearance,
        "Window",
        "Animations",
        "Short fades and slides when things change",
        &["animations"],
        |cx, r| cx.on_off(r, |s| &mut s.animations),
    ),
    row(
        Page::Appearance,
        "Waveforms",
        "Waveform scale",
        "Linear, or decibels so quiet parts show; clips, previews and the map",
        &["waveform_scale"],
        |cx, r| cx.choose(r, &[("Linear", false), ("dB", true)], |s| &mut s.waveform_db),
    ),
    row(
        Page::Appearance,
        "Graphics",
        "Renderer",
        "How the window is drawn; OpenGL can resize more smoothly on some systems",
        &["renderer"],
        |cx, r| {
            if cx.about.graphics_now.is_some_and(|now| now != cx.settings.graphics) {
                cx.note("Applies the next time Suhina starts");
            }
            cx.choose(r, &[("GPU", Graphics::Gpu), ("OpenGL", Graphics::OpenGl)], |s| &mut s.graphics);
        },
    ),
    // Theme.
    block(
        Page::Theme,
        "Theme",
        "Theme",
        "The themes to pick from, the theme in use and its colours",
        &["theme"],
        crate::settings::settings_view::theme_block,
    ),
    // Shortcuts.
    block(
        Page::Shortcuts,
        "Keys",
        "Shortcuts",
        "Every action's keys and the preset they come from",
        KEYMAP,
        crate::settings::settings_view::shortcuts_block,
    ),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;

    /// Kept in `settings.json` but changed outside the Settings window: the browser and the map
    /// window as they were left.
    const NOT_IN_THE_WINDOW: [&str; 7] = [
        "browser",
        "browser_width",
        "map_dot_size",
        "map_window",
        "map_window_rect",
        "session_mixer_height",
        "session_strip_fader",
    ];

    fn saved_keys() -> Vec<String> {
        let settings = Settings { midi_inputs_off: vec!["Keys".into()], ..Settings::default() };
        settings
            .to_text()
            .lines()
            .filter(|line| !line.starts_with('#'))
            .filter_map(|line| line.split_once('=').map(|(key, _)| key.trim().to_string()))
            .collect()
    }

    #[test]
    fn every_saved_setting_is_on_exactly_one_page() {
        for key in saved_keys() {
            let on: Vec<&str> = ROWS.iter().filter(|e| e.keys.contains(&key.as_str())).map(|e| e.name).collect();
            if NOT_IN_THE_WINDOW.contains(&key.as_str()) {
                assert!(on.is_empty(), "{key} is changed outside the window, yet shown by {on:?}");
            } else {
                assert_eq!(on.len(), 1, "{key} is shown by {on:?}");
            }
        }
        // And every key an entry names is one the settings file has, or the keymap.
        let saved = saved_keys();
        for entry in ROWS {
            for key in entry.keys {
                assert!(saved.iter().any(|k| k == key) || KEYMAP.contains(key), "{}: no saved key {key}", entry.name);
            }
        }
    }

    #[test]
    fn every_entry_has_a_name_of_its_own_and_its_sections_stay_together() {
        let mut names: Vec<&str> = ROWS.iter().map(|e| e.name).collect();
        names.sort_unstable();
        let count = names.len();
        names.dedup();
        assert_eq!(names.len(), count, "two entries share a name");
        // A page's sections and the pages themselves come in one run each, in the list's order.
        let mut seen: Vec<(Page, &str)> = Vec::new();
        for entry in ROWS {
            let here = (entry.page, entry.section);
            if seen.last() != Some(&here) {
                assert!(!seen.contains(&here), "{} › {} is split in two", entry.page.name(), entry.section);
                seen.push(here);
            }
        }
        let order: Vec<Page> = Page::ALL.iter().map(|(page, _)| *page).collect();
        let mut pages: Vec<Page> = seen.iter().map(|(page, _)| *page).collect();
        pages.dedup();
        assert_eq!(pages, order, "every page has settings, in the order of the list");
    }
}
