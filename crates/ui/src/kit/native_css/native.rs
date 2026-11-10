// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The native style engine's adapter: sheets in, `Computed` paint values and rectangles out.
use super::{Authored, Computed, Engine, StyleNode, Styled, element_id, name_of, shadows, text};
use egui::{Color32, Rect, Vec2};
use std::sync::Arc;
use suhina_style::{
    cascade::{self, AppliedSheet, ComputedStyle, Layer},
    media::Viewport,
    property::Property,
    selector::{Element, Tree},
    sheet::Sheet,
    value::{Color, Length, Value},
};

pub(super) struct Paint {
    sheet: Sheet,
    defaults: Sheet,
    palette: String,
}

impl Paint {
    /// `None` when the sheet does not parse at all. Declarations the engine does not know are
    /// skipped with a notice, as CSS does, and the rest applies.
    pub fn new(source: &str, palette: &str) -> Option<Self> {
        let sheet = Sheet::parse(source).ok()?;
        Some(Self { sheet, defaults: defaults(palette)?, palette: palette.into() })
    }

    pub fn palette(&mut self, palette: &str) -> Option<()> {
        if self.palette != palette {
            self.defaults = defaults(palette)?;
            self.palette = palette.into();
        }
        Some(())
    }

    /// The cascade of the user-agent sheet (and the palette's defaults beside it), the theme, the chosen variant's values, device panel sheets (each bounded to its
    /// panel's element) and the user's sheet, in that order of precedence.
    fn styles(
        &self,
        tree: &Tree,
        viewport: Viewport,
        extra: Option<&Sheet>,
        variant: Option<&Sheet>,
        devices: &[(&Sheet, usize)],
        user: Option<&Sheet>,
    ) -> Option<Vec<ComputedStyle>> {
        cascade::compute(tree, viewport, &self.applied(extra, variant, devices, user)).ok()
    }

    /// The sheets of [`Paint::styles`], in their order.
    fn applied<'s>(
        &'s self,
        extra: Option<&'s Sheet>,
        variant: Option<&'s Sheet>,
        devices: &[(&'s Sheet, usize)],
        user: Option<&'s Sheet>,
    ) -> Vec<AppliedSheet<'s>> {
        let mut sheets = Vec::with_capacity(5 + devices.len().min(16));
        if let Some(agent) = user_agent() {
            sheets.push(AppliedSheet { sheet: agent, layer: Layer::UserAgent, root: None });
        }
        sheets.push(AppliedSheet { sheet: &self.defaults, layer: Layer::UserAgent, root: None });
        if let Some(extra) = extra {
            sheets.push(AppliedSheet { sheet: extra, layer: Layer::UserAgent, root: None });
        }
        sheets.push(AppliedSheet { sheet: &self.sheet, layer: Layer::Theme, root: None });
        if let Some(variant) = variant {
            sheets.push(AppliedSheet { sheet: variant, layer: Layer::Variant, root: None });
        }
        for (sheet, root) in devices.iter().take(16) {
            sheets.push(AppliedSheet { sheet, layer: Layer::Device, root: Some(*root) });
        }
        if let Some(user) = user {
            sheets.push(AppliedSheet { sheet: user, layer: Layer::User, root: None });
        }
        sheets
    }
}

/// Suhina's user-agent sheet, parsed once: the first tier of every cascade.
pub(in crate::kit) fn user_agent() -> Option<&'static Sheet> {
    static SHEET: std::sync::OnceLock<Option<Sheet>> = std::sync::OnceLock::new();
    SHEET.get_or_init(|| Sheet::parse(suhina_theme_package::USER_AGENT_SHEET).ok()).as_ref()
}

