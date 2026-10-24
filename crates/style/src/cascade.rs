// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared style resolution for window elements and scoped SDK device panels.
use crate::{
    media::Viewport,
    property::Property,
    selector::{Specificity, Tree},
    sheet::{Declaration, Sheet},
    value::{Color, Value},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

/// Layers are an explicit host policy; a device cannot choose a higher layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Layer {
    Default,
    Theme,
    /// A theme variant's chosen values, above the theme's own rules.
    Variant,
    Device,
    User,
}

pub struct AppliedSheet<'a> {
    pub sheet: &'a Sheet,
    pub layer: Layer,
    pub root: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct ComputedStyle {
    values: BTreeMap<Property, Value>,
    variables: Arc<BTreeMap<String, String>>,
    authored: BTreeSet<Property>,
    /// The element's `animation` items with their keyframes resolved for it.
    keyframes: Vec<(crate::motion::Animation, Arc<crate::motion::Frames>)>,
}

impl Default for ComputedStyle {
    fn default() -> Self {
        Self {
            values: Property::ALL.iter().map(|p| (*p, p.initial())).collect(),
            variables: Arc::default(),
            authored: BTreeSet::new(),
            keyframes: Vec::new(),
        }
    }
}

impl ComputedStyle {
    pub fn get(&self, property: Property) -> Value {
        self.values.get(&property).cloned().unwrap_or_else(|| property.initial())
    }
    /// Every custom property the element sees, resolved.
    pub fn variables(&self) -> &BTreeMap<String, String> {
        &self.variables
    }

