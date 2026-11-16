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

/// The tiers of the cascade, lowest first: Suhina's user-agent sheet, its base sheet, the theme,
/// its variant, device panel sheets and the user's sheet. A rule of a higher tier beats every rule
/// of a lower one, whatever their specificity or `@layer`. Layers are an explicit host policy; a
/// device cannot choose a higher layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Layer {
    /// Suhina's user-agent sheet (`suhina_theme_package::USER_AGENT_SHEET`): the bare floor.
    UserAgent,
    /// Suhina's design under every theme (`suhina_theme_package::BASE_SHEET`), the core variables'
    /// registrations and the theme's colour map. Like the user agent's, its declarations are not
    /// [`ComputedStyle::authored`]: they are Suhina's, not a theme's.
    Base,
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

/// An element's computed style. Values are kept only for the properties it inherits (every one,
/// whatever its value: `color`, `font-size`) and the ones a rule sets; any other reads
/// as its property's initial value ([`ComputedStyle::get`]). An element's style is computed often,
/// and most of its ninety-odd properties are never written. A value kept may equal the initial one,
/// and equality compares what is kept: two styles compare equal when they were worked out alike,
/// not whenever every property reads the same.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ComputedStyle {
    values: BTreeMap<Property, Value>,
    variables: Arc<BTreeMap<String, String>>,
    authored: BTreeSet<Property>,
    /// The properties a rule above the base tier wrote on this very element, not ones it
    /// inherited: an item's own `color`, against the `color` its bar gives all its items.
    declared: BTreeSet<Property>,
    /// The element's `animation` items with their keyframes resolved for it.
    keyframes: Vec<(crate::motion::Animation, Arc<crate::motion::Frames>)>,
}

impl ComputedStyle {
    pub fn get(&self, property: Property) -> Value {
        match self.values.get(&property) {
            Some(value) => value.clone(),
            // An initial `currentcolor` (a border's, an accent's) is the element's foreground.
            None => match property.initial() {
                Value::Color(Color::Current) if property != Property::Color => self.get(Property::Color),
                initial => initial,
            },
        }
    }
    /// Every custom property the element sees, resolved.
    pub fn variables(&self) -> &BTreeMap<String, String> {
        &self.variables
    }

