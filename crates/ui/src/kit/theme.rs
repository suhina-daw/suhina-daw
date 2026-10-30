// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Design tokens from `docs/design/brief.md` › Tokens and the sizes in
//! `docs/design/components.md`. Views use these names, never raw colours or sizes.
//!
//! Colours come from the theme in use (a [`Palette`]), so the user can change them: a theme is a
//! JSON file of token names and `#RRGGBB` values (see [`Theme::from_json`]). Sizes are fixed.

use std::cell::Cell;

use egui::{Color32, FontFamily, FontId, Stroke, TextStyle};
use serde_json::{Map, Value};

const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

/// Declares the colour tokens once: the [`Palette`] field, its name in a theme file, the default
/// (the built-in dark theme), what it is used for, and a function returning it from the theme in
/// use.
macro_rules! palette {
    ($($field:ident, $key:literal, $hex:literal, $use:literal;)*) => {
        /// Every colour token of a theme.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct Palette {
            $(#[doc = $use] pub $field: Color32,)*
        }

        impl Palette {
            /// The built-in dark theme: the values in the design brief.
            pub const DARK: Self = Self { $($field: rgb($hex),)* };

            /// Each token's name in a theme file, what it is used for, and its colour.
            pub fn tokens(&self) -> Vec<(&'static str, &'static str, Color32)> {
                vec![$(($key, $use, self.$field),)*]
            }

            /// The colour of the token named `key` (or by its first name, see [`RENAMED`]), to
            /// change.
            pub fn token_mut(&mut self, key: &str) -> Option<&mut Color32> {
                let key = RENAMED.iter().find(|(old, _)| *old == key).map_or(key, |(_, new)| *new);
                match key {
                    $($key => Some(&mut self.$field),)*
                    _ => None,
                }
            }
        }

        $(#[doc = $use] pub fn $field() -> Color32 { PALETTE.with(|p| p.get().$field) })*
    };
}

palette! {
    grid_major, "grid.major", 0x30312D,
        "Arrangement bar lines and lane separators";
    grid_minor, "grid.minor", 0x252622,
        "Arrangement subdivisions between bar lines";
    track_header_background, "track.headerBackground", 0x252421,
        "The neutral surface behind Arrangement track controls";
    track_header_selected, "track.headerSelectedBackground", 0x34332F,
        "The surface behind a selected Arrangement track's controls";
    window_background, "window.background", 0x1D1C1A,
        "The window behind everything: grid, empty slots, lanes, Settings";
    panel_background, "panel.background", 0x252421,
        "Panels and headers: menu, control and status bars, track headers, beat lines";
    panel_hover, "panel.hoverBackground", 0x2A2926,
        "A panel or list row under the pointer";
    control_background, "control.background", 0x2E2D2A,
        "Fields, buttons and stopped clips";
    control_hover, "control.hoverBackground", 0x34332F,
        "A field, button or clip under the pointer";
    control_active, "control.activeBackground", 0x45433F,
        "Selected or pressed: a selected clip or track, a dragged field";
    control_playing, "control.playingBackground", 0x4A4843,
        "A playing clip or scene";
    control_selected, "control.selectedBackground", 0xC9C4BB,
        "The chosen option of a switch, a lit toggle, the level of a volume or pan field";
    border, "border", 0x383733,
        "Hairlines between regions and rows";
    border_strong, "border.strong", 0x45433F,
        "Launcher columns and arrangement track lines";
    text_primary, "text.primary", 0xECE8E1,
        "Names and values";
    text_secondary, "text.secondary", 0x9A958C,
        "Labels and notes";
    text_hint, "text.hint", 0x66625B,
        "Inactive text, hints and section titles";
    text_value, "text.value", 0xC9C4BB,
        "Clip names and values on fields";
    text_inverse, "text.inverse", 0x1D1C1A,
        "Text on lit fills: a switch's chosen option, a lit button";
    text_playing, "text.playing", 0xFFFFFF,
        "The name of a playing clip";
    text_on_color_light, "text.onColorLight", 0xECE8E1,
        "Light text on dark coloured tracks, returns and clips; paired with text.onColorDark for contrast";
    text_on_color_dark, "text.onColorDark", 0x1D1C1A,
        "Dark text on bright coloured tracks, returns and clips; paired with text.onColorLight for contrast";
    browser_text_primary, "browser.text.primary", 0xECE8E1,
        "Browser selection, search input and highlighted names";
    browser_text_secondary, "browser.text.secondary", 0x9A958C,
        "Browser item names, categories and preview labels";
    browser_text_hint, "browser.text.hint", 0x66625B,
        "Browser section headings, search placeholder and hints";
    browser_text_value, "browser.text.value", 0xC9C4BB,
        "Browser values and active preview controls";
    accent, "accent", 0xA6A3FF,
        "The playhead, queued clips, a key being set";
    selection, "selection", 0xE3C86A,
        "The selected device's outline";
    recording, "recording", 0xE5484D,
        "Recording and errors";
    recording_background, "recording.background", 0x3A2B2A,
        "Behind an armed track's arm button and a recording clip";
    meter_normal, "meter.normal", 0x8BC48A,
        "A level meter below the top";
    meter_hot, "meter.hot", 0xE3C86A,
        "A level meter near the top";
    slot_empty, "slot.empty", 0x4A4743,
        "The mark in an empty clip slot";
    track_muted, "track.mutedBackground", 0x34322F,
        "The fills of a muted track";
    pan_background, "pan.background", 0x232220,
        "Behind a track header's pan field";
    pan_centre, "pan.centre", 0x55524C,
        "The centre mark of a pan field";
    midi_clip_default, "clip.midiDefault", 0x4FC3F7,
        "RGB colour copied only to newly created MIDI tracks and clips; existing tracks and clips never change";
    audio_clip_default, "clip.audioDefault", 0xFF9A3C,
        "RGB colour copied only to newly created audio tracks and clips; existing tracks and clips never change";
    track_color_1, "track.1", 0xFF9A3C,
        "Colour-picker swatch 1; also effects in the browser (legacy track token)";
    track_color_2, "track.2", 0xF2D541,
        "Colour-picker swatch 2; also MIDI files in the browser (legacy track token)";
    track_color_3, "track.3", 0x4FC3F7,
        "Colour-picker swatch 3; also samples in the browser (legacy track token)";
    track_color_4, "track.4", 0xB38BFF,
        "Colour-picker swatch 4; also instruments and presets in the browser (legacy track token)";
    track_color_5, "track.5", 0x7FDB5A,
        "A colour the track colour palette offers";
    track_color_6, "track.6", 0xFF6FA8,
        "A colour the track colour palette offers";
    track_color_7, "track.7", 0x5C8DFF,
        "A colour the track colour palette offers";
    track_color_8, "track.8", 0x2FD9C4,
        "A colour the track colour palette offers";
    kind_kick, "kind.kick", 0xD06E5E,
        "Sample-map points of the kind kick";
    kind_snare, "kind.snare", 0x73AAC0,
        "Sample-map points of the kind snare";
    kind_clap, "kind.clap", 0x5EC0B0,
        "Sample-map points of the kind clap";
    kind_closed_hat, "kind.closedHat", 0xC6AC5E,
        "Sample-map points of the kind closed hat";
    kind_open_hat, "kind.openHat", 0xD08F6E,
        "Sample-map points of the kind open hat";
    kind_hat, "kind.hat", 0xD8C47A,
        "Sample-map points of the kind hat";
    kind_rim, "kind.rim", 0x8CA8E0,
        "Sample-map points of the kind rim";
    kind_tom, "kind.tom", 0xB074A4,
        "Sample-map points of the kind tom";
    kind_crash, "kind.crash", 0xE0A0C8,
        "Sample-map points of the kind crash";
    kind_ride, "kind.ride", 0xC8B8E8,
        "Sample-map points of the kind ride";
    kind_cymbal, "kind.cymbal", 0xA497CC,
        "Sample-map points of the kind cymbal";
    kind_percussion, "kind.percussion", 0x8FC06E,
        "Sample-map points of the kind percussion";
    kind_bass, "kind.bass", 0xA84F4F,
        "Sample-map points of the kind bass";
    kind_tonal, "kind.tonal", 0x6EB0D0,
        "Sample-map points of the kind tonal";
    kind_vocal, "kind.vocal", 0xE8D0A0,
        "Sample-map points of the kind vocal";
    kind_effect, "kind.effect", 0x9CC49C,
        "Sample-map points of the kind effect";
    kind_other, "kind.other", 0x8A8680,
        "Sample-map points of the kind other";
}

/// Tokens renamed since the first theme files, old name first; files with the old names still
/// read.
const RENAMED: [(&str, &str); 13] = [
    ("surface.0", "window.background"),
    ("surface.1", "panel.background"),
    ("hover.surface.1", "panel.hoverBackground"),
    ("surface.2", "control.background"),
    ("hover.surface.2", "control.hoverBackground"),
    ("surface.3", "control.activeBackground"),
    ("surface.3.bright", "control.playingBackground"),
    ("line", "border"),
    ("line.strong", "border.strong"),
    ("text.tertiary", "text.hint"),
    ("state.recording", "recording"),
    ("meter.ok", "meter.normal"),
    ("muted.fill", "track.mutedBackground"),
];

impl Default for Palette {
    fn default() -> Self {
        Self::DARK
    }
}

thread_local! {
    /// The colours in use. The UI draws on one thread; tests each get their own, so a test that
    /// changes the theme does not change another's.
    static PALETTE: Cell<Palette> = const { Cell::new(Palette::DARK) };
    static PALETTE_REVISION: Cell<u64> = const { Cell::new(0) };
}

/// The colours in use.
pub fn palette() -> Palette {
    PALETTE.with(Cell::get)
}

pub(crate) fn local_palette(palette: Palette) {
    PALETTE.with(|p| p.set(palette));
}

/// Scoped CSS overrides must never roll back a user theme change made while drawing.
pub(crate) fn palette_revision() -> u64 {
    PALETTE_REVISION.with(Cell::get)
}

/// Whether a background of the theme in use lets the desktop show through the window.
pub fn see_through() -> bool {
    let p = palette();
    [
        p.window_background,
        p.panel_background,
        p.panel_hover,
        p.control_background,
        p.control_hover,
        p.control_active,
        p.control_playing,
        p.control_selected,
        p.track_header_background,
        p.track_header_selected,
        p.pan_background,
        p.track_muted,
        p.recording_background,
    ]
    .iter()
    .any(|color| color.a() < 255)
}

/// Draws with `palette` from the next frame on: the tokens and egui's own visuals.
pub fn set_palette(ctx: &egui::Context, palette: Palette) {
    PALETTE_REVISION.with(|revision| revision.set(revision.get().wrapping_add(1)));
    PALETTE.with(|p| p.set(palette));
    ctx.all_styles_mut(|style| visuals(&mut style.visuals, &palette));
    ctx.request_repaint();
}

/// The id of the default theme, Suhina Dark, in the settings file.
pub const BUILT_IN: &str = "suhina-dark";

/// The themes that come with Suhina besides Suhina Dark: id, `theme.css` and `layout.css` (empty
/// when it has none), as a theme's folder holds them. Their artwork and Meadow's background are
/// attached by `built_in_themes`.
/// Dracula, Nord, Catppuccin and Solarized are popular open palettes (MIT, their licences in
/// `themes/licenses/`), mapped onto Suhina's tokens.
const BUILT_IN_FILES: [(&str, &str, &str); 15] = [
    ("suhina-studio-dark", include_str!("../../themes/suhina-studio-dark.theme.css"), ""),
    ("suhina-studio-light", include_str!("../../themes/suhina-studio-light.theme.css"), ""),
    ("editor-dark", include_str!("../../themes/editor-dark.theme.css"), ""),
    ("suhina-light", include_str!("../../themes/suhina-light.theme.css"), ""),
    ("high-contrast", include_str!("../../themes/high-contrast.theme.css"), ""),
    ("dracula", include_str!("../../themes/dracula.theme.css"), ""),
    ("nord", include_str!("../../themes/nord.theme.css"), ""),
    ("catppuccin-mocha", include_str!("../../themes/catppuccin-mocha.theme.css"), ""),
    ("catppuccin-latte", include_str!("../../themes/catppuccin-latte.theme.css"), ""),
    ("solarized-dark", include_str!("../../themes/solarized-dark.theme.css"), ""),
    ("suhina-glass", include_str!("../../themes/suhina-glass.theme.css"), ""),
    ("suhina-forest-amber", include_str!("../../themes/suhina-forest-amber.theme.css"), ""),
    (
        "suhina-sheet",
        include_str!("../../themes/suhina-sheet.theme.css"),
        include_str!("../../themes/suhina-sheet.layout.css"),
    ),
    (
        "suhina-meadow",
        include_str!("../../themes/suhina-meadow.theme.css"),
        include_str!("../../themes/suhina-meadow.layout.css"),
    ),
    (
        "suhina-blocks",
        include_str!("../../themes/suhina-blocks.theme.css"),
        include_str!("../../themes/suhina-blocks.layout.css"),
    ),
];

const SHEET_SVGS: &[(&str, &str)] = &[
    ("fader.cap", include_str!("../../../../assets/themes/sheet/fader.cap.svg")),
    ("fader.rail", include_str!("../../../../assets/themes/sheet/fader.rail.svg")),
    ("knob.body", include_str!("../../../../assets/themes/sheet/knob.body.svg")),
    ("knob.pointer", include_str!("../../../../assets/themes/sheet/knob.pointer.svg")),
    ("button", include_str!("../../../../assets/themes/sheet/button.svg")),
    ("play", include_str!("../../../../assets/themes/sheet/play.svg")),
    ("stop", include_str!("../../../../assets/themes/sheet/stop.svg")),
    ("record", include_str!("../../../../assets/themes/sheet/record.svg")),
    ("solo", include_str!("../../../../assets/themes/sheet/solo.svg")),
    ("arm", include_str!("../../../../assets/themes/sheet/arm.svg")),
    ("cue", include_str!("../../../../assets/themes/sheet/cue.svg")),
    ("track.midi", include_str!("../../../../assets/themes/sheet/track.midi.svg")),
    ("track.audio", include_str!("../../../../assets/themes/sheet/track.audio.svg")),
    ("track.name", include_str!("../../../../assets/themes/sheet/track.name.svg")),
    ("track.body", include_str!("../../../../assets/themes/sheet/track.body.svg")),
    ("meter", include_str!("../../../../assets/themes/sheet/meter.svg")),
];

const MEADOW_SVGS: &[(&str, &str)] = &[
    ("fader.cap", include_str!("../../../../assets/themes/meadow/fader.cap.svg")),
    ("fader.rail", include_str!("../../../../assets/themes/meadow/fader.rail.svg")),
    ("knob.body", include_str!("../../../../assets/themes/meadow/knob.body.svg")),
    ("knob.pointer", include_str!("../../../../assets/themes/meadow/knob.pointer.svg")),
    ("button", include_str!("../../../../assets/themes/meadow/button.svg")),
    ("track.midi", include_str!("../../../../assets/themes/meadow/track.midi.svg")),
    ("track.body", include_str!("../../../../assets/themes/meadow/track.body.svg")),
];

const BLOCKS_SVGS: &[(&str, &str)] = &[
    ("fader.cap", include_str!("../../../../assets/themes/blocks/fader.cap.svg")),
    ("fader.rail", include_str!("../../../../assets/themes/blocks/fader.rail.svg")),
    ("knob.body", include_str!("../../../../assets/themes/blocks/knob.body.svg")),
    ("knob.pointer", include_str!("../../../../assets/themes/blocks/knob.pointer.svg")),
    ("button", include_str!("../../../../assets/themes/blocks/button.svg")),
    ("track.midi", include_str!("../../../../assets/themes/blocks/track.midi.svg")),
    ("track.audio", include_str!("../../../../assets/themes/blocks/track.audio.svg")),
    ("track.name", include_str!("../../../../assets/themes/blocks/track.name.svg")),
    ("track.body", include_str!("../../../../assets/themes/blocks/track.body.svg")),
    ("track.body.color-ff9a3c", include_str!("../../../../assets/themes/blocks/track.body.color-ff9a3c.svg")),
    ("track.body.color-f2d541", include_str!("../../../../assets/themes/blocks/track.body.color-f2d541.svg")),
    ("track.body.color-4fc3f7", include_str!("../../../../assets/themes/blocks/track.body.color-4fc3f7.svg")),
    ("track.body.color-b38bff", include_str!("../../../../assets/themes/blocks/track.body.color-b38bff.svg")),
    ("track.body.color-7fdb5a", include_str!("../../../../assets/themes/blocks/track.body.color-7fdb5a.svg")),
    ("track.body.color-e5484d", include_str!("../../../../assets/themes/blocks/track.body.color-e5484d.svg")),
    ("track.body.color-80ada1", include_str!("../../../../assets/themes/blocks/track.body.color-80ada1.svg")),
    ("track.body.color-a84f4f", include_str!("../../../../assets/themes/blocks/track.body.color-a84f4f.svg")),
];

fn meadow_background() -> Option<std::sync::Arc<str>> {
    static BACKGROUND: std::sync::OnceLock<Option<std::sync::Arc<str>>> = std::sync::OnceLock::new();
    BACKGROUND
        .get_or_init(|| super::skin::encode_png(include_bytes!("../../../../assets/themes/meadow/meadow.png")).ok())
        .clone()
}

/// The themes that come with Suhina, Suhina Dark first, with their ids. They do not change;
/// editing one makes a copy.
pub fn built_in_themes() -> Vec<(&'static str, Theme)> {
    let mut themes = vec![(BUILT_IN, Theme::default())];
    themes.extend(BUILT_IN_FILES.iter().map(|(id, sheet, layout)| {
        let mut theme = from_sheet(sheet, layout).0;
        if matches!(*id, "suhina-sheet" | "suhina-meadow" | "suhina-blocks") {
            theme.skin.assets = SHEET_SVGS.iter().map(|(role, svg)| ((*role).into(), (*svg).into())).collect();
            if *id == "suhina-meadow" {
                theme.skin.assets.extend(MEADOW_SVGS.iter().map(|(role, svg)| ((*role).into(), (*svg).into())));
                theme.skin.background = meadow_background();
            } else if *id == "suhina-blocks" {
                theme.skin.assets.extend(BLOCKS_SVGS.iter().map(|(role, svg)| ((*role).into(), (*svg).into())));
            }
        }
        (*id, theme)
    }));
    themes
}

/// A theme from its own sheet (`theme.css`) and Suhina's `layout.css` beside it, with warnings
/// about what could not be read. Artwork and the background stay as paths for the theme's files
/// to resolve.
pub fn from_sheet(sheet: &str, layout: &str) -> (Theme, Vec<String>) {
    let mut theme = Theme::default();
    let mut warnings = Vec::new();
    match super::theme_vars::read(sheet) {
        Ok((declared, base, notices)) => {
            theme.vars.declared = std::sync::Arc::new(declared);
            theme.vars.base = std::sync::Arc::new(base);
            warnings.extend(notices);
        }
        Err(error) => warnings.push(error),
    }
    warnings.extend(super::theme_vars::resolve(&mut theme));
    warnings.extend(super::theme_css::read_host_rules(&mut theme, sheet));
    warnings.extend(super::theme_css::apply(&mut theme, layout));
    theme.skin.css = Some(sheet.into());
    (theme, warnings)
}

/// Whether `id` names a theme that comes with Suhina.
pub fn is_built_in(id: &str) -> bool {
    id.eq_ignore_ascii_case(BUILT_IN) || BUILT_IN_FILES.iter().any(|(b, _, _)| id.eq_ignore_ascii_case(b))
}

/// A named set of colours, kept in a theme file:
///
/// ```json
/// { "name": "Night", "colors": { "window.background": "#1D1C1A", "accent": "#A6A3FF" } }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    pub skin: super::skin::Skin,
    pub transparent_rgb: std::collections::BTreeMap<String, [u8; 3]>,
    pub name: String,
    pub colors: Palette,
    /// None inherits the application's global material strengths.
    pub liquid: Option<super::liquid::Options>,
    pub gradients: super::gradient::Gradients,
    /// The theme's custom properties and the chosen variant's values (`theme_vars`).
    pub vars: super::theme_vars::Vars,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            name: "Suhina Dark".into(),
            colors: Palette::DARK,
            liquid: None,
            gradients: Default::default(),
            transparent_rgb: Default::default(),
            skin: Default::default(),
            vars: Default::default(),
        }
    }
}