    pub fn variable(&self, name: &str) -> Option<&str> {
        self.variables.get(name).map(String::as_str)
    }
    /// Host defaults must not replace a native widget's own appearance. Inherited
    /// authored properties retain this marker across the component tree.
    pub fn authored(&self, property: Property) -> bool {
        self.authored.contains(&property)
    }
    /// Keyframe animations that apply, each with its frames resolved for this element.
    pub fn animations(&self) -> &[(crate::motion::Animation, Arc<crate::motion::Frames>)] {
        &self.keyframes
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Priority(u8, u8, u16, Specificity, usize);

fn priority(layer: Layer, important: bool, cascade: Option<u16>, specificity: Specificity, order: usize) -> Priority {
    // Native layers deliberately guarantee that a user's override wins over
    // device/theme sheets, including author !important. Within a layer normal
    // CSS importance, `@layer` order, specificity and source order apply.
    Priority(layer as u8, u8::from(important), Sheet::layer_rank(cascade, important), specificity, order)
}

/// Compute a bounded immutable snapshot. The host calls this on relevant changes,
/// never from the audio callback. Malformed declarations fall back to inherited/default values.
pub fn compute(tree: &Tree, viewport: Viewport, sheets: &[AppliedSheet<'_>]) -> Result<Vec<ComputedStyle>, String> {
    compute_inner(tree, viewport, sheets, false)
}

/// Migration adapters require every substituted value to be supported before
/// publishing a snapshot. Unsupported expressions leave the legacy path in charge.
pub fn compute_checked(
    tree: &Tree,
    viewport: Viewport,
    sheets: &[AppliedSheet<'_>],
) -> Result<Vec<ComputedStyle>, String> {
    compute_inner(tree, viewport, sheets, true)
}

fn compute_inner(
    tree: &Tree,
    viewport: Viewport,
    sheets: &[AppliedSheet<'_>],
    checked: bool,
) -> Result<Vec<ComputedStyle>, String> {
    if !viewport.valid() || sheets.len() > 32 {
        return Err("Invalid style viewport or sheet count".into());
    }
    if sheets.iter().any(|sheet| sheet.root.is_some_and(|root| tree.elements().get(root).is_none())) {
        return Err("Invalid style scope root".into());
    }
    let mut result: Vec<ComputedStyle> = Vec::with_capacity(tree.elements().len());
    let mut work = 2_000_000usize;
    let mut variable_bytes = 0usize;
    for (node, element) in tree.elements().iter().enumerate() {
        let parent = element.parent.and_then(|parent| result.get(parent));
        let mut computed = ComputedStyle::default();
        if let Some(parent) = parent {
            for property in Property::ALL.iter().filter(|p| p.inherits()) {
                computed.values.insert(*property, parent.get(*property));
                if parent.authored(*property) {
                    computed.authored.insert(*property);
                }
            }
            computed.variables = Arc::clone(&parent.variables);
        } else {
            // A registered property's initial value is what the root sees until a rule sets it.
            let initial: BTreeMap<String, String> = sheets
                .iter()
                .flat_map(|applied| applied.sheet.properties.iter())
                .filter_map(|registered| Some((registered.name.clone(), registered.initial.clone()?)))
                .collect();
            if !initial.is_empty() {
                computed.variables = Arc::new(initial);
            }
        }
        // The host's own values come first, so any rule may replace them.
        if !element.variables.is_empty() {
            let mut variables = (*computed.variables).clone();
            for (name, value) in element.variables.iter().filter(|(name, _)| name.starts_with("--")).take(16) {
                variables.insert(name.clone(), value.clone());
            }
            computed.variables = Arc::new(variables);
        }
        let mut winners: BTreeMap<&str, (Priority, &Declaration)> = BTreeMap::new();
        let mut order = 0usize;
        for applied in sheets {
            if applied.root.is_some_and(|root| !tree.contains(node, root)) {
                continue;
            }
            for rule in &applied.sheet.rules {
                order += 1;
                work = work.checked_sub(1).ok_or("Style cascade exceeded its work budget")?;
                if !rule.media.iter().all(|media| media.matches(viewport)) {
                    continue;
                }
                let specificity = rule
                    .selectors
                    .iter()
                    .filter_map(|selector| {
                        selector
                            .matches_with_budget(tree, node, applied.root, &mut work)
                            .then_some(selector.specificity)
                    })
                    .max();
                if work == 0 {
                    return Err("Style selector matching exceeded its work budget".into());
                }
                let Some(specificity) = specificity else {
                    continue;
                };
                for declaration in &rule.declarations {
                    let priority = priority(applied.layer, declaration.important, rule.layer, specificity, order);
                    if winners.get(declaration.name.as_str()).is_none_or(|(previous, _)| *previous <= priority) {
                        winners.insert(&declaration.name, (priority, declaration));
                    }
                }
            }
        }
        if winners.keys().any(|key| key.starts_with("--")) {
            let mut variables = (*computed.variables).clone();
            for (name, (_, declaration)) in winners.iter().filter(|(name, _)| name.starts_with("--")) {
                match declaration.value.trim() {
                    "initial" => {
                        match sheets
                            .iter()
                            .rev()
                            .flat_map(|applied| applied.sheet.properties.iter())
                            .find(|registered| registered.name == *name)
                            .and_then(|registered| registered.initial.clone())
                        {
                            Some(initial) => variables.insert((*name).into(), initial),
                            None => variables.remove(*name),
                        };
                    }
                    "inherit" | "unset" => {}
                    _ => {
                        variables.insert((*name).into(), declaration.value.clone());
                    }
                }
            }
            let mut resolved = crate::variables::resolve(&variables)?;
            // A registered property's value that does not match its syntax is invalid at
            // computed-value time: the property takes its initial value.
            for registered in sheets.iter().flat_map(|applied| applied.sheet.properties.iter()) {
                if let Some(value) = resolved.get(&registered.name)
                    && !registered.syntax.accepts(value)
                {
                    match &registered.initial {
                        Some(initial) => resolved.insert(registered.name.clone(), initial.clone()),
                        None => resolved.remove(&registered.name),
                    };
                }
            }
            variable_bytes = variable_bytes
                .checked_add(resolved.values().map(String::len).sum::<usize>())
                .filter(|bytes| *bytes <= 16 * 1024 * 1024)
                .ok_or("Computed variables exceed 16 MiB")?;
            computed.variables = Arc::new(resolved);
        }
        for (name, (priority, declaration)) in winners {
            let Some(property) = Property::from_name(name) else {
                continue;
            };
            let value = match crate::variables::substitute(&declaration.value, &computed.variables).and_then(|value| {
                match declaration.shorthand {
                    Some(shorthand) => shorthand.part(property, &value),
                    None => Ok(value),
                }
            }) {
                Ok(value) => value,
                Err(error) if checked => return Err(error),
                Err(_) => continue,
            };
            let value = match value.trim() {
                "inherit" => parent.map_or_else(|| property.initial(), |parent| parent.get(property)),
                "initial" => property.initial(),
                "unset" if property.inherits() => {
                    parent.map_or_else(|| property.initial(), |parent| parent.get(property))
                }
                "unset" => property.initial(),
                other => match property.parse(&crate::value::viewport_units(other, viewport.width, viewport.height)) {
                    Ok(value) => value,
                    Err(error) if checked => return Err(error),
                    Err(_) => continue,
                },
            };
            computed.values.insert(property, value);
            if priority.0 > Layer::Default as u8 {
                computed.authored.insert(property);
            }
        }
        if let Value::Animations(animations) = computed.get(Property::Animation) {
            for animation in animations {
                // The last keyframes rule of that name in the sheets that reach this element.
                let rule = sheets
                    .iter()
                    .rev()
                    .filter(|applied| applied.root.is_none_or(|root| tree.contains(node, root)))
                    .find_map(|applied| applied.sheet.keyframes.iter().rev().find(|(name, _)| *name == animation.name));
                let Some((_, frames)) = rule else { continue };
                let mut resolved: crate::motion::Frames = Vec::with_capacity(frames.len());
                for (offset, declarations) in frames {
                    let mut values = Vec::new();
                    for declaration in declarations.iter().filter(|d| !d.important) {
                        work = work.checked_sub(1).ok_or("Style cascade exceeded its work budget")?;
                        let Some(property) = Property::from_name(&declaration.name) else { continue };
                        let value = crate::variables::substitute(&declaration.value, &computed.variables)
                            .and_then(|value| match declaration.shorthand {
                                Some(shorthand) => shorthand.part(property, &value),
                                None => Ok(value),
                            })
                            .and_then(|value| {
                                property.parse(&crate::value::viewport_units(&value, viewport.width, viewport.height))
                            });
                        match value {
                            Ok(value) => values.push((property, value)),
                            Err(error) if checked => return Err(error),
                            Err(_) => {}
                        }
                    }
                    resolved.push((*offset, values));
                }
                for (_, values) in &resolved {
                    for (property, _) in values {
                        computed.authored.insert(*property);
                    }
                }
                computed.keyframes.push((animation, Arc::new(resolved)));
            }
        }
        // Inherited font size is a computed size, not an em expression to apply
        // again at each descendant. Other em lengths resolve against this size.
        let parent_size = match parent.map(|style| style.get(Property::FontSize)) {
            Some(Value::Length(crate::value::Length::Px(size))) => size,
            _ => 11.0,
        };
        let font_size = match computed.get(Property::FontSize) {
            Value::Length(crate::value::Length::Px(size)) => size,
            Value::Length(crate::value::Length::Em(scale) | crate::value::Length::Percent(scale)) => {
                parent_size * scale
            }
            _ => parent_size,
        }
        .clamp(1.0, 256.0);
        computed.values.insert(Property::FontSize, Value::Length(crate::value::Length::Px(font_size)));
        // Text shadows inherit their computed lengths. Resolve em once at the
        // authoring element; a child's larger font must not scale them again.
        for value in computed.values.values_mut() {
            if let Value::Shadows(shadows) = value {
                for shadow in shadows {
                    for length in shadow.offset.iter_mut().chain([&mut shadow.blur, &mut shadow.spread]) {
                        if let crate::value::Length::Em(scale) = *length {
                            *length = crate::value::Length::Px(scale * font_size);
                        }
                    }
                }
            } else if let Value::Gradients(gradients) = value {
                for gradient in gradients {
                    for stop in &mut gradient.stops {
                        if let Some(crate::value::Length::Em(scale)) = stop.position {
                            stop.position = Some(crate::value::Length::Px(scale * font_size));
                        }
                    }
                }
            }
        }
        let foreground = match computed.get(Property::Color) {
            Value::Color(Color::Current) => {
                parent.map_or(Value::Color(Color::Rgba([255; 4])), |parent| parent.get(Property::Color))
            }
            other => other,
        };
        computed.values.insert(Property::Color, foreground.clone());
        for value in computed.values.values_mut() {
            if *value == Value::Color(Color::Current) {
                *value = foreground.clone();
            }
        }
        result.push(computed);
    }
    Ok(result)
}

#[cfg(test)]
#[path = "cascade_tests.rs"]
mod tests;
