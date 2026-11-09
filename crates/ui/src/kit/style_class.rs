// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Stable native CSS classes. A scoped palette lets existing painter code inherit styles.
use super::theme;
use std::cell::RefCell;

thread_local! {
    /// The elements in scope, outermost first: interned ids, so entering one allocates nothing.
    static STACK: RefCell<Vec<super::element::Name>> = const { RefCell::new(Vec::new()) };
}

pub struct Scope {
    palette: theme::Palette,
    revision: u64,
    /// What the elements in scope inherited before this one was entered.
    inherited: Option<super::native_css::Computed>,
    /// The element's node in this frame's style tree, when the engine collected it.
    pub(super) node: Option<usize>,
}

thread_local! {
    /// The style of the innermost element that has one, for the properties CSS inherits: the font
    /// size [`theme::ui_font`] and the text shadows [`super::native_css::paint_text`] draw with.
    static INHERITED: RefCell<Option<super::native_css::Computed>> = const { RefCell::new(None) };
}

/// The `font-size` the text drawn now inherits, or `fallback` where no sheet sets one.
pub(super) fn inherited_font_size(fallback: f32) -> f32 {
    INHERITED.with(|inherited| {
        inherited.borrow().as_ref().filter(|style| style.authored.font_size).map_or(fallback, |style| style.font_size)
    })
}

/// Calls `paint` with the style the text drawn now inherits its `text-shadow` from, if any.
pub(super) fn with_inherited<R>(paint: impl FnOnce(Option<&super::native_css::Computed>) -> R) -> R {
    INHERITED.with(|inherited| paint(inherited.borrow().as_ref()))
}

impl Scope {
    /// This element's own style: the one resolved for its node, never an enclosing element's.
    pub(super) fn style(&self) -> Option<super::native_css::Computed> {
        super::native_css::style_of(self.node)
    }
}
impl Drop for Scope {
    fn drop(&mut self) {
        if theme::palette_revision() == self.revision {
            theme::local_palette(self.palette);
        }
        INHERITED.with(|inherited| *inherited.borrow_mut() = self.inherited.take());
        if self.node.is_some() {
            super::native_css::leave();
        }
        STACK.with(|s| {
            s.borrow_mut().pop();
        });
    }
}

/// Puts the element `class` in the style tree and the palette scope. `prepare` gives its node what
/// the app knows about it (its attributes, its variables) before its style is looked up.
pub(super) fn enter_inner(
    class: super::element::Name,
    name: Option<&str>,
    response: Option<&egui::Response>,
    prepare: impl FnOnce(usize),
) -> Scope {
    let node = super::native_css::enter(class, name, response);
    if let Some(node) = node {
        prepare(node);
    }
    let saved = theme::palette();
    STACK.with(|s| s.borrow_mut().push(class));
    let mut palette = saved;
    let own = super::native_css::style_of(node);
    if let Some(values) = &own {
        if values.authored.color {
            set_foreground(&mut palette, values.color, Some(class));
        }
        if values.authored.border {
            palette.border = values.border;
            palette.border_strong = values.border;
        }
        if values.authored.background {
            set_background(&mut palette, values.background, class);
        }
        // `accent-color` reaches every control drawn in the scope (a knob's arc, a lit button),
        // the parameter list of a built-in effect too.
        if values.authored.accent {
            palette.accent = values.accent;
        }
    }
    theme::local_palette(palette);
    // An element without a style of its own (not collected) inherits what its parent did.
    let inherited = INHERITED.with(|inherited| {
        let mut inherited = inherited.borrow_mut();
        match own {
            Some(own) => inherited.replace(own),
            None => inherited.clone(),
        }
    });
    Scope { palette: saved, revision: theme::palette_revision(), inherited, node }
}

/// A background for `class`: a view's sets the surfaces inside it, and a control's its own fill
/// too. So a view's background never repaints the buttons drawn on it.
fn set_background(palette: &mut theme::Palette, color: egui::Color32, class: super::element::Name) {
    palette.window_background = color;
    palette.panel_background = color;
    if !class.is_surface() {
        palette.control_background = color;
    }
}