impl Theme {
    pub fn rgba(&self, key: &str, color: Color32) -> [u8; 4] {
        let mut rgba = color.to_srgba_unmultiplied();
        if color.a() == 0
            && let Some(rgb) = self.transparent_rgb.get(key)
        {
            rgba[..3].copy_from_slice(rgb);
        }
        rgba
    }

    /// Reads a theme file. Tokens it leaves out keep Suhina Dark's colour, so a file only needs
    /// the colours it changes; unknown tokens and values that are not `#RRGGBB` or `#RRGGBBAA` are skipped with
    /// a warning. A `"$schema"` or `"description"` is allowed and ignored.
    pub fn from_json(text: &str) -> (Self, Vec<String>) {
        let mut theme = Self::default();
        let mut warnings = Vec::new();
        let root = match serde_json::from_str::<Value>(text) {
            Ok(Value::Object(root)) => root,
            Ok(_) => return (theme, vec!["a theme file is a JSON object".into()]),
            Err(e) => return (theme, vec![format!("the theme is not JSON: {e}")]),
        };
        match root.get("name") {
            Some(Value::String(name)) if !name.trim().is_empty() => theme.name = name.trim().to_string(),
            Some(_) => warnings.push("the theme's name is not text".into()),
            None => {}
        }
        match root.get("colors") {
            Some(Value::Object(colors)) => {
                for (key, value) in colors {
                    let canonical = RENAMED.iter().find(|(old, _)| *old == key).map_or(key.as_str(), |(_, new)| *new);
                    let parsed = value.as_str().and_then(parse_hex);
                    match (theme.colors.token_mut(canonical), parsed) {
                        (Some(slot), Some(color)) => {
                            *slot = color;
                            theme.transparent_rgb.remove(canonical);
                            if let Some([r, g, b, 0]) = value.as_str().and_then(parse_rgba)
                                && [r, g, b] != [0, 0, 0]
                            {
                                theme.transparent_rgb.insert(canonical.to_string(), [r, g, b]);
                            }
                        }
                        (None, _) => warnings.push(format!("{key} is not a colour token")),
                        (Some(_), None) => {
                            warnings.push(format!("{key}: {value} is not a #RRGGBB or #RRGGBBAA colour"))
                        }
                    }
                }
                // Older palettes only supplied the numbered colour-picker swatches.
                if !colors.contains_key("clip.midiDefault") {
                    theme.colors.midi_clip_default = theme.colors.track_color_3;
                }
                if !colors.contains_key("clip.audioDefault") {
                    theme.colors.audio_clip_default = theme.colors.track_color_1;
                }
                // Older files drew these with a background and a text colour.
                if !colors.contains_key("text.inverse") {
                    let [r, g, b, _] = theme.colors.window_background.to_srgba_unmultiplied();
                    let [r, g, b] = theme.transparent_rgb.get("window.background").copied().unwrap_or([r, g, b]);
                    theme.colors.text_inverse = Color32::from_rgb(r, g, b);
                }
                if !colors.contains_key("control.selectedBackground") {
                    theme.colors.control_selected = theme.colors.text_value;
                }
                // Keep old themes visually identical until these independent roles are edited.
                for (key, source, value) in [
                    ("browser.text.primary", "text.primary", theme.colors.text_primary),
                    ("browser.text.secondary", "text.secondary", theme.colors.text_secondary),
                    ("browser.text.hint", "text.hint", theme.colors.text_hint),
                    ("browser.text.value", "text.value", theme.colors.text_value),
                    ("text.onColorLight", "text.primary", theme.colors.text_primary),
                    ("text.onColorDark", "window.background", theme.colors.window_background),
                ] {
                    if !colors.contains_key(key)
                        && let Some(slot) = theme.colors.token_mut(key)
                    {
                        *slot = value;
                        if let Some(rgb) = theme.transparent_rgb.get(source).copied() {
                            theme.transparent_rgb.insert(key.to_string(), rgb);
                        }
                    }
                }
                // Text never takes a see-through background's alpha.
                if !colors.contains_key("text.onColorDark") {
                    let [r, g, b, _] = theme.colors.text_on_color_dark.to_srgba_unmultiplied();
                    let [r, g, b] = theme.transparent_rgb.remove("text.onColorDark").unwrap_or([r, g, b]);
                    theme.colors.text_on_color_dark = Color32::from_rgb(r, g, b);
                }
            }
            Some(_) => warnings.push("the theme's colors are not an object".into()),
            None => {}
        }
        if let Some(value) = root.get("liquid") {
            if let Some(values) = value.as_object() {
                let mut options = super::liquid::Options::default();
                for (key, slot) in
                    [("panels", &mut options.panels), ("tracks", &mut options.tracks), ("clips", &mut options.clips)]
                {
                    match values.get(key).and_then(Value::as_u64).filter(|n| *n <= 100) {
                        Some(n) => *slot = n as u8,
                        None => warnings.push(format!("liquid.{key} must be a number from 0 to 100")),
                    }
                }
                theme.liquid = Some(options);
            } else {
                warnings.push("liquid must be an object of panels, tracks and clips strengths".into());
            }
        }
        if let Some(value) = root.get("gradients") {
            theme.gradients = super::gradient::Gradients::from_json(value, &mut warnings);
        }
        if let Some(value) = root.get("skin") {
            theme.skin = super::skin::Skin::from_json(value, &mut warnings);
        }
        let known = |k: &str| {
            matches!(
                k,
                "advanced-stylesheet"
                    | "stylesheet"
                    | "skin"
                    | "name"
                    | "colors"
                    | "liquid"
                    | "gradients"
                    | "description"
                    | "$schema"
            )
        };
        for key in root.keys().filter(|k| !known(k)) {
            warnings.push(format!("{key} is not part of a theme"));
        }
        (theme, warnings)
    }

