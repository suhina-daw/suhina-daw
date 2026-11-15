// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Design tokens from `docs/design/brief.md` › Tokens and the sizes in
//! `docs/design/components.md`. Views use these names, never raw colours or sizes.
//!
//! Colours are the theme's variables ([`TOKENS`], held in a theme's [`Colors`]): the style
//! sheet's rules read them, and the views read the computed colours of their elements. Nothing
//! here names a colour a view paints. Sizes are fixed.

use std::cell::Cell;
use std::sync::Arc;

use egui::{Color32, FontFamily, FontId, Stroke, TextStyle};
use serde_json::{Map, Value};
pub use suhina_theme_package::meta::{FILE as META, THUMBNAIL, ThemeMeta};

/// Every colour token of a theme: its name in a theme file and what it is used for. Its custom
/// property is [`variable_name`] of the name, and Suhina Studio's value the one the core sheet
/// (`suhina_theme_package::CORE_SHEET`) registers for it ([`default_color`]).
pub const TOKENS: [(&str, &str); 37] = [
    ("grid.major", "Arrangement bar lines and lane separators"),
    ("grid.minor", "Arrangement subdivisions between bar lines"),
    ("track.headerBackground", "The neutral surface behind Arrangement track controls"),
    ("track.headerSelectedBackground", "The surface behind a selected Arrangement track's controls"),
    ("window.background", "The window behind everything: grid, empty slots, lanes, Settings"),
    ("panel.background", "Panels and headers: menu, control and status bars, track headers, beat lines"),
    ("panel.hoverBackground", "A panel or list row under the pointer"),
    ("control.background", "Fields, buttons and stopped clips"),
    ("control.hoverBackground", "A field, button or clip under the pointer"),
    ("control.activeBackground", "Selected or pressed: a selected clip or track, a dragged field"),
    ("control.playingBackground", "A playing clip or scene"),
    ("control.selectedBackground", "The chosen option of a switch, a lit toggle, the level of a volume or pan field"),
    ("border", "Hairlines between regions and rows"),
    ("border.strong", "Launcher columns and arrangement track lines"),
    ("text.primary", "Names and values"),
    ("text.secondary", "Labels and notes"),
    ("text.hint", "Inactive text, hints and section titles"),
    ("text.value", "Clip names and values on fields"),
    ("text.inverse", "Text on lit fills: a switch's chosen option, a lit button"),
    ("text.playing", "The name of a playing clip"),
    (
        "text.onColorLight",
        "Light text on dark coloured tracks, returns and clips; paired with text.onColorDark for contrast",
    ),
    (
        "text.onColorDark",
        "Dark text on bright coloured tracks, returns and clips; paired with text.onColorLight for contrast",
    ),
    ("browser.text.primary", "Browser selection, search input and highlighted names"),
    ("browser.text.secondary", "Browser item names, categories and preview labels"),
    ("browser.text.hint", "Browser section headings, search placeholder and hints"),
    ("browser.text.value", "Browser values and active preview controls"),
    ("accent", "The playhead, queued clips, a key being set"),
    (
        "automation.pin",
        "The pin of an automation lane pinned below its track's top lane (older theme files: their accent)",
    ),
    ("selection", "The selected device's outline"),
    ("recording", "Recording and errors"),
    ("recording.background", "Behind an armed track's arm button and a recording clip"),
    ("meter.normal", "A level meter below the top"),
    ("meter.hot", "A level meter near the top"),
    ("slot.empty", "The mark in an empty clip slot"),
    ("track.mutedBackground", "The fills of a muted track"),
    ("pan.background", "Behind a track header's pan field"),
    ("pan.centre", "The centre mark of a pan field"),
];

/// Whether `key` names a colour token.
pub fn is_token(key: &str) -> bool {
    TOKENS.iter().any(|(token, _)| *token == key)
}

/// Suhina Studio's value of a colour variable: its initial value in the core sheet.
pub use super::theme_vars::initial_color as default_color;

/// The value of the colour variable `variable` in the colour map `colors`, else Suhina Studio's
/// ([`default_color`]); transparent for a name that is neither.
pub fn color_in(colors: &Colors, variable: &str) -> Color32 {
    colors.get(variable).copied().or_else(|| default_color(variable)).unwrap_or(Color32::TRANSPARENT)
}