/// The colour the user-agent sheet alone gives the element `name` with `attributes`
/// (`sample-point[kind="kick"]`), as the element's own rule: what it looks like where no sheet
/// styles the window. `None` where the sheet gives it no colour.
pub fn user_agent_color(name: &str, attributes: &[(&str, &str)]) -> Option<Color32> {
    let sheet = user_agent()?;
    let element =
        attributes.iter().fold(Element::new(name, None), |element, (name, value)| element.with_attribute(name, value));
    let tree = Tree::new(vec![element]).ok()?;
    let viewport = Viewport { width: 1280.0, height: 800.0, reduced_motion: false };
    let style =
        cascade::last_style(&tree, None, viewport, &[AppliedSheet { sheet, layer: Layer::UserAgent, root: None }])
            .ok()?;
    match style.get(Property::Color) {
        color if color == Property::Color.initial() => None,
        Value::Color(Color::Rgba([r, g, b, a])) => Some(Color32::from_rgba_unmultiplied(r, g, b, a)),
        _ => None,
    }
}

/// The palette colours of the parts whose artwork takes their `color` (`currentColor`) where a
/// theme gives them none: a knob's body, a fader's rail and its cap, lit a little by the values'
/// text while the pointer is on it. Each is a rule on the part itself, so a colour a theme gives
/// an element around it (`knob`, `mixer`) is not inherited into the artwork. A knob's pointer
/// has none: it takes the knob's colour.
pub(super) const PART_COLOURS: &str = "\
knob-body { color: var(--control-background); }
fader-rail { color: color-mix(in srgb, var(--control-background), black 65%); }
fader-cap { color: color-mix(in srgb, var(--control-background), var(--text-value) 28%); }
fader-cap:hover, fader-cap:active { color: color-mix(in srgb, var(--control-hoverBackground), var(--text-value) 28%); }
";

/// The palette's values, the text they colour and [`PART_COLOURS`], beside the user-agent sheet
/// in its tier. Not part of the user-agent sheet, which has no variables: it goes with the palette
/// (#995 § 10).
fn defaults(palette: &str) -> Option<Sheet> {
    Sheet::parse(&format!(
        "{palette}\nhtml, body {{ color:var(--text-primary); font-size:{}px; }}\n{PART_COLOURS}",
        super::theme::TEXT
    ))
    .ok()
}

fn viewport(engine: &Engine) -> Viewport {
    Viewport { width: engine.size[0] as f32, height: engine.size[1] as f32, reduced_motion: !engine.motion }
}

fn variant(engine: &Engine) -> Option<&Sheet> {
    engine.variant.as_ref().map(|(_, sheet)| sheet.as_ref())
}

/// `html` and under it `body`, or the `window` element when the window is laid out.
fn roots(window: bool) -> Vec<Element> {
    vec![Element::new("html", None), Element::new(if window { "window" } else { "body" }, Some(0))]
}

/// A host element of the registry's `name`, with an optional id and project object.
pub(super) fn element(name: &str, id: Option<&str>, object: Option<&str>, parent: Option<usize>) -> Element {
    let mut element = Element::new(name, parent);
    element.id = id.map(str::to_owned);
    element.object = object.map(str::to_owned);
    element.states.push("enabled".into());
    element
}

/// The style tree's element for `node`, under the element at `parent`.
fn element_of(engine: &Engine, node: &StyleNode, parent: usize) -> Element {
    let mut element = Element::new(node.element.name(), Some(parent));
    element.id = Some(element_id(engine, node));
    element.object = name_of(engine, node).map(str::to_owned);
    element.variables.clone_from(&node.variables);
    element.attributes.clone_from(&node.attributes);
    element.states.push(if node.state & 4 != 0 { "disabled" } else { "enabled" }.into());
    for (bit, state) in [(1, "hover"), (2, "active"), (8, "focus"), (8, "focus-visible")] {
        if node.state & bit != 0 {
            element.states.push(state.into());
        }
    }
    // What the app knows about the element is an attribute: `[checked]`, `[selected]`.
    for (bit, attribute) in super::HOST_STATES {
        if node.state & bit != 0 {
            element.attributes.push((attribute.into(), String::new()));
        }
    }
    element
}