    /// The theme as a file, every token written out so it can be edited.
    pub fn to_json(&self) -> String {
        let colors: Map<String, Value> = self
            .colors
            .tokens()
            .into_iter()
            .map(|(key, _, color)| (key.to_string(), Value::String(hex_rgba(self.rgba(key, color)))))
            .collect();
        let mut root = Map::new();
        if self.skin != super::skin::Skin::default() {
            root.insert("skin".into(), self.skin.to_json());
        }
        root.insert("name".into(), Value::String(self.name.clone()));
        root.insert("colors".into(), Value::Object(colors));
        if self.gradients != super::gradient::Gradients::default() {
            root.insert("gradients".into(), self.gradients.to_json());
        }
        if let Some(options) = self.liquid {
            root.insert(
                "liquid".into(),
                serde_json::json!({ "panels": options.panels, "tracks": options.tracks, "clips": options.clips }),
            );
        }
        serde_json::to_string_pretty(&Value::Object(root)).unwrap_or_default()
    }
}

/// Original paint-only example, shared by the editor and portable packages.
pub const GLOSS_EXAMPLE: &str = include_str!("../../themes/dark-gloss.css");

/// Optional native Launcher density, shared by the editor and exported packages.
pub const COMPACT_SESSION_EXAMPLE: &str = include_str!("../../themes/compact-session.css");

