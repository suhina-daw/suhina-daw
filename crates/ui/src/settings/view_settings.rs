// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Settings of the window's layout and editing that live in their own lines of the settings
//! file: the Session mixer's height, where the detail area goes, and whether a double-click on an
//! empty slot or lane makes a clip (`docs/design/components.md` › Settings).

/// The Session mixer's height in points: the least, the default and the most a file may say
/// (the view clamps it further to the space it has).
pub const MIXER_HEIGHTS: (u32, u32, u32) = (46, 166, 2000);

/// Where the detail area goes: beside the browser, or under it across the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DetailLayout {
    /// The detail area starts right of the browser, which runs down to the status bar.
    #[default]
    Beside,
    /// The detail area spans the whole window width; the browser ends above it.
    FullWidth,
}

impl DetailLayout {
    fn param(self) -> &'static str {
        match self {
            Self::Beside => "beside",
            Self::FullWidth => "full_width",
        }
    }
}

/// The settings this module reads and writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewSettings {
    /// The Session mixer's height, in points.
    pub mixer_height: u32,
    /// The strips' fader height the divider under their device lists sets, in points; 0 is
    /// automatic.
    pub strip_fader: u32,
    pub detail_layout: DetailLayout,
    /// A double-click on an empty slot or an empty spot of a lane makes an empty clip of the track's kind.
    pub double_click_clip: bool,
    /// Clip menus carry a name field that renames in place, instead of Rename (off by default).
    pub menu_name_field: bool,
    /// Notes moved onto others of the same key and start replace them (on by default).
    pub notes_replace: bool,
    /// Ctrl+drag over a clip selects time; off (the default), a drag from a clip's upper half
    /// does.
    pub time_select_ctrl: bool,
    /// Mirror: a clip's lower half moves it and its upper half selects time; off (the
    /// default), its name bar moves it and its body selects time.
    pub clip_halves: bool,
    /// Alt held while an audio clip's edge is dragged stretches its audio (on by default).
    pub stretch_alt: bool,
    /// Ctrl+R with several clips selected renames them all at once (on by default, #693).
    pub rename_selected: bool,
    /// Clips picked while stopped move the play start marker to the first one's start (on by
    /// default, #721).
    pub play_from_clip: bool,
    /// New tracks all get the palette's first colour, instead of the next one in turn (off by
    /// default, #709).
    pub fixed_track_colour: bool,
    /// The browser shows Recent's kinds as chips over its list rather than as items indented
    /// under it in the sidebar (#711).
    pub recent_chips: bool,
    /// The Arrangement view leaves out the return tracks' rows (its "Hide returns" button).
    pub returns_hidden: bool,
}

impl Default for ViewSettings {
    fn default() -> Self {
        Self {
            mixer_height: MIXER_HEIGHTS.1,
            strip_fader: 0,
            detail_layout: DetailLayout::Beside,
            double_click_clip: true,
            menu_name_field: false,
            notes_replace: true,
            time_select_ctrl: false,
            clip_halves: false,
            stretch_alt: true,
            rename_selected: true,
            play_from_clip: true,
            fixed_track_colour: false,
            recent_chips: false,
            returns_hidden: false,
        }
    }
}

impl ViewSettings {
    /// The lines this module adds to the settings file.
    pub fn to_text(&self) -> String {
        format!(
            "session_mixer_height = {}\nsession_strip_fader = {}\ndetail_layout = {}\ndouble_click_clip = {}\nmenu_name_field = {}\nnotes_replace = {}\ntime_select = {}\nstretch_alt_drag = {}\nrename_selected_clips = {}\nplay_from_selected_clip = {}\nnew_track_colour = {}\nbrowser_recent = {}\narrangement_returns = {}\nclip_drag = {}\n",
            self.mixer_height,
            self.strip_fader,
            self.detail_layout.param(),
            if self.double_click_clip { "on" } else { "off" },
            if self.menu_name_field { "on" } else { "off" },
            if self.notes_replace { "on" } else { "off" },
            if self.time_select_ctrl { "ctrl_drag" } else { "clip_lower_half" },
            if self.stretch_alt { "on" } else { "off" },
            if self.rename_selected { "on" } else { "off" },
            if self.play_from_clip { "on" } else { "off" },
            if self.fixed_track_colour { "fixed" } else { "alternating" },
            if self.recent_chips { "chips" } else { "indented" },
            if self.returns_hidden { "hidden" } else { "shown" },
            if self.clip_halves { "halves" } else { "name_bar" },
        )
    }

    /// Reads one line of the settings file; `false` when the key and value are not one of these
    /// settings, so the caller goes on with its own.
    pub fn parse(&mut self, key: &str, value: &str) -> bool {
        match (key, value) {
            ("session_mixer_height", height) => match height.parse::<u32>() {
                Ok(height) => self.mixer_height = height.clamp(MIXER_HEIGHTS.0, MIXER_HEIGHTS.2),
                Err(_) => return false,
            },
            ("session_strip_fader", fader) => match fader.parse::<u32>() {
                Ok(fader) => self.strip_fader = fader.min(MIXER_HEIGHTS.2),
                Err(_) => return false,
            },
            ("detail_layout", "beside") => self.detail_layout = DetailLayout::Beside,
            ("detail_layout", "full_width") => self.detail_layout = DetailLayout::FullWidth,
            ("double_click_clip", "on" | "off") => self.double_click_clip = value == "on",
            ("menu_name_field", "on" | "off") => self.menu_name_field = value == "on",
            ("notes_replace", "on" | "off") => self.notes_replace = value == "on",
            ("time_select", "ctrl_drag" | "clip_lower_half") => self.time_select_ctrl = value == "ctrl_drag",
            ("stretch_alt_drag", "on" | "off") => self.stretch_alt = value == "on",
            ("rename_selected_clips", "on" | "off") => self.rename_selected = value == "on",
            ("play_from_selected_clip", "on" | "off") => self.play_from_clip = value == "on",
            ("new_track_colour", "fixed" | "alternating") => self.fixed_track_colour = value == "fixed",
            ("browser_recent", "chips" | "indented") => self.recent_chips = value == "chips",
            ("arrangement_returns", "hidden" | "shown") => self.returns_hidden = value == "hidden",
            ("clip_drag", "halves" | "name_bar") => self.clip_halves = value == "halves",
            _ => return false,
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;

    #[test]
    fn view_settings_survive_a_round_trip_through_the_settings_file() {
        let view = ViewSettings {
            mixer_height: 300,
            strip_fader: 90,
            detail_layout: DetailLayout::FullWidth,
            double_click_clip: false,
            menu_name_field: true,
            notes_replace: false,
            time_select_ctrl: true,
            clip_halves: true,
            stretch_alt: false,
            rename_selected: false,
            play_from_clip: false,
            fixed_track_colour: true,
            recent_chips: true,
            returns_hidden: true,
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
}
