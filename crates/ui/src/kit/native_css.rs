// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Stylo computed styles and Taffy layout, consumed by native egui controls.
//!
//! The host supplies a bounded tree; themes never supply HTML or executable code.
//! No resource provider is installed. This module is UI-thread-only.
use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use blitz_dom::{DocumentConfig, StyleThreading};
use blitz_html::HtmlDocument;
use blitz_traits::net::DummyNetProvider;
use blitz_traits::shell::{ColorScheme, Viewport};
use egui::{Color32, Rect, Vec2};

use super::{skin, theme};
mod diagnostics;
mod geometry;
mod native;
mod paint;
mod shadows;
mod text;
mod workspace;
pub use diagnostics::{Notice, inspect};
pub use paint::{background, background_rounded, background_with, level_fill, level_glow};
pub use text::paint_text;
pub use workspace::{Workspace, workspace};

const MAX_CONTEXTS: usize = 128;
const MAX_DEPTH: usize = 16;
const MAX_NODES: usize = 4096;

/// Styles exposed by native painters. Parsed CSS support is broader than this paint adapter.
#[derive(Clone, Debug)]
pub struct Computed {
    pub color: Color32,
    pub background: Color32,
    pub border: Color32,
    pub font_size: f32,
    pub radius: f32,
    pub opacity: f32,
    pub visible: bool,
    pub authored: Authored,
    gradients: Arc<[paint::Gradient]>,
    shadows: Arc<[shadows::Shadow]>,
    text_shadows: Arc<[text::Shadow]>,
}
impl Computed {
    pub fn has_gradient(&self) -> bool {
        !self.gradients.is_empty()
    }
}

/// Only authored properties override native defaults. An unrelated CSS rule must not
/// replace every label's native colour, size or corner radius with browser defaults.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Authored {
    pub color: bool,
    pub background: bool,
    pub border: bool,
    pub font_size: bool,
    pub radius: bool,
}