/// Keeps the text of the element whose style is `style` readable on `background`: when the
/// colour it inherited (a bar's light text in a menu that opens over a light window) has too
/// little contrast, its text takes the ink that reads on `background`. Lasts until the element's
/// scope ends. Returns that ink when it changed the text, for widgets styled from the inherited
/// colour.
pub(super) fn readable_on(
    style: Option<super::native_css::Computed>,
    background: egui::Color32,
) -> Option<egui::Color32> {
    let inherited = style.filter(|style| style.authored.color).map_or_else(theme::text_primary, |s| s.color);
    if theme::contrast(inherited, background) >= 3.0 {
        return None;
    }
    // Near black or near white, whichever reads on it: the palette's own text colours are the
    // inherited ones by now.
    let (dark, light) = (egui::Color32::from_rgb(0x1a, 0x1a, 0x1a), egui::Color32::from_rgb(0xf4, 0xf4, 0xf4));
    let ink = if theme::contrast(dark, background) >= theme::contrast(light, background) { dark } else { light };
    let mut palette = theme::palette();
    set_foreground(&mut palette, ink, None);
    theme::local_palette(palette);
    Some(ink)
}

fn set_foreground(palette: &mut theme::Palette, color: egui::Color32, class: Option<super::element::Name>) {
    palette.text_primary = color;
    palette.text_inverse = color;
    palette.text_secondary = color;
    palette.text_value = color;
    palette.text_hint = color;
    palette.browser_text_primary = color;
    palette.browser_text_secondary = color;
    palette.browser_text_value = color;
    palette.text_on_color_dark = color;
    palette.text_on_color_light = color;
    if class == Some(super::element::METER) {
        palette.meter_normal = color;
    }
}
use super::element::{ARRANGEMENT, DEVICE, DEVICE_CONTROL, Entered};
use suhina_style::property::Property;

/// Whether `el`, a value control, is drawn as a knob: the sheet's `appearance: knob` or `field`
/// on it or an element it is in (`mixer pan { appearance: knob; }`), else the user's preference.
pub fn knob_preference(el: &Entered, fallback: bool) -> bool {
    match el.style().and_then(|style| style.keyword(Property::Appearance)) {
        Some("knob") => true,
        Some("field") => false,
        _ => fallback,
    }
}

/// Whether the sheet draws a track's artwork behind `el` (`track-column { track-artwork: on; }`).
pub fn track_artwork(el: &Entered) -> bool {
    el.style().and_then(|style| style.keyword(Property::TrackArtwork)) == Some("on")
}

/// The Arrangement view's settings as the theme gives them (`arrangement { track-height: 96px; }`).
fn arrangement() -> Option<super::native_css::Computed> {
    super::native_css::settings_of(&[ARRANGEMENT])
}

/// The side the theme puts the Arrangement view's track headers on (`arrangement {
/// track-headers: left }`), when it says one: `Some(true)` for the left.
pub fn theme_track_headers_left() -> Option<bool> {
    match arrangement()?.keyword(Property::TrackHeaders)? {
        "left" => Some(true),
        "right" => Some(false),
        _ => None,
    }
}

/// The least and most a theme's row height for tracks may be, in points.
const MIN_TRACK_HEIGHT: f32 = 20.0;
const MAX_TRACK_HEIGHT: f32 = 400.0;

/// The height a theme gives an Arrangement track that has none of its own (`arrangement {
/// track-height: 96px }`), in place of Settings › Arrangement's default size.
pub fn theme_track_height() -> Option<f32> {
    arrangement()?.points(Property::TrackHeight).filter(|v| (MIN_TRACK_HEIGHT..=MAX_TRACK_HEIGHT).contains(v))
}

/// The narrowest the theme lets the Arrangement view's header column be (`arrangement {
/// header-min-width: 330px }`): the width its placements were made for, which the header's
/// narrower layouts would break. From 160 to 520 points.
pub fn theme_header_min_width() -> Option<f32> {
    arrangement()?.points(Property::HeaderMinWidth).filter(|v| (160.0..=520.0).contains(v))
}

/// How strongly the theme lets the track's colour tint its Arrangement header (`arrangement {
/// header-tint: 0 }` for none, up to 100), over the user's header colour strength; a see-through
/// theme takes the native tint away with 0.
pub fn theme_header_tint() -> Option<u8> {
    arrangement()?.integer(Property::HeaderTint).and_then(|v| u8::try_from(v).ok()).filter(|v| *v <= 100)
}

/// The height under which an Arrangement header keeps Suhina's own layout instead of the theme's
/// placements (`arrangement { compact-below: 48px }`), so a low track does not overlap.
pub fn theme_compact_below() -> Option<f32> {
    arrangement()?.points(Property::CompactBelow).filter(|v| (0.0..=200.0).contains(v))
}

