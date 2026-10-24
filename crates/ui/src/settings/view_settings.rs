// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Settings of the window's layout and editing that live in their own lines of the settings
//! file: the Session mixer's height, where the detail area goes, and whether a double-click on an
//! empty slot or lane makes a clip (`docs/design/components.md` › Settings).

/// The Session mixer's height in points: the least, the default and the most a file may say
/// (the view clamps it further to the space it has).
pub const MIXER_HEIGHTS: (u32, u32, u32) = (46, 166, 2000);

/// How Arrangement waveforms and note previews choose their ink, independently of clip names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WaveformColour {
    /// The theme's colour (`.clip-waveform`, `.clip-notes`), or Dark where it gives none.
    #[default]
    Theme,
    Auto,
    Light,
    Dark,
    Custom,
}

impl WaveformColour {
    fn name(self) -> &'static str {
        match self {
            Self::Theme => "theme",
            Self::Auto => "auto",
            Self::Light => "light",
            Self::Dark => "dark",
            Self::Custom => "custom",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WaveformAppearance {
    pub mode: WaveformColour,
    pub custom: egui::Color32,
}

impl Default for WaveformAppearance {
    fn default() -> Self {
        Self { mode: WaveformColour::Theme, custom: egui::Color32::from_rgb(232, 228, 221) }
    }
}

impl WaveformAppearance {
    pub fn ink(self, automatic: egui::Color32) -> egui::Color32 {
        match self.mode {
            WaveformColour::Auto => automatic,
            WaveformColour::Light => egui::Color32::from_rgb(240, 237, 232),
            WaveformColour::Dark | WaveformColour::Theme => egui::Color32::from_rgb(32, 31, 29),
            WaveformColour::Custom => self.custom,
        }
    }
}

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
    /// Experimental materials, independent of theme colours. Each 0..=100 strength defaults off.
    pub liquid: crate::kit::liquid::Options,
    pub track_sizes: super::track_sizes::TrackSizes,
    pub waveform: WaveformAppearance,
    /// Audio clip colour strength, as a percentage of its source RGB colour.
    pub audio_colour_strength: u8,
    /// Track header colour strength, as a percentage of its source RGB colour.
    pub header_colour_strength: u8,
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
    /// Pasting onto a selected arrangement clip replaces that clip at its start.
    pub replace_clip: bool,
    /// The window shows its own menu bar. The macOS app turns it off when the settings file does not
    /// say (`main.rs`), since the bar at the top of the screen has the same menus there.
    pub menu_bar: bool,
    /// Centre the always-visible application name in the application bar.
    pub app_name_centered: bool,
    /// Show the active project's name in the application bar.
    pub bar_project_name: bool,
    /// Show project tempo and meter in the application bar.
    pub bar_project_timing: bool,
    /// Centre project information; otherwise place it at the left.
    pub bar_project_centered: bool,
    /// Opacity of neutral surfaces above the private background image, 0..=100.
    pub background_tint: u8,
    /// Blur radius for the background image, in logical display pixels.
    pub background_blur: u8,
    /// Ctrl+drag over a clip selects time; off (the default), a drag from a clip's upper half
    /// does.
    pub time_select_ctrl: bool,
    /// Mirror: a clip's lower half moves it and its upper half selects time; off (the
    /// default), its name bar moves it and its body selects time.
    pub clip_halves: bool,
    /// The grid shows through the clips in the Arrangement view (on by default).
    pub grid_in_clips: bool,
    /// The Arrangement's waveform zoom in percent, 100 … 1600 (×1 … ×16): audio clips drawn
    /// larger, never louder. Saved as the factor.
    pub waveform_zoom: u32,
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
    /// A new clip takes its track's colour, instead of the theme's MIDI or audio clip default
    /// (on by default).
    pub new_clip_track_colour: bool,
    /// The browser shows Recent's kinds as chips over its list rather than as items indented
    /// under it in the sidebar (#711).
    pub recent_chips: bool,
    /// The Arrangement view leaves out the return tracks' rows (its "Hide returns" button).
    pub returns_hidden: bool,
}

impl Default for ViewSettings {
    fn default() -> Self {
        Self {
            liquid: crate::kit::liquid::Options::default(),
            track_sizes: super::track_sizes::TrackSizes::default(),
            waveform: WaveformAppearance::default(),
            audio_colour_strength: 82,
            header_colour_strength: 82,
            mixer_height: MIXER_HEIGHTS.1,
            strip_fader: 0,
            detail_layout: DetailLayout::Beside,
            double_click_clip: true,
            menu_name_field: false,
            notes_replace: true,
            replace_clip: false,
            menu_bar: true,
            app_name_centered: false,
            bar_project_name: true,
            bar_project_timing: false,
            bar_project_centered: true,
            background_tint: 70,
            background_blur: 12,
            time_select_ctrl: false,
            clip_halves: false,
            grid_in_clips: true,
            waveform_zoom: 100,
            stretch_alt: true,
            rename_selected: true,
            play_from_clip: true,
            fixed_track_colour: false,
            new_clip_track_colour: true,
            recent_chips: false,
            returns_hidden: false,
        }
    }
}

impl ViewSettings {
    /// The lines this module adds to the settings file.
    pub fn to_text(&self) -> String {
        format!(
            "session_mixer_height = {}\nsession_strip_fader = {}\ndetail_layout = {}\ndouble_click_clip = {}\nmenu_name_field = {}\nnotes_replace = {}\ntime_select = {}\nstretch_alt_drag = {}\nrename_selected_clips = {}\nplay_from_selected_clip = {}\nnew_track_colour = {}\nnew_clip_colour = {}\nbrowser_recent = {}\narrangement_returns = {}\nclip_drag = {}\nclip_grid = {}\nwaveform_zoom = {}\n",
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
            if self.new_clip_track_colour { "track" } else { "theme" },
            if self.recent_chips { "chips" } else { "indented" },
            if self.returns_hidden { "hidden" } else { "shown" },
            if self.clip_halves { "halves" } else { "name_bar" },
            if self.grid_in_clips { "on" } else { "off" },
            f64::from(self.waveform_zoom) / 100.0,
        ) + &format!(
            "waveform_colour = {}\nwaveform_custom_colour = {}\naudio_clip_colour_strength = {}\ntrack_header_colour_strength = {}\n",
            self.waveform.mode.name(),
            crate::kit::theme::hex(self.waveform.custom),
            self.audio_colour_strength,
            self.header_colour_strength,
        ) + &format!(
            "liquid_panel_strength = {}\nliquid_track_strength = {}\nliquid_clip_strength = {}\n",
            self.liquid.panels, self.liquid.tracks, self.liquid.clips,
        ) + &format!("replace_clip = {}\n", if self.replace_clip { "on" } else { "off" })
            + &format!("menu_bar = {}\n", if self.menu_bar { "on" } else { "off" })
            + &format!(
                "app_name_centered = {}\nbar_project_name = {}\nbar_project_timing = {}\nbar_project_centered = {}\n",
                self.app_name_centered, self.bar_project_name, self.bar_project_timing, self.bar_project_centered
            )
            + &format!("background_tint = {}\nbackground_blur = {}\n", self.background_tint, self.background_blur)
            + &self.track_sizes.to_text()
    }

