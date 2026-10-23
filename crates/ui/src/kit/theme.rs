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
        "A playing clip or scene, the fill of a volume field";
    border, "border", 0x383733,
        "Hairlines between regions and rows";
    border_strong, "border.strong", 0x45433F,
        "Session columns and arrangement track lines";
    text_primary, "text.primary", 0xECE8E1,
        "Names and values";
    text_secondary, "text.secondary", 0x9A958C,
        "Labels and notes";
    text_hint, "text.hint", 0x66625B,
        "Inactive text, hints and section titles";
    text_value, "text.value", 0xC9C4BB,
        "Clip names, values on fields, the chosen option of a switch";
    text_playing, "text.playing", 0xFFFFFF,
        "The name of a playing clip";
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
    track_color_1, "track.1", 0xFF9A3C,
        "The first default track colour: new tracks take them in turn; also effects in the browser";
    track_color_2, "track.2", 0xF2D541,
        "The second default track colour; also MIDI files in the browser";
    track_color_3, "track.3", 0x4FC3F7,
        "The third default track colour; also samples in the browser";
    track_color_4, "track.4", 0xB38BFF,
        "The fourth default track colour; also instruments and presets in the browser";
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
}

/// The colours in use.
pub fn palette() -> Palette {
    PALETTE.with(Cell::get)
}

/// Draws with `palette` from the next frame on: the tokens and egui's own visuals.
pub fn set_palette(ctx: &egui::Context, palette: Palette) {
    PALETTE.with(|p| p.set(palette));
    ctx.all_styles_mut(|style| visuals(&mut style.visuals, &palette));
    ctx.request_repaint();
}

/// The id of the default theme, Suhina Dark, in the settings file.
pub const BUILT_IN: &str = "suhina-dark";

/// The themes that come with Suhina besides Suhina Dark, as theme files: id and text.
/// Dracula, Nord, Catppuccin and Solarized are popular open palettes (MIT, their licences in
/// `themes/licenses/`), mapped onto Suhina's tokens.
const BUILT_IN_FILES: [(&str, &str); 8] = [
    ("editor-dark", include_str!("../../themes/editor-dark.json")),
    ("suhina-light", include_str!("../../themes/suhina-light.json")),
    ("high-contrast", include_str!("../../themes/high-contrast.json")),
    ("dracula", include_str!("../../themes/dracula.json")),
    ("nord", include_str!("../../themes/nord.json")),
    ("catppuccin-mocha", include_str!("../../themes/catppuccin-mocha.json")),
    ("catppuccin-latte", include_str!("../../themes/catppuccin-latte.json")),
    ("solarized-dark", include_str!("../../themes/solarized-dark.json")),
];

/// The themes that come with Suhina, Suhina Dark first, with their ids. They do not change;
/// editing one makes a copy.
pub fn built_in_themes() -> Vec<(&'static str, Theme)> {
    let mut themes = vec![(BUILT_IN, Theme::default())];
    themes.extend(BUILT_IN_FILES.iter().map(|(id, text)| (*id, Theme::from_json(text).0)));
    themes
}

/// Whether `id` names a theme that comes with Suhina.
pub fn is_built_in(id: &str) -> bool {
    id.eq_ignore_ascii_case(BUILT_IN) || BUILT_IN_FILES.iter().any(|(b, _)| id.eq_ignore_ascii_case(b))
}

/// A named set of colours, kept in a theme file:
///
/// ```json
/// { "name": "Night", "colors": { "window.background": "#1D1C1A", "accent": "#A6A3FF" } }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    pub name: String,
    pub colors: Palette,
}

impl Default for Theme {
    fn default() -> Self {
        Self { name: "Suhina Dark".into(), colors: Palette::DARK }
    }
}

impl Theme {
    /// Reads a theme file. Tokens it leaves out keep Suhina Dark's colour, so a file only needs
    /// the colours it changes; unknown tokens and values that are not `#RRGGBB` are skipped with
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
                    let parsed = value.as_str().and_then(parse_hex);
                    match (theme.colors.token_mut(key), parsed) {
                        (Some(slot), Some(color)) => *slot = color,
                        (None, _) => warnings.push(format!("{key} is not a colour token")),
                        (Some(_), None) => warnings.push(format!("{key}: {value} is not a #RRGGBB colour")),
                    }
                }
            }
            Some(_) => warnings.push("the theme's colors are not an object".into()),
            None => {}
        }
        let known = |k: &str| matches!(k, "name" | "colors" | "description" | "$schema");
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
            .map(|(key, _, color)| (key.to_string(), Value::String(hex(color))))
            .collect();
        let mut root = Map::new();
        root.insert("name".into(), Value::String(self.name.clone()));
        root.insert("colors".into(), Value::Object(colors));
        serde_json::to_string_pretty(&Value::Object(root)).unwrap_or_default()
    }
}