/// How a device panel's generated parameter list is laid out: the columns of the device's grid
/// (`device { grid-template-columns: repeat(3, 1fr); }`, at most 8) side by side, each cell as
/// tall as `device-control`'s `height` (16 to 160 points).
pub fn theme_param_grid() -> (usize, Option<f32>) {
    use suhina_style::value::Value;
    let columns = match super::native_css::settings_of(&[DEVICE])
        .and_then(|style| style.authored_value(Property::GridTemplateColumns))
    {
        Some(Value::TrackList(tracks)) => tracks.len().clamp(1, 8),
        _ => 1,
    };
    let row = super::native_css::settings_of(&[DEVICE, DEVICE_CONTROL])
        .and_then(|style| style.height)
        .filter(|v| (16.0..=160.0).contains(v));
    (columns, row)
}

/// Where a device panel's generated parameters show their names (`device { param-label: top }`):
/// `left`, `top` or `hidden`, over what each device says.
pub fn theme_param_label() -> Option<&'static str> {
    super::native_css::settings_of(&[DEVICE])?.keyword(Property::ParamLabel)
}

/// Whether an element `name` encloses what is drawn now (`pan` around a pan's knob).
pub fn within(name: super::element::Name) -> bool {
    STACK.with(|stack| stack.borrow().contains(&name))
}
/// The view being drawn, for artwork of one view: `mixer` (a strip, in any view), `session` or
/// `arrangement`.
pub fn view() -> Option<&'static str> {
    STACK.with(|stack| {
        let stack = stack.borrow();
        use super::element::{ARRANGEMENT, MIXER, SESSION};
        [MIXER, SESSION, ARRANGEMENT].into_iter().find(|view| stack.contains(view)).map(super::element::Name::name)
    })
}