/// The name of the custom property that holds the token `key`, without its `--`: `accent` is
/// `accent`, `window.background` is `window-background`. A theme's colours are kept by this name
/// ([`Colors`]).
pub fn variable_name(key: &str) -> String {
    key.replace('.', "-")
}

/// The token a custom property named `variable` (without its `--`) holds, as a theme file names it.
/// No token's name has a `-`, so this undoes [`variable_name`].
pub fn token_key(variable: &str) -> String {
    variable.replace('-', ".")
}

/// Whether `variable` (a name without `--`) is the one name of a colour token's custom property:
/// `window-background` is, `window.background` (a dot is not part of a custom property's name) is
/// not.
pub fn is_color_variable(variable: &str) -> bool {
    let key = token_key(variable);
    is_token(&key) && variable_name(&key) == variable
}

/// The colours a theme sets: its colour variables by name without the `--`
/// (`window-background`, `accent`), the names `:root { --… }` uses. A colour the theme does not
/// set is Suhina Studio's ([`default_color`]).
pub type Colors = std::collections::BTreeMap<String, Color32>;

thread_local! {
    /// Whether the theme in use is light and whether it lets the desktop show through, as the
    /// window's computed colours said when the styles last reset (`sync_root_visuals`).
    static SCHEME: Cell<(bool, bool)> = const { Cell::new((false, false)) };
    /// The `window`'s computed background when the styles last reset.
    static WINDOW_FILL: Cell<Color32> = const { Cell::new(Color32::TRANSPARENT) };
}

/// The colour the window's own background is: the `window` element's computed
/// `background-color` (faded over a backdrop), as of the last time the styles reset. For what
/// paints outside the style tree, such as the GPU pass that clears behind the live UI.
pub fn window_fill() -> Color32 {
    WINDOW_FILL.with(Cell::get)
}

/// Whether a background of the theme in use lets the desktop show through the window: the
/// computed background of the `window` or of its panels is see-through (a theme's own, or the
/// window's opacity over a backdrop). As of the last time the styles reset.
pub fn see_through() -> bool {
    SCHEME.with(Cell::get).1
}

/// Whether the theme in use is a light one, which the system's parts of the window follow: the
/// window's computed text colour is dark. As of the last time the styles reset.
pub fn is_light() -> bool {
    SCHEME.with(Cell::get).0
}

/// `colors` as the `:root` rule that sets them (`:root { --accent: #A6A3FF; }`), `None` when it
/// sets none. A name that is not a colour variable is left out.
pub fn colors_rule(colors: &Colors) -> Option<String> {
    let set: Vec<String> = colors
        .iter()
        .filter(|(variable, _)| is_color_variable(variable))
        .map(|(variable, color)| format!(" --{variable}: {};", hex(*color)))
        .collect();
    (!set.is_empty()).then(|| format!(":root {{{} }}", set.concat()))
}

/// Draws with the colour map `colors` from the next frame on, as a theme whose colours they are
/// (`native_css::set_colors`): for what draws without the app, which gives the theme's colours
/// itself each frame: tests, and tools that paint a widget alone.
pub fn use_colors(ctx: &egui::Context, colors: &Colors) {
    super::native_css::set_colors(colors);
    ctx.request_repaint();
}

/// Where the context keeps what the root's `Visuals` were last built from: the style engine's
/// resets ([`super::native_css::style_epoch`]).
fn visuals_key() -> egui::Id {
    egui::Id::NULL.with("suhina-root-visuals")
}

/// Builds egui's `Visuals` for the whole window from the sheet: the computed styles of `widget`
/// and its states ([`sheet_visuals`]), with what the rules for egui's widgets say over them
/// ([`super::style_class::root_visuals`]). Run at the start of a frame, it works only when the
/// engine's styles were reset since the last time (a theme, variant, `user.css`, width `@media`,
/// contrast, a colour drag or the window's opacity), not on every frame; the change shows in every
/// `Ui` made after it, and in the root's once it takes them ([`follow_root_visuals`]). Scoped
/// styles (`style_class::ui_for`) apply over these. It also tells egui whether the theme is light
/// or dark, which egui passes on to the native window (its title bar).
pub(super) fn sync_root_visuals(ctx: &egui::Context) {
    let source = super::native_css::style_epoch();
    let key = visuals_key();
    if ctx.data(|data| data.get_temp::<u64>(key)) == Some(source) {
        return;
    }
    ctx.data_mut(|data| data.insert_temp(key, source));
    let Some((fallback, light, clear)) = sheet_visuals() else { return };
    SCHEME.with(|scheme| scheme.set((light, clear)));
    let built = super::style_class::root_visuals(fallback);
    if ctx.global_style().visuals != built {
        ctx.all_styles_mut(|style| style.visuals = built.clone());
    }
    let scheme = if light { egui::Theme::Light } else { egui::Theme::Dark };
    if ctx.theme() != scheme {
        ctx.set_theme(scheme);
    }
}

