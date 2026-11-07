// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Design tokens from `docs/design/brief.md` › Tokens and the sizes in
//! `docs/design/components.md`. Views use these names, never raw colours or sizes.
//!
//! Colours come from the theme in use (a [`Palette`]), so the user can change them: a theme is a
//! JSON file of token names and `#RRGGBB` values (see [`Theme::from_json`]). Sizes are fixed.

use std::cell::Cell;
use std::sync::Arc;

use egui::{Color32, FontFamily, FontId, Stroke, TextStyle};
use serde_json::{Map, Value};
pub use suhina_theme_package::meta::{FILE as META, THUMBNAIL, ThemeMeta};

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
    grid_major, "grid.major", 0x2C2B29,
        "Arrangement bar lines and lane separators";
    grid_minor, "grid.minor", 0x252422,
        "Arrangement subdivisions between bar lines";
    track_header_background, "track.headerBackground", 0x252421,
        "The neutral surface behind Arrangement track controls";
    track_header_selected, "track.headerSelectedBackground", 0x3E3E4C,
        "The surface behind a selected Arrangement track's controls";
    window_background, "window.background", 0x1D1C1A,
        "The window behind everything: grid, empty slots, lanes, Settings";
    panel_background, "panel.background", 0x252421,
        "Panels and headers: menu, control and status bars, track headers, beat lines";
    panel_hover, "panel.hoverBackground", 0x31302C,
        "A panel or list row under the pointer";
    control_background, "control.background", 0x343330,
        "Fields, buttons and stopped clips";
    control_hover, "control.hoverBackground", 0x3F3D3A,
        "A field, button or clip under the pointer";
    control_active, "control.activeBackground", 0x4B4A46,
        "Selected or pressed: a selected clip or track, a dragged field";
    control_playing, "control.playingBackground", 0x545472,
        "A playing clip or scene";
    control_selected, "control.selectedBackground", 0xD0CCC6,
        "The chosen option of a switch, a lit toggle, the level of a volume or pan field";
    border, "border", 0x343330,
        "Hairlines between regions and rows";
    border_strong, "border.strong", 0x484642,
        "Launcher columns and arrangement track lines";
    text_primary, "text.primary", 0xECE8E1,
        "Names and values";
    text_secondary, "text.secondary", 0xB5B2AC,
        "Labels and notes";
    text_hint, "text.hint", 0x85837E,
        "Inactive text, hints and section titles";
    text_value, "text.value", 0xDAD6D0,
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
    browser_text_secondary, "browser.text.secondary", 0xB5B2AC,
        "Browser item names, categories and preview labels";
    browser_text_hint, "browser.text.hint", 0x85837E,
        "Browser section headings, search placeholder and hints";
    browser_text_value, "browser.text.value", 0xDAD6D0,
        "Browser values and active preview controls";
    accent, "accent", 0xA6A3FF,
        "The playhead, queued clips, a key being set";
    selection, "selection", 0xE3C86A,
        "The selected device's outline";
    recording, "recording", 0xE5484D,
        "Recording and errors";
    recording_background, "recording.background", 0x4D2F2B,
        "Behind an armed track's arm button and a recording clip";
    meter_normal, "meter.normal", 0x8BC48A,
        "A level meter below the top";
    meter_hot, "meter.hot", 0xE3C86A,
        "A level meter near the top";
    slot_empty, "slot.empty", 0x484642,
        "The mark in an empty clip slot";
    track_muted, "track.mutedBackground", 0x21201D,
        "The fills of a muted track";
    pan_background, "pan.background", 0x1F1E1C,
        "Behind a track header's pan field";
    pan_centre, "pan.centre", 0x817F7A,
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

/// Whether the theme in use is a light one, which the system's parts of the window follow.
pub fn is_light() -> bool {
    light(&palette())
}

/// Whether `p` is a light theme: one with dark text. A background can be see-through, so its
/// colour alone does not tell, but text is always made to read on what the theme looks like.
fn light(p: &Palette) -> bool {
    luminance(p.text_primary) < 0.18
}

/// The light or dark scheme of `p`. egui passes it on to the native window (its title bar).
fn scheme(p: &Palette) -> egui::Theme {
    if light(p) { egui::Theme::Light } else { egui::Theme::Dark }
}