/// Applies authored values to ordinary egui widgets as well as native painters.
/// The guard reborrows the same Ui: widget ids and layout are unchanged, and early
/// returns restore the caller's style without unsafe pointers or a new child Ui.
pub struct UiStyle<'a> {
    ui: &'a mut egui::Ui,
    previous: Option<std::sync::Arc<egui::Style>>,
    previous_clip: Option<egui::Rect>,
}
impl UiStyle<'_> {
    /// Bound both native painting and interaction to CSS geometry, restoring the
    /// caller's clipping on return without creating a different widget identity.
    pub fn clipped_to(mut self, rect: egui::Rect) -> Self {
        let previous = self.ui.clip_rect();
        self.previous_clip = Some(previous);
        self.ui.set_clip_rect(previous.intersect(rect));
        self
    }
}
impl std::ops::Deref for UiStyle<'_> {
    type Target = egui::Ui;
    fn deref(&self) -> &Self::Target {
        &*self.ui
    }
}
impl std::ops::DerefMut for UiStyle<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut *self.ui
    }
}
impl Drop for UiStyle<'_> {
    fn drop(&mut self) {
        if let Some(previous) = self.previous_clip.take() {
            self.ui.set_clip_rect(previous);
        }
        if let Some(previous) = self.previous.take() {
            self.ui.set_style(previous);
        }
    }
}
/// egui's own widgets in `ui` styled as the element whose style is `values` (`surface` for a
/// region or container, whose background lies behind its widgets rather than filling them).
pub(super) fn ui_for(ui: &mut egui::Ui, values: Option<super::native_css::Computed>, surface: bool) -> UiStyle<'_> {
    let color = values.as_ref().filter(|style| style.authored.color).map(|style| style.color);
    let font = values.as_ref().filter(|style| style.authored.font_size).map(|style| style.font_size);
    let radius = values.as_ref().filter(|style| style.authored.radius).map(|style| style.radius);
    // A border the sheet gives (a width, 0 for none) is the ordinary widgets' outline too, in
    // every state: without it they keep egui's own.
    let border = values
        .as_ref()
        .filter(|style| style.authored.border_width)
        .map(|style| egui::Stroke::new(style.border_width, style.border));
    // A panel's or view's background is the surface behind its widgets, not their fill: a
    // device panel's colour would otherwise hide its sliders' rails.
    let fill = values.as_ref().filter(|style| style.authored.background && !surface).map(|style| style.background);
    // egui's own widgets in the scope (Settings, dialogs, the start page, the browser) take the
    // sheet's generic control rules: `button`, `button:hover`, `button:active`,
    // `button:disabled` for buttons, check boxes and drop-downs, `button[checked]` for a
    // chosen row, and `text-input` for text fields.
    let control = |hovered: bool, pressed: bool, disabled: bool| {
        super::element::BUTTON
            .pointer(hovered, pressed, disabled)
            .style()
            .filter(|c| c.authored.background || c.authored.border || c.authored.color || c.authored.radius)
    };
    let controls = [
        control(false, false, false),
        control(true, false, false),
        control(true, true, false),
        control(false, false, true),
    ];
    // `button[checked]`: a chosen item in a list or a menu (egui's selection).
    let chosen = super::element::BUTTON
        .attr(super::element::Checked, true)
        .style()
        .filter(|c| c.authored.background || c.authored.color);
    let input =
        super::element::TEXT_INPUT.style().filter(|c| c.authored.background || c.authored.border || c.authored.color);
    let has_controls = controls.iter().any(Option::is_some) || input.is_some() || chosen.is_some();
    let previous = if color.is_some()
        || font.is_some()
        || radius.is_some()
        || border.is_some()
        || fill.is_some()
        || has_controls
    {
        let previous = ui.style().clone();
        let style = ui.style_mut();
        let [idle, hovered, pressed, disabled] = &controls;
        for (widget, own) in [
            (&mut style.visuals.widgets.inactive, idle.as_ref()),
            (&mut style.visuals.widgets.hovered, hovered.as_ref()),
            (&mut style.visuals.widgets.active, pressed.as_ref()),
            (&mut style.visuals.widgets.open, pressed.as_ref()),
            (&mut style.visuals.widgets.noninteractive, disabled.as_ref()),
        ] {
            let Some(own) = own else { continue };
            if own.authored.background {
                widget.bg_fill = own.background;
                widget.weak_bg_fill = own.background;
            }
            if own.authored.border || own.authored.border_width {
                widget.bg_stroke = egui::Stroke::new(own.border_width, own.border);
            }
            if own.authored.color {
                widget.fg_stroke.color = own.color;
            }
            if own.authored.radius {
                widget.corner_radius = egui::CornerRadius::same(own.radius.clamp(0.0, 128.0) as u8);
            }
        }
        if let Some(chosen) = &chosen {
            if chosen.authored.background {
                style.visuals.selection.bg_fill = chosen.background;
            }
            if chosen.authored.color {
                style.visuals.selection.stroke.color = chosen.color;
            }
        }
        if let Some(input) = &input {
            if input.authored.background {
                style.visuals.extreme_bg_color = input.background;
                style.visuals.text_edit_bg_color = Some(input.background);
            }
            if input.authored.color {
                style.visuals.widgets.inactive.fg_stroke.color = input.color;
            }
        }
        if let Some(stroke) = border {
            for widget in [
                &mut style.visuals.widgets.noninteractive,
                &mut style.visuals.widgets.inactive,
                &mut style.visuals.widgets.hovered,
                &mut style.visuals.widgets.active,
                &mut style.visuals.widgets.open,
            ] {
                widget.bg_stroke = stroke;
            }
            style.visuals.selection.stroke.width = style.visuals.selection.stroke.width.min(stroke.width.max(1.0));
        }
        if let Some(fill) = fill {
            for widget in [&mut style.visuals.widgets.inactive, &mut style.visuals.widgets.open] {
                widget.bg_fill = fill;
                widget.weak_bg_fill = fill;
            }
        }
        if let Some(color) = color {
            style.visuals.override_text_color = Some(color);
        }
        if let Some(size) = font {
            for font in style.text_styles.values_mut() {
                font.size = size;
            }
        }
        if let Some(radius) = radius {
            let radius = egui::CornerRadius::same(radius.clamp(0.0, 128.0) as u8);
            for widget in [
                &mut style.visuals.widgets.noninteractive,
                &mut style.visuals.widgets.inactive,
                &mut style.visuals.widgets.hovered,
                &mut style.visuals.widgets.active,
                &mut style.visuals.widgets.open,
            ] {
                widget.corner_radius = radius;
            }
        }
        Some(previous)
    } else {
        None
    };
    UiStyle { ui, previous, previous_clip: None }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kit::element::{MIXER, PAN, SEND, test_el};
    use crate::kit::skin;

    /// What `draw` sees once the frames it takes for the tree to resolve have passed, with `css` as
    /// the theme's sheet.
    fn frames<R>(css: &str, mut draw: impl FnMut() -> R) -> R {
        skin::configure(&skin::Skin { css: Some(css.into()), ..Default::default() });
        let ctx = egui::Context::default();
        let mut result = None;
        for _ in 0..3 {
            ctx.run_ui(egui::RawInput::default(), |ui| {
                let _frame = super::super::native_css::frame_with_motion(ui, false);
                result = Some(draw());
            })
            .textures_delta
            .clear();
        }
        skin::configure(&skin::Skin::default());
        result.unwrap()
    }

    #[test]
    fn settings_inherit_and_the_nearest_element_s_wins() {
        let css = "track-column { track-artwork: on; } track-column clip { track-artwork: off; }";
        let seen = frames(css, || {
            let column = test_el("track-column").enter();
            let slot = test_el("slot").enter();
            let clip = test_el("clip").enter();
            (track_artwork(&column), track_artwork(&slot), track_artwork(&clip))
        });
        assert_eq!(seen, (true, true, false));
    }

    #[test]
    fn a_pan_s_appearance_is_its_own_and_reaches_no_other_control() {
        for (value, forced) in [("knob", Some(true)), ("field", Some(false)), ("native", None)] {
            let css = format!("mixer pan {{ appearance: {value}; }}");
            let seen = frames(&css, || {
                let _mixer = MIXER.enter();
                let pan = PAN.enter();
                let own = [false, true].map(|preference| knob_preference(&pan, preference));
                drop(pan);
                let send = SEND.enter();
                (own, [false, true].map(|preference| knob_preference(&send, preference)))
            });
            assert_eq!(seen.0, [false, true].map(|preference| forced.unwrap_or(preference)), "{value}");
            assert_eq!(seen.1, [false, true], "a send keeps the user's choice");
        }
    }

    #[test]
    fn a_live_theme_change_survives_all_enclosing_style_scopes() {
        let original = theme::palette();
        let mut changed = original;
        changed.text_primary = egui::Color32::from_rgb(40, 50, 60);
        changed.grid_major = egui::Color32::from_rgba_unmultiplied(80, 90, 100, 120);
        let ctx = egui::Context::default();
        let inside = frames("browser { color: #123456; }", || {
            let _window = test_el("window").enter();
            let _browser = test_el("browser").enter();
            let inside = theme::text_primary();
            theme::set_palette(&ctx, changed);
            inside
        });
        assert_eq!(inside, theme::parse_hex("#123456").unwrap());
        assert_eq!(theme::palette(), changed);
        theme::set_palette(&ctx, original);
    }

    #[test]
    fn ordinary_widget_style_preserves_ui_identity_and_restores_caller() {
        let css = "settings { color: #123456; font-size: 19px; }";
        skin::configure(&skin::Skin { css: Some(css.into()), ..Default::default() });
        let context = egui::Context::default();
        context
            .run_ui(egui::RawInput::default(), |ui| {
                let _frame = super::super::native_css::frame_with_motion(ui, false);
                let original = ui.style().clone();
                let original_clip = ui.clip_rect();
                let id = ui.scope_id();
                {
                    let scope = crate::kit::element::test_el("settings").enter();
                    let limited = egui::Rect::from_min_size(ui.max_rect().min, egui::Vec2::splat(16.0));
                    let styled = scope.ui(ui).clipped_to(limited);
                    assert_eq!(styled.scope_id(), id);
                    assert_eq!(styled.clip_rect(), original_clip.intersect(limited));
                    assert_eq!(styled.visuals().override_text_color, theme::parse_hex("#123456"));
                    assert!(styled.style().text_styles.values().all(|font| font.size == 19.0));
                }
                assert!(std::sync::Arc::ptr_eq(ui.style(), &original));
                assert_eq!(ui.clip_rect(), original_clip);
            })
            .textures_delta
            .clear();
        skin::configure(&skin::Skin::default());
    }
    #[test]
    fn descendant_styles_are_scoped_and_restore_palette() {
        let original = theme::palette();
        let seen = frames("browser { color: #123456; } browser button { color: #ABCDEF; }", || {
            let browser = test_el("browser").enter();
            let in_browser = theme::text_primary();
            let in_button = {
                let _button = test_el("button").enter();
                theme::text_primary()
            };
            let after = theme::text_primary();
            drop(browser);
            (in_browser, in_button, after, theme::palette())
        });
        assert_eq!(seen.0, theme::parse_hex("#123456").unwrap());
        assert_eq!(seen.1, theme::parse_hex("#ABCDEF").unwrap());
        assert_eq!(seen.2, seen.0);
        assert_eq!(seen.3, original);
    }
}
