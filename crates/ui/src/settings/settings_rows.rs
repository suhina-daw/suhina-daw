// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Every setting the Settings window shows, in one table: its page, its section, its name and
//! note, the keys it is saved under and how its control is drawn (`docs/spec/ui-settings.md`).
//! A page shows its entries in this order, a new section heading where the section changes.
//!
//! Adding a setting is one entry under its section, most often a `row` whose control is one call:
//! `cx.on_off(r, |s| &mut s.field)` or `cx.choose(r, &[("Text", value), …], |s| &mut s.field)`.

use crate::settings::settings_view::{Entry, Page, block, row};
use crate::settings::track_sizes::TrackSize;
use crate::settings::view_settings::{DetailLayout, WaveformColour};
use crate::settings::{
    AtStart, DeviceLayout, EditorPlacement, Graphics, MixerControls, OnOpen, PianoRollTop, PlayFrom, Side, StopReturn,
    ToolbarSize, WarpMode,
};
use suhina_update::Channel;

/// The keys of `keymap.json`, where the keys and the mouse switches are saved.
pub(crate) const KEYMAP: &[&str] = &["keymap.json"];

/// The note under Editors on macOS: the editors open in a window of their own, and what is
/// missing is the device view's embed, which a host on this system cannot give (ADR 0006).
const EDITORS_NOTE_EMBED: &str = "Editors open in a window of their own; the device view needs Windows or X11";
/// The note under Editors where no editor opens at all: a system with no display (Linux without
/// an X server, Wayland without editors yet).
const EDITORS_NOTE_NONE: &str = "Editors need Windows or an X server (DISPLAY); they do not open here yet";