    pub fn variable(&self, name: &str) -> Option<&str> {
        self.variables.get(name).map(String::as_str)
    }
    /// The custom properties, shared rather than copied.
    pub fn shared_variables(&self) -> Arc<BTreeMap<String, String>> {
        Arc::clone(&self.variables)
    }
    /// Host defaults must not replace a native widget's own appearance. Inherited
    /// authored properties retain this marker across the component tree, until a user-agent or
    /// base rule on an element sets the property again.
    pub fn authored(&self, property: Property) -> bool {
        self.authored.contains(&property)
    }
    /// Whether a rule above the base tier wrote `property` on this element itself. An inherited
    /// value is not declared, though it is [`authored`](Self::authored), and a declaration that
    /// repeats the inherited value is.
    pub fn declared(&self, property: Property) -> bool {
        self.declared.contains(&property)
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

/// The style of the last element of `tree`, whose parent's style is `parent` (`None` for a root):
/// one element's style worked out without the rest of the tree's, for a host that styles each
/// element as it is drawn. Selectors see its ancestors in `tree`; siblings it does not have.
pub fn last_style(
    tree: &Tree,
    parent: Option<&ComputedStyle>,
    viewport: Viewport,
    sheets: &[AppliedSheet<'_>],
) -> Result<ComputedStyle, String> {
    let node = tree.elements().len().checked_sub(1).ok_or("An empty style tree")?;
    element_style(tree, node, parent, viewport, sheets, false, &mut Budget::default())
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
    let mut budget = Budget::default();
    for (node, element) in tree.elements().iter().enumerate() {
        let parent = element.parent.and_then(|parent| result.get(parent));
        let computed = element_style(tree, node, parent, viewport, sheets, checked, &mut budget)?;
        result.push(computed);
    }
    Ok(result)
}

/// Most styles a [`StyleCache`] keeps; past it, it starts again empty.
pub const MAX_CACHED_STYLES: usize = 8192;

/// Element styles kept between calls of [`compute_shared`], by what each depends on (#1489).
///
/// An element's style depends only on its parent's style, its own identity (kind, id, classes,
/// states, attributes, object), the scoped sheets it is the root of, the viewport and the sheets;
/// and on its siblings only when a sheet relates them (`+`, `~`) or tests a place among them
/// (`:nth-child`), and on its children only when a sheet tests `:empty`. Each distinct identity
/// and each distinct place (all of these, with the parent's place standing for the parent's
/// style and everything above it) gets a number of its own, compared in full, never by a hash
/// alone: two elements share a style only when everything a selector or inheritance can see of
/// them is the same. A tree that changed in a few elements then works out only those and the
/// elements under them, and identical elements (rows, headers, buttons in the same place) share
/// one computation even the first time.
///
/// The cache empties when the viewport or the caller's `sheets_key` changes (the caller gives a
/// key that changes whenever any sheet's content does), and when it would hold more than
/// [`MAX_CACHED_STYLES`].
#[derive(Debug, Default)]
pub struct StyleCache {
    inputs: Option<(Viewport, u64)>,
    structure: Structure,
    /// The ids some selector tests: any other id cannot change a style, so it is no part of an
    /// identity (the host gives elements without one an id of their own, `node-…`, which would
    /// otherwise keep identical elements apart).
    tested_ids: std::collections::BTreeSet<String>,
    /// Each distinct identity, numbered.
    identities: std::collections::HashMap<Identity, u32>,
    /// Each distinct place, numbered, with its style.
    places: std::collections::HashMap<Place, (u64, Arc<ComputedStyle>)>,
    /// Each distinct run of earlier siblings, numbered: the run before the last of them, its
    /// identity's number, and with `:empty`, whether it has children.
    runs: std::collections::HashMap<(Option<u32>, u32, bool), u32>,
    computed: usize,
}

impl StyleCache {
    /// Forgets every style kept.
    pub fn clear(&mut self) {
        self.inputs = None;
        self.identities.clear();
        self.places.clear();
        self.runs.clear();
    }

    /// How many styles the last [`compute_shared`] worked out rather than took from the cache.
    pub fn computed(&self) -> usize {
        self.computed
    }

    /// How many styles are kept.
    pub fn len(&self) -> usize {
        self.places.len()
    }

    /// Whether no style is kept.
    pub fn is_empty(&self) -> bool {
        self.places.is_empty()
    }

    /// The number of `element`'s identity, given one if it is new.
    fn identity(&mut self, element: &crate::selector::Element) -> u32 {
        let identity = Identity {
            kind: element.kind.clone(),
            id: element.id.clone().filter(|id| self.tested_ids.contains(id)),
            classes: element.classes.clone(),
            states: element.states.clone(),
            attributes: element.attributes.clone(),
            object: element.object.clone(),
        };
        let next = u32::try_from(self.identities.len()).unwrap_or(u32::MAX);
        *self.identities.entry(identity).or_insert(next)
    }

    /// The number of the run of siblings `before` followed by one of identity `identity`.
    fn run(&mut self, before: Option<u32>, identity: u32, empty: bool) -> u32 {
        let next = u32::try_from(self.runs.len()).unwrap_or(u32::MAX);
        *self.runs.entry((before, identity, empty)).or_insert(next)
    }
}

/// What selectors and `attr()` can see of an element itself.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Identity {
    kind: String,
    id: Option<String>,
    classes: Vec<String>,
    states: Vec<String>,
    attributes: Vec<(String, String)>,
    object: Option<String>,
}

/// Everything an element's style depends on besides the viewport and the sheets, each part
/// compared in full ([`StyleCache`]).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Place {
    /// The parent's place's number: its style, and its ancestors' identities, scopes and places.
    parent: Option<u64>,
    /// The element's identity's number.
    identity: u32,
    /// The scoped sheets (device panels) whose root is this element, by their index.
    roots: Vec<usize>,
    /// With a sheet that tests places among siblings: this one, and how many siblings there are.
    position: Option<(usize, usize)>,
    /// With a sheet that relates siblings: the run of the earlier siblings' identities (and with
    /// `:empty`, whether each has children), by its number; `None` for a first child.
    earlier: Option<Option<u32>>,
    /// With a sheet that tests `:empty`: whether the element has children.
    empty: Option<bool>,
}

/// What of an element's surroundings the sheets' selectors test, apart from its ancestors.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Structure {
    /// `+` or `~`: the earlier siblings themselves.
    siblings: bool,
    /// `:nth-child` and its kin: the element's place and how many siblings it has.
    position: bool,
    /// `:empty`: whether the element has children.
    children: bool,
}

