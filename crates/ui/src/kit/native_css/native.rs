// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The native style engine's adapter: sheets in, `Computed` paint values and rectangles out.
use super::{Authored, Computed, Engine, element_id, shadows, text};
use egui::{Color32, Rect, Vec2};
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

    /// The cascade of host defaults, the theme, the chosen variant's values, device panel sheets (each bounded to its
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
        let mut sheets = vec![AppliedSheet { sheet: &self.defaults, layer: Layer::Default, root: None }];
        if let Some(extra) = extra {
            sheets.push(AppliedSheet { sheet: extra, layer: Layer::Default, root: None });
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
        cascade::compute(tree, viewport, &sheets).ok()
    }
}

fn defaults(palette: &str) -> Option<Sheet> {
    Sheet::parse(&format!("{palette}\nhtml, body {{ color:var(--text-primary); font-size:{}px; }}", super::theme::TEXT))
        .ok()
}

fn viewport(engine: &Engine) -> Viewport {
    Viewport {
        width: engine.size[0] as f32,
        height: engine.size[1] as f32,
        dark: engine.dark,
        reduced_motion: !engine.motion,
    }
}

fn variant(engine: &Engine) -> Option<&Sheet> {
    engine.variant.as_ref().map(|(_, sheet)| sheet.as_ref())
}

fn roots(window: bool) -> Vec<Element> {
    let mut body = Element::new("body", Some(0));
    if window {
        body.classes.push("window".into());
    }
    vec![Element::new("html", None), body]
}

/// A host element: a `div` with one class, an optional id and project object.
pub(super) fn element(class: &str, id: Option<&str>, object: Option<&str>, parent: Option<usize>) -> Element {
    let mut element = Element::new("div", parent);
    element.id = id.map(str::to_owned);
    element.object = object.map(str::to_owned);
    element.classes.push(class.into());
    element.states.push("enabled".into());
    element
}

pub(super) fn retained(engine: &Engine) -> Option<Vec<(u64, Computed)>> {
    let paint = engine.native.as_ref()?;
    let mut elements = roots(false);
    for node in &engine.nodes {
        let mut element = Element::new("div", Some(node.parent.map_or(1, |parent| parent + 2)));
        element.id = Some(element_id(node));
        element.object = node.name.clone();
        element.classes.push(node.class.clone());
        element.variables.clone_from(&node.variables);
        element.states.push(if node.state & 4 != 0 { "disabled" } else { "enabled" }.into());
        for (bit, state) in
            [(1, "hover"), (2, "active"), (8, "focus"), (8, "focus-visible")].into_iter().chain(super::HOST_STATES)
        {
            if node.state & bit != 0 {
                element.states.push(state.into());
            }
        }
        elements.push(element);
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

pub(super) fn scoped(engine: &Engine, stack: &[String]) -> Option<Computed> {
    let mut elements = roots(false);
    for (index, class) in stack.iter().enumerate() {
        elements.push(element(class, Some(&format!("scope-{index}")), None, Some(index + 1)));
    }
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
    }
}