/// Gives `ui`, the window's root, the `Visuals` the frame's setup built, when they changed after it
/// was made: a `Ui` keeps the style it was created with.
pub fn follow_root_visuals(ui: &mut egui::Ui) {
    let global = ui.ctx().global_style();
    if !std::sync::Arc::ptr_eq(ui.style(), &global) && ui.style().visuals != global.visuals {
        ui.style_mut().visuals = global.visuals.clone();
    }
}

/// egui's `Visuals` as the palette gave them before the sheet did, with the colours `color` gives
/// by variable name: for the tests that check the sheet's `widget` gives the same, what the root
/// has with no rule of a theme for egui's widgets.
#[cfg(test)]
pub(super) fn palette_visuals_for(color: impl Fn(&str) -> Color32) -> egui::Visuals {
    let light = luminance(color("text-primary")) < 0.18;
    let mut visuals = shaped(if light { egui::Visuals::light() } else { egui::Visuals::dark() });
    visuals.panel_fill = color("panel-background");
    visuals.window_fill = color("panel-background");
    visuals.extreme_bg_color = color("window-background");
    visuals.faint_bg_color = color("panel-background");
    visuals.window_stroke = hairline(color("border"));
    visuals.selection.bg_fill = color("control-activeBackground");
    visuals.selection.stroke = hairline(color("accent"));
    for (widget, fill) in [
        (&mut visuals.widgets.noninteractive, "panel-background"),
        (&mut visuals.widgets.inactive, "control-background"),
        (&mut visuals.widgets.hovered, "control-hoverBackground"),
        (&mut visuals.widgets.active, "control-activeBackground"),
        (&mut visuals.widgets.open, "control-activeBackground"),
    ] {
        widget.bg_fill = color(fill);
        widget.weak_bg_fill = color(fill);
    }
    visuals.widgets.noninteractive.bg_stroke = hairline(color("border"));
    visuals.widgets.noninteractive.fg_stroke = hairline(color("text-secondary"));
    visuals.widgets.inactive.fg_stroke = hairline(color("text-value"));
    visuals.widgets.hovered.fg_stroke = hairline(color("text-primary"));
    visuals.widgets.active.fg_stroke = hairline(color("text-primary"));
    visuals.widgets.open.fg_stroke = hairline(color("text-primary"));
    visuals
}

/// Suhina Studio's value of the colour variable `variable`, for the tests.
#[cfg(test)]
pub(crate) fn studio(variable: &str) -> Color32 {
    default_color(variable).unwrap_or_else(|| panic!("no core colour {variable}"))
}

/// The opaque colour `0xRRGGBB`, for the tests.
#[cfg(test)]
pub(crate) const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

/// The colour variable `name` (with its `--`) as the root resolves it on `ctx` with the colours
/// set, after a frame: for the tests that check what a theme's variables come to.
#[cfg(test)]
pub(crate) fn root_in(ctx: &egui::Context, name: &str) -> Color32 {
    in_style_frame(ctx, |_| super::native_css::root_color(name))
        .unwrap_or_else(|| panic!("the root resolves no colour {name}"))
}