impl Structure {
    fn of(sheets: &[AppliedSheet<'_>]) -> Self {
        let mut structure = Self::default();
        for selector in sheets.iter().flat_map(|applied| applied.sheet.rules.iter()).flat_map(|rule| &rule.selectors) {
            structure.siblings |= selector.relates_siblings();
            structure.position |= selector.tests_position();
            structure.children |= selector.tests_children();
        }
        structure
    }
}

/// The elements the rules of `sheets` test for `state` (`hover`), by kind or id
/// ([`crate::selector::Selector::state_names`]): a host that gives a state to the ancestors of
/// the element it is on (CSS's `:hover`) gives it only to these, so a pointer moving between
/// elements does not change every element above them.
pub fn state_names(sheets: &[AppliedSheet<'_>], state: &str) -> crate::selector::StateNames {
    let mut names = crate::selector::StateNames::default();
    for selector in sheets.iter().flat_map(|applied| applied.sheet.rules.iter()).flat_map(|rule| &rule.selectors) {
        selector.state_names(state, &mut names);
    }
    names
}

/// [`compute`] with the styles shared: each element's style with the number of its place in
/// `cache` ([`StyleCache`]), the same for two elements only when their styles are. `sheets_key`
/// must change whenever the content of any of `sheets` does.
pub fn compute_shared(
    tree: &Tree,
    viewport: Viewport,
    sheets: &[AppliedSheet<'_>],
    sheets_key: u64,
    cache: &mut StyleCache,
) -> Result<Vec<(u64, Arc<ComputedStyle>)>, String> {
    if !viewport.valid() || sheets.len() > 32 {
        return Err("Invalid style viewport or sheet count".into());
    }
    if sheets.iter().any(|sheet| sheet.root.is_some_and(|root| tree.elements().get(root).is_none())) {
        return Err("Invalid style scope root".into());
    }
    let elements = tree.elements();
    // A tree whose new places might not fit starts the cache again before any is numbered, so
    // the numbers this call hands out stay the places they were given for.
    if cache.inputs != Some((viewport, sheets_key)) || cache.places.len() + elements.len() > MAX_CACHED_STYLES {
        let structure = Structure::of(sheets);
        cache.clear();
        cache.structure = structure;
        cache.tested_ids.clear();
        for selector in sheets.iter().flat_map(|applied| applied.sheet.rules.iter()).flat_map(|rule| &rule.selectors) {
            selector.ids(&mut cache.tested_ids);
        }
        cache.inputs = Some((viewport, sheets_key));
    }
    cache.computed = 0;
    let structure = cache.structure;
    let identities: Vec<u32> = elements.iter().map(|element| cache.identity(element)).collect();
    // Each element's place among its siblings and the run of those before it, in one pass over
    // each parent's children: only when a sheet tests them, and never by searching a list.
    let mut places: Vec<(usize, usize, Option<u32>)> = Vec::new();
    if structure.position || structure.siblings {
        places = vec![(0, 1, None); elements.len()];
        let roots: Vec<usize> =
            elements.iter().enumerate().filter(|(_, element)| element.parent.is_none()).map(|(node, _)| node).collect();
        let groups = std::iter::once(roots.as_slice()).chain((0..elements.len()).map(|node| tree.children(node)));
        for group in groups {
            let mut before = None;
            for (at, &node) in group.iter().enumerate() {
                if let Some(slot) = places.get_mut(node) {
                    *slot = (at, group.len(), before);
                }
                if structure.siblings {
                    let identity = identities.get(node).copied().unwrap_or(u32::MAX);
                    let empty = structure.children && tree.children(node).is_empty();
                    before = Some(cache.run(before, identity, empty));
                }
            }
        }
    }
    let mut result: Vec<(u64, Arc<ComputedStyle>)> = Vec::with_capacity(elements.len());
    let mut budget = Budget::default();
    for (node, element) in elements.iter().enumerate() {
        let parent = element.parent.and_then(|parent| result.get(parent));
        let (at, count, before) = places.get(node).copied().unwrap_or((0, 1, None));
        let place = Place {
            parent: parent.map(|(number, _)| *number),
            identity: identities.get(node).copied().unwrap_or(u32::MAX),
            roots: sheets
                .iter()
                .enumerate()
                .filter(|(_, applied)| applied.root == Some(node))
                .map(|(index, _)| index)
                .collect(),
            position: structure.position.then_some((at, count)),
            earlier: structure.siblings.then_some(before),
            empty: structure.children.then(|| tree.children(node).is_empty()),
        };
        if let Some((number, style)) = cache.places.get(&place) {
            result.push((*number, Arc::clone(style)));
            continue;
        }
        let style = Arc::new(element_style(
            tree,
            node,
            parent.map(|(_, style)| style.as_ref()),
            viewport,
            sheets,
            false,
            &mut budget,
        )?);
        cache.computed += 1;
        let number = cache.places.len() as u64;
        cache.places.insert(place, (number, Arc::clone(&style)));
        result.push((number, style));
    }
    Ok(result)
}

/// The work and memory one computation may spend, over all the elements it styles.
pub(crate) struct Budget {
    work: usize,
    variable_bytes: usize,
}

impl Default for Budget {
    fn default() -> Self {
        Self { work: 2_000_000, variable_bytes: 0 }
    }
}

/// The style of one element of `tree`, given its parent's computed style.
pub(crate) fn element_style(
    tree: &Tree,
    node: usize,
    parent: Option<&ComputedStyle>,
    viewport: Viewport,
    sheets: &[AppliedSheet<'_>],
    checked: bool,
    budget: &mut Budget,
) -> Result<ComputedStyle, String> {
    let element = tree.elements().get(node).ok_or("Invalid style element")?;
    let Budget { work, variable_bytes } = budget;
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
    let mut winners: BTreeMap<&str, (Priority, &Declaration)> = BTreeMap::new();
    let mut order = 0usize;
    for applied in sheets {
        if applied.root.is_some_and(|root| !tree.contains(node, root)) {
            continue;
        }
        for rule in &applied.sheet.rules {
            order += 1;
            *work = work.checked_sub(1).ok_or("Style cascade exceeded its work budget")?;
            if !rule.media.iter().all(|media| media.matches(viewport)) {
                continue;
            }
            let specificity = rule
                .selectors
                .iter()
                .filter_map(|selector| {
                    selector.matches_with_budget(tree, node, applied.root, work).then_some(selector.specificity)
                })
                .max();
            if *work == 0 {
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
        let custom = winners.iter().map(|(name, (_, declaration))| (*name, *declaration));
        let resolved = cascade_variables(&computed.variables, custom, sheets, Some(element), checked)?;
        *variable_bytes = variable_bytes
            .checked_add(resolved.values().map(String::len).sum::<usize>())
            .filter(|bytes| *bytes <= 16 * 1024 * 1024)
            .ok_or("Computed variables exceed 16 MiB")?;
        computed.variables = Arc::new(resolved);
    }
    for (name, (priority, declaration)) in winners {
        let Some(property) = Property::from_name(name) else {
            continue;
        };
        let value = match crate::variables::substitute_attributes(&declaration.value, element)
            .and_then(|value| crate::variables::substitute(&value, &computed.variables))
            .and_then(|value| match declaration.shorthand {
                Some(shorthand) => shorthand.part(property, &value),
                None => Ok(value),
            }) {
            Ok(value) => value,
            Err(error) if checked => return Err(error),
            Err(_) => continue,
        };
        let value = match value.trim() {
            "inherit" => parent.map_or_else(|| property.initial(), |parent| parent.get(property)),
            "initial" => property.initial(),
            "unset" if property.inherits() => parent.map_or_else(|| property.initial(), |parent| parent.get(property)),
            "unset" => property.initial(),
            other => match property.parse(&crate::value::viewport_units(other, viewport.width, viewport.height)) {
                Ok(value) => value,
                Err(error) if checked => return Err(error),
                Err(_) => continue,
            },
        };
        computed.values.insert(property, value);
        // A user-agent or base rule on the element replaces what it inherited from a sheet's rule
        // above it: its value is Suhina's, not one a theme wrote (`knob-body` under a styled
        // `knob`).
        if priority.0 > Layer::Base as u8 {
            computed.authored.insert(property);
            computed.declared.insert(property);
        } else {
            computed.authored.remove(&property);
            computed.declared.remove(&property);
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
                    *work = work.checked_sub(1).ok_or("Style cascade exceeded its work budget")?;
                    let Some(property) = Property::from_name(&declaration.name) else { continue };
                    let value = crate::variables::substitute_attributes(&declaration.value, element)
                        .and_then(|value| crate::variables::substitute(&value, &computed.variables))
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
            // A keyframe's property is written on this very element: it is also declared.
            for (_, values) in &resolved {
                for (property, _) in values {
                    computed.authored.insert(*property);
                    computed.declared.insert(*property);
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
        Value::Length(crate::value::Length::Em(scale) | crate::value::Length::Percent(scale)) => parent_size * scale,
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
    Ok(computed)
}

/// Applies the winning custom-property declarations to the `inherited` ones and resolves them.
/// `attr()` reads `element`'s attributes; without an element it makes a declaration invalid.
fn cascade_variables<'a>(
    inherited: &BTreeMap<String, String>,
    winners: impl Iterator<Item = (&'a str, &'a Declaration)>,
    sheets: &[AppliedSheet<'_>],
    element: Option<&crate::selector::Element>,
    checked: bool,
) -> Result<BTreeMap<String, String>, String> {
    let mut variables = inherited.clone();
    for (name, declaration) in winners.filter(|(name, _)| name.starts_with("--")) {
        match declaration.value.trim() {
            "initial" => {
                match sheets
                    .iter()
                    .rev()
                    .flat_map(|applied| applied.sheet.properties.iter())
                    .find(|registered| registered.name == name)
                    .and_then(|registered| registered.initial.clone())
                {
                    Some(initial) => variables.insert(name.into(), initial),
                    None => variables.remove(name),
                };
            }
            "inherit" | "unset" => {}
            value => {
                let substituted = match element {
                    Some(element) => crate::variables::substitute_attributes(value, element),
                    None if crate::variables::mentions_attr(value) => Err("attr() needs an element".into()),
                    None => Ok(value.to_owned()),
                };
                match substituted {
                    Ok(value) => {
                        variables.insert(name.into(), value);
                    }
                    // Invalid at computed-value time: the inherited value stays.
                    Err(error) if checked => return Err(error),
                    Err(_) => {}
                }
            }
        }
    }
    let mut resolved = crate::variables::resolve(&variables)?;
    // A registered property that substitution dropped (a cycle, or a `var()` with nothing to
    // stand for it) is invalid at computed-value time: it takes its initial value, and the
    // variables that read it are resolved again with that value.
    let mut dropped = false;
    for registered in sheets.iter().flat_map(|applied| applied.sheet.properties.iter()) {
        if variables.contains_key(&registered.name) && !resolved.contains_key(&registered.name) {
            match &registered.initial {
                Some(initial) => variables.insert(registered.name.clone(), initial.clone()),
                None => variables.remove(&registered.name),
            };
            dropped = true;
        }
    }
    if dropped {
        resolved = crate::variables::resolve(&variables)?;
    }
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
    Ok(resolved)
}

/// The custom properties the root element sees, from the sheets' registered initial values
/// and their `:root` rules alone: no element tree, and no other rule is matched. Rules inside
/// `@media` use `viewport`; sheets scoped to a device panel are left out.
pub fn root_variables(viewport: Viewport, sheets: &[AppliedSheet<'_>]) -> Result<BTreeMap<String, String>, String> {
    if !viewport.valid() || sheets.len() > 32 {
        return Err("Invalid style viewport or sheet count".into());
    }
    let initial: BTreeMap<String, String> = sheets
        .iter()
        .flat_map(|applied| applied.sheet.properties.iter())
        .filter_map(|registered| Some((registered.name.clone(), registered.initial.clone()?)))
        .collect();
    let mut winners: BTreeMap<&str, (Priority, &Declaration)> = BTreeMap::new();
    let mut order = 0usize;
    for applied in sheets.iter().filter(|applied| applied.root.is_none()) {
        for rule in &applied.sheet.rules {
            order += 1;
            if !rule.media.iter().all(|media| media.matches(viewport)) {
                continue;
            }
            let Some(specificity) =
                rule.selectors.iter().filter(|selector| selector.is_root()).map(|selector| selector.specificity).max()
            else {
                continue;
            };
            for declaration in rule.declarations.iter().filter(|declaration| declaration.name.starts_with("--")) {
                let priority = priority(applied.layer, declaration.important, rule.layer, specificity, order);
                if winners.get(declaration.name.as_str()).is_none_or(|(previous, _)| *previous <= priority) {
                    winners.insert(&declaration.name, (priority, declaration));
                }
            }
        }
    }
    let custom = winners.iter().map(|(name, (_, declaration))| (*name, *declaration));
    cascade_variables(&initial, custom, sheets, None, false)
}

/// Resolves a `<color>` value (a theme's swatch) for the given theme and variant sheets
/// without applying them: `var()` takes the [`root_variables`]. With no element to read,
/// `currentColor` and `attr()` are refused, and so is a value that is not a colour.
pub fn resolve_color(value: &str, viewport: Viewport, sheets: &[AppliedSheet<'_>]) -> Result<[u8; 4], String> {
    if crate::variables::mentions_attr(value) {
        return Err("A colour without an element cannot use attr()".into());
    }
    let variables = root_variables(viewport, sheets)?;
    let substituted = crate::variables::substitute(value, &variables)?;
    if crate::variables::mentions_attr(&substituted) {
        return Err("A colour without an element cannot use attr()".into());
    }
    match Color::parse(substituted.trim())? {
        Color::Rgba(rgba) => Ok(rgba),
        Color::Current => Err("A colour without an element cannot use currentColor".into()),
    }
}

#[cfg(test)]
#[path = "cascade_tests.rs"]
mod tests;