/// Optional panels own their dimensions so closed panels leave no fixed-size track.
pub const WORKSPACE_GRID_EXAMPLE: &str = "/* CSS owns panel geometry; removing these rules restores splitters. */\n#workspace { display:grid; grid-template-columns:minmax(0, 1fr) auto; grid-template-rows:minmax(0, 1fr) auto; gap:8px; }\n#panel-browser { grid-column:2; grid-row:1 / -1; width:240px; }\n#panel-main { grid-column:1; grid-row:1; }\n#panel-detail { grid-column:1; grid-row:2; height:180px; }\n";

/// The README shipped with themes: the format, capabilities and generated palette reference.
pub fn template_readme() -> String {
    let mut text = String::from(include_str!("../../themes/THEME-PACKAGE-README.md"));
    text.push_str("\n\n## Palette token reference\n\n| Token | Suhina Dark | Used for |\n|---|---|---|\n");
    for (key, purpose, color) in Palette::DARK.tokens() {
        text.push_str(&format!("| `{key}` | `{}` | {purpose} |\n", hex(color)));
    }
    text
}

/// Opaque `#RRGGBB`, or `#RRGGBBAA` when opacity is present.
pub fn hex(color: Color32) -> String {
    hex_rgba(color.to_srgba_unmultiplied())
}