/// The style of the element at `at` in this frame's tree, worked out from its parent's and the
/// elements it is in, without its siblings: as `Resolver::leaf_style` styles a leaf.
pub(super) fn fresh(engine: &mut Engine, at: usize) -> Option<Styled> {
    let mut chain = vec![at];
    while let Some(parent) = chain.last().and_then(|&node| engine.nodes.get(node)).and_then(|node| node.parent) {
        chain.push(parent);
    }
    chain.reverse();
    let parent = match chain.len().checked_sub(2).and_then(|p| chain.get(p)) {
        Some(&parent) => engine.served.get(parent).cloned().flatten()?,
        None => body(engine)?,
    };
    let mut elements = roots(false);
    let mut devices = Vec::new();
    for (index, &node) in chain.iter().enumerate() {
        let node = engine.nodes.get(node)?;
        elements.push(element_of(engine, node, index + 1));
        if let Some(sheet) = (node.sheet != 0).then(|| engine.device_sheets.get(&node.sheet)).flatten() {
            devices.push((sheet.as_ref(), index + 2));
        }
    }
    let tree = Tree::new(elements).ok()?;
    let paint = engine.native.as_ref()?;
    let user = engine.user.as_ref().map(|(_, sheet)| sheet.as_ref());
    let sheets = paint.applied(None, variant(engine), &devices, user);
    let raw = Arc::new(cascade::last_style(&tree, Some(&parent), viewport(engine), &sheets).ok()?);
    Some(Styled { computed: computed_from(Arc::clone(&raw)), raw })
}

/// `html` and `body` under it, the parent of the tree's outermost elements, worked out once.
fn body(engine: &mut Engine) -> Option<Arc<ComputedStyle>> {
    if let Some(body) = &engine.body {
        return Some(Arc::clone(body));
    }
    let tree = Tree::new(roots(false)).ok()?;
    let user = engine.user.as_ref().map(|(_, sheet)| sheet.as_ref());
    let paint = engine.native.as_ref()?;
    let body = Arc::new(paint.styles(&tree, viewport(engine), None, variant(engine), &[], user)?.pop()?);
    engine.body = Some(Arc::clone(&body));
    Some(body)
}

pub(super) fn retained(engine: &Engine) -> Option<Vec<(u64, Computed)>> {
    let paint = engine.native.as_ref()?;
    let mut elements = roots(false);
    for node in &engine.nodes {
        elements.push(element_of(engine, node, node.parent.map_or(1, |parent| parent + 2)));
    }
    // CSS :hover applies to the ancestors of the pointer target as well.
    for (index, node) in engine.nodes.iter().enumerate().rev() {
        if node.state & 1 == 0 {
            continue;
        }
        let mut parent = elements.get(index + 2).and_then(|element| element.parent);
        while let Some(index) = parent {
            let element = elements.get_mut(index)?;
            if !element.states.iter().any(|state| state == "hover") {
                element.states.push("hover".into());
            }
            parent = element.parent;
        }
    }
    let tree = Tree::new(elements).ok()?;
    let devices: Vec<(&Sheet, usize)> = engine
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, node)| node.sheet != 0)
        .filter_map(|(index, node)| engine.device_sheets.get(&node.sheet).map(|sheet| (sheet.as_ref(), index + 2)))
        .collect();
    let user = engine.user.as_ref().map(|(_, sheet)| sheet.as_ref());
    let values = paint.styles(&tree, viewport(engine), None, variant(engine), &devices, user)?;
    Some(engine.nodes.iter().zip(values.iter().skip(2)).map(|(node, value)| (node.key, computed(value))).collect())
}

#[cfg(test)]
pub(super) fn scoped(engine: &Engine, stack: &[String]) -> Option<Computed> {
    let mut elements = roots(false);
    for (index, class) in stack.iter().enumerate() {
        elements.push(element(class, Some(&format!("scope-{index}")), None, Some(index + 1)));
    }
    let tree = Tree::new(elements).ok()?;
    let user = engine.user.as_ref().map(|(_, sheet)| sheet.as_ref());
    engine.native.as_ref()?.styles(&tree, viewport(engine), None, variant(engine), &[], user)?.last().map(computed)
}

/// The style of the last of `path` (element names from `window` inward) under the theme's
/// `sheet`, the variant's and the user's, at the window's usual size
/// ([`crate::kit::theme_vars::element_styles`]).
pub(super) fn settings_of(
    sheet: &Sheet,
    variant: Option<&Sheet>,
    user: Option<&Sheet>,
    path: &[&str],
) -> Option<Computed> {
    crate::kit::theme_vars::element_styles(sheet, variant, user, &[path]).ok()?.first().map(computed)
}

