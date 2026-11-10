// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Every setting `settings.txt` keeps, described once (#764 › 12): its key, how its value is
//! written and how it is read back. [`Settings::to_text`] writes them in this order and
//! [`Settings::from_text`] finds a line's key here; the default is the struct's. The view's and
//! the updates' own settings keep their tables beside them, and the lists (MCP write folders,
//! audio devices, MIDI inputs off) are read and written apart, after these.

use super::*;

/// One setting in `settings.txt`.
pub(super) struct Field {
    pub(super) key: &'static str,
    /// Its value as the file keeps it.
    pub(super) write: fn(&Settings) -> String,
    /// Takes `value` from the file; `false` for a value it does not know, which leaves the default.
    pub(super) read: fn(&mut Settings, &str) -> bool,
}

fn on(b: bool) -> String {
    (if b { "on" } else { "off" }).to_string()
}

fn on_off(value: &str) -> Option<bool> {
    match value {
        "on" => Some(true),
        "off" => Some(false),
        _ => None,
    }
}

/// A switch kept as `on` or `off`.
macro_rules! switch {
    ($key:literal, $field:ident) => {
        Field { key: $key, write: |s| on(s.$field), read: |s, v| on_off(v).map(|b| s.$field = b).is_some() }
    };
}

/// A switch kept as one of two words: `$off` for `false`, `$on` for `true`.
macro_rules! either {
    ($key:literal, $field:ident, $off:literal, $on:literal) => {
        Field {
            key: $key,
            write: |s| (if s.$field { $on } else { $off }).to_string(),
            read: |s, v| match v {
                $off => {
                    s.$field = false;
                    true
                }
                $on => {
                    s.$field = true;
                    true
                }
                _ => false,
            },
        }
    };
}

/// Text kept as it is.
macro_rules! text {
    ($key:literal, $field:ident) => {
        Field {
            key: $key,
            write: |s| s.$field.to_string(),
            read: |s, v| {
                s.$field = v.to_string();
                true
            },
        }
    };
}

/// One of a type's values, each kept as a word.
macro_rules! choice {
    ($key:literal, $field:ident, { $($value:path => $word:literal),+ $(,)? }) => {
        Field {
            key: $key,
            write: |s| match s.$field { $($value => $word,)+ }.to_string(),
            read: |s, v| match v {
                $($word => {
                    s.$field = $value;
                    true
                })+
                _ => false,
            },
        }
    };
}

/// A whole number, kept in `range` (clamped into it, or refused outside it with `refuse`).
macro_rules! number {
    ($key:literal, $field:ident, $ty:ty, clamp $range:expr) => {
        Field {
            key: $key,
            write: |s| s.$field.to_string(),
            read: |s, v| v.parse::<$ty>().map(|n| s.$field = n.clamp($range.0, $range.1)).is_ok(),
        }
    };
    ($key:literal, $field:ident, $ty:ty, only $ok:expr) => {
        Field {
            key: $key,
            write: |s| s.$field.to_string(),
            read: |s, v| v.parse::<$ty>().ok().filter($ok).map(|n| s.$field = n).is_some(),
        }
    };
}

/// A value of a type with its own `param` and `parse`.
macro_rules! named {
    ($key:literal, $field:ident, $ty:ty) => {
        Field {
            key: $key,
            write: |s| s.$field.param().to_string(),
            read: |s, v| <$ty>::parse(v).map(|x| s.$field = x).is_some(),
        }
    };
}