pub fn hex_rgba([r, g, b, a]: [u8; 4]) -> String {
    if a == 255 { format!("#{r:02X}{g:02X}{b:02X}") } else { format!("#{r:02X}{g:02X}{b:02X}{a:02X}") }
}

/// Reads `#RRGGBB` or `#RRGGBBAA` (the `#` may be left out).
pub fn parse_hex(text: &str) -> Option<Color32> {
    let [r, g, b, a] = parse_rgba(text)?;
    Some(Color32::from_rgba_unmultiplied(r, g, b, a))
}

pub fn parse_rgba(text: &str) -> Option<[u8; 4]> {
    let digits = text.trim().strip_prefix('#').unwrap_or(text.trim());
    if !matches!(digits.len(), 6 | 8) || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let value = u32::from_str_radix(digits, 16).ok()?;
    Some(if digits.len() == 6 {
        [(value >> 16) as u8, (value >> 8) as u8, value as u8, 255]
    } else {
        [(value >> 24) as u8, (value >> 16) as u8, (value >> 8) as u8, value as u8]
    })
}

/// Text drawn on `fill`: the independent light or dark coloured-surface text role, whichever stands out more from
/// it, so a name stays readable on any track colour of any theme.
pub fn text_on(fill: Color32) -> Color32 {
    let (light, dark) = (text_on_color_light(), text_on_color_dark());
    if contrast(fill, light) >= contrast(fill, dark) { light } else { dark }
}