/// A `theme-swatch` element whose `color` attribute is `color`, as the theme in use styles it
/// over the user-agent sheet's rounded square.
pub(super) fn theme_swatch(engine: &Engine, color: &str) -> Option<Computed> {
    let mut elements = roots(false);
    elements.push(Element::new("theme-swatch", Some(1)).with_attribute("color", color));
    let tree = Tree::new(elements).ok()?;
    let user = engine.user.as_ref().map(|(_, sheet)| sheet.as_ref());
    engine.native.as_ref()?.styles(&tree, viewport(engine), None, variant(engine), &[], user)?.last().map(computed)
}

/// Properties that ask for theme layout when a theme writes them on a laid-out element.
const LAYOUT: &[Property] = &[
    Property::Display,
    Property::Width,
    Property::Height,
    Property::MinWidth,
    Property::MinHeight,
    Property::MaxWidth,
    Property::MaxHeight,
    Property::Order,
    Property::FlexDirection,
    Property::FlexWrap,
    Property::FlexGrow,
    Property::FlexShrink,
    Property::FlexBasis,
    Property::JustifyContent,
    Property::AlignItems,
    Property::AlignSelf,
    Property::AlignContent,
    Property::GridTemplateColumns,
    Property::GridTemplateRows,
    Property::GridColumn,
    Property::GridRow,
    Property::ColumnGap,
    Property::RowGap,
    Property::MarginTop,
    Property::MarginRight,
    Property::MarginBottom,
    Property::MarginLeft,
    Property::PaddingTop,
    Property::PaddingRight,
    Property::PaddingBottom,
    Property::PaddingLeft,
    Property::Visibility,
    Property::Layout,
    Property::Columns,
    Property::Rows,
    Property::Areas,
    Property::Area,
    Property::Visible,
];

/// Rectangles and styles of host elements laid out by the theme.
pub(super) struct Laid {
    /// Whether the theme wrote any layout property on these elements.
    pub requested: bool,
    /// For each given element: its rectangle (none when not displayed) and style.
    pub items: Vec<(Option<Rect>, Computed)>,
}

/// Lays out `elements` (parents index the full list, after `html` and `body.window`) in a
/// box of `size`, with host `defaults` under the theme.
pub(super) fn layout(engine: &Engine, elements: Vec<Element>, defaults: &str, size: [f32; 2]) -> Option<Laid> {
    let paint = engine.native.as_ref()?;
    let count = elements.len();
    let mut all = roots(true);
    all.extend(elements);
    let tree = Tree::new(all).ok()?;
    let defaults = Sheet::parse(defaults).ok()?;
    let user = engine.user.as_ref().map(|(_, sheet)| sheet.as_ref());
    let styles = paint.styles(&tree, viewport(engine), Some(&defaults), variant(engine), &[], user)?;
    let size = [size[0].clamp(0.0, 16384.0), size[1].clamp(0.0, 16384.0)];
    let rectangles = suhina_style::layout::compute(&tree, &styles, size).ok()?;
    let requested = styles.iter().skip(2).any(|style| LAYOUT.iter().any(|property| style.authored(*property)));
    let items = rectangles
        .iter()
        .zip(&styles)
        .skip(2)
        .take(count)
        .map(|(rect, style)| {
            let rect = rect.map(|r| Rect::from_min_size(egui::pos2(r.x, r.y), Vec2::new(r.width, r.height)));
            let rect = rect
                .filter(|r| r.is_finite() && [r.min.x, r.min.y, r.max.x, r.max.y].iter().all(|v| v.abs() <= 16384.0));
            (rect, computed(style))
        })
        .collect();
    Some(Laid { requested, items })
}

pub(super) fn computed(style: &ComputedStyle) -> Computed {
    computed_from(Arc::new(style.clone()))
}