/// The settings `Settings::to_text` writes first, in this order.
pub(super) const FIELDS: &[Field] = &[
    switch!("animations", animations),
    switch!("drag_hides_pointer", drag_hides_pointer),
    Field {
        key: "track_headers",
        write: |s| {
            match s.track_headers {
                None => "theme",
                Some(Side::Left) => "left",
                Some(Side::Right) => "right",
            }
            .to_string()
        },
        read: |s, v| match v {
            "theme" => {
                s.track_headers = None;
                true
            }
            "left" => {
                s.track_headers = Some(Side::Left);
                true
            }
            "right" => {
                s.track_headers = Some(Side::Right);
                true
            }
            _ => false,
        },
    },
    choice!("renderer", graphics, { Graphics::Gpu => "wgpu", Graphics::OpenGl => "glow" }),
    switch!("sample_stacking", sample_stacking),
    switch!("midi_stacking", midi_stacking),
    choice!("open_projects", on_open, { OnOpen::Ask => "ask", OnOpen::NewTab => "tab", OnOpen::Replace => "replace" }),
    Field {
        key: "at_start",
        write: |s| {
            match s.at_start {
                AtStart::Panel => "panel",
                AtStart::Blank => "blank",
                AtStart::Template => "template",
                AtStart::Last => "last",
            }
            .to_string()
        },
        read: |s, v| {
            s.at_start = match v {
                // `dialog` is what test builds wrote before the start panel took the dialog's place.
                "panel" | "dialog" => AtStart::Panel,
                "blank" => AtStart::Blank,
                "template" => AtStart::Template,
                "last" => AtStart::Last,
                _ => return false,
            };
            true
        },
    },
    text!("start_template", start_template),
    choice!("stop", stop, { StopReturn::Marker => "marker", StopReturn::Here => "here" }),
    choice!("play_start", play_from, { PlayFrom::Marker => "marker", PlayFrom::Clip => "clip" }),
    choice!("device_chain", device_layout, { DeviceLayout::Row => "row", DeviceLayout::Stack => "stack" }),
    choice!("mixer_controls", mixer_controls, { MixerControls::Field => "field", MixerControls::Knob => "knob" }),
    switch!("select_on_launch", select_on_launch),
    choice!("plugin_editors", plugin_editors, {
        EditorPlacement::Window => "window",
        EditorPlacement::DeviceView => "device",
    }),
    Field {
        key: "theme",
        write: |s| s.theme.clone(),
        read: |s, v| {
            // A built-in theme saved under an id it has since lost is its successor.
            if let Some(now) = renamed_theme(v).filter(|now| is_theme_id(now)) {
                s.theme = now;
                return true;
            }
            let ok = is_theme_id(v);
            if ok {
                s.theme = v.to_string();
            }
            ok
        },
    },
    text!("projects_folder", projects_folder),
    text!("templates_folder", templates_folder),
    text!("nam_models_folder", nam_models_folder),
    text!("irs_folder", irs_folder),
    text!("themes_folder", themes_folder),
    switch!("browser", browser_open),
    choice!("browser_side", browser_side, { Side::Left => "left", Side::Right => "right" }),
    number!("browser_width", browser_width, u32, clamp BROWSER_WIDTHS),
    switch!("preview_autoplay", preview_autoplay),
    number!("preview_volume", preview_volume, i32, clamp PREVIEW_VOLUMES),
    either!("drop_files", drop_separate, "one_track", "separate"),
    named!("warp_mode", warp_mode, WarpMode),
    either!("map_layout", map_grid, "cloud", "grid"),
    switch!("map_hover_preview", map_hover_preview),
    number!("map_max_seconds", map_max_seconds, u32, clamp MAP_MAX_SECONDS_RANGE),
    number!("map_dot_size", map_dot_size, u32, clamp MAP_DOT_SIZES),
    number!("map_label_size", map_label_size, u32, only |s: &u32| MAP_LABEL_SIZES.contains(s)),
    switch!("map_window", map_window),
    Field {
        key: "map_window_rect",
        write: |s| s.map_window_rect.map_or_else(|| "none".to_string(), |[x, y, w, h]| format!("{x}, {y}, {w}, {h}")),
        read: |s, v| {
            if v == "none" {
                s.map_window_rect = None;
                return true;
            }
            parse_window_rect(v).map(|rect| s.map_window_rect = Some(rect)).is_some()
        },
    },
    text!("audio_host", audio_host),
    text!("audio_device", audio_device),
    number!("audio_sample_rate", audio_sample_rate, u32, only |r: &u32| *r <= MAX_SAMPLE_RATE),
    number!("audio_buffer", audio_buffer, u32, only |f: &u32| *f == 0
        || (MIN_BUFFER_FRAMES..=MAX_BUFFER_FRAMES).contains(f)),
    switch!("audio_exclusive", audio_exclusive),
    number!("audio_cue_pair", audio_cue_pair, u16, only |p: &u16| *p <= MAX_CUE_PAIR),
    number!("audio_cue_volume", audio_cue_volume, i32, clamp CUE_VOLUMES),
    Field {
        key: "audio_input",
        write: |s| s.audio_input.clone(),
        read: |s, v| {
            s.audio_input = suhina_session::InputChoice::from_text(v).as_text().to_string();
            true
        },
    },
    number!("zoom", zoom, u32, clamp ZOOMS),
    either!("waveform_scale", waveform_db, "linear", "db"),
    choice!("piano_roll_top", piano_roll_top, { PianoRollTop::Track => "track", PianoRollTop::Window => "window" }),
    named!("piano_roll_toolbar", toolbar_size, ToolbarSize),
    Field {
        key: "backup",
        write: |s| s.backup.name().to_string(),
        read: |s, v| suhina_session::backup::Interval::parse(v).map(|i| s.backup = i).is_some(),
    },
    switch!("backup_clean_up", backup_clean_up),
];

/// The settings `Settings::to_text` writes after the view's.
pub(super) const LATER: &[Field] = &[
    named!("piano_key_style", piano_key_style, PianoKeyStyle),
    named!("piano_key_names", piano_key_names, PianoKeyNames),
    Field {
        key: "background_image",
        write: |s| s.background_image.replace(['\r', '\n'], ""),
        read: |s, v| {
            s.background_image = v.to_string();
            true
        },
    },
    switch!("agents", agents),
    switch!("mcp_project_folder", mcp_project_folder),
];

/// The settings written after the MCP write folders.
pub(super) const LAST: &[Field] =
    &[switch!("auto_arm", auto_arm), named!("setting_descriptions", setting_descriptions, Descriptions)];

/// The setting `key` names, among all of them.
pub(super) fn field(key: &str) -> Option<&'static Field> {
    FIELDS.iter().chain(LATER).chain(LAST).find(|f| f.key == key)
}
