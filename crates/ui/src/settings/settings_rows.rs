// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Every setting the Settings window shows, in one table: its page, its section, its name and
//! note, the keys it is saved under and how its control is drawn (`docs/spec/ui-settings.md`).
//! A page shows its entries in this order, a new section heading where the section changes.
//!
//! Adding a setting is one entry under its section, most often a `row` whose control is one call:
//! `cx.on_off(r, |s| &mut s.field)` or `cx.choose(r, &[("Text", value), …], |s| &mut s.field)`.

#[cfg(test)]
use crate::settings::settings_view::{ADVANCED, Role};
use crate::settings::settings_view::{Entry, Page, block, row};
use crate::settings::track_sizes::TrackSize;
use crate::settings::view_settings::{DetailLayout, FolderDrop, WaveformColour};
use crate::settings::{
    AtStart, DeviceLayout, EditorPlacement, Graphics, MixerControls, OnOpen, PianoRollTop, PlayFrom, Side, StopReturn,
    ToolbarSize, WarpMode,
};
use suhina_update::Channel;

/// The keys of `keymap.json`, where the keys and the mouse switches are saved.
pub(crate) const KEYMAP: &[&str] = &["keymap.json"];

/// Every setting the window shows, page by page.
pub(crate) static ROWS: &[Entry] = &[
    row(
        Page::Projects,
        "Projects",
        "Open projects in",
        "What happens when you open a project while another one is open",
        &["open_projects"],
        |cx, r| {
            let options = [("Ask", OnOpen::Ask), ("New tab", OnOpen::NewTab), ("Replace", OnOpen::Replace)];
            cx.choose(r, &options, |s| &mut s.on_open);
        },
    ),
    row(
        Page::Projects,
        "Projects",
        "Startup project",
        "What Suhina shows when it starts without a project to open",
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
        Page::Projects,
        "Projects",
        "Projects folder",
        "The folder a new project is saved in unless you choose another",
        &["projects_folder"],
        |cx, r| {
            let default = cx.about.projects_default.clone();
            cx.folder(r, &default, crate::interaction::pick::PickField::ProjectsFolder, |s| &mut s.projects_folder);
        },
    ),
    row(
        Page::Projects,
        "Projects",
        "Templates folder",
        "The folder that holds the projects you can start a new project from",
        &["templates_folder"],
        |cx, r| {
            let default = cx.about.templates_default.clone();
            cx.folder(r, &default, crate::interaction::pick::PickField::TemplatesFolder, |s| &mut s.templates_folder);
        },
    ),
    // Settings folder is named beside the default folder, not in the settings file: no saved key.
    row(
        Page::Projects,
        "Projects",
        "Settings folder",
        "The folder Suhina keeps its settings, keys, recent projects and themes in, used from the next start",
        &[],
        |cx, r| {
            let (default, value) = (cx.about.data_default.clone(), cx.settings.data_folder.clone());
            cx.moved_folder(r, &default, crate::interaction::pick::PickField::DataFolder, &value);
        },
    ),
    row(
        Page::Projects,
        "Projects",
        "Startup template",
        "The template a new project is made from when Suhina starts with one",
        &["start_template"],
        |cx, r| {
            let hint = "A template's file name, or a path";
            cx.folder(r, hint, crate::interaction::pick::PickField::StartTemplate, |s| &mut s.start_template);
        },
    )
    .when(|s| s.at_start == AtStart::Template),
    row(
        Page::Projects,
        "Backups",
        "Backup interval",
        "How often Suhina saves a spare copy of each changed project next to it",
        &["backup"],
        |cx, r| {
            let options: Vec<(String, suhina_session::backup::Interval)> =
                suhina_session::backup::Interval::ALL.iter().map(|i| (i.title(), *i)).collect();
            let last = cx.about.last_backup.clone();
            cx.note(format!(
                "When a copy of each changed project is written to .backup beside it; last backup: {last}"
            ));
            cx.dropdown(r, &options, |s| &mut s.backup);
        },
    ),
    row(
        Page::Projects,
        "Backups",
        "Delete old backups",
        "Suhina keeps the newest 20 spare copies of each project and deletes older ones",
        &["backup_clean_up"],
        |cx, r| cx.on_off(r, |s| &mut s.backup_clean_up),
    ),
    block(
        Page::Audio,
        "Status",
        "Audio status",
        "The output and input in use, how long sound takes to come out, and any dropouts",
        &[],
        crate::settings::settings_view::audio_now,
    )
    .status(),
    block(
        Page::Audio,
        "Output",
        "Audio device",
        "The sound card Suhina plays through and records from, and how it is set up",
        &[
            "audio_host",
            "audio_device",
            "audio_devices",
            "audio_sample_rate",
            "audio_buffer",
            "audio_exclusive",
            "audio_input",
        ],
        crate::settings::settings_view::audio_output,
    ),
    block(
        Page::Audio,
        "Cue",
        "Headphone output",
        "A second pair of outputs, such as headphones, for previews and cued tracks",
        &["audio_cue_pair", "audio_cue_volume"],
        crate::settings::settings_view::audio_cue,
    ),
    block(
        Page::Midi,
        "MIDI inputs",
        "MIDI inputs",
        "The MIDI keyboards and controllers Suhina listens to",
        &["midi_input_off"],
        crate::settings::settings_view::midi_inputs,
    ),
    row(
        Page::Midi,
        "Arming",
        "Arm selected track",
        "Selecting a track gets it ready to record, so your MIDI keyboard plays its instrument",
        &["auto_arm"],
        |cx, r| cx.on_off(r, |s| &mut s.auto_arm),
    ),
    block(
        Page::Plugins,
        "Actions",
        "Plug-in actions",
        "Look for new plug-ins, or open the list of every plug-in Suhina found",
        &[],
        crate::settings::settings_view::plugin_buttons,
    )
    .actions(),
    block(
        Page::Plugins,
        "Folders",
        "Plug-in folders",
        "The folders Suhina looks in for plug-ins",
        &[],
        crate::settings::settings_view::plugin_library,
    ),
    row(
        Page::Plugins,
        "Editors",
        "Plug-in editors open in",
        "Whether a plug-in opens in a window of its own or inside the device panel",
        &["plugin_editors"],
        |cx, r| {
            if !cx.about.editors_embeddable {
                cx.note("Editors need Windows or an X server (DISPLAY); they do not open on this system yet");
            }
            let options = [("Window", EditorPlacement::Window), ("Device view", EditorPlacement::DeviceView)];
            cx.choose(r, &options, |s| &mut s.plugin_editors);
        },
    ),
    row(
        Page::Devices,
        "NAM",
        "Amp models folder",
        "The folder the NAM Amp Modeler lists its amp models from",
        &["nam_models_folder"],
        |cx, r| {
            let default = cx.about.nam_models_default.clone();
            cx.folder(r, &default, crate::interaction::pick::PickField::NamModelsFolder, |s| &mut s.nam_models_folder);
        },
    ),
    row(
        Page::Devices,
        "NAM",
        "Cabinet sounds folder",
        "The folder the NAM Cab Loader lists its speaker cabinet recordings from",
        &["irs_folder"],
        |cx, r| {
            let default = cx.about.irs_default.clone();
            cx.folder(r, &default, crate::interaction::pick::PickField::IrFolder, |s| &mut s.irs_folder);
        },
    ),
    row(
        Page::Devices,
        "Device panels",
        "Device chain layout",
        "Whether a track's devices sit in a row under the view or in a column beside it",
        &["device_chain"],
        |cx, r| {
            let options = [("Row", DeviceLayout::Row), ("Stack", DeviceLayout::Stack)];
            cx.choose(r, &options, |s| &mut s.device_layout);
        },
    ),
    row(
        Page::Devices,
        "Device panels",
        "Detail area width",
        "Whether the area under the view stops at the browser or spans the whole window",
        &["detail_layout"],
        |cx, r| {
            let options = [("Beside", DetailLayout::Beside), ("Full width", DetailLayout::FullWidth)];
            cx.choose(r, &options, |s| &mut s.view.detail_layout);
        },
    ),
    row(
        Page::Importing,
        "Dropping files",
        "Tracks for dropped files",
        "Whether files you drop together go on one track or each on a track of its own",
        &["drop_files"],
        |cx, r| cx.choose(r, &[("One track", false), ("One each", true)], |s| &mut s.drop_separate),
    ),
    row(
        Page::Importing,
        "Dropping files",
        "Warp mode for samples",
        "How a dropped sample with a known tempo is sped up or slowed down to match the project",
        &["warp_mode"],
        |cx, r| {
            let options: Vec<(&str, WarpMode)> = WarpMode::ALL.iter().map(|m| (m.title(), *m)).collect();
            cx.choose(r, &options, |s| &mut s.warp_mode);
        },
    ),
    row(
        Page::Arrangement,
        "Clip editing",
        "Let audio clips overlap",
        "An audio clip moved over others plays along with them instead of replacing them",
        &["sample_stacking"],
        |cx, r| cx.on_off(r, |s| &mut s.sample_stacking),
    ),
    row(
        Page::Arrangement,
        "Clip editing",
        "Let MIDI clips overlap",
        "A MIDI clip moved over others plays along with them instead of replacing them",
        &["midi_stacking"],
        |cx, r| cx.on_off(r, |s| &mut s.midi_stacking),
    ),
    row(
        Page::Arrangement,
        "Clip editing",
        "Double-click makes a clip",
        "Double-clicking an empty place in a lane or an empty slot makes a new clip there",
        &["double_click_clip"],
        |cx, r| cx.on_off(r, |s| &mut s.view.double_click_clip),
    ),
    row(
        Page::Arrangement,
        "Clip editing",
        "Clip menu name field",
        "A clip's right-click menu has a field for its name",
        &["menu_name_field"],
        |cx, r| cx.on_off(r, |s| &mut s.view.menu_name_field),
    ),
    row(
        Page::Arrangement,
        "Clip editing",
        "Clip drag handle",
        "Which part of a clip you drag to move it, while dragging the rest of it selects time",
        &["clip_drag"],
        |cx, r| cx.choose(r, &[("Name bar", false), ("Lower half", true)], |s| &mut s.view.clip_halves),
    ),
    row(
        Page::Arrangement,
        "Clip editing",
        "Paste replaces clip",
        "Pasting onto a selected clip puts the copy in its place instead of after it",
        &["replace_clip"],
        |cx, r| cx.on_off(r, |s| &mut s.view.replace_clip),
    ),
    row(
        Page::Arrangement,
        "Time and play",
        "Select time with",
        "How a drag over a clip selects time instead of moving the clip",
        &["time_select"],
        |cx, r| cx.choose(r, &[("Ctrl+drag", true), ("Upper half", false)], |s| &mut s.view.time_select_ctrl),
    ),
    row(
        Page::Arrangement,
        "Time and play",
        "Start marker follows selection",
        "While stopped, selecting a clip moves the play start marker to the clip's start",
        &["play_from_selected_clip"],
        |cx, r| cx.on_off(r, |s| &mut s.view.play_from_clip),
    ),
    row(
        Page::Arrangement,
        "Time and play",
        "Play from",
        "Where playback starts when you press Play",
        &["play_start"],
        |cx, r| {
            let options = [("Marker", PlayFrom::Marker), ("Selected clip", PlayFrom::Clip)];
            cx.choose(r, &options, |s| &mut s.play_from);
        },
    ),
    row(
        Page::Arrangement,
        "Time and play",
        "Playhead after Stop",
        "Where the playhead goes when you press Stop",
        &["stop"],
        |cx, r| cx.choose(r, &[("Marker", StopReturn::Marker), ("Stay", StopReturn::Here)], |s| &mut s.stop),
    ),
    row(
        Page::Arrangement,
        "View",
        "Grid inside clips",
        "The beat and bar lines show faintly through clips",
        &["clip_grid"],
        |cx, r| cx.on_off(r, |s| &mut s.view.grid_in_clips),
    ),
    row(
        Page::Arrangement,
        "View",
        "Waveform size",
        "How large audio clips' waveforms are drawn, without changing the sound",
        &["waveform_zoom"],
        |cx, r| cx.choose(r, &[("×1", 100), ("×2", 200), ("×4", 400), ("×8", 800)], |s| &mut s.view.waveform_zoom),
    ),
    row(
        Page::Arrangement,
        "View",
        "Waveform and note colour",
        "The colour waveforms and notes are drawn in on clips",
        &["waveform_colour"],
        |cx, r| {
            cx.choose(
                r,
                &[
                    ("Theme", WaveformColour::Theme),
                    ("Auto", WaveformColour::Auto),
                    ("Light", WaveformColour::Light),
                    ("Dark", WaveformColour::Dark),
                    ("Custom", WaveformColour::Custom),
                ],
                |s| &mut s.view.waveform.mode,
            )
        },
    ),
    row(
        Page::Arrangement,
        "View",
        "Custom waveform colour",
        "The colour waveforms and notes take when their colour is Custom",
        &["waveform_custom_colour"],
        |cx, r| cx.waveform_colour(r),
    )
    .when(|s| s.view.waveform.mode == WaveformColour::Custom),
    row(
        Page::Arrangement,
        "View",
        "Audio clip colour strength",
        "How strongly audio clips show their own colour",
        &["audio_clip_colour_strength"],
        |cx, r| cx.colour_strength(r, |s| &mut s.view.audio_colour_strength),
    ),
    row(
        Page::Arrangement,
        "Tracks",
        "New track colour",
        "Whether new tracks take the theme's colour for their kind or all take the same colour",
        &["new_track_colour"],
        |cx, r| cx.choose(r, &[("By clip type", false), ("One colour", true)], |s| &mut s.view.fixed_track_colour),
    ),
    row(
        Page::Arrangement,
        "Tracks",
        "New clip colour",
        "Whether a new clip takes its track's colour or the theme's clip colour",
        &["new_clip_colour"],
        |cx, r| {
            cx.choose(r, &[("Track colour", true), ("Theme default", false)], |s| &mut s.view.new_clip_track_colour)
        },
    ),
    row(
        Page::Arrangement,
        "Tracks",
        "Track header colour strength",
        "How strongly track headers show their own colour",
        &["track_header_colour_strength"],
        |cx, r| cx.colour_strength(r, |s| &mut s.view.header_colour_strength),
    ),
    row(
        Page::Arrangement,
        "Tracks",
        "Track header side",
        "Which side of the timeline the track names and controls are on",
        &["track_headers"],
        |cx, r| {
            cx.choose(r, &[("Theme", None), ("Left", Some(Side::Left)), ("Right", Some(Side::Right))], |s| {
                &mut s.track_headers
            })
        },
    ),
    row(
        Page::Arrangement,
        "Tracks",
        "Return tracks",
        "The return tracks' rows show along the bottom of the Arrangement view",
        &["arrangement_returns"],
        |cx, r| cx.choose(r, &[("Shown", false), ("Hidden", true)], |s| &mut s.view.returns_hidden),
    ),
    row(
        Page::Arrangement,
        "Tracks",
        "Returns in new projects",
        "Whether a new blank project starts with the A and B return tracks or with none",
        &["new_project_returns"],
        |cx, r| cx.choose(r, &[("A and B", true), ("None", false)], |s| &mut s.view.new_returns),
    ),
    row(
        Page::Arrangement,
        "Tracks",
        "Alt+wheel resizes",
        "Whether Alt and the mouse wheel resize all tracks and their lanes or only the one under the pointer",
        &["alt_wheel"],
        |cx, r| {
            cx.choose(r, &[("All tracks", false), ("The one under the pointer", true)], |s| &mut s.view.alt_wheel_one)
        },
    ),
    row(
        Page::Arrangement,
        "Track sizes",
        "Default track size",
        "The named height ordinary tracks are drawn at",
        &["arrangement_track_size"],
        |cx, r| {
            cx.choose(
                r,
                &[
                    ("Tiny", TrackSize::Tiny),
                    ("Compact", TrackSize::Compact),
                    ("Small", TrackSize::Small),
                    ("Medium", TrackSize::Medium),
                    ("Large", TrackSize::Large),
                ],
                |s| &mut s.view.track_sizes.tracks,
            );
        },
    ),
    row(
        Page::Arrangement,
        "Track sizes",
        "Default return size",
        "The named height return tracks are drawn at",
        &["arrangement_return_size"],
        |cx, r| {
            cx.choose(
                r,
                &[
                    ("Tiny", TrackSize::Tiny),
                    ("Compact", TrackSize::Compact),
                    ("Small", TrackSize::Small),
                    ("Medium", TrackSize::Medium),
                    ("Large", TrackSize::Large),
                ],
                |s| &mut s.view.track_sizes.returns,
            );
        },
    ),
    row(
        Page::Arrangement,
        "Track sizes",
        "Tiny height",
        "The height of the smallest track size, always below Compact",
        &["arrangement_size_heights"],
        |cx, r| cx.track_size_height(r, TrackSize::Tiny),
    ),
    row(
        Page::Arrangement,
        "Track sizes",
        "Compact height",
        "The height of Compact, between Tiny and Small",
        &[],
        |cx, r| cx.track_size_height(r, TrackSize::Compact),
    ),
    row(
        Page::Arrangement,
        "Track sizes",
        "Small height",
        "The height of Small, between Compact and Medium",
        &[],
        |cx, r| cx.track_size_height(r, TrackSize::Small),
    ),
    row(
        Page::Arrangement,
        "Track sizes",
        "Medium height",
        "The height of Medium, between Small and Large",
        &[],
        |cx, r| cx.track_size_height(r, TrackSize::Medium),
    ),
    row(
        Page::Arrangement,
        "Track sizes",
        "Large height",
        "The height of the largest track size, always above Medium",
        &[],
        |cx, r| cx.track_size_height(r, TrackSize::Large),
    ),
    row(
        Page::Arrangement,
        "Tools",
        "Toolbar",
        "A row of tools over the Arrangement (Select, Range, Split, Erase, Draw, Mute) that the keys 1 to 6 choose while it has the focus",
        &["arrangement_toolbar"],
        |cx, r| cx.on_off(r, |s| &mut s.view.arrangement_toolbar),
    ),
    row(
        Page::Arrangement,
        "Tools",
        "Default grid",
        "The Arrangement's grid when Suhina starts, a fixed value or one that follows the zoom",
        &["arrangement_grid"],
        |cx, r| {
            let names: Vec<String> = super::view_settings::DEFAULT_GRIDS
                .iter()
                .map(|g| {
                    let mut name = g.to_string();
                    if let Some(first) = name.get_mut(..1) {
                        first.make_ascii_uppercase();
                    }
                    name
                })
                .collect();
            let options: Vec<(&str, &'static str)> =
                names.iter().zip(super::view_settings::DEFAULT_GRIDS).map(|(n, g)| (n.as_str(), g)).collect();
            cx.choose(r, &options, |s| &mut s.view.default_grid)
        },
    ),
    row(
        Page::Arrangement,
        "Automation",
        "Click anywhere adds",
        "Whether a click anywhere on an automation lane adds a point, not only a click on or near the line",
        &["automation_click"],
        |cx, r| cx.on_off(r, |s| &mut s.view.automation_click_anywhere),
    ),
    row(
        Page::Arrangement,
        "Advanced",
        "Edge drag stretches audio",
        "Dragging an audio clip's edge with Ctrl and Alt held stretches its sound to the new length",
        &["stretch_alt_drag"],
        |cx, r| cx.on_off(r, |s| &mut s.view.stretch_alt),
    ),
    row(
        Page::Arrangement,
        "Advanced",
        "Rename all selected clips",
        "Renaming a clip while several are selected gives them all the new name",
        &["rename_selected_clips"],
        |cx, r| cx.on_off(r, |s| &mut s.view.rename_selected),
    ),
    row(
        Page::Arrangement,
        "Advanced",
        "Exact track heights",
        "The track size heights are typed in pixels instead of set with sliders",
        &["arrangement_sizes_advanced"],
        |cx, r| cx.on_off(r, |s| &mut s.view.track_sizes.advanced),
    ),
    row(
        Page::Arrangement,
        "Advanced",
        "Copy names",
        "How a duplicated or pasted track is named when its name is taken, Bass (2) or Bass #2",
        &["copy_names"],
        |cx, r| cx.choose(r, &[("Wrapped (2)", true), ("Hash #2", false)], |s| &mut s.view.copy_wrapped),
    ),
    row(
        Page::Session,
        "Launching",
        "Select launched clip",
        "Starting a clip in the Launcher view also selects it and shows it in the detail area",
        &["select_on_launch"],
        |cx, r| cx.on_off(r, |s| &mut s.select_on_launch),
    ),
    row(
        Page::Session,
        "Mixer",
        "Mixer control style",
        "Whether each track's volume and pan show as numbers or as knobs",
        &["mixer_controls"],
        |cx, r| {
            let options = [("Field", MixerControls::Field), ("Knob", MixerControls::Knob)];
            cx.choose(r, &options, |s| &mut s.mixer_controls);
        },
    ),
    row(
        Page::ClipEditor,
        "Piano roll",
        "Full-height piano roll covers",
        "What the piano roll covers when you drag its divider all the way up",
        &["piano_roll_top"],
        |cx, r| {
            let options = [("Edited track", PianoRollTop::Track), ("Whole window", PianoRollTop::Window)];
            cx.choose(r, &options, |s| &mut s.piano_roll_top);
        },
    ),
    row(
        Page::ClipEditor,
        "Piano roll",
        "Piano roll grid source",
        "Whether the piano roll has its own grid or follows the one in the control bar",
        &["piano_roll_grid"],
        |cx, r| cx.choose(r, &[("Its own", false), ("Control bar", true)], |s| &mut s.piano_grid_follows),
    ),
    row(
        Page::ClipEditor,
        "Piano roll",
        "Piano roll toolbar size",
        "How large the piano roll's toolbar buttons are",
        &["piano_roll_toolbar"],
        |cx, r| {
            let options: Vec<(&str, ToolbarSize)> = ToolbarSize::ALL.iter().map(|s| (s.name(), *s)).collect();
            cx.choose(r, &options, |s| &mut s.toolbar_size);
        },
    ),
    row(
        Page::ClipEditor,
        "Piano roll",
        "Moved notes replace notes",
        "A note moved onto another at the same pitch and time takes its place",
        &["notes_replace"],
        |cx, r| cx.on_off(r, |s| &mut s.view.notes_replace),
    ),
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
        "Recent item kinds layout",
        "Whether the kinds of recent items show in the sidebar or as buttons above the list",
        &["browser_recent"],
        |cx, r| cx.choose(r, &[("Indented", false), ("Chips", true)], |s| &mut s.view.recent_chips),
    ),
    row(
        Page::Browser,
        "Browser",
        "Dropping on a folder",
        "Whether samples dragged onto a browser folder ask each time, move or copy (Ctrl copies and Shift moves)",
        &["browser_folder_drop"],
        |cx, r| {
            cx.choose(r, &[("Ask", FolderDrop::Ask), ("Move", FolderDrop::Move), ("Copy", FolderDrop::Copy)], |s| {
                &mut s.view.folder_drop
            });
        },
    ),
    row(
        Page::Browser,
        "Previews",
        "Preview selected file",
        "Selecting a sample or MIDI file plays it",
        &["preview_autoplay"],
        |cx, r| cx.on_off(r, |s| &mut s.preview_autoplay),
    ),
    row(
        Page::Browser,
        "Previews",
        "Preview volume",
        "How loud samples play when you preview them",
        &["preview_volume"],
        |cx, r| {
            let options = [("-18 dB", -18), ("-12 dB", -12), ("-6 dB", -6), ("0 dB", 0)];
            cx.choose(r, &options, |s| &mut s.preview_volume);
        },
    ),
    row(
        Page::Browser,
        "Sample map",
        "Sample map layout",
        "Whether the sample map shows its points as a free cloud or on a grid",
        &["map_layout"],
        |cx, r| cx.choose(r, &[("Cloud", false), ("Grid", true)], |s| &mut s.map_grid),
    ),
    row(
        Page::Browser,
        "Sample map",
        "Preview on map hover",
        "Pointing at a sample on the sample map plays it",
        &["map_hover_preview"],
        |cx, r| cx.on_off(r, |s| &mut s.map_hover_preview),
    ),
    row(
        Page::Browser,
        "Sample map",
        "Map sample length limit",
        "Samples longer than this are left off the sample map",
        &["map_max_seconds"],
        |cx, r| {
            let options = [("5 s", 5u32), ("10 s", 10), ("30 s", 30), ("60 s", 60)];
            cx.choose(r, &options, |s| &mut s.map_max_seconds);
        },
    ),
    row(
        Page::Browser,
        "Sample map",
        "Map labels",
        "How large the names of the groups in the sample map are, or Off to hide them",
        &["map_label_size"],
        |cx, r| {
            let options = [("Off", 0u32), ("50 %", 50), ("75 %", 75), ("100 %", 100)];
            cx.choose(r, &options, |s| &mut s.map_label_size);
        },
    ),
    row(Page::Browser, "Tags", "Colour tag limit", "How many colour tags you can make in the browser", &[], |cx, r| {
        let options = [("64", 64usize), ("128", 128), ("256", 256), ("1024", 1024)];
        cx.tag_limit(r, &options);
    }),
    block(
        Page::Keys,
        "Mouse and key behaviour",
        "Piano roll mouse behaviour",
        "What clicks, double-clicks and right-clicks do to notes in the piano roll",
        KEYMAP,
        crate::settings::settings_view::mouse_switches,
    ),
    block(
        Page::Keys,
        "Shortcuts",
        "Shortcuts",
        "The keys for every action",
        KEYMAP,
        crate::settings::settings_view::shortcuts_block,
    ),
    row(
        Page::Appearance,
        "Window",
        "Window zoom",
        "How large everything in the window is drawn",
        &["zoom"],
        |cx, r| cx.zoom(r),
    ),
    row(
        Page::Appearance,
        "Window",
        "Animations",
        "Things fade and slide briefly when they change",
        &["animations"],
        |cx, r| cx.on_off(r, |s| &mut s.animations),
    ),
    row(
        Page::Appearance,
        "Window",
        "Menu bar in window",
        "The menus show at the top of the window, and on macOS in the screen's menu bar too",
        &["menu_bar"],
        |cx, r| cx.on_off(r, |s| &mut s.view.menu_bar),
    ),
    row(
        Page::Appearance,
        "Waveforms",
        "Waveform scale",
        "Whether waveforms are drawn to show quiet parts larger or as they are",
        &["waveform_scale"],
        |cx, r| cx.choose(r, &[("Linear", false), ("dB", true)], |s| &mut s.waveform_db),
    ),
    row(
        Page::Appearance,
        "Background",
        "Background image",
        "A picture of your own, a PNG file, drawn behind the window's panels",
        &["background_image"],
        |cx, r| {
            cx.folder(r, "No image", crate::interaction::pick::PickField::BackgroundImage, |s| &mut s.background_image)
        },
    ),
    row(
        Page::Appearance,
        "Background",
        "Surface opacity",
        "How much the panels cover the background image",
        &["background_tint"],
        |cx, r| cx.colour_strength(r, |s| &mut s.view.background_tint),
    ),
    row(
        Page::Appearance,
        "Background",
        "Background blur",
        "How much the background image is blurred",
        &["background_blur"],
        |cx, r| cx.background_blur(r),
    ),
    row(
        Page::Appearance,
        "Application bar",
        "Application name position",
        "Where the name Suhina sits in the bar at the top of the window",
        &["app_name_centered"],
        |cx, r| cx.choose(r, &[("Left", false), ("Centre", true)], |s| &mut s.view.app_name_centered),
    ),
    row(
        Page::Appearance,
        "Application bar",
        "Show project name",
        "The bar at the top of the window shows the open project's name and whether it has unsaved changes",
        &["bar_project_name"],
        |cx, r| cx.on_off(r, |s| &mut s.view.bar_project_name),
    ),
    row(
        Page::Appearance,
        "Application bar",
        "Show project timing",
        "The bar at the top of the window shows the open project's tempo and time signature",
        &["bar_project_timing"],
        |cx, r| cx.on_off(r, |s| &mut s.view.bar_project_timing),
    ),
    row(
        Page::Appearance,
        "Application bar",
        "Project information position",
        "Where the project's name and timing sit in the bar at the top of the window",
        &["bar_project_centered"],
        |cx, r| cx.choose(r, &[("Left", false), ("Centre", true)], |s| &mut s.view.bar_project_centered),
    ),
    row(
        Page::Appearance,
        "Advanced",
        "Graphics renderer",
        "Whether the window is drawn with GPU or OpenGL, used from the next start",
        &["renderer"],
        |cx, r| {
            if cx.about.graphics_now.is_some_and(|now| now != cx.settings.graphics) {
                cx.note("Applies the next time Suhina starts");
            }
            cx.choose(r, &[("GPU", Graphics::Gpu), ("OpenGL", Graphics::OpenGl)], |s| &mut s.graphics);
        },
    ),
    row(
        Page::Appearance,
        "Advanced",
        "Panel liquid",
        "How see-through the Settings window's backdrop is, with the GPU renderer and animations on",
        &["liquid_panel_strength"],
        |cx, r| cx.liquid_strength(r, |s| &mut s.view.liquid.panels),
    ),
    row(
        Page::Appearance,
        "Advanced",
        "Track liquid",
        "How much light track headers reflect, with the GPU renderer and animations on",
        &["liquid_track_strength"],
        |cx, r| cx.liquid_strength(r, |s| &mut s.view.liquid.tracks),
    ),
    row(
        Page::Appearance,
        "Advanced",
        "Clip liquid",
        "How much light clip backgrounds reflect, with the GPU renderer and animations on",
        &["liquid_clip_strength"],
        |cx, r| cx.liquid_strength(r, |s| &mut s.view.liquid.clips),
    ),
    block(
        Page::Theme,
        "Theme",
        "Theme",
        "The themes to pick from, their variants, the theme in use and its settings",
        &["theme"],
        crate::settings::settings_view::theme_block,
    ),
    row(
        Page::Theme,
        "Folder",
        "Themes folder",
        "The folder themes are kept in and read from",
        &["themes_folder"],
        |cx, r| {
            let (default, value) = (cx.about.themes_default.clone(), cx.settings.themes_folder.clone());
            cx.moved_folder(r, &default, crate::interaction::pick::PickField::ThemesFolder, &value);
        },
    ),
    row(
        Page::Agents,
        "Agents",
        "AI agents edit projects",
        "An AI agent you start can change your open projects, and you can undo each change",
        &["agents"],
        |cx, r| cx.on_off(r, |s| &mut s.agents),
    ),
    row(
        Page::Agents,
        "MCP write permissions",
        "Project folder for agents",
        "An AI agent can save files in the open project's folder and the folders inside it",
        &["mcp_project_folder"],
        |cx, r| {
            let note = cx.about.mcp_project_folder.as_deref().unwrap_or("No saved project folder").to_string();
            cx.note(format!("{note} (including subfolders)"));
            cx.on_off(r, |s| &mut s.mcp_project_folder);
        },
    ),
    block(
        Page::Agents,
        "MCP write permissions",
        "More agent write folders",
        "More folders an AI agent can save files in, with the folders inside them",
        &["mcp_write_folders"],
        |cx, y| cx.mcp_write_folders(y),
    ),
    row(
        Page::Agents,
        "Updates",
        "Check for updates",
        "Each time Suhina starts it asks whether a newer version is out, sending nothing about you",
        &["check_updates"],
        |cx, r| cx.on_off(r, |s| &mut s.updates.check),
    ),
    row(Page::Agents, "Updates", "Update channel", "Which builds Suhina updates to", &["update_channel"], |cx, r| {
        if cx.settings.updates.test_builds_listed() {
            let options = [("Nightly", Channel::Nightly), ("Test builds", Channel::TestBuilds)];
            cx.choose(r, &options, |s| &mut s.updates.channel);
        } else {
            cx.choose(r, &[("Nightly", Channel::Nightly)], |s| &mut s.updates.channel);
        }
    }),
    row(
        Page::Agents,
        "Advanced",
        "Developer mode",
        "Test builds of changes still under review can be chosen as updates",
        &["developer_mode"],
        |cx, r| cx.on_off(r, |s| &mut s.updates.developer),
    ),
];

#[cfg(test)]
#[path = "settings_rows_tests.rs"]
mod tests;