#[derive(Default)]
struct Engine {
    source: Option<Arc<str>>,
    palette: String,
    size: [u32; 2],
    dark: bool,
    contexts: HashMap<Vec<String>, Computed>,
    toolbar: Option<(String, Vec<Module>)>,
    workspace: Option<(String, Option<Workspace>)>,
    toolbar_layout: bool,
    collecting: bool,
    nodes: Vec<StyleNode>,
    parents: Vec<usize>,
    occurrences: HashMap<(u64, String), usize>,
    previous: Vec<StyleNode>,
    values: HashMap<u64, Computed>,
    document: Option<HtmlDocument>,
    native: Option<native::Paint>,
    motion: bool,
    styles_changed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct StyleNode {
    key: u64,
    parent: Option<usize>,
    class: String,
    name: Option<String>,
    state: u8,
}
thread_local! {
    static ENGINE: RefCell<Engine> = RefCell::new(Engine::default());
}

/// Finishes the style snapshot even on an early return from the native frame.
pub struct Frame(egui::Context, f64);
impl Drop for Frame {
    fn drop(&mut self) {
        let (changed, animating) = suhina_plugins::trace::timed("native CSS tree", || finish_tree(self.1));
        if animating {
            self.0.request_repaint_after(std::time::Duration::from_millis(16));
        } else if changed {
            self.0.request_repaint();
        }
    }
}

pub fn frame(ui: &egui::Ui) -> Frame {
    frame_with_motion(ui, true)
}

/// Disabling motion suppresses CSS transitions/keyframes as well as extra ticks.
pub fn frame_with_motion(ui: &egui::Ui, motion: bool) -> Frame {
    begin_frame(ui);
    ENGINE.with(|engine| {
        let mut engine = engine.borrow_mut();
        if engine.motion != motion {
            engine.motion = motion;
            engine.document = None;
            engine.previous.clear();
            engine.values.clear();
        }
        engine.collecting = engine.source.is_some();
        engine.nodes.clear();
        engine.parents.clear();
        engine.occurrences.clear();
    });
    Frame(ui.ctx().clone(), ui.input(|input| input.time))
}

/// Register an actual native scope. Named project roles retain identity when reordered.
pub fn enter(class: &str, name: Option<&str>, response: Option<&egui::Response>) -> bool {
    ENGINE.with(|engine| {
        let mut engine = engine.borrow_mut();
        if !engine.collecting || engine.nodes.len() >= MAX_NODES || engine.parents.len() >= MAX_DEPTH {
            return false;
        }
        if !super::style_class::CLASSES.contains(&class) {
            return false;
        }
        let name = name.filter(|name| {
            name.len() <= 96 && name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        });
        let parent = engine.parents.last().copied();
        let parent_key = parent.and_then(|p| engine.nodes.get(p)).map_or(0, |n| n.key);
        let occurrence = engine.occurrences.entry((parent_key, class.into())).or_default();
        let index = *occurrence;
        *occurrence += 1;
        let mut hash = std::hash::DefaultHasher::new();
        (parent_key, class, name).hash(&mut hash);
        if let Some(response) = response {
            response.id.hash(&mut hash);
        } else if name.is_none() {
            index.hash(&mut hash);
        }
        let key = hash.finish();
        let state = response.map_or(0, |r| {
            u8::from(r.hovered())
                | u8::from(r.is_pointer_button_down_on()) << 1
                | u8::from(!r.enabled()) << 2
                | u8::from(r.has_focus()) << 3
        });
        let node = StyleNode { key, parent, class: class.into(), name: name.map(str::to_owned), state };
        let index = engine.nodes.len();
        engine.nodes.push(node);
        engine.parents.push(index);
        true
    })
}

pub fn leave() {
    ENGINE.with(|engine| {
        engine.borrow_mut().parents.pop();
    });
}

fn html_id(node: &StyleNode) -> String {
    if node.class == "button"
        && let Some(name) = node
            .name
            .as_deref()
            .filter(|name| ["play", "stop", "record", "browser-toggle", "metronome-toggle"].contains(name))
    {
        return format!("control-{name}");
    }
    if (node.class == "workspace" && node.name.as_deref() == Some("workspace"))
        || (node.class == "workspace-panel"
            && node.name.as_deref().is_some_and(|name| ["panel-browser", "panel-main", "panel-detail"].contains(&name)))
    {
        return node.name.clone().unwrap_or_default();
    }
    if node.class == "toolbar-module" && node.name.as_deref().is_some_and(|name| skin::MODULES.contains(&name)) {
        node.name.clone().unwrap_or_default()
    } else {
        format!("node-{}", node.key)
    }
}

fn native_state(state: u8) -> style_dom::ElementState {
    use style_dom::ElementState as S;
    let mut result = if state & 4 != 0 { S::DISABLED } else { S::ENABLED };
    for (bit, value) in [(1, S::HOVER), (2, S::ACTIVE), (8, S::FOCUS | S::FOCUSRING)] {
        if state & bit != 0 {
            result.insert(value);
        }
    }
    result
}

fn finish_tree(now: f64) -> (bool, bool) {
    ENGINE.with(|engine| {
        let mut engine = engine.borrow_mut();
        engine.collecting = false;
        let changed = engine.nodes != engine.previous;
        let animating_before = engine.document.as_ref().is_some_and(|doc| doc.is_animating());
        if engine.source.is_none()
            || (!changed
                && !engine.styles_changed
                && !animating_before
                && (engine.document.is_some() || (engine.native.is_some() && !engine.values.is_empty())))
        {
            return (false, false);
        }
        if engine.native.is_some() {
            if let Some(values) = suhina_plugins::trace::timed("style paint resolve", || native::retained(&engine)) {
                engine.values = values.into_iter().collect();
                engine.previous = engine.nodes.clone();
                engine.styles_changed = false;
                engine.document = None;
                return (true, false);
            }
            // A substituted value may require an unsupported feature. Switch the
            // entire sheet back, never publish a partly computed native snapshot.
            engine.native = None;
        }
        let same_tree = engine.nodes.len() == engine.previous.len()
            && engine
                .nodes
                .iter()
                .zip(&engine.previous)
                .all(|(a, b)| a.key == b.key && a.parent == b.parent && a.class == b.class && a.name == b.name);
        let styles_changed = engine.styles_changed;
        let retained = if same_tree { engine.document.take() } else { None };
        let mut doc = if let Some(doc) = retained {
            doc
        } else {
            let mut body = String::from("<!doctype html><html><body>");
            let mut open = Vec::new();
            for (index, node) in engine.nodes.iter().enumerate() {
                while open.last().copied() != node.parent {
                    if open.pop().is_none() {
                        break;
                    }
                    body.push_str("</div>");
                }
                body.push_str(&format!("<div id=\"{}\" class=\"{}\"", html_id(node), node.class));
                if let Some(name) = &node.name {
                    body.push_str(&format!(" data-object=\"{name}\""));
                }
                body.push('>');
                open.push(index);
            }
            for _ in open {
                body.push_str("</div>");
            }
            body.push_str("</body></html>");
            let defaults = format!("html,body {{ margin:0; color:var(--text-primary); font-size:{}px; }}", theme::TEXT);
            let Some(doc) = document(&engine, &body, &defaults) else {
                return (false, false);
            };
            doc
        };
        let mut states: Vec<_> = engine.nodes.iter().map(|node| node.state).collect();
        for node in &engine.nodes {
            if node.state & 1 == 0 {
                continue;
            }
            let mut parent = node.parent;
            while let Some(index) = parent {
                let Some(node) = engine.nodes.get(index) else {
                    break;
                };
                if let Some(state) = states.get_mut(index) {
                    *state |= 1;
                }
                parent = node.parent;
            }
        }
        let mut ids = Vec::new();
        for (index, node) in engine.nodes.iter().enumerate() {
            let state = states.get(index).copied().unwrap_or(0);
            let Some(id) = doc.get_element_by_id(&html_id(node)) else {
                continue;
            };
            let desired = native_state(state);
            let mask = style_dom::ElementState::HOVER
                | style_dom::ElementState::ACTIVE
                | style_dom::ElementState::DISABLED
                | style_dom::ElementState::ENABLED
                | style_dom::ElementState::FOCUS
                | style_dom::ElementState::FOCUSRING;
            if doc.get_node(id).is_some_and(|target| target.element_state().intersection(mask) != desired) {
                doc.snapshot_node_state_only(id);
                if let Some(target) = doc.get_node_mut(id) {
                    target.element_state_mut().remove(mask);
                    target.element_state_mut().insert(desired);
                    target.mark_ancestors_dirty();
                }
            }
            ids.push((node.key, id));
        }
        suhina_plugins::trace::timed("legacy CSS resolve", || doc.resolve(now));
        let values = ids.into_iter().filter_map(|(key, id)| computed(&doc, id).map(|values| (key, values))).collect();
        engine.values = values;
        engine.previous = engine.nodes.clone();
        let animating = engine.motion && doc.is_animating();
        engine.document = Some(doc);
        engine.styles_changed = false;
        // Publish the final sample too: once the animation stops, the preceding
        // frame still painted its penultimate values.
        (changed || styles_changed || animating || animating_before, animating)
    })
}

/// Refresh only when the authored CSS, palette or viewport changes.
pub fn begin_frame(ui: &egui::Ui) {
    let source = skin::current().css.clone();
    if source.is_none() {
        ENGINE.with(|engine| {
            if engine.borrow().source.is_some() {
                *engine.borrow_mut() = Engine::default();
            }
        });
        return;
    }
    let size = ui.ctx().input(|i| i.content_rect().size());
    let size = [size.x.clamp(1.0, 16384.0) as u32, size.y.clamp(1.0, 16384.0) as u32];
    let palette = theme::palette();
    let mut variables = String::from(":root {");
    for (key, _, color) in palette.tokens() {
        let variable = key.replace('.', "-");
        // Only referenced palette variables affect this stylesheet. Resetting all
        // style/layout caches for an unrelated opacity tick made native controls
        // rebuild many complete Stylo documents during every drag frame.
        // Escaped identifiers are conservatively treated as depending on all tokens.
        if key != "text.primary"
            && !source.as_deref().is_some_and(|css| css.contains('\\') || css.contains(&format!("--{variable}")))
        {
            continue;
        }
        variables.push_str(&format!("--{variable}:{};", theme::hex(color)));
    }
    variables.push_str("}\n");
    let dark = theme::contrast(palette.window_background, Color32::WHITE)
        > theme::contrast(palette.window_background, Color32::BLACK);
    ENGINE.with(|engine| {
        let mut engine = engine.borrow_mut();
        if engine.source != source {
            let native = suhina_plugins::trace::timed("style paint parse", || {
                source.as_deref().and_then(|source| native::Paint::new(source, &variables))
            });
            *engine = Engine { source, palette: variables, size, dark, native, ..Default::default() };
        } else if engine.size != size || engine.dark != dark || engine.palette != variables {
            // Palette drags and native resizing do not change the scope tree or
            // authored CSS. Keep its document and last computed snapshot so that
            // every control does not fall back to building its own Stylo document.
            // The retained document resolves the new inputs at the end of this pass.
            let previous_palette = engine.palette.clone();
            let viewport_changed = engine.size != size || engine.dark != dark;
            if let Some(doc) = engine.document.as_mut() {
                if previous_palette != variables {
                    doc.remove_user_agent_stylesheet(&previous_palette);
                    doc.add_user_agent_stylesheet(&variables);
                }
                if viewport_changed {
                    doc.set_viewport(Viewport::new(
                        size[0],
                        size[1],
                        1.0,
                        if dark { ColorScheme::Dark } else { ColorScheme::Light },
                    ));
                }
            }
            engine.palette = variables;
            let palette = engine.palette.clone();
            if engine.native.as_mut().is_some_and(|paint| paint.palette(&palette).is_none()) {
                engine.native = None;
            }
            engine.size = size;
            engine.dark = dark;
            engine.styles_changed = true;
            engine.contexts.clear();
            engine.toolbar = None;
            engine.workspace = None;
        }
    });
}

fn document(engine: &Engine, body: &str, defaults: &str) -> Option<HtmlDocument> {
    let css = engine.source.as_ref()?;
    if inspect(css).is_err() {
        return None;
    }
    // Stylesheets are supplied through the CSS API, never interpolated into HTML.
    let config = DocumentConfig {
        viewport: Some(Viewport::new(
            engine.size[0],
            engine.size[1],
            1.0,
            if engine.dark { ColorScheme::Dark } else { ColorScheme::Light },
        )),
        net_provider: Some(Arc::new(DummyNetProvider)),
        style_threading: StyleThreading::Sequential,
        ..Default::default()
    };
    let mut doc = HtmlDocument::from_html(body, config);
    doc.add_user_agent_stylesheet(defaults);
    doc.add_user_agent_stylesheet(&engine.palette);
    if !engine.motion {
        doc.add_user_agent_stylesheet("* { animation:none !important; transition:none !important; }");
    }
    let sheet = doc.make_stylesheet(css.as_ref(), style::stylesheets::Origin::Author);
    let root = doc.try_root_element()?.id;
    doc.add_stylesheet_for_node(sheet, root);
    Some(doc)
}

fn rgba(color: &style::color::AbsoluteColor) -> Color32 {
    let [r, g, b, a] = rgba_bytes(color);
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

fn rgba_bytes(color: &style::color::AbsoluteColor) -> [u8; 4] {
    let srgb = color.to_color_space(style::color::ColorSpace::Srgb);
    let channels = srgb.raw_components();
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    [byte(channels[0]), byte(channels[1]), byte(channels[2]), byte(channels[3])]
}

// Compatibility boundary only. Native painters have no Stylo types.
fn legacy_shadow(
    shadow: &style::values::computed::effects::SimpleShadow,
    foreground: &style::color::AbsoluteColor,
) -> suhina_style::paint::ResolvedShadow {
    suhina_style::paint::ResolvedShadow {
        offset: [shadow.horizontal.px(), shadow.vertical.px()],
        blur: shadow.blur.0.px(),
        spread: 0.0,
        color: rgba_bytes(&shadow.color.resolve_to_absolute(foreground)),
        inset: false,
    }
}

fn authored_property(
    doc: &HtmlDocument,
    id: blitz_dom::NodeId,
    property: style::properties::LonghandId,
    inherited: bool,
) -> bool {
    use style::properties::PropertyDeclarationId;
    use style::rule_tree::CascadeOrigin;
    let guard = doc.guard().read();
    let mut current = Some(id);
    for _ in 0..MAX_DEPTH + 3 {
        let Some(node) = current.and_then(|id| doc.get_node(id)) else {
            break;
        };
        if let Some(values) = node.primary_styles()
            && let Some(rules) = &values.rules
        {
            for rule in rules.self_and_ancestors() {
                if !matches!(
                    rule.cascade_level().origin(),
                    CascadeOrigin::Author | CascadeOrigin::Animations | CascadeOrigin::Transitions
                ) {
                    continue;
                }
                if let Some(source) = rule.style_source()
                    && source.read(&guard).contains(PropertyDeclarationId::Longhand(property))
                {
                    return true;
                }
            }
        }
        if !inherited {
            break;
        }
        current = node.parent;
    }
    false
}

fn computed(doc: &HtmlDocument, id: blitz_dom::NodeId) -> Option<Computed> {
    use style::properties::{LonghandId, PropertyDeclarationId};
    let node = doc.get_node(id)?;
    let values = node.primary_styles()?;
    let color = values.clone_color();
    let background = values.clone_background_color().resolve_to_absolute(&color);
    let border = values.clone_border_top_color().resolve_to_absolute(&color);
    let radius = values.computed_value_to_string(PropertyDeclarationId::Longhand(LonghandId::BorderTopLeftRadius));
    let radius = radius
        .split_whitespace()
        .next()
        .and_then(|v| v.strip_suffix("px"))
        .and_then(|v| v.parse::<f32>().ok())
        .unwrap_or(0.0)
        .clamp(0.0, 128.0);
    let serialized = values.computed_value_to_string(PropertyDeclarationId::Longhand(LonghandId::BackgroundImage));
    let mut hash = std::hash::DefaultHasher::new();
    serialized.hash(&mut hash);
    rgba(&color).hash(&mut hash);
    let key = hash.finish();
    let gradients = values
        .get_background()
        .background_image
        .0
        .iter()
        .take(4)
        .enumerate()
        .filter_map(|(index, image)| {
            if let style::values::computed::Image::Gradient(gradient) = image {
                paint::Gradient::from_css(gradient, &color, key.wrapping_add(index as u64))
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .into();
    Some(Computed {
        gradients,
        text_shadows: values
            .clone_text_shadow()
            .0
            .iter()
            .take(4)
            .map(|shadow| text::Shadow::from_native(&legacy_shadow(shadow, &color)))
            .collect::<Vec<_>>()
            .into(),
        shadows: values
            .clone_box_shadow()
            .0
            .iter()
            .take(4)
            .map(|shadow| {
                let mut value = legacy_shadow(&shadow.base, &color);
                value.spread = shadow.spread.px();
                value.inset = shadow.inset;
                shadows::Shadow::from_native(&value)
            })
            .collect::<Vec<_>>()
            .into(),
        authored: Authored {
            color: authored_property(doc, id, LonghandId::Color, true),
            background: authored_property(doc, id, LonghandId::BackgroundColor, false),
            border: authored_property(doc, id, LonghandId::BorderTopColor, false),
            font_size: authored_property(doc, id, LonghandId::FontSize, true),
            radius: authored_property(doc, id, LonghandId::BorderTopLeftRadius, false),
        },
        visible: values.computed_value_to_string(PropertyDeclarationId::Longhand(LonghandId::Visibility)) == "visible",
        color: rgba(&color),
        background: rgba(&background),
        border: rgba(&border),
        font_size: values.clone_font_size().computed_size().px().clamp(7.0, 96.0),
        radius,
        opacity: values.clone_opacity().clamp(0.0, 1.0),
    })
}

/// Compute inherited colours and native typography for the active class ancestry.
/// Sibling/object selectors require the host's complete retained component tree.
pub fn scoped(stack: &[String]) -> Option<Computed> {
    if stack.is_empty()
        || stack.len() > MAX_DEPTH
        || stack.iter().any(|s| !super::style_class::CLASSES.contains(&s.as_str()))
    {
        return None;
    }
    ENGINE.with(|engine| {
        let mut engine = engine.borrow_mut();
        engine.source.as_ref()?;
        if engine.source != skin::current().css {
            return None;
        }
        if let Some(values) =
            engine.parents.last().and_then(|at| engine.nodes.get(*at)).and_then(|node| engine.values.get(&node.key))
        {
            return Some(values.clone());
        }
        if let Some(values) = engine.contexts.get(stack) {
            return Some(values.clone());
        }
        if engine.contexts.len() >= MAX_CONTEXTS {
            return None;
        }
        if engine.native.is_some() {
            if let Some(values) = native::scoped(&engine, stack) {
                engine.contexts.insert(stack.to_vec(), values.clone());
                return Some(values);
            }
            engine.native = None;
        }
        let mut body = String::from("<!doctype html><html><body>");
        for (index, class) in stack.iter().enumerate() {
            body.push_str(&format!("<div class=\"{class}\" id=\"scope-{index}\">"));
        }
        for _ in stack {
            body.push_str("</div>");
        }
        body.push_str("</body></html>");
        let defaults = format!("html,body {{ margin:0; color:var(--text-primary); font-size:{}px; }}", theme::TEXT);
        let mut doc = document(&engine, &body, &defaults)?;
        doc.resolve(0.0);
        let id = doc.get_element_by_id(&format!("scope-{}", stack.len() - 1))?;
        let values = computed(&doc, id)?;
        engine.contexts.insert(stack.to_vec(), values.clone());
        Some(values)
    })
}

/// One real toolbar child. Drawing and input use this same Taffy rectangle.
#[derive(Clone, Debug)]
pub struct Module {
    pub name: String,
    pub rect: Rect,
    pub style: Computed,
}

pub fn toolbar_layout_requested() -> bool {
    ENGINE.with(|engine| engine.borrow().toolbar_layout)
}
fn requests_layout(doc: &HtmlDocument, id: blitz_dom::NodeId) -> bool {
    use style::properties::LonghandId::*;
    [
        Display,
        Width,
        Height,
        Order,
        FlexDirection,
        FlexWrap,
        FlexGrow,
        FlexShrink,
        FlexBasis,
        JustifyContent,
        AlignItems,
        AlignSelf,
        GridTemplateColumns,
        GridTemplateRows,
        GridColumnStart,
        GridColumnEnd,
        GridRowStart,
        GridRowEnd,
        ColumnGap,
        RowGap,
        MarginLeft,
        MarginRight,
        MarginTop,
        MarginBottom,
        PaddingLeft,
        PaddingRight,
        PaddingTop,
        PaddingBottom,
        Visibility,
    ]
    .into_iter()
    .any(|property| authored_property(doc, id, property, false))
}

/// Flex/Grid layout for actual ordered, visible module ids. No theme-supplied markup.
pub fn toolbar(order: &[(String, f32)], size: Vec2) -> Option<Vec<Module>> {
    if order.len() > skin::MODULES.len() || order.iter().any(|(id, _)| !skin::MODULES.contains(&id.as_str())) {
        return None;
    }
    ENGINE.with(|engine| {
        let mut engine = engine.borrow_mut();
        engine.source.as_ref()?;
        if engine.source != skin::current().css { return None; }
        let key = format!("{order:?}:{size:?}");
        if let Some((previous, modules)) = &engine.toolbar && previous == &key {
            return Some(modules.clone());
        }
        let mut body = String::from("<!doctype html><html><body class=\"window\"><div class=\"toolbar\" id=\"toolbar\">");
        let mut defaults = format!("html,body {{ margin:0; }} .toolbar {{ display:flex; width:{}px; height:{}px; gap:4px; align-items:center; }} .toolbar-module {{ flex-shrink:0; height:{}px; }}", size.x.max(1.0),size.y.max(1.0),size.y.max(24.0));
        for (id, width) in order {
            body.push_str(&format!("<div class=\"toolbar-module {id}\" id=\"{id}\"></div>"));
            defaults.push_str(&format!("#{id} {{ width:{}px; }}",width.clamp(1.0,4096.0)));
        }
        body.push_str("</div></body></html>");
        let mut doc = document(&engine, &body, &defaults)?;
        doc.resolve(0.0);
        engine.toolbar_layout = doc.get_element_by_id("toolbar").is_some_and(|id| requests_layout(&doc,id))
            || order.iter().any(|(name,_)| doc.get_element_by_id(name).is_some_and(|id| requests_layout(&doc,id)));
        let mut modules = Vec::new();
        for (name, _) in order {
            let id = doc.get_element_by_id(name)?;
            let node = doc.get_node(id)?;
            let layout = node.final_layout();
            if !node.has_boxes() { continue; }
            let position = node.absolute_position(0.0,0.0);
            let rect = Rect::from_min_size(egui::pos2(position.x,position.y), Vec2::new(layout.size.width,layout.size.height));
            if !rect.is_finite() || rect.width() > 16384.0 || rect.height() > 16384.0 { return None; }
            let style=computed(&doc,id)?;
            if style.visible { modules.push(Module { name:name.clone(),rect,style }); }
        }
        engine.toolbar = Some((key,modules.clone()));
        Some(modules)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_linear_gradients_match_legacy_ramp_pixels() {
        for image in [
            "linear-gradient(red,blue)",
            "linear-gradient(to right, red, transparent)",
            "linear-gradient(145deg, #12345620, currentColor 70%, #000)",
            "linear-gradient(to top right, white 0 45%, green 45% 100%)",
            "repeating-linear-gradient(90deg, transparent 0 21px, #e5ece7 21px 22px)",
            "linear-gradient(red, green, blue), linear-gradient(90deg, #fff3, #0008)",
        ] {
            let css = format!(".button {{color:#34789a; background-image:{image};}}");
            prepare(&css, 800.0);
            let stack = ["button".into()];
            let native = scoped(&stack).unwrap();
            ENGINE.with(|engine| {
                let mut engine = engine.borrow_mut();
                assert!(engine.native.is_some(), "{image}");
                engine.native = None;
                engine.contexts.clear();
            });
            let legacy = scoped(&stack).unwrap();
            assert_eq!(native.gradients.len(), legacy.gradients.len());
            assert!(!native.gradients.is_empty());
            for (native, legacy) in native.gradients.iter().zip(legacy.gradients.iter()) {
                for basis in [17.0, 100.0, 480.0] {
                    let size = Vec2::new(basis, basis * 0.5);
                    assert!(
                        native
                            .geometry_samples(size)
                            .iter()
                            .zip(legacy.geometry_samples(size))
                            .all(|(a, b)| (a - b).abs() < 0.00001)
                    );
                    for (native, legacy) in native.pixel_samples(basis).iter().zip(legacy.pixel_samples(basis)) {
                        assert!(
                            native.to_array().iter().zip(legacy.to_array()).all(|(a, b)| a.abs_diff(b) <= 2),
                            "{image}: {native:?} != {legacy:?}"
                        );
                    }
                }
            }
        }
        skin::configure(&skin::Skin::default());
    }
    #[test]
    fn native_paint_matches_legacy_values_without_constructing_html() {
        for css in [
            ".button { color:#123456; background-color:#abcdef80; border-color:currentColor; border-radius:4px; opacity:.7; }",
            ":root { --ink:#2468ac; } .toolbar { color:var(--ink); font-size:12px; } .button { font-size:1.5em; border-radius:.25em; box-shadow:inset 0 1px 2px -1px #fff4, 1px 2px 3px currentColor; text-shadow:0 1px 1px #000a; }",
            ".toolbar { font-size:12px; color:rgba(12, 34, 56, .5); } @media (max-width:900px) { .button { color:#654321; } }",
            ".button { box-shadow:0 0 2px; text-shadow:1px 2px; }",
            ".toolbar { font-size:12px; text-shadow:1em 1em 1em currentColor; } .button { font-size:20px; color:red; }",
        ] {
            prepare(css, 800.0);
            let stack = ["toolbar".into(), "button".into()];
            let native = scoped(&stack).unwrap();
            ENGINE.with(|engine| {
                let mut engine = engine.borrow_mut();
                assert!(engine.native.is_some(), "{css}");
                assert!(engine.document.is_none());
                engine.native = None;
                engine.contexts.clear();
            });
            let legacy = scoped(&stack).unwrap();
            assert_eq!(native.color, legacy.color, "{css}");
            assert_eq!(native.background, legacy.background, "{css}");
            assert_eq!(native.border, legacy.border, "{css}");
            assert_eq!(native.radius, legacy.radius, "{css}");
            assert_eq!(native.font_size, legacy.font_size, "{css}");
            assert_eq!(native.opacity, legacy.opacity, "{css}");
            assert_eq!(native.authored, legacy.authored, "{css}");
            assert_eq!(native.shadows, legacy.shadows, "{css}");
            assert_eq!(native.text_shadows, legacy.text_shadows, "{css}");
        }
        skin::configure(&skin::Skin::default());
    }

    #[test]
    fn unsupported_native_expressions_keep_the_whole_legacy_style() {
        for css in [
            ".button { color:red; background-image:radial-gradient(blue,green); }",
            ":root { --ink:color-mix(in srgb, red, blue); } .button { color:var(--ink); }",
            ".button { color:blue; border-radius:4px 8px; }",
        ] {
            prepare(css, 800.0);
            let actual = scoped(&["button".into()]).unwrap();
            ENGINE.with(|engine| assert!(engine.borrow().native.is_none(), "{css}"));
            assert!(actual.authored.color);
            if css.contains("radial-gradient") {
                assert!(actual.has_gradient());
            }
        }
        skin::configure(&skin::Skin::default());
    }

    #[test]
    fn native_states_live_reload_and_compatibility_switch_keep_stable_keys() {
        let nodes = vec![
            StyleNode { key: 1, parent: None, class: "toolbar".into(), name: None, state: 0 },
            StyleNode { key: 2, parent: Some(0), class: "button".into(), name: Some("play".into()), state: 0 },
        ];
        prepare(
            ".toolbar { color:#112233; } .toolbar:hover { color:#445566; } #control-play { box-shadow:0 1px; }",
            800.0,
        );
        ENGINE.with(|engine| engine.borrow_mut().nodes = nodes.clone());
        assert_eq!(finish_tree(0.0), (true, false));
        ENGINE.with(|engine| assert_eq!(engine.borrow().values[&2].color, Color32::from_rgb(0x11, 0x22, 0x33)));
        assert_eq!(finish_tree(0.1), (false, false), "unchanged scopes must use the retained result");
        ENGINE.with(|engine| engine.borrow_mut().nodes[1].state = 1);
        assert_eq!(finish_tree(0.2), (true, false));
        ENGINE.with(|engine| {
            let engine = engine.borrow();
            assert_eq!(engine.values[&2].color, Color32::from_rgb(0x44, 0x55, 0x66));
            assert!(engine.document.is_none());
        });
        for (source, native) in [
            (".button { color:#abcdef; }", true),
            (".button { color:#abcdef; background-image:radial-gradient(red,blue); }", false),
            (".button { color:#abcdef; }", true),
        ] {
            prepare(source, 800.0);
            ENGINE.with(|engine| engine.borrow_mut().nodes = nodes.clone());
            assert_eq!(finish_tree(0.3), (true, false));
            ENGINE.with(|engine| {
                let engine = engine.borrow();
                assert_eq!(engine.values[&2].color, Color32::from_rgb(0xab, 0xcd, 0xef));
                assert_eq!(engine.native.is_some(), native);
                assert_eq!(engine.document.is_some(), !native);
                assert_eq!(engine.values.len(), 2);
            });
        }
        skin::configure(&skin::Skin::default());
    }
    #[test]
    fn workspace_example_collapses_closed_panels() {
        prepare(theme::WORKSPACE_GRID_EXAMPLE, 800.0);
        let open = workspace(Vec2::new(800.0, 600.0), Some(240.0), Some((false, 180.0))).unwrap();
        assert_eq!(open.browser.unwrap().width(), 240.0);
        assert_eq!(open.detail.unwrap().height(), 180.0);
        let closed = workspace(Vec2::new(800.0, 600.0), None, None).unwrap();
        assert!(closed.browser.is_none() && closed.detail.is_none());
        assert!(
            closed.main.width() > 780.0 && closed.main.height() > 580.0,
            "closed panels reserved their fixed dimensions: {:?}",
            closed.main
        );
        skin::configure(&skin::Skin::default());
    }
    #[test]
    fn gloss_surfaces_have_opaque_bases_and_sheet_overlays_preserve_native_fills() {
        prepare(theme::GLOSS_EXAMPLE, 1138.0);
        for class in ["session", "arrangement", "browser", "detail", "device-panel", "mixer", "button"] {
            let style = scoped(&[class.into()]).unwrap();
            assert!(style.authored.background, "{class} has no explicit base");
            assert_eq!(style.background.a(), 255, "{class} exposes the desktop");
            assert!(style.has_gradient(), "{class} lost its gradient");
        }
        for css in [theme::GLOSS_EXAMPLE, include_str!("../../themes/suhina-sheet.css")] {
            prepare(css, 1138.0);
            for class in ["track-header", "track-name"] {
                let style = scoped(&[class.into()]).unwrap();
                assert!(!style.authored.background, "{class} overlay replaces the native fill");
                assert!(style.has_gradient());
            }
        }
        prepare(include_str!("../../themes/suhina-meadow.css"), 1138.0);
        for class in ["window", "application-bar", "mixer", "track-header", "track-name"] {
            assert_eq!(scoped(&[class.into()]).unwrap().background.a(), 0, "Meadow's deliberate transparency changed");
        }
        skin::configure(&skin::Skin::default());
    }
    #[test]
    fn responsive_workspace_and_toolbar_survive_resize() {
        let css = include_str!("../../tests/fixtures/responsive-theme.css");
        prepare(css, 1138.0);
        let ctx = egui::Context::default();
        for size in
            [Vec2::new(1138.0, 1032.0), Vec2::new(1138.0, 1032.0), Vec2::new(800.0, 700.0), Vec2::new(1138.0, 1032.0)]
        {
            ctx.run_ui(
                egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, size)), ..Default::default() },
                |ui| {
                    let _frame = frame(ui);
                    let _window = super::super::style_class::enter("window");
                    {
                        let _toolbar = super::super::style_class::enter("toolbar");
                        for name in ["transport", "position", "performance", "mapping"] {
                            let _module = super::super::style_class::enter_named("toolbar-module", name);
                            let _button = super::super::style_class::enter("button");
                        }
                    }
                    let modules = toolbar(
                        &[
                            ("transport".into(), 100.0),
                            ("position".into(), 100.0),
                            ("performance".into(), 80.0),
                            ("mapping".into(), 60.0),
                        ],
                        Vec2::new(size.x, 56.0),
                    )
                    .unwrap();
                    assert_eq!(modules.iter().any(|module| module.name == "performance"), size.x > 1000.0);
                    assert_eq!(modules.iter().any(|module| module.name == "mapping"), size.x > 820.0);
                    let layout = workspace(Vec2::new(size.x, size.y - 100.0), None, Some((false, 200.0))).unwrap();
                    assert!(layout.browser.is_none());
                    assert_eq!(layout.detail.unwrap().height(), if size.y <= 760.0 { 150.0 } else { 200.0 });
                    let _workspace = super::super::style_class::enter_named("workspace", "workspace");
                    {
                        let _main = super::super::style_class::enter_named("workspace-panel", "panel-main");
                        let _view = super::super::style_class::enter("arrangement");
                    }
                    {
                        let _detail = super::super::style_class::enter_named("workspace-panel", "panel-detail");
                        let _editor = super::super::style_class::enter("clip-editor");
                    }
                },
            )
            .textures_delta
            .clear();
        }
        skin::configure(&skin::Skin::default());
    }
    #[test]
    fn palette_and_resize_restyle_retained_tree_and_preserve_author_colours() {
        let saved = theme::palette();
        prepare(
            ".button { background-color:currentColor; } .browser { color:#123456; } @media (max-width:600px) { .browser { color:#654321; } }",
            800.0,
        );
        let ctx = egui::Context::default();
        for (pass, width) in [(0, 800.0), (1, 800.0), (2, 500.0)] {
            let mut palette = saved;
            palette.text_primary = Color32::from_rgba_unmultiplied(24, 48, 72, 80 + pass * 40);
            theme::local_palette(palette);
            ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(width, 600.0))),
                    ..Default::default()
                },
                |ui| {
                    let _frame = frame(ui);
                    if pass > 0 {
                        ENGINE.with(|engine| {
                            let engine = engine.borrow();
                            assert!(engine.native.is_some(), "live edit discarded the native style engine");
                            assert!(!engine.values.is_empty(), "live edit forced per-control fallback documents");
                        });
                    }
                    let _window = super::super::style_class::enter("window");
                    {
                        let _button = super::super::style_class::enter("button");
                    }
                    {
                        let _browser = super::super::style_class::enter("browser");
                    }
                },
            )
            .textures_delta
            .clear();
            ENGINE.with(|engine| {
                let engine = engine.borrow();
                let button = engine.nodes.iter().find(|node| node.class == "button").unwrap();
                assert_eq!(engine.values[&button.key].background, palette.text_primary);
                let browser = engine.nodes.iter().find(|node| node.class == "browser").unwrap();
                let expected = if width > 600.0 {
                    Color32::from_rgb(0x12, 0x34, 0x56)
                } else {
                    Color32::from_rgb(0x65, 0x43, 0x21)
                };
                assert_eq!(engine.values[&browser.key].color, expected);
                assert!(!engine.styles_changed);
                assert!(engine.document.is_none(), "native palette samples created a Stylo document");
            });
        }
        theme::local_palette(saved);
        skin::configure(&skin::Skin::default());
    }
    #[test]
    #[ignore = "manual timing comparison for live palette edits"]
    fn measure_primary_colour_edits() {
        let saved = theme::palette();
        let css = include_str!("../../themes/suhina-sheet.css");
        prepare(css, 800.0);
        let ctx = egui::Context::default();
        let mut samples = Vec::new();
        for sample in 0..62 {
            let mut palette = saved;
            palette.text_primary = Color32::from_rgba_unmultiplied(40, 60, 50, 100 + sample);
            theme::local_palette(palette);
            let start = std::time::Instant::now();
            ctx.run_ui(egui::RawInput::default(), |ui| {
                let _frame = frame(ui);
                let _window = super::super::style_class::enter("window");
                for class in ["toolbar", "browser", "session", "mixer", "settings"] {
                    let _scope = super::super::style_class::enter(class);
                    for _ in 0..8 {
                        let _button = super::super::style_class::enter("button");
                    }
                }
            })
            .textures_delta
            .clear();
            if sample > 1 {
                samples.push(start.elapsed().as_secs_f64() * 1000.0);
            }
        }
        samples.sort_by(f64::total_cmp);
        eprintln!("primary colour edit: median {:.3} ms, max {:.3} ms", samples[30], samples[59]);
        theme::local_palette(saved);
        skin::configure(&skin::Skin::default());
    }
    #[test]
    #[ignore = "manual same-build comparison of staged native and legacy paint resolution"]
    fn measure_native_paint_migration() {
        let saved = theme::palette();
        let css = ".window { color:var(--text-primary); } .button { background-color:#123456; border-radius:3px; box-shadow:0 1px 2px #0008; } .toolbar .button { text-shadow:0 1px #0008; }";
        for native in [false, true] {
            ENGINE.with(|engine| *engine.borrow_mut() = Engine::default());
            prepare(css, 800.0);
            if !native {
                ENGINE.with(|engine| engine.borrow_mut().native = None);
            }
            let ctx = egui::Context::default();
            let mut samples = Vec::new();
            for sample in 0..62 {
                let mut palette = saved;
                palette.text_primary = Color32::from_rgba_unmultiplied(40, 60, 50, 100 + sample);
                theme::local_palette(palette);
                let start = std::time::Instant::now();
                ctx.run_ui(egui::RawInput::default(), |ui| {
                    let _frame = frame(ui);
                    let _window = super::super::style_class::enter("window");
                    for class in ["toolbar", "browser", "session", "mixer", "settings"] {
                        let _scope = super::super::style_class::enter(class);
                        for _ in 0..8 {
                            let _button = super::super::style_class::enter("button");
                        }
                    }
                })
                .textures_delta
                .clear();
                if sample > 1 {
                    samples.push(start.elapsed().as_secs_f64() * 1000.0);
                }
            }
            samples.sort_by(f64::total_cmp);
            eprintln!("native={native}: median {:.3} ms, max {:.3} ms", samples[30], samples[59]);
        }
        theme::local_palette(saved);
        skin::configure(&skin::Skin::default());
    }
    #[test]
    fn unreferenced_palette_edits_keep_computed_contexts_but_variables_invalidate() {
        let saved = theme::palette();
        prepare(".browser { color: var(--accent); background-color: white; }", 800.0);
        let stack = ["browser".into()];
        let original = scoped(&stack).unwrap();
        let mut palette = saved;
        palette.grid_major = Color32::from_rgba_unmultiplied(120, 120, 120, 90);
        theme::local_palette(palette);
        prepare(".browser { color: var(--accent); background-color: white; }", 800.0);
        ENGINE.with(|engine| assert_eq!(engine.borrow().contexts.len(), 1, "unrelated opacity cleared style cache"));
        assert_eq!(scoped(&stack).unwrap().color, original.color);
        palette.accent = Color32::from_rgb(10, 30, 70);
        theme::local_palette(palette);
        prepare(".browser { color: var(--accent); background-color: white; }", 800.0);
        assert_eq!(scoped(&stack).unwrap().color, palette.accent);
        theme::local_palette(saved);
        skin::configure(&skin::Skin::default());
    }
    fn prepare(css: &str, width: f32) {
        let ctx = egui::Context::default();
        let s = skin::Skin { css: Some(Arc::from(css)), ..Default::default() };
        skin::configure(&s);
        ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(width, 600.0))),
                ..Default::default()
            },
            |ui| begin_frame(ui),
        )
        .textures_delta
        .clear();
    }
    #[test]
    fn stylo_resolves_variables_layers_modern_colors_and_media() {
        prepare(
            "@layer base,custom; @layer base { .browser { color:red; }} @layer custom { :root { --ink:rgb(10 20 30); } .browser { color:var(--ink); font-size:calc(10px + 2px); }} @media (max-width:500px) { .browser { color:rgb(30 40 50); }}",
            800.0,
        );
        let values = scoped(&["window".into(), "browser".into()]).unwrap();
        assert_eq!(values.color, Color32::from_rgb(10, 20, 30));
        assert!((values.font_size - 12.0).abs() < 0.1);
        prepare("@media (max-width:500px) { .browser { color:rgb(30 40 50); }}", 400.0);
        assert_eq!(scoped(&["browser".into()]).unwrap().color, Color32::from_rgb(30, 40, 50));
        skin::configure(&skin::Skin::default());
    }
    #[test]
    fn taffy_grid_changes_real_module_rectangles_and_hidden_modules() {
        prepare(
            ".toolbar { display:grid; grid-template-columns:80px 120px; gap:10px; } #tempo { width:auto; } #transport { width:auto; } #mapping { display:none; }",
            800.0,
        );
        let modules = toolbar(
            &[("tempo".into(), 62.0), ("transport".into(), 86.0), ("mapping".into(), 64.0)],
            Vec2::new(400.0, 32.0),
        )
        .unwrap();
        assert_eq!(modules.len(), 2);
        assert!((modules[0].rect.width() - 80.0).abs() < 0.1);
        assert!((modules[1].rect.left() - 90.0).abs() < 0.1);
        skin::configure(&skin::Skin::default());
    }
    #[test]
    fn retained_scopes_match_siblings_and_keep_named_track_identity() {
        prepare(
            ".mixer[data-object=track-2] { color:rgb(10 20 30); } .mixer + .mixer { background-color:rgb(40 50 60); }",
            800.0,
        );
        let ctx = egui::Context::default();
        for pass in 0..4 {
            let order = if pass < 2 { [1, 2] } else { [2, 1] };
            ctx.run_ui(egui::RawInput::default(), |ui| {
                let _frame = frame(ui);
                let _window = super::super::style_class::enter("window");
                let _session = super::super::style_class::enter("session");
                for (index, track) in order.iter().enumerate() {
                    let _track = super::super::style_class::enter_named("mixer", &format!("track-{track}"));
                    if pass > 0 && *track == 2 {
                        assert_eq!(theme::text_primary(), Color32::from_rgb(10, 20, 30));
                    }
                    if (pass == 1 || pass == 3) && index == 1 {
                        assert_eq!(theme::panel_background(), Color32::from_rgb(40, 50, 60));
                    }
                }
            })
            .textures_delta
            .clear();
        }
        skin::configure(&skin::Skin::default());
    }
    #[test]
    fn disabled_state_uses_the_native_response_without_changing_its_id() {
        prepare(".button:disabled { background-color:rgb(50 60 70); }", 800.0);
        let ctx = egui::Context::default();
        let mut previous_id = None;
        for pass in 0..2 {
            ctx.run_ui(egui::RawInput::default(), |ui| {
                let _frame = frame(ui);
                let _window = super::super::style_class::enter("window");
                let response = ui.add_enabled(false, egui::Button::new("Disabled"));
                let _button = super::super::style_class::enter_response("button", &response);
                if let Some(id) = previous_id {
                    assert_eq!(response.id, id);
                }
                previous_id = Some(response.id);
                assert!(!response.enabled());
                if pass == 1 {
                    assert_eq!(theme::control_background(), Color32::from_rgb(50, 60, 70));
                }
            })
            .textures_delta
            .clear();
        }
        skin::configure(&skin::Skin::default());
    }
    #[test]
    fn computed_linear_gradients_publish_their_real_rgba_pixels_and_reuse_textures() {
        prepare(".browser { background-image:linear-gradient(90deg in srgb, rgb(255 0 0 / 0), blue); }", 800.0);
        let style = scoped(&["browser".into()]).unwrap();
        ENGINE.with(|engine| assert!(engine.borrow().native.is_some()));
        assert_eq!(style.gradients.len(), 1);
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let rect = Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(200.0, 40.0));
            assert!(background_with(ui.painter(), rect, &style));
            assert!(background_with(ui.painter(), rect, &style));
        });
        let ramps: Vec<_> = output
            .textures_delta
            .set
            .values()
            .flat_map(|deltas| deltas.iter())
            .filter(|delta| delta.image.size() == [1024, 1])
            .collect();
        assert_eq!(ramps.len(), 1);
        let egui::ImageData::Color(image) = &ramps[0].image;
        let midpoint = image.pixels[512].to_srgba_unmultiplied();
        assert_eq!(midpoint[0], 0, "CSS alpha interpolation is premultiplied");
        assert!(midpoint[2] > 250);
        assert!((127..=129).contains(&midpoint[3]));
        assert_eq!(image.pixels[1023], Color32::BLUE);
        output.textures_delta.clear();
        skin::configure(&skin::Skin::default());
    }
    #[test]
    fn radial_and_conic_gradients_publish_spatial_pixels_at_the_authored_origin() {
        for (css, red, blue) in [
            (
                ".browser { background-image:radial-gradient(circle 20px at 25% 75%, red 0 99%, blue 100%); }",
                (64, 191),
                (255, 0),
            ),
            (
                ".browser { background-image:conic-gradient(from 90deg at 50% 50%, red 0deg 90deg, blue 90deg 360deg); }",
                (224, 160),
                (128, 0),
            ),
            (
                ".browser { background-image:repeating-radial-gradient(circle 20px at 50% 50%, red 0 50%, blue 50% 100%); }",
                (128, 128),
                (147, 128),
            ),
        ] {
            prepare(css, 800.0);
            let style = scoped(&["browser".into()]).unwrap();
            assert_eq!(style.gradients.len(), 1);
            let ctx = egui::Context::default();
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                let rect = Rect::from_min_size(egui::Pos2::ZERO, Vec2::splat(256.0));
                assert!(background_with(ui.painter(), rect, &style));
                assert!(background_with(ui.painter(), rect, &style));
            });
            let images: Vec<_> = output
                .textures_delta
                .set
                .values()
                .flat_map(|d| d.iter())
                .filter(|d| d.image.size() == [256, 256])
                .collect();
            assert_eq!(images.len(), 1, "spatial textures are cached");
            let egui::ImageData::Color(image) = &images[0].image;
            assert_eq!(image.pixels[red.1 * 256 + red.0], Color32::RED, "{css}");
            assert_eq!(image.pixels[blue.1 * 256 + blue.0], Color32::BLUE, "{css}");
            output.textures_delta.clear();
        }
        skin::configure(&skin::Skin::default());
    }
    #[test]
    fn retained_document_advances_keyframes_and_motion_setting_suppresses_them() {
        let css = ".browser { color:red; animation:ink 1s linear forwards; } @keyframes ink { from {color:red;} to {color:blue;} }";
        for motion in [true, false] {
            prepare(css, 800.0);
            let ctx = egui::Context::default();
            let mut colors = Vec::new();
            for time in [0.0, 0.5, 1.1] {
                ctx.run_ui(
                    egui::RawInput {
                        time: Some(time),
                        screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(800.0, 600.0))),
                        ..Default::default()
                    },
                    |ui| {
                        let _frame = frame_with_motion(ui, motion);
                        let _browser = super::super::style_class::enter("browser");
                    },
                )
                .textures_delta
                .clear();
                colors.push(ENGINE.with(|engine| {
                    let engine = engine.borrow();
                    let key = engine.nodes.iter().find(|n| n.class == "browser").unwrap().key;
                    engine.values[&key].color
                }));
            }
            if motion {
                assert!(colors[1].r() > 20 && colors[1].b() > 20, "{colors:?}");
                assert_eq!(colors[2], Color32::BLUE);
            } else {
                assert_eq!(colors, [Color32::RED; 3]);
                assert!(!ENGINE.with(|engine| engine.borrow().document.as_ref().unwrap().is_animating()));
            }
        }
        skin::configure(&skin::Skin::default());
    }
    #[test]
    fn native_state_change_starts_a_transition_instead_of_restarting_the_document() {
        prepare(
            ".button { background-color:red; transition:background-color 1s linear; } .button:disabled { background-color:blue; }",
            800.0,
        );
        let ctx = egui::Context::default();
        let mut colors = Vec::new();
        for (index, time) in [0.0, 0.1, 0.6, 1.2].into_iter().enumerate() {
            ctx.run_ui(
                egui::RawInput {
                    time: Some(time),
                    screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(800.0, 600.0))),
                    ..Default::default()
                },
                |ui| {
                    let _frame = frame(ui);
                    let response = ui.add_enabled(index == 0, egui::Button::new("Native state"));
                    let _scope = super::super::style_class::enter_response("button", &response);
                },
            )
            .textures_delta
            .clear();
            colors.push(ENGINE.with(|engine| {
                let engine = engine.borrow();
                let key = engine.nodes.iter().find(|n| n.class == "button").unwrap().key;
                engine.values[&key].background
            }));
        }
        assert_eq!(colors[0], Color32::RED);
        assert!(colors[2].r() > 20 && colors[2].b() > 20, "{colors:?}");
        assert_eq!(colors[3], Color32::BLUE);
        skin::configure(&skin::Skin::default());
    }
    #[test]
    fn computed_inset_shadow_uses_current_color_and_reuses_its_texture() {
        prepare(".browser { color:rgb(120 30 180); box-shadow:inset 8px 0 0 0 currentColor; }", 800.0);
        let style = scoped(&["browser".into()]).unwrap();
        assert_eq!(style.shadows.len(), 1);
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let rect = Rect::from_min_size(egui::Pos2::ZERO, Vec2::splat(40.0));
            assert!(background_with(ui.painter(), rect, &style));
            assert!(background_with(ui.painter(), rect, &style));
        });
        let images: Vec<_> = output
            .textures_delta
            .set
            .iter()
            .filter(|(id, _)| **id != egui::TextureId::default())
            .flat_map(|(_, deltas)| deltas.iter())
            .map(|d| d.image.clone())
            .collect();
        output.textures_delta.clear();
        assert_eq!(images.len(), 1);
        let egui::ImageData::Color(image) = &images[0];
        assert_eq!(
            image.pixels[(image.size[1] / 2) * image.size[0] + image.size[0] / 16],
            Color32::from_rgb(120, 30, 180)
        );
        assert_eq!(image.pixels[(image.size[1] / 2) * image.size[0] + image.size[0] / 2].a(), 0);
        output.textures_delta.clear();
        skin::configure(&skin::Skin::default());
    }
    #[test]
    fn keyframe_only_properties_are_applied_without_a_static_declaration() {
        prepare(
            ".browser { animation:ink 1s linear forwards; } @keyframes ink { from {color:red;} to {color:blue;} }",
            800.0,
        );
        let ctx = egui::Context::default();
        for time in [0.0, 1.1] {
            ctx.run_ui(
                egui::RawInput {
                    time: Some(time),
                    screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(800.0, 600.0))),
                    ..Default::default()
                },
                |ui| {
                    let _frame = frame(ui);
                    let _scope = super::super::style_class::enter("browser");
                },
            )
            .textures_delta
            .clear();
        }
        ENGINE.with(|engine| {
            let engine = engine.borrow();
            let key = engine.nodes.iter().find(|n| n.class == "browser").unwrap().key;
            assert!(engine.values[&key].authored.color);
            assert_eq!(engine.values[&key].color, Color32::BLUE);
        });
        skin::configure(&skin::Skin::default());
    }
    #[test]
    fn workspace_grid_resolves_real_panels_and_respects_native_visibility() {
        prepare(
            "#workspace { display:grid; grid-template-columns:minmax(0,1fr) 240px; grid-template-rows:minmax(0,1fr) 160px; gap:8px; } #panel-browser {grid-column:2;grid-row:1 / -1;} #panel-main {grid-column:1;grid-row:1;} #panel-detail {grid-column:1;grid-row:2;}",
            800.0,
        );
        let layout = workspace(Vec2::new(800.0, 600.0), Some(200.0), Some((false, 120.0))).unwrap();
        assert_eq!(layout.main, Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(552.0, 432.0)));
        assert_eq!(layout.browser.unwrap().left(), 560.0);
        assert_eq!(layout.detail.unwrap().top(), 440.0);
        let closed = workspace(Vec2::new(800.0, 600.0), None, None).unwrap();
        assert!(closed.browser.is_none() && closed.detail.is_none());
        prepare(".browser {color:red;}", 800.0);
        assert!(workspace(Vec2::new(800.0, 600.0), Some(200.0), Some((false, 120.0))).is_none());
        skin::configure(&skin::Skin::default());
    }
    #[test]
    fn unrelated_css_preserves_native_colours_font_sizes_and_corner_defaults() {
        prepare(".browser { color:rgb(10 20 30); }", 800.0);
        let original = theme::palette();
        {
            let _mixer = super::super::style_class::enter("mixer");
            assert_eq!(theme::palette(), original);
            assert_eq!(super::super::style_class::font_size(9.0), 9.0);
            assert_eq!(super::super::style_class::radius(4.0), 4.0);
        }
        {
            let _browser = super::super::style_class::enter("browser");
            assert_eq!(theme::text_primary(), Color32::from_rgb(10, 20, 30));
        }
        skin::configure(&skin::Skin::default());
    }
}