    /// Reads one line of the settings file; `false` when the key and value are not one of these
    /// settings, so the caller goes on with its own.
    pub fn parse(&mut self, key: &str, value: &str) -> bool {
        if self.track_sizes.parse(key, value) {
            return true;
        }
        match (key, value) {
            // Retired experimental native backdrop: accept old preferences without enabling it.
            ("windows_mica", "on" | "off") => {}
            ("background_tint" | "background_blur", value) => {
                let Ok(value) = value.parse::<u8>() else { return false };
                if key == "background_tint" {
                    self.background_tint = value.min(100);
                } else {
                    self.background_blur = value.min(40);
                }
            }
            (
                "app_name_centered" | "bar_project_name" | "bar_project_timing" | "bar_project_centered",
                "true" | "false",
            ) => {
                let on = value == "true";
                match key {
                    "app_name_centered" => self.app_name_centered = on,
                    "bar_project_name" => self.bar_project_name = on,
                    "bar_project_timing" => self.bar_project_timing = on,
                    _ => self.bar_project_centered = on,
                }
            }
            ("replace_clip", "on" | "off") => self.replace_clip = value == "on",
            ("menu_bar", "on" | "off") => self.menu_bar = value == "on",
            ("liquid_panel_strength" | "liquid_track_strength" | "liquid_clip_strength", strength) => {
                let Ok(strength) = strength.parse::<u32>() else { return false };
                let strength = strength.min(100) as u8;
                match key {
                    "liquid_panel_strength" => self.liquid.panels = strength,
                    "liquid_track_strength" => self.liquid.tracks = strength,
                    _ => self.liquid.clips = strength,
                }
            }
            ("audio_clip_colour_strength" | "track_header_colour_strength", strength) => {
                let Ok(strength) = strength.parse::<u32>() else { return false };
                let strength = strength.min(100) as u8;
                if key == "audio_clip_colour_strength" {
                    self.audio_colour_strength = strength;
                } else {
                    self.header_colour_strength = strength;
                }
            }
            ("waveform_colour", mode) => {
                self.waveform.mode = match mode {
                    "theme" => WaveformColour::Theme,
                    "auto" => WaveformColour::Auto,
                    "light" => WaveformColour::Light,
                    "dark" => WaveformColour::Dark,
                    "custom" => WaveformColour::Custom,
                    _ => return false,
                };
            }
            ("waveform_custom_colour", colour) => {
                let Some(colour) = crate::kit::theme::parse_hex(colour) else { return false };
                self.waveform.custom = colour;
            }
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
            ("new_clip_colour", "track" | "theme") => self.new_clip_track_colour = value == "track",
            ("browser_recent", "chips" | "indented") => self.recent_chips = value == "chips",
            ("arrangement_returns", "hidden" | "shown") => self.returns_hidden = value == "hidden",
            ("clip_drag", "halves" | "name_bar") => self.clip_halves = value == "halves",
            ("clip_grid", "on" | "off") => self.grid_in_clips = value == "on",
            ("waveform_zoom", zoom) => match zoom.parse::<f32>() {
                Ok(zoom) if zoom.is_finite() => self.waveform_zoom = (zoom.clamp(1.0, 16.0) * 100.0).round() as u32,
                _ => return false,
            },
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
            waveform: WaveformAppearance {
                mode: WaveformColour::Custom,
                custom: egui::Color32::from_rgb(120, 210, 80),
            },
            audio_colour_strength: 64,
            header_colour_strength: 75,
            mixer_height: 300,
            strip_fader: 90,
            detail_layout: DetailLayout::FullWidth,
            double_click_clip: false,
            menu_name_field: true,
            notes_replace: false,
            time_select_ctrl: true,
            clip_halves: true,
            grid_in_clips: false,
            waveform_zoom: 400,
            stretch_alt: false,
            rename_selected: false,
            play_from_clip: false,
            fixed_track_colour: true,
            new_clip_track_colour: false,
            recent_chips: true,
            returns_hidden: true,
            replace_clip: true,
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
    fn waveform_choices_round_trip_and_invalid_colours_preserve_defaults() {
        assert_eq!(Settings::from_text("").0.view.waveform.mode, WaveformColour::Theme);
        for mode in [
            WaveformColour::Theme,
            WaveformColour::Auto,
            WaveformColour::Light,
            WaveformColour::Dark,
            WaveformColour::Custom,
        ] {
            let mut settings = Settings::default();
            settings.view.waveform = WaveformAppearance { mode, custom: egui::Color32::from_rgb(23, 140, 212) };
            let (read, warnings) = Settings::from_text(&settings.to_text());
            assert!(warnings.is_empty());
            assert_eq!(read.view.waveform, settings.view.waveform);
        }
        let (read, warnings) = Settings::from_text("waveform_colour = rainbow\nwaveform_custom_colour = #xyz123\n");
        assert_eq!(warnings.len(), 2);
        assert_eq!(read.view.waveform, WaveformAppearance::default());
    }

    #[test]
    fn colour_strengths_are_bounded_and_invalid_input_does_not_change_defaults() {
        let (read, warnings) =
            Settings::from_text("audio_clip_colour_strength = 9999\ntrack_header_colour_strength = -1\n");
        assert_eq!(warnings.len(), 1);
        assert_eq!(read.view.audio_colour_strength, 100);
        assert_eq!(read.view.header_colour_strength, 82);
    }

    #[test]
    fn liquid_strengths_default_off_and_parse_independently_with_bounds() {
        assert_eq!(Settings::from_text("").0.view.liquid, crate::kit::liquid::Options::default());
        let (read, warnings) =
            Settings::from_text("liquid_panel_strength = 999\nliquid_track_strength = -1\nliquid_clip_strength = 37\n");
        assert_eq!(warnings.len(), 1);
        assert_eq!(read.view.liquid, crate::kit::liquid::Options { panels: 100, tracks: 0, clips: 37 });
    }
}