/// The note under Editors where the device view's embedded editor cannot open. On macOS the
/// editors still open, in a window of their own: a view of the plug-in host's process cannot
/// attach to another process's window (ADR 0006), so the embed is what is missing, not the
/// editors. On a system with no display at all (Linux without an X server), neither opens.
/// Both notes are one line of the row's note width: a longer one is cut mid-word.
pub(crate) fn editors_note(editors_embeddable: bool) -> Option<&'static str> {
    if editors_embeddable {
        return None;
    }
    Some(if cfg!(target_os = "macos") { EDITORS_NOTE_EMBED } else { EDITORS_NOTE_NONE })
}

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
    // Settings folder is named beside the default folder, not in the settings file: no saved key.
    row(
        Page::General,
        "Projects",
        "Settings folder",
        "Where Suhina keeps its settings, keys, recent projects and themes; used from the next start",
        &[],
        |cx, r| {
            let (default, value) = (cx.about.data_default.clone(), cx.settings.data_folder.clone());
            cx.moved_folder(r, &default, crate::interaction::pick::PickField::DataFolder, &value);
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
    // Export.
    row(
        Page::Export,
        "MCP write permissions",
        "Allow the project's folder",
        "Includes subfolders; turn off to revoke this default permission",
        &["mcp_project_folder"],
        |cx, r| {
            let note = cx.about.mcp_project_folder.as_deref().unwrap_or("No saved project folder").to_string();
            cx.note(format!("{note} (including subfolders)"));
            cx.on_off(r, |s| &mut s.mcp_project_folder);
        },
    ),
    block(
        Page::Export,
        "MCP write permissions",
        "MCP allowed write folders",
        "Add or remove folders; includes subfolders. Applies to MCP writes, not File > Export.",
        &["mcp_write_folders"],
        |cx, y| cx.mcp_write_folders(y),
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
    row(
        Page::Midi,
        "Arming",
        "Auto-arm the selected track",
        "Selecting a track arms it, so the MIDI controller plays that track's instrument; a track armed by hand stays armed",
        &["auto_arm"],
        |cx, r| cx.on_off(r, |s| &mut s.auto_arm),
    ),
    // Plug-ins.
    row(
        Page::Plugins,
        "Editors",
        "Editors",
        "In a window above Suhina, or inside the device panel as part of the chain",
        &["plugin_editors"],
        |cx, r| {
            if let Some(note) = editors_note(cx.about.editors_embeddable) {
                cx.note(note);
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
        "Waveform and note colour",
        "Theme uses the theme's colour (dark when it gives none); Auto follows each clip's brightness; Light, Dark and Custom use one colour for waveforms, MIDI notes and ARA notes",
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
        "Clips",
        "Custom waveform and note colour",
        "Choose the waveform and note colour; applies immediately",
        &["waveform_custom_colour"],
        |cx, r| cx.waveform_colour(r),
    )
    .when(|s| s.view.waveform.mode == WaveformColour::Custom),
    row(
        Page::Arrangement,
        "Clips",
        "Audio clip colour strength",
        "How strongly audio clips show their original colour; applies immediately",
        &["audio_clip_colour_strength"],
        |cx, r| cx.colour_strength(r, |s| &mut s.view.audio_colour_strength),
    ),
    row(
        Page::Arrangement,
        "Clips",
        "Ctrl+Alt-drag on an edge stretches audio",
        "An audio clip's edge dragged with Ctrl and Alt held time-stretches its audio to the new length, its pitch kept; off, it moves the edge as a plain drag does",
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
        "By clip type: MIDI and audio tracks use their theme defaults. One colour: all new tracks use the MIDI default. Existing custom colours stay unchanged",
        &["new_track_colour"],
        |cx, r| cx.choose(r, &[("By clip type", false), ("One colour", true)], |s| &mut s.view.fixed_track_colour),
    ),
    row(
        Page::Arrangement,
        "Tracks",
        "New clip colour",
        "Track colour: a new clip takes the colour its track has now. Theme default: MIDI and audio clips use the theme's clip defaults. Existing clips keep their colours",
        &["new_clip_colour"],
        |cx, r| {
            cx.choose(r, &[("Track colour", true), ("Theme default", false)], |s| &mut s.view.new_clip_track_colour)
        },
    ),
    row(
        Page::Arrangement,
        "Tracks",
        "Track header colour strength",
        "How strongly track headers show their original colour; applies immediately",
        &["track_header_colour_strength"],
        |cx, r| cx.colour_strength(r, |s| &mut s.view.header_colour_strength),
    ),
    row(
        Page::Arrangement,
        "Tracks",
        "Track headers",
        "Which side of the timeline they are on",
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
        "Whether the Arrangement shows the return tracks' rows along the bottom; its Hide returns button by the ruler switches it too",
        &["arrangement_returns"],
        |cx, r| cx.choose(r, &[("Shown", false), ("Hidden", true)], |s| &mut s.view.returns_hidden),
    ),
    row(
        Page::Arrangement,
        "Track sizes",
        "Default track size",
        "Named height for ordinary Arrangement tracks",
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
        "Return tracks have their own default size",
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
        "Advanced track sizes",
        "Show exact heights in pixels; all sizes stay ordered",
        &["arrangement_sizes_advanced"],
        |cx, r| cx.on_off(r, |s| &mut s.view.track_sizes.advanced),
    ),
    row(
        Page::Arrangement,
        "Track sizes",
        "Tiny height",
        "Adjust the smallest row; always below Compact",
        &["arrangement_size_heights"],
        |cx, r| cx.track_size_height(r, TrackSize::Tiny),
    ),
    row(Page::Arrangement, "Track sizes", "Compact height", "Adjust Compact between Tiny and Small", &[], |cx, r| {
        cx.track_size_height(r, TrackSize::Compact)
    }),
    row(Page::Arrangement, "Track sizes", "Small height", "Adjust Small between Compact and Medium", &[], |cx, r| {
        cx.track_size_height(r, TrackSize::Small)
    }),
    row(Page::Arrangement, "Track sizes", "Medium height", "Adjust Medium between Small and Large", &[], |cx, r| {
        cx.track_size_height(r, TrackSize::Medium)
    }),
    row(
        Page::Arrangement,
        "Track sizes",
        "Large height",
        "Adjust the largest named row; always above Medium",
        &[],
        |cx, r| cx.track_size_height(r, TrackSize::Large),
    ),
    // Session view and mixer.
    row(
        Page::Arrangement,
        "Pasting",
        "Replace clip",
        "Paste onto a selected clip at its start and remove that destination clip; off pastes after it",
        &["replace_clip"],
        |cx, r| cx.on_off(r, |s| &mut s.view.replace_clip),
    ),
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
        "Background",
        "Background image",
        "Choose your own PNG; desktop icons and other windows are never captured. Empty removes the image",
        &["background_image"],
        |cx, r| {
            cx.folder(r, "No image", crate::interaction::pick::PickField::BackgroundImage, |s| &mut s.background_image)
        },
    ),
    row(
        Page::Appearance,
        "Background",
        "Surface opacity",
        "Tint over the background image; 100% makes neutral panels solid",
        &["background_tint"],
        |cx, r| cx.colour_strength(r, |s| &mut s.view.background_tint),
    ),
    row(
        Page::Appearance,
        "Background",
        "Background blur",
        "Blur the chosen image; no live desktop capture",
        &["background_blur"],
        |cx, r| cx.background_blur(r),
    ),
    row(
        Page::Appearance,
        "Application bar",
        "Application name position",
        "Suhina is always shown",
        &["app_name_centered"],
        |cx, r| cx.choose(r, &[("Left", false), ("Centre", true)], |s| &mut s.view.app_name_centered),
    ),
    row(
        Page::Appearance,
        "Application bar",
        "Show project name",
        "Show the active project and its unsaved marker",
        &["bar_project_name"],
        |cx, r| cx.on_off(r, |s| &mut s.view.bar_project_name),
    ),
    row(
        Page::Appearance,
        "Application bar",
        "Show project timing",
        "Show the active project's tempo and time signature",
        &["bar_project_timing"],
        |cx, r| cx.on_off(r, |s| &mut s.view.bar_project_timing),
    ),
    row(
        Page::Appearance,
        "Application bar",
        "Project information position",
        "Fields sharing the same position appear together",
        &["bar_project_centered"],
        |cx, r| cx.choose(r, &[("Left", false), ("Centre", true)], |s| &mut s.view.bar_project_centered),
    ),
    row(
        Page::Appearance,
        "Liquid (experimental)",
        "Panel liquid",
        "Settings backdrop transparency; 0% is solid. Requires GPU renderer and Animations on",
        &["liquid_panel_strength"],
        |cx, r| cx.liquid_strength(r, |s| &mut s.view.liquid.panels),
    ),
    row(
        Page::Appearance,
        "Liquid (experimental)",
        "Track liquid",
        "Reflection on MIDI and audio track headers; 0% is flat. Requires GPU renderer and Animations on",
        &["liquid_track_strength"],
        |cx, r| cx.liquid_strength(r, |s| &mut s.view.liquid.tracks),
    ),
    row(
        Page::Appearance,
        "Liquid (experimental)",
        "Clip liquid",
        "Reflection on clip backgrounds; notes and waveforms stay sharp. 0% is flat; requires GPU and Animations on",
        &["liquid_clip_strength"],
        |cx, r| cx.liquid_strength(r, |s| &mut s.view.liquid.clips),
    ),
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
    row(
        Page::Appearance,
        "Menu bar",
        "Menu bar in the window",
        "Show the menus at the top of the window; on macOS the bar at the top of the screen has them too",
        &["menu_bar"],
        |cx, r| cx.on_off(r, |s| &mut s.view.menu_bar),
    ),
    // Theme.
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
        "Where themes are kept and read from; empty for the folder in Suhina's data",
        &["themes_folder"],
        |cx, r| {
            let (default, value) = (cx.about.themes_default.clone(), cx.settings.themes_folder.clone());
            cx.moved_folder(r, &default, crate::interaction::pick::PickField::ThemesFolder, &value);
        },
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

    /// The note under Editors tells the truth about this system: where the editors still open
    /// (macOS, #999), it does not say they do not; where they cannot (no display), it does.
    #[test]
    fn the_editors_note_names_what_is_missing_not_the_editors() {
        assert!(editors_note(true).is_none(), "an embeddable system needs no note");
        let note = editors_note(false).unwrap_or_default();
        if cfg!(target_os = "macos") {
            assert!(note.contains("window of their own"), "{note}");
            assert!(note.contains("the device view needs Windows or X11"), "{note}");
            assert!(!note.contains("do not open"), "{note}");
        } else {
            assert!(note.contains("DISPLAY"), "{note}");
            assert!(note.contains("do not open"), "{note}");
        }
    }

    /// Both notes under Editors fit the row's one line, measured in the window's own fonts:
    /// a longer note is cut mid-word and read by nobody (the macOS note was, #999).
    #[test]
    fn the_editors_notes_fit_the_one_line_the_row_gives_them() {
        let ctx = egui::Context::default();
        crate::kit::theme::install(&ctx);
        let font = crate::kit::theme::ui_font(crate::kit::theme::TEXT_SMALL);
        let budget = crate::settings::settings_view::NOTE_WIDTH;
        let mut over = Vec::new();
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            for note in [EDITORS_NOTE_EMBED, EDITORS_NOTE_NONE] {
                let width =
                    ui.fonts_mut(|f| f.layout_no_wrap(note.to_string(), font.clone(), egui::Color32::WHITE).size().x);
                if width > budget {
                    over.push(format!("{note} — {width:.0} px of {budget:.0}"));
                }
            }
        });
        // No renderer here to apply the font texture the pass made.
        out.textures_delta.clear();
        assert!(over.is_empty(), "a note longer than the row's one line: {}", over.join("; "));
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