/// Draws with `palette` from the next frame on: the tokens and egui's own visuals.
pub fn set_palette(ctx: &egui::Context, palette: Palette) {
    PALETTE_REVISION.with(|revision| revision.set(revision.get().wrapping_add(1)));
    PALETTE.with(|p| p.set(palette));
    ctx.all_styles_mut(|style| visuals(&mut style.visuals, &palette));
    ctx.set_theme(scheme(&palette));
    ctx.request_repaint();
}

/// The id of the default theme, Suhina Studio, in the settings file.
pub const BUILT_IN: &str = "suhina-studio";

/// The swatch of a theme that says none: its panels with a little of its accent.
pub const DEFAULT_SWATCH: &str = "color-mix(in oklab, var(--panel-background), var(--accent) 35%)";

/// A theme folder built into the app: its id and its files by path, as a folder on disk holds
/// them (`docs/spec/theme-skins.md` › Theme folders).
type BuiltInFolder = (&'static str, &'static [(&'static str, &'static [u8])]);

/// A built-in theme's folder, `crates/ui/themes/<id>/`, from the files named.
macro_rules! folder {
    ($id:literal: $($path:literal),* $(,)?) => {
        ($id, &[$(($path, include_bytes!(concat!("../../themes/", $id, "/", $path)) as &[u8])),*])
    };
}

/// The themes that come with Suhina, Suhina Studio first. Nord and Catppuccin
/// are popular open palettes (MIT, their licences in `themes/licenses/`), mapped onto Suhina's
/// tokens.
const BUILT_IN_FOLDERS: [BuiltInFolder; 10] = [
    folder!("suhina-studio": "theme.json", "theme.css", "thumbnail.svg", "variants/blue.json", "variants/darker.json", "variants/darkest.json", "variants/forest.json", "variants/light.json"),
    folder!("suhina-light": "theme.json", "theme.css", "thumbnail.svg"),
    folder!("high-contrast": "theme.json", "theme.css", "thumbnail.svg", "variants/light.json"),
    folder!("nord": "theme.json", "theme.css", "thumbnail.svg", "variants/snow-storm.json"),
    folder!("catppuccin-mocha": "theme.json", "theme.css", "thumbnail.svg", "variants/macchiato.json"),
    folder!("catppuccin-latte": "theme.json", "theme.css", "thumbnail.svg"),
    folder!("suhina-glass": "theme.json", "theme.css", "thumbnail.svg", "variants/frost.json", "variants/midnight.json"),
    folder!("suhina-sheet": "theme.json", "theme.css", "thumbnail.svg", "assets/arm.svg", "assets/button.svg", "assets/cue.svg", "assets/fader.cap.svg", "assets/fader.rail.svg", "assets/knob.body.svg", "assets/knob.pointer.svg", "assets/meter.svg", "assets/play.svg", "assets/record.svg", "assets/solo.svg", "assets/stop.svg", "assets/track.audio.svg", "assets/track.body.svg", "assets/track.midi.svg", "assets/track.name.svg"),
    folder!("suhina-meadow": "theme.json", "theme.css", "thumbnail.svg", "background.png", "assets/arm.svg", "assets/button.svg", "assets/cue.svg", "assets/fader.cap.svg", "assets/fader.rail.svg", "assets/knob.body.svg", "assets/knob.pointer.svg", "assets/meter.svg", "assets/play.svg", "assets/record.svg", "assets/solo.svg", "assets/stop.svg", "assets/track.audio.svg", "assets/track.body.svg", "assets/track.midi.svg", "assets/track.name.svg"),
    folder!("suhina-blocks": "theme.json", "theme.css", "thumbnail.svg", "assets/arm.svg", "assets/button.svg", "assets/cue.svg", "assets/fader.cap.svg", "assets/fader.rail.svg", "assets/knob.body.svg", "assets/knob.pointer.svg", "assets/meter.svg", "assets/play.svg", "assets/record.svg", "assets/solo.svg", "assets/stop.svg", "assets/track.audio.svg", "assets/track.body.color-4fc3f7.svg", "assets/track.body.color-7fdb5a.svg", "assets/track.body.color-80ada1.svg", "assets/track.body.color-a84f4f.svg", "assets/track.body.color-b38bff.svg", "assets/track.body.color-e5484d.svg", "assets/track.body.color-f2d541.svg", "assets/track.body.color-ff9a3c.svg", "assets/track.body.svg", "assets/track.midi.svg", "assets/track.name.svg"),
];

