// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Staged native paint adapter. A sheet is accepted as a whole, never partially.
use super::{Authored, Computed, Engine, html_id, shadows, text};
use egui::Color32;
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
    pub fn new(source: &str, palette: &str) -> Option<Self> {
        let sheet = Sheet::parse(source).ok()?;
        if !sheet.supports_native_paint() {
            return None;
        }
        Some(Self { sheet, defaults: defaults(palette)?, palette: palette.into() })
    }

    pub fn palette(&mut self, palette: &str) -> Option<()> {
        if self.palette != palette {
            self.defaults = defaults(palette)?;
            self.palette = palette.into();
        }
        Some(())
    }

    fn resolve(&self, tree: &Tree, viewport: Viewport) -> Option<Vec<Computed>> {
        cascade::compute_checked(
            tree,
            viewport,
            &[
                AppliedSheet { sheet: &self.defaults, layer: Layer::Default, root: None },
                AppliedSheet { sheet: &self.sheet, layer: Layer::Theme, root: None },
            ],
        )
        .ok()
        .map(|styles| styles.iter().map(computed).collect())
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

fn roots() -> Vec<Element> {
    vec![Element::new("html", None), Element::new("body", Some(0))]
}

pub(super) fn retained(engine: &Engine) -> Option<Vec<(u64, Computed)>> {
    let paint = engine.native.as_ref()?;
    let mut elements = roots();
    for node in &engine.nodes {
        let mut element = Element::new("div", Some(node.parent.map_or(1, |parent| parent + 2))).with_id(&html_id(node));
        element.classes.push(node.class.clone());
        element.states.push(if node.state & 4 != 0 { "disabled" } else { "enabled" }.into());
        for (bit, state) in [(1, "hover"), (2, "active"), (8, "focus"), (8, "focus-visible")] {
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
    let values = paint.resolve(&tree, viewport(engine))?;
    Some(engine.nodes.iter().zip(values.into_iter().skip(2)).map(|(node, value)| (node.key, value)).collect())
}

pub(super) fn scoped(engine: &Engine, stack: &[String]) -> Option<Computed> {
    let mut elements = roots();
    for (index, class) in stack.iter().enumerate() {
        let mut element = Element::new("div", Some(index + 1)).with_id(&format!("scope-{index}"));
        element.classes.push(class.clone());
        element.states.push("enabled".into());
        elements.push(element);
    }
    let tree = Tree::new(elements).ok()?;
    engine.native.as_ref()?.resolve(&tree, viewport(engine))?.pop()
}

fn computed(style: &ComputedStyle) -> Computed {
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
        _ => 0.0,
    };
    let foreground = match style.get(Property::Color) {
        Value::Color(Color::Rgba(value)) => value,
        _ => [255; 4],
    };
    let resolved_shadows = |property| match style.get(property) {
        Value::Shadows(values) => values.iter().map(|shadow| shadow.resolve(font, foreground)).collect::<Vec<_>>(),
        _ => Vec::new(),
    };
    Computed {
        color: color(Property::Color),
        background: color(Property::BackgroundColor),
        border: color(Property::BorderColor),
        font_size: font.clamp(7.0, 96.0),
        radius: radius.clamp(0.0, 128.0),
        opacity: match style.get(Property::Opacity) {
            Value::Number(value) => value,
            _ => 1.0,
        },
        visible: true,
        authored: Authored {
            color: style.authored(Property::Color),
            background: style.authored(Property::BackgroundColor),
            border: style.authored(Property::BorderColor),
            font_size: style.authored(Property::FontSize),
            radius: style.authored(Property::Radius),
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
    }
}