/// [`computed`], keeping `style` itself.
fn computed_from(resolved: Arc<ComputedStyle>) -> Computed {
    let style = resolved.as_ref();
    let color = |property| match style.get(property) {
        Value::Color(Color::Rgba([r, g, b, a])) => Color32::from_rgba_unmultiplied(r, g, b, a),
        _ => Color32::TRANSPARENT,
    };
    let font = match style.get(Property::FontSize) {
        Value::Length(Length::Px(value)) => value,
        _ => super::theme::TEXT,
    };
    let radius = match style.get(Property::Radius) {
        Value::Length(Length::Px(value)) => value,
        Value::Length(Length::Em(value)) => value * font,
        // A share of the size, which the painter learns only later: 50 % or more is round
        // (clamped to half the shorter side there), less is that share of 256 points.
        Value::Length(Length::Percent(value)) => value * 256.0,
        _ => 0.0,
    };
    let foreground = match style.get(Property::Color) {
        Value::Color(Color::Rgba(value)) => value,
        _ => [255; 4],
    };
    // currentColor in other colours resolves to the foreground.
    let ink = |property| match style.get(property) {
        Value::Color(Color::Current) => {
            let [r, g, b, a] = foreground;
            Color32::from_rgba_unmultiplied(r, g, b, a)
        }
        _ => color(property),
    };
    let resolved_shadows = |property| match style.get(property) {
        Value::Shadows(values) => values.iter().map(|shadow| shadow.resolve(font, foreground)).collect::<Vec<_>>(),
        _ => Vec::new(),
    };
    Computed {
        color: color(Property::Color),
        background: ink(Property::BackgroundColor),
        border: ink(Property::BorderColor),
        font_size: font.clamp(7.0, 96.0),
        radius: radius.clamp(0.0, 128.0),
        border_width: match style.get(Property::BorderWidth) {
            Value::Length(Length::Px(value)) => value.clamp(0.0, 16.0),
            _ => 0.0,
        },
        text_align: match style.get(Property::TextAlign) {
            _ if !style.authored(Property::TextAlign) => None,
            Value::Keyword("left" | "start") => Some(-1),
            Value::Keyword("center") => Some(0),
            Value::Keyword("right" | "end") => Some(1),
            _ => None,
        },
        width: match style.get(Property::Width) {
            Value::Length(Length::Px(value)) if style.authored(Property::Width) => Some(value.clamp(0.0, 512.0)),
            _ => None,
        },
        height: match style.get(Property::Height) {
            Value::Length(Length::Px(value)) if style.authored(Property::Height) => Some(value.clamp(0.0, 512.0)),
            _ => None,
        },
        opacity: match style.get(Property::Opacity) {
            Value::Number(value) => value,
            _ => 1.0,
        },
        visible: style.get(Property::Visibility) == Value::Keyword("visible")
            && style.get(Property::Visible) != Value::Bool(false),
        accent: ink(Property::AccentColor),
        authored: Authored {
            color: style.authored(Property::Color),
            background: style.authored(Property::BackgroundColor),
            border: style.authored(Property::BorderColor),
            font_size: style.authored(Property::FontSize),
            radius: style.authored(Property::Radius),
            accent: style.authored(Property::AccentColor),
            border_width: style.authored(Property::BorderWidth),
        },
        variables: style.shared_variables(),
        gradients: match style.get(Property::BackgroundImage) {
            Value::Gradients(values) => values
                .iter()
                .map(|gradient| super::paint::Gradient::from_native(gradient.resolved(font, foreground)))
                .collect::<Vec<_>>()
                .into(),
            _ => Vec::new().into(),
        },
        shadows: resolved_shadows(Property::BoxShadow)
            .iter()
            .map(shadows::Shadow::from_native)
            .collect::<Vec<_>>()
            .into(),
        text_shadows: resolved_shadows(Property::TextShadow)
            .iter()
            .map(text::Shadow::from_native)
            .collect::<Vec<_>>()
            .into(),
        transitions: match style.get(Property::Transition) {
            Value::Transitions(list) => list.into(),
            _ => Vec::new().into(),
        },
        animations: style.animations().to_vec().into(),
        resolved: Arc::clone(&resolved),
    }
}