/// What `read` gives in a frame of the style engine on `ctx`, drawn with the colours set (a theme's
/// colour map applies from the frame after it is set, so a frame runs first): for the tests that
/// read what elements say.
#[cfg(test)]
pub(crate) fn in_style_frame<R>(ctx: &egui::Context, read: impl Fn(&mut egui::Ui) -> R) -> R {
    let mut out = None;
    for _ in 0..2 {
        let mut output = ctx.run_ui(Default::default(), |ui| {
            let _frame = super::native_css::frame_with_motion(ui, false);
            out = Some(read(ui));
        });
        output.textures_delta.clear();
    }
    out.unwrap_or_else(|| panic!("the frame ran"))
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
    folder!("suhina-sheet": "theme.json", "theme.css", "thumbnail.svg", "assets/folder.svg", "assets/arm.svg", "assets/button.svg", "assets/cue.svg", "assets/fader.cap.svg", "assets/fader.rail.svg", "assets/knob.body.svg", "assets/knob.pointer.svg", "assets/meter.svg", "assets/play.svg", "assets/record.svg", "assets/solo.svg", "assets/stop.svg", "assets/track.audio.svg", "assets/track.body.svg", "assets/track.midi.svg", "assets/track.name.svg"),
    folder!("suhina-meadow": "theme.json", "theme.css", "thumbnail.svg", "background.png", "assets/folder.svg", "assets/arm.svg", "assets/button.svg", "assets/cue.svg", "assets/fader.cap.svg", "assets/fader.rail.svg", "assets/knob.body.svg", "assets/knob.pointer.svg", "assets/meter.svg", "assets/play.svg", "assets/record.svg", "assets/solo.svg", "assets/stop.svg", "assets/track.audio.svg", "assets/track.body.svg", "assets/track.midi.svg", "assets/track.name.svg"),
    folder!("suhina-blocks": "theme.json", "theme.css", "thumbnail.svg", "assets/folder.svg", "assets/arm.svg", "assets/button.svg", "assets/cue.svg", "assets/fader.cap.svg", "assets/fader.rail.svg", "assets/knob.body.svg", "assets/knob.pointer.svg", "assets/meter.svg", "assets/play.svg", "assets/record.svg", "assets/solo.svg", "assets/stop.svg", "assets/track.audio.svg", "assets/track.body.svg", "assets/track.midi.svg", "assets/track.name.svg"),
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
                    let theme = read.map_or_else(|_| Theme::default(), |(theme, _)| theme);
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
    theme.skin.base = theme.meta.base;
    match super::theme_vars::read(sheet) {
        Ok((declared, base, notices)) => {
            theme.vars.declared = std::sync::Arc::new(declared);
            theme.vars.base = std::sync::Arc::new(base);
            warnings.extend(notices);
        }
        Err(error) => warnings.push(error),
    }
    warnings.extend(theme.vars.base.keys().filter_map(|name| former_kind_token(name.strip_prefix("--kind-")?)));
    warnings.extend(former_track_colours_notice(theme.vars.base.keys().map(String::as_str)));
    warnings.extend(former_host_variables_notice(sheet));
    warnings.extend(super::theme_css::read_host_rules(&mut theme, sheet));
    warnings.extend(former_colour_artwork_notice(theme.skin.assets.keys().cloned()));
    if let Some((toolbar, offsets)) = theme.meta.toolbar.as_ref().and_then(read_toolbar) {
        theme.skin.toolbar = Some(toolbar);
        theme.skin.toolbar_offsets = offsets;
    }
    warnings.extend(super::theme_vars::resolve(&mut theme));
    (theme, warnings)
}

/// The notice for a sample-map colour a theme of an earlier version gives as a palette token
/// (`--kind-kick`, `"kind.kick"`) for the sound kind `kind`: the `kind.*` tokens are gone, and a
/// theme colours a kind with a rule for its `sample-point` (#1019). `None` for a name that is not
/// a kind.
pub fn former_kind_token(kind: &str) -> Option<String> {
    suhina_soundmap::kinds::Kind::ALL.iter().find(|k| k.id() == kind)?;
    Some(format!(
        "--kind-{kind} is no longer read: the sample map's colours are rules, \
         sample-point[kind=\"{kind}\"] {{ color: … }}"
    ))
}