/// The contrast ratio of two colours, 1 … 21 (WCAG 2's relative luminance).
pub fn contrast(a: Color32, b: Color32) -> f32 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

fn luminance(color: Color32) -> f32 {
    let linear = |v: u8| {
        let s = f32::from(v) / 255.0;
        if s <= 0.040_45 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * linear(color.r()) + 0.7152 * linear(color.g()) + 0.0722 * linear(color.b())
}

/// Grid rows, headers, fields.
pub const ROW: f32 = 20.0;
/// A row of the browser's lists and tree.
pub const BROWSER_ROW: f32 = 17.0;
pub const MENU_BAR: f32 = 20.0;
pub const CONTROL_BAR: f32 = 40.0;
pub const STATUS_BAR: f32 = 20.0;
pub const TRACK_COLUMN: f32 = 116.0;
pub const RETURN_COLUMN: f32 = 84.0;
pub const SCENE_COLUMN: f32 = 96.0;
pub const KNOB: f32 = 22.0;
pub const TOGGLE: f32 = 18.0;
/// Padding inside cells and strips.
pub const PAD: f32 = 6.0;

/// Text sizes: UI text, small values and labels, rulers and key labels.
pub const TEXT: f32 = 12.0;
pub const TEXT_SMALL: f32 = 11.0;
pub const TEXT_TINY: f32 = 9.0;

pub fn ui_font(size: f32) -> FontId {
    FontId::new(super::style_class::font_size(size), FontFamily::Proportional)
}

/// Every number, time and position is monospaced.
pub fn mono_font(size: f32) -> FontId {
    FontId::new(super::style_class::font_size(size), FontFamily::Monospace)
}

pub fn hairline(color: Color32) -> Stroke {
    Stroke::new(1.0, color)
}

/// The default UI typeface, shared with the website (SIL Open Font License 1.1).
pub(crate) const UI_FONT: &[u8] = include_bytes!("../../fonts/manrope/Manrope-Variable.ttf");
/// Preserve the previous UI font as an embedded alternative and glyph fallback.
const LEGACY_UI_FONT: &[u8] = include_bytes!("../../fonts/encode-sans/EncodeSansSemiExpanded-Regular.ttf");
/// Numbers, times and positions: IBM Plex Mono (SIL Open Font License 1.1,
/// `fonts/ibm-plex-mono/OFL.txt`).
pub(crate) const MONO_FONT: &[u8] = include_bytes!("../../fonts/ibm-plex-mono/IBMPlexMono-Regular.ttf");

/// egui's fonts with ours first: Manrope for UI text, IBM Plex Mono for numbers. egui's own
/// fonts stay behind them for characters ours lack.
fn fonts() -> egui::FontDefinitions {
    let mut fonts = egui::FontDefinitions::default();
    let manrope = egui::FontData::from_static(UI_FONT).tweak(egui::FontTweak {
        coords: egui::epaint::text::VariationCoords::new([(b"wght", 500.0)]),
        ..Default::default()
    });
    fonts.font_data.insert("manrope".into(), std::sync::Arc::new(manrope));
    fonts.font_data.insert("encode-sans".into(), std::sync::Arc::new(egui::FontData::from_static(LEGACY_UI_FONT)));
    fonts.font_data.insert("ibm-plex-mono".into(), std::sync::Arc::new(egui::FontData::from_static(MONO_FONT)));
    fonts.families.entry(FontFamily::Proportional).or_default().insert(0, "encode-sans".into());
    fonts.families.entry(FontFamily::Proportional).or_default().insert(0, "manrope".into());
    fonts.families.insert(FontFamily::Name("encode-sans".into()), vec!["encode-sans".into()]);
    fonts.families.entry(FontFamily::Monospace).or_default().insert(0, "ibm-plex-mono".into());
    fonts
}

/// Sets the fonts, and egui's visuals and text styles to the tokens, for the few stock widgets
/// the views use.
pub fn install(ctx: &egui::Context) {
    ctx.set_fonts(fonts());
    ctx.set_theme(egui::Theme::Dark);
    let palette = palette();
    ctx.all_styles_mut(|style| {
        style.text_styles = [
            (TextStyle::Heading, ui_font(TEXT)),
            (TextStyle::Body, ui_font(TEXT)),
            (TextStyle::Button, ui_font(TEXT)),
            (TextStyle::Small, ui_font(TEXT_SMALL)),
            (TextStyle::Monospace, mono_font(TEXT_SMALL)),
        ]
        .into();
        style.spacing.item_spacing = egui::vec2(6.0, 4.0);
        style.spacing.button_padding = egui::vec2(6.0, 2.0);
        style.spacing.interact_size.y = ROW;
        // Text is read, not selected: no text cursor over labels.
        style.interaction.selectable_labels = false;
        visuals(&mut style.visuals, &palette);
    });
}

/// egui's visuals in the colours of `p`.
fn visuals(visuals: &mut egui::Visuals, p: &Palette) {
    // egui's own light or dark defaults under the tokens, by how light the window is.
    let [r, g, b, _] = p.window_background.to_array();
    let light = 0.299 * f32::from(r) + 0.587 * f32::from(g) + 0.114 * f32::from(b) > 128.0;
    *visuals = if light { egui::Visuals::light() } else { egui::Visuals::dark() };
    visuals.panel_fill = p.panel_background;
    visuals.window_fill = p.panel_background;
    visuals.extreme_bg_color = p.window_background;
    visuals.text_edit_bg_color = Some(Color32::TRANSPARENT);
    visuals.faint_bg_color = p.panel_background;
    visuals.window_stroke = hairline(p.border);
    visuals.window_corner_radius = egui::CornerRadius::ZERO;
    visuals.menu_corner_radius = egui::CornerRadius::ZERO;
    visuals.window_shadow = egui::epaint::Shadow::NONE;
    visuals.popup_shadow = egui::epaint::Shadow::NONE;
    visuals.selection.bg_fill = p.control_active;
    visuals.selection.stroke = hairline(p.accent);
    visuals.override_text_color = None;
    for (widget, fill) in [
        (&mut visuals.widgets.noninteractive, p.panel_background),
        (&mut visuals.widgets.inactive, p.control_background),
        (&mut visuals.widgets.hovered, p.control_hover),
        (&mut visuals.widgets.active, p.control_active),
        (&mut visuals.widgets.open, p.control_active),
    ] {
        widget.bg_fill = fill;
        widget.weak_bg_fill = fill;
        widget.corner_radius = egui::CornerRadius::ZERO;
        widget.bg_stroke = Stroke::NONE;
        widget.expansion = 0.0;
    }
    visuals.widgets.noninteractive.bg_stroke = hairline(p.border);
    visuals.widgets.noninteractive.fg_stroke = hairline(p.text_secondary);
    visuals.widgets.inactive.fg_stroke = hairline(p.text_value);
    visuals.widgets.hovered.fg_stroke = hairline(p.text_primary);
    visuals.widgets.active.fg_stroke = hairline(p.text_primary);
    visuals.widgets.open.fg_stroke = hairline(p.text_primary);
}

#[cfg(test)]
#[path = "theme_tests.rs"]
mod tests;