/// The README that goes with the theme template: the file format and every token, written so a
/// person or an AI agent can make a theme from it alone.
pub fn template_readme() -> String {
    let mut text = String::from(
        "# Suhina theme template\n\
\n\
A Suhina theme is one JSON file. `template.json` beside this README is Suhina Dark with every\n\
colour written out; copy it, give it a name and change the colours.\n\
\n\
## The file\n\
\n\
```json\n\
{\n  \"name\": \"Night Blue\",\n  \"colors\": {\n    \"window.background\": \"#14161A\",\n    \"accent\": \"#7FD1B9\"\n  }\n}\n\
```\n\
\n\
- `name`: what Settings shows. Any text.\n\
- `colors`: token name to colour. Every colour is `#RRGGBB` (six hex digits, no alpha).\n\
- A token left out keeps Suhina Dark's colour, so a theme may list only what it changes.\n\
- An unknown token or a colour that is not `#RRGGBB` is skipped, and Suhina shows a warning.\n\
- Nothing else belongs in the file (`description` and `$schema` are allowed and ignored).\n\
\n\
## Installing it\n\
\n\
Save it as `<id>.json` in Suhina's `themes` folder (Settings › Appearance › Theme › Open folder\n\
shows it), or drop the file on Suhina's window. The file name without `.json` is the theme's\n\
id: letters, digits and `-`, for example `night-blue.json`. Pick it in Settings › Appearance ›\n\
Theme. While it is in use, every save of the file shows within a second.\n\
\n\
## Making a good theme\n\
\n\
- The backgrounds go in steps: `window.background` (furthest back), then `panel.background`,\n\
  `control.background`, `control.activeBackground` and `control.playingBackground`. Keep each a\n\
  little lighter than the one before in a dark theme, a little darker in a light one.\n\
- Each `*.hoverBackground` is one small step from the background it hovers over.\n\
- `text.primary` must read well on every background; `text.secondary` a little less, `text.hint`\n\
  least (it is for hints and inactive text). Keep `text.primary` on `window.background` at a\n\
  contrast of at least 7:1 and `text.hint` at 3:1.\n\
- `text.value` is drawn on `control.background` and is the fill of the chosen option of a switch,\n\
  whose label is then drawn in `window.background`: the two must contrast.\n\
- `accent` marks the playhead, queued clips and a key being set; it should stand out from every\n\
  background. `recording` is also used for errors.\n\
- Track and clip colours come from the project, not the theme.\n\
\n\
## Tokens\n\
\n\
| Token | Suhina Dark | Used for |\n\
|---|---|---|\n",
    );
    for (key, purpose, color) in Palette::DARK.tokens() {
        text.push_str(&format!("| `{key}` | `{}` | {purpose} |\n", hex(color)));
    }
    text
}

/// `#RRGGBB`, as theme files write a colour.
pub fn hex(color: Color32) -> String {
    format!("#{:02X}{:02X}{:02X}", color.r(), color.g(), color.b())
}

/// Reads `#RRGGBB` (the `#` may be left out).
pub fn parse_hex(text: &str) -> Option<Color32> {
    let digits = text.trim().strip_prefix('#').unwrap_or(text.trim());
    if digits.len() != 6 || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(digits, 16).ok().map(rgb)
}

/// Text drawn on `fill`: `text.primary` or `window.background`, whichever stands out more from
/// it, so a name stays readable on any track colour of any theme.
pub fn text_on(fill: Color32) -> Color32 {
    let (light, dark) = (text_primary(), window_background());
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
pub const CONTROL_BAR: f32 = 28.0;
pub const STATUS_BAR: f32 = 20.0;
pub const TRACK_COLUMN: f32 = 116.0;
pub const RETURN_COLUMN: f32 = 84.0;
pub const SCENE_COLUMN: f32 = 96.0;
pub const KNOB: f32 = 22.0;
pub const TOGGLE: f32 = 18.0;
/// Padding inside cells and strips.
pub const PAD: f32 = 6.0;

/// Text sizes: UI text, small values and labels, rulers and key labels.
pub const TEXT: f32 = 11.0;
pub const TEXT_SMALL: f32 = 10.0;
pub const TEXT_TINY: f32 = 9.0;

pub fn ui_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

/// Every number, time and position is monospaced.
pub fn mono_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}

pub fn hairline(color: Color32) -> Stroke {
    Stroke::new(1.0, color)
}

/// The UI typeface: Encode Sans Semi Expanded (SIL Open Font License 1.1,
/// `fonts/encode-sans/OFL.txt`).
const UI_FONT: &[u8] = include_bytes!("../../fonts/encode-sans/EncodeSansSemiExpanded-Regular.ttf");
/// Numbers, times and positions: IBM Plex Mono (SIL Open Font License 1.1,
/// `fonts/ibm-plex-mono/OFL.txt`).
const MONO_FONT: &[u8] = include_bytes!("../../fonts/ibm-plex-mono/IBMPlexMono-Regular.ttf");

/// egui's fonts with ours first: Encode Sans for UI text, IBM Plex Mono for numbers. egui's own
/// fonts stay behind them for characters ours lack.
fn fonts() -> egui::FontDefinitions {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert("encode-sans".into(), std::sync::Arc::new(egui::FontData::from_static(UI_FONT)));
    fonts.font_data.insert("ibm-plex-mono".into(), std::sync::Arc::new(egui::FontData::from_static(MONO_FONT)));
    fonts.families.entry(FontFamily::Proportional).or_default().insert(0, "encode-sans".into());
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