/// Whether `name` (without its `--`) is a track colour a theme of an earlier version set:
/// `track-1` to `track-8`, `clip-midiDefault`, `clip-audioDefault`. Tracks and clips keep their
/// own colours, and the colour picker and new tracks take Suhina's, so a theme no longer sets
/// them (#1020).
pub fn is_former_track_colour(name: &str) -> bool {
    matches!(name, "clip-midiDefault" | "clip-audioDefault")
        || name.strip_prefix("track-").is_some_and(|n| matches!(n, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8"))
}

/// One notice naming the former track colours among a theme's `:root` variables `names`, if any.
pub fn former_track_colours_notice<'a>(names: impl IntoIterator<Item = &'a str>) -> Option<String> {
    let former: Vec<_> =
        names.into_iter().filter(|name| name.strip_prefix("--").is_some_and(is_former_track_colour)).collect();
    (!former.is_empty()).then(|| {
        format!(
            "{} no longer read: a track's or clip's colour is its own, and a theme draws it from \
             the element's color attribute, attr(color type(<color>))",
            former.join(", ")
        )
    })
}

/// The notice for a sheet that reads a variable the app set on tracks and clips in an earlier
/// version (`--track-colour`, `--track-fill`, `--clip-colour`): the app sets none now, so a rule
/// that reads one falls back without it.
pub fn former_host_variables_notice(sheet: &str) -> Option<String> {
    let used: Vec<_> =
        ["--track-colour", "--track-fill", "--clip-colour"].into_iter().filter(|name| sheet.contains(name)).collect();
    (!used.is_empty()).then(|| {
        format!(
            "{} no longer set: a track's or clip's colour is the element's color attribute, \
             attr(color type(<color>))",
            used.join(", ")
        )
    })
}

/// The notice for artwork chosen by an exact track colour (`track.body.color-ff9a3c`), which no
/// track picks any more: artwork takes a track's colour as `currentColor`.
fn former_colour_artwork_notice(roles: impl IntoIterator<Item = String>) -> Option<String> {
    let colour = |role: &str| {
        role.split('.').any(|part| {
            part.strip_prefix("color-").is_some_and(|hex| hex.len() == 6 && hex.bytes().all(|b| b.is_ascii_hexdigit()))
        })
    };
    let used: Vec<_> = roles.into_iter().filter(|role| colour(role)).collect();
    (!used.is_empty()).then(|| {
        format!(
            "{} not used: artwork is not chosen by a track's colour; it takes the colour as currentColor",
            used.join(", ")
        )
    })
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

/// The colours an older theme file may leave out that [`Theme::from_json`] copies from another,
/// each with the one it is copied from.
const COPIED: [(&str, &str); 7] = [
    ("browser.text.primary", "text.primary"),
    ("browser.text.secondary", "text.secondary"),
    ("browser.text.hint", "text.hint"),
    ("browser.text.value", "text.value"),
    ("text.onColorLight", "text.primary"),
    ("text.onColorDark", "window.background"),
    ("automation.pin", "accent"),
];

/// Every colour an older theme file may leave out, which [`Theme::from_json`] works out from
/// others: [`COPIED`]'s, and two it works out from a background and a text colour.
fn derived() -> impl Iterator<Item = &'static str> {
    ["text.inverse", "control.selectedBackground"].into_iter().chain(COPIED.iter().map(|(key, _)| *key))
}

/// A named set of colours, kept in a theme file:
///
/// ```json
/// { "name": "Night", "colors": { "window.background": "#1D1C1A", "accent": "#A6A3FF" } }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    pub skin: super::skin::Skin,
    /// The rgb of each colour in [`Theme::colors`] that is fully transparent, by the same names.
    pub transparent_rgb: std::collections::BTreeMap<String, [u8; 3]>,
    pub name: String,
    /// The colours the theme sets; any other is Suhina Studio's ([`Theme::color`]).
    pub colors: Colors,
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
            colors: Colors::new(),
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

/// The sheet a new theme starts with: nothing but a comment, so the user-agent sheet styles every
/// element until the theme says otherwise.
pub const NEW_THEME_SHEET: &str = "/* A new theme. Suhina's user-agent sheet styles every element this sheet does not.\n   \
     Offer quick controls with @property and a label, for example:\n   \
     @property --accent-glow { syntax: \"<color>\"; initial-value: #A6A3FF; label: \"Glow\"; } */\n";

impl Theme {
    /// A new theme named `name`, of its own: the starting sheet ([`NEW_THEME_SHEET`]) and
    /// metadata, and nothing else.
    pub fn new_blank(name: &str) -> Self {
        let mut theme = Self { name: name.into(), meta: ThemeMeta::new(name, DEFAULT_SWATCH), ..Self::default() };
        theme.skin.css = Some(Arc::from(NEW_THEME_SHEET));
        theme
    }

    /// The colour variable `variable` (a name without `--`) as the theme sets it, else Suhina
    /// Studio's ([`color_in`]).
    pub fn color(&self, variable: &str) -> Color32 {
        color_in(&self.colors, variable)
    }

    /// `color`, the value of the colour variable `variable` (a name without `--`), as four bytes:
    /// a fully transparent colour keeps the rgb it was written with.
    pub fn rgba(&self, variable: &str, color: Color32) -> [u8; 4] {
        let mut rgba = color.to_srgba_unmultiplied();
        if color.a() == 0
            && let Some(rgb) = self.transparent_rgb.get(variable)
        {
            rgba[..3].copy_from_slice(rgb);
        }
        rgba
    }

    /// Sets the colour variable `variable` (a name without `--`) to `rgba`.
    pub fn set_color(&mut self, variable: &str, [r, g, b, a]: [u8; 4]) {
        self.colors.insert(variable.to_owned(), Color32::from_rgba_unmultiplied(r, g, b, a));
        self.transparent_rgb.remove(variable);
        if a == 0 && [r, g, b] != [0, 0, 0] {
            self.transparent_rgb.insert(variable.to_owned(), [r, g, b]);
        }
    }

    /// Sets the colour variable `variable` to `rgba` as read from a file or a sheet: a colour that
    /// is Suhina Studio's own is not one the theme sets, so it is left out of the map.
    pub fn read_color(&mut self, variable: &str, [r, g, b, a]: [u8; 4]) {
        let color = Color32::from_rgba_unmultiplied(r, g, b, a);
        if a != 0 && default_color(variable) == Some(color) {
            self.reset_color(variable);
        } else {
            self.set_color(variable, [r, g, b, a]);
        }
    }

    /// Sets the token `key` to `color`, a colour worked out from the theme's others. One that is
    /// Suhina Studio's own is not kept, so the theme sets only what differs.
    fn derive(&mut self, key: &str, color: Color32, transparent_rgb: Option<[u8; 3]>) {
        let variable = variable_name(key);
        let same = default_color(&variable) == Some(color) && transparent_rgb.is_none();
        self.reset_color(&variable);
        if !same {
            self.colors.insert(variable.clone(), color);
            if let Some(rgb) = transparent_rgb.filter(|_| color.a() == 0) {
                self.transparent_rgb.insert(variable, rgb);
            }
        }
    }

    /// Makes the colour variable `variable` Suhina Studio's again: the theme no longer sets it.
    pub fn reset_color(&mut self, variable: &str) {
        self.colors.remove(variable);
        self.transparent_rgb.remove(variable);
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
                    // A track colour an earlier version kept in every theme file: tracks keep their
                    // own now, so it is left behind without a word, and the file still converts.
                    if is_former_track_colour(&key.replace('.', "-")) {
                        continue;
                    }
                    match (is_token(key), value.as_str().and_then(parse_rgba)) {
                        (true, Some(rgba)) => theme.read_color(&variable_name(key), rgba),
                        (false, _) => warnings.push(
                            key.strip_prefix("kind.")
                                .and_then(former_kind_token)
                                .unwrap_or_else(|| format!("{key} is not a colour token")),
                        ),
                        (true, None) => warnings.push(format!("{key}: {value} is not a #RRGGBB or #RRGGBBAA colour")),
                    }
                }
                // A colour an older file leaves out is worked out from the ones it sets, so the
                // file keeps looking as it did until the colour is edited. It counts as set.
                let has = |key: &str| colors.contains_key(key) && is_token(key);
                let rgb_of = |theme: &Theme, key: &str, color: Color32| -> [u8; 3] {
                    let [r, g, b, _] = color.to_srgba_unmultiplied();
                    theme.transparent_rgb.get(&variable_name(key)).copied().unwrap_or([r, g, b])
                };
                // Older files drew these with a background and a text colour.
                if !has("text.inverse") {
                    let [r, g, b] = rgb_of(&theme, "window.background", theme.color("window-background"));
                    theme.derive("text.inverse", Color32::from_rgb(r, g, b), None);
                }
                if !has("control.selectedBackground") && has("text.value") {
                    let value = theme.color("text-value");
                    let rgb = theme.transparent_rgb.get("text-value").copied();
                    theme.derive("control.selectedBackground", value, rgb);
                }
                // Keep old themes visually identical until these independent roles are edited.
                for (key, source) in COPIED {
                    if !has(key) {
                        let value = theme.color(&variable_name(source));
                        let rgb = theme.transparent_rgb.get(&variable_name(source)).copied();
                        theme.derive(key, value, rgb);
                    }
                }
                // Text never takes a see-through background's alpha.
                if !has("text.onColorDark") {
                    let [r, g, b] = rgb_of(&theme, "text.onColorDark", theme.color("text-onColorDark"));
                    theme.derive("text.onColorDark", Color32::from_rgb(r, g, b), None);
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

    /// The theme as a file: the colours it sets, and nothing for a colour it leaves to Suhina Studio.
    /// A file that leaves out a colour [`Theme::from_json`] works out from others (older files'
    /// text roles) gets that colour written when working it out would change it, so reading the
    /// file gives the same theme back.
    pub fn to_json(&self) -> String {
        let mut colors: Map<String, Value> = self
            .colors
            .iter()
            .map(|(variable, color)| (token_key(variable), Value::String(hex_rgba(self.rgba(variable, *color)))))
            .collect();
        let read = |colors: &Map<String, Value>| {
            let text = serde_json::json!({ "colors": colors }).to_string();
            Self::from_json(&text).0
        };
        let reread = read(&colors);
        for key in derived() {
            let variable = variable_name(key);
            let (mine, theirs) = (self.color(&variable), reread.color(&variable));
            let (mine, theirs) = (self.rgba(&variable, mine), reread.rgba(&variable, theirs));
            if mine != theirs && !colors.contains_key(key) {
                colors.insert(key.to_string(), Value::String(hex_rgba(mine)));
            }
        }
        let mut root = Map::new();
        if self.skin != super::skin::Skin::default() {
            root.insert("skin".into(), self.skin.to_json());
        }
        root.insert("name".into(), Value::String(self.name.clone()));
        if !colors.is_empty() {
            root.insert("colors".into(), Value::Object(colors));
        }
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

/// The README shipped with themes: the format, capabilities and generated palette reference.
pub fn template_readme() -> String {
    let mut text = String::from(include_str!("../../themes/THEME-PACKAGE-README.md"));
    text.push_str("\n\n## Colour token reference\n\n| Token | Suhina Studio | Used for |\n|---|---|---|\n");
    for (key, purpose) in TOKENS {
        let color = color_in(&Colors::new(), &variable_name(key));
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

/// Text drawn on `fill`: the light or the dark ink for text on a colour, whichever stands out more
/// from it, so a name stays readable on any track colour of any theme. The two inks are the
/// `color`s of `label[kind="on-light"]` and `label[kind="on-dark"]` where the text is drawn.
pub fn text_on(fill: Color32) -> Color32 {
    let (light, dark) = on_colour_inks();
    if contrast(fill, light) >= contrast(fill, dark) { light } else { dark }
}

thread_local! {
    /// The inks [`text_on`] read last, with the style engine's frame and the custom properties in
    /// scope they were read with (held, so the map is not freed and its address taken by another).
    #[allow(clippy::type_complexity)]
    static ON_COLOUR: std::cell::RefCell<Option<(u64, Arc<std::collections::BTreeMap<String, String>>, (Color32, Color32))>> =
        const { std::cell::RefCell::new(None) };
}

/// The light and the dark ink for text on a colour where the text is drawn: the `color`s of
/// `label[kind="on-light"]` and `label[kind="on-dark"]`. Looked up again only when the custom
/// properties in scope changed since the last time in the same frame, so a view that names many
/// clips adds two elements to the style tree, not two a clip; each frame looks them up afresh, so
/// every frame's tree holds the same elements.
fn on_colour_inks() -> (Color32, Color32) {
    use super::element::LABEL;
    let epoch = super::native_css::frame_number();
    let variables = super::style_class::with_inherited(|style| style.map(|style| style.shared_variables()))
        .unwrap_or_else(super::native_css::root_variables);
    let kept = ON_COLOUR.with(|kept| {
        kept.borrow()
            .as_ref()
            .filter(|(at, held, _)| *at == epoch && Arc::ptr_eq(held, &variables))
            .map(|(_, _, inks)| *inks)
    });
    kept.unwrap_or_else(|| {
        let inks =
            (LABEL.attribute("kind", "on-light").look().color(), LABEL.attribute("kind", "on-dark").look().color());
        ON_COLOUR.with(|kept| *kept.borrow_mut() = Some((epoch, variables, inks)));
        inks
    })
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
    FontId::new(super::style_class::inherited_font_size(size), FontFamily::Proportional)
}

/// Every number, time and position is monospaced.
pub fn mono_font(size: f32) -> FontId {
    FontId::new(super::style_class::inherited_font_size(size), FontFamily::Monospace)
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
        style.visuals = shaped(egui::Visuals::dark());
    });
    // egui's own visuals are in place: the next frame builds the sheet's over them.
    ctx.data_mut(|data| data.remove::<u64>(visuals_key()));
}

/// `visuals` in Suhina's shape: square corners, no shadows, no outline on a widget at rest, and a
/// text field that shows what is behind it.
fn shaped(mut visuals: egui::Visuals) -> egui::Visuals {
    visuals.text_edit_bg_color = Some(Color32::TRANSPARENT);
    visuals.window_corner_radius = egui::CornerRadius::ZERO;
    visuals.menu_corner_radius = egui::CornerRadius::ZERO;
    visuals.window_shadow = egui::epaint::Shadow::NONE;
    visuals.popup_shadow = egui::epaint::Shadow::NONE;
    visuals.override_text_color = None;
    for widget in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget.corner_radius = egui::CornerRadius::ZERO;
        widget.bg_stroke = Stroke::NONE;
        widget.expansion = 0.0;
    }
    visuals
}

/// egui's visuals from the sheet, where no rule of a theme for egui's widgets says otherwise: the
/// computed styles of `widget` (at rest, `:hover`, `:active` and `:disabled`; `[checked]` the
/// selection; `[kind="window"]` the frames of windows and menus and the panels; `[kind="field"]`
/// behind a text field), each looked up inside the `window`, so a backdrop's fading reaches them.
/// Light or dark as the theme is: whether the window's text (its computed `color`) is dark. A
/// background can be see-through, so its colour alone does not tell; text is always made to read
/// on what the theme looks like. See-through as the window's or its panels' computed background
/// is. `None` where no sheet styles the window.
fn sheet_visuals() -> Option<(egui::Visuals, bool, bool)> {
    use super::element::{Checked, Look, WIDGET, WINDOW};
    let window_look = WINDOW.probe();
    let root = window_look.style()?;
    let light = luminance(root.color()) < 0.18;
    let window = WINDOW.enter();
    let look = |builder: super::element::Builder<'static>| builder.probe();
    let rest = look(WIDGET.builder());
    let hovered = look(WIDGET.pointer(true, false, false));
    let pressed = look(WIDGET.pointer(true, true, false));
    let disabled = look(WIDGET.pointer(false, false, true));
    let chosen = look(WIDGET.attr(Checked, true));
    let frame = look(WIDGET.attribute("kind", "window"));
    let field = look(WIDGET.attribute("kind", "field"));
    let node = window.node();
    drop(window);
    super::native_css::discard(node);
    let mut visuals = shaped(if light { egui::Visuals::light() } else { egui::Visuals::dark() });
    visuals.panel_fill = frame.background();
    visuals.window_fill = frame.background();
    visuals.faint_bg_color = frame.background();
    visuals.window_stroke = hairline(frame.border());
    visuals.extreme_bg_color = field.background();
    visuals.selection.bg_fill = chosen.background();
    visuals.selection.stroke = hairline(chosen.accent());
    for (widget, own) in [
        (&mut visuals.widgets.noninteractive, &disabled),
        (&mut visuals.widgets.inactive, &rest),
        (&mut visuals.widgets.hovered, &hovered),
        (&mut visuals.widgets.active, &pressed),
        (&mut visuals.widgets.open, &pressed),
    ] {
        widget.bg_fill = Look::background(own);
        widget.weak_bg_fill = Look::background(own);
        widget.fg_stroke = hairline(Look::color(own));
    }
    visuals.widgets.noninteractive.bg_stroke = hairline(disabled.border());
    let clear = root.background().a() < u8::MAX || frame.background().a() < u8::MAX;
    WINDOW_FILL.with(|fill| fill.set(root.background()));
    Some((visuals, light, clear))
}

#[cfg(test)]
#[path = "theme_tests.rs"]
mod tests;