/// The themes that come with Suhina, Suhina Studio first, with their ids. They do not change;
/// editing one makes a copy.
pub fn built_in_themes() -> Vec<(&'static str, Theme)> {
    static THEMES: std::sync::OnceLock<Vec<(&'static str, Theme)>> = std::sync::OnceLock::new();
    THEMES
        .get_or_init(|| {
            BUILT_IN_FOLDERS
                .iter()
                .map(|(id, files)| {
                    let files: std::collections::BTreeMap<&str, &[u8]> = files.iter().copied().collect();
                    let read = from_files(|path, _| Ok(files.get(path).map(|bytes| bytes.to_vec())));
                    // Every built-in folder reads whole (`the_themes_that_come_with_suhina_read_whole`).
                    debug_assert!(read.as_ref().is_ok_and(|(_, warnings)| warnings.is_empty()), "{id}: {read:?}");
                    let mut theme = read.map_or_else(|_| Theme::default(), |(theme, _)| theme);
                    if *id == BUILT_IN {
                        // Suhina Studio's sheet sets only colours, which are in the palette now, so
                        // the style engine has nothing to do for the default theme and stays off.
                        theme.vars.sheet = theme.skin.css.take();
                    }
                    (*id, theme)
                })
                .collect()
        })
        .clone()
}

/// The variants that come with the built-in themes, with their ids (`nord@snow-storm`).
pub fn built_in_variants() -> Vec<(String, Theme)> {
    static VARIANTS: std::sync::OnceLock<Vec<(String, Theme)>> = std::sync::OnceLock::new();
    VARIANTS
        .get_or_init(|| {
            let themes = built_in_themes();
            let mut found = Vec::new();
            for (id, files) in BUILT_IN_FOLDERS {
                let Some((_, base)) = themes.iter().find(|(b, _)| *b == id) else { continue };
                for (path, bytes) in files {
                    let Some(stem) = path.strip_prefix("variants/").and_then(|p| p.strip_suffix(".json")) else {
                        continue;
                    };
                    let text = String::from_utf8_lossy(bytes);
                    // Every built-in variant reads whole (`the_built_in_variants_read_whole`).
                    let Ok((variant, _)) = super::theme_vars::Variant::from_json(stem, &text) else { continue };
                    let (theme, _) = super::theme_vars::with_variant(base, Some(&variant));
                    found.push((format!("{id}@{stem}"), theme));
                }
            }
            found
        })
        .clone()
}

/// The files of the built-in theme `id` by path, as its folder holds them.
pub fn built_in_files(id: &str) -> Option<Vec<(&'static str, &'static [u8])>> {
    BUILT_IN_FOLDERS.iter().find(|(b, _)| b.eq_ignore_ascii_case(id)).map(|(_, files)| files.to_vec())
}

/// A theme from its folder's files, read through `file` (a path relative to the folder and the
/// most bytes it may have; `Ok(None)` when it is not there): `theme.json`, `theme.css`, the
/// artwork and background the sheet names and the optional `thumbnail.svg`. Its variants are
/// read on their own.
pub fn from_files(
    mut file: impl FnMut(&str, u64) -> Result<Option<Vec<u8>>, String>,
) -> Result<(Theme, Vec<String>), String> {
    let text = |bytes: Vec<u8>, name: &str| String::from_utf8(bytes).map_err(|_| format!("{name}: not UTF-8 text"));
    let json = file(META, MAX_META)?.ok_or("a theme needs a theme.json")?;
    let (meta, mut warnings) = ThemeMeta::parse(&json)?;
    let css = file("theme.css", super::theme_css::MAX_CSS as u64)?.ok_or("a theme needs a theme.css")?;
    let sheet = text(css, "theme.css")?;
    let (mut theme, problems) = from_folder(meta, &sheet);
    warnings.extend(problems);
    if let Some(bytes) = file(THUMBNAIL, super::skin::MAX_SVG as u64)? {
        theme.thumbnail = Some(thumbnail(&text(bytes, THUMBNAIL)?)?);
        theme.picture = super::theme_vars::picture(&theme);
    }
    resolve_resources(&mut theme, &mut |path, max| {
        let bytes = file(path, max)?.ok_or_else(|| format!("{path}: not in the theme"))?;
        if bytes.len() as u64 > max { Err(format!("{path} exceeds its theme size limit")) } else { Ok(bytes) }
    })?;
    Ok((theme, warnings))
}

/// The largest a `theme.json` may be.
pub const MAX_META: u64 = 64 * 1024;

/// A thumbnail's source after the artwork checks and the 16:9 check.
pub fn thumbnail(source: &str) -> Result<Arc<str>, String> {
    super::skin::validate_artwork(source).map_err(|e| format!("{THUMBNAIL}: {e}"))?;
    suhina_theme_package::meta::check_thumbnail(source)?;
    Ok(Arc::from(source))
}

/// Replaces the artwork and background paths a theme's sheet names by their contents, read
/// through `resource` (a path in the theme's folder and the most bytes it may have), within the
/// package limits.
pub fn resolve_resources(
    theme: &mut Theme,
    resource: &mut impl FnMut(&str, u64) -> Result<Vec<u8>, String>,
) -> Result<(), String> {
    use super::skin;
    if let Some(background) = &theme.skin.background
        && !background.starts_with("data:image/png;base64,")
    {
        // A large picture (a 4K wallpaper) is scaled down to fit rather than refused.
        let bytes = resource(background, 64 * 1024 * 1024)?;
        let bytes = super::artwork_png::fit_background(&bytes)?;
        theme.skin.background = Some(skin::encode_png(&bytes)?);
    }
    let mut artwork_bytes = 0usize;
    for (role, source) in &mut theme.skin.assets {
        if !source.trim_start().starts_with('<') && !skin::png_artwork(source) {
            let png = skin::artwork_extension(source) == "png";
            let data = resource(source, if png { skin::MAX_PNG as u64 } else { skin::MAX_SVG as u64 })?;
            *source = if png {
                skin::encode_png(&data)?.to_string()
            } else {
                String::from_utf8(data).map_err(|e| e.to_string())?
            };
        }
        artwork_bytes = artwork_bytes
            .checked_add(source.len())
            .filter(|total| *total <= skin::MAX_ARTWORK_BYTES)
            .ok_or("combined artwork exceeds 16 MiB")?;
        skin::validate_artwork(source).map_err(|e| format!("{role}: {e}"))?;
    }
    Ok(())
}

/// A theme from its `theme.json` and its own sheet (`theme.css`), with warnings about what could
/// not be read. Artwork and the background stay as paths for the theme's files to resolve.
pub fn from_folder(meta: ThemeMeta, sheet: &str) -> (Theme, Vec<String>) {
    let mut theme = Theme { name: meta.name.clone(), meta, ..Theme::default() };
    let mut warnings = Vec::new();
    theme.skin.css = Some(sheet.into());
    match super::theme_vars::read(sheet) {
        Ok((declared, base, notices)) => {
            theme.vars.declared = std::sync::Arc::new(declared);
            theme.vars.base = std::sync::Arc::new(base);
            warnings.extend(notices);
        }
        Err(error) => warnings.push(error),
    }
    warnings.extend(super::theme_css::read_host_rules(&mut theme, sheet));
    if let Some((toolbar, offsets)) = theme.meta.toolbar.as_ref().and_then(read_toolbar) {
        theme.skin.toolbar = Some(toolbar);
        theme.skin.toolbar_offsets = offsets;
    }
    warnings.extend(super::theme_vars::resolve(&mut theme));
    (theme, warnings)
}

/// The toolbar arrangement in `theme.json` as the toolbar editor has it: `items` (names only) and
/// `hidden`, and `offsets` of items moved by hand. `None` for an arrangement with groups or
/// spaces, which this editor cannot show and leaves as it is.
fn read_toolbar(value: &Value) -> Option<(super::skin::Toolbar, std::collections::BTreeMap<String, i32>)> {
    let names = |key: &str| -> Option<Vec<String>> {
        match value.get(key) {
            None => Some(Vec::new()),
            Some(Value::Array(items)) => items.iter().map(|item| item.as_str().map(str::to_owned)).collect(),
            Some(_) => None,
        }
    };
    let known = |name: &String| super::skin::MODULES.contains(&name.as_str());
    // A module listed twice counts once, at its first place, as in a skin's toolbar.
    let unique = |names: Vec<String>| -> Vec<String> {
        let mut seen: Vec<String> = Vec::new();
        for name in names.into_iter().filter(known) {
            if !seen.contains(&name) {
                seen.push(name);
            }
        }
        seen
    };
    let order = unique(names("items")?);
    let hidden = unique(names("hidden")?);
    let offsets = value
        .get("offsets")
        .and_then(Value::as_object)
        .map(|offsets| {
            offsets
                .iter()
                .filter(|(name, _)| known(name))
                .filter_map(|(name, offset)| {
                    // The same bound as a skin's offsets and the toolbar editor.
                    let offset = i32::try_from(offset.as_i64()?).ok().filter(|n| (-16384..=16384).contains(n))?;
                    Some((name.clone(), offset))
                })
                .collect()
        })
        .unwrap_or_default();
    let mut toolbar = super::skin::Toolbar { order, hidden };
    // Items this arrangement leaves out keep their place in the default one, at the end.
    for module in super::skin::Toolbar::default().order {
        if !toolbar.order.contains(&module) {
            toolbar.order.push(module);
        }
    }
    Some((toolbar, offsets))
}

/// The theme's `theme.json` as Suhina writes it: its metadata under the theme's name, and the
/// toolbar arrangement as the toolbar editor left it (kept as written when unchanged).
pub fn file_meta(theme: &Theme) -> ThemeMeta {
    let mut meta = ThemeMeta { name: theme.name.clone(), ..theme.meta.clone() };
    let read = meta.toolbar.as_ref().and_then(read_toolbar);
    let current = theme.skin.toolbar.clone().map(|toolbar| (toolbar, theme.skin.toolbar_offsets.clone()));
    if read != current {
        meta.toolbar = current.map(|(toolbar, offsets)| {
            let mut value = serde_json::json!({ "items": toolbar.order, "hidden": toolbar.hidden });
            if !offsets.is_empty() {
                value["offsets"] = serde_json::json!(offsets);
            }
            value
        });
    }
    meta
}

/// A theme of a sheet alone, such as one written for a test or the theme editor's CSS box,
/// named "Theme" with the default swatch.
pub fn from_css(sheet: &str) -> (Theme, Vec<String>) {
    from_folder(ThemeMeta::new("Theme", DEFAULT_SWATCH), sheet)
}

/// The text of [`file_meta`].
pub fn meta_json(theme: &Theme) -> String {
    file_meta(theme).to_json()
}

/// Whether `id` names a theme that comes with Suhina.
pub fn is_built_in(id: &str) -> bool {
    BUILT_IN_FOLDERS.iter().any(|(b, _)| id.eq_ignore_ascii_case(b))
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
    /// Its `theme.json`; the name in it is [`Theme::name`].
    pub meta: ThemeMeta,
    /// `thumbnail.svg` as written, its `var()` values not yet substituted.
    pub thumbnail: Option<Arc<str>>,
    /// The thumbnail for the variant chosen, its `var()` values substituted.
    pub picture: Option<Arc<str>>,
    /// The swatch's colour for the variant chosen, `None` when it does not resolve.
    pub swatch: Option<Color32>,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            name: "Suhina Studio".into(),
            colors: Palette::DARK,
            liquid: None,
            gradients: Default::default(),
            transparent_rgb: Default::default(),
            skin: Default::default(),
            vars: Default::default(),
            meta: ThemeMeta::new("Suhina Studio", DEFAULT_SWATCH),
            thumbnail: None,
            picture: None,
            swatch: None,
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

    /// Reads a theme file. Tokens it leaves out keep Suhina Studio's colour, so a file only needs
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
                if !colors.contains_key("control.selectedBackground") && colors.contains_key("text.value") {
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
    text.push_str("\n\n## Palette token reference\n\n| Token | Suhina Studio | Used for |\n|---|---|---|\n");
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

/// The contrast ratio of `text` drawn over `fill`, 1 … 21: like [`contrast`], but translucent
/// text is first blended over the fill, so faint text counts as faint.
pub fn text_contrast(text: Color32, fill: Color32) -> f32 {
    let [r, g, b, a] = text.to_srgba_unmultiplied();
    let [fr, fg, fb, _] = fill.to_srgba_unmultiplied();
    let a = f32::from(a) / 255.0;
    let blend = |t: u8, f: u8| (f32::from(t) * a + f32::from(f) * (1.0 - a)).round() as u8;
    contrast(Color32::from_rgb(blend(r, fr), blend(g, fg), blend(b, fb)), fill)
}

fn luminance(color: Color32) -> f32 {
    let linear = |v: u8| {
        let s = f32::from(v) / 255.0;
        if s <= 0.040_45 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) }
    };
    let [r, g, b, _] = color.to_srgba_unmultiplied();
    0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b)
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
    let palette = palette();
    ctx.set_theme(scheme(&palette));
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
    // egui's own light or dark defaults under the tokens.
    *visuals = if light(p) { egui::Visuals::light() } else { egui::Visuals::dark() };
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
